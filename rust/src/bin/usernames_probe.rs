// (Unofficial) Rust side of the `Usernames` lane; the oracle is
// `tools/fixtures/usernames_probe.cpp` over the real `td::Usernames`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin usernames_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/usernames_probe.exe <script.txt>
//
// Same script language and same stdout bytes as the C++ probe; `--check
// <case>` exists only so the harness can compare "did the process abort".
//
// The model lives in `telegram_usernames.rs` because the `td_api_objects` lane
// drives `get_usernames_object()` on top of it; this file is the narration.

use std::io::Write;

#[allow(dead_code)]
#[path = "../script_tokens.rs"]
mod script_tokens;

#[allow(dead_code)]
#[path = "../telegram_usernames.rs"]
mod telegram_usernames;

use script_tokens::*;
use telegram_usernames::*;

fn hex_encode(bytes: &[u8]) -> Vec<u8> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize]);
        out.push(DIGITS[(b & 15) as usize]);
    }
    out
}

fn print_state(usernames: &Usernames, hex: bool, out: &mut std::io::Stdout) {
    let rendered = usernames.render();
    if hex {
        let mut line = b"statehex=".to_vec();
        line.extend_from_slice(&hex_encode(&rendered));
        line.push(b'\n');
        out.write_all(&line).unwrap();
    } else {
        let mut line = b"state=".to_vec();
        line.extend_from_slice(&rendered);
        line.push(b'\n');
        out.write_all(&line).unwrap();
    }
}

fn print_accessors(usernames: &Usernames, out: &mut std::io::Stdout) {
    let first = usernames.first_username();
    let editable = usernames.editable_username();
    let mut line = format!(
        "acc empty={} hasfirst={} first=",
        usernames.is_empty() as u8,
        usernames.has_first_username() as u8
    )
    .into_bytes();
    if first.is_empty() {
        line.push(b'-');
    } else {
        line.extend_from_slice(&hex_encode(first));
    }
    line.extend_from_slice(
        format!(" haseditable={} editable=", usernames.has_editable_username() as u8).as_bytes(),
    );
    if editable.is_empty() {
        line.push(b'-');
    } else {
        line.extend_from_slice(&hex_encode(editable));
    }
    line.push(b'\n');
    out.write_all(&line).unwrap();
}

fn check_case(name: &str) -> i32 {
    if name == "reorder_missing" {
        let usernames = Usernames::new(Vec::new(), parse_items(b"a:1:1:0:0,b:1:0:0:0"));
        let mut out = std::io::stdout();
        let mut line = b"before=".to_vec();
        line.extend_from_slice(&usernames.render());
        line.push(b'\n');
        out.write_all(&line).unwrap();
        out.flush().unwrap();
        let _ = usernames.reorder_to(vec![b"b".to_vec()]);
        return 0;
    }
    2
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--check" {
        std::process::exit(check_case(&args[2]));
    }
    if args.len() != 2 {
        eprintln!("usage: usernames_probe <script.txt>");
        std::process::exit(2);
    }
    let script = std::fs::read(&args[1]).expect("cannot read script");
    let mut current = Usernames::default();
    let mut hex_mode = false;
    let mut out = std::io::stdout();
    for raw_line in script.split(|c| *c == b'\n') {
        let line: &[u8] = raw_line.trim_ascii();
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens = split_tokens(line);
        match tokens[0] {
            b"new" => {
                current = Usernames::new(parse_name(tokens[1]), parse_items(tokens[2]));
                print_state(&current, hex_mode, &mut out);
            }
            b"print" => print_state(&current, false, &mut out),
            b"printhex" => print_state(&current, true, &mut out),
            b"hexmode" => hex_mode = parse_flag(tokens[1]),
            b"acc" => print_accessors(&current, &mut out),
            b"canadd" => {
                out.write_all(
                    format!("canadd={}\n", current.can_add_secondary(&parse_name(tokens[1])) as u8).as_bytes(),
                )
                .unwrap();
            }
            b"candel" => {
                out.write_all(
                    format!("candel={}\n", current.can_delete_secondary(&parse_name(tokens[1])) as u8).as_bytes(),
                )
                .unwrap();
            }
            b"cantoggle" => {
                out.write_all(
                    format!(
                        "cantoggle={}\n",
                        current.can_toggle(parse_flag(tokens[1]), &parse_name(tokens[2])) as u8
                    )
                    .as_bytes(),
                )
                .unwrap();
            }
            b"canreorder" => {
                out.write_all(
                    format!("canreorder={}\n", current.can_reorder_to(&parse_csv(tokens[1])) as u8).as_bytes(),
                )
                .unwrap();
            }
            b"change" => {
                current = current.change_editable_username(parse_name(tokens[1]));
                print_state(&current, hex_mode, &mut out);
            }
            b"toggle" => {
                let for_bot = parse_flag(tokens[1]);
                let name = parse_name(tokens[2]);
                let is_active = parse_flag(tokens[3]);
                if !current.can_toggle(for_bot, &name) {
                    out.write_all(b"toggle=skip\n").unwrap();
                    continue;
                }
                current = current.toggle(for_bot, &name, is_active);
                print_state(&current, hex_mode, &mut out);
            }
            b"addsec" => {
                current = current.add_secondary(&parse_name(tokens[1]));
                print_state(&current, hex_mode, &mut out);
            }
            b"delsec" => {
                current = current.delete_secondary(&parse_name(tokens[1]));
                print_state(&current, hex_mode, &mut out);
            }
            b"deact" => {
                current = current.deactivate_all();
                print_state(&current, hex_mode, &mut out);
            }
            b"reorder" => {
                current = current.reorder_to(parse_csv(tokens[1]));
                print_state(&current, hex_mode, &mut out);
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}