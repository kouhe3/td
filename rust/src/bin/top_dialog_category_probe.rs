// (Unofficial) Rust side of the `top_dialog_category` lane; the oracle is
// `tools/fixtures/top_dialog_category_probe.cpp` over the real
// `td/telegram/TopDialogCategory.{h,cpp}` and
// `td/telegram/NotificationGroupType.{h,cpp}`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin top_dialog_category_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/top_dialog_category_probe.exe <script.txt>
//        ... --check tl_null | name_size | input_size | ngt_null |
//            ngt_db_oor | ngt_partial_oor | ngt_obj_oor | ngt_print_oor
//
// Rules this side reproduces (docs/facts.md F264):
//   * `Size` is 10; `name(Size)` and `input(Size)` are UNREACHABLE;
//   * the td_api side collapses `topChatCategoryForwardChats` (id 1695922133)
//     onto `ForwardUsers`(6), so `ForwardChats`(7) is reachable only from the
//     wire side `topPeerCategoryForwardChats`;
//   * a null td_api category is `Size`, a null wire category is a CHECK;
//   * `is_database_notification_group_type` and
//     `is_partial_notification_group_type` differ only on `SecretChat`, and an
//     out-of-range value hits `UNREACHABLE` in all four group-type functions.

use std::io::Write;

#[allow(dead_code)]
#[path = "../script_tokens.rs"]
mod script_tokens;

use script_tokens::{split_tokens, trim_line};

/// `td::TopDialogCategory`; `Size` is the end marker and is not a category.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    Correspondent = 0,
    BotPm = 1,
    BotInline = 2,
    Group = 3,
    Channel = 4,
    Call = 5,
    ForwardUsers = 6,
    ForwardChats = 7,
    BotApp = 8,
    BotGuest = 9,
    Size = 10,
}

impl Category {
    fn value(self) -> i32 {
        self as i32
    }

    fn from_int(value: i32) -> Category {
        match value {
            0 => Category::Correspondent,
            1 => Category::BotPm,
            2 => Category::BotInline,
            3 => Category::Group,
            4 => Category::Channel,
            5 => Category::Call,
            6 => Category::ForwardUsers,
            7 => Category::ForwardChats,
            8 => Category::BotApp,
            9 => Category::BotGuest,
            10 => Category::Size,
            other => panic!("TopDialogCategory: bad value {}", other),
        }
    }

    /// `get_top_dialog_category_name`: `Size` is UNREACHABLE.
    fn name(self) -> &'static str {
        match self {
            Category::Correspondent => "correspondent",
            Category::BotPm => "bot_pm",
            Category::BotInline => "bot_inline",
            Category::Group => "group",
            Category::Channel => "channel",
            Category::Call => "call",
            Category::ForwardUsers => "forward_users",
            Category::ForwardChats => "forward_chats",
            Category::BotApp => "bot_app",
            Category::BotGuest => "bot_guest",
            Category::Size => panic!("get_top_dialog_category_name(Size): UNREACHABLE"),
        }
    }

    /// `get_top_dialog_category(td_api)`: `topChatCategoryForwardChats` lands on
    /// `ForwardUsers`, so `ForwardChats` never comes out of this direction.
    fn from_td_api(token: &[u8]) -> (i32, Category) {
        match token {
            b"users" => (1026706816, Category::Correspondent),
            b"bots" => (-1577129195, Category::BotPm),
            b"botsinline" => (377023356, Category::BotInline),
            b"groups" => (1530056846, Category::Group),
            b"channels" => (-500825885, Category::Channel),
            b"calls" => (356208861, Category::Call),
            b"forwardchats" => (1695922133, Category::ForwardUsers),
            b"webappbots" => (100062973, Category::BotApp),
            b"guestbots" => (-1792882579, Category::BotGuest),
            other => panic!("bad td_api category name {}", String::from_utf8_lossy(other)),
        }
    }

    /// `get_top_dialog_category(telegram_api)`.
    fn from_wire(token: &[u8]) -> Category {
        match token {
            b"correspondents" => Category::Correspondent,
            b"botspm" => Category::BotPm,
            b"botsinline" => Category::BotInline,
            b"groups" => Category::Group,
            b"channels" => Category::Channel,
            b"phonecalls" => Category::Call,
            b"forwardusers" => Category::ForwardUsers,
            b"forwardchats" => Category::ForwardChats,
            b"botsapp" => Category::BotApp,
            b"botsguest" => Category::BotGuest,
            other => panic!("bad wire category name {}", String::from_utf8_lossy(other)),
        }
    }

    /// `get_input_top_peer_category`: `Size` is UNREACHABLE.
    fn wire_id(self) -> i32 {
        match self {
            Category::Correspondent => 104314861,
            Category::BotPm => -1419371685,
            Category::BotInline => 344356834,
            Category::Group => -1122524854,
            Category::Channel => 371037736,
            Category::Call => 511092620,
            Category::ForwardUsers => -1472172887,
            Category::ForwardChats => -68239120,
            Category::BotApp => -39945236,
            Category::BotGuest => 1814361053,
            Category::Size => panic!("get_input_top_peer_category(Size): UNREACHABLE"),
        }
    }
}

/// `td::NotificationGroupType`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupType {
    Messages = 0,
    Mentions = 1,
    SecretChat = 2,
    Calls = 3,
}

impl GroupType {
    fn value(self) -> i32 {
        self as i32
    }

    fn from_int(value: i32) -> GroupType {
        match value {
            0 => GroupType::Messages,
            1 => GroupType::Mentions,
            2 => GroupType::SecretChat,
            3 => GroupType::Calls,
            other => panic!("NotificationGroupType: bad value {}", other),
        }
    }

    /// `is_database_notification_group_type`: Calls is not stored.
    fn is_database(self) -> bool {
        match self {
            GroupType::Messages | GroupType::Mentions | GroupType::SecretChat => true,
            GroupType::Calls => false,
        }
    }

    /// `is_partial_notification_group_type`: only Messages and Mentions.
    fn is_partial(self) -> bool {
        match self {
            GroupType::Messages | GroupType::Mentions => true,
            GroupType::SecretChat | GroupType::Calls => false,
        }
    }

    /// `get_notification_group_type_object().get_id()`.
    fn object_id(self) -> i32 {
        match self {
            GroupType::Messages => -1702481123,
            GroupType::Mentions => -2050324051,
            GroupType::SecretChat => 1390759476,
            GroupType::Calls => 1379123538,
        }
    }

    /// `get_notification_group_type(td_api object)`: dispatch on the object id.
    fn from_object_id(id: i32) -> GroupType {
        match id {
            -1702481123 => GroupType::Messages,
            -2050324051 => GroupType::Mentions,
            1390759476 => GroupType::SecretChat,
            1379123538 => GroupType::Calls,
            other => panic!("get_notification_group_type: unknown object id {}", other),
        }
    }

    /// `operator<<`: an out-of-range value is UNREACHABLE.
    fn print(self) -> &'static str {
        match self {
            GroupType::Messages => "Messages",
            GroupType::Mentions => "Mentions",
            GroupType::SecretChat => "SecretChat",
            GroupType::Calls => "Calls",
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    if args.len() == 3 && args[1] == "--check" {
        let _ = writeln!(out, "before");
        out.flush().unwrap();
        match args[2].as_str() {
            "tl_null" => panic!("get_top_dialog_category(wire nullptr): CHECK"),
            "name_size" => {
                let _ = writeln!(out, "after={}", Category::Size.name());
            }
            "input_size" => {
                let _ = writeln!(out, "after={}", Category::Size.wire_id());
            }
            "ngt_null" => panic!("get_notification_group_type(td_api nullptr): CHECK"),
            "ngt_db_oor" => {
                let _ = writeln!(out, "after={}", GroupType::from_int(7).is_database() as u8);
            }
            "ngt_partial_oor" => {
                let _ = writeln!(out, "after={}", GroupType::from_int(7).is_partial() as u8);
            }
            "ngt_obj_oor" => {
                let _ = writeln!(out, "after={}", GroupType::from_int(7).object_id());
            }
            "ngt_print_oor" => {
                let _ = writeln!(out, "after={}", GroupType::from_int(7).print());
            }
            other => {
                eprintln!("unknown check case {}", other);
                std::process::exit(2);
            }
        }
        out.flush().unwrap();
        return;
    }
    if args.len() != 2 {
        eprintln!("usage: top_dialog_category_probe <script.txt>");
        std::process::exit(2);
    }

    let script = std::fs::read(&args[1]).expect("cannot read script");
    for raw_line in script.split(|c| *c == b'\n') {
        let line = trim_line(raw_line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens = split_tokens(line);
        let text = |token: &[u8]| std::str::from_utf8(token).unwrap().to_string();
        match tokens[0] {
            b"cat" => {
                let (id, category) = Category::from_td_api(tokens[1]);
                let _ = writeln!(
                    out,
                    "cat={} id={} enum={} name={}",
                    text(tokens[1]),
                    id,
                    category.value(),
                    category.name()
                );
            }
            b"catnull" => {
                // A null `TopChatCategory` is `Size`, not an abort.
                let _ = writeln!(out, "catnull=enum={}", Category::Size.value());
            }
            b"cattl" => {
                let category = Category::from_wire(tokens[1]);
                let _ = writeln!(
                    out,
                    "cattl={} enum={} name={}",
                    text(tokens[1]),
                    category.value(),
                    category.name()
                );
            }
            b"name" => {
                let value: i32 = text(tokens[1]).parse().unwrap();
                let _ = writeln!(out, "name={} print={}", value, Category::from_int(value).name());
            }
            b"input" => {
                let value: i32 = text(tokens[1]).parse().unwrap();
                let _ = writeln!(out, "input={} id={}", value, Category::from_int(value).wire_id());
            }
            b"ngt" => {
                let value: i32 = text(tokens[1]).parse().unwrap();
                let group_type = GroupType::from_int(value);
                let _ = writeln!(
                    out,
                    "ngt={} db={} partial={} obj={} print={}",
                    value,
                    group_type.is_database() as u8,
                    group_type.is_partial() as u8,
                    group_type.object_id(),
                    group_type.print()
                );
            }
            b"ngtfrom" => {
                let value: i32 = text(tokens[1]).parse().unwrap();
                let group_type = GroupType::from_int(value);
                let _ = writeln!(
                    out,
                    "ngtfrom={} obj={} back={}",
                    value,
                    group_type.object_id(),
                    GroupType::from_object_id(group_type.object_id()).value()
                );
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}