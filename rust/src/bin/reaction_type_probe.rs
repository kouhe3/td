//! Judge `td::ReactionType` (td/telegram/ReactionType.{h,cpp}) against the C++
//! oracle (`tools/fixtures/reaction_type_probe.cpp`).
//!
//!   reaction_type_probe <script.txt>
//!   reaction_type_probe --check <case>
//!
//! Specs: `-` (empty), `$` (paid), `e:<hex>`, `c:<id>`, `tl:empty`,
//! `tl:emoji:<hex>`, `tl:custom:<id>`, `tl:paid`, `td:null`, `td:emoji:<hex>`,
//! `td:custom:<id>`, `td:paid`.

#[allow(dead_code)]
#[path = "../emoji_table.rs"]
mod emoji_table;
#[allow(dead_code)]
#[path = "../emoji.rs"]
mod emoji;
#[allow(dead_code)]
#[path = "../md5.rs"]
mod md5;
#[allow(dead_code)]
#[path = "../telegram_reaction_type.rs"]
mod telegram_reaction_type;

use std::io::BufRead;

use telegram_reaction_type::api_ids;
use telegram_reaction_type::{
    reaction_types_hash, reaction_types_object, InputReaction, ReactionType, TdReaction, TdReactionInput,
    TelegramReaction,
};

fn parse_i64(text: &str) -> Option<i64> {
    text.parse::<i64>().ok()
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    if text == "-" {
        return Some(Vec::new());
    }
    if text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for index in (0..bytes.len()).step_by(2) {
        let nibble = |c: u8| -> Option<u8> {
            match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                _ => None,
            }
        };
        out.push((nibble(bytes[index])? << 4) | nibble(bytes[index + 1])?);
    }
    Some(out)
}

fn to_hex(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn make(spec: &str) -> Option<ReactionType> {
    match spec {
        "-" => Some(ReactionType::default()),
        "$" => Some(ReactionType::paid()),
        "tl:empty" => Some(ReactionType::from_telegram(TelegramReaction::Empty)),
        "tl:paid" => Some(ReactionType::from_telegram(TelegramReaction::Paid)),
        "td:null" => Some(ReactionType::from_td(TdReactionInput::Null)),
        "td:paid" => Some(ReactionType::from_td(TdReactionInput::Paid)),
        _ => {
            if let Some(rest) = spec.strip_prefix("e:") {
                return Some(ReactionType::from_string(String::from_utf8_lossy(&from_hex(rest)?).into_owned()));
            }
            if let Some(rest) = spec.strip_prefix("c:") {
                return Some(ReactionType::from_td(TdReactionInput::Custom(parse_i64(rest)?)));
            }
            if let Some(rest) = spec.strip_prefix("tl:emoji:") {
                return Some(ReactionType::from_telegram(TelegramReaction::Emoji(from_hex(rest)?)));
            }
            if let Some(rest) = spec.strip_prefix("tl:custom:") {
                return Some(ReactionType::from_telegram(TelegramReaction::Custom(parse_i64(rest)?)));
            }
            if let Some(rest) = spec.strip_prefix("td:emoji:") {
                return Some(ReactionType::from_td(TdReactionInput::Emoji(from_hex(rest)?)));
            }
            if let Some(rest) = spec.strip_prefix("td:custom:") {
                return Some(ReactionType::from_td(TdReactionInput::Custom(parse_i64(rest)?)));
            }
            None
        }
    }
}

fn input_line(reaction: &ReactionType) -> String {
    match reaction.input_reaction() {
        InputReaction::Empty => format!("id={} empty", api_ids::REACTION_EMPTY),
        InputReaction::Emoji(emoji) => format!(
            "id={} emoji emoji={}",
            api_ids::REACTION_EMOJI,
            to_hex(emoji.as_bytes())
        ),
        InputReaction::Custom(document_id) => format!(
            "id={} custom doc={}",
            api_ids::REACTION_CUSTOM_EMOJI,
            document_id
        ),
        InputReaction::Paid => format!("id={} paid", api_ids::REACTION_PAID),
    }
}

fn object_line(reaction: &ReactionType) -> String {
    match reaction.reaction_type_object() {
        None => "null".to_string(),
        Some(TdReaction::Emoji(emoji)) => format!(
            "{} emoji emoji={}",
            api_ids::REACTION_TYPE_EMOJI,
            to_hex(emoji.as_bytes())
        ),
        Some(TdReaction::Custom(custom_emoji_id)) => format!(
            "{} custom doc={}",
            api_ids::REACTION_TYPE_CUSTOM_EMOJI,
            custom_emoji_id
        ),
        Some(TdReaction::Paid) => format!("{} paid", api_ids::REACTION_TYPE_PAID),
    }
}

fn td_reaction_name(id: i32) -> &'static str {
    match id {
        api_ids::REACTION_TYPE_EMOJI => "emoji",
        api_ids::REACTION_TYPE_CUSTOM_EMOJI => "custom",
        api_ids::REACTION_TYPE_PAID => "paid",
        _ => "unknown",
    }
}

fn vector_object_line(types: &[ReactionType], paid_available: bool) -> String {
    let mut out = String::from("[");
    let mut first = true;
    for object in reaction_types_object(types, paid_available) {
        if !first {
            out.push(',');
        }
        first = false;
        match object {
            None => out.push_str("null"),
            Some(TdReaction::Emoji(emoji)) => {
                out.push_str(td_reaction_name(api_ids::REACTION_TYPE_EMOJI));
                out.push(':');
                out.push_str(&to_hex(emoji.as_bytes()));
            }
            Some(TdReaction::Custom(id)) => {
                out.push_str(td_reaction_name(api_ids::REACTION_TYPE_CUSTOM_EMOJI));
                out.push(':');
                out.push_str(&id.to_string());
            }
            Some(TdReaction::Paid) => out.push_str(td_reaction_name(api_ids::REACTION_TYPE_PAID)),
        }
    }
    out.push(']');
    out
}

fn check_case(name: &str) -> i32 {
    match name {
        "bad_b64" => {
            let reaction = ReactionType::from_string("#zz".to_string());
            println!("hash={}", reaction.hash());
            0
        }
        "short_b64" => {
            let reaction = ReactionType::from_string("#abc".to_string());
            println!("hash={}", reaction.hash());
            0
        }
        "ok_custom" => {
            let reaction = ReactionType::from_td(TdReactionInput::Custom(1));
            println!("hash={}", reaction.hash());
            0
        }
        other => {
            eprintln!("unknown check case {other}");
            2
        }
    }
}

fn run_script(script_path: &str) -> i32 {
    let file = match std::fs::File::open(script_path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("cannot read {script_path}: {error}");
            return 2;
        }
    };
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() || tokens[0].starts_with('#') {
            continue;
        }
        match tokens[0] {
            "print" | "hash" | "input" | "obj" | "upd" => {
                let Some(spec) = tokens.get(1).copied() else {
                    println!("reject {} argc={}", tokens[0], tokens.len());
                    continue;
                };
                let Some(value) = (tokens.len() == 2).then(|| make(spec)).flatten() else {
                    println!("reject {} argc={}", tokens[0], tokens.len());
                    continue;
                };
                match tokens[0] {
                    "print" => println!(
                        "print {spec} str={} empty={} custom={} paid={} {value}",
                        to_hex(value.get_string().as_bytes()),
                        i32::from(value.is_empty()),
                        i32::from(value.is_custom_reaction()),
                        i32::from(value.is_paid_reaction())
                    ),
                    "hash" => println!("hash {spec} {}", value.hash()),
                    "input" => println!("input {spec} {}", input_line(&value)),
                    "obj" => println!("obj {spec} {}", object_line(&value)),
                    _ => match value.update_default_reaction_type() {
                        None => println!("upd {spec} null"),
                        Some(TdReaction::Emoji(_)) => {
                            println!("upd {spec} {}", td_reaction_name(api_ids::REACTION_TYPE_EMOJI))
                        }
                        Some(TdReaction::Custom(_)) => {
                            println!("upd {spec} {}", td_reaction_name(api_ids::REACTION_TYPE_CUSTOM_EMOJI))
                        }
                        Some(TdReaction::Paid) => {
                            println!("upd {spec} {}", td_reaction_name(api_ids::REACTION_TYPE_PAID))
                        }
                    },
                }
            }
            "cmp" => {
                let (Some(a), Some(b)) = (
                    tokens.get(1).and_then(|spec| make(spec)),
                    tokens.get(2).and_then(|spec| make(spec)),
                ) else {
                    println!("reject cmp argc={}", tokens.len());
                    continue;
                };
                if tokens.len() != 3 {
                    println!("reject cmp argc={}", tokens.len());
                    continue;
                }
                println!(
                    "cmp {} {} lt={} eq={} ne={}",
                    tokens[1],
                    tokens[2],
                    i32::from(a.less(&b)),
                    i32::from(a == b),
                    i32::from(a != b)
                );
            }
            "vec" => {
                if tokens.len() < 3 || (tokens[1] != "0" && tokens[1] != "1") {
                    println!("reject vec argc={}", tokens.len());
                    continue;
                }
                let mut types = Vec::new();
                let mut ok = true;
                for spec in &tokens[2..] {
                    match make(spec) {
                        Some(value) => types.push(value),
                        None => ok = false,
                    }
                }
                if !ok {
                    println!("reject vec spec");
                    continue;
                }
                println!(
                    "vec {} {}",
                    tokens[1..].join(" "),
                    vector_object_line(&types, tokens[1] == "1")
                );
            }
            "vechash" => {
                if tokens.len() < 2 {
                    println!("reject vechash argc={}", tokens.len());
                    continue;
                }
                let mut types = Vec::new();
                let mut ok = true;
                for spec in &tokens[1..] {
                    match make(spec) {
                        Some(value) => types.push(value),
                        None => ok = false,
                    }
                }
                if !ok {
                    println!("reject vechash spec");
                    continue;
                }
                println!("vechash {} {}", tokens[1..].join(" "), reaction_types_hash(&types));
            }
            other => println!("reject unknown={other}"),
        }
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--check" {
        std::process::exit(check_case(&args[2]));
    }
    if args.len() != 2 {
        eprintln!("usage: reaction_type_probe <script.txt> | --check <case>");
        std::process::exit(2);
    }
    std::process::exit(run_script(&args[1]));
}