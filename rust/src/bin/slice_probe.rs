//! tdutils Slice's view operations on the Rust side: remove_prefix /
//! remove_suffix / substr / truncate.
//!
//! ```text
//! slice_probe <script.txt>
//! slice_probe --check <op> <len> <a> [b]
//! ```
//!
//! Script lines: `new <name> <hex|->` (shared bytes), `prefix <name> <n>`,
//! `suffix <name> <n>`, `trunc <name> <n>`, `substr <name> <from>`,
//! `substr2 <name> <from> <n>`, `print <name>`.
//!
//! The view keeps a byte offset into the shared buffer, so `off=` exposes what
//! moved: remove_prefix and substr advance it, truncate and remove_suffix do
//! not. Out-of-range prefix/suffix/substr abort, matching td's CHECK; those
//! cases are driven one per process through --check.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;
use std::rc::Rc;

struct Slice {
    data: Rc<Vec<u8>>,
    begin: usize,
    end: usize,
}

impl Slice {
    fn new(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        Self { data: Rc::new(bytes), begin: 0, end: len }
    }
    fn size(&self) -> usize {
        self.end - self.begin
    }
    fn bytes(&self) -> &[u8] {
        &self.data[self.begin..self.end]
    }
    fn remove_prefix(&mut self, count: usize) {
        require(count <= self.size());
        self.begin += count;
    }
    fn remove_suffix(&mut self, count: usize) {
        require(count <= self.size());
        self.end -= count;
    }
    fn truncate(&mut self, size: usize) {
        if self.size() > size {
            self.end = self.begin + size;
        }
    }
    fn substr(&self, from: usize) -> Self {
        require(from <= self.size());
        Self { data: Rc::clone(&self.data), begin: self.begin + from, end: self.end }
    }
    fn substr2(&self, from: usize, size: usize) -> Self {
        require(from <= self.size());
        Self {
            data: Rc::clone(&self.data),
            begin: self.begin + from,
            end: self.begin + from + size.min(self.size() - from),
        }
    }
}

/// td's CHECK aborts the process; the comparator only asks "did it return".
fn require(condition: bool) {
    if !condition {
        std::process::abort();
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

fn hex_of(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn dump(op: &str, name: &str, args: &str, slice: &Slice) {
    println!(
        "{} {} {} size={} off={} hex={}",
        op,
        name,
        args,
        slice.size(),
        slice.begin,
        hex_of(slice.bytes())
    );
}

fn number(parts: &[&str], index: usize) -> usize {
    parts.get(index).map_or(0, |text| text.parse().unwrap_or(0))
}

fn run_check(args: &[String]) -> ExitCode {
    // --check <op> <len> <a> [b]
    if args.len() < 5 {
        eprintln!("usage: slice_probe --check <op> <len> <a> [b]");
        return ExitCode::from(2);
    }
    let op = args[2].as_str();
    let len: usize = args[3].parse().unwrap();
    let a: usize = args[4].parse().unwrap();
    let b: usize = args.get(5).map_or(0, |text| text.parse().unwrap());
    let storage = b"abcdefgh".to_vec();
    if len > storage.len() {
        eprintln!("len must be <= 8");
        return ExitCode::from(2);
    }
    let mut view = Slice::new(storage);
    view.end = len;
    match op {
        "prefix" => view.remove_prefix(a),
        "suffix" => view.remove_suffix(a),
        "trunc" => view.truncate(a),
        "substr" => view = view.substr(a),
        "substr2" => view = view.substr2(a, b),
        other => {
            eprintln!("unknown op {other}");
            return ExitCode::from(2);
        }
    }
    println!("check op={} len={} a={} size={} off={} hex={}", op, len, a, view.size(), view.begin, hex_of(view.bytes()));
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() >= 2 && args[1] == "--check" {
        return run_check(&args);
    }
    if args.len() != 2 {
        eprintln!("usage: slice_probe <script.txt> | --check <op> <len> <a> [b]");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    let mut holders: HashMap<String, Slice> = HashMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let op = parts[0];
        let name = parts.get(1).copied().unwrap_or("");
        match op {
            "new" => {
                holders.insert(name.to_string(), Slice::new(unhex(parts.get(2).copied().unwrap_or("-"))));
            }
            "print" => dump(op, name, "", &holders[name]),
            "prefix" | "suffix" | "trunc" | "substr" | "substr2" => {
                let a = number(&parts, 2);
                let mut slice = holders.remove(name).expect("unknown slice name");
                match op {
                    "prefix" => {
                        slice.remove_prefix(a);
                        dump(op, name, &format!("n={a}"), &slice);
                    }
                    "suffix" => {
                        slice.remove_suffix(a);
                        dump(op, name, &format!("n={a}"), &slice);
                    }
                    "trunc" => {
                        slice.truncate(a);
                        dump(op, name, &format!("n={a}"), &slice);
                    }
                    "substr" => {
                        slice = slice.substr(a);
                        dump(op, name, &format!("from={a}"), &slice);
                    }
                    _ => {
                        let b = number(&parts, 3);
                        slice = slice.substr2(a, b);
                        dump(op, name, &format!("from={a} n={b}"), &slice);
                    }
                }
                holders.insert(name.to_string(), slice);
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}