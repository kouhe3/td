//! tdutils SliceHash on the Rust side.
//!
//! ```text
//! slice_hash_probe <script.txt>
//! ```
//!
//! Script lines: `new <name> <hex|->`, `hash <name>`, `mhash <name>` (same
//! bytes through a mutable view).
//!
//! `result: u32 = 0; for c in bytes { result = result * 123456789 + (c as i8 as i32 as u32) }`.
//! The accumulator wraps modulo 2^32 every step, and the byte is a **signed**
//! char, so 0x80 contributes 0xFFFFFF80. The empty slice hashes to 0.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

const MUL: u32 = 123_456_789;

fn slice_hash(bytes: &[u8]) -> u32 {
    let mut result: u32 = 0;
    for &byte in bytes {
        result = result.wrapping_mul(MUL).wrapping_add(byte as i8 as i32 as u32);
    }
    result
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
        eprintln!("usage: slice_hash_probe <script.txt>");
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
            "hash" | "mhash" => {
                let bytes = &payloads[name];
                println!("{} {} size={} h={}", op, name, bytes.len(), slice_hash(bytes));
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}