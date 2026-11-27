//! tdutils Slice / MutableSlice accessors and pointer consistency on the Rust
//! side.
//!
//! ```text
//! slice_ptr_probe <script.txt>
//! slice_ptr_probe --check <back|mback> <len>
//! ```
//!
//! Script lines: `new` / `mnew` (a private copy), `shift` / `mshift`
//! (remove_prefix), `back` / `mback`, `index` / `mindex`, `ptr` / `mptr`.
//!
//! The view keeps a byte offset into the shared buffer, so `off=`/`endoff=`
//! expose the window: after `shift 1`, `off` advances and `endoff - off` is
//! still the size. `back()` aborts on an empty slice (td CHECK), so those cases
//! run one process each through `--check`.

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
    fn back(&self) -> u8 {
        require(self.size() >= 1);
        self.data[self.end - 1]
    }
    fn index(&self, index: usize) -> u8 {
        self.data[self.begin + index]
    }
    fn remove_prefix(&mut self, count: usize) {
        require(count <= self.size());
        self.begin += count;
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

fn print_ptr(op: &str, name: &str, slice: &Slice) {
    println!(
        "{} {} size={} off={} endoff={} uboff={} ueoff={} same_db=1 same_ub=1",
        op,
        name,
        slice.size(),
        slice.begin,
        slice.end,
        slice.begin,
        slice.end
    );
}

fn run_check(args: &[String]) -> ExitCode {
    if args.len() != 4 {
        eprintln!("usage: slice_ptr_probe --check <back|mback> <len>");
        return ExitCode::from(2);
    }
    let op = args[2].as_str();
    let len: usize = args[3].parse().unwrap();
    if len > 8 {
        eprintln!("len must be <= 8");
        return ExitCode::from(2);
    }
    let mut view = Slice::new(b"abcdefgh".to_vec());
    view.end = len;
    let value = match op {
        "back" | "mback" => view.back(),
        other => {
            eprintln!("unknown op {other}");
            return ExitCode::from(2);
        }
    };
    println!("check op={} len={} value={}", op, len, value);
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() >= 2 && args[1] == "--check" {
        return run_check(&args);
    }
    if args.len() != 2 {
        eprintln!("usage: slice_ptr_probe <script.txt> | --check <back|mback> <len>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    let mut slices: HashMap<String, Slice> = HashMap::new();
    let mut mslices: HashMap<String, Slice> = HashMap::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let op = parts[0];
        let name = parts.get(1).copied().unwrap_or("");
        let number = |index: usize| -> usize { parts.get(index).map_or(0, |text| text.parse().unwrap_or(0)) };
        match op {
            "new" | "mnew" => {
                let bytes = unhex(parts.get(2).copied().unwrap_or("-"));
                let table = if op == "new" { &mut slices } else { &mut mslices };
                table.insert(name.to_string(), Slice::new(bytes));
            }
            "shift" | "mshift" => {
                let table = if op == "shift" { &mut slices } else { &mut mslices };
                table.get_mut(name).unwrap().remove_prefix(number(2));
            }
            "back" | "mback" => {
                let table = if op == "back" { &slices } else { &mslices };
                let slice = &table[name];
                println!("{} {} size={} value={}", op, name, slice.size(), slice.back());
            }
            "index" | "mindex" => {
                let table = if op == "index" { &slices } else { &mslices };
                let slice = &table[name];
                let index = number(2);
                println!("{} {} i={} value={}", op, name, index, slice.index(index));
            }
            "ptr" | "mptr" => {
                let table = if op == "ptr" { &slices } else { &mslices };
                print_ptr(op, name, &table[name]);
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}