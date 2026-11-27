//! tdutils begins_with / ends_with (free functions in misc.h) on the Rust side.
//!
//! ```text
//! slice_prefix_probe <script.txt>
//! ```
//!
//! Script lines: `new <name> <hex|->`, `begins <str> <affix>`,
//! `ends <str> <affix>`.
//!
//! An affix longer than the string is rejected by the length guard, the empty
//! affix always matches, and the comparison is byte for byte. `ends_with` uses
//! the tail window, so an empty suffix is the empty tail.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

fn begins_with(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len() && haystack[..needle.len()] == *needle
}

fn ends_with(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len() && haystack[haystack.len() - needle.len()..] == *needle
}

fn unhex(text: &str) -> Vec<u8> {
    if text == "-" {
        return Vec::new();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let digits = std::str::from_utf8(pair).unwrap();
        out.push(u8::from_str_radix(digits, 16).unwrap());
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: slice_prefix_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    let mut payloads: HashMap<String, Vec<u8>> = HashMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let op = parts[0];
        let name = parts.get(1).copied().unwrap_or("");
        match op {
            "new" => {
                payloads.insert(name.to_string(), unhex(parts.get(2).copied().unwrap_or("-")));
            }
            "begins" | "ends" => {
                let affix_name = parts.get(2).copied().unwrap_or("");
                let haystack = &payloads[name];
                let needle = &payloads[affix_name];
                let result = if op == "begins" { begins_with(haystack, needle) } else { ends_with(haystack, needle) };
                println!("{} {} {} result={}", op, name, affix_name, result as i32);
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}