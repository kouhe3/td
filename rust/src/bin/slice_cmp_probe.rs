//! tdutils Slice::find / rfind and operator== / operator!= / operator< on the
//! Rust side.
//!
//! ```text
//! slice_cmp_probe <script.txt>
//! ```
//!
//! Script lines: `new <name> <hex|->`, `find <name> <byte>`,
//! `rfind <name> <byte>`, `cmp <a> <b>`.
//!
//! `pos` is printed as usize, so "not found" is 18446744073709551615 (npos).
//! `less` follows memcmp: the common prefix compares as unsigned bytes, then
//! the shorter slice is smaller. `equals` needs equal lengths, so any two empty
//! slices are equal.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

const NPOS: usize = usize::MAX;

struct Payload {
    bytes: Vec<u8>,
}

impl Payload {
    fn find(&self, byte: u8) -> usize {
        for (pos, &current) in self.bytes.iter().enumerate() {
            if current == byte {
                return pos;
            }
        }
        NPOS
    }
    fn rfind(&self, byte: u8) -> usize {
        for pos in (0..self.bytes.len()).rev() {
            if self.bytes[pos] == byte {
                return pos;
            }
        }
        NPOS
    }
    fn equals(&self, other: &Self) -> bool {
        self.bytes.len() == other.bytes.len() && self.bytes == other.bytes
    }
    fn less(&self, other: &Self) -> bool {
        self.bytes < other.bytes
    }
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
        eprintln!("usage: slice_cmp_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    let mut payloads: HashMap<String, Payload> = HashMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let op = parts[0];
        let name = parts.get(1).copied().unwrap_or("");
        match op {
            "new" => {
                payloads.insert(name.to_string(), Payload { bytes: unhex(parts.get(2).copied().unwrap_or("-")) });
            }
            "find" | "rfind" => {
                let byte: u8 = parts.get(2).map_or(0, |text| text.parse().unwrap());
                let payload = &payloads[name];
                let pos = if op == "find" { payload.find(byte) } else { payload.rfind(byte) };
                println!("{} {} c={} pos={}", op, name, byte, pos);
            }
            "cmp" => {
                let other = parts.get(2).copied().unwrap_or("");
                let left = &payloads[name];
                let right = &payloads[other];
                println!(
                    "cmp {} {} eq={} ne={} lt={} gt={}",
                    name,
                    other,
                    left.equals(right) as i32,
                    (!left.equals(right)) as i32,
                    left.less(right) as i32,
                    right.less(left) as i32
                );
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}