// (Unofficial) Rust side of the `td_api_objects` lane; the oracle is
// `tools/fixtures/td_api_objects_probe.cpp` over the generated `td_api` JSON
// printers plus three assembly functions.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin td_api_objects_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/td_api_objects_probe.exe <script.txt>
//        rust/target/.../td_api_objects_probe.exe --check badutf
//
// Judged (facts F234/F235): the *printing* rules of the generated JSON storer
// and the three mappings `Usernames::get_usernames_object()`,
// `get_outline_object(path, zoom, source)` and
// `MessageId::get_message_id(telegram_api::Message*, bool)`.
//
// Printing rules, as measured from the oracle:
//   * `@type` is the first field, the rest keep declaration order, no whitespace;
//   * `int64` is a JSON **string**, `int32` is a number;
//   * `double` is `%f`-style with six decimals, so `nan` / `inf` / `-inf` come
//     out bare (not valid JSON);
//   * `bytes` is standard base64;
//   * `"` `\` and 7..=13 are short escapes, other bytes below 32 are `\u00xx`,
//     ASCII 32..=127 is verbatim, and >= 128 is `\uXXXX` (surrogate pairs above
//     the BMP); invalid UTF-8 aborts inside JsonBuilder;
//   * a **null object_ptr field is omitted** from its object, while a null
//     element of a vector prints as `null`; an empty vector prints `[]`.

use std::io::Write;

#[allow(dead_code)]
#[path = "../script_tokens.rs"]
mod script_tokens;

#[allow(dead_code)]
#[path = "../telegram_message_id.rs"]
mod telegram_message_id;

#[allow(dead_code)]
#[path = "../telegram_outline.rs"]
mod telegram_outline;

#[allow(dead_code)]
#[path = "../telegram_usernames.rs"]
mod telegram_usernames;

use script_tokens::*;
use telegram_message_id::*;
use telegram_outline::{encode, outline_paths, Point};
use telegram_usernames::{parse_items, Usernames, UsernamesObject};

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < input.len() {
        let left = (input.len() - i).min(3);
        let mut c = (input[i] as u32) << 16;
        i += 1;
        out.push(BASE64[(c >> 18) as usize]);
        if left != 1 {
            c |= (input[i] as u32) << 8;
            i += 1;
        }
        out.push(BASE64[((c >> 12) & 63) as usize]);
        if left == 3 {
            c |= input[i] as u32;
            i += 1;
        }
        if left != 1 {
            out.push(BASE64[((c >> 6) & 63) as usize]);
        } else {
            out.push(b'=');
        }
        if left == 3 {
            out.push(BASE64[(c & 63) as usize]);
        } else {
            out.push(b'=');
        }
    }
    out
}

fn push_u16_escape(out: &mut String, value: u32) {
    out.push_str("\\u");
    for shift in [12, 8, 4, 0] {
        out.push(char::from_digit((value >> shift) & 15, 16).unwrap());
    }
}

/// `td::JsonString`: the same escapes and the same UTF-8 CHECKs.
fn json_string(value: &[u8], out: &mut String) {
    out.push('"');
    let mut pos = 0;
    while pos < value.len() {
        let ch = value[pos];
        match ch {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            8 => out.push_str("\\b"),
            12 => out.push_str("\\f"),
            10 => out.push_str("\\n"),
            13 => out.push_str("\\r"),
            9 => out.push_str("\\t"),
            0..=31 => push_u16_escape(out, ch as u32),
            32..=127 => out.push(ch as char),
            _ => {
                let text = std::str::from_utf8(&value[pos..])
                    .ok()
                    .and_then(|rest| rest.chars().next());
                let decoded = text.unwrap_or_else(|| panic!("JsonBuilder: string is not valid UTF-8"));
                let code = decoded as u32;
                if code < 0x10000 {
                    push_u16_escape(out, code);
                } else {
                    push_u16_escape(out, 0xD7C0 + (code >> 10));
                    push_u16_escape(out, 0xDC00 + (code & 0x3FF));
                }
                pos += decoded.len_utf8() - 1;
            }
        }
        pos += 1;
    }
    out.push('"');
}

fn json_double(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf".to_string() } else { "-inf".to_string() };
    }
    format!("{value:.6}")
}

fn json_int64(value: i64, out: &mut String) {
    out.push('"');
    out.push_str(&value.to_string());
    out.push('"');
}

fn json_point(point: Point, out: &mut String) {
    out.push_str("{\"@type\":\"point\",\"x\":");
    out.push_str(&json_double(point.x));
    out.push_str(",\"y\":");
    out.push_str(&json_double(point.y));
    out.push('}');
}

fn json_string_array(values: &[Vec<u8>], out: &mut String) {
    out.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        json_string(value, out);
    }
    out.push(']');
}

fn usernames_json(object: Option<&UsernamesObject>) -> String {
    let object = match object {
        Some(object) => object,
        None => return "null".to_string(),
    };
    let mut out = String::from("{\"@type\":\"usernames\",\"active_usernames\":");
    json_string_array(&object.active, &mut out);
    out.push_str(",\"disabled_usernames\":");
    json_string_array(&object.disabled, &mut out);
    out.push_str(",\"editable_username\":");
    json_string(&object.editable, &mut out);
    out.push_str(",\"collectible_usernames\":");
    json_string_array(&object.collectible, &mut out);
    out.push_str(",\"secondary_username\":");
    json_string(&object.secondary, &mut out);
    out.push('}');
    out
}

fn json_vector_path_command(command: &telegram_outline::PathCommand, out: &mut String) {
    match command {
        telegram_outline::PathCommand::Line(end) => {
            out.push_str("{\"@type\":\"vectorPathCommandLine\",\"end_point\":");
            json_point(*end, out);
            out.push('}');
        }
        telegram_outline::PathCommand::CubicBezier { start_control, end_control, end } => {
            out.push_str("{\"@type\":\"vectorPathCommandCubicBezierCurve\",\"start_control_point\":");
            json_point(*start_control, out);
            out.push_str(",\"end_control_point\":");
            json_point(*end_control, out);
            out.push_str(",\"end_point\":");
            json_point(*end, out);
            out.push('}');
        }
    }
}

fn outline_json(paths: Option<&[telegram_outline::ClosedPath]>) -> String {
    let paths = match paths {
        Some(paths) => paths,
        None => return "null".to_string(),
    };
    let mut out = String::from("{\"@type\":\"outline\",\"paths\":[");
    let mut first = true;
    for path in paths {
        // The C++ only pushes a closedVectorPath when the subpath had commands.
        if path.commands.is_empty() {
            continue;
        }
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str("{\"@type\":\"closedVectorPath\",\"commands\":[");
        for (index, command) in path.commands.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            json_vector_path_command(command, &mut out);
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    out
}

fn chat_position_json(list: Option<&str>, order: i64, is_pinned: bool, with_source: bool) -> String {
    let mut out = String::from("{\"@type\":\"chatPosition\"");
    if let Some(list) = list {
        out.push_str(",\"list\":{\"@type\":\"");
        out.push_str(list);
        out.push_str("\"}");
    }
    out.push_str(",\"order\":");
    json_int64(order, &mut out);
    out.push_str(",\"is_pinned\":");
    out.push_str(if is_pinned { "true" } else { "false" });
    if with_source {
        out.push_str(",\"source\":{\"@type\":\"chatSourceMtprotoProxy\"}");
    }
    out.push('}');
    out
}

fn print_rules(out: &mut std::io::Stdout) {
    let mut lines = String::new();
    lines.push_str("ok={\"@type\":\"ok\"}\n");
    lines.push_str("error={\"@type\":\"error\",\"code\":0,\"message\":\"\"}\n");

    let controls: Vec<u8> = (0u8..32).chain(std::iter::once(127)).collect();
    let mut message = String::new();
    json_string(&controls, &mut message);
    lines.push_str(&format!("escapes={{\"@type\":\"error\",\"code\":1,\"message\":{message}}}\n"));

    let utf = "a\"b\\c\u{e9}\u{1F600}";
    let mut message = String::new();
    json_string(utf.as_bytes(), &mut message);
    lines.push_str(&format!(
        "utfe={{\"@type\":\"error\",\"code\":{},\"message\":{message}}}\n",
        i32::MIN
    ));

    let mut data = String::new();
    json_string(&base64(&[0x00, 0x01, 0xff, 0x7f]), &mut data);
    lines.push_str(&format!("data={{\"@type\":\"data\",\"data\":{data}}}\n"));
    lines.push_str("dataempty={\"@type\":\"data\",\"data\":\"\"}\n");

    lines.push_str(&format!("point={}\n", point_json(1.5, -0.0)));
    lines.push_str(&format!("pointbig={}\n", point_json(1e300, 1e-7)));
    lines.push_str(&format!("pointint={}\n", point_json(2.0, 3.0)));
    lines.push_str(&format!("pointnan={}\n", point_json(f64::NAN, f64::INFINITY)));
    lines.push_str(&format!(
        "pointneginf={}\n",
        point_json(f64::NEG_INFINITY, 0.0000005)
    ));

    lines.push_str(&format!(
        "posmax={}\n",
        chat_position_json(Some("chatListMain"), i64::MAX, true, false)
    ));
    lines.push_str(&format!(
        "posmin={}\n",
        chat_position_json(Some("chatListArchive"), i64::MIN, false, false)
    ));
    lines.push_str(&format!("posnull={}\n", chat_position_json(None, 0, false, false)));
    lines.push_str("cpath_empty={\"@type\":\"closedVectorPath\",\"commands\":[]}\n");
    lines.push_str("outline_empty={\"@type\":\"outline\",\"paths\":[]}\n");

    let full = concat!(
        "{\"@type\":\"outline\",\"paths\":[",
        "{\"@type\":\"closedVectorPath\",\"commands\":[",
        "{\"@type\":\"vectorPathCommandLine\",\"end_point\":{\"@type\":\"point\",\"x\":0.250000,\"y\":-3.500000}},",
        "{\"@type\":\"vectorPathCommandCubicBezierCurve\",",
        "\"start_control_point\":{\"@type\":\"point\",\"x\":1.000000,\"y\":2.000000},",
        "\"end_control_point\":{\"@type\":\"point\",\"x\":3.000000,\"y\":4.000000},",
        "\"end_point\":{\"@type\":\"point\",\"x\":5.000000,\"y\":6.000000}}]},",
        "null]}"
    );
    lines.push_str(&format!("outline_full={full}\n"));
    out.write_all(lines.as_bytes()).unwrap();
}

fn point_json(x: f64, y: f64) -> String {
    let mut out = String::new();
    json_point(Point { x, y }, &mut out);
    out
}

fn render_message_id(message_id: MessageId) -> String {
    format!(
        "msgid raw={} valid={} sched={} type={} text={}",
        message_id.id,
        message_id.is_valid() as u8,
        message_id.is_scheduled() as u8,
        message_id.get_type() as i32,
        message_id.render()
    )
}

/// The payload after the first `n` tokens: `outline <zoom> <svg path...>` needs
/// the rest of the line verbatim, spaces included.
fn rest_after_tokens(line: &[u8], n: usize) -> &[u8] {
    let mut pos = 0;
    let mut seen = 0;
    while pos < line.len() && seen < n {
        while pos < line.len() && line[pos] == b' ' {
            pos += 1;
        }
        while pos < line.len() && line[pos] != b' ' {
            pos += 1;
        }
        seen += 1;
    }
    while pos < line.len() && line[pos] == b' ' {
        pos += 1;
    }
    &line[pos..]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--check" {
        if args[2] != "badutf" {
            eprintln!("unknown check case");
            std::process::exit(2);
        }
        let mut out = std::io::stdout();
        out.write_all(b"before\n").unwrap();
        out.flush().unwrap();
        let mut message = String::new();
        json_string(b"x\xffy", &mut message);
        let _ = writeln!(out, "after={message}");
        out.flush().unwrap();
        return;
    }
    if args.len() != 2 {
        eprintln!("usage: td_api_objects_probe <script.txt>");
        std::process::exit(2);
    }
    let script = std::fs::read(&args[1]).expect("cannot read script");
    let mut out = std::io::stdout();
    for raw_line in script.split(|c| *c == b'\n') {
        let line = trim_line(raw_line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens = split_tokens(line);
        match tokens[0] {
            b"ulist" => {
                let usernames = Usernames::new(parse_name(tokens[1]), parse_items(tokens[2]));
                let object = usernames.get_usernames_object();
                let _ = writeln!(out, "ulist={}", usernames_json(object.as_ref()));
            }
            b"obj" => {
                let object = UsernamesObject {
                    active: parse_csv(tokens[1]),
                    disabled: parse_csv(tokens[2]),
                    editable: parse_name(tokens[3]),
                    collectible: parse_csv(tokens[4]),
                    secondary: parse_name(tokens[5]),
                };
                let _ = writeln!(out, "obj={}", usernames_json(Some(&object)));
            }
            b"outline" => {
                let zoom: f64 = std::str::from_utf8(tokens[1]).unwrap().parse().unwrap();
                let svg = rest_after_tokens(line, 2);
                let encoded = encode(svg);
                let paths = outline_paths(&encoded, zoom);
                let json = match &paths {
                    Ok(paths) => outline_json(Some(paths)),
                    Err(_) => outline_json(None),
                };
                let _ = writeln!(out, "outline={json}");
            }
            b"msgid" => {
                let kind = match tokens[1] {
                    b"message" => TlMessageKind::Message,
                    b"empty" => TlMessageKind::Empty,
                    b"service" => TlMessageKind::Service,
                    _ => {
                        eprintln!("bad message kind");
                        std::process::exit(2);
                    }
                };
                let id: i32 = std::str::from_utf8(tokens[2]).unwrap().parse().unwrap();
                let date: i32 = std::str::from_utf8(tokens[3]).unwrap().parse().unwrap();
                let scheduled = tokens[4] == b"1";
                let message_id = MessageId::from_tl_message(kind, id, date, scheduled);
                let _ = writeln!(out, "{}", render_message_id(message_id));
            }
            b"rules" => print_rules(&mut out),
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}