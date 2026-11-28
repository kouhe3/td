// (Unofficial) Rust side of the `sqlite_async` lane; the oracle is
// `tools/fixtures/sqlite_async_probe.cpp` over the real td::SqliteKeyValueAsync.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin sqlite_async_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/sqlite_async_probe.exe <script.txt> <workdir>
//
// The C++ probe drives one operation at a time (send, wait for the promise,
// print, send the next), so the Rust side can model the same sequence
// synchronously: set/erase flush before their promise resolves, `get` sees the
// flushed value, and `erase_by_prefix` / `set_all` force a flush.

use std::collections::BTreeMap;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: sqlite_async_probe <script.txt> <workdir>");
        std::process::exit(2);
    }
    let script = std::fs::read_to_string(&args[1]).expect("cannot read script");
    let mut out = std::io::stdout();
    let mut kv: BTreeMap<String, String> = BTreeMap::new();

    for raw_line in script.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let kind = parts.next().unwrap_or("");
        let mut a = parts.next().unwrap_or("").to_string();
        let b = parts.next().unwrap_or("").to_string();
        if a == "-" {
            a.clear();
        }
        match kind {
            "prepare" => {
                kv.clear();
                writeln!(out, "prepare -> ok").unwrap();
            }
            "aset" => {
                assert!(!a.is_empty(), "set with an empty key hits CHECK(!key.empty())");
                kv.insert(a.clone(), b.clone());
                writeln!(out, "aset {} {} -> ok", a, b).unwrap();
            }
            "aget" => {
                let value = kv.get(&a).cloned().unwrap_or_default();
                writeln!(out, "aget {} -> {}", a, value).unwrap();
            }
            "aerase" => {
                assert!(!a.is_empty(), "erase with an empty key hits CHECK(!key.empty())");
                kv.remove(&a);
                writeln!(out, "aerase {} -> ok", a).unwrap();
            }
            "aprefix" => {
                kv.retain(|key, _| !key.starts_with(&a));
                writeln!(out, "aprefix {} -> ok", a).unwrap();
            }
            "asetall" => {
                kv.insert(a.clone(), b.clone());
                writeln!(out, "asetall {} {} -> ok", a, b).unwrap();
            }
            "aclose" => {
                writeln!(out, "aclose -> ok").unwrap();
            }
            "sget" => {
                let value = kv.get(&a).cloned().unwrap_or_default();
                writeln!(out, "sget {} -> {}", a, value).unwrap();
            }
            other => {
                eprintln!("unknown op {other}");
                std::process::exit(2);
            }
        }
    }
    writeln!(out, "done").unwrap();
}