// (Unofficial) Rust side of the `topic_id` lane; the oracle is
// `tools/fixtures/topic_id_probe.cpp` over the real td::ForumTopicId and
// td::SavedMessagesTopicId.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin topic_id_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/topic_id_probe.exe <script.txt>

use std::io::Write;

const SERVER_ID_SHIFT: i64 = 20;
const FULL_TYPE_MASK: i64 = (1 << SERVER_ID_SHIFT) - 1;
const ZERO_CHANNEL_ID: i64 = -1000000000000;
const ZERO_SECRET_CHAT_ID: i64 = -2000000000000;
const MAX_USER_ID: i64 = (1i64 << 40) - 1;
const MAX_CHAT_ID: i64 = 999999999999;
const MAX_CHANNEL_ID: i64 = 1000000000000 - (1i64 << 31);
const MAX_MONOFORUM_CHANNEL_ID: i64 = 3000000000000;
const HIDDEN_AUTHOR_DIALOG_ID: i64 = 2666000;

#[derive(Clone, Copy, Default)]
struct MessageId {
    id: i64,
}

impl MessageId {
    fn is_server(&self) -> bool {
        (self.id & FULL_TYPE_MASK) == 0 && self.id > 0 && self.id <= ((i32::MAX as i64) << SERVER_ID_SHIFT)
    }

    fn get_server_message_id_force(&self) -> i32 {
        assert!(!self.is_scheduled());
        (self.id >> SERVER_ID_SHIFT) as i32
    }

    fn is_scheduled(&self) -> bool {
        (self.id & 4) != 0
    }

    fn from_server(id: i32) -> MessageId {
        MessageId { id: (id as i64) << SERVER_ID_SHIFT }
    }

    fn render(&self) -> String {
        if self.id <= 0 || self.id > ((i32::MAX as i64) << SERVER_ID_SHIFT) {
            return format!("invalid message {}", self.id);
        }
        if (self.id & FULL_TYPE_MASK) == 0 {
            return format!("server message {}", self.id >> SERVER_ID_SHIFT);
        }
        let ty = self.id & 7;
        if ty == 1 {
            return format!("yet unsent message {}.{}", self.id >> SERVER_ID_SHIFT, self.id & FULL_TYPE_MASK);
        }
        if ty == 2 {
            return format!("local message {}.{}", self.id >> SERVER_ID_SHIFT, self.id & FULL_TYPE_MASK);
        }
        format!("bugged message {}", self.id)
    }
}

#[derive(Clone, Copy, Default)]
struct ForumTopicId {
    id: i32,
}

impl ForumTopicId {
    fn is_valid(&self) -> bool {
        self.id > 0
    }

    fn from_top_thread_message_id(message_id: MessageId) -> ForumTopicId {
        assert!(message_id.id == 0 || message_id.is_server());
        ForumTopicId { id: message_id.get_server_message_id_force() }
    }

    fn to_top_thread_message_id(&self) -> MessageId {
        MessageId::from_server(self.id)
    }

    fn render(&self) -> String {
        format!("topic {}", self.id)
    }
}

#[derive(Clone, Copy, Default)]
struct DialogId {
    id: i64,
}

impl DialogId {
    fn get_type(&self) -> i32 {
        let id = self.id;
        if id < 0 {
            if -MAX_CHAT_ID <= id {
                return 1; // Chat
            }
            if ZERO_CHANNEL_ID - MAX_CHANNEL_ID <= id && id != ZERO_CHANNEL_ID {
                return 2; // Channel
            }
            if ZERO_SECRET_CHAT_ID + i32::MIN as i64 <= id && id != ZERO_SECRET_CHAT_ID {
                return 3; // SecretChat
            }
            if ZERO_CHANNEL_ID - MAX_MONOFORUM_CHANNEL_ID <= id {
                return 2; // Channel
            }
        } else if 0 < id && id <= MAX_USER_ID {
            return 4; // User
        }
        0
    }

    fn is_valid(&self) -> bool {
        self.get_type() != 0
    }

    fn render(&self) -> String {
        format!("chat {}", self.id)
    }
}

#[derive(Clone, Copy, Default)]
struct SavedMessagesTopicId {
    dialog: DialogId,
}

impl SavedMessagesTopicId {
    fn is_valid(&self) -> bool {
        self.dialog.is_valid()
    }

    fn is_author_hidden(&self) -> bool {
        self.dialog.id == HIDDEN_AUTHOR_DIALOG_ID
    }

    fn render(&self) -> String {
        if !self.is_valid() {
            return "[no topic]".to_string();
        }
        if self.is_author_hidden() {
            return "[Author Hidden topic]".to_string();
        }
        format!("[topic of {}]", self.dialog.render())
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: topic_id_probe <script.txt>");
        std::process::exit(2);
    }
    let script = std::fs::read(&args[1]).expect("cannot read script");
    let mut out = std::io::stdout();
    for raw_line in script.split(|c| *c == b'\n') {
        let line: &[u8] = raw_line.trim_ascii();
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens: Vec<&[u8]> = line.split(|c| *c == b' ').filter(|t| !t.is_empty()).collect();
        match tokens[0] {
            b"forum" => {
                let topic = ForumTopicId { id: parse_i64(tokens[1]) as i32 };
                let _ = writeln!(out, "forum raw={} valid={} print={}", topic.id, topic.is_valid() as u8, topic.render());
            }
            b"fids" => {
                let ids: Vec<ForumTopicId> = parse_i64_csv(tokens[1])
                    .into_iter()
                    .map(|value| ForumTopicId { id: value as i32 })
                    .collect();
                let mut line = format!("fids n={}", ids.len());
                for id in &ids {
                    line.push_str(&format!(" [{}]", id.render()));
                }
                let _ = writeln!(out, "{line}");
            }
            b"fmsgs" => {
                let ids: Vec<ForumTopicId> = parse_i64_csv(tokens[1])
                    .into_iter()
                    .map(|value| ForumTopicId { id: value as i32 })
                    .collect();
                let mut line = format!("fmsgs n={}", ids.len());
                for id in &ids {
                    line.push_str(&format!(" {}", id.id));
                }
                let _ = writeln!(out, "{line}");
            }
            b"frommsg" | b"frommsg_force" => {
                let message_id = MessageId { id: parse_i64(tokens[1]) };
                if tokens[0] == b"frommsg" && !message_id.is_server() {
                    let _ = writeln!(out, "frommsg=n/a");
                    continue;
                }
                let topic = ForumTopicId::from_top_thread_message_id(message_id);
                let _ = writeln!(
                    out,
                    "frommsg raw={} top={} print={}",
                    message_id.id,
                    topic.to_top_thread_message_id().render(),
                    topic.render()
                );
            }
            b"saved" => {
                let dialog = DialogId { id: parse_i64(tokens[1]) };
                let saved = SavedMessagesTopicId { dialog };
                let _ = writeln!(
                    out,
                    "saved dialog={} valid={} unique={} hidden={} print={}",
                    dialog.id,
                    saved.is_valid() as u8,
                    saved.dialog.id,
                    saved.is_author_hidden() as u8,
                    saved.render()
                );
            }
            b"savedeq" => {
                let lhs = SavedMessagesTopicId { dialog: DialogId { id: parse_i64(tokens[1]) } };
                let rhs = SavedMessagesTopicId { dialog: DialogId { id: parse_i64(tokens[2]) } };
                let _ = writeln!(
                    out,
                    "savedeq eq={} ne={}",
                    (lhs.dialog.id == rhs.dialog.id) as u8,
                    (lhs.dialog.id != rhs.dialog.id) as u8
                );
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}