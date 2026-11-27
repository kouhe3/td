//! tdutils BufferSlice ownership semantics on the Rust side.
//!
//! ```text
//! buffer_slice_probe <script.txt>
//! ```
//!
//! `clone()` shares the allocation and only copies the [begin, end) window;
//! `copy()` allocates a fresh buffer with the window's bytes. `as_mutable_slice()`
//! does not copy on write, so mutating a clone is visible through every slice
//! whose window covers that byte.

use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;
use std::rc::Rc;

#[derive(Clone)]
struct Slice {
    data: Rc<RefCell<Vec<u8>>>,
    begin: usize,
    end: usize,
}

impl Slice {
    fn new(bytes: &[u8]) -> Self {
        Self { data: Rc::new(RefCell::new(bytes.to_vec())), begin: 0, end: bytes.len() }
    }
    fn size(&self) -> usize {
        self.end - self.begin
    }
    fn as_slice(&self) -> Vec<u8> {
        self.data.borrow()[self.begin..self.end].to_vec()
    }
    fn clone_slice(&self) -> Self {
        Self { data: Rc::clone(&self.data), begin: self.begin, end: self.end }
    }
    fn copy_slice(&self) -> Self {
        let content = self.as_slice();
        Self::new(&content)
    }
    fn mutate(&mut self, index: usize, value: u8) {
        self.data.borrow_mut()[self.begin + index] = value;
    }
    fn confirm_read(&mut self, count: usize) -> bool {
        self.begin += count;
        assert!(self.begin <= self.end, "confirm_read past the end");
        self.begin == self.end
    }
    fn truncate(&mut self, limit: usize) {
        if self.size() > limit {
            self.end = self.begin + limit;
        }
    }
    fn after(&self, offset: usize) -> Vec<u8> {
        self.data.borrow()[self.begin + offset..self.end].to_vec()
    }
}

fn unhex(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index + 1 < bytes.len() {
        let high = (bytes[index] as char).to_digit(16).unwrap_or(0) as u8;
        let low = (bytes[index + 1] as char).to_digit(16).unwrap_or(0) as u8;
        out.push((high << 4) | low);
        index += 2;
    }
    out
}

fn hex_of(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: buffer_slice_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read {}: {error}", args[1]);
            return ExitCode::from(1);
        }
    };

    let mut slices: HashMap<String, Slice> = HashMap::new();
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        match tokens[0] {
            "new" => {
                let name = tokens[1];
                let bytes = unhex(tokens.get(2).copied().unwrap_or(""));
                slices.insert(name.to_string(), Slice::new(&bytes));
                out.push_str(&format!("new {name} size={}\n", bytes.len()));
            }
            "clone" | "copy" => {
                let name = tokens[1];
                let source = tokens[2];
                let Some(src) = slices.get(source) else {
                    print!("{out}");
                    eprintln!("unknown slice {source}");
                    return ExitCode::from(2);
                };
                let made = if tokens[0] == "clone" { src.clone_slice() } else { src.copy_slice() };
                let size = made.size();
                slices.insert(name.to_string(), made);
                out.push_str(&format!("{} {name} from {source} size={size}\n", tokens[0]));
            }
            "mutate" => {
                let name = tokens[1];
                let index: usize = tokens[2].parse().unwrap_or(0);
                // The script writes the byte in decimal, the same way the C++
                // probe's `tokens >> unsigned int` reads it.
                let value: u8 = tokens[3].parse().unwrap_or(0);
                let Some(slice) = slices.get_mut(name) else {
                    print!("{out}");
                    eprintln!("unknown slice {name}");
                    return ExitCode::from(2);
                };
                slice.mutate(index, value);
                out.push_str(&format!("mutate {name} idx={index} byte={value:02x}\n"));
            }
            "read" => {
                let name = tokens[1];
                let count: usize = tokens[2].parse().unwrap_or(0);
                let Some(slice) = slices.get_mut(name) else {
                    print!("{out}");
                    eprintln!("unknown slice {name}");
                    return ExitCode::from(2);
                };
                let drained = slice.confirm_read(count);
                out.push_str(&format!("read {name} drained={} size={}\n", drained as u8, slice.size()));
            }
            "truncate" => {
                let name = tokens[1];
                let limit: usize = tokens[2].parse().unwrap_or(0);
                let Some(slice) = slices.get_mut(name) else {
                    print!("{out}");
                    eprintln!("unknown slice {name}");
                    return ExitCode::from(2);
                };
                slice.truncate(limit);
                out.push_str(&format!("truncate {name} size={}\n", slice.size()));
            }
            "after" => {
                let name = tokens[1];
                let offset: usize = tokens[2].parse().unwrap_or(0);
                let Some(slice) = slices.get(name) else {
                    print!("{out}");
                    eprintln!("unknown slice {name}");
                    return ExitCode::from(2);
                };
                out.push_str(&format!("after {name} hex={}\n", hex_of(&slice.after(offset))));
            }
            "print" => {
                let name = tokens[1];
                let Some(slice) = slices.get(name) else {
                    print!("{out}");
                    eprintln!("unknown slice {name}");
                    return ExitCode::from(2);
                };
                out.push_str(&format!("print {name} size={} hex={}\n", slice.size(), hex_of(&slice.as_slice())));
            }
            other => {
                print!("{out}");
                eprintln!("unknown op {other}");
                return ExitCode::from(2);
            }
        }
    }
    print!("{out}");
    ExitCode::SUCCESS
}