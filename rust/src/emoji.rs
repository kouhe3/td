//! `tdutils/td/utils/emoji.{h,cpp}` — the emoji table predicates.
//!
//! The table itself is generated (`rust/src/emoji_table.rs`, 2370 entries, no
//! zero-width joiners). Everything here is the rule layer on top of it:
//!   * `is_emoji_element` accepts an exact table entry, or the same entry with a
//!     single trailing `U+FE0F` stripped (two stacked selectors are rejected);
//!   * `is_emoji` additionally splits on `U+200D` and requires **every** segment
//!     to be an element, which is how ZWJ sequences are accepted even though the
//!     table has none;
//!   * `find_emojis` takes the longest table entry at a position, then extends
//!     over `ZWJ` + another entry while that works (a selector after each part is
//!     skipped);
//!   * the modifier helpers work on the raw bytes.
//!
//! `is_emoji` only looks for the first `0xE2` inside the first
//! `MAX_EMOJI_LENGTH + 4` bytes; a ZWJ later than that is treated as a plain
//! byte, exactly like the C++ loop.

use crate::emoji_table::EMOJIS;

pub const MAX_EMOJI_LENGTH: usize = 28;

const VARIATION_SELECTOR: &[u8] = b"\xEF\xB8\x8F";
const ZERO_WIDTH_JOINER: &[u8] = b"\xE2\x80\x8D";

fn contains(bytes: &[u8]) -> bool {
    match std::str::from_utf8(bytes) {
        Ok(text) => EMOJIS.binary_search(&text).is_ok(),
        Err(_) => false,
    }
}

/// `is_emoji_element`: an exact table entry, or one with a single trailing
/// variation selector removed.
fn is_emoji_element(value: &[u8]) -> bool {
    let len = value.len();
    if len > MAX_EMOJI_LENGTH + 3 {
        return false;
    }
    if contains(value) {
        return true;
    }
    if len <= 3 || &value[len - 3..] != VARIATION_SELECTOR {
        return false;
    }
    if len >= 6 && &value[len - 6..len - 3] == VARIATION_SELECTOR {
        return false;
    }
    contains(&value[..len - 3])
}

pub fn is_emoji(value: &[u8]) -> bool {
    let window = &value[..value.len().min(MAX_EMOJI_LENGTH + 4)];
    let Some(mut i) = window.iter().position(|&byte| byte == 0xE2) else {
        return is_emoji_element(value);
    };

    let mut start_pos = 0;
    while i + 3 < value.len() {
        if &value[i..i + 3] == ZERO_WIDTH_JOINER {
            if !is_emoji_element(&value[start_pos..i]) {
                return false;
            }
            start_pos = i + 3;
            i += 2;
        }
        i += 1;
    }
    is_emoji_element(&value[start_pos..])
}

pub fn is_emoji_str(value: &str) -> bool {
    is_emoji(value.as_bytes())
}

/// `EmojiAutomaton::process`'s answer for one position: the length of the
/// longest table entry starting there.
fn emoji_prefix_length(bytes: &[u8], position: usize) -> usize {
    let max_length = (bytes.len() - position).min(MAX_EMOJI_LENGTH);
    for length in (1..=max_length).rev() {
        if contains(&bytes[position..position + length]) {
            return length;
        }
    }
    0
}

fn skip_emoji_selector(bytes: &[u8], position: usize) -> usize {
    if position + 3 <= bytes.len() && &bytes[position..position + 3] == VARIATION_SELECTOR {
        position + 3
    } else {
        position
    }
}

/// `find_emojis`: the non-overlapping emoji in order, longest first at each
/// position. Returned as byte ranges so the caller decides how to represent them.
pub fn find_emojis(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let length = emoji_prefix_length(bytes, i);
        if length == 0 {
            i += 1;
            continue;
        }
        let mut end = skip_emoji_selector(bytes, i + length);
        while end + 3 < bytes.len() && &bytes[end..end + 3] == ZERO_WIDTH_JOINER {
            let next_length = emoji_prefix_length(bytes, end + 3);
            if next_length == 0 {
                break;
            }
            end = skip_emoji_selector(bytes, end + 3 + next_length);
        }
        result.push((i, end));
        i = end;
    }
    result
}

/// `get_fitzpatrick_modifier`: 2..6, or 0 when the last four bytes are not a
/// skin-tone modifier.
pub fn get_fitzpatrick_modifier(emoji: &[u8]) -> i32 {
    let len = emoji.len();
    if len < 4 || emoji[len - 4] != 0xF0 || emoji[len - 3] != 0x9F || emoji[len - 2] != 0x8F {
        return 0;
    }
    let c = emoji[len - 1];
    if !(0xBB..=0xBF).contains(&c) {
        return 0;
    }
    i32::from(c - 0xBB) + 2
}

pub fn remove_fitzpatrick_modifier(emoji: &[u8]) -> &[u8] {
    let mut value = emoji;
    while get_fitzpatrick_modifier(value) != 0 {
        value = &value[..value.len() - 4];
    }
    value
}

const MODIFIERS: [&[u8]; 8] = [
    b"\xEF\xB8\x8F",                 // variation selector-16
    b"\xE2\x80\x8D\xE2\x99\x80",     // ZWJ + female sign
    b"\xE2\x80\x8D\xE2\x99\x82",     // ZWJ + male sign
    b"\xF0\x9F\x8F\xBB",             // fitzpatrick type-1-2
    b"\xF0\x9F\x8F\xBC",             // fitzpatrick type-3
    b"\xF0\x9F\x8F\xBD",             // fitzpatrick type-4
    b"\xF0\x9F\x8F\xBE",             // fitzpatrick type-5
    b"\xF0\x9F\x8F\xBF",             // fitzpatrick type-6
];

/// `remove_emoji_modifiers`: strips the eight known modifiers; with
/// `remove_selectors == false` the variation selector is kept.
pub fn remove_emoji_modifiers(emoji: &[u8], remove_selectors: bool) -> Vec<u8> {
    let start_index = usize::from(!remove_selectors);
    let mut out = Vec::with_capacity(emoji.len());
    let mut i = 0;
    while i < emoji.len() {
        let mut found = false;
        for modifier in &MODIFIERS[start_index..] {
            if i + modifier.len() <= emoji.len() && &emoji[i..i + modifier.len()] == *modifier {
                i += modifier.len();
                found = true;
                break;
            }
        }
        if !found {
            out.push(emoji[i]);
            i += 1;
        }
    }
    out
}

/// `remove_emoji_selectors`: for an emoji, every `U+FE0F`; otherwise the string
/// unchanged. The C++ version ends with `CHECK(is_emoji(str))`, mirrored here by
/// an assertion.
pub fn remove_emoji_selectors(emoji: &[u8]) -> Vec<u8> {
    if !is_emoji(emoji) {
        return emoji.to_vec();
    }
    let mut out = Vec::with_capacity(emoji.len());
    let mut i = 0;
    while i < emoji.len() {
        if i + 3 <= emoji.len() && &emoji[i..i + 3] == VARIATION_SELECTOR {
            i += 3;
        } else {
            out.push(emoji[i]);
            i += 1;
        }
    }
    assert!(is_emoji(&out), "remove_emoji_selectors must stay an emoji");
    out
}