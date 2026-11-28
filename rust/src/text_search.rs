// 拆分自 `rust/src/lib.rs`（一次性 codemod，见 tasks/plan.md T119）；只改可见性与位置，未改实现。
#![allow(unused_imports)]

use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{LazyLock, Mutex};

pub(crate) const CATEGORY_LETTER: u8 = 1;
pub(crate) const CATEGORY_DECIMAL_NUMBER: u8 = 2;
pub(crate) const CATEGORY_NUMBER: u8 = 3;

pub(crate) fn unicode_simple_category(code: u32) -> u8 {
    // The original jump table only narrows the search start; the boundary found
    // this way is the same one its forward scan stops at.
    let packed = (code << 5) + 30;
    let index = UNICODE_SIMPLE_CATEGORY_RANGES.partition_point(|&boundary| boundary <= packed);
    (UNICODE_SIMPLE_CATEGORY_RANGES[index - 1] & 31) as u8
}

/// `binary_search_ranges` from tdutils/td/utils/unicode.cpp: the array holds
/// (range_begin, replacement) pairs. A negative replacement maps the rest of the range
/// linearly, a replacement above 0x10ffff selects one of the case-folding helpers.
pub(crate) fn binary_search_ranges(ranges: &[i32], code: u32) -> u32 {
    if code > 0x10ffff {
        return 0;
    }
    let code_int = code as i32;
    let mut left = 0usize;
    let mut right = ranges.len();
    while left < right {
        let middle = ((left + right + 2) >> 2) << 1;
        let begin = match ranges.get(middle) {
            Some(begin) => *begin,
            // the C++ search reads one past the last pair only to learn it is past the end
            None => i32::MAX,
        };
        if begin <= code_int {
            left = middle;
        } else {
            right = middle - 2;
        }
    }
    let (start, value) = match (ranges.get(left), ranges.get(left + 1)) {
        (Some(start), Some(value)) => (*start, *value),
        _ => return 0,
    };
    if value < 0 {
        return (code_int - start + !value) as u32;
    }
    if value <= 0x10ffff {
        return value as u32;
    }
    match value - 0x200000 {
        0 => code & 0xffff_fffe,
        1 => code | 1,
        2 => (code - 1) | 1,
        _ => 0,
    }
}

pub(crate) fn prepare_search_character(code: u32) -> u32 {
    if code < TABLE_SIZE {
        PREPARE_SEARCH_CHARACTER_TABLE[code as usize] as u32
    } else {
        binary_search_ranges(&PREPARE_SEARCH_CHARACTER_RANGES, code)
    }
}

pub(crate) fn remove_diacritics(code: u32) -> u32 {
    if code < TABLE_SIZE {
        WITHOUT_DIACRITICS_TABLE[code as usize] as u32
    } else {
        binary_search_ranges(&WITHOUT_DIACRITICS_RANGES, code)
    }
}

/// `next_utf8_unsafe` from tdutils/td/utils/utf8.cpp
pub(crate) fn decode_utf8_code(bytes: &[u8], pos: usize) -> (u32, usize) {
    let first = bytes[pos];
    let continuation = |offset: usize| bytes.get(pos + offset).map_or(0, |byte| (byte & 0x3f) as u32);
    if first & 0x80 == 0 {
        (first as u32, pos + 1)
    } else if first & 0x20 == 0 {
        (((first & 0x1f) as u32) << 6 | continuation(1), pos + 2)
    } else if first & 0x10 == 0 {
        (
            ((first & 0x0f) as u32) << 12 | continuation(1) << 6 | continuation(2),
            pos + 3,
        )
    } else {
        (
            ((first & 0x07) as u32) << 18 | continuation(1) << 12 | continuation(2) << 6 | continuation(3),
            pos + 4,
        )
    }
}

/// `utf8_get_search_words` from tdutils/td/utils/utf8.cpp
pub(crate) fn get_search_words(text: &[u8]) -> Vec<Vec<u8>> {
    let mut words = Vec::new();
    let mut word = Vec::new();
    let mut in_word = false;
    let mut pos = 0usize;
    while pos < text.len() {
        let (code, next_pos) = decode_utf8_code(text, pos);
        pos = next_pos;
        let code = prepare_search_character(code);
        if code == 0 {
            continue;
        }
        if code == ' ' as u32 {
            if in_word {
                words.push(std::mem::take(&mut word));
                in_word = false;
            }
        } else {
            in_word = true;
            append_utf8_code(&mut word, remove_diacritics(code));
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// `Hints::fix_words` from tdutils/td/utils/Hints.cpp: keeps only the longest word of
/// every prefix chain, so a word that begins another one is dropped.
pub(crate) fn fix_words(mut words: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    words.sort();
    let mut fixed = Vec::with_capacity(words.len());
    for index in 0..words.len() {
        if index + 1 < words.len() && words[index + 1].starts_with(&words[index]) {
            continue;
        }
        fixed.push(std::mem::take(&mut words[index]));
    }
    if fixed.len() == 1 && fixed[0].is_empty() {
        fixed.clear();
    }
    fixed
}

/// `get_en_to_ru_simple_rules` from tdutils/td/utils/translit.cpp
pub(crate) const EN_TO_RU_SIMPLE_RULES: [(u32, &str); 26] = [
    ('a' as u32, "а"), ('b' as u32, "б"), ('c' as u32, "к"), ('d' as u32, "д"), ('e' as u32, "е"),
    ('f' as u32, "ф"), ('g' as u32, "г"), ('h' as u32, "х"), ('i' as u32, "и"), ('j' as u32, "й"),
    ('k' as u32, "к"), ('l' as u32, "л"), ('m' as u32, "м"), ('n' as u32, "н"), ('o' as u32, "о"),
    ('p' as u32, "п"), ('q' as u32, "к"), ('r' as u32, "р"), ('s' as u32, "с"), ('t' as u32, "т"),
    ('u' as u32, "у"), ('v' as u32, "в"), ('w' as u32, "в"), ('x' as u32, "кс"), ('y' as u32, "и"),
    ('z' as u32, "з"),
];

/// `get_en_to_ru_complex_rules`; the order matters, "shch" is matched before "sh" and "sch"
pub(crate) const EN_TO_RU_COMPLEX_RULES: [(&str, &str); 17] = [
    ("ch", "ч"), ("ei", "ей"), ("ey", "ей"), ("ia", "ия"), ("iy", "ий"), ("jo", "е"),
    ("ju", "ю"), ("ja", "я"), ("kh", "х"), ("shch", "щ"), ("sh", "ш"), ("sch", "щ"),
    ("ts", "ц"), ("yo", "е"), ("yu", "ю"), ("ya", "я"), ("zh", "ж"),
];

/// `get_ru_to_en_simple_rules`
pub(crate) const RU_TO_EN_SIMPLE_RULES: [(u32, &str); 33] = [
    (0x430, "a"), (0x431, "b"), (0x432, "v"), (0x433, "g"), (0x434, "d"), (0x435, "e"),
    (0x451, "e"), (0x436, "zh"), (0x437, "z"), (0x438, "i"), (0x439, "y"), (0x43a, "k"),
    (0x43b, "l"), (0x43c, "m"), (0x43d, "n"), (0x43e, "o"), (0x43f, "p"), (0x440, "r"),
    (0x441, "s"), (0x442, "t"), (0x443, "u"), (0x444, "f"), (0x445, "kh"), (0x446, "ts"),
    (0x447, "ch"), (0x448, "sh"), (0x449, "sch"), (0x44a, ""), (0x44b, "y"), (0x44c, ""),
    (0x44d, "e"), (0x44e, "yu"), (0x44f, "ya"),
];

/// `get_ru_to_en_complex_rules`
pub(crate) const RU_TO_EN_COMPLEX_RULES: [(&str, &str); 5] =
    [("ий", "y"), ("ия", "ia"), ("кс", "x"), ("yo", "e"), ("jo", "e")];

pub(crate) fn append_transliteration(out: &mut Vec<u8>, code: u32, simple_rules: &[(u32, &str)]) {
    match simple_rules.iter().find(|(rule_code, _)| *rule_code == code) {
        Some((_, replacement)) => out.extend_from_slice(replacement.as_bytes()),
        None => append_utf8_code(out, code),
    }
}

/// `add_word_transliterations` from tdutils/td/utils/translit.cpp
pub(crate) fn add_word_transliterations(
    result: &mut Vec<Vec<u8>>,
    word: &[u8],
    allow_partial: bool,
    simple_rules: &[(u32, &str)],
    complex_rules: &[(&str, &str)],
) {
    let mut transliteration = Vec::new();
    let mut pos = 0usize;
    while pos < word.len() {
        let (code, next_pos) = decode_utf8_code(word, pos);
        pos = next_pos;
        append_transliteration(&mut transliteration, code, simple_rules);
    }
    if !transliteration.is_empty() {
        result.push(std::mem::take(&mut transliteration));
    }

    pos = 0;
    while pos < word.len() {
        let suffix = &word[pos..];
        let mut matched = false;
        for (rule_begin, rule_end) in complex_rules {
            if suffix.starts_with(rule_begin.as_bytes()) {
                matched = true;
                pos += rule_begin.len();
                transliteration.extend_from_slice(rule_end.as_bytes());
                break;
            }
            if allow_partial && rule_begin.as_bytes().starts_with(suffix) {
                let mut partial = transliteration.clone();
                partial.extend_from_slice(rule_end.as_bytes());
                result.push(partial);
            }
        }
        if matched {
            continue;
        }
        let (code, next_pos) = decode_utf8_code(word, pos);
        pos = next_pos;
        append_transliteration(&mut transliteration, code, simple_rules);
    }
    if !transliteration.is_empty() {
        result.push(transliteration);
    }
}

/// `get_word_transliterations` from tdutils/td/utils/translit.cpp
pub(crate) fn word_transliterations(word: &[u8], allow_partial: bool) -> Vec<Vec<u8>> {
    let mut result = Vec::new();
    add_word_transliterations(
        &mut result,
        word,
        allow_partial,
        &EN_TO_RU_SIMPLE_RULES,
        &EN_TO_RU_COMPLEX_RULES,
    );
    add_word_transliterations(
        &mut result,
        word,
        allow_partial,
        &RU_TO_EN_SIMPLE_RULES,
        &RU_TO_EN_COMPLEX_RULES,
    );
    result.sort_unstable();
    result.dedup();
    result
}

/// Port of `td::Hints` (tdutils/td/utils/Hints.cpp) for the operations
/// `search_strings_by_prefix` drives: add, set_rating and search.
pub(crate) struct Hints {
    word_to_keys: BTreeMap<Vec<u8>, Vec<i64>>,
    translit_word_to_keys: BTreeMap<Vec<u8>, Vec<i64>>,
    key_to_name: BTreeMap<i64, Vec<u8>>,
    key_to_rating: BTreeMap<i64, i64>,
}

impl Hints {
    pub(crate) fn new() -> Hints {
        Hints {
            word_to_keys: BTreeMap::new(),
            translit_word_to_keys: BTreeMap::new(),
            key_to_name: BTreeMap::new(),
            key_to_rating: BTreeMap::new(),
        }
    }

    pub(crate) fn add(&mut self, key: i64, name: &[u8]) {
        let old_name = match self.key_to_name.get(&key) {
            Some(old_name) => {
                if old_name == name {
                    return;
                }
                old_name.clone()
            }
            None => Vec::new(),
        };
        if !old_name.is_empty() {
            let mut old_transliterations = Vec::new();
            for old_word in fix_words(get_search_words(&old_name)) {
                self.delete_word(&old_word, key, false);
                for transliteration in word_transliterations(&old_word, false) {
                    if transliteration != old_word {
                        old_transliterations.push(transliteration);
                    }
                }
            }
            for word in fix_words(old_transliterations) {
                self.delete_word(&word, key, true);
            }
        }
        if name.is_empty() {
            self.key_to_name.remove(&key);
            self.key_to_rating.remove(&key);
            return;
        }

        let mut transliterations = Vec::new();
        for word in fix_words(get_search_words(name)) {
            self.add_word(&word, key, false);
            for transliteration in word_transliterations(&word, false) {
                if transliteration != word {
                    transliterations.push(transliteration);
                }
            }
        }
        for word in fix_words(transliterations) {
            self.add_word(&word, key, true);
        }
        self.key_to_name.insert(key, name.to_vec());
    }

    pub(crate) fn set_rating(&mut self, key: i64, rating: i64) {
        self.key_to_rating.insert(key, rating);
    }

    pub(crate) fn add_word(&mut self, word: &[u8], key: i64, transliteration: bool) {
        let word_to_keys = if transliteration {
            &mut self.translit_word_to_keys
        } else {
            &mut self.word_to_keys
        };
        word_to_keys.entry(word.to_vec()).or_default().push(key);
    }

    pub(crate) fn delete_word(&mut self, word: &[u8], key: i64, transliteration: bool) {
        let word_to_keys = if transliteration {
            &mut self.translit_word_to_keys
        } else {
            &mut self.word_to_keys
        };
        let mut remove_list = false;
        if let Some(keys) = word_to_keys.get_mut(word) {
            if let Some(position) = keys.iter().position(|&stored_key| stored_key == key) {
                if keys.len() == 1 {
                    remove_list = true;
                } else if let Some(last_key) = keys.pop() {
                    keys[position] = last_key;
                }
            }
        }
        if remove_list {
            word_to_keys.remove(word);
        }
    }

    pub(crate) fn add_search_results(&self, results: &mut Vec<i64>, word: &[u8], transliteration: bool) {
        let word_to_keys = if transliteration {
            &self.translit_word_to_keys
        } else {
            &self.word_to_keys
        };
        for (stored_word, keys) in word_to_keys
            .range::<[u8], _>((std::ops::Bound::Included(word), std::ops::Bound::Unbounded))
        {
            if !stored_word.starts_with(word) {
                break;
            }
            results.extend_from_slice(keys);
        }
    }

    pub(crate) fn search_word(&self, word: &[u8]) -> Vec<i64> {
        let mut results = Vec::new();
        self.add_search_results(&mut results, word, true);
        for transliteration in word_transliterations(word, true) {
            self.add_search_results(&mut results, &transliteration, false);
        }
        results.sort_unstable();
        results.dedup();
        results
    }

    pub(crate) fn rating(&self, key: i64) -> i64 {
        self.key_to_rating.get(&key).copied().unwrap_or(0)
    }

    pub(crate) fn search(&self, query: &[u8], limit: i32, return_all_for_empty_query: bool) -> (usize, Vec<i64>) {
        if limit < 0 {
            return (self.key_to_name.len(), Vec::new());
        }
        let words = fix_words(get_search_words(query));
        let mut results = Vec::new();
        if words.is_empty() {
            if return_all_for_empty_query {
                results.extend(self.key_to_name.keys().copied());
            }
        } else {
            for (index, word) in words.iter().enumerate() {
                let keys = self.search_word(word);
                if index == 0 {
                    results = keys;
                    continue;
                }
                // both lists are sorted by key, so a merge yields the sorted intersection
                let mut intersection = Vec::with_capacity(results.len().min(keys.len()));
                let (mut results_pos, mut keys_pos) = (0usize, 0usize);
                while results_pos < results.len() && keys_pos < keys.len() {
                    match results[results_pos].cmp(&keys[keys_pos]) {
                        std::cmp::Ordering::Less => results_pos += 1,
                        std::cmp::Ordering::Greater => keys_pos += 1,
                        std::cmp::Ordering::Equal => {
                            intersection.push(results[results_pos]);
                            results_pos += 1;
                            keys_pos += 1;
                        }
                    }
                }
                results = intersection;
            }
        }
        let total_size = results.len();
        // CompareByRating: by rating, then by key
        results.sort_unstable_by_key(|&key| (self.rating(key), key));
        results.truncate(limit as usize);
        (total_size, results)
    }
}

/// `search_strings_by_prefix` from td/telegram/misc.cpp
pub(crate) fn search_strings_by_prefix(
    strings: &[Vec<u8>],
    query: &[u8],
    limit: i32,
    return_all_for_empty_query: bool,
) -> (usize, Vec<i32>) {
    let mut hints = Hints::new();
    for (index, string) in strings.iter().enumerate() {
        let key = index as i64;
        hints.add(key, if string.is_empty() { b" " } else { string });
        hints.set_rating(key, key);
    }
    let (total_count, positions) = hints.search(query, limit, return_all_for_empty_query);
    (total_count, positions.into_iter().map(|key| key as i32).collect())
}

pub(crate) fn search_strings_by_prefix_request(request: &Json) -> String {
    let format_error = |message: String| format!("Failed to parse JSON object as TDLib request: {}", message);
    let mut strings = match request.get("strings") {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Arr(values)) => {
            let mut strings = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    // from_json(string) keeps the default for Null
                    Json::Null => strings.push(Vec::new()),
                    Json::Str(bytes) => match std::str::from_utf8(bytes) {
                        Ok(text) => strings.push(text.as_bytes().to_vec()),
                        Err(_) => return error_object(400, "Strings must be encoded in UTF-8"),
                    },
                    other => {
                        return error_object(
                            400,
                            &format_error(format!("Expected String, but receive {}", json_type_name(other))),
                        )
                    }
                }
            }
            strings
        }
        Some(other) => {
            return error_object(
                400,
                &format_error(format!("Expected Array, but receive {}", json_type_name(other))),
            )
        }
    };
    let mut query = match request.get("query") {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::Str(bytes)) => match std::str::from_utf8(bytes) {
            Ok(text) => text.as_bytes().to_vec(),
            Err(_) => return error_object(400, "Strings must be encoded in UTF-8"),
        },
        Some(other) => {
            return error_object(
                400,
                &format_error(format!("Expected String, but receive {}", json_type_name(other))),
            )
        }
    };
    let limit = match request.get("limit") {
        None | Some(Json::Null) => 0,
        Some(value) => match json_int32_field(value) {
            Ok(limit) => limit,
            Err(message) => return error_object(400, &format_error(message)),
        },
    };
    let return_all_for_empty_query = !json_flag(request.get("return_none_for_empty_query"));
    if !clean_input_string(&mut query) {
        return error_object(400, "Strings must be encoded in UTF-8");
    }
    for string in &mut strings {
        if !clean_input_string(string) {
            return error_object(400, "Strings must be encoded in UTF-8");
        }
    }
    let (total_count, positions) = search_strings_by_prefix(&strings, &query, limit, return_all_for_empty_query);
    let mut out = String::from("{\"@type\":\"foundPositions\",\"total_count\":");
    out.push_str(&total_count.to_string());
    out.push_str(",\"positions\":[");
    for (index, position) in positions.iter().enumerate() {
        if index != 0 {
            out.push(',');
        }
        out.push_str(&position.to_string());
    }
    out.push_str("]}");
    out
}

pub(crate) fn is_filename_char(code: u32) -> bool {
    if code < 32 {
        return false;
    }
    if code < 127 {
        // < > : " / \ | ? * & ` '
        return !matches!(
            code,
            0x22 | 0x26 | 0x27 | 0x2a | 0x2f | 0x3a | 0x3c | 0x3e | 0x3f | 0x5c | 0x60 | 0x7c
        );
    }
    matches!(
        unicode_simple_category(code),
        CATEGORY_LETTER | CATEGORY_DECIMAL_NUMBER | CATEGORY_NUMBER
    )
}

pub(crate) fn clean_filename_part(name: &str, max_length: usize) -> String {
    let mut result = String::new();
    let mut size = 0;
    for character in name.chars() {
        if size >= max_length {
            break;
        }
        let mut code = character;
        if !is_filename_char(code as u32) {
            if prepare_search_character(code as u32) == 0 {
                continue;
            }
            code = ' ';
        }
        if result.is_empty() && (code == ' ' || code == '.') {
            continue;
        }
        result.push(code);
        size += 1;
    }
    while result.ends_with(' ') || result.ends_with('.') {
        result.pop();
    }
    result
}

pub(crate) fn path_view(path: &str) -> (usize, usize) {
    // (PathView::last_slash_ + 1, PathView::last_dot_)
    let bytes = path.as_bytes();
    let size = bytes.len();
    let mut last_slash = size as isize - 1;
    while last_slash >= 0 && bytes[last_slash as usize] != b'/' && bytes[last_slash as usize] != b'\\' {
        last_slash -= 1;
    }
    let mut last_dot = size;
    let mut index = size as isize - 1;
    while index > last_slash + 1 {
        if bytes[index as usize] == b'.' {
            last_dot = index as usize;
            break;
        }
        index -= 1;
    }
    ((last_slash + 1) as usize, last_dot)
}

pub(crate) fn path_extension(path: &str) -> &str {
    let (_, last_dot) = path_view(path);
    if last_dot == path.len() {
        return "";
    }
    &path[last_dot + 1..]
}

pub(crate) fn path_file_stem(path: &str) -> &str {
    let (file_name_start, last_dot) = path_view(path);
    &path[file_name_start..last_dot]
}

pub(crate) fn clean_filename(name: &str) -> String {
    let file_name = clean_filename_part(path_file_stem(name), 64);
    let extension = clean_filename_part(path_extension(name), 16);
    if extension.is_empty() {
        return file_name;
    }
    if file_name.is_empty() {
        return extension;
    }
    let mut result = file_name;
    result.push('.');
    result.push_str(&extension);
    result
}

/// `emoji::is_emoji` also splits on zero-width joiners, so a ZWJ sequence is one
/// emoji here exactly like it is on the C++ side (the table itself has none).
fn is_emoji(value: &str) -> bool {
    crate::emoji::is_emoji_str(value)
}

pub(crate) fn match_emoji_at(text: &str, pos: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let max_length = (bytes.len() - pos).min(28);
    for length in (1..=max_length).rev() {
        let end = pos + length;
        if !text.is_char_boundary(end) {
            continue;
        }
        if is_emoji(&text[pos..end]) {
            return Some(length);
        }
    }
    None
}

pub(crate) fn skip_emoji_selector(bytes: &[u8], pos: usize) -> usize {
    if pos + 3 <= bytes.len() && bytes[pos] == 0xEF && bytes[pos + 1] == 0xB8 && bytes[pos + 2] == 0x8F {
        pos + 3
    } else {
        pos
    }
}

pub(crate) fn find_emoji_ranges(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(length) = match_emoji_at(text, i) else {
            i += utf8_width(bytes[i]);
            continue;
        };
        let mut end = skip_emoji_selector(bytes, i + length);
        while end + 3 < bytes.len()
            && bytes[end] == 0xE2
            && bytes[end + 1] == 0x80
            && bytes[end + 2] == 0x8D
        {
            match match_emoji_at(text, end + 3) {
                Some(next_length) => end = skip_emoji_selector(bytes, end + 3 + next_length),
                None => break,
            }
        }
        ranges.push((i, end - i));
        i = end;
    }
    ranges
}

pub(crate) fn emoji_ranges_json(text: &str) -> String {
    let mut utf16_positions: Vec<(usize, usize)> = Vec::new();
    let mut utf16 = 0usize;
    for (byte_pos, c) in text.char_indices() {
        utf16_positions.push((byte_pos, utf16));
        utf16 += c.len_utf16();
    }
    let to_utf16 = |byte_pos: usize| -> usize {
        match utf16_positions.binary_search_by_key(&byte_pos, |(byte, _)| *byte) {
            Ok(index) => utf16_positions[index].1,
            Err(index) => utf16_positions[index - 1].1 + text[utf16_positions[index - 1].0..byte_pos].chars().map(char::len_utf16).sum::<usize>(),
        }
    };
    let parts: Vec<String> = find_emoji_ranges(text)
        .into_iter()
        .map(|(start, length)| {
            let offset = to_utf16(start);
            let end = to_utf16(start + length);
            format!(
                "{{\"@type\":\"textRange\",\"offset\":{},\"length\":{}}}",
                offset,
                end - offset
            )
        })
        .collect();
    format!(
        "{{\"@type\":\"textRanges\",\"ranges\":[{}]}}",
        parts.join(",")
    )
}
