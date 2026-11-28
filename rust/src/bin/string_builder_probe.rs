//! tdutils StringBuilder on the Rust side, script-driven and ASCII-only.
//!
//! ```text
//! string_builder_probe <script.txt>
//! ```
//!
//! The buffer model is replicated literally, because it is what the oracle judges:
//!
//! * `StringBuilder()` allocates `RESERVED + 100` bytes and exposes `100` of them
//!   (`end - begin`), `use_buffer = true`;
//! * `StringBuilder(MutableSlice(n))` ignores the caller's slice when `n <= RESERVED`,
//!   allocating `RESERVED + 100` instead, and always keeps `use_buffer = false`; above
//!   `RESERVED` the usable size is `n - RESERVED`;
//! * `operator<<(Slice)`, `append_char` and `FixedDouble` append into the reserved
//!   `RESERVED` bytes when the growth attempt fails, cutting at `end + RESERVED - 1`;
//!   once `current > end` they only set the error flag;
//! * `operator<<(int)` and friends reserve one byte and then write up to 20 digits
//!   without further checks, so on a non-growable builder they can pass `end` with no
//!   error (`RESERVED` is sized for that); `push_back`/`append_char`/`Slice` then refuse;
//! * `%p` is 16 lowercase hex digits with no `0x`; `std::ostream` fixed formatting is
//!   `format!("{:.*}", …)` with `inf`/`-inf` and a sign-less `nan`, and a negative
//!   precision behaves as the omitted printf precision, i.e. 6.
//!
//! Script ops match `tools/fixtures/string_builder_probe.cpp` one for one.

use std::process::ExitCode;

const RESERVED: usize = 30;

#[derive(Clone)]
enum Val {
    I32(i32),
    U32(u32),
    Ch(u8),
    Slice(Vec<u8>),
}

struct Builder {
    buf: Vec<u8>,
    current: usize,
    end: usize,
    error: bool,
    use_buffer: bool,
}

fn hex_or_dash(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn unhex(text: &str) -> Vec<u8> {
    if text == "-" {
        return Vec::new();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index + 1 < bytes.len() {
        let hi = (bytes[index] as char).to_digit(16).unwrap();
        let lo = (bytes[index + 1] as char).to_digit(16).unwrap();
        out.push((hi * 16 + lo) as u8);
        index += 2;
    }
    out
}

impl Builder {
    fn new_default() -> Self {
        Builder { buf: vec![0; RESERVED + 100], current: 0, end: 100, error: false, use_buffer: true }
    }

    fn new_slice(cap: usize) -> Self {
        if cap <= RESERVED {
            Builder { buf: vec![0; RESERVED + 100], current: 0, end: 100, error: false, use_buffer: false }
        } else {
            Builder { buf: vec![0; cap], current: 0, end: cap - RESERVED, error: false, use_buffer: false }
        }
    }

    fn size(&self) -> usize {
        self.current
    }

    fn clear(&mut self) {
        self.current = 0;
        self.error = false;
    }

    fn put(&mut self, byte: u8) {
        self.buf[self.current] = byte;
        self.current += 1;
    }

    fn reserve(&mut self) -> bool {
        if self.end > self.current {
            return true;
        }
        self.reserve_inner(RESERVED)
    }

    fn reserve_size(&mut self, size: usize) -> bool {
        if self.end > self.current && self.end - self.current >= size {
            return true;
        }
        self.reserve_inner(size)
    }

    fn reserve_inner(&mut self, size: usize) -> bool {
        if !self.use_buffer {
            return false;
        }
        let old_data_size = self.current;
        if size >= usize::MAX - RESERVED - old_data_size - 1 {
            return false;
        }
        let need_data_size = old_data_size + size;
        let old_buffer_size = self.end;
        if old_buffer_size >= (usize::MAX - RESERVED) / 2 - 2 {
            return false;
        }
        let mut new_buffer_size = (old_buffer_size + 1) * 2;
        if new_buffer_size < need_data_size {
            new_buffer_size = need_data_size;
        }
        if new_buffer_size < 100 {
            new_buffer_size = 100;
        }
        new_buffer_size += RESERVED;
        let mut next = vec![0u8; new_buffer_size];
        next[..old_data_size].copy_from_slice(&self.buf[..old_data_size]);
        self.buf = next;
        self.current = old_data_size;
        self.end = new_buffer_size - RESERVED;
        true
    }

    // operator<<(Slice): the reserved bytes are usable, the last one stays free for the NUL.
    fn push_slice(&mut self, slice: &[u8]) {
        let mut size = slice.len();
        if !self.reserve_size(size) {
            if self.end < self.current {
                self.error = true;
                return;
            }
            let available_size = self.end + RESERVED - 1 - self.current;
            if size > available_size {
                self.error = true;
                size = available_size;
            }
        }
        self.buf[self.current..self.current + size].copy_from_slice(&slice[..size]);
        self.current += size;
    }

    fn append_char(&mut self, mut count: usize, byte: u8) {
        if !self.reserve_size(count) {
            if self.end < self.current {
                self.error = true;
                return;
            }
            let available_size = self.end + RESERVED - 1 - self.current;
            if count > available_size {
                self.error = true;
                count = available_size;
            }
        }
        for offset in 0..count {
            self.buf[self.current + offset] = byte;
        }
        self.current += count;
    }

    fn push_back(&mut self, byte: u8) {
        if self.end <= self.current {
            if !self.reserve_inner(RESERVED) {
                self.error = true;
                return;
            }
        }
        self.put(byte);
    }

    fn push_char(&mut self, byte: u8) {
        if !self.reserve() {
            self.error = true;
            return;
        }
        self.put(byte);
    }

    fn print_uint(&mut self, mut x: u64) {
        if x < 100 {
            if x < 10 {
                self.put(b'0' + x as u8);
            } else {
                self.put(b'0' + (x / 10) as u8);
                self.put(b'0' + (x % 10) as u8);
            }
            return;
        }
        let begin_ptr = self.current;
        loop {
            self.put(b'0' + (x % 10) as u8);
            x /= 10;
            if x == 0 {
                break;
            }
        }
        let mut begin = begin_ptr;
        let mut end = self.current - 1;
        while begin < end {
            self.buf.swap(begin, end);
            begin += 1;
            end -= 1;
        }
    }

    fn print_int_i32(&mut self, x: i32) {
        if x < 0 {
            if x == i32::MIN {
                // print x + 1 and bump the last digit, so no negation of the minimum happens.
                self.print_int_i32(x + 1);
                let last = self.current - 1;
                self.buf[last] += 1;
                return;
            }
            self.put(b'-');
            self.print_uint((-x) as u64);
            return;
        }
        self.print_uint(x as u64);
    }

    fn print_int_i64(&mut self, x: i64) {
        if x < 0 {
            if x == i64::MIN {
                self.print_int_i64(x + 1);
                let last = self.current - 1;
                self.buf[last] += 1;
                return;
            }
            self.put(b'-');
            self.print_uint((-x) as u64);
            return;
        }
        self.print_uint(x as u64);
    }

    fn push_i32(&mut self, x: i32) {
        if !self.reserve() {
            self.error = true;
            return;
        }
        self.print_int_i32(x);
    }

    fn push_u32(&mut self, x: u32) {
        if !self.reserve() {
            self.error = true;
            return;
        }
        self.print_uint(x as u64);
    }

    fn push_i64(&mut self, x: i64) {
        if !self.reserve() {
            self.error = true;
            return;
        }
        self.print_int_i64(x);
    }

    fn push_u64(&mut self, x: u64) {
        if !self.reserve() {
            self.error = true;
            return;
        }
        self.print_uint(x);
    }

    // std::ostream << double with std::fixed and the classic locale.
    fn push_fixed(&mut self, d: f64, precision: i32) {
        let reserve_size = (308 + precision as i64 + 4) as usize;
        if !self.reserve_size(reserve_size) {
            self.error = true;
            return;
        }
        let text = fixed_text(d, precision);
        let left = (self.end + RESERVED) as i64 - self.current as i64;
        let mut len = text.len();
        if len as i64 >= left {
            self.error = true;
            len = if left > 0 { (left - 1) as usize } else { 0 };
        }
        self.buf[self.current..self.current + len].copy_from_slice(&text.as_bytes()[..len]);
        self.current += len;
    }

    fn push_ptr(&mut self, value: u64) {
        if !self.reserve() {
            self.error = true;
            return;
        }
        let text = format!("{:016x}", value);
        for byte in text.as_bytes() {
            self.put(*byte);
        }
    }

    fn render(&mut self, value: &Val) {
        match value {
            Val::I32(x) => self.push_i32(*x),
            Val::U32(x) => self.push_u32(*x),
            Val::Ch(byte) => self.push_char(*byte),
            Val::Slice(bytes) => self.push_slice(bytes),
        }
    }

    fn push_pair(&mut self, left: &Val, right: &Val) {
        self.push_char(b'[');
        self.render(left);
        self.push_char(b';');
        self.render(right);
        self.push_char(b']');
    }

    fn push_vec(&mut self, values: &[Val]) {
        self.push_char(b'{');
        if !values.is_empty() {
            self.render(&values[0]);
            for value in &values[1..] {
                self.push_slice(b", ");
                self.render(value);
            }
        }
        self.push_char(b'}');
    }

    fn push_char_array(&mut self, bytes: &[u8]) {
        // operator<<(const char (&str)[N]) appends N - 1 bytes.
        self.push_slice(bytes);
    }

    fn push_bool(&mut self, value: bool) {
        self.push_slice(if value { b"true" } else { b"false" });
    }

    fn as_cslice(&mut self) -> usize {
        self.buf[self.current] = 0;
        self.current
    }
}

fn fixed_text(d: f64, precision: i32) -> String {
    if d.is_infinite() {
        return if d.is_sign_negative() { "-inf".to_string() } else { "inf".to_string() };
    }
    if d.is_nan() {
        // The oracle drops the sign of a negative NaN.
        return "nan".to_string();
    }
    // printf takes a negative precision as if it were omitted, i.e. 6 for %f.
    let digits = if precision < 0 { 6 } else { precision as usize };
    format!("{:.*}", digits, d)
}

fn from_bits(bits: u64) -> f64 {
    f64::from_bits(bits)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: string_builder_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match std::fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(_) => {
            eprintln!("cannot read {}", args[1]);
            return ExitCode::from(2);
        }
    };

    let mut builder: Option<Builder> = None;
    let literal0: &[u8] = b"";
    let literal1: &[u8] = b"abc";
    let literal2: &[u8] = b"0123456789";

    for line in text.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() || tokens[0].starts_with('#') {
            continue;
        }
        let op = tokens[0];
        if op == "new" {
            if tokens.len() > 1 {
                let cap: usize = tokens[1].parse().unwrap();
                builder = Some(Builder::new_slice(cap));
                println!("new kind=slice cap={}", tokens[1]);
            } else {
                builder = Some(Builder::new_default());
                println!("new kind=default");
            }
            continue;
        }
        let sb = match builder.as_mut() {
            Some(sb) => sb,
            None => {
                println!("unknown {}", op);
                continue;
            }
        };
        match op {
            "clr" => {
                sb.clear();
                println!("clr size={} err={}", sb.size(), sb.error as u8);
            }
            "dump" => {
                let size = sb.size();
                let error = sb.error;
                let csize = sb.as_cslice();
                let nul = (sb.buf[csize] == 0) as u8;
                println!(
                    "dump size={} err={} cs={} csize={} nul={}",
                    size,
                    error as u8,
                    hex_or_dash(&sb.buf[..csize]),
                    csize,
                    nul
                );
            }
            "s" => {
                let value = unhex(tokens[1]);
                let before = sb.size();
                sb.push_slice(&value);
                report(sb, &format!("s in={}", tokens[1]), before);
            }
            "cstr" => {
                let mut value = unhex(tokens[1]);
                if let Some(position) = value.iter().position(|&b| b == 0) {
                    value.truncate(position);
                }
                let before = sb.size();
                sb.push_slice(&value);
                report(sb, &format!("cstr in={}", tokens[1]), before);
            }
            "lit" => {
                let index: usize = tokens[1].parse().unwrap();
                let literal = match index {
                    0 => literal0,
                    1 => literal1,
                    _ => literal2,
                };
                let before = sb.size();
                sb.push_char_array(literal);
                report(sb, &format!("lit idx={}", tokens[1]), before);
            }
            "ch" | "sch" | "uch" => {
                let raw: u64 = tokens[1].parse().unwrap();
                let before = sb.size();
                if op == "ch" {
                    sb.push_char(raw as u8);
                } else if op == "sch" {
                    sb.push_i32(raw as u8 as i8 as i32);
                } else {
                    sb.push_u32(raw as u8 as u32);
                }
                report(sb, &format!("{} v={}", op, raw), before);
            }
            "b" => {
                let value = tokens[1].parse::<u64>().unwrap() != 0;
                let before = sb.size();
                sb.push_bool(value);
                report(sb, &format!("b v={}", value as u8), before);
            }
            "i32" | "u32" | "il" | "ul" | "i64" | "u64" => {
                let value_text = tokens[1];
                let before = sb.size();
                let width;
                match op {
                    "i32" => {
                        sb.push_i32(value_text.parse::<i32>().unwrap());
                        width = 4;
                    }
                    "u32" => {
                        sb.push_u32(value_text.parse::<u32>().unwrap());
                        width = 4;
                    }
                    "il" => {
                        sb.push_i32(value_text.parse::<i64>().unwrap() as i32);
                        width = 4;
                    }
                    "ul" => {
                        sb.push_u32(value_text.parse::<u64>().unwrap() as u32);
                        width = 4;
                    }
                    "i64" => {
                        sb.push_i64(value_text.parse::<i64>().unwrap());
                        width = 8;
                    }
                    _ => {
                        sb.push_u64(value_text.parse::<u64>().unwrap());
                        width = 8;
                    }
                }
                report(sb, &format!("{} v={} w={}", op, value_text, width), before);
            }
            "d" | "dp" => {
                let (precision_text, bits_text) = if op == "dp" {
                    (tokens[1], tokens[2])
                } else {
                    ("6", tokens[1])
                };
                let precision: i32 = precision_text.parse().unwrap();
                let bits = u64::from_str_radix(bits_text, 16).unwrap();
                let before = sb.size();
                sb.push_fixed(from_bits(bits), precision);
                report(sb, &format!("{} bits={} p={}", op, bits_text, precision), before);
            }
            "ptr" => {
                let value = u64::from_str_radix(tokens[1], 16).unwrap();
                let before = sb.size();
                sb.push_ptr(value);
                report(sb, &format!("ptr v={}", tokens[1]), before);
            }
            "pair" => {
                let parse = |tag: &str, arg: &str| -> Val {
                    match tag {
                        "i32" => Val::I32(arg.parse::<i64>().unwrap() as i32),
                        "u32" => Val::U32(arg.parse::<u64>().unwrap() as u32),
                        "ch" => Val::Ch(arg.parse::<u64>().unwrap() as u8),
                        _ => Val::Slice(unhex(arg)),
                    }
                };
                let left = parse(tokens[1], tokens[2]);
                let right = parse(tokens[3], tokens[4]);
                let before = sb.size();
                sb.push_pair(&left, &right);
                report(sb, &format!("pair a={}:{} b={}:{}", tokens[1], tokens[2], tokens[3], tokens[4]), before);
            }
            "vec" => {
                let ty = tokens[1];
                let mut values = Vec::new();
                for arg in &tokens[2..] {
                    values.push(match ty {
                        "i32" => Val::I32(arg.parse::<i64>().unwrap() as i32),
                        "u32" => Val::U32(arg.parse::<u64>().unwrap() as u32),
                        "ch" => Val::Ch(arg.parse::<u64>().unwrap() as u8),
                        "b" => Val::Slice(if *arg != "0" { b"true".to_vec() } else { b"false".to_vec() }),
                        _ => Val::Slice(unhex(arg)),
                    });
                }
                let before = sb.size();
                sb.push_vec(&values);
                report(sb, &format!("vec ty={} n={}", ty, values.len()), before);
            }
            "ac" => {
                let count: usize = tokens[1].parse().unwrap();
                let byte: u8 = tokens[2].parse().unwrap();
                let before = sb.size();
                sb.append_char(count, byte);
                report(sb, &format!("ac count={} v={}", tokens[1], tokens[2]), before);
            }
            "pb" => {
                let byte: u8 = tokens[1].parse().unwrap();
                let before = sb.size();
                sb.push_back(byte);
                report(sb, &format!("pb v={}", tokens[1]), before);
            }
            "pop" => {
                // CHECK(current > begin) aborts in the oracle; abort() keeps the exit status comparable.
                if sb.current == 0 {
                    std::process::abort();
                }
                sb.current -= 1;
                println!("pop size={} err={}", sb.size(), sb.error as u8);
            }
            _ => println!("unknown {}", op),
        }
    }
    ExitCode::SUCCESS
}

fn report(sb: &Builder, prefix: &str, before: usize) {
    let after = sb.size();
    let added = if after > before { &sb.buf[before..after] } else { &sb.buf[0..0] };
    println!("{} out={} size={} err={}", prefix, hex_or_dash(added), after, sb.error as u8);
}