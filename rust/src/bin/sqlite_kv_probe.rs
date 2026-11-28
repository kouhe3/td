//! Judge `td::SqliteKeyValue` (tddb/td/db/SqliteKeyValue.h) against the C++
//! oracle (`tools/fixtures/sqlite_kv_probe.cpp`), which runs this repository's
//! own SQLCipher 3.31 build.
//!
//!   sqlite_kv_probe <script.txt> <db_path>
//!   sqlite_kv_probe --check <db_path> <case>
//!
//! The statements live in `rust/src/sqlite_kv.rs`; this file only narrates.
//! Every command prints exactly one line, and the line order of `getall`
//! (rowid order) versus `range`/`prefix` (key order) is part of the criterion.
//! Values are hex-truncated to 32 bytes with `vlen` carrying the real length;
//! `crc` prints a zlib crc32 over all bytes for values too big to print.
//!
//! `--check` exists because a zero-length key aborts `get_all()` on both sides
//! (`CHECK(!key.empty())` in C++, the `assert!` in `get_all`): one process per
//! case, and the harness only compares "did it abort", never the exact code --
//! TDLib's `process_abort` is a fail-fast exception while Rust's `panic=abort`
//! leaves through the CRT, so the numbers differ by platform, not by behaviour.

#[allow(dead_code)]
#[path = "../binlog_format.rs"]
mod binlog_format;
#[path = "../sqlite_kv.rs"]
mod sqlite_kv;

use std::io::BufRead;

use sqlite_kv::SqliteKeyValue;

const VALUE_CAP: usize = 32;

fn hex_all(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex_head(bytes: &[u8]) -> String {
    if bytes.len() <= VALUE_CAP {
        hex_all(bytes)
    } else {
        hex_all(&bytes[..VALUE_CAP])
    }
}

fn yes(flag: bool) -> &'static str {
    if flag {
        "1"
    } else {
        "0"
    }
}

type Rows = Vec<(Vec<u8>, Vec<u8>)>;

fn collect(kv: &SqliteKeyValue, prefix: &[u8]) -> Rows {
    let mut rows = Rows::new();
    kv.get_by_prefix(prefix, |key, value| {
        rows.push((key.to_vec(), value.to_vec()));
        true
    });
    rows
}

fn print_rows(tag: &str, rows: &Rows) {
    let mut line = format!("{tag} n={}", rows.len());
    for (key, value) in rows {
        line.push_str(&format!(" {}={}", hex_all(key), hex_head(value)));
    }
    println!("{line}");
}

fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

struct State {
    kv: Option<SqliteKeyValue>,
    path: String,
}

impl State {
    fn open(&mut self, allow_creation: bool) -> Result<(), String> {
        let kv = SqliteKeyValue::open(&self.path, allow_creation, "kv")?;
        self.kv = Some(kv);
        Ok(())
    }

    fn close(&mut self) {
        if let Some(kv) = self.kv.as_mut() {
            kv.close();
        }
        self.kv = None;
    }

    fn require(&mut self) -> &mut SqliteKeyValue {
        self.kv.as_mut().expect("the key-value is not open")
    }
}

fn run_script(script_path: &str, db_path: &str) -> i32 {
    let file = match std::fs::File::open(script_path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("cannot read {script_path}: {e}");
            return 2;
        }
    };
    let mut state = State {
        kv: None,
        path: db_path.to_string(),
    };
    let mut first = true;
    for line in std::io::BufReader::new(file).lines() {
        let line = match line {
            Ok(line) => line,
            Err(e) => {
                eprintln!("read error: {e}");
                return 2;
            }
        };
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() || tokens[0].starts_with('#') {
            continue;
        }
        let command = tokens[0];

        if first && command == "attach" {
            if let Err(e) = state.open(false) {
                println!("reject open reason={e}");
                return 3;
            }
            let rows = collect(state.kv.as_ref().unwrap(), b"");
            println!("attach ok=1 n={}", rows.len());
            first = false;
            continue;
        }
        if first {
            sqlite_kv::destroy(db_path).ok();
            if let Err(e) = state.open(true) {
                println!("reject init reason={e}");
                return 3;
            }
            first = false;
        }

        match command {
            "set" => {
                let (Some(key), Some(value)) =
                    (tokens.get(1).and_then(|t| binlog_format::from_hex(t)), tokens.get(2).and_then(|t| binlog_format::from_hex(t)))
                else {
                    println!("reject set argc={}", tokens.len());
                    continue;
                };
                state.require().set(&key, &value);
                println!("set key={} vlen={}", hex_all(&key), value.len());
            }
            "get" | "crc" => {
                let Some(key) = tokens.get(1).and_then(|t| binlog_format::from_hex(t)) else {
                    println!("reject {command} argc={}", tokens.len());
                    continue;
                };
                let value = state.require().get(&key);
                if command == "get" {
                    println!(
                        "get key={} value={} vlen={}",
                        hex_all(&key),
                        hex_head(&value),
                        value.len()
                    );
                } else {
                    println!(
                        "crc key={} vlen={} crc={:08x}",
                        hex_all(&key),
                        value.len(),
                        binlog_format::crc32(&value)
                    );
                }
            }
            "getall" => {
                let rows = collect(state.kv.as_ref().unwrap(), b"");
                print_rows("getall", &rows);
            }
            "prefix" => {
                let Some(prefix) = tokens.get(1).and_then(|t| binlog_format::from_hex(t)) else {
                    println!("reject prefix argc={}", tokens.len());
                    continue;
                };
                let rows = collect(state.kv.as_ref().unwrap(), &prefix);
                let mut line = format!("prefix p={} n={}", hex_all(&prefix), rows.len());
                for (key, value) in &rows {
                    line.push_str(&format!(" {}={}", hex_all(key), hex_head(value)));
                }
                println!("{line}");
            }
            "range" => {
                let (Some(from), Some(till)) = (
                    tokens.get(1).and_then(|t| binlog_format::from_hex(t)),
                    tokens.get(2).and_then(|t| binlog_format::from_hex(t)),
                ) else {
                    println!("reject range argc={}", tokens.len());
                    continue;
                };
                let mut rows = Rows::new();
                state.kv.as_ref().unwrap().get_by_range(&from, &till, |key, value| {
                    rows.push((key.to_vec(), value.to_vec()));
                    true
                });
                let mut line = format!(
                    "range from={} till={} n={}",
                    hex_all(&from),
                    hex_all(&till),
                    rows.len()
                );
                for (key, value) in &rows {
                    line.push_str(&format!(" {}={}", hex_all(key), hex_head(value)));
                }
                println!("{line}");
            }
            "erase" => {
                let Some(key) = tokens.get(1).and_then(|t| binlog_format::from_hex(t)) else {
                    println!("reject erase argc={}", tokens.len());
                    continue;
                };
                state.require().erase(&key);
                println!("erase key={}", hex_all(&key));
            }
            "erasebatch" => {
                let mut keys = Vec::new();
                let mut ok = true;
                for token in &tokens[1..] {
                    match binlog_format::from_hex(token) {
                        Some(key) => keys.push(key),
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok {
                    println!("reject erasebatch");
                    continue;
                }
                state.require().erase_batch(&keys);
                println!("erasebatch keys={}", tokens.len() - 1);
            }
            "eraseprefix" => {
                let Some(prefix) = tokens.get(1).and_then(|t| binlog_format::from_hex(t)) else {
                    println!("reject eraseprefix argc={}", tokens.len());
                    continue;
                };
                let removed = collect(state.kv.as_ref().unwrap(), &prefix).len();
                state.require().erase_by_prefix(&prefix);
                println!("eraseprefix p={} removed={removed}", hex_all(&prefix));
            }
            "count" => {
                let n = state.require().get_all().len();
                println!("count n={n}");
            }
            "begin" | "wbegin" | "commit" => {
                let result = match command {
                    "begin" => state.require().begin_read_transaction(),
                    "wbegin" => state.require().begin_write_transaction(),
                    _ => state.require().commit_transaction(),
                };
                match result {
                    Ok(()) => println!("{command} ok=1"),
                    Err(e) => println!("{command} ok=0 err={e}"),
                }
            }
            "close" => {
                state.close();
                println!("close ok=1 empty={}", yes(state.kv.is_none()));
            }
            "reopen" => {
                state.close();
                if let Err(e) = state.open(true) {
                    println!("reject init reason={e}");
                    return 3;
                }
                let n = collect(state.kv.as_ref().unwrap(), b"").len();
                println!("reopen ok=1 n={n}");
            }
            "drop" => {
                let mut kv = match state.kv.take() {
                    Some(kv) => kv,
                    None => {
                        // `drop()` on an already closed KV is a no-op returning OK.
                        println!("drop ok=1 empty=1");
                        continue;
                    }
                };
                let result = kv.drop_table();
                println!(
                    "drop ok={} empty={}",
                    yes(result.is_ok()),
                    yes(kv.is_empty())
                );
            }
            "destroy" => {
                state.close();
                let result = sqlite_kv::destroy(db_path);
                println!("destroy ok={}", yes(result.is_ok()));
            }
            "footprint" => {
                println!(
                    "footprint f={} j={} w={} s={}",
                    yes(exists(db_path)),
                    yes(exists(&format!("{db_path}-journal"))),
                    yes(exists(&format!("{db_path}-wal"))),
                    yes(exists(&format!("{db_path}-shm")))
                );
            }
            other => println!("reject unknown={other}"),
        }
    }
    if let Some(kv) = state.kv.as_mut() {
        kv.close();
    }
    0
}

fn run_check(db_path: &str, case_name: &str) -> i32 {
    sqlite_kv::destroy(db_path).ok();
    let mut kv = match SqliteKeyValue::open(db_path, true, "kv") {
        Ok(kv) => kv,
        Err(e) => {
            println!("reject init reason={e}");
            return 3;
        }
    };
    kv.set(b"", b"x");
    match case_name {
        "empty_getall" => {
            println!("before n={}", kv.get_all().len());
            println!("after");
        }
        "empty_prefix" => {
            let mut n = 0;
            kv.get_by_prefix(b"", |_, _| {
                n += 1;
                true
            });
            println!("before n={n}");
            println!("after");
        }
        other => {
            eprintln!("unknown check case {other}");
            return 2;
        }
    }
    kv.close();
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 4 && args[1] == "--check" {
        std::process::exit(run_check(&args[2], &args[3]));
    }
    if args.len() != 3 {
        eprintln!(
            "usage: sqlite_kv_probe <script.txt> <db_path>\n       sqlite_kv_probe --check <db_path> <case>"
        );
        std::process::exit(2);
    }
    std::process::exit(run_script(&args[1], &args[2]));
}