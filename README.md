# psmem

**psmem** is a Rust command-line tool that provides memory usage statistics by analyzing running processes on Linux systems. It aggregates memory information by program name and displays it in a human-readable format.

## Key Features

- Groups processes by executable name and aggregates their memory usage
- Shows private and shared memory (PSS - Proportional Set Size) for each program
- Uses parallel processing via Rayon for efficient data collection
- Reads from `/proc/*/smaps_rollup` (faster) or falls back to `/proc/*/smaps`
- Displays results sorted by private memory usage with totals

## Output Format

```
    Private + Shared (PSS)   = Memory used    Program
...
  63.4 MiB  +      7.8 MiB   =     71.2 MiB	  python3.10 (6)
  95.8 MiB  +      5.3 MiB   =    101.1 MiB	  nvim (4)
 103.4 MiB  +      2.0 MiB   =    105.3 MiB	  mono-sgen (2)
 257.4 MiB  +     12.4 MiB   =    269.8 MiB	  thunderbird (2)
 932.6 MiB  +     68.0 MiB   =   1000.6 MiB	  franz (12)
   1.0 GiB  +     37.1 MiB   =      1.1 GiB	  node (6)
   1.1 GiB  +    347.0 KiB   =      1.1 GiB	  rust-analyzer
   1.8 GiB  +     23.8 MiB   =      1.8 GiB	  gopls (8)
   3.5 GiB  +    139.7 MiB   =      3.6 GiB	  firefox-bin (31)
-------------------------------------------
   9.1 GiB  +    314.5 MiB   =      9.4 GiB	
===========================================
```

The tool is designed for system administrators and developers who want to quickly identify which programs are consuming the most memory on their Linux systems.

## Usage

```bash
cargo run
```

## Building

```bash
cargo build --release
```
