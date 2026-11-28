//! td::get_word_transliterations on the Rust side: the four rule tables
//! transcribed verbatim from tdutils/td/utils/translit.cpp plus the two-pass scan.
//!
//! ```text
//! translit_probe <script.txt>
//! ```
//!
//! Script lines: `tr <hex> <0|1>` and `once <hex> <0|1>`, exactly as the C++
//! oracle takes them. `tr` prints `tr in=… partial=… n=<k>` and then one line
//! ` out<i>=<hex>` per returned string in returned order; `once` runs the same
//! word `ONCE_REPEATS` times in this process and prints whether every repetition
//! produced the same bytes.
//!
//! The first pass transliterates character by character with the simple table;
//! the second restarts at the beginning, takes the first complex rule in table
//! order whose key the suffix begins with (one element per step, `break`), and
//! while `allow_partial` is set appends `s + rule.second` for every rule whose
//! key begins with the whole remaining suffix. `unique` sorts by bytes and drops
//! adjacent duplicates. `next_utf8_unsafe` derives the sequence length from the
//! lead byte without validating it; the driver only feeds payloads whose last
//! sequence fits inside the word, so no read past the end is modelled here.

use std::env;
use std::fs;
use std::process::ExitCode;

const ONCE_REPEATS: usize = 5;

/// `get_en_to_ru_simple_rules`
const EN_TO_RU_SIMPLE_RULES: [(u32, &str); 26] = [
    ('a' as u32, "а"),
    ('b' as u32, "б"),
    ('c' as u32, "к"),
    ('d' as u32, "д"),
    ('e' as u32, "е"),
    ('f' as u32, "ф"),
    ('g' as u32, "г"),
    ('h' as u32, "х"),
    ('i' as u32, "и"),
    ('j' as u32, "й"),
    ('k' as u32, "к"),
    ('l' as u32, "л"),
    ('m' as u32, "м"),
    ('n' as u32, "н"),
    ('o' as u32, "о"),
    ('p' as u32, "п"),
    ('q' as u32, "к"),
    ('r' as u32, "р"),
    ('s' as u32, "с"),
    ('t' as u32, "т"),
    ('u' as u32, "у"),
    ('v' as u32, "в"),
    ('w' as u32, "в"),
    ('x' as u32, "кс"),
    ('y' as u32, "и"),
    ('z' as u32, "з"),
];

/// `get_en_to_ru_complex_rules`, in table order: `shch` is tried before `sh`,
/// and `sh` before `sch`, so `sch` can never win a full match.
const EN_TO_RU_COMPLEX_RULES: [(&str, &str); 17] = [
    ("ch", "ч"),
    ("ei", "ей"),
    ("ey", "ей"),
    ("ia", "ия"),
    ("iy", "ий"),
    ("jo", "е"),
    ("ju", "ю"),
    ("ja", "я"),
    ("kh", "х"),
    ("shch", "щ"),
    ("sh", "ш"),
    ("sch", "щ"),
    ("ts", "ц"),
    ("yo", "е"),
    ("yu", "ю"),
    ("ya", "я"),
    ("zh", "ж"),
];

/// `get_ru_to_en_simple_rules`
const RU_TO_EN_SIMPLE_RULES: [(u32, &str); 33] = [
    (0x430, "a"),
    (0x431, "b"),
    (0x432, "v"),
    (0x433, "g"),
    (0x434, "d"),
    (0x435, "e"),
    (0x451, "e"),
    (0x436, "zh"),
    (0x437, "z"),
    (0x438, "i"),
    (0x439, "y"),
    (0x43a, "k"),
    (0x43b, "l"),
    (0x43c, "m"),
    (0x43d, "n"),
    (0x43e, "o"),
    (0x43f, "p"),
    (0x440, "r"),
    (0x441, "s"),
    (0x442, "t"),
    (0x443, "u"),
    (0x444, "f"),
    (0x445, "kh"),
    (0x446, "ts"),
    (0x447, "ch"),
    (0x448, "sh"),
    (0x449, "sch"),
    (0x44a, ""),
    (0x44b, "y"),
    (0x44c, ""),
    (0x44d, "e"),
    (0x44e, "yu"),
    (0x44f, "ya"),
];

/// `get_ru_to_en_complex_rules`
const RU_TO_EN_COMPLEX_RULES: [(&str, &str); 5] =
    [("ий", "y"), ("ия", "ia"), ("кс", "x"), ("yo", "e"), ("jo", "e")];

/// `next_utf8_unsafe`: the length comes from the lead byte alone.
fn next_utf8_unsafe(data: &[u8], pos: usize) -> (u32, usize) {
    let lead = u32::from(data[pos]);
    if lead & 0x80 == 0 {
        (lead, pos + 1)
    } else if lead & 0x20 == 0 {
        let code = ((lead & 0x1f) << 6) | (u32::from(data[pos + 1]) & 0x3f);
        (code, pos + 2)
    } else if lead & 0x10 == 0 {
        let code = ((lead & 0x0f) << 12)
            | ((u32::from(data[pos + 1]) & 0x3f) << 6)
            | (u32::from(data[pos + 2]) & 0x3f);
        (code, pos + 3)
    } else {
        assert!(lead & 0x08 == 0, "next_utf8_unsafe: UNREACHABLE lead byte {lead:#04x}");
        let code = ((lead & 0x07) << 18)
            | ((u32::from(data[pos + 1]) & 0x3f) << 12)
            | ((u32::from(data[pos + 2]) & 0x3f) << 6)
            | (u32::from(data[pos + 3]) & 0x3f);
        (code, pos + 4)
    }
}

/// `append_utf8_character`
fn append_utf8_character(out: &mut Vec<u8>, code: u32) {
    if code <= 0x7f {
        out.push(code as u8);
    } else if code <= 0x7ff {
        out.push(0xc0 | (code >> 6) as u8);
        out.push(0x80 | (code & 0x3f) as u8);
    } else if code <= 0xffff {
        out.push(0xe0 | (code >> 12) as u8);
        out.push(0x80 | ((code >> 6) & 0x3f) as u8);
        out.push(0x80 | (code & 0x3f) as u8);
    } else {
        out.push(0xf0 | (code >> 18) as u8);
        out.push(0x80 | ((code >> 12) & 0x3f) as u8);
        out.push(0x80 | ((code >> 6) & 0x3f) as u8);
        out.push(0x80 | (code & 0x3f) as u8);
    }
}

fn append_transliteration(out: &mut Vec<u8>, code: u32, simple_rules: &[(u32, &str)]) {
    match simple_rules.iter().find(|(rule_code, _)| *rule_code == code) {
        Some((_, replacement)) => out.extend_from_slice(replacement.as_bytes()),
        None => append_utf8_character(out, code),
    }
}

/// `begins_with(Slice, Slice)`
fn begins_with(text: &[u8], prefix: &[u8]) -> bool {
    text.len() >= prefix.len() && &text[..prefix.len()] == prefix
}

/// `add_word_transliterations`
fn add_word_transliterations(
    result: &mut Vec<Vec<u8>>,
    word: &[u8],
    allow_partial: bool,
    simple_rules: &[(u32, &str)],
    complex_rules: &[(&str, &str)],
) {
    let mut transliteration = Vec::new();
    let mut pos = 0usize;
    while pos < word.len() {
        let (code, next_pos) = next_utf8_unsafe(word, pos);
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
            if begins_with(suffix, rule_begin.as_bytes()) {
                matched = true;
                pos += rule_begin.len();
                transliteration.extend_from_slice(rule_end.as_bytes());
                break;
            }
            if allow_partial && begins_with(rule_begin.as_bytes(), suffix) {
                let mut partial = transliteration.clone();
                partial.extend_from_slice(rule_end.as_bytes());
                result.push(partial);
            }
        }
        if matched {
            continue;
        }
        let (code, next_pos) = next_utf8_unsafe(word, pos);
        pos = next_pos;
        append_transliteration(&mut transliteration, code, simple_rules);
    }
    if !transliteration.is_empty() {
        result.push(transliteration);
    }
}

/// `get_word_transliterations`
fn get_word_transliterations(word: &[u8], allow_partial: bool) -> Vec<Vec<u8>> {
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
    // td::unique: sort by bytes with operator<, then drop adjacent duplicates
    result.sort_unstable();
    result.dedup();
    result
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

fn join_hex(values: &[Vec<u8>]) -> String {
    if values.is_empty() {
        return "-".to_string();
    }
    values.iter().map(|value| hex_or_dash(value)).collect::<Vec<_>>().join(",")
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: translit_probe <script.txt>");
        return ExitCode::from(2);
    }
    let text = match fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read {}: {error}", args[1]);
            return ExitCode::from(2);
        }
    };
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() || parts[0].starts_with('#') {
            continue;
        }
        let op = parts[0];
        if op != "tr" && op != "once" {
            println!("unknown {op}");
            continue;
        }
        let word = unhex(parts.get(1).copied().unwrap_or("-"));
        let allow_partial = parts.get(2).map_or(0, |text| text.parse::<i32>().unwrap_or(0)) != 0;
        let in_hex = hex_or_dash(&word);
        let partial = i32::from(allow_partial);
        if op == "tr" {
            let result = get_word_transliterations(&word, allow_partial);
            println!("tr in={in_hex} partial={partial} n={}", result.len());
            for (index, value) in result.iter().enumerate() {
                println!(" out{index}={}", hex_or_dash(value));
            }
        } else {
            let first_result = get_word_transliterations(&word, allow_partial);
            let first = join_hex(&first_result);
            let mut count = first_result.len();
            let mut identical = true;
            for _ in 1..ONCE_REPEATS {
                let again = get_word_transliterations(&word, allow_partial);
                count = again.len();
                if join_hex(&again) != first {
                    identical = false;
                }
            }
            println!(
                "once in={in_hex} partial={partial} repeats={ONCE_REPEATS} identical={} n={count}",
                i32::from(identical)
            );
            println!(" out={first}");
        }
    }
    ExitCode::SUCCESS
}