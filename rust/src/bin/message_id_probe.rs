// (Unofficial) Rust side of the `MessageId` lane; the oracle is
// `tools/fixtures/message_id_probe.cpp` over the real `td::MessageId`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin message_id_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/message_id_probe.exe <script.txt>
//
// Same script language and same stdout bytes as the C++ probe; `--check <case>`
// exists only so the harness can compare "did the process abort".
//
// The model lives in `telegram_message_id.rs` because the `td_api_objects` lane
// drives `MessageId::get_message_id(telegram_api::Message*)` on top of it.

use std::io::Write;

#[allow(dead_code)]
#[path = "../telegram_message_id.rs"]
mod telegram_message_id;

use telegram_message_id::*;

fn parse_i64(token: &[u8]) -> i64 {
    let mut value: i64 = 0;
    let mut negative = false;
    let mut digits = token;
    if digits.first() == Some(&b'-') {
        negative = true;
        digits = &digits[1..];
    }
    assert!(!digits.is_empty(), "bad integer");
    for c in digits {
        assert!(c.is_ascii_digit(), "bad integer");
        value = value * 10 + (c - b'0') as i64;
    }
    if negative {
        -value
    } else {
        value
    }
}

fn parse_i64_csv(token: &[u8]) -> Vec<i64> {
    if token == b"-" {
        return Vec::new();
    }
    let mut parts: Vec<&[u8]> = token.split(|c| *c == b',').collect();
    if token.last() == Some(&b',') {
        parts.pop();
    }
    parts.into_iter().map(parse_i64).collect()
}

fn type_name(ty: MessageType) -> &'static str {
    match ty {
        MessageType::None => "None",
        MessageType::Server => "Server",
        MessageType::YetUnsent => "YetUnsent",
        MessageType::Local => "Local",
    }
}

fn dump(message_id: MessageId) {
    let (yet, local) = if message_id.is_valid() || message_id.is_scheduled() {
        (
            if message_id.is_yet_unsent() { "1" } else { "0" },
            if message_id.is_local() { "1" } else { "0" },
        )
    } else {
        ("n/a", "n/a")
    };
    let scheduled_server = if message_id.is_valid_scheduled() {
        if message_id.is_scheduled_server() { "1" } else { "0" }
    } else {
        "n/a"
    };
    let any = if !message_id.is_scheduled() || message_id.is_valid_scheduled() {
        if message_id.is_any_server() { "1" } else { "0" }
    } else {
        "n/a"
    };
    println!(
        "dump raw={} print={} valid={} vsched={} vspon={} type={} sched={} server={} ssrv={} local={} yet={} any={}",
        message_id.id,
        message_id.render(),
        message_id.is_valid() as u8,
        message_id.is_valid_scheduled() as u8,
        message_id.is_valid_sponsored() as u8,
        type_name(message_id.get_type()),
        message_id.is_scheduled() as u8,
        message_id.is_server() as u8,
        scheduled_server,
        local,
        yet,
        any
    );
}

fn check_case(name: &str) -> i32 {
    let mut out = std::io::stdout();
    if name == "date_ordinary" {
        let value = MessageId::from_int64(1000);
        let _ = writeln!(out, "before={}", value.render());
        out.flush().unwrap();
        let _ = value.get_scheduled_message_date();
        return 0;
    }
    if name == "ssrv_ordinary" {
        let value = MessageId::from_int64(1000);
        let _ = writeln!(out, "before={}", value.render());
        out.flush().unwrap();
        let _ = value.get_scheduled_server_message_id_force();
        return 0;
    }
    if name == "cmp_mixed" {
        let ordinary = MessageId::from_int64(1000);
        let scheduled = MessageId::from_scheduled(5, (1 << 30) + 10, false);
        let _ = writeln!(out, "before={} {}", ordinary.render(), scheduled.render());
        out.flush().unwrap();
        assert_eq!(ordinary.is_scheduled(), scheduled.is_scheduled());
        let _ = ordinary.id < scheduled.id;
        return 0;
    }
    if name == "next_none" {
        let value = MessageId::from_int64(1000);
        let _ = writeln!(out, "before={}", value.render());
        out.flush().unwrap();
        let next = value.get_next_message_id(MessageType::None);
        let _ = writeln!(out, "next={}", next.render());
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
        eprintln!("usage: message_id_probe <script.txt>");
        std::process::exit(2);
    }
    let script = std::fs::read(&args[1]).expect("cannot read script");
    let mut current = MessageId::default();
    let mut out = std::io::stdout();
    for raw_line in script.split(|c| *c == b'\n') {
        let line: &[u8] = raw_line.trim_ascii();
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens: Vec<&[u8]> = line.split(|c| *c == b' ').filter(|t| !t.is_empty()).collect();
        match tokens[0] {
            b"raw" => current = MessageId::from_int64(parse_i64(tokens[1])),
            b"srv" => current = MessageId::from_server(parse_i64(tokens[1]) as i32),
            b"sched" => {
                let force = tokens.len() > 3 && tokens[3] == b"1";
                current = MessageId::from_scheduled(parse_i64(tokens[1]) as i32, parse_i64(tokens[2]) as i32, force);
            }
            b"dump" => dump(current),
            b"prev" => {
                if current.is_scheduled() {
                    let _ = writeln!(out, "prev=n/a");
                } else {
                    let _ = writeln!(out, "prev={}", current.get_prev_server_message_id().id);
                }
            }
            b"nextsrv" => {
                if current.is_scheduled() {
                    let _ = writeln!(out, "nextsrv=n/a");
                } else {
                    let _ = writeln!(out, "nextsrv={}", current.get_next_server_message_id().id);
                }
            }
            b"next" => {
                let ty = match parse_i64(tokens[1]) {
                    0 => MessageType::None,
                    1 => MessageType::Server,
                    2 => MessageType::YetUnsent,
                    _ => MessageType::Local,
                };
                if ty == MessageType::None {
                    let _ = writeln!(out, "next=n/a");
                } else if current.is_scheduled() && !current.is_valid_scheduled() {
                    let _ = writeln!(out, "next=n/a");
                } else {
                    let _ = writeln!(out, "next={}", current.get_next_message_id(ty).render());
                }
            }
            b"date" => {
                if current.is_valid_scheduled() {
                    let _ = writeln!(out, "date={}", current.get_scheduled_message_date());
                } else {
                    let _ = writeln!(out, "date=n/a");
                }
            }
            b"srvget" => {
                if current.is_server() {
                    let _ = writeln!(out, "srv={}", current.get_server_message_id_force());
                } else {
                    let _ = writeln!(out, "srv=n/a");
                }
            }
            b"ssrv" => {
                if current.is_valid_scheduled() && current.is_scheduled_server() {
                    let _ = writeln!(out, "ssrv={}", current.get_scheduled_server_message_id_force());
                } else {
                    let _ = writeln!(out, "ssrv=n/a");
                }
            }
            b"cmp" => {
                let lhs = MessageId::from_int64(parse_i64(tokens[1]));
                let rhs = MessageId::from_int64(parse_i64(tokens[2]));
                if lhs.is_scheduled() != rhs.is_scheduled() {
                    let _ = writeln!(out, "cmp=n/a");
                } else {
                    let _ = writeln!(
                        out,
                        "cmp lt={} gt={} le={} ge={} eq={}",
                        (lhs.id < rhs.id) as u8,
                        (lhs.id > rhs.id) as u8,
                        (lhs.id <= rhs.id) as u8,
                        (lhs.id >= rhs.id) as u8,
                        (lhs.id == rhs.id) as u8
                    );
                }
            }
            b"ids" => {
                let ids: Vec<MessageId> =
                    parse_i64_csv(tokens[1]).into_iter().map(MessageId::from_int64).collect();
                let mut line = format!("ids n={}", ids.len());
                for id in &ids {
                    line.push_str(&format!(" [{}]", id.render()));
                }
                let _ = writeln!(out, "{line}");
            }
            b"srvids" => {
                let ids: Vec<MessageId> =
                    parse_i64_csv(tokens[1]).into_iter().map(MessageId::from_int64).collect();
                let mut line = format!("srvids n={}", ids.len());
                for id in &ids {
                    line.push_str(&format!(" {}", id.get_server_message_id_force()));
                }
                let _ = writeln!(out, "{line}");
            }
            b"ssrvids" => {
                let ids: Vec<MessageId> =
                    parse_i64_csv(tokens[1]).into_iter().map(MessageId::from_int64).collect();
                let mut line = format!("ssrvids n={}", ids.len());
                for id in &ids {
                    line.push_str(&format!(" {}", id.get_scheduled_server_message_id()));
                }
                let _ = writeln!(out, "{line}");
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}