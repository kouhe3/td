// 拆分自 `rust/src/lib.rs`（一次性 codemod，见 tasks/plan.md T119）；只改可见性与位置，未改实现。
#![allow(unused_imports)]

use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{LazyLock, Mutex};

pub(crate) struct LogState {
    level: i32,
    stream: &'static str,
    stream_path: String,
    stream_max_file_size: i64,
    stream_redirect_stderr: bool,
    tag_levels: [(&'static str, i32); 20],
}

pub(crate) static LOG_STATE: std::sync::Mutex<LogState> = std::sync::Mutex::new(LogState {
    level: 5,
    stream: "logStreamDefault",
    stream_path: String::new(),
    stream_max_file_size: 0,
    stream_redirect_stderr: false,
    tag_levels: [
        ("actor", 14),
        ("binlog", 12),
        ("config_recoverer", 3),
        ("connections", 3),
        ("dc", 6),
        ("dns_resolver", 4),
        ("fd", 13),
        ("file_gc", 3),
        ("file_loader", 4),
        ("file_references", 3),
        ("get_difference", 3),
        ("mtproto", 11),
        ("net_query", 3),
        ("notifications", 3),
        ("proxy", 4),
        ("raw_mtproto", 14),
        ("sqlite", 14),
        ("td_init", 7),
        ("td_requests", 3),
        ("update_file", 3),
    ],
});

pub(crate) fn check_language_pack_name(name: &str) -> bool {
    name.len() <= 64 && name.bytes().all(|byte| byte == b'_' || byte.is_ascii_alphabetic())
}

pub(crate) fn check_language_code_name(name: &str) -> bool {
    let valid_chars = name
        .bytes()
        .all(|byte| byte == b'-' || byte.is_ascii_alphanumeric());
    valid_chars
        && name.len() <= 64
        && (name.starts_with('X') || name.is_empty() || name.len() >= 2)
}

pub(crate) fn is_valid_language_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

/// `LanguagePackManager::add_language_database` + `SqliteKeyValue`: the language pack is a SQLite
/// database (`PRAGMA journal_mode=WAL`, no encryption) with one `kv_<target>_<code>` table per language.
pub(crate) struct LanguagePackDatabase {
    connection: rusqlite::Connection,
    table: String,
}

impl LanguagePackDatabase {
    pub(crate) fn open(path: &str, language_pack: &str, language_code: &str) -> Option<LanguagePackDatabase> {
        let table = format!("\"kv_{}_{}\"", language_pack, language_code);
        let connection = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
        )
        .ok()?;
        connection
            .execute(&format!("CREATE TABLE IF NOT EXISTS {} (k BLOB PRIMARY KEY, v BLOB)", table), [])
            .ok()?;
        Some(LanguagePackDatabase { connection, table })
    }

    pub(crate) fn get(&self, key: &str) -> Option<Vec<u8>> {
        let mut statement = self
            .connection
            .prepare(&format!("SELECT v FROM {} WHERE k = ?1", self.table))
            .ok()?;
        // SqliteKeyValue binds both key and value as blobs; TEXT never equals BLOB in SQLite
        let mut rows = statement.query([key.as_bytes()]).ok()?;
        match rows.next() {
            Ok(Some(row)) => row.get::<_, Vec<u8>>(0).ok(),
            _ => None,
        }
    }
}

/// `td::full_split`: empty input yields no parts, a trailing delimiter yields a final empty part.
pub(crate) fn split_nul(bytes: &[u8]) -> Vec<&[u8]> {
    if bytes.is_empty() {
        return Vec::new();
    }
    bytes.split(|byte| *byte == 0).collect()
}

/// `LanguagePackManager::load_database_language_version`: empty or missing means "no version" (-1).
pub(crate) fn database_language_version(value: Option<Vec<u8>>) -> i32 {
    match value {
        None => -1,
        Some(value) if value.is_empty() => -1,
        // td::to_integer<int32> returns T() on malformed input
        Some(value) => std::str::from_utf8(&value)
            .ok()
            .and_then(|text| text.parse::<i32>().ok())
            .unwrap_or(0),
    }
}

pub(crate) fn deleted_language_pack_string_json() -> String {
    "{\"@type\":\"languagePackStringValueDeleted\"}".to_string()
}

/// `load_language_string_unsafe` + `get_language_pack_string_value_object`: the stored blob is
/// `1<ordinary>`, `2<zero>\0<one>\0<two>\0<few>\0<many>\0<other>`, or anything else for "deleted".
pub(crate) fn language_pack_string_value_json(value: &[u8]) -> String {
    match value[0] {
        b'1' => {
            let mut body = String::new();
            json_escape(&String::from_utf8_lossy(&value[1..]), &mut body);
            format!("{{\"@type\":\"languagePackStringValueOrdinary\",\"value\":\"{}\"}}", body)
        }
        b'2' => {
            let parts = split_nul(&value[1..]);
            if parts.len() != 6 {
                return deleted_language_pack_string_json();
            }
            let names = ["zero_value", "one_value", "two_value", "few_value", "many_value", "other_value"];
            let mut body = String::new();
            for (index, part) in parts.iter().enumerate() {
                if index > 0 {
                    body.push(',');
                }
                body.push('"');
                body.push_str(names[index]);
                body.push_str("\":\"");
                json_escape(&String::from_utf8_lossy(part), &mut body);
                body.push('"');
            }
            format!("{{\"@type\":\"languagePackStringValuePluralized\",{}}}", body)
        }
        _ => deleted_language_pack_string_json(),
    }
}

pub(crate) fn language_pack_string_json(path: &str, language_pack: &str, language_code: &str, key: &str) -> String {
    if path.is_empty() {
        // add_language_database("") opens no database at all
        return error_object(404, "Not Found");
    }
    let Some(database) = LanguagePackDatabase::open(path, language_pack, language_code) else {
        // a database that can't be opened (or isn't SQLite) behaves like an empty one
        return error_object(404, "Not Found");
    };
    let version = database_language_version(database.get("!version"));
    match database.get(key) {
        // a missing key is "deleted" only when the pack is versioned; otherwise the pack isn't loaded
        None => {
            if version == -1 {
                error_object(404, "Not Found")
            } else {
                deleted_language_pack_string_json()
            }
        }
        Some(value) if value.is_empty() => {
            if version == -1 {
                error_object(404, "Not Found")
            } else {
                deleted_language_pack_string_json()
            }
        }
        Some(value) => language_pack_string_value_json(&value),
    }
}

pub(crate) fn log_verbosity_json(level: i32) -> String {
    format!("{{\"@type\":\"logVerbosityLevel\",\"verbosity_level\":{}}}", level)
}

/// `OptionManager::is_synchronous_option`: only these two are answered without a Td round trip
pub(crate) fn synchronous_option_value(name: &str) -> Option<String> {
    let value = match name {
        "version" => "1.8.71",
        "commit_hash" => env!("TD_GIT_COMMIT_HASH"),
        _ => return None,
    };
    let mut body = String::from("{\"@type\":\"optionValueString\",\"value\":\"");
    json_escape(value, &mut body);
    body.push_str("\"}");
    Some(body)
}

pub(crate) fn tag_level(tag: &str) -> Option<i32> {
    let state = LOG_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state
        .tag_levels
        .iter()
        .find(|(name, _)| *name == tag)
        .map(|(_, level)| *level)
}

/// td/utils/port/path.cpp realpath: lexical resolution, native separators, drive case kept
pub(crate) fn absolute_path(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let bytes = unified.as_bytes();
    let mut prefix = String::new();
    let mut rest = unified.as_str();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        prefix = unified[..2].to_string();
        rest = &unified[2..];
    }
    let mut parts: Vec<String> = Vec::new();
    if prefix.is_empty() && !rest.starts_with('/') {
        if let Ok(cwd) = std::env::current_dir() {
            let cwd = cwd.to_string_lossy().replace('\\', "/");
            let cwd_bytes = cwd.as_bytes();
            let mut cwd_rest = cwd.as_str();
            if cwd_bytes.len() >= 2 && cwd_bytes[1] == b':' {
                prefix = cwd[..2].to_string();
                cwd_rest = &cwd[2..];
            }
            for part in cwd_rest.split('/') {
                if !part.is_empty() && part != "." {
                    parts.push(part.to_string());
                }
            }
        }
    }
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other.to_string()),
        }
    }
    let joined = parts.join("\\");
    if prefix.is_empty() {
        format!("\\{}", joined)
    } else {
        format!("{}\\{}", prefix, joined)
    }
}

pub(crate) fn log_action(kind: &str, json: &Json) -> Option<String> {
    match kind {
        "getLogVerbosityLevel" => {
            let state = LOG_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            Some(log_verbosity_json(state.level))
        }
        "setLogVerbosityLevel" => {
            let new_level = match request_int32(json, "new_verbosity_level") {
                Ok(level) => level,
                Err(message) => return Some(request_error(&message)),
            };
            // Logging::set_verbosity_level rejects negatives (level 0 is the fatal level)
            if new_level < 0 {
                return Some(error_object(400, "Wrong new verbosity level specified"));
            }
            let mut state = LOG_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.level = new_level;
            Some(ok_object())
        }
        "getLogStream" => {
            let state = LOG_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            match state.stream {
                "logStreamEmpty" => Some("{\"@type\":\"logStreamEmpty\"}".to_string()),
                "logStreamFile" => {
                    let mut body = String::from("{\"@type\":\"logStreamFile\",\"path\":\"");
                    json_escape(&state.stream_path, &mut body);
                    body.push_str(&format!(
                        "\",\"max_file_size\":{},\"redirect_stderr\":{}}}",
                        state.stream_max_file_size,
                        if state.stream_redirect_stderr { "true" } else { "false" }
                    ));
                    Some(body)
                }
                _ => Some("{\"@type\":\"logStreamDefault\"}".to_string()),
            }
        }
        "setLogStream" => {
            let stream = match request_object(json, "log_stream") {
                Ok(stream) => stream,
                Err(message) => return Some(request_error(&message)),
            };
            let mut kind_name = "logStreamDefault";
            let mut path = String::new();
            let mut max_file_size = 0i64;
            let mut redirect_stderr = false;
            if let Some(stream) = stream {
                kind_name = match json_class_name(stream, &LOG_STREAM_CONSTRUCTORS) {
                    Ok("logStreamEmpty") => "logStreamEmpty",
                    Ok("logStreamFile") => "logStreamFile",
                    Ok(_) => "logStreamDefault",
                    Err(message) => return Some(request_error(&message)),
                };
                match request_string(stream, "path") {
                    Ok(value) => path = value,
                    Err(message) => return Some(request_error(&message)),
                }
                match request_int64(stream, "max_file_size") {
                    Ok(value) => max_file_size = value,
                    Err(message) => return Some(request_error(&message)),
                }
                match request_bool(stream, "redirect_stderr") {
                    Ok(value) => redirect_stderr = value,
                    Err(message) => return Some(request_error(&message)),
                }
            }
            if kind_name == "logStreamFile" {
                if max_file_size <= 0 {
                    return Some(error_object(400, "Max log file size must be positive"));
                }
                if path.is_empty() {
                    return Some(error_object(400, "Log file path must be non-empty"));
                }
                let opened = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .write(true)
                    .open(&path);
                if opened.is_err() {
                    return Some(error_object(
                        400,
                        &format!("File \"{}\" can't be opened/created for appending", path),
                    ));
                }
                path = absolute_path(&path);
            }
            let mut state = LOG_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.stream = match kind_name {
                "logStreamEmpty" => "logStreamEmpty",
                "logStreamFile" => "logStreamFile",
                _ => "logStreamDefault",
            };
            state.stream_path = if kind_name == "logStreamFile" {
                path.to_string()
            } else {
                String::new()
            };
            state.stream_max_file_size = if kind_name == "logStreamFile" { max_file_size } else { 0 };
            state.stream_redirect_stderr = kind_name == "logStreamFile" && redirect_stderr;
            Some(ok_object())
        }
        "getLogTags" => {
            let state = LOG_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut out = String::from("{\"@type\":\"logTags\",\"tags\":[");
            for (index, (name, _)) in state.tag_levels.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('"');
                out.push_str(name);
                out.push('"');
            }
            out.push_str("]}");
            Some(out)
        }
        "getLogTagVerbosityLevel" => {
            let tag = match request_string(json, "tag") {
                Ok(tag) => tag,
                Err(message) => return Some(request_error(&message)),
            };
            Some(match tag_level(&tag) {
                Some(level) => log_verbosity_json(level),
                None => error_object(400, "Log tag is not found"),
            })
        }
        "setLogTagVerbosityLevel" => {
            let tag = match request_string(json, "tag") {
                Ok(tag) => tag,
                Err(message) => return Some(request_error(&message)),
            };
            let new_level = match request_int32(json, "new_verbosity_level") {
                Ok(level) => level,
                Err(message) => return Some(request_error(&message)),
            };
            let mut state = LOG_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            match state.tag_levels.iter_mut().find(|(name, _)| *name == tag) {
                Some(entry) => {
                    // the oracle clamps a per-tag level to at least 1 (0 is the fatal level)
                    entry.1 = new_level.max(1);
                    Some(ok_object())
                }
                None => Some(error_object(400, "Log tag is not found")),
            }
        }
        "addLogMessage" => {
            if let Err(message) = request_int32(json, "verbosity_level") {
                return Some(request_error(&message));
            }
            if let Err(message) = request_string(json, "text") {
                return Some(request_error(&message));
            }
            Some(ok_object())
        }
        _ => None,
    }
}

pub(crate) fn is_allowed_quote_entity_type(entity_type: EType) -> bool {
    matches!(
        entity_type,
        EType::Bold
            | EType::Italic
            | EType::Underline
            | EType::Strikethrough
            | EType::Spoiler
            | EType::CustomEmoji
            | EType::FormattedDate
    )
}

pub(crate) fn process_quote_entities(entities: &mut Vec<TextEntity>, length: i32) {
    entities.retain(|entity| is_allowed_quote_entity_type(entity.etype));
    entities.retain(|entity| {
        entity.offset >= 0
            && entity.offset < length
            && entity.length > 0
            && entity.length <= length - entity.offset
    });
    remove_empty_entities(entities);
    fix_entities(entities);
    remove_empty_entities(entities);
}

pub(crate) fn search_quote(
    text: &str,
    quote: &str,
    quote_position: i32,
    text_entities: &mut Vec<TextEntity>,
    quote_entities: &mut Vec<TextEntity>,
) -> i32 {
    let length = text.chars().map(char::len_utf16).sum::<usize>() as i32;
    let quote_length = quote.chars().map(char::len_utf16).sum::<usize>() as i32;
    if quote_length == 0 || quote_length > length {
        return -1;
    }
    process_quote_entities(text_entities, length);
    process_quote_entities(quote_entities, quote_length);

    let quote_position = quote_position.clamp(0, length - 1);
    let mut byte_positions: Vec<Option<usize>> = Vec::with_capacity(length as usize);
    for (byte_index, c) in text.char_indices() {
        byte_positions.push(Some(byte_index));
        if c as u32 >= 0x10000 {
            byte_positions.push(None);
        }
    }
    if byte_positions.len() != length as usize {
        return -1;
    }
    let quote_bytes = quote.as_bytes();
    let check_position = |position: i32| -> bool {
        if position < 0 || position > length - quote_length {
            return false;
        }
        let Some(Some(byte_position)) = byte_positions.get(position as usize) else {
            return false;
        };
        text.as_bytes()
            .get(*byte_position..*byte_position + quote_bytes.len())
            == Some(quote_bytes)
    };
    let mut i = 0i32;
    while quote_position - i >= 0 || quote_position + i + 1 <= length - quote_length {
        if check_position(quote_position - i) {
            return quote_position - i;
        }
        if check_position(quote_position + i + 1) {
            return quote_position + i + 1;
        }
        i += 1;
    }
    -1
}

pub(crate) fn search_quote_request(json: &Json) -> String {
    let read = |key: &str| -> Result<(String, Vec<TextEntity>), String> {
        match json.get(key) {
            None | Some(Json::Null) => Err("Text and quote must be non-empty".to_string()),
            Some(value) => {
                let wrapper = Json::Obj(vec![("text".to_string(), value.clone())]);
                read_formatted_text(&wrapper, false)
            }
        }
    };
    let (text, mut text_entities) = match read("text") {
        Ok(value) => value,
        Err(message) => return error_object(400, &message),
    };
    let (quote, mut quote_entities) = match read("quote") {
        Ok(value) => value,
        Err(message) => return error_object(400, &message),
    };
    let quote_position = match request_int32(json, "quote_position") {
        Ok(position) => position,
        Err(message) => return request_error(&message),
    };
    let position = search_quote(&text, &quote, quote_position, &mut text_entities, &mut quote_entities);
    if position == -1 {
        return error_object(404, "Not Found");
    }
    format!("{{\"@type\":\"foundPosition\",\"position\":{}}}", position)
}

/// td/telegram/SynchronousRequests.cpp: the actions `td_execute` runs without a client
pub(crate) const SYNCHRONOUS_ACTIONS: [&str; 29] = [
    "searchQuote",
    "getTextEntities",
    "getTextEmojiRanges",
    "parseTextEntities",
    "parseMarkdown",
    "getMarkdownText",
    "searchStringsByPrefix",
    "checkQuickReplyShortcutName",
    "getCountryFlagEmoji",
    "getFileMimeType",
    "getFileExtension",
    "cleanFileName",
    "getLanguagePackString",
    "getPhoneNumberInfoSync",
    "getChatFolderDefaultIconName",
    "getJsonValue",
    "getJsonString",
    "getThemeParametersJsonString",
    "getPushReceiverId",
    "setLogStream",
    "getLogStream",
    "setLogVerbosityLevel",
    "getLogVerbosityLevel",
    "getLogTags",
    "setLogTagVerbosityLevel",
    "getLogTagVerbosityLevel",
    "addLogMessage",
    "testReturnError",
    "getOption",
];

pub(crate) const LOG_STREAM_CONSTRUCTORS: [(i32, &str); 3] = [
    (1390581436, "logStreamDefault"),
    (1532136933, "logStreamFile"),
    (-499912244, "logStreamEmpty"),
];

/// td/tl/tl_json.h: a missing or null field keeps its default, a wrong type is an error
pub(crate) fn request_error(message: &str) -> String {
    error_object(
        400,
        &format!("Failed to parse JSON object as TDLib request: {}", message),
    )
}

pub(crate) const UTF8_MESSAGE: &str = "Strings must be encoded in UTF-8";

/// Marks an undecodable "@type": ClientJson::from_response replaces such a message with UTF8_MESSAGE
/// and drops the parse prefix, while a field-level UTF-8 error keeps it (F092).
pub(crate) const BARE_UTF8_SENTINEL: &str = "\u{1}undecodable-@type";

pub(crate) fn validation_error(message: &str) -> String {
    if message == BARE_UTF8_SENTINEL {
        error_object(400, UTF8_MESSAGE)
    } else {
        request_error(message)
    }
}

pub(crate) fn expected_type_error(expected: &str, value: &Json) -> String {
    format!("Expected {}, but receive {}", expected, json_type_name(value))
}

// F081: td_execute parses the whole request object before deciding that the method is not
// synchronous. This table-driven validator covers the top-level fields of all 1114 functions;
// nested object fields are validated recursively as td::from_json does (F084).
pub(crate) fn validate_arguments(name: &str, json: &Json) -> Result<(), String> {
    let Some((_, fields)) = tl_schema::ARGUMENTS.iter().find(|(known, _)| *known == name) else {
        return Ok(());
    };
    validate_fields(json, fields)
}

pub(crate) fn validate_fields(json: &Json, fields: &[(&str, &str)]) -> Result<(), String> {
    for (field, type_name) in fields.iter() {
        let Some(value) = json.get(field) else {
            continue;
        };
        if matches!(value, Json::Null) {
            continue;
        }
        validate_value(value, type_name)?;
    }
    Ok(())
}

pub(crate) fn validate_value(value: &Json, type_name: &str) -> Result<(), String> {
    // every td::from_json overload treats Null as "keep the default", vectors included
    if matches!(value, Json::Null) {
        return Ok(());
    }
    let base = type_name.trim_end_matches('?');
    if let Some(inner) = base.strip_prefix("vector<").and_then(|rest| rest.strip_suffix('>')) {
        return match value {
            Json::Arr(items) => items.iter().try_for_each(|item| validate_value(item, inner)),
            other => Err(expected_type_error("Array", other)),
        };
    }
    match base {
        "int32" => json_int32_field(value).map(|_| ()),
        "int53" | "int64" => json_int64_field(value).map(|_| ()),
        "string" | "bytes" | "String" => match value {
            Json::Str(_) => Ok(()),
            other => Err(expected_type_error("String", other)),
        },
        // from_json(bool) falls back to from_json(int32) for any non-Boolean value, so numbers and
        // numeric strings are accepted; the failure message still names the original JSON type
        "bool" | "Bool" => match value {
            Json::Bool(_) => Ok(()),
            other if json_int32_field(other).is_ok() => Ok(()),
            other => Err(expected_type_error("Boolean", other)),
        },
        "double" | "Double" => match value {
            Json::Int(..) | Json::Float(..) | Json::Num(_) => Ok(()),
            other => Err(expected_type_error("Number", other)),
        },
        _ => match value {
            // A field typed as a concrete constructor ignores its "@type" but still validates every
            // declared field it finds (td/tl/tl_json.h from_json is_constructible overload). An
            // abstract class resolves "@type" to a constructor and validates that constructor.
            Json::Obj(_) => match tl_schema::CONSTRUCTORS.iter().find(|(known, _)| *known == base) {
                Some((_, fields)) => validate_fields(value, fields),
                None => validate_object(value),
            },
            other => Err(expected_type_error("Object", other)),
        },
    }
}

pub(crate) fn validate_object(value: &Json) -> Result<(), String> {
    let name = match value.get("@type") {
        None => return Err("Can't find field \"@type\"".to_string()),
        Some(Json::Str(bytes)) => match std::str::from_utf8(bytes) {
            Ok(name) => name,
            Err(_) => return Err(BARE_UTF8_SENTINEL.to_string()),
        },
        // The oracle also accepts the constructor id instead of the name (F082).
        Some(Json::Int(_, literal)) | Some(Json::Float(_, literal)) | Some(Json::Num(literal)) => {
            let constructor = to_integer_i32(literal.as_bytes());
            match tl_schema::CONSTRUCTOR_IDS.iter().find(|(id, _)| *id == constructor) {
                Some((_, name)) => name,
                None => {
                    return Err(format!("Unknown constructor 0x{:08x}", constructor as u32))
                }
            }
        }
        Some(other) => return Err(expected_type_error("String", other)),
    };
    let Some((_, fields)) = tl_schema::CONSTRUCTORS.iter().find(|(known, _)| *known == name) else {
        return Err(format!("Unknown class \"{}\"", name));
    };
    validate_fields(value, fields)
}

pub(crate) fn request_string(json: &Json, key: &str) -> Result<String, String> {
    match json.get(key) {
        None | Some(Json::Null) => Ok(String::new()),
        Some(Json::Str(bytes)) => Ok(String::from_utf8_lossy(bytes).into_owned()),
        Some(other) => Err(expected_type_error("String", other)),
    }
}

pub(crate) fn request_int32(json: &Json, key: &str) -> Result<i32, String> {
    match json.get(key) {
        None => Ok(0),
        Some(value) => json_int32_field(value),
    }
}

pub(crate) fn request_int64(json: &Json, key: &str) -> Result<i64, String> {
    match json.get(key) {
        None => Ok(0),
        Some(value) => json_int64_field(value),
    }
}

pub(crate) fn request_object<'a>(json: &'a Json, key: &str) -> Result<Option<&'a Json>, String> {
    match json.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(value @ Json::Obj(_)) => Ok(Some(value)),
        Some(other) => Err(expected_type_error("Object", other)),
    }
}

/// 返回 None 表示字段存在但不是合法 UTF-8（TDLib 让各 action 自己决定怎么报）
pub(crate) fn request_text(json: &Json, key: &str) -> Result<Option<String>, String> {
    match json.get(key) {
        None | Some(Json::Null) => Ok(Some(String::new())),
        Some(Json::Str(bytes)) => Ok(std::str::from_utf8(bytes).ok().map(str::to_string)),
        Some(other) => Err(expected_type_error("String", other)),
    }
}

pub(crate) fn request_bytes<'a>(json: &'a Json, key: &str) -> Result<&'a [u8], String> {
    match json.get(key) {
        None | Some(Json::Null) => Ok(b""),
        Some(Json::Str(bytes)) => Ok(bytes.as_slice()),
        Some(other) => Err(expected_type_error("String", other)),
    }
}

pub(crate) fn request_bool(json: &Json, key: &str) -> Result<bool, String> {
    match json.get(key) {
        None | Some(Json::Null) => Ok(false),
        Some(Json::Bool(flag)) => Ok(*flag),
        Some(value @ (Json::Int(..) | Json::Float(..) | Json::Num(_) | Json::Str(_))) => {
            match json_int32_field(value) {
                Ok(number) => Ok(number != 0),
                Err(_) => Err(expected_type_error("Boolean", value)),
            }
        }
        Some(other) => Err(expected_type_error("Boolean", other)),
    }
}

macro_rules! req {
    ($value:expr) => {
        match $value {
            Ok(value) => value,
            Err(message) => return request_error(&message),
        }
    };
}

pub(crate) fn encode_json_text(value: &Json) -> String {
    let mut out = String::new();
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Json::Int(_, literal) | Json::Float(_, literal) | Json::Num(literal) => {
            out.push_str(literal)
        }
        Json::Str(bytes) => {
            out.push('"');
            json_escape(&String::from_utf8_lossy(bytes), &mut out);
            out.push('"');
        }
        Json::Arr(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&encode_json_text(item));
            }
            out.push(']');
        }
        Json::Obj(members) => {
            out.push('{');
            for (index, (name, member)) in members.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('"');
                json_escape(name, &mut out);
                out.push_str("\":");
                out.push_str(&encode_json_text(member));
            }
            out.push('}');
        }
    }
    out
}

/// td/telegram/ClientJson.cpp: "@extra" is echoed in every response, errors included
pub(crate) fn append_extra(result: String, extra: &str) -> String {
    let mut out = result;
    if out.ends_with('}') {
        out.pop();
        out.push_str(",\"@extra\":");
        out.push_str(extra);
        out.push('}');
    }
    out
}

pub(crate) fn execute(request: &[u8]) -> String {
    let json = match Json::parse(request) {
        Ok(json) => json,
        Err(message) => {
            return error_object(
                400,
                &format!("Failed to parse request as JSON object: {}", message),
            )
        }
    };
    if !matches!(json, Json::Obj(_)) {
        return error_object(400, "Expected a JSON object");
    }
    let extra = json.get("@extra").map(encode_json_text);
    let result = execute_object(&json);
    match extra {
        Some(extra) => append_extra(result, &extra),
        None => result,
    }
}

pub(crate) fn execute_object(json: &Json) -> String {
    let kind = match json.get("@type") {
        None => return request_error("Can't find field \"@type\""),
        Some(Json::Str(bytes)) => match std::str::from_utf8(bytes) {
            // an undecodable "@type" is the only way to get invalid UTF-8 into an error message, and
            // ClientJson::from_response rewrites such a message to this exact text (F092)
            Err(_) => return error_object(400, UTF8_MESSAGE),
            Ok(name) => {
                if SYNCHRONOUS_ACTIONS.contains(&name) {
                    if let Err(message) = validate_arguments(name, &json) {
                        return validation_error(&message);
                    }
                    name
                } else if api_functions::FUNCTIONS.iter().any(|(_, known)| *known == name) {
                    if let Err(message) = validate_arguments(name, &json) {
                        return validation_error(&message);
                    }
                    return error_object(400, "The method can't be executed synchronously");
                } else {
                    return request_error(&format!("Unknown class \"{}\"", name));
                }
            }
        },
        Some(Json::Int(_, literal)) | Some(Json::Float(_, literal)) | Some(Json::Num(literal)) => {
            let constructor = to_integer_i32(literal.as_bytes());
            match api_functions::FUNCTIONS.iter().find(|(id, _)| *id == constructor) {
                Some((_, name)) if SYNCHRONOUS_ACTIONS.contains(name) => {
                    if let Err(message) = validate_arguments(name, &json) {
                        return validation_error(&message);
                    }
                    *name
                }
                Some((_, name)) => {
                    if let Err(message) = validate_arguments(name, &json) {
                        return validation_error(&message);
                    }
                    return error_object(400, "The method can't be executed synchronously");
                }
                None => {
                    return request_error(&format!(
                        "Unknown constructor 0x{:08x}",
                        constructor as u32
                    ))
                }
            }
        }
        Some(other) => {
            return request_error(&format!(
                "Expected String or Integer, but receive {}",
                json_type_name(other)
            ))
        }
    };
    if let Some(result) = log_action(kind, json) {
        return result;
    }
    match kind {
        "searchQuote" => search_quote_request(json),
        "getTextEntities" => match req!(request_text(json, "text")) {
            Some(text) => entities_json(&text),
            None => error_object(400, "Text must be encoded in UTF-8"),
        },
        "getTextEmojiRanges" => match req!(request_text(json, "text")) {
            Some(text) => emoji_ranges_json(&text),
            None => error_object(400, "Text must be encoded in UTF-8"),
        },
        "checkQuickReplyShortcutName" => match req!(request_text(json, "name")) {
            Some(name) => check_shortcut_name(&name),
            None => error_object(200, "Strings must be encoded in UTF-8"),
        },
        "getCountryFlagEmoji" => {
            text_object(&country_flag_emoji(&req!(request_string(json, "country_code"))))
        }
        "getPhoneNumberInfoSync" => {
            phone_number_info_json(&req!(request_string(json, "phone_number_prefix")))
        }
        "testReturnError" => match req!(request_object(json, "error")) {
            Some(error) => error_object(
                req!(request_int32(error, "code")) as i64,
                &req!(request_string(error, "message")),
            ),
            None => error_object(404, "Not Found"),
        },
        "getFileMimeType" => text_object(file_mime_type(&req!(request_string(json, "file_name")))),
        "getFileExtension" => {
            text_object(mime_type_extension(&req!(request_string(json, "mime_type"))))
        }
        "cleanFileName" => match req!(request_text(json, "file_name")) {
            Some(file_name) => text_object(&clean_filename(&file_name)),
            None => text_object(""),
        },
        "searchStringsByPrefix" => search_strings_by_prefix_request(json),
        "parseTextEntities" => parse_text_entities(json),
        "parseMarkdown" => parse_markdown_request(json),
        "getMarkdownText" => get_markdown_text_request(json),
        "getJsonValue" => {
            let raw = req!(request_bytes(json, "json"));
            match std::str::from_utf8(raw) {
                Err(_) => error_object(400, "JSON has invalid encoding"),
                Ok(text) => match Json::parse(text.as_bytes()) {
                    Ok(value) => {
                        let mut out = String::new();
                        emit_typed_json_value(&value, &mut out);
                        out
                    }
                    Err(message) => error_object(400, &message),
                },
            }
        }
        "getJsonString" => match req!(request_object(json, "json_value")) {
            Some(value) => {
                let mut out = String::new();
                match emit_json_text(value, &mut out) {
                    Ok(()) => text_object(&out),
                    Err(message) => request_error(&message),
                }
            }
            None => text_object("null"),
        },
        "getThemeParametersJsonString" => {
            text_object(&theme_parameters_json_string(req!(request_object(json, "theme"))))
        }
        "getChatFolderDefaultIconName" => match req!(request_object(json, "folder")) {
            Some(folder) => match chat_folder_default_icon_name(Some(folder)) {
                Some(name) => {
                    let mut out = String::from("{\"@type\":\"chatFolderIcon\",\"name\":\"");
                    json_escape(&name, &mut out);
                    out.push_str("\"}");
                    out
                }
                None => error_object(400, "Chat folder must be non-empty"),
            },
            None => error_object(400, "Chat folder must be non-empty"),
        },
        "getPushReceiverId" => {
            match push_receiver_id(&req!(request_string(json, "payload"))) {
                Ok(id) => format!("{{\"@type\":\"pushReceiverId\",\"id\":\"{}\"}}", id),
                Err(message) => error_object(400, &message),
            }
        }
        "getOption" => {
            let name = req!(request_string(json, "name"));
            match synchronous_option_value(&name) {
                Some(value) => value,
                None => error_object(400, "The option can't be get synchronously"),
            }
        }
        "getLanguagePackString" => {
            let database_path = req!(request_string(json, "language_pack_database_path"));
            let pack = req!(request_string(json, "localization_target"));
            let code = req!(request_string(json, "language_pack_id"));
            let key = req!(request_string(json, "key"));
            if pack.is_empty() || !check_language_pack_name(&pack) {
                return error_object(400, "Localization target is invalid");
            }
            if code.is_empty() || !check_language_code_name(&code) {
                return error_object(400, "Language pack ID is invalid");
            }
            if !is_valid_language_key(&key) {
                return error_object(400, "Key is invalid");
            }
            language_pack_string_json(&database_path, &pack, &code, &key)
        }
        _ => error_object(400, "The method can't be executed synchronously"),
    }
}

pub(crate) fn check_shortcut_name(name: &str) -> String {
    let mut length = 0usize;
    for c in name.chars() {
        if !is_hashtag_letter(c) {
            return error_object(200, "A letter is not allowed");
        }
        length += 1;
    }
    if length == 0 {
        return error_object(200, "Name must be non-empty");
    }
    if length > 32 {
        return error_object(200, "Name is too long");
    }
    ok_object()
}

pub(crate) fn country_flag_emoji(country_code: &str) -> String {
    let chars: Vec<char> = country_code.chars().collect();
    if chars.len() != 2 || !chars[0].is_ascii_alphabetic() || !chars[1].is_ascii_alphabetic() {
        return String::new();
    }
    let first = chars[0].to_ascii_uppercase();
    let second = chars[1].to_ascii_uppercase();
    match (first, second) {
        ('Y', 'L') => return "\u{1F52E}".to_string(),
        ('F', 'T') => return "\u{1F48E}".to_string(),
        ('X', 'G') => return "\u{1F6F0}".to_string(),
        ('X', 'V') => return "\u{1F30D}".to_string(),
        _ => {}
    }
    let mut out = String::with_capacity(2);
    out.push(char::from_u32(0x1F1A5 + first as u32).unwrap_or('\u{fffd}'));
    out.push(char::from_u32(0x1F1A5 + second as u32).unwrap_or('\u{fffd}'));
    out
}

pub(crate) fn phone_number_info_json(phone_number_prefix: &str) -> String {
    let phone_number: String = phone_number_prefix.chars().filter(|c| c.is_ascii_digit()).collect();
    if phone_number.is_empty() {
        return phone_number_info_object_json(None, "", "", false);
    }
    let is_anonymous = phone_number.starts_with("888");

    let bytes = phone_number.as_bytes();
    let mut best: Option<(&country_table::Country, &country_table::CallingCode)> = None;
    let mut best_length = 0;
    let mut is_prefix = false;
    for country in country_table::COUNTRIES.iter() {
        for calling_code in country.calling_codes.iter() {
            let code = calling_code.calling_code.as_bytes();
            if bytes.starts_with(code) {
                let rest = &bytes[code.len()..];
                for prefix in calling_code.prefixes.iter() {
                    if prefix.as_bytes().starts_with(rest) {
                        is_prefix = true;
                    }
                    if code.len() + prefix.len() > best_length && rest.starts_with(prefix.as_bytes()) {
                        best = Some((country, calling_code));
                        best_length = code.len() + prefix.len();
                    }
                }
            }
            if code.starts_with(bytes) {
                is_prefix = true;
            }
        }
    }

    let Some((country, calling_code)) = best else {
        return if is_prefix {
            phone_number_info_object_json(None, &phone_number, "", is_anonymous)
        } else {
            phone_number_info_object_json(None, "", &phone_number, is_anonymous)
        };
    };

    let formatted_part = &phone_number[calling_code.calling_code.len()..];
    let mut formatted_phone_number = String::new();
    let mut max_matched_digits = 0;
    for pattern in calling_code.patterns.iter() {
        let pattern = pattern.as_bytes();
        let mut result = String::new();
        let mut pattern_pos = 0;
        let mut is_failed_match = false;
        let mut matched_digits = 0;
        for &digit in formatted_part.as_bytes() {
            while pattern_pos < pattern.len() && pattern[pattern_pos] != b'X' && !pattern[pattern_pos].is_ascii_digit()
            {
                result.push(pattern[pattern_pos] as char);
                pattern_pos += 1;
            }
            if pattern_pos >= pattern.len() || pattern[pattern_pos] == b'X' {
                result.push(digit as char);
                pattern_pos += 1;
            } else if digit == pattern[pattern_pos] {
                matched_digits += 1;
                result.push(digit as char);
                pattern_pos += 1;
            } else {
                is_failed_match = true;
                break;
            }
        }
        if pattern[pattern_pos.min(pattern.len())..].iter().any(|c| c.is_ascii_digit()) {
            is_failed_match = true;
        }
        if !is_failed_match && matched_digits >= max_matched_digits {
            max_matched_digits = matched_digits;
            while pattern_pos < pattern.len() {
                result.push(if pattern[pattern_pos] == b'X' { '-' } else { ' ' });
                pattern_pos += 1;
            }
            formatted_phone_number = result;
        }
    }

    let formatted: &str =
        if formatted_phone_number.is_empty() { formatted_part } else { &formatted_phone_number };
    phone_number_info_object_json(
        Some(country),
        &calling_code.calling_code,
        formatted,
        is_anonymous,
    )
}

pub(crate) fn phone_number_info_object_json(
    country: Option<&country_table::Country>,
    country_calling_code: &str,
    formatted_phone_number: &str,
    is_anonymous: bool,
) -> String {
    let mut out = String::from("{\"@type\":\"phoneNumberInfo\"");
    if let Some(country) = country {
        out.push_str(",\"country\":");
        out.push_str(&country_info_json(country));
    }
    out.push_str(",\"country_calling_code\":\"");
    json_escape(country_calling_code, &mut out);
    out.push_str("\",\"formatted_phone_number\":\"");
    json_escape(formatted_phone_number, &mut out);
    out.push_str("\",\"is_anonymous\":");
    out.push_str(if is_anonymous { "true" } else { "false" });
    out.push('}');
    out
}

pub(crate) fn country_info_json(country: &country_table::Country) -> String {
    let mut out = String::from("{\"@type\":\"countryInfo\",\"country_code\":\"");
    json_escape(country.country_code, &mut out);
    out.push_str("\",\"name\":\"");
    json_escape(country.default_name, &mut out);
    out.push_str("\",\"english_name\":\"");
    json_escape(country.default_name, &mut out);
    out.push_str("\",\"flag_emoji\":\"");
    json_escape(&country_flag_emoji(country.country_code), &mut out);
    out.push_str("\",\"is_hidden\":");
    out.push_str(if country.is_hidden { "true" } else { "false" });
    out.push_str(",\"calling_codes\":[");
    for (index, calling_code) in country.calling_codes.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        json_escape(calling_code.calling_code, &mut out);
        out.push('"');
    }
    out.push_str("]}");
    out
}
