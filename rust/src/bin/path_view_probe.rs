//! tdutils PathView on the Rust side.
//!
//! ```text
//! path_view_probe <script.txt>
//! ```
//!
//! Script lines: `path <text>`, `rel <path> <dir> [force]`, `dirfile <text>`.
//!
//! Both `/` and `\` are separators. The extension scan stops at `last_slash + 2`,
//! so a dot in the first byte of the file name is not an extension. Slices are
//! printed as hex, "-" for empty; `parent_dir_noslash` of a root-only path is a
//! single `\` (the Windows build's TD_DIR_SLASH).

use std::env;
use std::fs;
use std::process::ExitCode;

const DIR_SLASH: u8 = b'\\';

fn is_slash(byte: u8) -> bool {
    byte == b'/' || byte == b'\\'
}

struct PathView<'a> {
    path: &'a [u8],
    last_slash: i32,
    last_dot: i32,
}

impl<'a> PathView<'a> {
    fn new(path: &'a [u8]) -> Self {
        let mut last_slash = path.len() as i32 - 1;
        while last_slash >= 0 && !is_slash(path[last_slash as usize]) {
            last_slash -= 1;
        }
        let mut last_dot = path.len() as i32;
        let mut index = last_dot - 1;
        while index > last_slash + 1 {
            if path[index as usize] == b'.' {
                last_dot = index;
                break;
            }
            index -= 1;
        }
        Self { path, last_slash, last_dot }
    }

    fn empty(&self) -> bool {
        self.path.is_empty()
    }
    fn is_dir(&self) -> bool {
        !self.empty() && is_slash(self.path[self.path.len() - 1])
    }
    fn parent_dir(&self) -> &'a [u8] {
        &self.path[..(self.last_slash + 1) as usize]
    }
    fn parent_dir_noslash(&self) -> Vec<u8> {
        if self.last_slash < 0 {
            return b".".to_vec();
        }
        if self.last_slash == 0 {
            return vec![DIR_SLASH];
        }
        self.path[..self.last_slash as usize].to_vec()
    }
    fn extension(&self) -> &'a [u8] {
        if self.last_dot == self.path.len() as i32 {
            return &[];
        }
        &self.path[(self.last_dot + 1) as usize..]
    }
    fn without_extension(&self) -> &'a [u8] {
        &self.path[..self.last_dot as usize]
    }
    fn file_stem(&self) -> &'a [u8] {
        &self.path[(self.last_slash + 1) as usize..self.last_dot as usize]
    }
    fn file_name(&self) -> &'a [u8] {
        &self.path[(self.last_slash + 1) as usize..]
    }
    fn file_name_without_extension(&self) -> &'a [u8] {
        &self.path[(self.last_slash + 1) as usize..self.last_dot as usize]
    }
    fn is_absolute(&self) -> bool {
        !self.empty()
            && (is_slash(self.path[0])
                || (self.path.len() >= 3 && self.path[1] == b':' && is_slash(self.path[2])))
    }
}

fn relative<'a>(path: &'a [u8], dir: &[u8], force: bool) -> &'a [u8] {
    if path.len() >= dir.len() && &path[..dir.len()] == dir {
        return &path[dir.len()..];
    }
    if force {
        return &[];
    }
    path
}

fn dir_and_file(path: &[u8]) -> &[u8] {
    let mut last_slash = path.len() as i32 - 1;
    while last_slash >= 0 && !is_slash(path[last_slash as usize]) {
        last_slash -= 1;
    }
    if last_slash < 0 {
        return &[];
    }
    last_slash -= 1;
    while last_slash >= 0 && !is_slash(path[last_slash as usize]) {
        last_slash -= 1;
    }
    if last_slash < 0 {
        return &[];
    }
    &path[(last_slash + 1) as usize..]
}

fn hex_or_dash(data: &[u8]) -> String {
    if data.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// "-" stands for the empty path, which cannot be written as a token.
fn decode(token: &str) -> &str {
    if token == "-" {
        ""
    } else {
        token
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: path_view_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let op = parts[0];
        let value = decode(parts.get(1).copied().unwrap_or("")).as_bytes();
        match op {
            "path" => {
                let view = PathView::new(value);
                println!(
                    "path in={} empty={} dir={} abs={} rel={} parent={} pns={} ext={} stem={} name={} nodot={} nox={}",
                    hex_or_dash(value),
                    view.empty() as i32,
                    view.is_dir() as i32,
                    view.is_absolute() as i32,
                    (!view.is_absolute()) as i32,
                    hex_or_dash(view.parent_dir()),
                    hex_or_dash(&view.parent_dir_noslash()),
                    hex_or_dash(view.extension()),
                    hex_or_dash(view.file_stem()),
                    hex_or_dash(view.file_name()),
                    hex_or_dash(view.file_name_without_extension()),
                    hex_or_dash(view.without_extension())
                );
            }
            "rel" => {
                let dir = decode(parts.get(2).copied().unwrap_or("")).as_bytes();
                let force = parts.get(3).copied().unwrap_or("0") == "1";
                println!(
                    "rel in={} dir={} force={} out={}",
                    hex_or_dash(value),
                    hex_or_dash(dir),
                    force as i32,
                    hex_or_dash(relative(value, dir, force))
                );
            }
            "dirfile" => {
                println!("dirfile in={} out={}", hex_or_dash(value), hex_or_dash(dir_and_file(value)));
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}