// (Unofficial) Rust side of the `DialogId` lane; the oracle is
// `tools/fixtures/dialog_id_probe.cpp` over the real `td::DialogId`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin dialog_id_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/dialog_id_probe.exe <script.txt>
//
// Same script language and same stdout bytes as the C++ probe; `--check <case>`
// exists only so the harness can compare "did the process abort".

use std::io::Write;

const ZERO_SECRET_CHAT_ID: i64 = -2000000000000;
const ZERO_CHANNEL_ID: i64 = -1000000000000;
const MAX_USER_ID: i64 = (1i64 << 40) - 1;
const MAX_CHAT_ID: i64 = 999999999999;
const MAX_CHANNEL_ID: i64 = 1000000000000 - (1i64 << 31);
const MIN_MONOFORUM_CHANNEL_ID: i64 = 1000000000000 + (1i64 << 31) + 1;
const MAX_MONOFORUM_CHANNEL_ID: i64 = 3000000000000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DialogType {
    None,
    User,
    Chat,
    Channel,
    SecretChat,
}

#[derive(Clone, Copy, Default)]
struct DialogId {
    id: i64,
}

impl DialogId {
    fn from_int64(id: i64) -> DialogId {
        DialogId { id }
    }

    fn from_user(id: i64) -> DialogId {
        if 0 < id && id <= MAX_USER_ID {
            DialogId { id }
        } else {
            DialogId { id: 0 }
        }
    }

    fn from_chat(id: i64) -> DialogId {
        if 0 < id && id <= MAX_CHAT_ID {
            DialogId { id: -id }
        } else {
            DialogId { id: 0 }
        }
    }

    fn from_channel(id: i64) -> DialogId {
        let is_regular = 0 < id && id < MAX_CHANNEL_ID;
        let is_monoforum = MIN_MONOFORUM_CHANNEL_ID <= id && id < MAX_MONOFORUM_CHANNEL_ID;
        if is_regular || is_monoforum {
            DialogId { id: ZERO_CHANNEL_ID - id }
        } else {
            DialogId { id: 0 }
        }
    }

    fn from_secret(id: i32) -> DialogId {
        if id != 0 {
            DialogId { id: ZERO_SECRET_CHAT_ID + id as i64 }
        } else {
            DialogId { id: 0 }
        }
    }

    fn get_type(&self) -> DialogType {
        let id = self.id;
        if id < 0 {
            if -MAX_CHAT_ID <= id {
                return DialogType::Chat;
            }
            if ZERO_CHANNEL_ID - MAX_CHANNEL_ID <= id && id != ZERO_CHANNEL_ID {
                return DialogType::Channel;
            }
            if ZERO_SECRET_CHAT_ID + i32::MIN as i64 <= id && id != ZERO_SECRET_CHAT_ID {
                return DialogType::SecretChat;
            }
            if ZERO_CHANNEL_ID - MAX_MONOFORUM_CHANNEL_ID <= id {
                return DialogType::Channel;
            }
        } else if 0 < id && id <= MAX_USER_ID {
            return DialogType::User;
        }
        DialogType::None
    }

    fn is_valid(&self) -> bool {
        self.get_type() != DialogType::None
    }

    fn get_user_id(&self) -> i64 {
        assert!(self.get_type() == DialogType::User);
        self.id
    }

    fn get_chat_id(&self) -> i64 {
        assert!(self.get_type() == DialogType::Chat);
        -self.id
    }

    fn get_channel_id(&self) -> i64 {
        assert!(self.get_type() == DialogType::Channel);
        ZERO_CHANNEL_ID - self.id
    }

    fn get_secret_chat_id(&self) -> i32 {
        assert!(self.get_type() == DialogType::SecretChat);
        (self.id - ZERO_SECRET_CHAT_ID) as i32
    }

    fn render(&self) -> String {
        format!("chat {}", self.id)
    }
}

fn type_name(t: DialogType) -> &'static str {
    match t {
        DialogType::None => "None",
        DialogType::User => "User",
        DialogType::Chat => "Chat",
        DialogType::Channel => "Channel",
        DialogType::SecretChat => "SecretChat",
    }
}

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

fn print_list(tag: &str, ids: &[DialogId], out: &mut std::io::Stdout) {
    let mut line = format!("{tag} n={}", ids.len());
    for id in ids {
        line.push_str(&format!(" [{}]", id.render()));
    }
    let _ = writeln!(out, "{line}");
}

fn check_case(name: &str) -> i32 {
    let mut out = std::io::stdout();
    let (dialog, which) = match name {
        "uid_on_chat" => (DialogId::from_int64(-5), "uid"),
        "cid_on_channel" => (DialogId::from_int64(-1000000000005), "cid"),
        "chid_on_user" => (DialogId::from_int64(42), "chid"),
        "sid_on_chat" => (DialogId::from_int64(-5), "sid"),
        _ => return 2,
    };
    let _ = writeln!(out, "before={}", dialog.render());
    out.flush().unwrap();
    let value: i64 = match which {
        "uid" => dialog.get_user_id(),
        "cid" => dialog.get_chat_id(),
        "chid" => dialog.get_channel_id(),
        _ => dialog.get_secret_chat_id() as i64,
    };
    let _ = writeln!(out, "value={value}");
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--check" {
        std::process::exit(check_case(&args[2]));
    }
    if args.len() != 2 {
        eprintln!("usage: dialog_id_probe <script.txt>");
        std::process::exit(2);
    }
    let script = std::fs::read(&args[1]).expect("cannot read script");
    let mut current = DialogId::default();
    let mut out = std::io::stdout();
    for raw_line in script.split(|c| *c == b'\n') {
        let line: &[u8] = raw_line.trim_ascii();
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens: Vec<&[u8]> = line.split(|c| *c == b' ').filter(|t| !t.is_empty()).collect();
        match tokens[0] {
            b"raw" => current = DialogId::from_int64(parse_i64(tokens[1])),
            b"user" => current = DialogId::from_user(parse_i64(tokens[1])),
            b"chat" => current = DialogId::from_chat(parse_i64(tokens[1])),
            b"channel" => current = DialogId::from_channel(parse_i64(tokens[1])),
            b"secret" => current = DialogId::from_secret(parse_i64(tokens[1]) as i32),
            b"dump" => {
                let _ = writeln!(
                    out,
                    "dump raw={} print={} valid={} type={}",
                    current.id,
                    current.render(),
                    current.is_valid() as u8,
                    type_name(current.get_type())
                );
            }
            b"uid" => {
                if current.get_type() == DialogType::User {
                    let _ = writeln!(out, "uid={}", current.get_user_id());
                } else {
                    let _ = writeln!(out, "uid=n/a");
                }
            }
            b"cid" => {
                if current.get_type() == DialogType::Chat {
                    let _ = writeln!(out, "cid={}", current.get_chat_id());
                } else {
                    let _ = writeln!(out, "cid=n/a");
                }
            }
            b"chid" => {
                if current.get_type() == DialogType::Channel {
                    let _ = writeln!(out, "chid={}", current.get_channel_id());
                } else {
                    let _ = writeln!(out, "chid=n/a");
                }
            }
            b"sid" => {
                if current.get_type() == DialogType::SecretChat {
                    let _ = writeln!(out, "sid={}", current.get_secret_chat_id());
                } else {
                    let _ = writeln!(out, "sid=n/a");
                }
            }
            b"ids" => {
                let ids: Vec<DialogId> =
                    parse_i64_csv(tokens[1]).into_iter().map(DialogId::from_int64).collect();
                print_list("ids", &ids, &mut out);
            }
            b"usrs" | b"chats" | b"chans" | b"secs" => {
                let command = tokens[0];
                let ids: Vec<DialogId> = parse_i64_csv(tokens[1])
                    .into_iter()
                    .map(|value| match command {
                        b"usrs" => DialogId::from_user(value),
                        b"chats" => DialogId::from_chat(value),
                        b"chans" => DialogId::from_channel(value),
                        _ => DialogId::from_secret(value as i32),
                    })
                    .collect();
                let tag = std::str::from_utf8(command).unwrap();
                print_list(tag, &ids, &mut out);
            }
            b"rmsecret" => {
                let ids: Vec<DialogId> =
                    parse_i64_csv(tokens[1]).into_iter().map(DialogId::from_int64).collect();
                let kept: Vec<DialogId> = ids
                    .into_iter()
                    .filter(|id| id.get_type() != DialogType::SecretChat)
                    .collect();
                print_list("rmsecret", &kept, &mut out);
            }
            b"cmp" => {
                let lhs = DialogId::from_int64(parse_i64(tokens[1]));
                let rhs = DialogId::from_int64(parse_i64(tokens[2]));
                let _ = writeln!(out, "cmp eq={} ne={}", (lhs.id == rhs.id) as u8, (lhs.id != rhs.id) as u8);
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}