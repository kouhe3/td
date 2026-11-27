//! tdutils Hints on the Rust side, ASCII-only word surface.
//!
//! ```text
//! hints_probe <script.txt>
//! ```
//!
//! Script lines: `add`, `remove`, `rate`, `search`, `sempty`, `has`, `name`,
//! `size`.
//!
//! Words are the ASCII words of the name (lowercase letters and digits), then
//! `fix_words` sorts them and drops every word that is a prefix of the next one.
//! `search` intersects the per-word key lists, reports the count before the
//! limit, and sorts by (rating ascending, key ascending). The unicode search
//! tables and the transliteration rules of td are not replicated; the corpus
//! stays within [a-z0-9] and single spaces.

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::process::ExitCode;

struct Hints {
    word_to_keys: BTreeMap<Vec<u8>, Vec<i64>>,
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

fn get_words(name: &[u8]) -> Vec<Vec<u8>> {
    let mut words = Vec::new();
    let mut current: Vec<u8> = Vec::new();
    for &c in name {
        let mapped = if c.is_ascii_uppercase() {
            c.to_ascii_lowercase()
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            c
        } else {
            b' '
        };
        if mapped == b' ' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(mapped);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    fix_words(words)
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
        Self { word_to_keys: BTreeMap::new(), key_to_name: HashMap::new(), key_to_rating: HashMap::new() }
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
            for word in get_words(&old_name) {
                delete_word(&mut self.word_to_keys, &word, key);
            }
        }
        if name.is_empty() {
            self.key_to_name.remove(&key);
            self.key_to_rating.remove(&key);
            return;
        }
        for word in get_words(name) {
            add_word(&mut self.word_to_keys, &word, key);
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
        for (stored, keys) in self.word_to_keys.range(word.to_vec()..) {
            if !stored.starts_with(word) {
                break;
            }
            results.extend_from_slice(keys);
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
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}