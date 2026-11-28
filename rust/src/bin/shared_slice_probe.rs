//! tdutils SharedSlice / UniqueSharedSlice / UniqueSlice semantics on the Rust side.
//!
//! ```text
//! shared_slice_probe <script.txt>
//! ```
//!
//! A default-constructed `SharedSlice` and `SharedSlice(Slice())` are
//! indistinguishable through the public API (`Slice()` points its data at "",
//! and `is_unique()` is not public), so both trace as size=0 and both
//! clone/convert to an empty slice. `UniqueSharedSlice` does not copy on write:
//! the constructor steals the buffer when the source is the only owner and
//! deep-copies it otherwise, leaving the source empty in both cases.

use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;
use std::rc::Rc;

type Buf = Rc<RefCell<Vec<u8>>>;

#[derive(Default)]
struct SharedSlice(Option<Buf>);

impl SharedSlice {
    fn new(bytes: &[u8]) -> Self {
        Self(Some(Rc::new(RefCell::new(bytes.to_vec()))))
    }
    fn clone_slice(&self) -> Self {
        Self(self.0.as_ref().map(Rc::clone))
    }
    fn size(&self) -> usize {
        self.0.as_ref().map_or(0, |buf| buf.borrow().len())
    }
    fn empty(&self) -> bool {
        self.size() == 0
    }
    fn as_slice(&self) -> Vec<u8> {
        self.0.as_ref().map_or_else(Vec::new, |buf| buf.borrow().clone())
    }
    fn clone_as_buffer_slice(&self) -> Vec<u8> {
        self.as_slice()
    }
    fn clear(&mut self) {
        self.0 = None;
    }
}

#[derive(Default)]
struct UniqueSharedSlice(Option<Buf>);

impl UniqueSharedSlice {
    fn from_shared(from: SharedSlice) -> Self {
        match from.0 {
            None => Self(None),
            // the source is the only owner: steal the buffer instead of copying
            Some(buf) if Rc::strong_count(&buf) == 1 => Self(Some(buf)),
            Some(buf) => Self(Some(Rc::new(RefCell::new(buf.borrow().clone())))),
        }
    }
    fn into_shared(self) -> SharedSlice {
        SharedSlice(self.0)
    }
    fn mutate(&mut self, index: usize, value: u8) {
        let buf = self.0.as_ref().expect("mutate on a null unique slice");
        buf.borrow_mut()[index] = value;
    }
    fn size(&self) -> usize {
        self.0.as_ref().map_or(0, |buf| buf.borrow().len())
    }
    fn empty(&self) -> bool {
        self.size() == 0
    }
    fn as_slice(&self) -> Vec<u8> {
        self.0.as_ref().map_or_else(Vec::new, |buf| buf.borrow().clone())
    }
    fn clear(&mut self) {
        self.0 = None;
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

fn unknown(out: &str, name: &str) -> ExitCode {
    print!("{out}");
    eprintln!("unknown slice {name}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: shared_slice_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("read {}: {error}", args[1]);
            return ExitCode::from(1);
        }
    };

    let mut shared: HashMap<String, SharedSlice> = HashMap::new();
    let mut unique_shared: HashMap<String, UniqueSharedSlice> = HashMap::new();
    let mut buffers: HashMap<String, Vec<u8>> = HashMap::new();
    let mut uniques: HashMap<String, Option<Vec<u8>>> = HashMap::new();
    let mut secures: HashMap<String, Option<Vec<u8>>> = HashMap::new();
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
                let made = SharedSlice::new(&unhex(tokens.get(2).copied().unwrap_or("")));
                let size = made.size();
                shared.insert(name.to_string(), made);
                out.push_str(&format!("new {name} size={size}\n"));
            }
            "default" => {
                let name = tokens[1];
                shared.insert(name.to_string(), SharedSlice::default());
                out.push_str(&format!("default {name} size=0\n"));
            }
            "clone" => {
                let (name, source) = (tokens[1], tokens[2]);
                let Some(src) = shared.get(source) else {
                    return unknown(&out, source);
                };
                let made = src.clone_slice();
                let size = made.size();
                shared.insert(name.to_string(), made);
                out.push_str(&format!("clone {name} from {source} size={size}\n"));
            }
            "unique" => {
                let (name, source) = (tokens[1], tokens[2]);
                let Some(src) = shared.get_mut(source) else {
                    return unknown(&out, source);
                };
                // the C++ constructor takes the source by value, so the script sees
                // the moved-from slice as null afterwards
                let made = UniqueSharedSlice::from_shared(std::mem::take(src));
                let size = made.size();
                unique_shared.insert(name.to_string(), made);
                out.push_str(&format!("unique {name} from {source} size={size}\n"));
            }
            "to_shared" => {
                let (name, source) = (tokens[1], tokens[2]);
                let Some(src) = unique_shared.get_mut(source) else {
                    return unknown(&out, source);
                };
                let made = std::mem::take(src).into_shared();
                let size = made.size();
                shared.insert(name.to_string(), made);
                out.push_str(&format!("to_shared {name} from {source} size={size}\n"));
            }
            "mutate" => {
                let name = tokens[1];
                let index: usize = tokens[2].parse().unwrap_or(0);
                let value: u8 = tokens[3].parse().unwrap_or(0);
                let Some(slice) = unique_shared.get_mut(name) else {
                    return unknown(&out, name);
                };
                slice.mutate(index, value);
                out.push_str(&format!("mutate {name} idx={index} byte={value:02x}\n"));
            }
            "buffer" => {
                let (name, source) = (tokens[1], tokens[2]);
                let Some(src) = shared.get(source) else {
                    return unknown(&out, source);
                };
                let made = src.clone_as_buffer_slice();
                let size = made.len();
                buffers.insert(name.to_string(), made);
                out.push_str(&format!("buffer {name} from {source} size={size}\n"));
            }
            "clear" => {
                let name = tokens[1];
                if let Some(slice) = shared.get_mut(name) {
                    slice.clear();
                } else if let Some(slice) = unique_shared.get_mut(name) {
                    slice.clear();
                } else if let Some(slice) = buffers.get_mut(name) {
                    slice.clear();
                } else if let Some(slice) = uniques.get_mut(name) {
                    *slice = None;
                } else if let Some(slice) = secures.get_mut(name) {
                    *slice = None;
                } else {
                    return unknown(&out, name);
                }
                out.push_str(&format!("clear {name}\n"));
            }
            "unew" | "snew" => {
                let name = tokens[1];
                let size: usize = tokens[2].parse().unwrap_or(0);
                let byte: u8 = tokens[3].parse().unwrap_or(0);
                let made = Some(vec![byte; size]);
                if tokens[0] == "unew" {
                    uniques.insert(name.to_string(), made);
                    out.push_str(&format!("unew {name} size={size}\n"));
                } else {
                    secures.insert(name.to_string(), made);
                    out.push_str(&format!("snew {name} size={size}\n"));
                }
            }
            "ucopy" | "scopy" => {
                let (name, source) = (tokens[1], tokens[2]);
                let table = if tokens[0] == "ucopy" { &uniques } else { &secures };
                let Some(src) = table.get(source) else {
                    return unknown(&out, source);
                };
                let made = src.clone();
                let size = made.as_ref().map_or(0, |bytes| bytes.len());
                if tokens[0] == "ucopy" {
                    uniques.insert(name.to_string(), made);
                } else {
                    secures.insert(name.to_string(), made);
                }
                out.push_str(&format!("{} {name} from {source} size={size}\n", tokens[0]));
            }
            "umutate" | "smutate" => {
                let name = tokens[1];
                let index: usize = tokens[2].parse().unwrap_or(0);
                let value: u8 = tokens[3].parse().unwrap_or(0);
                let table = if tokens[0] == "umutate" { &mut uniques } else { &mut secures };
                let Some(slice) = table.get_mut(name) else {
                    return unknown(&out, name);
                };
                slice.as_mut().expect("mutate on a null slice")[index] = value;
                out.push_str(&format!("{} {name} idx={index} byte={value:02x}\n", tokens[0]));
            }
            "uclear" | "sclear" => {
                let name = tokens[1];
                let table = if tokens[0] == "uclear" { &mut uniques } else { &mut secures };
                let Some(slice) = table.get_mut(name) else {
                    return unknown(&out, name);
                };
                *slice = None;
                out.push_str(&format!("{} {name}\n", tokens[0]));
            }
            "print" => {
                let name = tokens[1];
                if let Some(slice) = shared.get(name) {
                    out.push_str(&format!(
                        "print {name} kind=shared size={} empty={} hex={}\n",
                        slice.size(),
                        slice.empty() as u8,
                        hex_of(&slice.as_slice())
                    ));
                } else if let Some(slice) = unique_shared.get(name) {
                    out.push_str(&format!(
                        "print {name} kind=unique_shared size={} empty={} hex={}\n",
                        slice.size(),
                        slice.empty() as u8,
                        hex_of(&slice.as_slice())
                    ));
                } else if let Some(slice) = buffers.get(name) {
                    out.push_str(&format!(
                        "print {name} kind=buffer size={} hex={}\n",
                        slice.len(),
                        hex_of(slice)
                    ));
                } else if let Some(slice) = uniques.get(name) {
                    let bytes = slice.as_deref().unwrap_or(&[]);
                    out.push_str(&format!(
                        "print {name} kind=unique size={} empty={} hex={}\n",
                        bytes.len(),
                        bytes.is_empty() as u8,
                        hex_of(bytes)
                    ));
                } else if let Some(slice) = secures.get(name) {
                    let bytes = slice.as_deref().unwrap_or(&[]);
                    out.push_str(&format!(
                        "print {name} kind=secure size={} empty={} hex={}\n",
                        bytes.len(),
                        bytes.is_empty() as u8,
                        hex_of(bytes)
                    ));
                } else {
                    return unknown(&out, name);
                }
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
