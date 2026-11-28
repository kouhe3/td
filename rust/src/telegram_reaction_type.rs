//! `td::ReactionType` — `td/telegram/ReactionType.{h,cpp}`.
//!
//! One string encodes four states:
//!   * `""`                       empty
//!   * `"👍"`                      a plain emoji
//!   * `"#" + base64(8 B LE i64)` a custom emoji (the document id)
//!   * `"$"`                      the paid reaction
//!
//! Two decoders (telegram_api / td_api) and three encoders (input / td_api
//! object / `updateDefaultReactionType`) sit around it. The asymmetries the C++
//! code really has (docs/facts.md F287):
//!   * `operator<` special-cases paid to the front, so the order is
//!     `"$" < "" < "#…" < emoji` -- not the byte order of the strings;
//!   * an empty reaction becomes `reactionEmpty` on the wire but `nullptr` on
//!     both td_api exits;
//!   * `reactionEmoji` whose emoticon starts with `#` or equals `$` is cleared,
//!     and the td_api decoder additionally requires valid UTF-8;
//!   * `get_hash` is the raw document id for a custom emoji and
//!     `md5(remove_emoji_selectors(s))` (first 8 bytes, big-endian) otherwise.

use crate::md5::md5;

pub const PAID: &str = "$";

/// The `td::td_api` / `td::telegram_api` constructor ids the narration prints.
pub mod api_ids {
    pub const REACTION_EMPTY: i32 = 2046153753;
    pub const REACTION_EMOJI: i32 = 455247544;
    pub const REACTION_CUSTOM_EMOJI: i32 = -1992950669;
    pub const REACTION_PAID: i32 = 1379771627;
    pub const REACTION_TYPE_EMOJI: i32 = -1942084920;
    pub const REACTION_TYPE_CUSTOM_EMOJI: i32 = -989117709;
    pub const REACTION_TYPE_PAID: i32 = 436294381;
}

/// What `get_input_reaction()` returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputReaction {
    Empty,
    Emoji(String),
    Custom(i64),
    Paid,
}

/// What `get_reaction_type_object()` / `get_update_default_reaction_type()` return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TdReaction {
    Emoji(String),
    Custom(i64),
    Paid,
}

/// Input of the `telegram_api::Reaction` constructor.
pub enum TelegramReaction {
    Empty,
    Emoji(Vec<u8>),
    Custom(i64),
    Paid,
}

/// Input of the `td_api::ReactionType` constructor.
pub enum TdReactionInput {
    Null,
    /// Raw bytes: the C++ side needs `check_utf8` to accept them.
    Emoji(Vec<u8>),
    Custom(i64),
    Paid,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReactionType {
    reaction: String,
}

impl ReactionType {
    pub fn from_string(value: String) -> ReactionType {
        ReactionType { reaction: value }
    }

    pub fn paid() -> ReactionType {
        ReactionType {
            reaction: PAID.to_string(),
        }
    }

    pub fn from_telegram(kind: TelegramReaction) -> ReactionType {
        match kind {
            TelegramReaction::Empty => ReactionType::default(),
            TelegramReaction::Emoji(emoticon) => {
                let mut value = ReactionType {
                    reaction: String::from_utf8_lossy(&emoticon).into_owned(),
                };
                if value.is_custom_reaction() || value.is_paid_reaction() {
                    value.reaction.clear();
                }
                value
            }
            TelegramReaction::Custom(id) => ReactionType {
                reaction: custom_string(id),
            },
            TelegramReaction::Paid => ReactionType::paid(),
        }
    }

    pub fn from_td(kind: TdReactionInput) -> ReactionType {
        match kind {
            TdReactionInput::Null => ReactionType::default(),
            TdReactionInput::Emoji(bytes) => {
                // `check_utf8` on the C++ side; a failure leaves the reaction empty.
                let Ok(emoji) = String::from_utf8(bytes) else {
                    return ReactionType::default();
                };
                let mut value = ReactionType { reaction: emoji };
                if value.is_custom_reaction() || value.is_paid_reaction() {
                    value.reaction.clear();
                }
                value
            }
            TdReactionInput::Custom(id) => ReactionType {
                reaction: custom_string(id),
            },
            TdReactionInput::Paid => ReactionType::paid(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.reaction.is_empty()
    }

    /// `reaction_[0] == '#'`; on an empty string the C++ `operator[]` yields
    /// `'\0'`, so `None` here is the same answer.
    pub fn is_custom_reaction(&self) -> bool {
        self.reaction.as_bytes().first() == Some(&b'#')
    }

    pub fn is_paid_reaction(&self) -> bool {
        self.reaction == PAID
    }

    pub fn get_string(&self) -> &str {
        &self.reaction
    }

    pub fn input_reaction(&self) -> InputReaction {
        if self.is_empty() {
            return InputReaction::Empty;
        }
        if self.is_custom_reaction() {
            return InputReaction::Custom(self.custom_emoji_id());
        }
        if self.is_paid_reaction() {
            return InputReaction::Paid;
        }
        InputReaction::Emoji(self.reaction.clone())
    }

    pub fn reaction_type_object(&self) -> Option<TdReaction> {
        if self.is_empty() {
            return None;
        }
        if self.is_custom_reaction() {
            return Some(TdReaction::Custom(self.custom_emoji_id()));
        }
        if self.is_paid_reaction() {
            return Some(TdReaction::Paid);
        }
        Some(TdReaction::Emoji(self.reaction.clone()))
    }

    pub fn update_default_reaction_type(&self) -> Option<TdReaction> {
        if self.is_empty() {
            return None;
        }
        self.reaction_type_object()
    }

    /// `get_hash()`: the document id for a custom emoji, otherwise
    /// `get_md5_string_hash(remove_emoji_selectors(reaction))`.
    pub fn hash(&self) -> u64 {
        if self.is_custom_reaction() {
            self.custom_emoji_id() as u64
        } else {
            get_md5_string_hash(&remove_emoji_selectors(&self.reaction))
        }
    }

    pub fn less(&self, other: &ReactionType) -> bool {
        if self.is_paid_reaction() {
            return !other.is_paid_reaction();
        }
        if other.is_paid_reaction() {
            return false;
        }
        self.reaction < other.reaction
    }

    /// `get_custom_emoji_id`: CHECKs both the base64 decode and the 8-byte length,
    /// so a malformed `#…` panics here exactly like the C++ CHECK aborts.
    fn custom_emoji_id(&self) -> i64 {
        let decoded = base64_decode(&self.reaction[1..]).expect("custom emoji base64");
        assert_eq!(decoded.len(), 8, "custom emoji length");
        i64::from_le_bytes(decoded[..8].try_into().unwrap())
    }
}

impl std::fmt::Display for ReactionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            return write!(f, "empty reaction");
        }
        if self.is_custom_reaction() {
            return write!(f, "custom reaction {}", self.custom_emoji_id());
        }
        if self.is_paid_reaction() {
            return write!(f, "paid reaction");
        }
        write!(f, "reaction {}", self.reaction)
    }
}

/// `get_reaction_types_object(vec, paid_reactions_available)`: a paid reaction is
/// prepended when available.
pub fn reaction_types_object(types: &[ReactionType], paid_available: bool) -> Vec<Option<TdReaction>> {
    let mut result = Vec::with_capacity(types.len() + usize::from(paid_available));
    if paid_available {
        result.push(ReactionType::paid().reaction_type_object());
    }
    for reaction_type in types {
        result.push(reaction_type.reaction_type_object());
    }
    result
}

/// `get_reaction_types_hash(vec)`.
pub fn reaction_types_hash(types: &[ReactionType]) -> i64 {
    let mut numbers: Vec<u64> = Vec::with_capacity(types.len() * 2);
    for reaction_type in types {
        if reaction_type.is_custom_reaction() {
            let id = reaction_type.custom_emoji_id() as u64;
            numbers.push(id >> 32);
            numbers.push(id & 0xFFFF_FFFF);
        } else {
            let emoji = remove_emoji_selectors(&reaction_type.reaction);
            let digest = md5(emoji.as_bytes());
            let get = |index: usize| u32::from(digest[index]);
            let value = (get(0) << 24)
                .wrapping_add(get(1) << 16)
                .wrapping_add(get(2) << 8)
                .wrapping_add(get(3));
            numbers.push(0);
            // the C++ side stores the int32 through `push_back(uint64)`, so a
            // negative value sign-extends
            numbers.push((value as i32) as u64);
        }
    }
    get_vector_hash(&numbers)
}

/// `get_md5_string_hash`: the first 8 digest bytes, big-endian.
fn get_md5_string_hash(value: &str) -> u64 {
    let digest = md5(value.as_bytes());
    let mut result = 0u64;
    for (index, byte) in digest.iter().take(8).enumerate() {
        result += u64::from(*byte) << (56 - 8 * index);
    }
    result
}

/// `get_vector_hash`.
fn get_vector_hash(numbers: &[u64]) -> i64 {
    let mut acc = 0u64;
    for number in numbers {
        acc ^= acc >> 21;
        acc ^= acc << 35;
        acc ^= acc >> 4;
        acc = acc.wrapping_add(*number);
    }
    acc as i64
}

/// `remove_emoji_selectors`: for an emoji, drop every `U+FE0F`; otherwise the
/// string unchanged. The rules (table, ZWJ split, the trailing `CHECK`) live in
/// `rust/src/emoji.rs`, shared with `text_search` and the emoji lane.
fn remove_emoji_selectors(value: &str) -> String {
    String::from_utf8_lossy(&crate::emoji::remove_emoji_selectors(value.as_bytes())).into_owned()
}

fn custom_string(custom_emoji_id: i64) -> String {
    format!("#{}", base64_encode(&custom_emoji_id.to_le_bytes()))
}

const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `td::base64_encode`: the standard alphabet, with `=` padding.
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map(|value| u32::from(*value)).unwrap_or(0);
        let b2 = chunk.get(2).map(|value| u32::from(*value)).unwrap_or(0);
        let value = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[((value >> 18) & 63) as usize] as char);
        out.push(BASE64_ALPHABET[((value >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(BASE64_ALPHABET[((value >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(BASE64_ALPHABET[(value & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// `td::base64_decode`: rejects unknown characters and non-zero leftover bits;
/// a trailing group without padding is accepted.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    let mut padding = 0usize;
    for &byte in bytes {
        if byte == b'=' {
            padding += 1;
            continue;
        }
        if padding > 0 {
            return None;
        }
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1u32 << bits) - 1;
        }
    }
    if padding > 2 || (bits > 0 && buffer != 0) {
        return None;
    }
    Some(out)
}