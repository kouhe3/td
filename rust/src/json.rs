// 拆分自 `rust/src/lib.rs`（一次性 codemod，见 tasks/plan.md T119）；只改可见性与位置，未改实现。
#![allow(unused_imports)]

use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    // numbers keep their literal, like JsonValue::get_number in TDLib, because
    // integer fields are parsed from the literal with to_integer_safe
    Int(i64, String),
    Float(f64, String),
    // literals that don't fit i64: TDLib keeps them and fails later with
    // "Can't parse as an integer string"
    Num(String),
    // bytes, not String: TDLib keeps raw bytes and the actions decide whether they are UTF-8
    Str(Vec<u8>),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// td::json_decode: max depth 100, trailing content rejected, messages verbatim
    pub(crate) fn parse(bytes: &[u8]) -> Result<Json, String> {
        let mut pos = 0usize;
        let value = parse_value(bytes, &mut pos, 100)?;
        skip_json_ws(bytes, &mut pos);
        if pos != bytes.len() {
            return Err("Expected string end".to_string());
        }
        Ok(value)
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(members) => members.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(bytes) => std::str::from_utf8(bytes).ok(),
            _ => None,
        }
    }

    pub(crate) fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Json::Str(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub(crate) fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(value, _) => Some(*value),
            Json::Float(value, _) => Some(*value as i64),
            Json::Num(_) => None,
            _ => None,
        }
    }
}

pub(crate) fn skip_json_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }
}

pub(crate) fn json_starts_with(bytes: &[u8], pos: usize, token: &[u8]) -> bool {
    bytes.len() >= pos + token.len() && &bytes[pos..pos + token.len()] == token
}

pub(crate) fn parse_value(bytes: &[u8], pos: &mut usize, depth: i32) -> Result<Json, String> {
    if depth < 0 {
        return Err("Too big object depth".to_string());
    }
    skip_json_ws(bytes, pos);
    match bytes.get(*pos).copied() {
        Some(b'{') => {
            *pos += 1;
            let mut members = Vec::new();
            skip_json_ws(bytes, pos);
            if bytes.get(*pos) == Some(&b'}') {
                *pos += 1;
                return Ok(Json::Obj(members));
            }
            loop {
                if *pos >= bytes.len() {
                    return Err("Unexpected string end".to_string());
                }
                let key = parse_json_string(bytes, pos)?;
                skip_json_ws(bytes, pos);
                if bytes.get(*pos) != Some(&b':') {
                    return Err("':' expected".to_string());
                }
                *pos += 1;
                let value = parse_value(bytes, pos, depth - 1)?;
                members.push((String::from_utf8_lossy(&key).into_owned(), value));
                skip_json_ws(bytes, pos);
                match bytes.get(*pos) {
                    Some(b'}') => {
                        *pos += 1;
                        return Ok(Json::Obj(members));
                    }
                    Some(b',') => {
                        *pos += 1;
                        skip_json_ws(bytes, pos);
                    }
                    _ if *pos >= bytes.len() => {
                        return Err("Unexpected string end".to_string())
                    }
                    _ => return Err("Unexpected symbol while parsing JSON Object".to_string()),
                }
            }
        }
        Some(b'[') => {
            *pos += 1;
            let mut items = Vec::new();
            skip_json_ws(bytes, pos);
            if bytes.get(*pos) == Some(&b']') {
                *pos += 1;
                return Ok(Json::Arr(items));
            }
            loop {
                if *pos >= bytes.len() {
                    return Err("Unexpected string end".to_string());
                }
                items.push(parse_value(bytes, pos, depth - 1)?);
                skip_json_ws(bytes, pos);
                match bytes.get(*pos) {
                    Some(b']') => {
                        *pos += 1;
                        return Ok(Json::Arr(items));
                    }
                    Some(b',') => {
                        *pos += 1;
                        skip_json_ws(bytes, pos);
                    }
                    _ if *pos >= bytes.len() => {
                        return Err("Unexpected string end".to_string())
                    }
                    _ => return Err("Unexpected symbol while parsing JSON Array".to_string()),
                }
            }
        }
        Some(b'"') => parse_json_string(bytes, pos).map(Json::Str),
        Some(b't') => {
            if json_starts_with(bytes, *pos, b"true") {
                *pos += 4;
                Ok(Json::Bool(true))
            } else {
                Err("Token starts with 't' -- true expected".to_string())
            }
        }
        Some(b'f') => {
            if json_starts_with(bytes, *pos, b"false") {
                *pos += 5;
                Ok(Json::Bool(false))
            } else {
                Err("Token starts with 'f' -- false expected".to_string())
            }
        }
        Some(b'n') => {
            if json_starts_with(bytes, *pos, b"null") {
                *pos += 4;
                Ok(Json::Null)
            } else {
                Err("Token starts with 'n' -- null expected".to_string())
            }
        }
        Some(b'-' | b'+' | b'.' | b'0'..=b'9') => Ok(parse_json_number(bytes, pos)),
        Some(0) | None => Err("Unexpected string end".to_string()),
        Some(byte) => {
            if byte.is_ascii() && byte != 0 {
                Err(format!("Unexpected symbol '{}'", byte as char))
            } else {
                Err("Unexpected symbol".to_string())
            }
        }
    }
}

pub(crate) fn parse_json_number(bytes: &[u8], pos: &mut usize) -> Json {
    let start = *pos;
    let mut is_float = false;
    while *pos < bytes.len() {
        match bytes[*pos] {
            b'-' | b'+' | b'0'..=b'9' => *pos += 1,
            b'.' | b'e' | b'E' => {
                is_float = true;
                *pos += 1;
            }
            _ => break,
        }
    }
    let text = String::from_utf8_lossy(&bytes[start..*pos]).into_owned();
    if is_float {
        return Json::Float(to_double_prefix(&text), text);
    }
    match text.parse::<i64>() {
        Ok(value) => Json::Int(value, text),
        Err(_) => Json::Num(text),
    }
}

/// td::to_double: the stringstream reads the longest numeric prefix, 0.0 when it can't
pub(crate) fn to_double_prefix(text: &str) -> f64 {
    let mut end = text.len();
    while end > 0 {
        if let Ok(value) = text[..end].parse::<f64>() {
            // libstdc++ 的 num_get 在溢出时给出 ±DBL_MAX，Rust 给 ±inf
            return if value.is_infinite() {
                if value > 0.0 {
                    f64::MAX
                } else {
                    -f64::MAX
                }
            } else {
                value
            };
        }
        end -= 1;
    }
    0.0
}

pub(crate) fn parse_json_string(bytes: &[u8], pos: &mut usize) -> Result<Vec<u8>, String> {
    if bytes.get(*pos) != Some(&b'"') {
        return Err("Opening '\"' expected".to_string());
    }
    *pos += 1;
    let mut out: Vec<u8> = Vec::new();
    loop {
        let byte = match bytes.get(*pos) {
            Some(byte) => *byte,
            None => return Err("Closing '\"' not found".to_string()),
        };
        *pos += 1;
        match byte {
            b'"' => return Ok(out),
            b'\\' => {
                let escape = match bytes.get(*pos) {
                    Some(escape) => *escape,
                    None => return Err("Closing '\"' not found".to_string()),
                };
                *pos += 1;
                match escape {
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'n' => out.push(10),
                    b'r' => out.push(13),
                    b't' => out.push(9),
                    b'u' => {
                        let mut code = parse_json_hex4(bytes, pos)?;
                        if code > 0xd7ff
                            && code < 0xe000
                            && *pos + 6 <= bytes.len()
                            && bytes[*pos] == b'\\'
                            && bytes[*pos + 1] == b'u'
                        {
                            let saved = *pos;
                            *pos += 2;
                            let low = parse_json_hex4(bytes, pos)?;
                            if low > 0xd7ff && low < 0xe000 {
                                code = (((code & 0x3ff) << 10) | (low & 0x3ff)) + 0x10000;
                            } else {
                                *pos = saved;
                            }
                        }
                        // 孤立代理对按 TDLib 的 append_utf8_character_unsafe 编成原始三字节，
                        // 下游 check_utf8 会判成「not encoded in UTF-8」
                        push_utf8_unsafe(&mut out, code);
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
    }
}

pub(crate) fn push_utf8_unsafe(out: &mut Vec<u8>, code: u32) {
    if code < 0x80 {
        out.push(code as u8);
    } else if code < 0x800 {
        out.push(0xc0 | (code >> 6) as u8);
        out.push(0x80 | (code & 0x3f) as u8);
    } else if code < 0x10000 {
        out.push(0xe0 | (code >> 12) as u8);
        out.push(0x80 | ((code >> 6) & 0x3f) as u8);
        out.push(0x80 | (code & 0x3f) as u8);
    } else {
        out.push(0xf0 | (code >> 18) as u8);
        out.push(0x80 | ((code >> 12) & 0x3f) as u8);
        out.push(0x80 | ((code >> 6) & 0x3f) as u8);
        out.push(0x80 | (code & 0x3f) as u8);
    }
}

pub(crate) fn parse_json_hex4(bytes: &[u8], pos: &mut usize) -> Result<u32, String> {
    if *pos + 4 > bytes.len() {
        return Err("\\u has less than 4 symbols".to_string());
    }
    let mut code = 0u32;
    for _ in 0..4 {
        let digit = hex_value_byte(bytes[*pos])
            .ok_or_else(|| "Invalid \\u -- not hex digit".to_string())?;
        code = code * 16 + digit;
        *pos += 1;
    }
    Ok(code)
}

pub(crate) fn hex_value_byte(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some(u32::from(byte - b'0')),
        b'a'..=b'f' => Some(u32::from(byte - b'a') + 10),
        b'A'..=b'F' => Some(u32::from(byte - b'A') + 10),
        _ => None,
    }
}

pub(crate) fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

pub(crate) fn json_escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
        }
    }
}

pub(crate) fn text_object(text: &str) -> String {
    let mut body = String::new();
    json_escape(text, &mut body);
    format!("{{\"@type\":\"text\",\"text\":\"{}\"}}", body)
}

pub(crate) fn error_object(code: i64, message: &str) -> String {
    let mut body = String::new();
    json_escape(message, &mut body);
    format!("{{\"@type\":\"error\",\"code\":{},\"message\":\"{}\"}}", code, body)
}

pub(crate) fn ok_object() -> String {
    "{\"@type\":\"ok\"}".to_string()
}

pub(crate) fn entities_json(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let off = utf16_prefix(&chars);
    let parts: Vec<String> = find_entities(&chars)
        .into_iter()
        .map(|(pos, len, kind)| {
            let offset = off[pos];
            let length = off[pos + len] - off[pos];
            let type_body = match kind {
                Kind::MediaTimestamp(value) => format!(
                    "{{\"@type\":\"{}\",\"media_timestamp\":{}}}",
                    kind.type_name(),
                    value
                ),
                _ => format!("{{\"@type\":\"{}\"}}", kind.type_name()),
            };
            format!(
                "{{\"@type\":\"textEntity\",\"offset\":{},\"length\":{},\"type\":{}}}",
                offset, length, type_body
            )
        })
        .collect();
    format!("{{\"@type\":\"textEntities\",\"entities\":[{}]}}", parts.join(","))
}

// ---------------------------------------------------------------------------
// getFileMimeType / getFileExtension: tdutils/td/utils/MimeType.cpp over the
// gperf tables that tdutils/generate/generate_mime_types_gperf.cpp builds from
// tdutils/generate/mime_types.txt. MIME_TYPES_DATA is that 786-line input
// ("mime_type extension...", whitespace separated); both lookup tables are
// derived from it with the generator's exact rules.
// ---------------------------------------------------------------------------

pub(crate) fn mime_lookup(table: &[(&'static str, &'static str)], key: &str) -> &'static str {
    if key.is_empty() {
        return "";
    }
    let lowered = key.to_ascii_lowercase();
    match table.binary_search_by(|(candidate, _)| (*candidate).cmp(lowered.as_str())) {
        Ok(index) => table[index].1,
        Err(_) => "",
    }
}

pub(crate) fn file_mime_type(file_name: &str) -> &'static str {
    mime_lookup(&mime_tables::EXTENSION_TO_MIME, path_extension(file_name))
}

pub(crate) fn mime_type_extension(mime_type: &str) -> &'static str {
    mime_lookup(&mime_tables::MIME_TO_EXTENSION, mime_type)
}
