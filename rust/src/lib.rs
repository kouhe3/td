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

enum Kind {
    Mention,
    Hashtag,
    BotCommand,
    Cashtag,
    BankCard,
    MediaTimestamp(i32),
}

impl Kind {
    fn type_name(&self) -> &'static str {
        match self {
            Kind::Mention => "textEntityTypeMention",
            Kind::Hashtag => "textEntityTypeHashtag",
            Kind::BotCommand => "textEntityTypeBotCommand",
            Kind::Cashtag => "textEntityTypeCashtag",
            Kind::BankCard => "textEntityTypeBankCardNumber",
            Kind::MediaTimestamp(_) => "textEntityTypeMediaTimestamp",
        }
    }
}

fn find_mentions(chars: &[char], out: &mut Vec<(usize, usize, Kind)>) {
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
        if (2..=32).contains(&name_len) && !next_is_word && name_len >= 4 {
            out.push((i, j - i, Kind::Mention));
        }
        i = j.max(i + 1);
    }
}

fn find_hashtags(chars: &[char], out: &mut Vec<(usize, usize, Kind)>) {
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
            has_letter |= chars[j].is_alphabetic();
            j += 1;
        }
        let body_len = j - i - 1;
        if !(1..=256).contains(&body_len) || !has_letter {
            i = j.max(i + 1);
            continue;
        }
        if j < chars.len() && is_hashtag_letter(chars[j]) {
            i = j.max(i + 1);
            continue;
        }
        if j < chars.len() && chars[j] == '@' {
            let mut k = j + 1;
            while k < chars.len() && is_ascii_ident(chars[k]) {
                k += 1;
            }
            let mention_len = k - j - 1;
            if mention_len >= 3 && mention_len <= 32 {
                out.push((i, k - i, Kind::Hashtag));
                i = k;
                continue;
            }
        }
        out.push((i, j - i, Kind::Hashtag));
        i = j;
    }
}

fn find_bot_commands(chars: &[char], out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '/' {
            i += 1;
            continue;
        }
        if i > 0 {
            let prev = chars[i - 1];
            if is_word_char(prev) || prev == '/' || prev == '<' || prev == '>' {
                i += 1;
                continue;
            }
        }
        let mut j = i + 1;
        while j < chars.len() && is_ascii_ident(chars[j]) {
            j += 1;
        }
        let command_len = j - i - 1;
        if !(1..=64).contains(&command_len) {
            i = j.max(i + 1);
            continue;
        }
        if j < chars.len() && chars[j] == '@' {
            let mut k = j + 1;
            while k < chars.len() && is_ascii_ident(chars[k]) {
                k += 1;
            }
            let mention_len = k - j - 1;
            if !(3..=32).contains(&mention_len) {
                i = j.max(i + 1);
                continue;
            }
            j = k;
        }
        if j < chars.len() {
            let next = chars[j];
            if is_word_char(next) || next == '/' || next == '<' || next == '>' {
                i = j;
                continue;
            }
        }
        out.push((i, j - i, Kind::BotCommand));
        i = j;
    }
}

fn find_cashtags(chars: &[char], out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' {
            i += 1;
            continue;
        }
        if i > 0 && (is_hashtag_letter(chars[i - 1]) || chars[i - 1] == '$') {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if chars.len() >= j + 5 && chars[j..j + 5].iter().collect::<String>() == "1INCH" {
            j += 5;
        } else {
            while j < chars.len() && chars[j].is_ascii_uppercase() && j - i <= 8 {
                j += 1;
            }
            if j - i - 1 < 1 || j - i - 1 > 8 {
                i += 1;
                continue;
            }
        }
        if j < chars.len() && chars[j] == '@' {
            let mut k = j + 1;
            while k < chars.len() && is_ascii_ident(chars[k]) {
                k += 1;
            }
            let mention_len = k - j - 1;
            if (3..=32).contains(&mention_len) {
                j = k;
            }
        }
        if j < chars.len() && is_hashtag_letter(chars[j]) {
            i = j.max(i + 1);
            continue;
        }
        out.push((i, j - i, Kind::Cashtag));
        i = j;
    }
}

fn is_valid_bank_card(chars: &[char]) -> bool {
    let digits: Vec<u32> = chars.iter().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let n = digits.len();
    let mut sum = 0;
    for i in (0..n).rev() {
        let d = digits[i] as i32;
        if (n - 1 - i) % 2 == 0 {
            sum += d;
        } else {
            sum += if d < 5 { 2 * d } else { 2 * d - 9 };
        }
    }
    if sum % 10 != 0 {
        return false;
    }
    let p1 = digits[0];
    let p2 = p1 * 10 + digits[1];
    let p3 = p2 * 10 + digits[2];
    let p4 = p3 * 10 + digits[3];
    if p1 == 4 {
        return n == 13 || n == 16 || n == 18 || n == 19;
    }
    if (51..=55).contains(&p2) || (2221..=2720).contains(&p4) {
        return n == 16;
    }
    if p2 == 34 || p2 == 37 {
        return n == 15;
    }
    if p2 == 62 || p2 == 81 {
        return n >= 16;
    }
    if (2200..=2204).contains(&p4) {
        return n == 16;
    }
    true
}

fn find_bank_cards(chars: &[char], out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        if i > 0 {
            let prev = chars[i - 1];
            if prev == '.' || prev == ',' || prev == '+' || prev == '-' || prev == '_' || prev.is_alphabetic() {
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == ' ' || chars[i] == '-') {
                    i += 1;
                }
                continue;
            }
        }
        let begin = i;
        let mut digit_count = 0usize;
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == ' ' || chars[i] == '-') {
            if chars[i] == ' ' && (16..=19).contains(&digit_count) && digit_count == i - begin {
                break;
            }
            if chars[i].is_ascii_digit() {
                digit_count += 1;
            }
            i += 1;
        }
        if !(13..=19).contains(&digit_count) {
            continue;
        }
        let mut end = i;
        while end > begin && !chars[end - 1].is_ascii_digit() {
            end -= 1;
        }
        if end - begin > 2 * digit_count - 1 {
            continue;
        }
        if end < chars.len() {
            let next = chars[end];
            if next == '-' || next == '_' || next.is_alphabetic() {
                continue;
            }
        }
        if is_valid_bank_card(&chars[begin..end]) {
            out.push((begin, end - begin, Kind::BankCard));
        }
    }
}

fn find_media_timestamps(chars: &[char], out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != ':' {
            i += 1;
            continue;
        }
        let mut begin = i;
        while begin > 0 && (chars[begin - 1] == ':' || chars[begin - 1].is_ascii_digit()) {
            begin -= 1;
        }
        let mut end = i;
        while end + 1 < chars.len() && (chars[end + 1] == ':' || chars[end + 1].is_ascii_digit()) {
            end += 1;
        }
        end += 1;
        if begin == i || end == i + 1 || !chars[i + 1].is_ascii_digit() {
            i = end;
            continue;
        }
        if begin > 0 && is_word_char(chars[begin - 1]) {
            i = end;
            continue;
        }
        if end < chars.len() && is_word_char(chars[end]) {
            i = end;
            continue;
        }
        let part: String = chars[begin..end].iter().collect();
        let parts: Vec<&str> = part.split(':').collect();
        if parts.len() < 2 || parts.len() > 3 {
            i = end;
            continue;
        }
        let last = parts[parts.len() - 1];
        if last.len() != 2 {
            i = end;
            continue;
        }
        let Ok(seconds) = last.parse::<i32>() else {
            i = end;
            continue;
        };
        if seconds >= 60 {
            i = end;
            continue;
        }
        let value = if parts.len() == 2 {
            let first = parts[0];
            if first.is_empty() || first.len() > 4 {
                i = end;
                continue;
            }
            let Ok(minutes) = first.parse::<i32>() else {
                i = end;
                continue;
            };
            minutes * 60 + seconds
        } else {
            let (h, m) = (parts[0], parts[1]);
            if h.is_empty() || m.is_empty() || h.len() > 2 || m.len() > 2 {
                i = end;
                continue;
            }
            let (Ok(hours), Ok(minutes)) = (h.parse::<i32>(), m.parse::<i32>()) else {
                i = end;
                continue;
            };
            if minutes >= 60 {
                i = end;
                continue;
            }
            hours * 3600 + minutes * 60 + seconds
        };
        out.push((begin, end - begin, Kind::MediaTimestamp(value)));
        i = end;
    }
}

fn find_entities(chars: &[char]) -> Vec<(usize, usize, Kind)> {
    let mut all = Vec::new();
    find_mentions(chars, &mut all);
    find_bot_commands(chars, &mut all);
    find_hashtags(chars, &mut all);
    find_cashtags(chars, &mut all);
    find_bank_cards(chars, &mut all);
    find_media_timestamps(chars, &mut all);
    all.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut kept: Vec<(usize, usize, Kind)> = Vec::with_capacity(all.len());
    let mut last_end = 0usize;
    for e in all {
        if e.0 >= last_end {
            last_end = e.0 + e.1;
            kept.push(e);
        }
    }
    kept
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