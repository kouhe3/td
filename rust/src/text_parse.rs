// 拆分自 `rust/src/lib.rs`（一次性 codemod，见 tasks/plan.md T119）；只改可见性与位置，未改实现。
#![allow(unused_imports)]

use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{LazyLock, Mutex};

pub(crate) fn byte_at(bytes: &[u8], pos: usize) -> u8 {
    bytes.get(pos).copied().unwrap_or(0)
}

pub(crate) fn is_space_byte(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0 | 0x0b)
}

pub(crate) fn is_alpha_byte(c: u8) -> bool {
    let c = c | 0x20;
    c >= b'a' && c <= b'z'
}

pub(crate) fn is_digit_byte(c: u8) -> bool {
    c >= b'0' && c <= b'9'
}

pub(crate) fn is_alnum_byte(c: u8) -> bool {
    is_alpha_byte(c) || is_digit_byte(c)
}

pub(crate) fn hex_digit_value(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some(u32::from(c - b'0')),
        b'a'..=b'f' => Some(u32::from(c - b'a') + 10),
        b'A'..=b'F' => Some(u32::from(c - b'A') + 10),
        _ => None,
    }
}

pub(crate) fn is_hex_digit_byte(c: u8) -> bool {
    hex_digit_value(c).is_some()
}

pub(crate) fn utf8_is_first_code_unit(c: u8) -> bool {
    c & 0xc0 != 0x80
}

pub(crate) fn to_lower_bytes(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .map(|&c| if c.is_ascii_uppercase() { c | 0x20 } else { c })
        .collect()
}

pub(crate) fn tolower_begins_with(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len()
        && prefix
            .iter()
            .enumerate()
            .all(|(index, &expected)| bytes[index].to_ascii_lowercase() == expected)
}

pub(crate) fn bytes_to_string(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    }
}

pub(crate) fn to_integer_i64(bytes: &[u8]) -> i64 {
    let mut value: u64 = 0;
    let mut pos = 0;
    let mut is_negative = false;
    if bytes.first() == Some(&b'-') {
        is_negative = true;
        pos = 1;
    }
    while pos < bytes.len() && is_digit_byte(bytes[pos]) {
        value = value
            .wrapping_mul(10)
            .wrapping_add(u64::from(bytes[pos] - b'0'));
        pos += 1;
    }
    if value > i64::MAX as u64 {
        value = (!value).wrapping_add(1);
        is_negative = !is_negative;
        if value > i64::MAX as u64 {
            return i64::MIN;
        }
    }
    let value = value as i64;
    if is_negative {
        value.wrapping_neg()
    } else {
        value
    }
}

pub(crate) fn to_integer_i32(bytes: &[u8]) -> i32 {
    let mut value: u32 = 0;
    let mut pos = 0;
    let mut is_negative = false;
    if bytes.first() == Some(&b'-') {
        is_negative = true;
        pos = 1;
    }
    while pos < bytes.len() && is_digit_byte(bytes[pos]) {
        value = value
            .wrapping_mul(10)
            .wrapping_add(u32::from(bytes[pos] - b'0'));
        pos += 1;
    }
    if value > i32::MAX as u32 {
        value = (!value).wrapping_add(1);
        is_negative = !is_negative;
        if value > i32::MAX as u32 {
            return i32::MIN;
        }
    }
    let value = value as i32;
    if is_negative {
        value.wrapping_neg()
    } else {
        value
    }
}

pub(crate) fn to_integer_safe_i64(bytes: &[u8]) -> Option<i64> {
    let value = to_integer_i64(bytes);
    if value.to_string().as_bytes() == bytes {
        Some(value)
    } else {
        None
    }
}

pub(crate) fn to_integer_safe_i32(bytes: &[u8]) -> Option<i32> {
    let value = to_integer_i32(bytes);
    if value.to_string().as_bytes() == bytes {
        Some(value)
    } else {
        None
    }
}

/// td/utils/HttpUrl.cpp
pub(crate) struct HttpUrl {
    https: bool,
    userinfo: Vec<u8>,
    host: Vec<u8>,
    is_ipv6: bool,
    specified_port: i32,
    query: Vec<u8>,
}

impl HttpUrl {
    pub(crate) fn get_url(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(self.host.len() + self.query.len() + 16);
        result.extend_from_slice(if self.https { b"https://" } else { b"http://" });
        if !self.userinfo.is_empty() {
            result.extend_from_slice(&self.userinfo);
            result.push(b'@');
        }
        result.extend_from_slice(&self.host);
        if self.specified_port > 0 {
            result.push(b':');
            result.extend_from_slice(self.specified_port.to_string().as_bytes());
        }
        result.extend_from_slice(&self.query);
        result
    }
}

/// inet_pton(AF_INET6) over the text inside the brackets.
pub(crate) fn is_valid_ipv6(text: &[u8]) -> bool {
    let size = text.len();
    if size == 0 {
        return false;
    }
    let mut written = 0usize;
    let mut pos = 0usize;
    let mut compressed = false;
    if text[0] == b':' {
        if size < 2 || text[1] != b':' {
            return false;
        }
        compressed = true;
        pos = 2;
    }
    while pos < size {
        let start = pos;
        let mut value: u32 = 0;
        let mut digits = 0;
        while let Some(digit) = text.get(pos).copied().and_then(hex_digit_value) {
            if digits == 4 {
                return false;
            }
            value = value * 16 + digit;
            digits += 1;
            pos += 1;
        }
        if digits == 0 {
            return false;
        }
        if text.get(pos) == Some(&b'.') {
            // an IPv4 address must terminate the IPv6 address
            if written + 4 > 16 {
                return false;
            }
            let mut octets = [0u8; 4];
            let mut cursor = start;
            for (index, octet) in octets.iter_mut().enumerate() {
                let mut number: u32 = 0;
                let mut count = 0;
                while let Some(digit) = text.get(cursor).copied().filter(|c| is_digit_byte(*c)) {
                    number = number * 10 + u32::from(digit - b'0');
                    if number > 255 {
                        return false;
                    }
                    count += 1;
                    cursor += 1;
                }
                if count == 0 || count > 3 {
                    return false;
                }
                *octet = number as u8;
                if index < 3 {
                    if text.get(cursor) != Some(&b'.') {
                        return false;
                    }
                    cursor += 1;
                }
            }
            if cursor != size {
                return false;
            }
            return written + 4 == 16 || compressed;
        }
        if written + 2 > 16 {
            return false;
        }
        written += 2;
        if pos == size {
            break;
        }
        if text[pos] != b':' {
            return false;
        }
        pos += 1;
        if pos == size {
            return false;
        }
        if text[pos] == b':' {
            if compressed {
                return false;
            }
            compressed = true;
            pos += 1;
        }
    }
    written == 16 || compressed
}

pub(crate) fn check_url_part(part: &[u8], name: &str, allow_colon: bool) -> Result<(), String> {
    let mut pos = 0usize;
    while pos < part.len() {
        let c = part[pos];
        if is_alnum_byte(c)
            || matches!(
                c,
                b'.' | b'-'
                    | b'_'
                    | b'!'
                    | b'$'
                    | b','
                    | b'~'
                    | b'*'
                    | b'\''
                    | b'('
                    | b')'
                    | b';'
                    | b'&'
                    | b'+'
                    | b'='
            )
            || (allow_colon && c == b':')
        {
            pos += 1;
            continue;
        }
        if c == b'%' {
            pos += 1;
            if is_hex_digit_byte(byte_at(part, pos)) {
                pos += 1;
                if is_hex_digit_byte(byte_at(part, pos)) {
                    pos += 1;
                    continue;
                }
            }
            return Err(format!("Wrong percent-encoded symbol in URL {}", name));
        }
        if c >= 128 {
            // plain UTF-8 symbols are allowed
            pos += 1;
            continue;
        }
        return Err(format!("Disallowed character in URL {}", name));
    }
    Ok(())
}

pub(crate) fn parse_url(url: &[u8]) -> Result<HttpUrl, String> {
    let mut pos = 0usize;
    while pos < url.len() && !matches!(url[pos], b':' | b'/' | b'?' | b'#' | b'@' | b'[' | b']') {
        pos += 1;
    }
    let protocol = to_lower_bytes(&url[..pos]);
    let https;
    if url[pos..].starts_with(b"://") {
        if protocol == b"http" {
            https = false;
        } else if protocol == b"https" {
            https = true;
        } else {
            return Err("Unsupported URL protocol".to_string());
        }
        pos += 3;
    } else {
        pos = 0;
        https = false;
    }

    let host_begin = pos;
    while pos < url.len() && !matches!(url[pos], b'/' | b'?' | b'#') {
        pos += 1;
    }
    let userinfo_host_port = &url[host_begin..pos];

    let mut port = 0i32;
    let mut userinfo_host = userinfo_host_port;
    let colon = userinfo_host_port
        .iter()
        .rposition(|&c| c == b':' || c == b']' || c == b'@');
    if let Some(colon) = colon {
        if colon > 0 && userinfo_host_port[colon] == b':' {
            let mut port_slice = &userinfo_host_port[colon + 1..];
            while port_slice.len() > 1 && port_slice[0] == b'0' {
                port_slice = &port_slice[1..];
            }
            port = match to_integer_safe_i32(port_slice) {
                Some(value) if value != 0 => value,
                _ => -1,
            };
            userinfo_host = &userinfo_host_port[..colon];
        }
    }
    if port < 0 || port > 65535 {
        return Err("Wrong port number specified in the URL".to_string());
    }

    let (userinfo, host) = match userinfo_host.iter().rposition(|&c| c == b'@') {
        Some(at_pos) => (&userinfo_host[..at_pos], &userinfo_host[at_pos + 1..]),
        None => (&userinfo_host[..0], userinfo_host),
    };

    let mut is_ipv6 = false;
    if !host.is_empty() && host[0] == b'[' && host[host.len() - 1] == b']' {
        if !is_valid_ipv6(&host[1..host.len() - 1]) {
            return Err("Wrong IPv6 address specified in the URL".to_string());
        }
        is_ipv6 = true;
    }
    if host.is_empty() {
        return Err("URL host is empty".to_string());
    }
    if host == b"." {
        return Err("Host is invalid".to_string());
    }

    let specified_port = port;

    let mut query = &url[pos..];
    while !query.is_empty() && is_space_byte(query[query.len() - 1]) {
        query = &query[..query.len() - 1];
    }
    let mut query_str: Vec<u8> = Vec::with_capacity(query.len() + 1);
    if query.is_empty() || query[0] != b'/' {
        query_str.push(b'/');
    }
    for &c in query {
        if c <= 0x20 {
            query_str.push(b'%');
            query_str.push(b"0123456789ABCDEF"[usize::from(c / 16)]);
            query_str.push(b"0123456789ABCDEF"[usize::from(c % 16)]);
        } else {
            query_str.push(c);
        }
    }

    let host_str = to_lower_bytes(host);
    if is_ipv6 {
        for &c in &host_str[1..host_str.len() - 1] {
            if !(c == b':' || is_digit_byte(c) || (b'a'..=b'f').contains(&c) || c == b'.') {
                return Err("Wrong IPv6 URL host".to_string());
            }
        }
    } else {
        check_url_part(&host_str, "host", false)?;
        check_url_part(userinfo, "userinfo", true)?;
    }

    Ok(HttpUrl {
        https,
        userinfo: userinfo.to_vec(),
        host: host_str,
        is_ipv6,
        specified_port,
        query: query_str,
    })
}

pub(crate) fn check_link_impl(link: &[u8]) -> Result<Vec<u8>, String> {
    let mut link = link;
    let mut is_tg = false;
    let mut is_ton = false;
    let mut is_tonsite = false;
    if tolower_begins_with(link, b"tg:") {
        link = &link[3..];
        is_tg = true;
    } else if tolower_begins_with(link, b"ton:") {
        link = &link[4..];
        is_ton = true;
    } else if tolower_begins_with(link, b"tonsite:") {
        link = &link[8..];
        is_tonsite = true;
    }
    if (is_tg || is_ton || is_tonsite) && link.starts_with(b"//") {
        link = &link[2..];
    }
    let http_url = parse_url(link)?;
    if is_tg || is_ton || is_tonsite {
        if tolower_begins_with(link, b"http://")
            || http_url.https
            || !http_url.userinfo.is_empty()
            || http_url.specified_port != 0
            || http_url.is_ipv6
        {
            return Err(if is_tg {
                "Wrong tg URL".to_string()
            } else {
                "Wrong ton URL".to_string()
            });
        }
        let mut query: &[u8] = &http_url.query;
        if query.len() > 1 && query[1] == b'?' {
            query = &query[1..];
        }
        for &c in &http_url.host {
            if !is_alnum_byte(c) && c != b'-' && c != b'_' && !(is_tonsite && c == b'.') {
                return Err("Unallowed characters in URL host".to_string());
            }
        }
        let mut result = Vec::new();
        result.extend_from_slice(if is_tg {
            b"tg".as_slice()
        } else if is_tonsite {
            b"tonsite".as_slice()
        } else {
            b"ton".as_slice()
        });
        result.extend_from_slice(b"://");
        result.extend_from_slice(&http_url.host);
        result.extend_from_slice(query);
        return Ok(result);
    }
    if !http_url.host.contains(&b'.') && !http_url.is_ipv6 {
        return Err("Wrong HTTP URL".to_string());
    }
    Ok(http_url.get_url())
}

/// LinkManager::check_link; the message of the invalid link reason is part of
/// the error returned by get_message_entities for textUrl entities.
pub(crate) fn check_link(link: &[u8]) -> Result<Vec<u8>, String> {
    check_link_impl(link).map_err(|reason| match std::str::from_utf8(link) {
        Ok(text) => format!("URL '{}' is invalid: {}", text, reason),
        Err(_) => format!("URL is invalid: {}", reason),
    })
}

pub(crate) fn get_checked_link(link: &[u8]) -> Option<Vec<u8>> {
    check_link_impl(link).ok()
}

pub(crate) fn split_parameter(parameter: &[u8]) -> (&[u8], &[u8]) {
    match parameter.iter().position(|&c| c == b'=') {
        Some(pos) => (&parameter[..pos], &parameter[pos + 1..]),
        None => (parameter, &[]),
    }
}

pub(crate) fn get_link_user_id(url: &[u8]) -> Option<i64> {
    let lower_cased_url = to_lower_bytes(url);
    let mut url: &[u8] = &lower_cased_url;
    if !url.starts_with(b"tg:") {
        return None;
    }
    url = &url[3..];
    if url.starts_with(b"//") {
        url = &url[2..];
    }
    let host = b"user";
    if !url.starts_with(host) || (url.len() > host.len() && !b"/?#".contains(&url[host.len()])) {
        return None;
    }
    url = &url[host.len()..];
    if url.starts_with(b"/") {
        url = &url[1..];
    }
    if !url.starts_with(b"?") {
        return None;
    }
    url = &url[1..];
    if let Some(pos) = url.iter().position(|&c| c == b'#') {
        url = &url[..pos];
    }
    for parameter in url.split(|&c| c == b'&') {
        let (key, value) = split_parameter(parameter);
        if key == b"id" {
            return match to_integer_safe_i64(value) {
                Some(user_id) if 0 < user_id && user_id <= MAX_USER_ID => Some(user_id),
                _ => None,
            };
        }
    }
    None
}

pub(crate) fn check_tg_url_host(url: &[u8], host: &[u8]) -> Option<Vec<u8>> {
    let min_size = 3 + 2 + host.len() + 1;
    let lower_cased_head = to_lower_bytes(if url.len() > min_size {
        &url[..min_size]
    } else {
        url
    });
    let mut lower_cased_url: &[u8] = &lower_cased_head;
    let mut url = url;
    if !lower_cased_url.starts_with(b"tg:") {
        return None;
    }
    lower_cased_url = &lower_cased_url[3..];
    url = &url[3..];
    if url.starts_with(b"//") {
        lower_cased_url = &lower_cased_url[2..];
        url = &url[2..];
    }
    if !lower_cased_url.starts_with(host)
        || (url.len() > host.len() && !b"/?#".contains(&url[host.len()]))
    {
        return None;
    }
    url = &url[host.len()..];
    if url.starts_with(b"/") {
        url = &url[1..];
    }
    if !url.starts_with(b"?") {
        return None;
    }
    url = &url[1..];
    if let Some(pos) = url.iter().position(|&c| c == b'#') {
        url = &url[..pos];
    }
    Some(url.to_vec())
}

pub(crate) fn get_link_custom_emoji_id(url: &[u8]) -> Option<i64> {
    let query = check_tg_url_host(url, b"emoji")?;
    for parameter in query.split(|&c| c == b'&') {
        let (key, value) = split_parameter(parameter);
        if key == b"id" {
            return match to_integer_safe_i64(value) {
                Some(custom_emoji_id) if custom_emoji_id != 0 => Some(custom_emoji_id),
                _ => None,
            };
        }
    }
    None
}

/// td/telegram/FormattedDate.h flags
pub(crate) const DATE_RELATIVE: i32 = 1;
pub(crate) const DATE_SHORT_TIME: i32 = 2;
pub(crate) const DATE_LONG_TIME: i32 = 4;
pub(crate) const DATE_SHORT_DATE: i32 = 8;
pub(crate) const DATE_LONG_DATE: i32 = 16;
pub(crate) const DATE_DAY_OF_WEEK: i32 = 32;

pub(crate) fn get_formatted_date(date: i32, date_format: &[u8]) -> Result<(i32, i32), String> {
    let date_flags = get_date_flags(date_format)?;
    Ok((date, date_flags))
}

pub(crate) fn get_date_flags(date_format: &[u8]) -> Result<i32, String> {
    if date_format == b"r" || date_format == b"R" {
        return Ok(DATE_RELATIVE);
    }
    let mut date_flags = 0;
    for &c in date_format {
        date_flags |= match c {
            b't' => DATE_SHORT_TIME,
            b'T' => DATE_LONG_TIME,
            b'd' => DATE_SHORT_DATE,
            b'D' => DATE_LONG_DATE,
            b'w' | b'W' => DATE_DAY_OF_WEEK,
            _ => return Err("Invalid date format used".to_string()),
        };
    }
    Ok(date_flags)
}

pub(crate) fn get_link_formatted_date(url: &[u8]) -> Option<(i32, i32)> {
    let query = check_tg_url_host(url, b"time")?;
    let mut date = 0i32;
    let mut format: &[u8] = b"";
    for parameter in query.split(|&c| c == b'&') {
        let (key, value) = split_parameter(parameter);
        if key == b"unix" {
            date = match to_integer_safe_i32(value) {
                Some(value) if value > 0 => value,
                _ => return None,
            };
        }
        if key == b"format" {
            format = value;
        }
    }
    if date == 0 {
        return None;
    }
    get_formatted_date(date, format).ok()
}

pub(crate) fn encode_code(code: u32, units: &mut [u8; 4]) -> usize {
    match char::from_u32(code) {
        Some(character) => character.encode_utf8(units).len(),
        None => {
            // lone surrogate: TDLib appends it anyway and rejects the text later
            units[0] = 0xe0 | (code >> 12) as u8;
            units[1] = 0x80 | ((code >> 6) & 0x3f) as u8;
            units[2] = 0x80 | (code & 0x3f) as u8;
            3
        }
    }
}

pub(crate) fn append_utf8_code(out: &mut Vec<u8>, code: u32) {
    let mut units = [0u8; 4];
    let len = encode_code(code, &mut units);
    out.extend_from_slice(&units[..len]);
}

pub(crate) fn write_utf8_code(buf: &mut [u8], pos: usize, code: u32) -> usize {
    let mut units = [0u8; 4];
    let len = encode_code(code, &mut units);
    buf[pos..pos + len].copy_from_slice(&units[..len]);
    len
}

pub(crate) fn decode_html_entity(bytes: &[u8], pos: usize) -> Option<(u32, usize)> {
    let mut end_pos = pos + 1;
    let code;
    if byte_at(bytes, pos + 1) == b'#' {
        let mut value: u32 = 0;
        end_pos += 1;
        if matches!(byte_at(bytes, pos + 2), b'x' | b'X') {
            end_pos += 1;
            while let Some(digit) = hex_digit_value(byte_at(bytes, end_pos)) {
                value = value.wrapping_mul(16).wrapping_add(digit);
                end_pos += 1;
            }
        } else {
            while is_digit_byte(byte_at(bytes, end_pos)) {
                value = value
                    .wrapping_mul(10)
                    .wrapping_add(u32::from(byte_at(bytes, end_pos) - b'0'));
                end_pos += 1;
            }
        }
        if value == 0 || value >= 0x10ffff || end_pos - pos >= 10 {
            return None;
        }
        code = value;
    } else {
        while is_alpha_byte(byte_at(bytes, end_pos)) {
            end_pos += 1;
        }
        let entity = &bytes[pos + 1..end_pos];
        code = if entity == b"lt" {
            u32::from(b'<')
        } else if entity == b"gt" {
            u32::from(b'>')
        } else if entity == b"amp" {
            u32::from(b'&')
        } else if entity == b"quot" {
            u32::from(b'"')
        } else {
            return None;
        };
    }
    let next_pos = if byte_at(bytes, end_pos) == b';' {
        end_pos + 1
    } else {
        end_pos
    };
    Some((code, next_pos))
}

pub(crate) fn reserved_character_error(c: u8) -> String {
    format!(
        "Character '{}' is reserved and must be escaped with the preceding '\\'",
        c as char
    )
}

pub(crate) fn parse_markdown(text: &[u8]) -> Result<(String, Vec<TextEntity>), String> {
    let mut buf = text.to_vec();
    let size = buf.len();
    let mut result_size = 0usize;
    let mut entities: Vec<TextEntity> = Vec::new();
    let mut utf16_offset = 0i32;
    let mut i = 0usize;
    while i < size {
        let c = buf[i];
        if c == b'\\' && matches!(byte_at(&buf, i + 1), b'_' | b'*' | b'`' | b'[') {
            i += 1;
            buf[result_size] = buf[i];
            result_size += 1;
            utf16_offset += 1;
            i += 1;
            continue;
        }
        if c != b'_' && c != b'*' && c != b'`' && c != b'[' {
            if utf8_is_first_code_unit(c) {
                utf16_offset += 1 + i32::from(c >= 0xf0);
            }
            buf[result_size] = c;
            result_size += 1;
            i += 1;
            continue;
        }

        // begin of an entity
        let begin_pos = i;
        let mut end_character = c;
        let mut is_pre = false;
        if c == b'[' {
            end_character = b']';
        }

        i += 1;

        let mut language: Vec<u8> = Vec::new();
        if c == b'`' && byte_at(&buf, i) == b'`' && byte_at(&buf, i + 1) == b'`' {
            i += 2;
            is_pre = true;
            let mut language_end = i;
            while !is_space_byte(byte_at(&buf, language_end)) && byte_at(&buf, language_end) != b'`' {
                language_end += 1;
            }
            if i != language_end && language_end < size && byte_at(&buf, language_end) != b'`' {
                language = buf[i..language_end].to_vec();
                i = language_end;
            }
            // skip one new line in the beginning of the text
            if matches!(byte_at(&buf, i), b'\n' | b'\r') {
                let first = byte_at(&buf, i);
                let second = byte_at(&buf, i + 1);
                if matches!(second, b'\n' | b'\r') && first != second {
                    i += 2;
                } else {
                    i += 1;
                }
            }
        }

        let entity_offset = utf16_offset;
        while i < size
            && (buf[i] != end_character
                || (is_pre && !(byte_at(&buf, i + 1) == b'`' && byte_at(&buf, i + 2) == b'`')))
        {
            let current = buf[i];
            if utf8_is_first_code_unit(current) {
                utf16_offset += 1 + i32::from(current >= 0xf0);
            }
            buf[result_size] = current;
            result_size += 1;
            i += 1;
        }
        if i == size {
            return Err(format!(
                "Can't find end of the entity starting at byte offset {}",
                begin_pos
            ));
        }

        if entity_offset != utf16_offset {
            let entity_length = utf16_offset - entity_offset;
            match c {
                b'_' => entities.push(TextEntity::new(EType::Italic, entity_offset, entity_length)),
                b'*' => entities.push(TextEntity::new(EType::Bold, entity_offset, entity_length)),
                b'[' => {
                    let url = if byte_at(&buf, i + 1) != b'(' {
                        // TDLib takes the URL from the already compacted buffer
                        buf[begin_pos + 1..i].to_vec()
                    } else {
                        i += 2;
                        let mut url = Vec::new();
                        while i < size && buf[i] != b')' {
                            url.push(buf[i]);
                            i += 1;
                        }
                        url
                    };
                    if let Some(user_id) = get_link_user_id(&url) {
                        let mut entity = TextEntity::new(EType::MentionName, entity_offset, entity_length);
                        entity.user_id = user_id;
                        entities.push(entity);
                    } else if let Some(checked) = get_checked_link(&url) {
                        let mut entity = TextEntity::new(EType::TextUrl, entity_offset, entity_length);
                        entity.argument = bytes_to_string(checked);
                        entities.push(entity);
                    }
                }
                b'`' => {
                    if is_pre {
                        if language.is_empty() {
                            entities.push(TextEntity::new(EType::Pre, entity_offset, entity_length));
                        } else {
                            let mut entity = TextEntity::new(EType::PreCode, entity_offset, entity_length);
                            entity.argument = bytes_to_string(language);
                            entities.push(entity);
                        }
                    } else {
                        entities.push(TextEntity::new(EType::Code, entity_offset, entity_length));
                    }
                }
                _ => unreachable!(),
            }
        }
        if is_pre {
            i += 2;
        }
        i += 1;
    }
    buf.truncate(result_size);
    Ok((bytes_to_string(buf), entities))
}

pub(crate) struct NestedEntity {
    etype: EType,
    argument: Vec<u8>,
    entity_offset: i32,
    entity_byte_offset: usize,
    entity_begin_pos: usize,
}

pub(crate) fn parse_markdown_v2(text: &[u8]) -> Result<(String, Vec<TextEntity>), String> {
    const RESERVED: &[u8] = b"_*[]()~`>#+-=|{}.!\n";
    const RESERVED_IN_CODE: &[u8] = b"`";

    let mut buf = text.to_vec();
    let size = buf.len();
    let mut result_size = 0usize;
    let mut entities: Vec<TextEntity> = Vec::new();
    let mut utf16_offset = 0i32;
    let mut nested: Vec<NestedEntity> = Vec::new();
    let mut have_blockquote = false;
    let mut can_start_blockquote = true;

    let mut i = 0usize;
    while i < size {
        let c = buf[i];
        if c == b'\\' && byte_at(&buf, i + 1) > 0 && byte_at(&buf, i + 1) <= 126 {
            i += 1;
            utf16_offset += 1;
            buf[result_size] = buf[i];
            result_size += 1;
            if buf[i] != b'\r' {
                can_start_blockquote = buf[i] == b'\n';
            }
            i += 1;
            continue;
        }

        let reserved_characters = match nested.last().map(|entity| entity.etype) {
            Some(EType::Code) | Some(EType::Pre) | Some(EType::PreCode) => RESERVED_IN_CODE,
            _ => RESERVED,
        };
        if !reserved_characters.contains(&c) {
            if utf8_is_first_code_unit(c) {
                utf16_offset += 1 + i32::from(c >= 0xf0);
                if c != b'\r' {
                    can_start_blockquote = false;
                }
            }
            buf[result_size] = c;
            result_size += 1;
            i += 1;
            continue;
        }

        let mut is_end_of_an_entity = false;
        if let Some(back) = nested.last() {
            if have_blockquote && c == b'\n' && (i + 1 == size || byte_at(&buf, i + 1) != b'>') {
                is_end_of_an_entity = true;
            } else {
                is_end_of_an_entity = match back.etype {
                    EType::Bold => c == b'*',
                    EType::Italic => c == b'_' && byte_at(&buf, i + 1) != b'_',
                    EType::Code => c == b'`',
                    EType::Pre | EType::PreCode => {
                        c == b'`'
                            && byte_at(&buf, i + 1) == b'`'
                            && byte_at(&buf, i + 2) == b'`'
                    }
                    EType::TextUrl => c == b']',
                    EType::Underline => c == b'_' && byte_at(&buf, i + 1) == b'_',
                    EType::Strikethrough => c == b'~',
                    EType::Spoiler => c == b'|' && byte_at(&buf, i + 1) == b'|',
                    EType::CustomEmoji => c == b']',
                    EType::BlockQuote => false,
                    _ => unreachable!(),
                };
            }
        }

        if !is_end_of_an_entity {
            // begin of an entity
            let entity_byte_offset = i;
            let etype;
            let mut argument: Vec<u8> = Vec::new();
            match c {
                b'_' => {
                    if byte_at(&buf, i + 1) == b'_' {
                        etype = EType::Underline;
                        i += 1;
                    } else {
                        etype = EType::Italic;
                    }
                }
                b'*' => etype = EType::Bold,
                b'~' => etype = EType::Strikethrough,
                b'|' => {
                    if byte_at(&buf, i + 1) == b'|' {
                        i += 1;
                        etype = EType::Spoiler;
                    } else {
                        return Err(reserved_character_error(c));
                    }
                }
                b'[' => etype = EType::TextUrl,
                b'`' => {
                    if byte_at(&buf, i + 1) == b'`' && byte_at(&buf, i + 2) == b'`' {
                        i += 3;
                        let mut pre_type = EType::Pre;
                        let mut language_end = i;
                        while !is_space_byte(byte_at(&buf, language_end))
                            && byte_at(&buf, language_end) != b'`'
                        {
                            language_end += 1;
                        }
                        if i != language_end
                            && language_end < size
                            && byte_at(&buf, language_end) != b'`'
                        {
                            pre_type = EType::PreCode;
                            argument = buf[i..language_end].to_vec();
                            i = language_end;
                        }
                        // skip one new line in the beginning of the text
                        if matches!(byte_at(&buf, i), b'\n' | b'\r') {
                            let first = byte_at(&buf, i);
                            let second = byte_at(&buf, i + 1);
                            if matches!(second, b'\n' | b'\r') && first != second {
                                i += 2;
                            } else {
                                i += 1;
                            }
                        }
                        i -= 1;
                        etype = pre_type;
                    } else {
                        etype = EType::Code;
                    }
                }
                b'!' => {
                    if byte_at(&buf, i + 1) == b'[' {
                        i += 1;
                        etype = EType::CustomEmoji;
                    } else {
                        return Err(reserved_character_error(c));
                    }
                }
                b'\n' => {
                    utf16_offset += 1;
                    buf[result_size] = b'\n';
                    result_size += 1;
                    can_start_blockquote = true;
                    i += 1;
                    continue;
                }
                b'>' => {
                    if !can_start_blockquote {
                        return Err(reserved_character_error(c));
                    }
                    if have_blockquote {
                        i += 1;
                        continue;
                    }
                    etype = EType::BlockQuote;
                    have_blockquote = true;
                }
                _ => return Err(reserved_character_error(c)),
            }
            nested.push(NestedEntity {
                etype,
                argument,
                entity_offset: utf16_offset,
                entity_byte_offset,
                entity_begin_pos: result_size,
            });
            i += 1;
            continue;
        }

        // end of an entity
        let mut etype = nested.last().unwrap().etype;
        if c == b'\n' && etype != EType::BlockQuote {
            let back = nested.last().unwrap();
            let ends_before_newline = etype == EType::Spoiler
                && (back.entity_byte_offset as i64 == i as i64 - 2
                    || (back.entity_byte_offset as i64 == i as i64 - 3
                        && result_size != 0
                        && buf[result_size - 1] == b'\r'));
            if !ends_before_newline {
                return Err(format!(
                    "Can't find end of {} entity at byte offset {}",
                    back.etype.message_name(),
                    back.entity_byte_offset
                ));
            }
            nested.pop();
            match nested.last() {
                Some(back) if back.etype == EType::BlockQuote => {
                    etype = EType::ExpandableBlockQuote;
                }
                Some(back) => {
                    return Err(format!(
                        "Can't find end of {} entity at byte offset {}",
                        back.etype.message_name(),
                        back.entity_byte_offset
                    ));
                }
                // TDLib CHECK-fails here: have_blockquote means that a
                // BlockQuote is always below the entity that was just popped
                None => {
                    return Err(format!(
                        "Can't find end of {} entity at byte offset {}",
                        EType::BlockQuote.message_name(),
                        i
                    ));
                }
            }
        }

        let mut argument = std::mem::take(&mut nested.last_mut().unwrap().argument);
        let entity_offset = nested.last().unwrap().entity_offset;
        let entity_begin_pos = nested.last().unwrap().entity_begin_pos;
        let mut user_id = 0i64;
        let mut custom_emoji_id = 0i64;
        let mut date = (0i32, 0i32);
        let mut skip_entity = utf16_offset == entity_offset;
        match etype {
            EType::Bold | EType::Italic | EType::Code | EType::Strikethrough => {}
            EType::Underline | EType::Spoiler => i += 1,
            EType::Pre | EType::PreCode => i += 2,
            EType::TextUrl => {
                let url = if byte_at(&buf, i + 1) != b'(' {
                    // use the entity text as a URL
                    buf[entity_begin_pos..result_size].to_vec()
                } else {
                    i += 2;
                    let url_begin_pos = i;
                    let mut url = Vec::new();
                    while i < size && buf[i] != b')' {
                        if buf[i] == b'\\' && byte_at(&buf, i + 1) > 0 && byte_at(&buf, i + 1) <= 126 {
                            url.push(buf[i + 1]);
                            i += 2;
                            continue;
                        }
                        url.push(buf[i]);
                        i += 1;
                    }
                    if byte_at(&buf, i) != b')' {
                        return Err(format!("Can't find end of a URL at byte offset {}", url_begin_pos));
                    }
                    url
                };
                user_id = get_link_user_id(&url).unwrap_or(0);
                if user_id == 0 {
                    match get_checked_link(&url) {
                        Some(checked) => argument = checked,
                        None => skip_entity = true,
                    }
                }
            }
            EType::CustomEmoji => {
                if byte_at(&buf, i + 1) != b'(' {
                    return Err("The entity must contain a tg://emoji or tg://time URL".to_string());
                }
                i += 2;
                let url_begin_pos = i;
                let mut url = Vec::new();
                while i < size && buf[i] != b')' {
                    if buf[i] == b'\\' && byte_at(&buf, i + 1) > 0 && byte_at(&buf, i + 1) <= 126 {
                        url.push(buf[i + 1]);
                        i += 2;
                        continue;
                    }
                    url.push(buf[i]);
                    i += 1;
                }
                if byte_at(&buf, i) != b')' {
                    return Err(format!("Can't find end of a URL at byte offset {}", url_begin_pos));
                }
                if let Some(id) = get_link_custom_emoji_id(&url) {
                    custom_emoji_id = id;
                } else if let Some(formatted_date) = get_link_formatted_date(&url) {
                    date = formatted_date;
                    etype = EType::FormattedDate;
                } else {
                    return Err("Invalid tg://emoji or tg://time URL specified".to_string());
                }
            }
            EType::BlockQuote | EType::ExpandableBlockQuote => {
                have_blockquote = false;
                buf[result_size] = buf[i];
                result_size += 1;
                can_start_blockquote = true;
                utf16_offset += 1;
                skip_entity = false;
            }
            _ => unreachable!(),
        }

        if !skip_entity {
            let entity_length = utf16_offset - entity_offset;
            if user_id != 0 {
                let mut entity = TextEntity::new(EType::MentionName, entity_offset, entity_length);
                entity.user_id = user_id;
                entities.push(entity);
            } else if custom_emoji_id != 0 {
                let mut entity = TextEntity::new(etype, entity_offset, entity_length);
                entity.custom_emoji_id = custom_emoji_id;
                entities.push(entity);
            } else if date.0 > 0 {
                let mut entity = TextEntity::new(etype, entity_offset, entity_length);
                entity.date = date.0;
                entity.date_flags = date.1;
                entities.push(entity);
            } else {
                let mut entity = TextEntity::new(etype, entity_offset, entity_length);
                entity.argument = bytes_to_string(argument);
                entities.push(entity);
            }
        }
        nested.pop();
        i += 1;
    }

    if have_blockquote {
        let mut etype = EType::BlockQuote;
        if let Some(back) = nested.last() {
            if back.etype == EType::Spoiler && back.entity_byte_offset as i64 == size as i64 - 2 {
                nested.pop();
                etype = EType::ExpandableBlockQuote;
            }
        }
        if let Some(back) = nested.last() {
            if back.etype == EType::BlockQuote {
                let entity_length = utf16_offset - back.entity_offset;
                if entity_length != 0 {
                    entities.push(TextEntity::new(etype, back.entity_offset, entity_length));
                }
                nested.pop();
            }
        }
    }
    if let Some(back) = nested.last() {
        return Err(format!(
            "Can't find end of {} entity at byte offset {}",
            back.etype.message_name(),
            back.entity_byte_offset
        ));
    }

    sort_entities(&mut entities);
    buf.truncate(result_size);
    Ok((bytes_to_string(buf), entities))
}

pub(crate) struct HtmlNestedEntity {
    tag_name: Vec<u8>,
    argument: Vec<u8>,
    date: i32,
    entity_offset: i32,
    entity_begin_pos: usize,
}

pub(crate) fn parse_html(text: &[u8]) -> Result<(String, Vec<TextEntity>), String> {
    let mut buf = text.to_vec();
    let size = buf.len();
    let mut result_end = 0usize;
    let mut entities: Vec<TextEntity> = Vec::new();
    let mut utf16_offset = 0i32;
    let mut need_recheck_utf8 = false;
    let mut nested: Vec<HtmlNestedEntity> = Vec::new();

    let mut i = 0usize;
    while i < size {
        let c = buf[i];
        if c == b'&' {
            if let Some((code, next_pos)) = decode_html_entity(&buf, i) {
                i = next_pos;
                utf16_offset += 1 + i32::from(code > 0xffff);
                if (0xd800..=0xdfff).contains(&code) {
                    // half of a surrogate pair
                    need_recheck_utf8 = true;
                }
                result_end += write_utf8_code(&mut buf, result_end, code);
                continue;
            }
        }
        if c != b'<' {
            if utf8_is_first_code_unit(c) {
                utf16_offset += 1 + i32::from(c >= 0xf0);
            }
            buf[result_end] = c;
            result_end += 1;
            i += 1;
            continue;
        }

        let begin_pos = i;
        i += 1;
        if byte_at(&buf, i) != b'/' {
            // begin of an entity
            while !is_space_byte(byte_at(&buf, i)) && byte_at(&buf, i) != b'>' {
                i += 1;
            }
            if byte_at(&buf, i) == 0 {
                return Err(format!("Unclosed start tag at byte offset {}", begin_pos));
            }
            let tag_name = to_lower_bytes(&buf[begin_pos + 1..i]);
            if !matches!(
                tag_name.as_slice(),
                b"a" | b"b"
                    | b"strong"
                    | b"i"
                    | b"em"
                    | b"s"
                    | b"strike"
                    | b"del"
                    | b"u"
                    | b"ins"
                    | b"tg-spoiler"
                    | b"tg-emoji"
                    | b"tg-time"
                    | b"span"
                    | b"pre"
                    | b"code"
                    | b"blockquote"
            ) {
                return Err(format!(
                    "Unsupported start tag \"{}\" at byte offset {}",
                    String::from_utf8_lossy(&tag_name),
                    begin_pos
                ));
            }

            let mut argument: Vec<u8> = Vec::new();
            let mut date = 0i32;
            while byte_at(&buf, i) != b'>' {
                while byte_at(&buf, i) != 0 && is_space_byte(byte_at(&buf, i)) {
                    i += 1;
                }
                if byte_at(&buf, i) == b'>' {
                    break;
                }
                let attribute_begin_pos = i;
                while !is_space_byte(byte_at(&buf, i))
                    && !matches!(byte_at(&buf, i), b'=' | b'>' | b'/' | b'"' | b'\'')
                {
                    i += 1;
                }
                let attribute_name_end = i;
                if attribute_begin_pos == attribute_name_end {
                    return Err(format!(
                        "Empty attribute name in the tag \"{}\" at byte offset {}",
                        String::from_utf8_lossy(&tag_name),
                        begin_pos
                    ));
                }
                while byte_at(&buf, i) != 0 && is_space_byte(byte_at(&buf, i)) {
                    i += 1;
                }
                if byte_at(&buf, i) != b'=' {
                    if byte_at(&buf, i) == 0 {
                        return Err(format!(
                            "Unclosed start tag \"{}\" at byte offset {}",
                            String::from_utf8_lossy(&tag_name),
                            begin_pos
                        ));
                    }
                    if tag_name.as_slice() == b"blockquote"
                        && &buf[attribute_begin_pos..attribute_name_end] == b"expandable"
                    {
                        argument = b"1".to_vec();
                    }
                    continue;
                }
                i += 1;
                while byte_at(&buf, i) != 0 && is_space_byte(byte_at(&buf, i)) {
                    i += 1;
                }
                if byte_at(&buf, i) == 0 {
                    return Err(format!(
                        "Unclosed start tag \"{}\" at byte offset {}",
                        String::from_utf8_lossy(&tag_name),
                        begin_pos
                    ));
                }

                let mut attribute_value: Vec<u8> = Vec::new();
                if !matches!(byte_at(&buf, i), b'\'' | b'"') {
                    // a name token
                    let token_begin_pos = i;
                    while is_alnum_byte(byte_at(&buf, i))
                        || byte_at(&buf, i) == b'.'
                        || byte_at(&buf, i) == b'-'
                    {
                        i += 1;
                    }
                    attribute_value = to_lower_bytes(&buf[token_begin_pos..i]);
                    if !is_space_byte(byte_at(&buf, i)) && byte_at(&buf, i) != b'>' {
                        return Err(format!(
                            "Unexpected end of name token at byte offset {}",
                            token_begin_pos
                        ));
                    }
                } else {
                    // a string literal
                    let end_character = byte_at(&buf, i);
                    i += 1;
                    while byte_at(&buf, i) != end_character && byte_at(&buf, i) != 0 {
                        if byte_at(&buf, i) == b'&' {
                            if let Some((code, next_pos)) = decode_html_entity(&buf, i) {
                                i = next_pos;
                                append_utf8_code(&mut attribute_value, code);
                                continue;
                            }
                        }
                        attribute_value.push(buf[i]);
                        i += 1;
                    }
                    if byte_at(&buf, i) == end_character {
                        i += 1;
                    }
                }
                if byte_at(&buf, i) == 0 {
                    return Err(format!("Unclosed start tag at byte offset {}", begin_pos));
                }

                let attribute_name = &buf[attribute_begin_pos..attribute_name_end];
                if tag_name.as_slice() == b"a" && attribute_name == b"href" {
                    argument = attribute_value;
                } else if tag_name.as_slice() == b"code"
                    && attribute_name == b"class"
                    && attribute_value.starts_with(b"language-")
                {
                    argument = attribute_value[9..].to_vec();
                } else if tag_name.as_slice() == b"span"
                    && attribute_name == b"class"
                    && attribute_value.starts_with(b"tg-")
                {
                    argument = attribute_value[3..].to_vec();
                } else if tag_name.as_slice() == b"tg-emoji" && attribute_name == b"emoji-id" {
                    argument = attribute_value;
                } else if tag_name.as_slice() == b"blockquote" && attribute_name == b"expandable" {
                    argument = b"1".to_vec();
                } else if tag_name.as_slice() == b"tg-time" && attribute_name == b"unix" {
                    date = to_integer_i32(&attribute_value);
                } else if tag_name.as_slice() == b"tg-time" && attribute_name == b"format" {
                    argument = attribute_value;
                }
            }
            if tag_name.as_slice() == b"span" && argument != b"spoiler" {
                return Err(format!(
                    "Tag \"span\" must have class \"tg-spoiler\" at byte offset {}",
                    begin_pos
                ));
            }

            nested.push(HtmlNestedEntity {
                tag_name,
                argument,
                date,
                entity_offset: utf16_offset,
                entity_begin_pos: result_end,
            });
            i += 1;
            continue;
        }

        // end of an entity
        if nested.is_empty() {
            return Err(format!("Unexpected end tag at byte offset {}", begin_pos));
        }
        while !is_space_byte(byte_at(&buf, i)) && byte_at(&buf, i) != b'>' {
            i += 1;
        }
        let end_tag_name = to_lower_bytes(&buf[begin_pos + 2..i]);
        while is_space_byte(byte_at(&buf, i)) && byte_at(&buf, i) != 0 {
            i += 1;
        }
        if byte_at(&buf, i) != b'>' {
            return Err(format!("Unclosed end tag at byte offset {}", begin_pos));
        }

        let back = nested.last().unwrap();
        if !end_tag_name.is_empty() && end_tag_name != back.tag_name {
            return Err(format!(
                "Unmatched end tag at byte offset {}, expected \"</{}>\", found \"</{}>\"",
                begin_pos,
                String::from_utf8_lossy(&back.tag_name),
                String::from_utf8_lossy(&end_tag_name)
            ));
        }

        let HtmlNestedEntity {
            tag_name,
            argument,
            date,
            entity_offset,
            entity_begin_pos,
        } = nested.pop().unwrap();
        if utf16_offset > entity_offset {
            let entity_length = utf16_offset - entity_offset;
            match tag_name.as_slice() {
                b"i" | b"em" => entities.push(TextEntity::new(EType::Italic, entity_offset, entity_length)),
                b"b" | b"strong" => entities.push(TextEntity::new(EType::Bold, entity_offset, entity_length)),
                b"s" | b"strike" | b"del" => {
                    entities.push(TextEntity::new(EType::Strikethrough, entity_offset, entity_length))
                }
                b"u" | b"ins" => entities.push(TextEntity::new(EType::Underline, entity_offset, entity_length)),
                b"tg-spoiler" | b"span" => {
                    entities.push(TextEntity::new(EType::Spoiler, entity_offset, entity_length))
                }
                b"tg-emoji" => {
                    let custom_emoji_id = to_integer_safe_i64(&argument).unwrap_or(0);
                    if custom_emoji_id == 0 {
                        return Err("Invalid custom emoji identifier specified".to_string());
                    }
                    let mut entity = TextEntity::new(EType::CustomEmoji, entity_offset, entity_length);
                    entity.custom_emoji_id = custom_emoji_id;
                    entities.push(entity);
                }
                b"tg-time" => {
                    let formatted_date = get_formatted_date(date, &argument)?;
                    if date > 0 {
                        let mut entity = TextEntity::new(EType::FormattedDate, entity_offset, entity_length);
                        entity.date = formatted_date.0;
                        entity.date_flags = formatted_date.1;
                        entities.push(entity);
                    }
                }
                b"a" => {
                    let url = if argument.is_empty() {
                        buf[entity_begin_pos..result_end].to_vec()
                    } else {
                        argument
                    };
                    if let Some(user_id) = get_link_user_id(&url) {
                        let mut entity = TextEntity::new(EType::MentionName, entity_offset, entity_length);
                        entity.user_id = user_id;
                        entities.push(entity);
                    } else if let Some(checked) = get_checked_link(&url) {
                        let mut entity = TextEntity::new(EType::TextUrl, entity_offset, entity_length);
                        entity.argument = bytes_to_string(checked);
                        entities.push(entity);
                    }
                }
                b"pre" => {
                    let merges = entities.last().map_or(false, |last| {
                        last.etype == EType::Code
                            && last.offset == entity_offset
                            && last.length == entity_length
                            && !last.argument.is_empty()
                    });
                    if merges {
                        entities.last_mut().unwrap().etype = EType::PreCode;
                    } else {
                        entities.push(TextEntity::new(EType::Pre, entity_offset, entity_length));
                    }
                }
                b"code" => {
                    let merges = entities.last().map_or(false, |last| {
                        last.etype == EType::Pre
                            && last.offset == entity_offset
                            && last.length == entity_length
                            && !argument.is_empty()
                    });
                    if merges {
                        let last = entities.last_mut().unwrap();
                        last.etype = EType::PreCode;
                        last.argument = bytes_to_string(argument);
                    } else {
                        let mut entity = TextEntity::new(EType::Code, entity_offset, entity_length);
                        entity.argument = bytes_to_string(argument);
                        entities.push(entity);
                    }
                }
                b"blockquote" => {
                    let etype = if argument.is_empty() {
                        EType::BlockQuote
                    } else {
                        EType::ExpandableBlockQuote
                    };
                    entities.push(TextEntity::new(etype, entity_offset, entity_length));
                }
                _ => unreachable!(),
            }
        }
        i += 1;
    }

    if let Some(back) = nested.last() {
        return Err(format!(
            "Can't find end tag corresponding to start tag \"{}\"",
            String::from_utf8_lossy(&back.tag_name)
        ));
    }

    for entity in &mut entities {
        if entity.etype == EType::Code && !entity.argument.is_empty() {
            entity.argument.clear();
        }
    }

    sort_entities(&mut entities);
    buf.truncate(result_end);
    if need_recheck_utf8 && std::str::from_utf8(&buf).is_err() {
        return Err("Text contains invalid Unicode characters after decoding HTML entities, check for unmatched surrogate code units".to_string());
    }
    Ok((bytes_to_string(buf), entities))
}

pub(crate) fn formatting_type_json(date_flags: i32) -> Option<String> {
    if date_flags == 0 {
        return None;
    }
    if date_flags & DATE_RELATIVE != 0 {
        return Some("{\"@type\":\"dateTimeFormattingTypeRelative\"}".to_string());
    }
    let time_precision = if date_flags & DATE_SHORT_TIME != 0 {
        "Short"
    } else if date_flags & DATE_LONG_TIME != 0 {
        "Long"
    } else {
        "None"
    };
    let date_precision = if date_flags & DATE_SHORT_DATE != 0 {
        "Short"
    } else if date_flags & DATE_LONG_DATE != 0 {
        "Long"
    } else {
        "None"
    };
    Some(format!(
        "{{\"@type\":\"dateTimeFormattingTypeAbsolute\",\"time_precision\":{{\"@type\":\"dateTimePartPrecision{time_precision}\"}},\"date_precision\":{{\"@type\":\"dateTimePartPrecision{date_precision}\"}},\"show_day_of_week\":{}}}",
        date_flags & DATE_DAY_OF_WEEK != 0
    ))
}

pub(crate) fn entity_type_json(entity: &TextEntity) -> String {
    match entity.etype {
        EType::MediaTimestamp => format!(
            "{{\"@type\":\"textEntityTypeMediaTimestamp\",\"media_timestamp\":{}}}",
            entity.media_timestamp
        ),
        EType::PreCode | EType::TextUrl => {
            let (name, key) = if entity.etype == EType::PreCode {
                ("textEntityTypePreCode", "language")
            } else {
                ("textEntityTypeTextUrl", "url")
            };
            let mut out = format!("{{\"@type\":\"{}\",\"{}\":\"", name, key);
            json_escape(&entity.argument, &mut out);
            out.push_str("\"}");
            out
        }
        EType::MentionName => format!(
            "{{\"@type\":\"textEntityTypeMentionName\",\"user_id\":{}}}",
            entity.user_id
        ),
        EType::CustomEmoji => format!(
            "{{\"@type\":\"textEntityTypeCustomEmoji\",\"custom_emoji_id\":\"{}\"}}",
            entity.custom_emoji_id
        ),
        EType::FormattedDate => {
            let mut out = format!(
                "{{\"@type\":\"textEntityTypeDateTime\",\"unix_time\":{}",
                entity.date
            );
            if let Some(formatting_type) = formatting_type_json(entity.date_flags) {
                out.push_str(",\"formatting_type\":");
                out.push_str(&formatting_type);
            }
            out.push('}');
            out
        }
        _ => format!("{{\"@type\":\"{}\"}}", entity.etype.type_name()),
    }
}

pub(crate) fn formatted_text_json(text: &str, entities: &[TextEntity]) -> String {
    let mut out = String::from("{\"@type\":\"formattedText\",\"text\":\"");
    json_escape(text, &mut out);
    out.push_str("\",\"entities\":[");
    for (index, entity) in entities.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"@type\":\"textEntity\",\"offset\":{},\"length\":{},\"type\":",
            entity.offset, entity.length
        ));
        out.push_str(&entity_type_json(entity));
        out.push('}');
    }
    out.push_str("]}");
    out
}

pub(crate) fn json_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "Null",
        Json::Bool(_) => "Boolean",
        Json::Int(..) | Json::Float(..) | Json::Num(_) => "Number",
        Json::Str(_) => "String",
        Json::Arr(_) => "Array",
        Json::Obj(_) => "Object",
    }
}

pub(crate) fn json_int64_field(value: &Json) -> Result<i64, String> {
    let owned;
    let literal: &str = match value {
        Json::Null => return Ok(0),
        Json::Int(_, literal) | Json::Float(_, literal) | Json::Num(literal) => literal,
        Json::Str(bytes) => {
            // detail::get_to_integer_safe_error checks UTF-8 before quoting the literal
            owned = match std::str::from_utf8(bytes) {
                Ok(text) => text.to_string(),
                Err(_) => return Err(UTF8_MESSAGE.to_string()),
            };
            &owned
        }
        other => {
            return Err(format!(
                "Expected String or Number, but receive {}",
                json_type_name(other)
            ))
        }
    };
    match to_integer_safe_i64(literal.as_bytes()) {
        Some(number) => Ok(number),
        None => Err(format!("Can't parse as an integer string \"{}\"", literal)),
    }
}

/// td_api constructor identifiers, which the JSON layer also accepts as "@type"
pub(crate) const TEXT_ENTITY_TYPE_CONSTRUCTORS: [(i32, &str); 24] = [
    (934535013, "textEntityTypeMention"),
    (-1023958307, "textEntityTypeHashtag"),
    (1222915915, "textEntityTypeCashtag"),
    (-1150997581, "textEntityTypeBotCommand"),
    (-1312762756, "textEntityTypeUrl"),
    (1425545249, "textEntityTypeEmailAddress"),
    (-1160140246, "textEntityTypePhoneNumber"),
    (105986320, "textEntityTypeBankCardNumber"),
    (516305059, "textEntityTypeTonAddress"),
    (-1128210000, "textEntityTypeBold"),
    (-118253987, "textEntityTypeItalic"),
    (792317842, "textEntityTypeUnderline"),
    (961529082, "textEntityTypeStrikethrough"),
    (544019899, "textEntityTypeSpoiler"),
    (-974534326, "textEntityTypeCode"),
    (1648958606, "textEntityTypePre"),
    (-945325397, "textEntityTypePreCode"),
    (-1003999032, "textEntityTypeBlockQuote"),
    (36572261, "textEntityTypeExpandableBlockQuote"),
    (445719651, "textEntityTypeTextUrl"),
    (-1570974289, "textEntityTypeMentionName"),
    (1724820677, "textEntityTypeCustomEmoji"),
    (-1841898992, "textEntityTypeMediaTimestamp"),
    (-1544268588, "textEntityTypeDateTime"),
];

pub(crate) const DATE_TIME_FORMATTING_CONSTRUCTORS: [(i32, &str); 5] = [
    (-648483424, "dateTimeFormattingTypeRelative"),
    (47463317, "dateTimeFormattingTypeAbsolute"),
    (1790302111, "dateTimePartPrecisionNone"),
    (1290158159, "dateTimePartPrecisionShort"),
    (-105378824, "dateTimePartPrecisionLong"),
];

pub(crate) fn json_class_name<'a>(value: &'a Json, constructors: &[(i32, &'static str)]) -> Result<&'a str, String> {
    match value.get("@type") {
        Some(Json::Str(bytes)) => {
            Ok(std::str::from_utf8(bytes).unwrap_or("\u{fffd}"))
        }
        Some(Json::Int(_, literal)) | Some(Json::Float(_, literal)) | Some(Json::Num(literal)) => {
            let constructor = to_integer_i32(literal.as_bytes());
            match constructors.iter().find(|(id, _)| *id == constructor) {
                Some((_, name)) => Ok(name),
                None => Err(format!("Unknown constructor 0x{:08x}", constructor as u32)),
            }
        }
        None => Err("Can't find field \"@type\"".to_string()),
        Some(other) => Err(format!(
            "Expected String or Integer, but receive {}",
            json_type_name(other)
        )),
    }
}

pub(crate) fn json_nullable_class_name<'a>(
    value: Option<&'a Json>,
    constructors: &[(i32, &'static str)],
) -> Result<Option<&'a str>, String> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(value) => json_class_name(value, constructors).map(Some),
    }
}

/// FormattedDate::get_formatted_date(date, DateTimeFormattingType)
pub(crate) fn formatting_type_flags(value: Option<&Json>) -> Result<i32, String> {
    let Some(value) = value.filter(|value| !matches!(value, Json::Null)) else {
        return Ok(0);
    };
    let class_name = json_class_name(value, &DATE_TIME_FORMATTING_CONSTRUCTORS)?;
    if class_name == "dateTimeFormattingTypeRelative" {
        return Ok(DATE_RELATIVE);
    }
    if class_name != "dateTimeFormattingTypeAbsolute" {
        return Err(format!("Unknown class \"{}\"", class_name));
    }

    let mut date_flags = 0;
    if let Some(precision) = json_nullable_class_name(value.get("time_precision"), &DATE_TIME_FORMATTING_CONSTRUCTORS)? {
        date_flags |= match precision {
            "dateTimePartPrecisionNone" => 0,
            "dateTimePartPrecisionShort" => DATE_SHORT_TIME,
            "dateTimePartPrecisionLong" => DATE_LONG_TIME,
            other => return Err(format!("Unknown class \"{}\"", other)),
        };
    }
    if let Some(precision) = json_nullable_class_name(value.get("date_precision"), &DATE_TIME_FORMATTING_CONSTRUCTORS)? {
        date_flags |= match precision {
            "dateTimePartPrecisionNone" => 0,
            "dateTimePartPrecisionShort" => DATE_SHORT_DATE,
            "dateTimePartPrecisionLong" => DATE_LONG_DATE,
            other => return Err(format!("Unknown class \"{}\"", other)),
        };
    }
    if matches!(value.get("show_day_of_week"), Some(Json::Bool(true))) {
        date_flags |= DATE_DAY_OF_WEEK;
    }
    Ok(date_flags)
}

/// get_message_entities for td_api::textEntity objects taken from a JSON request
pub(crate) fn get_message_entities(entities: Option<&Json>, allow_all: bool) -> Result<Vec<TextEntity>, String> {
    let format_error =
        |message: String| format!("Failed to parse JSON object as TDLib request: {}", message);

    let mut result = Vec::new();
    let items = match entities {
        None | Some(Json::Null) => return Ok(result),
        Some(Json::Arr(items)) => items,
        Some(other) => {
            return Err(format_error(format!(
                "Expected Array, but receive {}",
                json_type_name(other)
            )))
        }
    };
    for item in items {
        if matches!(item, Json::Null) {
            continue;
        }
        if !matches!(item, Json::Obj(_)) {
            return Err(format_error(format!(
                "Expected Object, but receive {}",
                json_type_name(item)
            )));
        }

        // a missing or null "type" makes the entity unusable
        let Some(entity_type) = item.get("type") else {
            continue;
        };
        if matches!(entity_type, Json::Null) {
            continue;
        }

        let offset = match item.get("offset") {
            None => 0,
            Some(value) => json_int32_field(value).map_err(format_error)?,
        };
        let length = match item.get("length") {
            None => 0,
            Some(value) => json_int32_field(value).map_err(format_error)?,
        };

        let entity = match json_class_name(entity_type, &TEXT_ENTITY_TYPE_CONSTRUCTORS).map_err(format_error)? {
            "textEntityTypeMention" => TextEntity::new(EType::Mention, offset, length),
            "textEntityTypeHashtag" => TextEntity::new(EType::Hashtag, offset, length),
            "textEntityTypeBotCommand" => TextEntity::new(EType::BotCommand, offset, length),
            "textEntityTypeUrl" => TextEntity::new(EType::Url, offset, length),
            "textEntityTypeEmailAddress" => TextEntity::new(EType::EmailAddress, offset, length),
            "textEntityTypeCashtag" => TextEntity::new(EType::Cashtag, offset, length),
            "textEntityTypePhoneNumber" => TextEntity::new(EType::PhoneNumber, offset, length),
            "textEntityTypeBankCardNumber" => TextEntity::new(EType::BankCardNumber, offset, length),
            "textEntityTypeBold" => TextEntity::new(EType::Bold, offset, length),
            "textEntityTypeItalic" => TextEntity::new(EType::Italic, offset, length),
            "textEntityTypeUnderline" => TextEntity::new(EType::Underline, offset, length),
            "textEntityTypeStrikethrough" => TextEntity::new(EType::Strikethrough, offset, length),
            "textEntityTypeBlockQuote" => TextEntity::new(EType::BlockQuote, offset, length),
            "textEntityTypeCode" => TextEntity::new(EType::Code, offset, length),
            "textEntityTypePre" => TextEntity::new(EType::Pre, offset, length),
            "textEntityTypePreCode" => {
                let mut language = entity_type
                    .get("language")
                    .and_then(Json::as_bytes)
                    .unwrap_or(b"")
                    .to_vec();
                if !clean_input_string(&mut language) {
                    return Err("MessageEntityPreCode.language must be encoded in UTF-8".to_string());
                }
                let mut entity = TextEntity::new(EType::PreCode, offset, length);
                entity.argument = bytes_to_string(language);
                entity
            }
            "textEntityTypeTextUrl" => {
                let mut url = entity_type
                    .get("url")
                    .and_then(Json::as_bytes)
                    .unwrap_or(b"")
                    .to_vec();
                if !clean_input_string(&mut url) {
                    return Err("MessageEntityTextUrl.url must be encoded in UTF-8".to_string());
                }
                if let Some(user_id) = get_link_user_id(&url) {
                    let mut entity = TextEntity::new(EType::MentionName, offset, length);
                    entity.user_id = user_id;
                    entity
                } else {
                    let checked = check_link(&url).map_err(|message| format!("Entity {}", message))?;
                    let mut entity = TextEntity::new(EType::TextUrl, offset, length);
                    entity.argument = bytes_to_string(checked);
                    entity
                }
            }
            "textEntityTypeMentionName" => {
                let user_id = match entity_type.get("user_id") {
                    None => 0,
                    Some(value) => json_int64_field(value).map_err(format_error)?,
                };
                let mut entity = TextEntity::new(EType::MentionName, offset, length);
                entity.user_id = user_id;
                entity
            }
            "textEntityTypeMediaTimestamp" => {
                let media_timestamp = match entity_type.get("media_timestamp") {
                    None => 0,
                    Some(value) => json_int32_field(value).map_err(format_error)?,
                };
                if media_timestamp < 0 {
                    return Err("Invalid media timestamp specified".to_string());
                }
                let mut entity = TextEntity::new(EType::MediaTimestamp, offset, length);
                entity.media_timestamp = media_timestamp;
                entity
            }
            "textEntityTypeSpoiler" => TextEntity::new(EType::Spoiler, offset, length),
            "textEntityTypeCustomEmoji" => {
                let custom_emoji_id = match entity_type.get("custom_emoji_id") {
                    None => 0,
                    Some(value) => json_int64_field(value).map_err(format_error)?,
                };
                if custom_emoji_id == 0 {
                    return Err("Invalid custom emoji identifier specified".to_string());
                }
                let mut entity = TextEntity::new(EType::CustomEmoji, offset, length);
                entity.custom_emoji_id = custom_emoji_id;
                entity
            }
            "textEntityTypeExpandableBlockQuote" => {
                TextEntity::new(EType::ExpandableBlockQuote, offset, length)
            }
            "textEntityTypeDateTime" => {
                let date = match entity_type.get("unix_time") {
                    None => 0,
                    Some(value) => json_int32_field(value).map_err(format_error)?,
                };
                if date <= 0 {
                    return Err("Invalid date specified".to_string());
                }
                let date_flags = formatting_type_flags(entity_type.get("formatting_type"))
                    .map_err(format_error)?;
                let mut entity = TextEntity::new(EType::FormattedDate, offset, length);
                entity.date = date;
                entity.date_flags = date_flags;
                entity
            }
            "textEntityTypeTonAddress" => TextEntity::new(EType::TonAddress, offset, length),
            other => return Err(format_error(format!("Unknown class \"{}\"", other))),
        };

        if !allow_all && !is_user_entity(entity.etype) {
            continue;
        }
        result.push(entity);
    }
    Ok(result)
}

pub(crate) fn json_int32_field(value: &Json) -> Result<i32, String> {
    let owned;
    let literal: &str = match value {
        Json::Null => return Ok(0),
        Json::Int(_, literal) | Json::Float(_, literal) | Json::Num(literal) => literal,
        Json::Str(bytes) => {
            // detail::get_to_integer_safe_error checks UTF-8 before quoting the literal
            owned = match std::str::from_utf8(bytes) {
                Ok(text) => text.to_string(),
                Err(_) => return Err(UTF8_MESSAGE.to_string()),
            };
            &owned
        }
        other => return Err(format!("Expected Number, but receive {}", json_type_name(other))),
    };
    match to_integer_safe_i32(literal.as_bytes()) {
        Some(number) => Ok(number),
        None => Err(format!("Can't parse as an integer string \"{}\"", literal)),
    }
}

pub(crate) fn parse_entities_error(message: &str) -> String {
    error_object(400, &format!("Can't parse entities: {}", message))
}

/// the formattedText argument shared by parseMarkdown and getMarkdownText
pub(crate) fn read_formatted_text(request: &Json, allow_all: bool) -> Result<(String, Vec<TextEntity>), String> {
    let format_error =
        |message: String| format!("Failed to parse JSON object as TDLib request: {}", message);
    let text_object = match request.get("text") {
        None | Some(Json::Null) => return Err("Text must be non-empty".to_string()),
        Some(text @ Json::Obj(_)) => text,
        Some(other) => {
            return Err(format_error(format!(
                "Expected Object, but receive {}",
                json_type_name(other)
            )))
        }
    };
    let text = match text_object.get("text") {
        None | Some(Json::Null) => String::new(),
        Some(Json::Str(bytes)) => match std::str::from_utf8(bytes) {
            Ok(value) => value.to_string(),
            Err(_) => return Err("Strings must be encoded in UTF-8".to_string()),
        },
        Some(other) => {
            return Err(format_error(format!(
                "Expected String, but receive {}",
                json_type_name(other)
            )))
        }
    };
    let entities = get_message_entities(text_object.get("entities"), allow_all)?;
    Ok((text, entities))
}

pub(crate) fn parse_markdown_request(request: &Json) -> String {
    let (mut text, mut entities) = match read_formatted_text(request, true) {
        Ok(text) => text,
        Err(message) => return error_object(400, &message),
    };
    if let Err(message) = fix_formatted_text(&mut text, &mut entities) {
        return error_object(400, &message);
    }

    let (text, entities) = parse_markdown_v3(text.as_bytes(), entities);
    let mut text = bytes_to_string(text);
    let mut entities = entities;
    if let Err(message) = fix_formatted_text(&mut text, &mut entities) {
        return error_object(400, &message);
    }
    formatted_text_json(&text, &entities)
}

pub(crate) fn get_markdown_text_request(request: &Json) -> String {
    let (mut text, mut entities) = match read_formatted_text(request, false) {
        Ok(text) => text,
        Err(message) => return error_object(400, &message),
    };
    if let Err(message) = fix_formatted_text(&mut text, &mut entities) {
        return error_object(400, &message);
    }

    let (text, entities) = get_markdown_v3(text, entities);
    formatted_text_json(&text, &entities)
}

// td_api::textParseMode::ID values, used when "@type" is given as a number
pub(crate) const TEXT_PARSE_MODE_HTML_ID: i32 = 1660208627;
pub(crate) const TEXT_PARSE_MODE_MARKDOWN_ID: i32 = 360073407;

pub(crate) fn parse_text_entities(request: &Json) -> String {
    let format_error = |message: String| {
        error_object(
            400,
            &format!("Failed to parse JSON object as TDLib request: {}", message),
        )
    };
    let text = match request.get("text") {
        None | Some(Json::Null) => String::new(),
        Some(Json::Str(bytes)) => match std::str::from_utf8(bytes) {
            Ok(text) => text.to_string(),
            Err(_) => return error_object(400, "Text must be encoded in UTF-8"),
        },
        Some(other) => {
            return format_error(format!(
                "Expected String, but receive {}",
                json_type_name(other)
            ))
        }
    };
    let parse_mode = match request.get("parse_mode") {
        None | Some(Json::Null) => return error_object(400, "Parse mode must be non-empty"),
        Some(parse_mode @ Json::Obj(_)) => parse_mode,
        Some(other) => {
            return format_error(format!(
                "Expected Object, but receive {}",
                json_type_name(other)
            ))
        }
    };
    let unknown_constructor = |constructor: i32| {
        format_error(format!("Unknown constructor 0x{:08x}", constructor as u32))
    };
    let is_html = match parse_mode.get("@type") {
        Some(Json::Str(bytes)) => match std::str::from_utf8(bytes) {
            Err(_) => return format_error("Strings must be encoded in UTF-8".to_string()),
            Ok(name) => match name {
                "textParseModeHTML" => true,
                "textParseModeMarkdown" => false,
                other => return format_error(format!("Unknown class \"{}\"", other)),
            },
        },
        Some(Json::Int(_, literal)) | Some(Json::Float(_, literal)) | Some(Json::Num(literal)) => {
            match to_integer_i32(literal.as_bytes()) {
                TEXT_PARSE_MODE_HTML_ID => true,
                TEXT_PARSE_MODE_MARKDOWN_ID => false,
                other => return unknown_constructor(other),
            }
        }
        Some(other) => {
            return format_error(format!(
                "Expected String or Integer, but receive {}",
                json_type_name(other)
            ))
        }
        None => return format_error("Can't find field \"@type\"".to_string()),
    };
    let markdown_version = if is_html {
        None
    } else {
        match parse_mode.get("version") {
            None => Some(0),
            Some(version) => match json_int32_field(version) {
                Ok(version) => Some(version),
                Err(message) => return format_error(message),
            },
        }
    };

    // check the length before choosing the parser, as SynchronousRequests does
    if text.chars().count() > 65536 {
        return parse_entities_error("Text is too long");
    }

    let parsed = match markdown_version {
        None => parse_html(text.as_bytes()),
        Some(0 | 1) => parse_markdown(text.as_bytes()),
        Some(2) => parse_markdown_v2(text.as_bytes()),
        Some(_) => return parse_entities_error("Wrong Markdown version specified"),
    };
    match parsed {
        Ok((text, entities)) => formatted_text_json(&text, &entities),
        Err(message) => parse_entities_error(&message),
    }
}
