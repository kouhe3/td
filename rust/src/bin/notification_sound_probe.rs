// (Unofficial) Rust side of the `notification_sound` lane; the oracle is
// `tools/fixtures/notification_sound_probe.cpp` over the real
// `td/telegram/NotificationSound.cpp`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin notification_sound_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/notification_sound_probe.exe <script.txt>
//        ... --check store | parse
//
// Only the pure value semantics are judged (docs/facts.md F249): printing,
// the "a Local sound is the default" predicate, both equivalence functions, the
// ringtone-ID mapping, the two factories, the TL-object mapping, the input
// mapping and duplication. `store`/`parse` are unreachable offline because the
// log-event storers and parser `LOG_CHECK` the actor context — the two
// `--check` cases pin that they abort on both sides.

use std::io::Write;

#[allow(dead_code)]
#[path = "../script_tokens.rs"]
mod script_tokens;

use script_tokens::{split_tokens, trim_line};

#[derive(Clone, PartialEq, Eq)]
enum Sound {
    None,
    Local { title: Vec<u8>, data: Vec<u8> },
    Ringtone(i64),
}

/// `None` is the C++ `nullptr`; `Some` is a real object.
type SoundPtr = Option<Sound>;

/// The C++ prints the raw bytes, so invalid UTF-8 in a title goes out as-is.
fn render(sound: &SoundPtr) -> Vec<u8> {
    match sound {
        None => b"DefaultSound".to_vec(),
        Some(Sound::None) => b"NoSound".to_vec(),
        Some(Sound::Local { title, data }) => {
            let mut out = b"LocalSound[".to_vec();
            out.extend_from_slice(title);
            out.push(b'|');
            out.extend_from_slice(data);
            out.push(b']');
            out
        }
        Some(Sound::Ringtone(id)) => format!("Ringtone[{}]", id).into_bytes(),
    }
}

fn type_name(sound: &SoundPtr) -> &'static str {
    match sound {
        None => "null",
        Some(Sound::None) => "none",
        Some(Sound::Local { .. }) => "local",
        Some(Sound::Ringtone(_)) => "ringtone",
    }
}

/// A `Local` sound **is** the default, exactly like `nullptr` (F249).
fn is_default(sound: &SoundPtr) -> bool {
    match sound {
        None => true,
        Some(Sound::Local { .. }) => true,
        Some(_) => false,
    }
}

fn are_equivalent(lhs: &SoundPtr, rhs: &SoundPtr) -> bool {
    if is_default(lhs) {
        return is_default(rhs);
    }
    if is_default(rhs) {
        return false;
    }
    match (lhs, rhs) {
        (Some(Sound::None), Some(Sound::None)) => true,
        (Some(Sound::Ringtone(lhs_id)), Some(Sound::Ringtone(rhs_id))) => lhs_id == rhs_id,
        // The C++ `UNREACHABLE()` for Local is dead: Local is default and never
        // gets this far.
        _ => false,
    }
}

fn are_different_equivalent(lhs: &SoundPtr, rhs: &SoundPtr) -> bool {
    match (lhs, rhs) {
        (None, Some(Sound::Local { .. })) => true,
        (Some(Sound::Local { .. }), None) => true,
        (Some(Sound::Local { title: lhs_t, data: lhs_d }), Some(Sound::Local { title: rhs_t, data: rhs_d })) => {
            lhs_t != rhs_t || lhs_d != rhs_d
        }
        _ => false,
    }
}

fn ringtone_id(sound: &SoundPtr) -> i64 {
    match sound {
        None => -1,
        Some(Sound::None) => 0,
        Some(Sound::Local { .. }) => -1,
        Some(Sound::Ringtone(id)) => *id,
    }
}

fn legacy(sound: &[u8]) -> SoundPtr {
    if sound == b"default" {
        return None;
    }
    if sound.is_empty() {
        return Some(Sound::None);
    }
    Some(Sound::Local { title: sound.to_vec(), data: sound.to_vec() })
}

fn from_bool_id(use_default_sound: bool, ringtone_id: i64) -> SoundPtr {
    if use_default_sound || ringtone_id == -1 {
        return None;
    }
    if ringtone_id == 0 {
        return Some(Sound::None);
    }
    Some(Sound::Ringtone(ringtone_id))
}

enum TlSound {
    Default,
    None,
    Local { title: Vec<u8>, data: Vec<u8> },
    Ringtone(i64),
}

fn from_tl(object: Option<TlSound>) -> SoundPtr {
    match object? {
        TlSound::Default => None,
        TlSound::None => Some(Sound::None),
        TlSound::Local { title, data } => Some(Sound::Local { title, data }),
        // `ringtone ID 0 or -1` logs and becomes NoSound; it does not CHECK.
        TlSound::Ringtone(id) => {
            if id == 0 || id == -1 {
                Some(Sound::None)
            } else {
                Some(Sound::Ringtone(id))
            }
        }
    }
}

enum InputSound {
    Null,
    Default,
    None,
    Local { title: Vec<u8>, data: Vec<u8> },
    Ringtone(i64),
}

fn get_input(sound: &SoundPtr, return_non_null: bool) -> InputSound {
    match sound {
        None => {
            if return_non_null {
                InputSound::Default
            } else {
                InputSound::Null
            }
        }
        Some(Sound::None) => InputSound::None,
        Some(Sound::Local { title, data }) => InputSound::Local { title: title.clone(), data: data.clone() },
        Some(Sound::Ringtone(id)) => InputSound::Ringtone(*id),
    }
}

fn duplicate(sound: &SoundPtr) -> SoundPtr {
    sound.clone()
}

// ------------------------------------------------------------------- parsing

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

fn unhex(hex: &[u8]) -> Vec<u8> {
    assert!(hex.len() % 2 == 0, "bad hex");
    hex.chunks(2)
        .map(|pair| {
            let hi = hex_digit(pair[0]).expect("bad hex digit");
            let lo = hex_digit(pair[1]).expect("bad hex digit");
            (hi << 4) | lo
        })
        .collect()
}

fn parse_text(token: &[u8]) -> Vec<u8> {
    if token == b"-" {
        return Vec::new();
    }
    if token.len() > 4 && &token[..4] == b"hex-" {
        return unhex(&token[4..]);
    }
    token.to_vec()
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn parse_spec(token: &[u8]) -> SoundPtr {
    if token == b"-" {
        return None;
    }
    if token == b"none" {
        return Some(Sound::None);
    }
    if token.len() > 6 && &token[..6] == b"local:" {
        let body = &token[6..];
        let bar = body.iter().position(|c| *c == b'|').expect("bad local spec");
        return Some(Sound::Local {
            title: parse_text(&body[..bar]),
            data: parse_text(&body[bar + 1..]),
        });
    }
    if token.len() > 5 && &token[..5] == b"ring:" {
        let id: i64 = std::str::from_utf8(&token[5..]).unwrap().parse().unwrap();
        // The probe builds a sound through `get_notification_sound(false, id)`,
        // so `ring:0` and `ring:-1` are NoSound and the default, not ringtones.
        return from_bool_id(false, id);
    }
    panic!("bad sound spec");
}

fn print_sound(prefix: &str, sound: &SoundPtr, out: &mut std::io::Stdout) {
    let mut line = format!("{} type={} text=", prefix, type_name(sound)).into_bytes();
    line.extend_from_slice(&render(sound));
    line.push(b'\n');
    out.write_all(&line).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    if args.len() == 3 && args[1] == "--check" {
        // Both paths must abort before printing `after`: the log-event storers
        // and the parser CHECK the actor context through `G()`.
        match args[2].as_str() {
            "store" => {
                let _ = writeln!(out, "before type={}", type_name(&Some(Sound::Ringtone(7))));
                out.flush().unwrap();
                panic!("LogEventStorerCalcLength CHECK(set_context(G()))");
            }
            "parse" => {
                let _ = writeln!(out, "before");
                out.flush().unwrap();
                panic!("LogEventParser CHECK(set_context(G()))");
            }
            _ => {
                eprintln!("unknown check case");
                std::process::exit(2);
            }
        }
    }
    if args.len() != 2 {
        eprintln!("usage: notification_sound_probe <script.txt>");
        std::process::exit(2);
    }

    let script = std::fs::read(&args[1]).expect("cannot read script");
    for raw_line in script.split(|c| *c == b'\n') {
        let line = trim_line(raw_line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens = split_tokens(line);
        match tokens[0] {
            b"print" => print_sound("print", &parse_spec(tokens[1]), &mut out),
            b"def" => {
                let lhs = parse_spec(tokens[1]);
                let rhs = parse_spec(tokens[2]);
                let _ = writeln!(out, "def lhs={} rhs={}", is_default(&lhs) as u8, is_default(&rhs) as u8);
            }
            b"eq" => {
                let lhs = parse_spec(tokens[1]);
                let rhs = parse_spec(tokens[2]);
                let _ = writeln!(out, "eq={}", are_equivalent(&lhs, &rhs) as u8);
            }
            b"diff" => {
                let lhs = parse_spec(tokens[1]);
                let rhs = parse_spec(tokens[2]);
                let _ = writeln!(out, "diff={}", are_different_equivalent(&lhs, &rhs) as u8);
            }
            b"rid" => {
                let sound = parse_spec(tokens[1]);
                let _ = writeln!(out, "rid={}", ringtone_id(&sound));
            }
            b"legacy" => print_sound("legacy", &legacy(&parse_text(tokens[1])), &mut out),
            b"boolsound" => {
                let use_default = tokens[1] == b"1";
                let id: i64 = std::str::from_utf8(tokens[2]).unwrap().parse().unwrap();
                print_sound("boolsound", &from_bool_id(use_default, id), &mut out);
            }
            b"tl" => {
                let object = match tokens[1] {
                    b"null" => None,
                    b"default" => Some(TlSound::Default),
                    b"none" => Some(TlSound::None),
                    b"local" => Some(TlSound::Local {
                        title: parse_text(tokens[2]),
                        data: parse_text(tokens[3]),
                    }),
                    b"ring" => {
                        let id: i64 = std::str::from_utf8(tokens[2]).unwrap().parse().unwrap();
                        Some(TlSound::Ringtone(id))
                    }
                    other => {
                        eprintln!("bad tl kind {}", String::from_utf8_lossy(other));
                        std::process::exit(2);
                    }
                };
                print_sound("tl", &from_tl(object), &mut out);
            }
            b"input" => {
                let sound = parse_spec(tokens[1]);
                let return_non_null = tokens[2] == b"1";
                let text = match get_input(&sound, return_non_null) {
                    InputSound::Null => "input=null".to_string(),
                    InputSound::Default => "input=default".to_string(),
                    InputSound::None => "input=none".to_string(),
                    InputSound::Local { title, data } => {
                        format!("input=local title={} data={}", hex_encode(&title), hex_encode(&data))
                    }
                    InputSound::Ringtone(id) => format!("input=ring id={}", id),
                };
                let _ = writeln!(out, "{}", text);
            }
            b"dup" => print_sound("dup", &duplicate(&parse_spec(tokens[1])), &mut out),
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}