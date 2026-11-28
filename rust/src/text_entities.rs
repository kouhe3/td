// 拆分自 `rust/src/lib.rs`（一次性 codemod，见 tasks/plan.md T119）；只改可见性与位置，未改实现。
#![allow(unused_imports)]

use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{LazyLock, Mutex};

pub(crate) fn emit_typed_json_value(value: &Json, out: &mut String) {
    match value {
        Json::Null => out.push_str("{\"@type\":\"jsonValueNull\"}"),
        Json::Bool(flag) => {
            out.push_str("{\"@type\":\"jsonValueBoolean\",\"value\":");
            out.push_str(if *flag { "true" } else { "false" });
            out.push('}');
        }
        Json::Int(number, _) => out.push_str(&format!(
            "{{\"@type\":\"jsonValueNumber\",\"value\":{:.6}}}",
            *number as f64
        )),
        Json::Float(number, _) => out.push_str(&format!(
            "{{\"@type\":\"jsonValueNumber\",\"value\":{:.6}}}",
            number
        )),
        Json::Num(literal) => out.push_str(&format!(
            "{{\"@type\":\"jsonValueNumber\",\"value\":{:.6}}}",
            to_double_prefix(literal)
        )),
        Json::Str(bytes) => {
            out.push_str("{\"@type\":\"jsonValueString\",\"value\":\"");
            json_escape(&String::from_utf8_lossy(bytes), out);
            out.push_str("\"}");
        }
        Json::Arr(items) => {
            out.push_str("{\"@type\":\"jsonValueArray\",\"values\":[");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                emit_typed_json_value(item, out);
            }
            out.push_str("]}");
        }
        Json::Obj(members) => {
            out.push_str("{\"@type\":\"jsonValueObject\",\"members\":[");
            for (index, (key, member)) in members.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str("{\"@type\":\"jsonObjectMember\",\"key\":\"");
                json_escape(key, out);
                out.push_str("\",\"value\":");
                emit_typed_json_value(member, out);
                out.push('}');
            }
            out.push_str("]}");
        }
    }
}

pub(crate) const JSON_VALUE_CONSTRUCTORS: [(i32, &str); 6] = [
    (-92872499, "jsonValueNull"),
    (-2142186576, "jsonValueBoolean"),
    (-1010822033, "jsonValueNumber"),
    (1597947313, "jsonValueString"),
    (-322064168, "jsonValueArray"),
    (-964952256, "jsonValueObject"),
];

/// getJsonString: JsonValue::to_string, with the JSON layer's typed field reads
pub(crate) fn emit_json_text(value: &Json, out: &mut String) -> Result<(), String> {
    if !matches!(value, Json::Obj(_)) {
        return Err(expected_type_error("Object", value));
    }
    match json_class_name(value, &JSON_VALUE_CONSTRUCTORS)? {
        "jsonValueNull" => out.push_str("null"),
        "jsonValueBoolean" => {
            let flag = request_bool(value, "value")?;
            out.push_str(if flag { "true" } else { "false" });
        }
        "jsonValueNumber" => {
            let number = match value.get("value") {
                None | Some(Json::Null) => 0.0,
                Some(Json::Int(number, _)) => *number as f64,
                Some(Json::Float(number, _)) => *number,
                Some(Json::Num(literal)) => to_double_prefix(literal),
                Some(other) => return Err(expected_type_error("Number", other)),
            };
            out.push_str(&format!("{:.6}", number));
        }
        "jsonValueString" => {
            out.push('"');
            json_escape(&request_string(value, "value")?, out);
            out.push('"');
        }
        "jsonValueArray" => {
            out.push('[');
            match value.get("values") {
                None | Some(Json::Null) => {}
                Some(Json::Arr(items)) => {
                    for (index, item) in items.iter().enumerate() {
                        if index > 0 {
                            out.push(',');
                        }
                        emit_json_text(item, out)?;
                    }
                }
                Some(other) => return Err(expected_type_error("Array", other)),
            }
            out.push(']');
        }
        "jsonValueObject" => {
            out.push('{');
            match value.get("members") {
                None | Some(Json::Null) => {}
                Some(Json::Arr(members)) => {
                    for (index, member) in members.iter().enumerate() {
                        if index > 0 {
                            out.push(',');
                        }
                        if !matches!(member, Json::Obj(_)) {
                            return Err(expected_type_error("Object", member));
                        }
                        out.push('"');
                        json_escape(&request_string(member, "key")?, out);
                        out.push_str("\":");
                        match member.get("value") {
                            None | Some(Json::Null) => out.push_str("null"),
                            Some(member_value) => emit_json_text(member_value, out)?,
                        }
                    }
                }
                Some(other) => return Err(expected_type_error("Array", other)),
            }
            out.push('}');
        }
        other => return Err(format!("Unknown class \"{}\"", other)),
    }
    Ok(())
}

pub(crate) fn color_text(color: i64) -> String {
    format!("#{:06x}", color as u32 & 0xFF_FF_FF)
}

pub(crate) fn theme_parameters_json_string(theme: Option<&Json>) -> String {
    let Some(theme) = theme else {
        return String::new();
    };
    if matches!(theme, Json::Null) {
        return String::new();
    }
    const FIELDS: [(&str, &str); 15] = [
        ("bg_color", "background_color"),
        ("secondary_bg_color", "secondary_background_color"),
        ("text_color", "text_color"),
        ("hint_color", "hint_color"),
        ("link_color", "link_color"),
        ("button_color", "button_color"),
        ("button_text_color", "button_text_color"),
        ("header_bg_color", "header_background_color"),
        ("bottom_bar_bg_color", "bottom_bar_background_color"),
        ("section_bg_color", "section_background_color"),
        ("section_separator_color", "section_separator_color"),
        ("accent_text_color", "accent_text_color"),
        ("section_header_text_color", "section_header_text_color"),
        ("subtitle_text_color", "subtitle_text_color"),
        ("destructive_text_color", "destructive_text_color"),
    ];
    let mut out = String::from("{");
    for (index, (json_key, field)) in FIELDS.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        out.push_str(json_key);
        out.push_str("\":\"");
        out.push_str(&color_text(theme.get(field).and_then(Json::as_i64).unwrap_or(0)));
        out.push('"');
    }
    out.push('}');
    out
}

pub(crate) const FOLDER_ICON_NAMES: [&str; 30] = [
    "All", "Unread", "Unmuted", "Bots", "Channels", "Groups", "Private", "Custom", "Setup", "Cat", "Crown",
    "Favorite", "Flower", "Game", "Home", "Love", "Mask", "Party", "Sport", "Study", "Trade", "Travel", "Work",
    "Airplane", "Book", "Light", "Like", "Money", "Note", "Palette",
];

pub(crate) fn json_array_len(value: Option<&Json>) -> usize {
    match value {
        Some(Json::Arr(items)) => items.len(),
        _ => 0,
    }
}

pub(crate) fn json_flag(value: Option<&Json>) -> bool {
    // from_json(bool): a Boolean keeps its value, anything else goes through from_json(int32)
    match value {
        Some(Json::Bool(flag)) => *flag,
        Some(other) => json_int32_field(other).map_or(false, |number| number != 0),
        None => false,
    }
}

pub(crate) fn chat_folder_default_icon_name(folder: Option<&Json>) -> Option<String> {
    let folder = folder?;
    if let Some(name) = folder.get("icon").and_then(|icon| icon.get("name")).and_then(Json::as_str) {
        if FOLDER_ICON_NAMES.contains(&name) {
            return Some(name.to_string());
        }
    }
    if json_array_len(folder.get("pinned_chat_ids")) > 0
        || json_array_len(folder.get("included_chat_ids")) > 0
        || json_array_len(folder.get("excluded_chat_ids")) > 0
    {
        return Some("Custom".to_string());
    }
    let contacts = json_flag(folder.get("include_contacts")) || json_flag(folder.get("include_non_contacts"));
    let bots = json_flag(folder.get("include_bots"));
    let groups = json_flag(folder.get("include_groups"));
    let channels = json_flag(folder.get("include_channels"));
    if contacts {
        if !bots && !groups && !channels {
            return Some("Private".to_string());
        }
    } else if !bots && !channels {
        if !groups {
            return Some("Custom".to_string());
        }
        return Some("Groups".to_string());
    } else if !bots && !groups {
        return Some("Channels".to_string());
    } else if !groups && !channels {
        return Some("Bots".to_string());
    }
    let exclude_read = json_flag(folder.get("exclude_read"));
    let exclude_muted = json_flag(folder.get("exclude_muted"));
    if exclude_read && !exclude_muted {
        return Some("Unread".to_string());
    }
    if exclude_muted && !exclude_read {
        return Some("Unmuted".to_string());
    }
    Some("Custom".to_string())
}

pub(crate) fn base64url_prefix_value(text: &str) -> Option<i64> {
    let prefix: &[u8] = text.as_bytes().get(..12)?;
    let decoded = base64_decode_strict(prefix, true)?;
    if decoded.len() != 9 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&decoded[..8]);
    Some(i64::from_le_bytes(bytes))
}

pub(crate) fn push_receiver_id(payload: &str) -> Result<i64, String> {
    if payload == "{}" {
        return Ok(0);
    }
    let Ok(mut root) = Json::parse(payload.as_bytes()) else {
        return Err("Failed to parse payload as JSON object".to_string());
    };
    if !matches!(root, Json::Obj(_)) {
        return Err("Expected JSON object".to_string());
    }
    if let Some(data) = root.get("data") {
        if !matches!(data, Json::Obj(_)) {
            return Err("Field \"data\" must be of type Object".to_string());
        }
        root = data.clone();
    }
    if let Some(value) = root.get("p") {
        let encrypted = match value {
            Json::Str(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            Json::Int(_, literal) | Json::Float(_, literal) | Json::Num(literal) => literal.clone(),
            _ => return Err("Field \"p\" must be of type String".to_string()),
        };
        if encrypted.len() < 12 {
            return Err("Encrypted payload is too small".to_string());
        }
        return base64url_prefix_value(&encrypted)
            .ok_or_else(|| "Failed to base64url-decode payload".to_string());
    }
    if let Some(value) = root.get("user_id") {
        let literal = match value {
            Json::Int(_, literal) | Json::Float(_, literal) | Json::Num(literal) => literal.clone(),
            Json::Str(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => return Err("Field \"user_id\" must be a Number".to_string()),
        };
        let Some(user_id) = to_integer_safe_i64(literal.as_bytes()) else {
            return Err("Field \"user_id\" must be a valid Number".to_string());
        };
        if user_id <= 0 {
            return Err(format!("Receive wrong user_id {}", user_id));
        }
        return Ok(user_id);
    }
    Ok(0)
}

// ---------------------------------------------------------------------------
// parseTextEntities: td/telegram/MessageEntity.cpp parse_markdown,
// parse_markdown_v2 and parse_html, with the LinkManager (URL checks and
// tg:// link parsing), HttpUrl (parse_url / get_url) and FormattedDate
// helpers they call. Offsets and lengths count UTF-16 code units.
//
// The byte buffers are mutated in place exactly like the C++ code does, since
// parse_markdown and parse_html read back already compacted bytes.
// ---------------------------------------------------------------------------

pub(crate) const MAX_USER_ID: i64 = (1 << 40) - 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EType {
    Mention,
    Hashtag,
    BotCommand,
    Url,
    EmailAddress,
    Bold,
    Italic,
    Code,
    Pre,
    PreCode,
    TextUrl,
    MentionName,
    Cashtag,
    PhoneNumber,
    Underline,
    Strikethrough,
    BlockQuote,
    BankCardNumber,
    MediaTimestamp,
    Spoiler,
    CustomEmoji,
    ExpandableBlockQuote,
    FormattedDate,
    TonAddress,
}

impl EType {
    /// index of MessageEntity::Type, used for the entity type bit masks
    pub(crate) fn index(self) -> u32 {
        self as u32
    }

    pub(crate) fn type_name(self) -> &'static str {
        match self {
            EType::Mention => "textEntityTypeMention",
            EType::Hashtag => "textEntityTypeHashtag",
            EType::BotCommand => "textEntityTypeBotCommand",
            EType::Url => "textEntityTypeUrl",
            EType::EmailAddress => "textEntityTypeEmailAddress",
            EType::Bold => "textEntityTypeBold",
            EType::Italic => "textEntityTypeItalic",
            EType::Code => "textEntityTypeCode",
            EType::Pre => "textEntityTypePre",
            EType::PreCode => "textEntityTypePreCode",
            EType::TextUrl => "textEntityTypeTextUrl",
            EType::MentionName => "textEntityTypeMentionName",
            EType::Cashtag => "textEntityTypeCashtag",
            EType::PhoneNumber => "textEntityTypePhoneNumber",
            EType::Underline => "textEntityTypeUnderline",
            EType::Strikethrough => "textEntityTypeStrikethrough",
            EType::BlockQuote => "textEntityTypeBlockQuote",
            EType::BankCardNumber => "textEntityTypeBankCardNumber",
            EType::MediaTimestamp => "textEntityTypeMediaTimestamp",
            EType::Spoiler => "textEntityTypeSpoiler",
            EType::CustomEmoji => "textEntityTypeCustomEmoji",
            EType::ExpandableBlockQuote => "textEntityTypeExpandableBlockQuote",
            EType::FormattedDate => "textEntityTypeDateTime",
            EType::TonAddress => "textEntityTypeTonAddress",
        }
    }

    /// MessageEntity.cpp operator<<; these names appear inside error messages.
    pub(crate) fn message_name(self) -> &'static str {
        match self {
            EType::Mention => "Mention",
            EType::Hashtag => "Hashtag",
            EType::BotCommand => "BotCommand",
            EType::Url => "Url",
            EType::EmailAddress => "EmailAddress",
            EType::Bold => "Bold",
            EType::Italic => "Italic",
            EType::Underline => "Underline",
            EType::Strikethrough => "Strikethrough",
            EType::BlockQuote => "BlockQuote",
            EType::Code => "Code",
            EType::Pre => "Pre",
            EType::PreCode => "PreCode",
            EType::TextUrl => "TextUrl",
            EType::MentionName => "MentionName",
            EType::Cashtag => "Cashtag",
            EType::PhoneNumber => "PhoneNumber",
            EType::BankCardNumber => "BankCardNumber",
            EType::MediaTimestamp => "MediaTimestamp",
            EType::Spoiler => "Spoiler",
            EType::CustomEmoji => "CustomEmoji",
            EType::ExpandableBlockQuote => "ExpandableBlockQuote",
            EType::FormattedDate => "Date",
            EType::TonAddress => "TonAddress",
        }
    }

    /// MessageEntity::get_type_priority
    pub(crate) fn priority(self) -> i32 {
        match self {
            EType::Mention => 50,
            EType::Hashtag => 50,
            EType::BotCommand => 50,
            EType::Url => 50,
            EType::EmailAddress => 50,
            EType::Bold => 90,
            EType::Italic => 91,
            EType::Code => 20,
            EType::Pre => 11,
            EType::PreCode => 10,
            EType::TextUrl => 49,
            EType::MentionName => 49,
            EType::Cashtag => 50,
            EType::PhoneNumber => 50,
            EType::Underline => 92,
            EType::Strikethrough => 93,
            EType::BlockQuote => 0,
            EType::BankCardNumber => 50,
            EType::MediaTimestamp => 50,
            EType::Spoiler => 94,
            EType::CustomEmoji => 99,
            EType::ExpandableBlockQuote => 0,
            EType::FormattedDate => 30,
            EType::TonAddress => 50,
        }
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct TextEntity {
    pub(crate) etype: EType,
    pub(crate) offset: i32,
    pub(crate) length: i32,
    pub(crate) argument: String,
    pub(crate) user_id: i64,
    pub(crate) custom_emoji_id: i64,
    pub(crate) date: i32,
    pub(crate) date_flags: i32,
    pub(crate) media_timestamp: i32,
}

impl TextEntity {
    pub(crate) fn new(etype: EType, offset: i32, length: i32) -> TextEntity {
        TextEntity {
            etype,
            offset,
            length,
            argument: String::new(),
            user_id: 0,
            custom_emoji_id: 0,
            date: 0,
            date_flags: 0,
            media_timestamp: -1,
        }
    }
}

pub(crate) fn sort_entities(entities: &mut [TextEntity]) {
    entities.sort_by(|left, right| {
        left.offset
            .cmp(&right.offset)
            .then_with(|| right.length.cmp(&left.length))
            .then_with(|| left.etype.priority().cmp(&right.etype.priority()))
    });
}

// ---------------------------------------------------------------------------
// parseMarkdown / getMarkdownText: the v3 pipeline of MessageEntity.cpp
// ---------------------------------------------------------------------------

pub(crate) fn entity_type_mask(etype: EType) -> u32 {
    1 << etype.index()
}

pub(crate) fn splittable_entities_mask() -> u32 {
    entity_type_mask(EType::Bold)
        | entity_type_mask(EType::Italic)
        | entity_type_mask(EType::Underline)
        | entity_type_mask(EType::Strikethrough)
        | entity_type_mask(EType::Spoiler)
}

pub(crate) fn blockquote_entities_mask() -> u32 {
    entity_type_mask(EType::BlockQuote) | entity_type_mask(EType::ExpandableBlockQuote)
}

pub(crate) fn continuous_entities_mask() -> u32 {
    entity_type_mask(EType::Mention)
        | entity_type_mask(EType::Hashtag)
        | entity_type_mask(EType::BotCommand)
        | entity_type_mask(EType::Url)
        | entity_type_mask(EType::EmailAddress)
        | entity_type_mask(EType::TextUrl)
        | entity_type_mask(EType::MentionName)
        | entity_type_mask(EType::Cashtag)
        | entity_type_mask(EType::PhoneNumber)
        | entity_type_mask(EType::BankCardNumber)
        | entity_type_mask(EType::MediaTimestamp)
        | entity_type_mask(EType::CustomEmoji)
        | entity_type_mask(EType::TonAddress)
}

pub(crate) fn pre_entities_mask() -> u32 {
    entity_type_mask(EType::Pre)
        | entity_type_mask(EType::Code)
        | entity_type_mask(EType::PreCode)
        | entity_type_mask(EType::FormattedDate)
}

pub(crate) fn user_entities_mask() -> u32 {
    splittable_entities_mask()
        | blockquote_entities_mask()
        | entity_type_mask(EType::TextUrl)
        | entity_type_mask(EType::MentionName)
        | entity_type_mask(EType::CustomEmoji)
        | pre_entities_mask()
}

pub(crate) fn is_splittable_entity(etype: EType) -> bool {
    entity_type_mask(etype) & splittable_entities_mask() != 0
}

pub(crate) fn is_blockquote_entity(etype: EType) -> bool {
    entity_type_mask(etype) & blockquote_entities_mask() != 0
}

pub(crate) fn is_continuous_entity(etype: EType) -> bool {
    entity_type_mask(etype) & continuous_entities_mask() != 0
}

pub(crate) fn is_pre_entity(etype: EType) -> bool {
    entity_type_mask(etype) & pre_entities_mask() != 0
}

pub(crate) fn is_user_entity(etype: EType) -> bool {
    entity_type_mask(etype) & user_entities_mask() != 0
}

pub(crate) const SPLITTABLE_ENTITY_TYPE_COUNT: usize = 5;

pub(crate) fn splittable_entity_type_index(etype: EType) -> usize {
    match etype {
        EType::Bold => 0,
        EType::Italic => 1,
        EType::Underline => 2,
        EType::Strikethrough => 3,
        EType::Spoiler => 4,
        _ => unreachable!(),
    }
}

/// td/utils/utf8.cpp utf8_utf16_length
pub(crate) fn text_length(text: &[u8]) -> i32 {
    let mut result = 0;
    for &c in text {
        if utf8_is_first_code_unit(c) {
            result += 1 + i32::from((c & 0xf8) == 0xf0);
        }
    }
    result
}

/// td/utils/utf8.cpp utf8_utf16_truncate
pub(crate) fn utf16_truncate(text: &[u8], mut length: i32) -> &[u8] {
    for (index, &c) in text.iter().enumerate() {
        if utf8_is_first_code_unit(c) {
            if length <= 0 {
                return &text[..index];
            }
            length -= 1;
            if c >= 0xf0 {
                length -= 1;
            }
        }
    }
    text
}

/// td/utils/utf8.cpp utf8_utf16_substr
pub(crate) fn utf8_utf16_substr(text: &[u8], offset: i32, length: i32) -> &[u8] {
    let rest = if offset == 0 {
        text
    } else {
        let cut = utf16_truncate(text, offset).len();
        &text[cut..]
    };
    utf16_truncate(rest, length)
}

pub(crate) fn is_valid_user_id(user_id: i64) -> bool {
    0 < user_id && user_id <= MAX_USER_ID
}

pub(crate) fn remove_empty_entities(entities: &mut Vec<TextEntity>) {
    entities.retain(|entity| {
        if entity.length <= 0 {
            return false;
        }
        match entity.etype {
            EType::TextUrl => !entity.argument.is_empty(),
            EType::MentionName => is_valid_user_id(entity.user_id),
            EType::CustomEmoji => entity.custom_emoji_id != 0,
            EType::FormattedDate => entity.date > 0,
            _ => true,
        }
    });
}

pub(crate) fn are_entities_valid(entities: &[TextEntity]) -> bool {
    if entities.is_empty() {
        return true;
    }

    let mut end_pos = [-1i32; SPLITTABLE_ENTITY_TYPE_COUNT];
    let mut stack: Vec<&TextEntity> = Vec::new();
    let mut nested_entity_type_mask = 0u32;
    for entity in entities {
        while let Some(top) = stack.last() {
            if entity.offset < top.offset + top.length {
                break;
            }
            nested_entity_type_mask &= !entity_type_mask(top.etype);
            stack.pop();
        }

        if let Some(top) = stack.last() {
            if entity.offset + entity.length > top.offset + top.length {
                // entity intersects some previous entity
                return false;
            }
            if nested_entity_type_mask & entity_type_mask(entity.etype) != 0 {
                // entity has the same type as one of the previous nested
                return false;
            }
            if is_pre_entity(top.etype) {
                // Pre and Code can't contain nested entities
                return false;
            }
            // parents are not pre after this point
            if is_pre_entity(entity.etype) && (nested_entity_type_mask & !blockquote_entities_mask()) != 0 {
                // Pre and Code can't be part of other entities, except blockquote
                return false;
            }
            if (is_continuous_entity(entity.etype) || is_blockquote_entity(entity.etype))
                && (nested_entity_type_mask & continuous_entities_mask()) != 0
            {
                // continuous and blockquote can't be part of other continuous entity
                return false;
            }
            if is_blockquote_entity(entity.etype) && (nested_entity_type_mask & blockquote_entities_mask()) != 0 {
                // blockquote entities can't be nested
                return false;
            }
            if (nested_entity_type_mask & splittable_entities_mask()) != 0 {
                // the previous nested entity may be needed to split for consistency
                return false;
            }
        }

        if is_splittable_entity(entity.etype) {
            let index = splittable_entity_type_index(entity.etype);
            if end_pos[index] >= entity.offset {
                // the entities can be merged
                return false;
            }
            end_pos[index] = entity.offset + entity.length;
        }
        stack.push(entity);
        nested_entity_type_mask |= entity_type_mask(entity.etype);
    }
    true
}

// removes all intersecting entities, including nested
pub(crate) fn remove_intersecting_entities(entities: &mut Vec<TextEntity>) {
    let mut last_entity_end = 0i32;
    let mut left_entities = 0usize;
    for index in 0..entities.len() {
        if entities[index].offset >= last_entity_end {
            last_entity_end = entities[index].offset + entities[index].length;
            if index != left_entities {
                entities[left_entities] = entities[index].clone();
            }
            left_entities += 1;
        }
    }
    entities.truncate(left_entities);
}

// continuous_entities and blockquote_entities must be pre-sorted and non-overlapping
pub(crate) fn remove_entities_intersecting_blockquote(
    entities: &mut Vec<TextEntity>,
    blockquote_entities: &[TextEntity],
) {
    if blockquote_entities.is_empty() {
        return;
    }

    let mut blockquote_it = 0usize;
    let mut left_entities = 0usize;
    for index in 0..entities.len() {
        while blockquote_it < blockquote_entities.len()
            && (!is_blockquote_entity(blockquote_entities[blockquote_it].etype)
                || blockquote_entities[blockquote_it].offset + blockquote_entities[blockquote_it].length
                    <= entities[index].offset)
        {
            blockquote_it += 1;
        }
        if blockquote_it < blockquote_entities.len() {
            let blockquote = &blockquote_entities[blockquote_it];
            if blockquote.offset + blockquote.length < entities[index].offset + entities[index].length
                || (entities[index].offset < blockquote.offset
                    && blockquote.offset < entities[index].offset + entities[index].length)
            {
                continue;
            }
        }
        if index != left_entities {
            entities[left_entities] = entities[index].clone();
        }
        left_entities += 1;
    }
    entities.truncate(left_entities);
}

pub(crate) struct SplitEntitiesState<'a> {
    entities: &'a [TextEntity],
    result: Vec<TextEntity>,
    it: usize,
    begin_pos: [i32; SPLITTABLE_ENTITY_TYPE_COUNT],
    end_pos: [i32; SPLITTABLE_ENTITY_TYPE_COUNT],
}

impl SplitEntitiesState<'_> {
    pub(crate) fn flush_entities(&mut self, offset: i32) {
        for etype in [
            EType::Bold,
            EType::Italic,
            EType::Underline,
            EType::Strikethrough,
            EType::Spoiler,
        ] {
            let index = splittable_entity_type_index(etype);
            if self.end_pos[index] != 0 && self.begin_pos[index] < offset {
                if self.end_pos[index] <= offset {
                    let length = self.end_pos[index] - self.begin_pos[index];
                    self.result
                        .push(TextEntity::new(etype, self.begin_pos[index], length));
                    self.begin_pos[index] = 0;
                    self.end_pos[index] = 0;
                } else {
                    let length = offset - self.begin_pos[index];
                    self.result
                        .push(TextEntity::new(etype, self.begin_pos[index], length));
                    self.begin_pos[index] = offset;
                }
            }
        }
    }

    pub(crate) fn add_entities(&mut self, end_offset: i32) {
        while self.it < self.entities.len() {
            let entity = self.entities[self.it].clone();
            if entity.offset >= end_offset {
                break;
            }
            let index = splittable_entity_type_index(entity.etype);
            if entity.offset <= self.end_pos[index] && self.end_pos[index] != 0 {
                if entity.offset + entity.length > self.end_pos[index] {
                    self.end_pos[index] = entity.offset + entity.length;
                }
            } else {
                self.flush_entities(entity.offset);
                self.begin_pos[index] = entity.offset;
                self.end_pos[index] = entity.offset + entity.length;
            }
            self.it += 1;
        }
        self.flush_entities(end_offset);
    }
}

// entities must contain only splittable entities
pub(crate) fn split_entities(entities: &mut Vec<TextEntity>, other_entities: &[TextEntity]) {
    let mut state = SplitEntitiesState {
        entities,
        result: Vec::new(),
        it: 0,
        begin_pos: [0; SPLITTABLE_ENTITY_TYPE_COUNT],
        end_pos: [0; SPLITTABLE_ENTITY_TYPE_COUNT],
    };
    let mut nested_entities_stack: Vec<usize> = Vec::new();
    for (index, other_entity) in other_entities.iter().enumerate() {
        while let Some(&top) = nested_entities_stack.last() {
            let top_entity = &other_entities[top];
            if other_entity.offset < top_entity.offset + top_entity.length {
                break;
            }
            // remove non-intersecting entities from the stack
            let old_size = state.result.len();
            state.add_entities(top_entity.offset + top_entity.length);
            if is_pre_entity(top_entity.etype) {
                state.result.truncate(old_size);
            }
            nested_entities_stack.pop();
        }
        state.add_entities(other_entity.offset);
        nested_entities_stack.push(index);
    }
    // add_offset(std::numeric_limits<int32>::max())
    while let Some(&top) = nested_entities_stack.last() {
        let top_entity = &other_entities[top];
        let old_size = state.result.len();
        state.add_entities(top_entity.offset + top_entity.length);
        if is_pre_entity(top_entity.etype) {
            state.result.truncate(old_size);
        }
        nested_entities_stack.pop();
    }
    state.add_entities(i32::MAX);
    let result = state.result;
    *entities = result;

    // entities are sorted only by offset now, re-sort if needed
    sort_entities(entities);
}

pub(crate) fn resplit_entities(splittable_entities: Vec<TextEntity>, entities: Vec<TextEntity>) -> Vec<TextEntity> {
    if splittable_entities.is_empty() {
        return entities;
    }

    let mut splittable_entities = splittable_entities;
    let mut entities = entities;
    split_entities(&mut splittable_entities, &entities); // can merge some entities

    if entities.is_empty() {
        return splittable_entities;
    }

    entities.extend(splittable_entities);
    sort_entities(&mut entities);
    entities
}

pub(crate) fn fix_entities(entities: &mut Vec<TextEntity>) {
    sort_entities(entities);

    if are_entities_valid(entities) {
        // fast path
        return;
    }

    let mut continuous_entities = Vec::new();
    let mut blockquote_entities = Vec::new();
    let mut splittable_entities = Vec::new();
    for entity in entities.drain(..) {
        if is_splittable_entity(entity.etype) {
            splittable_entities.push(entity);
        } else if is_blockquote_entity(entity.etype) {
            blockquote_entities.push(entity);
        } else {
            continuous_entities.push(entity);
        }
    }
    remove_intersecting_entities(&mut continuous_entities); // continuous entities can't intersect each other

    if !blockquote_entities.is_empty() {
        remove_intersecting_entities(&mut blockquote_entities); // blockquote entities can't intersect each other

        // blockquote entities can contain continuous entities, but can't intersect them in the other ways
        remove_entities_intersecting_blockquote(&mut continuous_entities, &blockquote_entities);

        continuous_entities.extend(blockquote_entities);
        sort_entities(&mut continuous_entities);
    }

    // must be called once to not merge some adjacent entities
    *entities = resplit_entities(splittable_entities, continuous_entities);
}

// td/telegram/misc.cpp
pub(crate) fn replace_offending_characters(text: &mut [u8]) {
    let mut pos = 0usize;
    while pos < text.len() {
        if text[pos] == 0xe2
            && text.get(pos + 1) == Some(&0x80)
            && matches!(text.get(pos + 2), Some(&0x8e) | Some(&0x8f))
        {
            while text.get(pos + 3) == Some(&0xe2)
                && text.get(pos + 4) == Some(&0x80)
                && matches!(text.get(pos + 5), Some(&0x8e) | Some(&0x8f))
            {
                text[pos + 2] = 0x8c;
                pos += 3;
            }
            pos += 2;
        }
        pos += 1;
    }
}

pub(crate) const TEXT_LENGTH_LIMIT: usize = 35000; // server-side limit

pub(crate) fn clean_input_string(text: &mut Vec<u8>) -> bool {
    if std::str::from_utf8(text).is_err() {
        return false;
    }

    let size = text.len();
    let mut new_size = 0usize;
    let mut pos = 0usize;
    while pos < size {
        let c = text[pos];
        match c {
            // remove control characters, but allow '\n' and ignore '\r'
            0..=9 | 11 | 12 | 14..=32 => {
                text[new_size] = b' ';
                new_size += 1;
            }
            b'\r' => {}
            _ => {
                // remove \xe2\x80[\xa8-\xae]
                if c == 0xe2
                    && pos + 2 < size
                    && text[pos + 1] == 0x80
                    && (0xa8..=0xae).contains(&text[pos + 2])
                {
                    pos += 2;
                } else if c == 0xcc && pos + 1 < size && matches!(text[pos + 1], 0xb3 | 0xbf | 0x8a) {
                    // remove vertical lines \xcc[\xb3\xbf\x8a]
                    pos += 1;
                } else {
                    text[new_size] = text[pos];
                    new_size += 1;
                }
            }
        }
        if new_size >= TEXT_LENGTH_LIMIT - 3 && utf8_is_first_code_unit(text[new_size - 1]) {
            new_size -= 1;
            break;
        }
        pos += 1;
    }

    text.truncate(new_size);
    replace_offending_characters(text);
    true
}

/// like clean_input_string but also fixes entities
pub(crate) fn clean_input_string_with_entities(text: &[u8], entities: &mut Vec<TextEntity>) -> Result<Vec<u8>, String> {
    let mut nested_entities_stack: Vec<(usize, i32)> = Vec::new();
    let mut current_entity = 0usize;
    let mut utf16_offset = 0i32;
    let mut utf16_skipped = 0i32;
    let text_size = text.len();

    let mut result = Vec::with_capacity(text_size);
    let mut pos = 0usize;
    while pos <= text_size {
        let c = byte_at(text, pos);
        let is_utf8_character_begin = utf8_is_first_code_unit(c);
        if is_utf8_character_begin {
            while let Some(&(index, skipped_before)) = nested_entities_stack.last() {
                let entity_end = entities[index].offset + entities[index].length;
                if utf16_offset < entity_end {
                    break;
                }
                if utf16_offset != entity_end {
                    return Err(format!(
                        "Entity beginning at UTF-16 offset {} ends in a middle of a UTF-16 symbol at byte offset {}",
                        entities[index].offset, pos
                    ));
                }
                entities[index].offset -= skipped_before;
                entities[index].length -= utf16_skipped - skipped_before;
                nested_entities_stack.pop();
            }
            while current_entity < entities.len() && utf16_offset >= entities[current_entity].offset {
                if utf16_offset != entities[current_entity].offset {
                    return Err(format!(
                        "Entity begins in a middle of a UTF-16 symbol at byte offset {}",
                        pos
                    ));
                }
                nested_entities_stack.push((current_entity, utf16_skipped));
                current_entity += 1;
            }
        }
        if pos == text_size {
            break;
        }

        match c {
            // remove control characters, but allow '\n' and ignore '\r'
            0..=9 | 11 | 12 | 14..=32 => {
                result.push(b' ');
                utf16_offset += 1;
                pos += 1;
            }
            b'\r' => {
                utf16_offset += 1;
                utf16_skipped += 1;
                pos += 1;
            }
            _ => {
                if is_utf8_character_begin {
                    utf16_offset += 1 + i32::from(c >= 0xf0);
                }
                if c == 0xe2
                    && pos + 2 < text_size
                    && byte_at(text, pos + 1) == 0x80
                    && (0xa8..=0xae).contains(&byte_at(text, pos + 2))
                {
                    pos += 3;
                    utf16_skipped += 1;
                    continue;
                }
                if c == 0xcc && pos + 1 < text_size && matches!(byte_at(text, pos + 1), 0xb3 | 0xbf | 0x8a) {
                    pos += 2;
                    utf16_skipped += 1;
                    continue;
                }

                result.push(text[pos]);
                pos += 1;
            }
        }
    }

    if current_entity != entities.len() {
        return Err(format!(
            "Entity begins after the end of the text at UTF-16 offset {}",
            entities[current_entity].offset
        ));
    }
    if let Some(&(index, _)) = nested_entities_stack.last() {
        return Err(format!(
            "Entity beginning at UTF-16 offset {} ends after the end of the text at UTF-16 offset {}",
            entities[index].offset,
            entities[index].offset + entities[index].length
        ));
    }

    replace_offending_characters(&mut result);
    Ok(result)
}

/// removes empty entities; only the "is there any non-whitespace character"
/// answer of remove_invalid_entities is needed with skip_trim = true
pub(crate) fn remove_invalid_entities(text: &[u8], entities: &mut Vec<TextEntity>) -> bool {
    if !entities.is_empty() {
        remove_empty_entities(entities);
    }
    // only ' ' and '\n' are whitespace here
    text.iter().any(|&c| c != b'\n' && c != b' ')
}

/// fix_formatted_text with the arguments used by both parseMarkdown and
/// getMarkdownText: allow_empty = true, allow_empty_string = true,
/// skip_new_entities = true, skip_bot_commands = true,
/// skip_media_timestamps = true, skip_trim = true.
pub(crate) fn fix_formatted_text(text: &mut String, entities: &mut Vec<TextEntity>) -> Result<(), String> {
    let mut result;
    if entities.is_empty() {
        // fast path
        result = text.as_bytes().to_vec();
        if !clean_input_string(&mut result) {
            return Err("Strings must be encoded in UTF-8".to_string());
        }
    } else {
        for entity in entities.iter() {
            if entity.offset < 0 || entity.offset > 1000000 {
                return Err(format!("Receive an entity with incorrect offset {}", entity.offset));
            }
            if entity.length < 0 || entity.length > 1000000 {
                return Err(format!("Receive an entity with incorrect length {}", entity.length));
            }
        }
        remove_empty_entities(entities);

        fix_entities(entities);

        result = clean_input_string_with_entities(text.as_bytes(), entities)?;
    }

    // allow_empty is true for both requests
    if !remove_invalid_entities(&result, entities) {
        text.clear();
        entities.clear();
        return Ok(());
    }

    // re-fix entities after removal of some characters; the sort order can be
    // incorrect by type and some splittable entities may be concatenated
    fix_entities(entities);

    if result.len() > TEXT_LENGTH_LIMIT {
        let mut new_size = TEXT_LENGTH_LIMIT;
        while !utf8_is_first_code_unit(result[new_size]) {
            new_size -= 1;
        }
        result.truncate(new_size);

        let length = text_length(&result);
        entities.retain(|entity| entity.offset + entity.length <= length);
    }

    *text = bytes_to_string(result);
    Ok(())
}

pub(crate) fn find_text_url_entities_v3(text: &[u8]) -> Vec<[usize; 4]> {
    let size = text.len();
    let mut result = Vec::new();
    let mut i = 0usize;
    while i < size {
        if text[i] != b'[' {
            i += 1;
            continue;
        }

        let text_begin = i;
        let mut text_end = text_begin + 1;
        while text_end < size && text[text_end] != b']' {
            text_end += 1;
        }

        if text_end == size || text_end == text_begin + 1 {
            i = text_end + 1; // prevent quadratic asymptotic
            continue;
        }

        let url_begin = text_end + 1;
        if url_begin == size || text[url_begin] != b'(' {
            i = text_end + 1;
            continue;
        }

        let mut url_end = url_begin + 1;
        while url_end < size && text[url_end] != b')' {
            url_end += 1;
        }

        // prevent quadratic asymptotic; disallows [a](b[c](t.me)
        if url_end < size && get_checked_link(&text[url_begin + 1..url_end]).is_some() {
            result.push([text_begin, text_end, url_begin, url_end]);
        }
        i = url_end + 1;
    }
    result
}

pub(crate) struct TextUrlState {
    result_text: Vec<u8>,
    result_entities: Vec<TextEntity>,
    result_text_utf16_length: i32,
    part_entities: Vec<TextEntity>,
    part_splittable_entities: [Vec<TextEntity>; SPLITTABLE_ENTITY_TYPE_COUNT],
    part_begin: i32,
    max_end: i32,
    skipped_length: i32,
}

pub(crate) fn text_url_add_part(state: &mut TextUrlState, text: &mut &[u8], part_end: i32) {
    // we have [part_begin, max_end) kept part and [max_end, part_end) part to parse text_url entities
    if state.max_end != state.part_begin {
        // add all entities from the kept part
        let kept_part_text = utf8_utf16_substr(text, 0, state.max_end - state.part_begin);
        let kept_size = kept_part_text.len();
        state.result_text.extend_from_slice(kept_part_text);
        *text = &text[kept_size..];

        state.result_entities.append(&mut state.part_entities);
        state.result_text_utf16_length += state.max_end - state.part_begin;
    }

    let mut splittable_entity_pos = [0usize; SPLITTABLE_ENTITY_TYPE_COUNT];
    if part_end != state.max_end {
        // try to find text_url entities in the left part
        let parsed_part_text = utf8_utf16_substr(text, 0, part_end - state.max_end);
        let parsed_size = parsed_part_text.len();
        *text = &text[parsed_size..];

        let text_urls = find_text_url_entities_v3(parsed_part_text);

        let mut text_utf16_offset = state.max_end;
        let mut prev_pos = 0usize;
        for &[text_begin_pos, text_end_pos, url_begin_pos, url_end_pos] in &text_urls {
            let before_text_url = &parsed_part_text[prev_pos..text_begin_pos];
            let before_text_url_length = text_length(before_text_url);
            state.result_text_utf16_length += before_text_url_length;
            state.result_text.extend_from_slice(before_text_url);
            text_utf16_offset += before_text_url_length;

            let text_url = &parsed_part_text[text_begin_pos + 1..text_end_pos];
            let text_url_length = text_length(text_url);
            let url = &parsed_part_text[url_begin_pos + 1..url_end_pos];
            let url_length = text_length(url);
            let mut text_url_entity =
                TextEntity::new(EType::TextUrl, state.result_text_utf16_length, text_url_length);
            text_url_entity.argument = bytes_to_string(get_checked_link(url).unwrap_or_default());
            state.result_entities.push(text_url_entity);
            state.result_text.extend_from_slice(text_url);
            state.result_text_utf16_length += text_url_length;

            let initial_utf16_length = 1 + text_url_length + 1 + 1 + url_length + 1;

            // adjust splittable entities, removing deleted parts from them
            // in the segment [text_utf16_offset, text_utf16_offset + initial_utf16_length)
            // the first character and the last (url_utf16_length + 3) characters are deleted
            for index in 0..SPLITTABLE_ENTITY_TYPE_COUNT {
                loop {
                    let pos = splittable_entity_pos[index];
                    if pos >= state.part_splittable_entities[index].len()
                        || state.part_splittable_entities[index][pos].offset
                            >= text_utf16_offset + initial_utf16_length
                    {
                        break;
                    }
                    let current = state.part_splittable_entities[index][pos].clone();
                    let mut offset = current.offset;
                    let mut length = current.length;
                    if offset + length > text_utf16_offset + 1 + text_url_length {
                        // ends after last removed part; truncate length
                        length = text_utf16_offset + 1 + text_url_length - offset;
                    }
                    if offset >= text_utf16_offset + 1 {
                        offset -= 1;
                    } else if offset + length >= text_utf16_offset + 1 {
                        length -= 1;
                    }
                    if length > 0 {
                        let skipped_length = state.skipped_length;
                        if offset < text_utf16_offset && offset + length > text_utf16_offset {
                            // entity intersects start on the new text_url entity; split it
                            state.result_entities.push(TextEntity::new(
                                current.etype,
                                offset - skipped_length,
                                text_utf16_offset - offset,
                            ));
                            length -= text_utf16_offset - offset;
                            offset = text_utf16_offset;
                        }
                        state.result_entities.push(TextEntity::new(
                            current.etype,
                            offset - skipped_length,
                            length,
                        ));
                    }
                    let entity = &mut state.part_splittable_entities[index][pos];
                    if entity.offset + entity.length > text_utf16_offset + initial_utf16_length {
                        // begins before end of the segment, but ends after it
                        // need to keep the entity for future segments, so split the entity
                        entity.length = entity.offset + entity.length - (text_utf16_offset + initial_utf16_length);
                        entity.offset = text_utf16_offset + initial_utf16_length;
                    } else {
                        splittable_entity_pos[index] += 1;
                    }
                }
            }
            text_utf16_offset += initial_utf16_length;

            state.skipped_length += 2 + 2 + url_length;
            prev_pos = url_end_pos + 1;
        }

        state.result_text.extend_from_slice(&parsed_part_text[prev_pos..]);
        state.result_text_utf16_length += part_end - text_utf16_offset;
    }

    // now add all left splittable entities from [part_begin, part_end)
    for index in 0..SPLITTABLE_ENTITY_TYPE_COUNT {
        loop {
            let pos = splittable_entity_pos[index];
            if pos >= state.part_splittable_entities[index].len()
                || state.part_splittable_entities[index][pos].offset >= part_end
            {
                break;
            }
            let current = state.part_splittable_entities[index][pos].clone();
            let skipped_length = state.skipped_length;
            if current.offset + current.length > part_end {
                // begins before end of the segment, but ends after it
                // need to keep the entity for future segments, so split the entity
                // entities don't intersect each other, so there can be at most one such entity
                state.result_entities.push(TextEntity::new(
                    current.etype,
                    current.offset - skipped_length,
                    part_end - current.offset,
                ));

                let entity = &mut state.part_splittable_entities[index][pos];
                entity.length = entity.offset + entity.length - part_end;
                entity.offset = part_end;
            } else {
                state.result_entities.push(TextEntity::new(
                    current.etype,
                    current.offset - skipped_length,
                    current.length,
                ));
                splittable_entity_pos[index] += 1;
            }
        }
        let splittable_entities = &mut state.part_splittable_entities[index];
        if splittable_entity_pos[index] == splittable_entities.len() {
            splittable_entities.clear();
        } else {
            // only one entity can be left
            let last = splittable_entities.last().cloned().unwrap();
            splittable_entities.clear();
            splittable_entities.push(last);
        }
    }

    state.part_begin = part_end;
}

// entities must be valid for the text
pub(crate) fn parse_text_url_entities_v3(text: &[u8], entities: &[TextEntity]) -> (Vec<u8>, Vec<TextEntity>) {
    // continuous entities can't intersect TextUrl entities,
    // so try to find new TextUrl entities only between the predetermined continuous entities
    let mut state = TextUrlState {
        result_text: Vec::new(),
        result_entities: Vec::new(),
        result_text_utf16_length: 0,
        part_entities: Vec::new(),
        part_splittable_entities: std::array::from_fn(|_| Vec::new()),
        part_begin: 0,
        max_end: 0,
        skipped_length: 0,
    };
    let mut text = text;
    for entity in entities {
        if is_splittable_entity(entity.etype) {
            let index = splittable_entity_type_index(entity.etype);
            state.part_splittable_entities[index].push(entity.clone());
            continue;
        }
        debug_assert!(is_continuous_entity(entity.etype));

        if entity.offset > state.max_end {
            // found a gap from max_end to entity.offset between predetermined entities
            text_url_add_part(&mut state, &mut text, entity.offset);
        }

        state.max_end = entity.offset + entity.length;
        let mut kept = entity.clone();
        kept.offset -= state.skipped_length;
        state.part_entities.push(kept);
    }
    let part_end = state.part_begin + text_length(text);
    text_url_add_part(&mut state, &mut text, part_end);

    (state.result_text, state.result_entities)
}

pub(crate) fn find_splittable_entities_v3(text: &[u8], entities: &[TextEntity]) -> Vec<TextEntity> {
    let mut unallowed_boundaries: HashSet<i32> = HashSet::new();
    for entity in entities {
        unallowed_boundaries.insert(entity.offset + 1);
        unallowed_boundaries.insert(entity.offset + entity.length + 1);
        if matches!(
            entity.etype,
            EType::Mention
                | EType::Hashtag
                | EType::BotCommand
                | EType::Cashtag
                | EType::PhoneNumber
                | EType::BankCardNumber
                | EType::TonAddress
        ) {
            for i in 1..entity.length {
                unallowed_boundaries.insert(entity.offset + i + 1);
            }
        }
    }

    // URLs and email addresses absorb adjacent markup like "~~t.me/a~~", so markup
    // at their beginning can only open an entity and markup at their end can only
    // close one, while markup inside them isn't parsed
    let mut opening_only_positions: HashSet<i32> = HashSet::new();
    let mut closing_only_positions: HashSet<i32> = HashSet::new();
    let is_markup_character = |c: u8| matches!(c, b'_' | b'*' | b'~' | b'|');

    let chars: Vec<char> = String::from_utf8_lossy(text).chars().collect();
    let chars_offset = utf16_prefix(&chars);
    let mut left_text = text;
    let mut left_text_offset = 0i32;
    for (position, length, kind) in find_entities(&chars) {
        if matches!(kind, Kind::MediaTimestamp(_)) {
            // skip_media_timestamps
            continue;
        }
        let offset = chars_offset[position] as i32;
        let length = (chars_offset[position + length] - chars_offset[position]) as i32;

        if !matches!(kind, Kind::Url | Kind::EmailAddress) {
            for i in 0..=length {
                unallowed_boundaries.insert(offset + i + 1);
            }
            continue;
        }

        let skipped = utf8_utf16_substr(left_text, 0, offset - left_text_offset);
        left_text = &left_text[skipped.len()..];
        let entity_text = utf8_utf16_substr(left_text, 0, length);
        left_text = &left_text[entity_text.len()..];
        left_text_offset = offset + length;

        let mut prefix_length = 0i32;
        while prefix_length < length && is_markup_character(entity_text[prefix_length as usize]) {
            opening_only_positions.insert(offset + prefix_length + 1);
            prefix_length += 1;
        }
        let mut suffix_length = 0i32;
        while prefix_length + suffix_length < length
            && is_markup_character(entity_text[entity_text.len() - suffix_length as usize - 1])
        {
            suffix_length += 1;
            closing_only_positions.insert(offset + length - suffix_length + 1);
        }
        for i in prefix_length + 1..length - suffix_length {
            unallowed_boundaries.insert(offset + i + 1);
        }
    }

    let mut result = Vec::new();
    let mut splittable_entity_offset = [0i32; SPLITTABLE_ENTITY_TYPE_COUNT];
    let mut utf16_offset = 0i32;
    let mut i = 0usize;
    while i + 1 < text.len() {
        let c = text[i];
        if utf8_is_first_code_unit(c) {
            utf16_offset += 1 + i32::from(c >= 0xf0);
        }
        if is_markup_character(c)
            && text[i] == text[i + 1]
            && !unallowed_boundaries.contains(&(utf16_offset + 1))
        {
            let mut j = i + 2;
            while j != text.len()
                && text[j] == text[i]
                && !unallowed_boundaries.contains(&(utf16_offset + (j - i) as i32))
            {
                j += 1;
            }
            if j == i + 2 {
                let etype = match c {
                    b'_' => EType::Italic,
                    b'*' => EType::Bold,
                    b'~' => EType::Strikethrough,
                    _ => EType::Spoiler,
                };
                let index = splittable_entity_type_index(etype);
                if splittable_entity_offset[index] != 0 {
                    if !opening_only_positions.contains(&utf16_offset) {
                        let length = utf16_offset - splittable_entity_offset[index] - 1;
                        if length > 0 {
                            result.push(TextEntity::new(etype, splittable_entity_offset[index], length));
                        }
                        splittable_entity_offset[index] = 0;
                    }
                } else if !closing_only_positions.contains(&utf16_offset) {
                    splittable_entity_offset[index] = utf16_offset + 1;
                }
            }
            utf16_offset += (j - i - 1) as i32;
            i = j - 1;
        }
        i += 1;
    }
    result
}

pub(crate) fn parse_pre_entities_v3(text: &[u8]) -> (Vec<u8>, Vec<TextEntity>) {
    let mut result = Vec::new();
    let mut entities = Vec::new();
    let size = text.len();
    let mut utf16_offset = 0i32;
    let mut i = 0usize;
    while i < size {
        let c = text[i];
        if c != b'`' {
            if utf8_is_first_code_unit(c) {
                utf16_offset += 1 + i32::from(c >= 0xf0);
            }
            result.push(c);
            i += 1;
            continue;
        }

        let mut j = i + 1;
        while j < size && text[j] == b'`' {
            j += 1;
        }

        if j - i == 1 || j - i == 3 {
            // trying to find end of the entity
            let mut entity_length = 0i32;
            let mut is_found = false;
            let mut end_tag_begin = j;
            while end_tag_begin < size {
                let cur_c = text[end_tag_begin];
                if cur_c == b'`' {
                    // possible end tag
                    let mut end_tag_end = end_tag_begin + 1;
                    while end_tag_end < size && text[end_tag_end] == b'`' {
                        end_tag_end += 1;
                    }
                    if end_tag_end - end_tag_begin == j - i {
                        // end tag found
                        let mut entity_begin = j;
                        let mut language_code: &[u8] = b"";
                        if j - i == 3 {
                            let mut language_code_end = j;
                            while language_code_end < end_tag_begin - 1
                                && (33..=126).contains(&text[language_code_end])
                            {
                                language_code_end += 1;
                            }
                            if language_code_end < end_tag_begin - 1 && text[language_code_end] == b'\n' {
                                language_code = &text[entity_begin..language_code_end];
                                entity_begin = language_code_end + 1;
                                entity_length -= (entity_begin - j) as i32;
                            }
                        }
                        if !language_code.is_empty() {
                            let mut entity = TextEntity::new(EType::PreCode, utf16_offset, entity_length);
                            entity.argument = bytes_to_string(language_code.to_vec());
                            entities.push(entity);
                        } else {
                            let etype = if j - i == 3 { EType::Pre } else { EType::Code };
                            entities.push(TextEntity::new(etype, utf16_offset, entity_length));
                        }
                        result.extend_from_slice(&text[entity_begin..end_tag_begin]);
                        utf16_offset += entity_length;
                        i = end_tag_end - 1;
                        is_found = true;
                        break;
                    } else {
                        // not an end tag, skip
                        entity_length += (end_tag_end - end_tag_begin) as i32;
                        end_tag_begin = end_tag_end - 1;
                    }
                } else if utf8_is_first_code_unit(cur_c) {
                    entity_length += 1 + i32::from(cur_c >= 0xf0);
                }
                end_tag_begin += 1;
            }
            if is_found {
                i += 1;
                continue;
            }
        }

        result.extend_from_slice(&text[i..j]);
        utf16_offset += (j - i) as i32;
        i = j;
    }
    (result, entities)
}

pub(crate) struct PreEntitiesState<'a> {
    text: &'a [u8],
    result_text: Vec<u8>,
    result_entities: Vec<TextEntity>,
    result_text_utf16_length: i32,
    part_begin: i32,
    max_end: i32,
    skipped_length: i32,
}

impl PreEntitiesState<'_> {
    pub(crate) fn add_part(&mut self, part_end: i32) {
        // we have [part_begin, max_end) kept part and [max_end, part_end) part to parse pre entities
        if self.max_end != self.part_begin {
            // add the kept part
            let kept_part_text = utf8_utf16_substr(self.text, 0, self.max_end - self.part_begin);
            let kept_size = kept_part_text.len();
            self.result_text.extend_from_slice(kept_part_text);
            self.text = &self.text[kept_size..];

            self.result_text_utf16_length += self.max_end - self.part_begin;
        }

        if part_end != self.max_end {
            // try to find pre entities in the left part
            let parsed_part_text = utf8_utf16_substr(self.text, 0, part_end - self.max_end);
            let parsed_size = parsed_part_text.len();
            self.text = &self.text[parsed_size..];

            if !parsed_part_text.contains(&b'`') {
                // fast path, no pre entities; just append the text
                self.result_text.extend_from_slice(parsed_part_text);
                self.result_text_utf16_length += part_end - self.max_end;
            } else {
                let (parsed_text, parsed_entities) = parse_pre_entities_v3(parsed_part_text);
                let new_skipped_length = (parsed_part_text.len() - parsed_text.len()) as i32;
                self.result_text.extend_from_slice(&parsed_text);
                let offset = self.result_text_utf16_length;
                for mut entity in parsed_entities {
                    entity.offset += offset;
                    self.result_entities.push(entity);
                }
                self.result_text_utf16_length += part_end - self.max_end - new_skipped_length;
                self.skipped_length += new_skipped_length;
            }
        }

        self.part_begin = part_end;
    }
}

// entities must be valid for the text; nothing can intersect pre entities, so
// ignore all '`' inside the predetermined entities
pub(crate) fn parse_pre_entities_v3_with_entities(
    text: &[u8],
    entities: Vec<TextEntity>,
) -> (Vec<u8>, Vec<TextEntity>) {
    let mut state = PreEntitiesState {
        text,
        result_text: Vec::new(),
        result_entities: Vec::new(),
        result_text_utf16_length: 0,
        part_begin: 0,
        max_end: 0,
        skipped_length: 0,
    };

    for entity in entities {
        if entity.offset > state.max_end {
            // found a gap from max_end to entity.offset between predetermined entities
            state.add_part(entity.offset);
        }

        state.max_end = state.max_end.max(entity.offset + entity.length);
        let mut kept = entity;
        kept.offset -= state.skipped_length;
        state.result_entities.push(kept);
    }
    let part_end = state.part_begin + text_length(state.text);
    state.add_part(part_end);

    (state.result_text, state.result_entities)
}

pub(crate) type MarkdownParser = fn(&[u8], Vec<TextEntity>) -> (Vec<u8>, Vec<TextEntity>);

pub(crate) struct MarkdownPartParser<'a> {
    left_text: &'a [u8],
    result_text: Vec<u8>,
    result_entities: Vec<TextEntity>,
    part_entities: Vec<TextEntity>,
    result_text_utf16_length: i32,
    part_begin: i32,
}

impl<'a> MarkdownPartParser<'a> {
    pub(crate) fn new(text: &'a [u8]) -> MarkdownPartParser<'a> {
        MarkdownPartParser {
            left_text: text,
            result_text: Vec::new(),
            result_entities: Vec::new(),
            part_entities: Vec::new(),
            result_text_utf16_length: 0,
            part_begin: 0,
        }
    }

    pub(crate) fn add_part(&mut self, part_end: i32, parse: MarkdownParser) {
        let part_text = utf8_utf16_substr(self.left_text, 0, part_end - self.part_begin);
        let part_size = part_text.len();
        self.left_text = &self.left_text[part_size..];

        let (text, mut entities) = parse(part_text, std::mem::take(&mut self.part_entities));

        self.result_text.extend_from_slice(&text);
        for entity in entities.iter_mut() {
            entity.offset += self.result_text_utf16_length;
        }
        self.result_entities.append(&mut entities);
        self.result_text_utf16_length += text_length(&text);
        self.part_begin = part_end;
    }
}

// entities must be valid and can contain only splittable and continuous entities
// __italic__ ~~strikethrough~~ **bold** ||spoiler|| and [text_url](telegram.org)
// entities are left to be parsed
pub(crate) fn parse_markdown_v3_without_pre(text: &[u8], entities: Vec<TextEntity>) -> (Vec<u8>, Vec<TextEntity>) {
    let mut text = text;
    let mut entities = entities;
    let parsed_text_url_text;
    if text.contains(&b'[') {
        let (parsed_text, parsed_entities) = parse_text_url_entities_v3(text, &entities);
        parsed_text_url_text = parsed_text;
        text = &parsed_text_url_text;
        entities = parsed_entities;
    }
    // splittable entities are sorted only within a fixed type now

    let mut have_splittable_entities = false;
    let mut i = 0usize;
    while i + 1 < text.len() {
        if matches!(text[i], b'_' | b'*' | b'~' | b'|') && text[i] == text[i + 1] {
            have_splittable_entities = true;
            break;
        }
        i += 1;
    }
    if !have_splittable_entities {
        // fast path
        sort_entities(&mut entities);
        return (text.to_vec(), entities);
    }

    let found_splittable_entities = find_splittable_entities_v3(text, &entities);
    let mut removed_pos: Vec<i32> = Vec::new();
    for entity in &found_splittable_entities {
        removed_pos.push(entity.offset - 1);
        removed_pos.push(entity.offset + entity.length + 1);
    }
    removed_pos.sort_unstable();

    let mut new_text = Vec::with_capacity(text.len().saturating_sub(2 * removed_pos.len()));
    let mut j = 0usize;
    let mut utf16_offset = 0i32;
    let mut i = 0usize;
    while i < text.len() {
        let c = text[i];
        if utf8_is_first_code_unit(c) {
            utf16_offset += 1 + i32::from(c >= 0xf0);
        }
        if j < removed_pos.len() && utf16_offset == removed_pos[j] {
            // skip the markup character
            i += 1;
            utf16_offset += 1;
            j += 1;
        } else {
            new_text.push(text[i]);
        }
        i += 1;
    }

    entities.extend(found_splittable_entities);
    for entity in entities.iter_mut() {
        let removed_before_begin = removed_pos.partition_point(|&pos| pos <= entity.offset) as i32;
        let removed_before_end = removed_pos.partition_point(|&pos| pos <= entity.offset + entity.length) as i32;
        entity.length -= 2 * (removed_before_end - removed_before_begin);
        entity.offset -= 2 * removed_before_begin;
    }

    remove_empty_entities(&mut entities);

    sort_entities(&mut entities);
    (new_text, entities)
}

// entities must be valid and can contain only splittable, continuous and pre entities
pub(crate) fn parse_markdown_v3_without_blockquote(
    text: &[u8],
    entities: Vec<TextEntity>,
) -> (Vec<u8>, Vec<TextEntity>) {
    let mut text = text.to_vec();
    let mut entities = entities;
    if text.contains(&b'`') {
        let (parsed_text, parsed_entities) = parse_pre_entities_v3_with_entities(&text, entities);
        text = parsed_text;
        entities = parsed_entities;
    }

    if !entities.iter().any(|entity| is_pre_entity(entity.etype)) {
        // fast path
        return parse_markdown_v3_without_pre(&text, entities);
    }

    let mut parser = MarkdownPartParser::new(&text);
    let mut i = 0usize;
    while i < entities.len() {
        let entity = entities[i].clone();
        debug_assert!(
            is_splittable_entity(entity.etype)
                || is_pre_entity(entity.etype)
                || is_continuous_entity(entity.etype)
        );
        if is_pre_entity(entity.etype) {
            parser.add_part(entity.offset, parse_markdown_v3_without_pre);

            let part_text = utf8_utf16_substr(parser.left_text, 0, entity.length);
            let part_size = part_text.len();
            parser.left_text = &parser.left_text[part_size..];

            parser.result_text.extend_from_slice(part_text);
            let mut kept = entity.clone();
            kept.offset = parser.result_text_utf16_length;
            parser.result_entities.push(kept);
            parser.result_text_utf16_length += entity.length;
            parser.part_begin = entity.offset + entity.length;
        } else {
            let mut kept = entity.clone();
            kept.offset -= parser.part_begin;
            parser.part_entities.push(kept);
        }
        i += 1;
    }
    let part_end = parser.part_begin + text_length(parser.left_text);
    parser.add_part(part_end, parse_markdown_v3_without_pre);
    (parser.result_text, parser.result_entities)
}

// text entities must be valid; returned entities must be resplit and fixed
pub(crate) fn parse_markdown_v3(text: &[u8], entities: Vec<TextEntity>) -> (Vec<u8>, Vec<TextEntity>) {
    if !entities.iter().any(|entity| is_blockquote_entity(entity.etype)) {
        // fast path
        return parse_markdown_v3_without_blockquote(text, entities);
    }

    let mut parser = MarkdownPartParser::new(text);
    let mut i = 0usize;
    while i < entities.len() {
        let entity = entities[i].clone();
        debug_assert!(
            is_splittable_entity(entity.etype)
                || is_pre_entity(entity.etype)
                || is_continuous_entity(entity.etype)
                || is_blockquote_entity(entity.etype)
        );
        if is_blockquote_entity(entity.etype) {
            parser.add_part(entity.offset, parse_markdown_v3_without_blockquote);

            let offset = parser.result_text_utf16_length;
            let mut quote = entity.clone();
            quote.offset = offset;
            let index = parser.result_entities.len();
            parser.result_entities.push(quote);

            while i + 1 < entities.len()
                && entities[i + 1].offset + entities[i + 1].length <= entity.offset + entity.length
            {
                i += 1;
                let mut next = entities[i].clone();
                next.offset -= parser.part_begin;
                parser.part_entities.push(next);
            }
            parser.add_part(entity.offset + entity.length, parse_markdown_v3_without_blockquote);

            parser.result_entities[index].length = parser.result_text_utf16_length - offset;
        } else {
            let mut kept = entity.clone();
            kept.offset -= parser.part_begin;
            parser.part_entities.push(kept);
        }
        i += 1;
    }
    let part_end = parser.part_begin + text_length(parser.left_text);
    parser.add_part(part_end, parse_markdown_v3_without_blockquote);

    (parser.result_text, parser.result_entities)
}

pub(crate) fn is_valid_language_code(code: &[u8]) -> bool {
    code.iter().all(|&c| (33..=126).contains(&c))
}

// text entities must be valid
pub(crate) fn get_markdown_v3(text: String, entities: Vec<TextEntity>) -> (String, Vec<TextEntity>) {
    if entities.is_empty() {
        return (text, entities);
    }
    if entities.iter().any(|entity| !is_user_entity(entity.etype)) {
        return (text, entities);
    }

    let mut result_text: Vec<u8> = Vec::with_capacity(text.len());
    let mut result_entities: Vec<TextEntity> = Vec::new();
    let mut nested_entities_stack: Vec<(TextEntity, i32)> = Vec::new();
    let mut current_entity = 0usize;
    let mut utf16_offset = 0i32;
    let mut utf16_added = 0i32;

    let source = text.as_bytes();
    for pos in 0..=source.len() {
        let c = source.get(pos).copied().unwrap_or(0);
        if utf8_is_first_code_unit(c) {
            while let Some((entity, added_before)) = nested_entities_stack.last() {
                let entity_end = entity.offset + entity.length;
                if utf16_offset < entity_end {
                    break;
                }
                let entity = entity.clone();
                let added_before = *added_before;
                nested_entities_stack.pop();

                let mut need_entity = false;
                match entity.etype {
                    EType::Italic => {
                        result_text.extend_from_slice(b"__");
                        utf16_added += 2;
                    }
                    EType::Bold => {
                        result_text.extend_from_slice(b"**");
                        utf16_added += 2;
                    }
                    EType::Strikethrough => {
                        result_text.extend_from_slice(b"~~");
                        utf16_added += 2;
                    }
                    EType::Spoiler => {
                        result_text.extend_from_slice(b"||");
                        utf16_added += 2;
                    }
                    EType::TextUrl => {
                        result_text.extend_from_slice(b"](");
                        result_text.extend_from_slice(entity.argument.as_bytes());
                        result_text.push(b')');
                        utf16_added += 3 + text_length(entity.argument.as_bytes());
                    }
                    EType::Code => {
                        result_text.push(b'`');
                        utf16_added += 1;
                    }
                    EType::Pre => {
                        result_text.extend_from_slice(b"```");
                        utf16_added += 3;
                    }
                    EType::PreCode => {
                        if is_valid_language_code(entity.argument.as_bytes()) {
                            result_text.extend_from_slice(b"```");
                            utf16_added += 3;
                        } else {
                            need_entity = true;
                        }
                    }
                    _ => need_entity = true,
                }
                if need_entity {
                    let mut kept = entity;
                    kept.offset += added_before;
                    kept.length += utf16_added - added_before;
                    result_entities.push(kept);
                }
            }

            while current_entity < entities.len() && utf16_offset >= entities[current_entity].offset {
                let entity = &entities[current_entity];
                match entity.etype {
                    EType::Italic => {
                        result_text.extend_from_slice(b"__");
                        utf16_added += 2;
                    }
                    EType::Bold => {
                        result_text.extend_from_slice(b"**");
                        utf16_added += 2;
                    }
                    EType::Strikethrough => {
                        result_text.extend_from_slice(b"~~");
                        utf16_added += 2;
                    }
                    EType::Spoiler => {
                        result_text.extend_from_slice(b"||");
                        utf16_added += 2;
                    }
                    EType::TextUrl => {
                        result_text.push(b'[');
                        utf16_added += 1;
                    }
                    EType::Code => {
                        result_text.push(b'`');
                        utf16_added += 1;
                    }
                    EType::Pre => {
                        result_text.extend_from_slice(b"```");
                        utf16_added += 3;
                        if c != b'\n' {
                            result_text.push(b'\n');
                            utf16_added += 1;
                        }
                    }
                    EType::PreCode => {
                        if is_valid_language_code(entity.argument.as_bytes()) {
                            result_text.extend_from_slice(b"```");
                            utf16_added += 3;
                            if !entity.argument.is_empty() {
                                result_text.extend_from_slice(entity.argument.as_bytes());
                                utf16_added += text_length(entity.argument.as_bytes());
                            }
                            if c != b'\n' {
                                result_text.push(b'\n');
                                utf16_added += 1;
                            }
                        }
                    }
                    _ => {
                        // keep as is
                    }
                }
                let kept = entity.clone();
                current_entity += 1;
                nested_entities_stack.push((kept, utf16_added));
            }
            utf16_offset += 1 + i32::from(c >= 0xf0);
        }
        if pos == source.len() {
            break;
        }

        result_text.push(source[pos]);
    }

    sort_entities(&mut result_entities);
    let result_text = bytes_to_string(result_text);

    let (parsed_text, parsed_entities) = parse_markdown_v3(result_text.as_bytes(), result_entities.clone());
    if parsed_text != text.as_bytes() || parsed_entities != entities {
        return (text, entities);
    }
    (result_text, result_entities)
}
