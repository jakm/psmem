use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;

use clap::Parser;
use itertools::Itertools;
use lazy_static::lazy_static;
use rayon::prelude::*;
use regex::Regex;

// TODO features:
//  - take in account systemd cgroups for a more interesting tree view? (https://stackoverflow.com/questions/43892327/process-grouping-in-linux)
// TODO add cli arg to distinguish column to sort by; maybe add filtering by program name

/// A Rust program that analyzes memory usage of running processes by parsing /proc filesystem
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Show PIDs of ungrouped processes (those that have no other process with the same executable)
    #[arg(short, long, default_value_t = false)]
    pid: bool,
}

#[derive(Clone, Debug)]
struct ProgramInfo {
    name: String,
    pids: Vec<i32>,
    mem_info: MemInfo,
}

#[derive(Copy, Clone, Debug, Default)]
struct MemInfo {
    private: u64,
    shared: u64,
}

impl MemInfo {
    /// Checks if both private and shared memory are zero
    fn empty(&self) -> bool {
        self.private == 0 && self.shared == 0
    }
}

/// Main function that collects and displays memory usage statistics for all running processes
fn main() {
    let args = Args::parse();

    let mut stats: HashMap<String, ProgramInfo> = HashMap::new();
    let (sender, receiver) = channel();

    procfs::process::all_processes()
        .unwrap()
        .par_bridge()
        .filter_map(|p| p.ok())
        .for_each_with(sender, |s, p| {
            let exe = match search_process_executable(&p) {
                Some(exe) => exe,
                None => return,
            };

            // collect memory info from /proc/pid/smaps_rollup if available, otherwise fall back to /proc/pid/smaps
            let path = format!("/proc/{}/smaps_rollup", p.pid);
            let rollup_path = Path::new(OsStr::new(&path));
            let info = count_mem_info_rollup(rollup_path).unwrap_or_else(|_| count_mem_info(&p));

            let msg = (exe, p.pid, info);

            s.send(msg).unwrap();
        });

    for msg in receiver {
        let (exe, pid, info) = msg;

        if info.empty() {
            continue;
        }

        let name: String = match exe.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => "<unknown>".into(),
        };

        stats
            .entry(name.clone())
            .and_modify(|e| {
                e.pids.push(pid);
                e.mem_info.private += info.private;
                e.mem_info.shared += info.shared;
            })
            .or_insert_with(|| ProgramInfo {
                name,
                pids: vec![pid],
                mem_info: info,
            });
    }

    if stats.len() == 0 {
        println!(r#"No processes ¯\_(ツ)_/¯"#);
        return;
    }

    let pid_header = if args.pid {
        format!("{:>9}  ", "PID")
    } else {
        String::new()
    };
    let header = format!(
        "{:>10}   +   {:>10}  =   Memory used\t{}Program",
        "Private", "Shared (PSS)", pid_header
    );

    println!("{}\n", header);

    let ordered_stats = stats
        .values()
        .into_iter()
        .sorted_by(|a, b| Ord::cmp(&a.mem_info.private, &b.mem_info.private));

    let mut total = MemInfo::default();

    for p in ordered_stats {
        let name = if p.pids.len() > 1 {
            format!("{} ({})", p.name, p.pids.len())
        } else {
            p.name.clone()
        };
        let title = if !args.pid {
            name
        } else if p.pids.len() > 1 {
            format!("{}{}", " ".repeat(pid_header.len()), name)
        } else {
            format!("{:>9}  {}", p.pids[0], name)
        };
        print_mem_info(&title, &p.mem_info);

        total.private += p.mem_info.private;
        total.shared += p.mem_info.shared;
    }

    println!("{}", "-".repeat(43));
    print_mem_info("", &total);
    println!("{}", "=".repeat(43));
}

/// Resolves the executable path for a process, following symlinks to get the actual binary
fn search_process_executable(p: &procfs::process::Process) -> Option<PathBuf> {
    if p.pid <= 1 {
        return None;
    }

    let mut exe = match p.exe() {
        Ok(exe) => exe.to_path_buf(),
        Err(e) => {
            match e {
                procfs::ProcError::PermissionDenied(_) => {
                    // we don't have permission to read the executable, skip logging
                }
                procfs::ProcError::NotFound(_) => {
                    // process has no executable, skip logging
                }
                _ => {
                    eprintln!("error getting executable of process {}: {:?}", p.pid, e);
                }
            }

            return None;
        }
    };

    while exe.is_symlink() {
        // if the executable is a symlink, dereference it and update `exe`
        match std::fs::read_link(&exe) {
            Ok(link) => exe = link,
            Err(e) => {
                eprintln!(
                    "error reading symlink to executable of process {}: {:?}",
                    p.pid, e
                );
                return None;
            }
        }
    }

    Some(exe)
}

/// Formats and prints memory usage information in a human-readable table format
fn print_mem_info(title: &str, mem_info: &MemInfo) {
    let (private, private_unit) = humanize_bytes(mem_info.private);
    let (shared, shared_unit) = humanize_bytes(mem_info.shared);
    let (total, total_unit) = humanize_bytes(mem_info.private + mem_info.shared);

    println!(
        "{:6.1} {}   +   {:8.1} {}  =  {:8.1} {}\t{}",
        private, private_unit, shared, shared_unit, total, total_unit, title,
    );
}

const KIBIBYTE: u64 = 1024;
const MEBIBYTE: u64 = 1024 * 1024;
const GIBIBYTE: u64 = 1024 * 1024 * 1024;

const UNIT_BYTE: &str = "  B";
const UNIT_KIBIBYTE: &str = "KiB";
const UNIT_MEBIBYTE: &str = "MiB";
const UNIT_GIBIBYTE: &str = "GiB";

/// Converts bytes to human-readable format with appropriate unit (B, KiB, MiB, GiB)
fn humanize_bytes(size: u64) -> (f64, &'static str) {
    if size > GIBIBYTE {
        return (size as f64 / GIBIBYTE as f64, UNIT_GIBIBYTE);
    }
    if size > MEBIBYTE {
        return (size as f64 / MEBIBYTE as f64, UNIT_MEBIBYTE);
    }
    if size > KIBIBYTE {
        return (size as f64 / KIBIBYTE as f64, UNIT_KIBIBYTE);
    }

    (size as f64, UNIT_BYTE)
}

/// Parses memory information from /proc/pid/smaps_rollup file using regex patterns
fn count_mem_info_rollup(p: &Path) -> Result<MemInfo, Box<dyn std::error::Error>> {
    lazy_static! {
        static ref PRIVATE_RE: Regex =
            Regex::new(r"(?m)^Private_(Clean|Dirty|Hugetlb):\s+(?P<size>\d+) kB$").unwrap();
        static ref PSS_RE: Regex = Regex::new(r"(?m)^Pss:\s+(?P<size>\d+) kB$").unwrap();
    }

    let mut f = File::open(p)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;

    let private = sum_values_regex(&buf, &PRIVATE_RE);
    let pss = sum_values_regex(&buf, &PSS_RE);
    let shared = pss - private;

    Ok(MemInfo { private, shared })
}

/// Sums all numeric values captured by a regex pattern from a string
fn sum_values_regex(s: &String, re: &Regex) -> u64 {
    let mut sum = 0;

    let captures = re.captures_iter(&s);
    for cap in captures {
        if let Some(cap) = cap.name("size") {
            if let Ok(size) = cap.as_str().parse::<u64>() {
                sum += size * 1024
            }
        }
    }

    sum
}

/// Calculates memory information by parsing /proc/pid/smaps entries for a process
fn count_mem_info(p: &procfs::process::Process) -> MemInfo {
    lazy_static! {
        static ref PRIVATE_FIELDS: Vec<String> = vec![
            "Private_Clean".to_string(),
            "Private_Dirty".to_string(),
            "Private_Hugetlb".to_string()
        ];
    }

    let mut info = MemInfo::default();
    if let Ok(maps) = p.smaps() {
        maps.iter()
            .map(|mm| {
                let private = sum_values(&mm.extension.map, &PRIVATE_FIELDS);
                let pss = mm.extension.map.get("Pss").unwrap_or(&0).clone();
                let shared = pss - private;
                (private, shared)
            })
            .fold(&mut info, |info, items| {
                info.private += items.0;
                info.shared += items.1;
                info
            });
    }

    info
}

/// Sums values from a HashMap for the specified keys
fn sum_values(map: &HashMap<String, u64>, keys: &[String]) -> u64 {
    keys.iter().filter_map(|k| map.get(k)).sum()
}
