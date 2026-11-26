use std::cell::RefCell;
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};

thread_local! {
    static LAST_RESULT: RefCell<CString> = RefCell::new(CString::new("").unwrap());
}

fn set_result(s: String) -> *const c_char {
    LAST_RESULT.with(|slot| {
        let mut buf = slot.borrow_mut();
        *buf = CString::new(s).unwrap_or_default();
        buf.as_ptr()
    })
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_ascii_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_hashtag_letter(c: char) -> bool {
    c == '_' || c == '\u{200c}' || c == '\u{b7}' || c.is_alphabetic() || c.is_numeric()
}

fn is_letter(c: char) -> bool {
    c.is_alphabetic()
}

fn utf16_prefix(chars: &[char]) -> Vec<usize> {
    let mut out = Vec::with_capacity(chars.len() + 1);
    let mut acc = 0;
    for c in chars {
        out.push(acc);
        acc += c.len_utf16();
    }
    out.push(acc);
    out
}

fn find_mentions(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        if i > 0 && is_word_char(chars[i - 1]) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && is_ascii_ident(chars[j]) {
            j += 1;
        }
        let name_len = j - i - 1;
        let next_is_word = j < chars.len() && is_word_char(chars[j]);
        if (2..=32).contains(&name_len) && !next_is_word {
            out.push((i, j - i));
        }
        i = j.max(i + 1);
    }
    out
}

fn find_hashtags(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '#' {
            i += 1;
            continue;
        }
        if i > 0 && is_hashtag_letter(chars[i - 1]) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut has_letter = false;
        while j < chars.len() && is_hashtag_letter(chars[j]) {
            has_letter |= is_letter(chars[j]);
            j += 1;
        }
        let body_len = j - i - 1;
        let next_is_hashtag_letter = j < chars.len() && is_hashtag_letter(chars[j]);
        if (1..=256).contains(&body_len) && has_letter && !next_is_hashtag_letter {
            out.push((i, j - i));
        }
        i = j.max(i + 1);
    }
    out
}

fn extract_string_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let colon = rest.find(':')? + 1;
    let mut it = rest[colon..].chars().peekable();
    while matches!(it.peek(), Some(c) if c.is_whitespace()) {
        it.next();
    }
    if it.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    let mut high_surrogate: Option<u32> = None;
    while let Some(c) = it.next() {
        match c {
            '"' => return Some(out),
            '\\' => {
                let e = it.next()?;
                match e {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let hex: String = (0..4).filter_map(|_| it.next()).collect();
                        let code = u32::from_str_radix(&hex, 16).ok()?;
                        if (0xd800..0xdc00).contains(&code) {
                            high_surrogate = Some(code);
                            continue;
                        }
                        if (0xdc00..0xe000).contains(&code) {
                            let high = high_surrogate.take()?;
                            let combined = 0x10000 + ((high - 0xd800) << 10) + (code - 0xdc00);
                            out.push(char::from_u32(combined)?);
                            continue;
                        }
                        high_surrogate = None;
                        out.push(char::from_u32(code)?);
                    }
                    other => out.push(other),
                }
            }
            c => out.push(c),
        }
    }
    None
}

fn entities_json(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let off = utf16_prefix(&chars);
    let mut entities: Vec<(usize, usize, &str)> = Vec::new();
    for (pos, len) in find_mentions(&chars) {
        if len - 1 >= 4 {
            entities.push((pos, len, "textEntityTypeMention"));
        }
    }
    for (pos, len) in find_hashtags(&chars) {
        entities.push((pos, len, "textEntityTypeHashtag"));
    }
    entities.sort_by_key(|e| e.0);
    let parts: Vec<String> = entities
        .iter()
        .map(|(pos, len, kind)| {
            let (pos, len) = (*pos, *len);
            let offset = off[pos];
            let length = off[pos + len] - off[pos];
            format!(
                "{{\"@type\":\"textEntity\",\"offset\":{},\"length\":{},\"type\":{{\"@type\":\"{}\"}}}}",
                offset, length, kind
            )
        })
        .collect();
    format!("{{\"@type\":\"textEntities\",\"entities\":[{}]}}", parts.join(","))
}

fn execute(request: &str) -> String {
    match extract_string_field(request, "@type").as_deref() {
        Some("getTextEntities") => match extract_string_field(request, "text") {
            Some(text) => entities_json(&text),
            None => "{\"@type\":\"error\",\"code\":400,\"message\":\"Text must be encoded in UTF-8\"}".to_string(),
        },
        Some(other) => format!(
            "{{\"@type\":\"error\",\"code\":400,\"message\":\"tdjson_rs: unimplemented request type {}\"}}",
            other.replace('"', "'")
        ),
        None => "{\"@type\":\"error\",\"code\":400,\"message\":\"tdjson_rs: malformed request\"}".to_string(),
    }
}

#[no_mangle]
pub extern "C" fn td_execute(request: *const c_char) -> *const c_char {
    if request.is_null() {
        return set_result(String::new());
    }
    let text = unsafe { CStr::from_ptr(request) }.to_string_lossy().into_owned();
    set_result(execute(&text))
}

fn unimplemented(name: &str) -> *const c_char {
    set_result(format!("tdjson_rs: {} not implemented", name))
}

#[no_mangle]
pub extern "C" fn td_create_client_id() -> c_int {
    1
}

#[no_mangle]
pub extern "C" fn td_send(_client_id: c_int, _request: *const c_char) {
    unimplemented("td_send");
}

#[no_mangle]
pub extern "C" fn td_receive(_timeout: c_double) -> *const c_char {
    std::ptr::null()
}

#[no_mangle]
pub extern "C" fn td_set_log_message_callback(_max_verbosity_level: c_int, _callback: *mut c_void) {}

#[no_mangle]
pub extern "C" fn td_json_client_create() -> *mut c_void {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn td_json_client_send(_client: *mut c_void, _request: *const c_char) {
    unimplemented("td_json_client_send");
}

#[no_mangle]
pub extern "C" fn td_json_client_receive(_client: *mut c_void, _timeout: c_double) -> *const c_char {
    std::ptr::null()
}

#[no_mangle]
pub extern "C" fn td_json_client_execute(_client: *mut c_void, request: *const c_char) -> *const c_char {
    td_execute(request)
}

#[no_mangle]
pub extern "C" fn td_json_client_destroy(_client: *mut c_void) {}