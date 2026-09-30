//! Rust parity driver. Report format: tools/DRIVER_FORMAT.md

use cjson_core::{
    compare, estimate_compare_cost, last_error_offset, minify, parse, print_formatted,
    print_unformatted,
};
use std::env;
use std::fs;
use std::process::ExitCode;

#[derive(Default)]
struct Sections {
    parse: bool,
    print: bool,
    tree: bool,
}

fn parse_sections(arg: &str) -> Option<Sections> {
    let mut s = Sections::default();
    for part in arg.split(',') {
        match part {
            "parse" => s.parse = true,
            "print" => s.print = true,
            "tree" => s.tree = true,
            _ => return None,
        }
    }
    if s.parse || s.print || s.tree {
        Some(s)
    } else {
        None
    }
}

fn main() -> ExitCode {
    let mut sections = Sections {
        parse: true,
        print: true,
        tree: true,
    };
    let mut path: Option<String> = None;
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--sections" {
            let Some(list) = args.next() else {
                eprintln!("Usage: cjson-driver [--sections parse,print,tree] <file>");
                return ExitCode::from(2);
            };
            let Some(s) = parse_sections(&list) else {
                eprintln!("Usage: cjson-driver [--sections parse,print,tree] <file>");
                return ExitCode::from(2);
            };
            sections = s;
        } else if path.is_none() {
            path = Some(a);
        } else {
            eprintln!("Usage: cjson-driver [--sections parse,print,tree] <file>");
            return ExitCode::from(2);
        }
    }
    let Some(path) = path else {
        eprintln!("Usage: cjson-driver [--sections parse,print,tree] <file>");
        return ExitCode::from(2);
    };

    let input = match fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("failed to read {path}: {e}");
            return ExitCode::from(2);
        }
    };

    let parsed = parse(&input);

    if sections.parse {
        println!("=== parse ===");
        match &parsed {
            Ok(_) => {
                println!("status: ok");
                println!("error_offset: -");
            }
            Err(_) => {
                let off = last_error_offset().unwrap_or(0);
                println!("status: fail");
                println!("error_offset: {off}");
            }
        }
    }

    if sections.print {
        println!("=== print ===");
        println!("compact:");
        if let Ok(root) = &parsed {
            if let Ok(c) = print_unformatted(root) {
                print!("{c}");
            }
        }
        println!("\nend_compact");
        println!("pretty:");
        if let Ok(root) = &parsed {
            if let Ok(p) = print_formatted(root) {
                print!("{p}");
            }
        }
        println!("\nend_pretty");
    }

    if sections.tree {
        println!("=== tree ===");
        match &parsed {
            Ok(root) => {
                println!("array_size: {}", root.array_size());
                if estimate_compare_cost(root) > 1_000_000 {
                    println!("compare_self: skipped");
                } else if compare(root, root, true) {
                    println!("compare_self: same");
                } else {
                    println!("compare_self: diff");
                }
            }
            Err(_) => {
                println!("array_size: -");
                println!("compare_self: skipped");
            }
        }
        println!("minify:");
        let mut minbuf = String::from_utf8_lossy(&input).into_owned();
        // Preserve exact bytes when valid UTF-8; for arbitrary bytes use lossy like C's char*.
        if let Ok(s) = std::str::from_utf8(&input) {
            minbuf = s.to_owned();
        }
        minify(&mut minbuf);
        print!("{minbuf}");
        println!("\nend_minify");
    }

    ExitCode::SUCCESS
}
