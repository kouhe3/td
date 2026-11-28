//! tdutils Hints on the Rust side, including unicode search words and the
//! transliteration maps of translit.cpp.
//!
//! ```text
//! hints_probe <script.txt>
//! ```
//!
//! Script lines: `add`, `remove`, `rate`, `search`, `sempty`, `has`, `name`,
//! `size`, `words`.
//!
//! `utf8_get_search_words` folds case, full-width ASCII, diacritics and the
//! Cyrillic и/й, е/ё pairs through the lookups of the generated
//! `unicode_tables.rs`; `fix_words` sorts the words and drops every word that is
//! a prefix of the next one. `add` indexes those words into one map and their
//! transliterations (the four rule tables of translit.cpp) into a second one.
//! `search_word` searches the transliteration map first, then the
//! transliterations of the query with `allow_partial = true` in the word map,
//! then dedups. `search` intersects the per-word key lists, reports the count
//! before the limit, and sorts by (rating ascending, key ascending).

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::ops::Bound;
use std::process::ExitCode;

#[allow(dead_code)]
#[path = "../unicode_tables.rs"]
mod unicode_tables;

struct Hints {
    word_to_keys: BTreeMap<Vec<u8>, Vec<i64>>,
    translit_word_to_keys: BTreeMap<Vec<u8>, Vec<i64>>,
    key_to_name: HashMap<i64, Vec<u8>>,
    key_to_rating: HashMap<i64, i64>,
}

fn fix_words(mut words: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    words.sort();
    let mut kept: Vec<Vec<u8>> = Vec::with_capacity(words.len());
    for index in 0..words.len() {
        let is_last = index + 1 == words.len();
        if is_last || !words[index + 1].starts_with(&words[index]) {
            kept.push(words[index].clone());
        }
    }
    if kept.len() == 1 && kept[0].is_empty() {
        kept.clear();
    }
    kept
}

fn binary_search_ranges(ranges: &[i32], code: u32) -> u32 {
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

fn prepare_search_character(code: u32) -> u32 {
    if code < unicode_tables::TABLE_SIZE {
        unicode_tables::PREPARE_SEARCH_CHARACTER_TABLE[code as usize] as u32
    } else {
        binary_search_ranges(&unicode_tables::PREPARE_SEARCH_CHARACTER_RANGES, code)
    }
}

fn remove_diacritics(code: u32) -> u32 {
    if code < unicode_tables::TABLE_SIZE {
        unicode_tables::WITHOUT_DIACRITICS_TABLE[code as usize] as u32
    } else {
        binary_search_ranges(&unicode_tables::WITHOUT_DIACRITICS_RANGES, code)
    }
}

fn next_utf8(data: &[u8], pos: usize) -> (u32, usize) {
    let at = |offset: usize| data.get(pos + offset).copied().unwrap_or(0) as u32;
    let a = at(0);
    if a & 0x80 == 0 {
        (a, pos + 1)
    } else if a & 0x20 == 0 {
        (((a & 0x1f) << 6) | (at(1) & 0x3f), pos + 2)
    } else if a & 0x10 == 0 {
        (((a & 0x0f) << 12) | ((at(1) & 0x3f) << 6) | (at(2) & 0x3f), pos + 3)
    } else {
        (((a & 0x07) << 18) | ((at(1) & 0x3f) << 12) | ((at(2) & 0x3f) << 6) | (at(3) & 0x3f), pos + 4)
    }
}

fn append_utf8(out: &mut Vec<u8>, code: u32) {
    if code <= 0x7f {
        out.push(code as u8);
    } else if code <= 0x7ff {
        out.push((0xc0 | (code >> 6)) as u8);
        out.push((0x80 | (code & 0x3f)) as u8);
    } else if code <= 0xffff {
        out.push((0xe0 | (code >> 12)) as u8);
        out.push((0x80 | ((code >> 6) & 0x3f)) as u8);
        out.push((0x80 | (code & 0x3f)) as u8);
    } else {
        out.push((0xf0 | (code >> 18)) as u8);
        out.push((0x80 | ((code >> 12) & 0x3f)) as u8);
        out.push((0x80 | ((code >> 6) & 0x3f)) as u8);
        out.push((0x80 | (code & 0x3f)) as u8);
    }
}

fn get_search_words(data: &[u8]) -> Vec<Vec<u8>> {
    let mut words = Vec::new();
    let mut word: Vec<u8> = Vec::new();
    let mut in_word = false;
    let mut pos = 0usize;
    while pos < data.len() {
        let (code, next) = next_utf8(data, pos);
        pos = next;
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
            append_utf8(&mut word, remove_diacritics(code));
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

fn get_words(name: &[u8]) -> Vec<Vec<u8>> {
    fix_words(get_search_words(name))
}

fn en_to_ru_simple(code: u32) -> Option<&'static [u8]> {
    Some(match code {
        0x61 => b"\xd0\xb0",             // a
        0x62 => b"\xd0\xb1",             // b
        0x63 => b"\xd0\xba",             // c
        0x64 => b"\xd0\xb4",             // d
        0x65 => b"\xd0\xb5",             // e
        0x66 => b"\xd1\x84",             // f
        0x67 => b"\xd0\xb3",             // g
        0x68 => b"\xd1\x85",             // h
        0x69 => b"\xd0\xb8",             // i
        0x6a => b"\xd0\xb9",             // j
        0x6b => b"\xd0\xba",             // k
        0x6c => b"\xd0\xbb",             // l
        0x6d => b"\xd0\xbc",             // m
        0x6e => b"\xd0\xbd",             // n
        0x6f => b"\xd0\xbe",             // o
        0x70 => b"\xd0\xbf",             // p
        0x71 => b"\xd0\xba",             // q
        0x72 => b"\xd1\x80",             // r
        0x73 => b"\xd1\x81",             // s
        0x74 => b"\xd1\x82",             // t
        0x75 => b"\xd1\x83",             // u
        0x76 => b"\xd0\xb2",             // v
        0x77 => b"\xd0\xb2",             // w
        0x78 => b"\xd0\xba\xd1\x81",     // x
        0x79 => b"\xd0\xb8",             // y
        0x7a => b"\xd0\xb7",             // z
        _ => return None,
    })
}

const EN_TO_RU_COMPLEX: &[(&[u8], &[u8])] = &[
    (b"ch", b"\xd1\x87"),
    (b"ei", b"\xd0\xb5\xd0\xb9"),
    (b"ey", b"\xd0\xb5\xd0\xb9"),
    (b"ia", b"\xd0\xb8\xd1\x8f"),
    (b"iy", b"\xd0\xb8\xd0\xb9"),
    (b"jo", b"\xd0\xb5"),
    (b"ju", b"\xd1\x8e"),
    (b"ja", b"\xd1\x8f"),
    (b"kh", b"\xd1\x85"),
    (b"shch", b"\xd1\x89"),
    (b"sh", b"\xd1\x88"),
    (b"sch", b"\xd1\x89"),
    (b"ts", b"\xd1\x86"),
    (b"yo", b"\xd0\xb5"),
    (b"yu", b"\xd1\x8e"),
    (b"ya", b"\xd1\x8f"),
    (b"zh", b"\xd0\xb6"),
];

fn ru_to_en_simple(code: u32) -> Option<&'static [u8]> {
    Some(match code {
        0x430 => b"a",
        0x431 => b"b",
        0x432 => b"v",
        0x433 => b"g",
        0x434 => b"d",
        0x435 => b"e",
        0x451 => b"e",
        0x436 => b"zh",
        0x437 => b"z",
        0x438 => b"i",
        0x439 => b"y",
        0x43a => b"k",
        0x43b => b"l",
        0x43c => b"m",
        0x43d => b"n",
        0x43e => b"o",
        0x43f => b"p",
        0x440 => b"r",
        0x441 => b"s",
        0x442 => b"t",
        0x443 => b"u",
        0x444 => b"f",
        0x445 => b"kh",
        0x446 => b"ts",
        0x447 => b"ch",
        0x448 => b"sh",
        0x449 => b"sch",
        0x44a => b"",
        0x44b => b"y",
        0x44c => b"",
        0x44d => b"e",
        0x44e => b"yu",
        0x44f => b"ya",
        _ => return None,
    })
}

const RU_TO_EN_COMPLEX: &[(&[u8], &[u8])] = &[
    (b"\xd0\xb8\xd0\xb9", b"y"),
    (b"\xd0\xb8\xd1\x8f", b"ia"),
    (b"\xd0\xba\xd1\x81", b"x"),
    (b"yo", b"e"),
    (b"jo", b"e"),
];

fn add_word_transliterations(
    result: &mut Vec<Vec<u8>>,
    word: &[u8],
    allow_partial: bool,
    simple_rules: fn(u32) -> Option<&'static [u8]>,
    complex_rules: &[(&[u8], &[u8])],
) {
    let mut s: Vec<u8> = Vec::new();
    let mut pos = 0usize;
    while pos < word.len() {
        let (code, next) = next_utf8(word, pos);
        pos = next;
        match simple_rules(code) {
            Some(replacement) => s.extend_from_slice(replacement),
            None => append_utf8(&mut s, code),
        }
    }
    if !s.is_empty() {
        result.push(std::mem::take(&mut s));
    }

    pos = 0usize;
    while pos < word.len() {
        let suffix = &word[pos..];
        let mut found = false;
        for &(from, to) in complex_rules {
            if suffix.starts_with(from) {
                found = true;
                pos += from.len();
                s.extend_from_slice(to);
                break;
            }
            if allow_partial && from.starts_with(suffix) {
                let mut partial = s.clone();
                partial.extend_from_slice(to);
                result.push(partial);
            }
        }
        if found {
            continue;
        }

        let (code, next) = next_utf8(word, pos);
        pos = next;
        match simple_rules(code) {
            Some(replacement) => s.extend_from_slice(replacement),
            None => append_utf8(&mut s, code),
        }
    }
    if !s.is_empty() {
        result.push(s);
    }
}

fn get_word_transliterations(word: &[u8], allow_partial: bool) -> Vec<Vec<u8>> {
    let mut result = Vec::new();
    add_word_transliterations(&mut result, word, allow_partial, en_to_ru_simple, EN_TO_RU_COMPLEX);
    add_word_transliterations(&mut result, word, allow_partial, ru_to_en_simple, RU_TO_EN_COMPLEX);
    result.sort();
    result.dedup();
    result
}

fn add_word(word_to_keys: &mut BTreeMap<Vec<u8>, Vec<i64>>, word: &[u8], key: i64) {
    let keys = word_to_keys.entry(word.to_vec()).or_default();
    assert!(!keys.contains(&key));
    keys.push(key);
}

fn delete_word(word_to_keys: &mut BTreeMap<Vec<u8>, Vec<i64>>, word: &[u8], key: i64) {
    if let Some(keys) = word_to_keys.get_mut(word) {
        if let Some(position) = keys.iter().position(|&k| k == key) {
            keys.swap_remove(position);
        }
        if keys.is_empty() {
            word_to_keys.remove(word);
        }
    }
}

impl Hints {
    fn new() -> Self {
        Self {
            word_to_keys: BTreeMap::new(),
            translit_word_to_keys: BTreeMap::new(),
            key_to_name: HashMap::new(),
            key_to_rating: HashMap::new(),
        }
    }

    fn size(&self) -> usize {
        self.key_to_name.len()
    }

    fn add(&mut self, key: i64, name: &[u8]) {
        if let Some(old_name) = self.key_to_name.get(&key) {
            if old_name.as_slice() == name {
                return;
            }
            let old_name = old_name.clone();
            let mut old_transliterations = Vec::new();
            for word in get_words(&old_name) {
                delete_word(&mut self.word_to_keys, &word, key);
                for transliteration in get_word_transliterations(&word, false) {
                    if transliteration != word {
                        old_transliterations.push(transliteration);
                    }
                }
            }
            for word in fix_words(old_transliterations) {
                delete_word(&mut self.translit_word_to_keys, &word, key);
            }
        }
        if name.is_empty() {
            self.key_to_name.remove(&key);
            self.key_to_rating.remove(&key);
            return;
        }
        let mut transliterations = Vec::new();
        for word in get_words(name) {
            add_word(&mut self.word_to_keys, &word, key);
            for transliteration in get_word_transliterations(&word, false) {
                if transliteration != word {
                    transliterations.push(transliteration);
                }
            }
        }
        for word in fix_words(transliterations) {
            add_word(&mut self.translit_word_to_keys, &word, key);
        }
        self.key_to_name.insert(key, name.to_vec());
    }

    fn remove(&mut self, key: i64) {
        self.add(key, &[]);
    }

    fn set_rating(&mut self, key: i64, rating: i64) {
        self.key_to_rating.insert(key, rating);
    }

    fn search_word(&self, word: &[u8]) -> Vec<i64> {
        let mut results = Vec::new();
        add_search_results(&mut results, word, &self.translit_word_to_keys);
        for transliteration in get_word_transliterations(word, true) {
            add_search_results(&mut results, &transliteration, &self.word_to_keys);
        }
        results.sort_unstable();
        results.dedup();
        results
    }

    fn rating(&self, key: i64) -> i64 {
        *self.key_to_rating.get(&key).unwrap_or(&0)
    }

    fn search(&self, query: &[u8], limit: i32, return_all_for_empty_query: bool) -> (usize, Vec<i64>) {
        let mut results: Vec<i64> = Vec::new();
        if limit < 0 {
            return (self.size(), results);
        }
        let words = get_words(query);
        if return_all_for_empty_query && words.is_empty() {
            results.extend(self.key_to_name.keys().copied());
        }
        for (index, word) in words.iter().enumerate() {
            let keys = self.search_word(word);
            if index == 0 {
                results = keys;
                continue;
            }
            let mut merged = Vec::with_capacity(results.len().min(keys.len()));
            let (mut left, mut right) = (0, 0);
            while left < results.len() && right < keys.len() {
                if results[left] < keys[right] {
                    left += 1;
                } else if results[left] > keys[right] {
                    right += 1;
                } else {
                    merged.push(results[left]);
                    left += 1;
                    right += 1;
                }
            }
            results = merged;
        }
        let total = results.len();
        results.sort_by(|&left, &right| {
            let left_rating = self.rating(left);
            let right_rating = self.rating(right);
            (left_rating, left).cmp(&(right_rating, right))
        });
        if total >= limit as usize {
            results.truncate(limit as usize);
        }
        (total, results)
    }

    fn has_key(&self, key: i64) -> bool {
        self.key_to_name.contains_key(&key)
    }

    fn key_to_string(&self, key: i64) -> Vec<u8> {
        self.key_to_name.get(&key).cloned().unwrap_or_default()
    }
}

fn add_search_results(results: &mut Vec<i64>, word: &[u8], word_to_keys: &BTreeMap<Vec<u8>, Vec<i64>>) {
    for (stored, keys) in word_to_keys.range::<[u8], _>((Bound::Included(word), Bound::Unbounded)) {
        if !stored.starts_with(word) {
            break;
        }
        results.extend_from_slice(keys);
    }
}

fn unhex(text: &str) -> Vec<u8> {
    if text == "-" {
        return Vec::new();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let digits = std::str::from_utf8(pair).unwrap();
        out.push(u8::from_str_radix(digits, 16).unwrap());
    }
    out
}

fn hex_or_dash(data: &[u8]) -> String {
    if data.is_empty() {
        return "-".to_string();
    }
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn keys_to_string(keys: &[i64]) -> String {
    if keys.is_empty() {
        return "-".to_string();
    }
    keys.iter().map(|key| key.to_string()).collect::<Vec<_>>().join(",")
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: hints_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    let mut hints = Hints::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let op = parts[0];
        match op {
            "add" => {
                let key: i64 = parts[1].parse().unwrap();
                let name = unhex(parts.get(2).copied().unwrap_or("-"));
                hints.add(key, &name);
                println!("add key={} size={}", key, hints.size());
            }
            "remove" => {
                let key: i64 = parts[1].parse().unwrap();
                hints.remove(key);
                println!("remove key={} size={}", key, hints.size());
            }
            "rate" => {
                let key: i64 = parts[1].parse().unwrap();
                let rating: i64 = parts[2].parse().unwrap();
                hints.set_rating(key, rating);
                println!("rate key={} rating={}", key, rating);
            }
            "search" | "sempty" => {
                let (query, limit) = if op == "search" {
                    (unhex(parts.get(1).copied().unwrap_or("-")), parts.get(2).map_or(0, |t| t.parse().unwrap()))
                } else {
                    (Vec::new(), parts.get(1).map_or(0, |t| t.parse().unwrap()))
                };
                let all = op == "sempty" || parts.get(3).copied().unwrap_or("") == "all";
                let (total, keys) = hints.search(&query, limit, all);
                println!(
                    "{} q={} limit={} total={} keys={}",
                    op,
                    hex_or_dash(&query),
                    limit,
                    total,
                    keys_to_string(&keys)
                );
            }
            "has" => {
                let key: i64 = parts[1].parse().unwrap();
                println!("has key={} r={}", key, hints.has_key(key) as i32);
            }
            "name" => {
                let key: i64 = parts[1].parse().unwrap();
                println!("name key={} name={}", key, hex_or_dash(&hints.key_to_string(key)));
            }
            "size" => println!("size n={}", hints.size()),
            "words" => {
                let text = unhex(parts.get(1).copied().unwrap_or("-"));
                let words = get_search_words(&text);
                let out = words.iter().map(|word| hex_or_dash(word)).collect::<Vec<_>>().join(",");
                println!("words in={} out={}", hex_or_dash(&text), if out.is_empty() { "-" } else { out.as_str() });
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}