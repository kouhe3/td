// (Unofficial) Rust side of the `sqlite_safe` lane; the oracle is
// `tools/fixtures/sqlite_safe_probe.cpp` over the real td::SqliteConnectionSafe
// and td::SqliteKeyValueSafe.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin sqlite_safe_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/sqlite_safe_probe.exe <script.txt> <workdir>
//
// The Rust side models the contract, not sqlite: the lifecycle (lazy open,
// reuse, close keeps the file, close_and_destroy removes it, the state counter
// that shows up in the fatal message) plus the KV delegating to its own copy of
// the connection. Narration bytes match the C++ probe.

use std::collections::BTreeMap;
use std::io::Write;

#[derive(Default)]
struct Connection {
    opened: bool,
    close_state: i32,
}

impl Connection {
    fn get(&mut self, fs_exists: bool) {
        if !self.opened {
            if !fs_exists {
                eprintln!("Can't open database in state {}: database disappeared", self.close_state);
                std::process::abort();
            }
            self.opened = true;
        }
    }

    fn close(&mut self) {
        self.close_state += 1;
        self.opened = false;
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: sqlite_safe_probe <script.txt> <workdir>");
        std::process::exit(2);
    }
    let script = std::fs::read_to_string(&args[1]).expect("cannot read script");
    let mut out = std::io::stdout();

    let mut fs_exists = false;
    let mut connection: Option<Connection> = None;
    let mut kv: Option<BTreeMap<String, String>> = None;

    for raw_line in script.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        match tokens[0] {
            "prepare" => {
                fs_exists = true;
                writeln!(out, "prepare exists=1").unwrap();
            }
            "prepare_empty" => {
                fs_exists = false;
                writeln!(out, "prepare_empty exists=0").unwrap();
            }
            "conn" => {
                connection = Some(Connection::default());
                writeln!(out, "conn exists={}", fs_exists as u8).unwrap();
            }
            "get" => {
                connection.as_mut().expect("no connection").get(fs_exists);
                writeln!(out, "get exists={}", fs_exists as u8).unwrap();
            }
            "kv" => {
                kv = Some(BTreeMap::new());
                writeln!(out, "kv exists={}", fs_exists as u8).unwrap();
            }
            "set" => {
                let value = tokens.get(2).copied().unwrap_or("");
                kv.as_mut().expect("no kv").insert(tokens[1].to_string(), value.to_string());
                writeln!(out, "set {} {}", tokens[1], value).unwrap();
            }
            "sget" => {
                let value = kv.as_ref().expect("no kv").get(tokens[1]).cloned().unwrap_or_default();
                writeln!(out, "sget {}={}", tokens[1], value).unwrap();
            }
            "close" => {
                connection.as_mut().expect("no connection").close();
                writeln!(out, "close exists={}", fs_exists as u8).unwrap();
            }
            "destroy" => {
                let connection = connection.as_mut().expect("no connection");
                connection.close();
                connection.close_state += 65536;
                fs_exists = false;
                writeln!(out, "destroy exists=0").unwrap();
            }
            "drop" => {
                kv = None;
                connection = None;
                writeln!(out, "drop exists={}", fs_exists as u8).unwrap();
            }
            "exists" => {
                writeln!(out, "exists exists={}", fs_exists as u8).unwrap();
            }
            other => {
                eprintln!("unknown command {other}");
                std::process::exit(2);
            }
        }
    }
    writeln!(out, "done").unwrap();
}