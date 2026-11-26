use std::cell::RefCell;
use std::collections::HashSet;
mod emoji_table;
mod mime_tables;

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
    TonAddress,
    Url,
    EmailAddress,
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
            Kind::TonAddress => "textEntityTypeTonAddress",
            Kind::Url => "textEntityTypeUrl",
            Kind::EmailAddress => "textEntityTypeEmailAddress",
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
        let mut size = 0usize;
        let mut has_letter = false;
        let mut capped_end: Option<usize> = None;
        while j < chars.len() && is_hashtag_letter(chars[j]) {
            j += 1;
            if size == 255 {
                capped_end = Some(j);
            }
            if size != 256 {
                has_letter |= chars[j - 1].is_alphabetic();
                size += 1;
            }
        }
        let mut end = capped_end.unwrap_or(j);
        if size < 1 {
            i = j.max(i + 1);
            continue;
        }
        if capped_end.is_none() && j < chars.len() && chars[j] == '@' {
            let mut k = j + 1;
            while k < chars.len() && k - j < 33 && is_ascii_ident(chars[k]) {
                k += 1;
            }
            if k - j - 1 >= 3 {
                j = k;
                end = k;
            }
        }
        if j < chars.len() && chars[j] == '#' {
            i = j;
            continue;
        }
        if !has_letter {
            i = j;
            continue;
        }
        out.push((i, end - i, Kind::Hashtag));
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

const BAD_PATH_END_CHARS: [char; 9] = ['.', ':', ';', ',', '(', '\'', '?', '!', '`'];

fn is_bad_path_end_char(c: char) -> bool {
    BAD_PATH_END_CHARS.contains(&c)
}

fn is_alpha_digit_or_underscore_or_minus(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn ascii_prefix_ci(chars: &[char], prefix: &str) -> bool {
    chars.len() >= prefix.len() && chars.iter().zip(prefix.chars()).all(|(&a, b)| a.eq_ignore_ascii_case(&b))
}

fn ascii_suffix_ci(chars: &[char], suffix: &str) -> bool {
    chars.len() >= suffix.len()
        && chars[chars.len() - suffix.len()..]
            .iter()
            .zip(suffix.chars())
            .all(|(&a, b)| a.eq_ignore_ascii_case(&b))
}

fn ascii_eq_ci(chars: &[char], other: &str) -> bool {
    chars.len() == other.len() && ascii_prefix_ci(chars, other)
}

fn is_unicode_separator(c: char) -> bool {
    matches!(
        c,
        ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

fn is_url_unicode_symbol(c: char) -> bool {
    if ('\u{2000}'..='\u{206f}').contains(&c) {
        return c == '\u{200c}' || c == '\u{200d}' || ('\u{2010}'..='\u{2015}').contains(&c);
    }
    !is_unicode_separator(c)
}

fn is_url_path_symbol(c: char) -> bool {
    !matches!(c, '\n' | '<' | '>' | '"' | '\u{ab}' | '\u{bb}') && is_url_unicode_symbol(c)
}

fn is_protocol_symbol(c: char) -> bool {
    if (c as u32) < 0x80 {
        c.is_ascii_alphanumeric() || c == '+' || c == '-'
    } else {
        !is_unicode_separator(c)
    }
}

fn is_user_data_symbol(c: char) -> bool {
    !matches!(
        c,
        '\n' | '/' | '[' | ']' | '{' | '}' | '(' | ')' | '\'' | '`' | '<' | '>' | '"' | '@' | '\u{ab}' | '\u{bb}'
    ) && is_url_unicode_symbol(c)
}

fn is_domain_symbol(c: char) -> bool {
    if (c as u32) < 0xc0 {
        c == '.' || c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '~'
    } else {
        is_url_unicode_symbol(c)
    }
}

fn byte_len(chars: &[char]) -> usize {
    chars.iter().map(|c| c.len_utf8()).sum()
}

fn byte_prefix(chars: &[char]) -> Vec<usize> {
    let mut out = Vec::with_capacity(chars.len() + 1);
    let mut acc = 0;
    for c in chars {
        out.push(acc);
        acc += c.len_utf8();
    }
    out.push(acc);
    out
}

fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    chars[from..].iter().position(|&c| c == target).map(|pos| pos + from)
}

fn match_tg_urls(chars: &[char]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let end = chars.len();
    let byte_off = byte_prefix(chars);
    let mut ptr = 0;
    while byte_off[end] - byte_off[ptr] > 5 {
        let Some(colon) = find_char(chars, ptr, ':') else { break };
        ptr = colon;

        let mut url_begin = None;
        if end - ptr >= 3 && chars[ptr + 1] == '/' && chars[ptr + 2] == '/' {
            if ptr >= 2 && chars[ptr - 2].to_ascii_lowercase() == 't' && chars[ptr - 1].to_ascii_lowercase() == 'g' {
                url_begin = Some(ptr - 2);
            } else if ptr >= 3
                && chars[ptr - 3].to_ascii_lowercase() == 't'
                && chars[ptr - 2].to_ascii_lowercase() == 'o'
                && chars[ptr - 1].to_ascii_lowercase() == 'n'
            {
                url_begin = Some(ptr - 3);
            } else if ptr >= 7
                && chars[ptr - 7].to_ascii_lowercase() == 't'
                && chars[ptr - 6].to_ascii_lowercase() == 'o'
                && chars[ptr - 5].to_ascii_lowercase() == 'n'
                && chars[ptr - 4].to_ascii_lowercase() == 's'
                && chars[ptr - 3].to_ascii_lowercase() == 'i'
                && chars[ptr - 2].to_ascii_lowercase() == 't'
                && chars[ptr - 1].to_ascii_lowercase() == 'e'
            {
                url_begin = Some(ptr - 3);
            }
        }
        let Some(url_begin) = url_begin else {
            ptr += 1;
            continue;
        };

        ptr += 3;
        let domain_begin = ptr;
        while ptr != end && ptr - domain_begin != 253 && is_alpha_digit_or_underscore_or_minus(chars[ptr]) {
            ptr += 1;
        }
        if ptr == domain_begin {
            continue;
        }
        if ptr != end && matches!(chars[ptr], '/' | '?' | '#') {
            let mut path_end = ptr + 1;
            while path_end != end && is_url_path_symbol(chars[path_end]) {
                path_end += 1;
            }
            while path_end > ptr + 1 && is_bad_path_end_char(chars[path_end - 1]) {
                path_end -= 1;
            }
            if chars[ptr] == '/' || path_end > ptr + 1 {
                ptr = path_end;
            }
        }
        result.push((url_begin, ptr));
    }
    result
}

fn match_urls(chars: &[char]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let end = chars.len();
    let mut begin = 0;

    loop {
        let Some(dot_pos) = find_char(chars, begin, '.') else { break };
        if dot_pos + 1 == end {
            break;
        }
        if chars[dot_pos + 1] == ' ' {
            begin = dot_pos + 2;
            continue;
        }

        let mut domain_begin = dot_pos;
        while domain_begin != begin {
            domain_begin -= 1;
            if !is_domain_symbol(chars[domain_begin]) {
                domain_begin += 1;
                break;
            }
        }

        let mut last_at = None;
        let mut domain_end = dot_pos;
        while domain_end != end {
            let code = chars[domain_end];
            if code == '@' {
                last_at = Some(domain_end);
            } else if !is_domain_symbol(code) {
                break;
            }
            domain_end += 1;
        }

        if last_at.is_some() {
            while domain_begin != begin {
                domain_begin -= 1;
                if !is_user_data_symbol(chars[domain_begin]) {
                    domain_begin += 1;
                    break;
                }
            }
        }

        let mut url_end = domain_end;
        if url_end != end && chars[url_end] == ':' {
            let mut port_end = url_end + 1;
            while port_end != end && chars[port_end].is_ascii_digit() {
                port_end += 1;
            }
            let mut port_begin = url_end + 1;
            while port_begin != port_end && chars[port_begin] == '0' {
                port_begin += 1;
            }
            if port_begin != port_end && port_end - port_begin <= 5 {
                let mut port = 0u32;
                for &c in &chars[port_begin..port_end] {
                    port = port * 10 + c.to_digit(10).unwrap_or(0);
                }
                if port <= 65535 {
                    url_end = port_end;
                }
            }
        }

        if url_end != end && matches!(chars[url_end], '/' | '?' | '#') {
            let mut path_end = url_end + 1;
            while path_end != end && is_url_path_symbol(chars[path_end]) {
                path_end += 1;
            }
            while path_end > url_end + 1 && is_bad_path_end_char(chars[path_end - 1]) {
                path_end -= 1;
            }
            if chars[url_end] == '/' || path_end > url_end + 1 {
                url_end = path_end;
            }
        }
        while url_end > dot_pos + 1 && chars[url_end - 1] == '.' {
            url_end -= 1;
        }

        let mut is_bad = false;
        let mut url_begin = domain_begin;
        if url_begin != begin && chars[url_begin - 1] == '@' {
            if last_at.is_some() {
                is_bad = true;
            }
            let mut user_data_begin = url_begin - 1;
            while user_data_begin != begin {
                user_data_begin -= 1;
                if !is_user_data_symbol(chars[user_data_begin]) {
                    user_data_begin += 1;
                    break;
                }
            }
            if user_data_begin == url_begin - 1 {
                is_bad = true;
            }
            url_begin = user_data_begin;
        }

        if url_begin != begin {
            let prefix = &chars[begin..url_begin];
            if prefix.len() >= 6 && prefix.ends_with(&[':', '/', '/']) {
                let protocol_end = url_begin - 3;
                let mut protocol_begin = protocol_end;
                while protocol_begin != begin {
                    protocol_begin -= 1;
                    if !is_protocol_symbol(chars[protocol_begin]) {
                        protocol_begin += 1;
                        break;
                    }
                }
                let protocol = &chars[protocol_begin..protocol_end];
                if ascii_suffix_ci(protocol, "http") && !ascii_eq_ci(protocol, "shttp") {
                    url_begin -= 7;
                } else if ascii_suffix_ci(protocol, "https") {
                    url_begin -= 8;
                } else if ascii_suffix_ci(protocol, "ftp")
                    && !ascii_eq_ci(protocol, "tftp")
                    && !ascii_eq_ci(protocol, "sftp")
                {
                    url_begin -= 6;
                } else if ascii_suffix_ci(protocol, "tonsite") {
                    url_begin -= 10;
                } else {
                    is_bad = true;
                }
            } else {
                let last = chars[url_begin - 1];
                if is_word_char(last) || last == '/' || last == '#' || last == '@' {
                    is_bad = true;
                }
            }
        }

        if !is_bad {
            if url_end > dot_pos + 1 {
                result.push((url_begin, url_end));
            }
            while url_end != end && chars[url_end] == '.' {
                url_end += 1;
            }
        } else {
            while chars[url_end - 1] != '.' {
                url_end -= 1;
            }
        }

        if url_end <= dot_pos {
            url_end = dot_pos + 1;
        }
        begin = url_end;
    }

    result
}

fn split_chars<'a>(chars: &'a [char], delimiter: char) -> Vec<&'a [char]> {
    let mut result = Vec::new();
    if chars.is_empty() {
        return result;
    }
    let mut rest = chars;
    while let Some(pos) = rest.iter().position(|&c| c == delimiter) {
        result.push(&rest[..pos]);
        rest = &rest[pos + 1..];
    }
    result.push(rest);
    result
}

fn is_email_address(chars: &[char]) -> bool {
    let Some(at_pos) = chars.iter().position(|&c| c == '@') else { return false };
    let userdata = &chars[..at_pos];
    let domain = &chars[at_pos + 1..];
    if domain.is_empty() {
        return false;
    }

    let mut prev = 0;
    let mut userdata_part_count = 0;
    for i in 0..userdata.len() {
        if userdata[i] == '.' || userdata[i] == '+' {
            if i - prev >= 27 {
                return false;
            }
            userdata_part_count += 1;
            prev = i + 1;
        } else if !is_alpha_digit_or_underscore_or_minus(userdata[i]) {
            return false;
        }
    }
    userdata_part_count += 1;
    if userdata_part_count >= 12 {
        return false;
    }
    let last_part_length = userdata.len() - prev;
    if last_part_length == 0 || last_part_length >= 36 {
        return false;
    }

    let domain_parts = split_chars(domain, '.');
    if domain_parts.len() <= 1 || domain_parts.len() > 7 {
        return false;
    }
    let tld = domain_parts[domain_parts.len() - 1];
    if tld.len() <= 1 || tld.len() >= 9 {
        return false;
    }
    if !tld.iter().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    for part in &domain_parts[..domain_parts.len() - 1] {
        if part.is_empty() || part.len() >= 31 {
            return false;
        }
        if !part.iter().all(|&c| is_alpha_digit_or_underscore_or_minus(c)) {
            return false;
        }
        if !part[0].is_ascii_alphanumeric() || !part[part.len() - 1].is_ascii_alphanumeric() {
            return false;
        }
    }

    true
}

const COMMON_TLDS: &str = "
    aaa aarp abb abbott abbvie abc able abogado abudhabi ac academy accenture accountant accountants
    aco actor ad ads adult ae aeg aero aetna af afl africa ag agakhan agency ai aig airbus airforce
    airtel akdn al alibaba alipay allfinanz allstate ally alsace alstom am amazon americanexpress
    americanfamily amex amfam amica amsterdam analytics android anquan anz ao aol apartments app
    apple aq aquarelle ar arab aramco archi army arpa art arte as asda asia associates at athleta
    attorney au auction audi audible audio auspost author auto autos aw aws ax axa az azure ba baby
    baidu banamex band bank bar barcelona barclaycard barclays barefoot bargains baseball basketball
    bauhaus bayern bb bbc bbt bbva bcg bcn bd be beats beauty beer bentley berlin best bestbuy bet
    bf bg bh bharti bi bible bid bike bing bingo bio biz bj black blackfriday blockbuster blog
    bloomberg blue bm bms bmw bn bnpparibas bo boats boehringer bofa bom bond boo book booking bosch
    bostik boston bot boutique box br bradesco bridgestone broadway broker brother brussels bs bt
    build builders business buy buzz bv bw by bz bzh ca cab cafe cal call calvinklein cam camera
    camp canon capetown capital capitalone car caravan cards care career careers cars casa case cash
    casino cat catering catholic cba cbn cbre cc cd center ceo cern cf cfa cfd cg ch chanel channel
    charity chase chat cheap chintai christmas chrome church ci cipriani circle cisco citadel citi
    citic city ck cl claims cleaning click clinic clinique clothing cloud club clubmed cm cn co
    coach codes coffee college cologne com commbank community company compare computer comsec condos
    construction consulting contact contractors cooking cool coop corsica country coupon coupons
    courses cpa cr credit creditcard creditunion cricket crown crs cruise cruises cu cuisinella cv
    cw cx cy cymru cyou cz dabur dad dance data date dating datsun day dclk dds de deal dealer deals
    degree delivery dell deloitte delta democrat dental dentist desi design dev dhl diamonds diet
    digital direct directory discount discover dish diy dj dk dm dnp do docs doctor dog domains dot
    download drive dtv dubai dunlop dupont durban dvag dvr dz earth eat ec eco edeka edu education
    ee eg email emerck energy engineer engineering enterprises epson equipment er ericsson erni es
    esq estate et eu eurovision eus events exchange expert exposed express extraspace fage fail
    fairwinds faith family fan fans farm farmers fashion fast fedex feedback ferrari ferrero fi
    fidelity fido film final finance financial fire firestone firmdale fish fishing fit fitness fj
    fk flickr flights flir florist flowers fly fm fo foo food football ford forex forsale forum
    foundation fox fr free fresenius frl frogans frontier ftr fujitsu fun fund furniture futbol fyi
    ga gal gallery gallo gallup game games gap garden gay gb gbiz gd gdn ge gea gent genting george
    gf gg ggee gh gi gift gifts gives giving gl glass gle global globo gm gmail gmbh gmo gmx gn
    godaddy gold goldpoint golf goo goodyear goog google gop got gov gp gq gr grainger graphics
    gratis green gripe grocery group gs gt gu gucci guge guide guitars guru gw gy hair hamburg
    hangout haus hbo hdfc hdfcbank health healthcare help helsinki here hermes hiphop hisamitsu
    hitachi hiv hk hkt hm hn hockey holdings holiday homedepot homegoods homes homesense honda horse
    hospital host hosting hot hotels hotmail house how hr hsbc ht hu hughes hyatt hyundai ibm icbc
    ice icu id ie ieee ifm ikano il im imamat imdb immo immobilien in inc industries infiniti info
    ing ink institute insurance insure int international intuit investments io ipiranga iq ir irish
    is ismaili ist istanbul it itau itv jaguar java jcb je jeep jetzt jewelry jio jll jm jmp jnj jo
    jobs joburg jot joy jp jpmorgan jprs juegos juniper kaufen kddi ke kerryhotels kerrylogistics
    kerryproperties kfh kg kh ki kia kids kim kindle kitchen kiwi km kn koeln komatsu kosher kp kpmg
    kpn kr krd kred kuokgroup kw ky kyoto kz la lacaixa lamborghini lamer lancaster land landrover
    lanxess lasalle lat latino latrobe law lawyer lb lc lds lease leclerc lefrak legal lego lexus
    lgbt li lidl life lifeinsurance lifestyle lighting like lilly limited limo lincoln link lipsy
    live living lk llc llp loan loans locker locus lol london lotte lotto love lpl lplfinancial lr
    ls lt ltd ltda lu lundbeck luxe luxury lv ly ma madrid maif maison makeup man management mango
    map market marketing markets marriott marshalls mattel mba mc mckinsey md me med media meet
    melbourne meme memorial men menu merckmsd mg mh miami microsoft mil mini mint mit mitsubishi mk
    ml mlb mls mm mma mn mo mobi mobile moda moe moi mom monash money monster mormon mortgage moscow
    moto motorcycles mov movie mp mq mr ms msd mt mtn mtr mu museum music mv mw mx my mz na nab
    nagoya name navy nba nc ne nec net netbank netflix network neustar new news next nextdirect
    nexus nf nfl ng ngo nhk ni nico nike nikon ninja nissan nissay nl no nokia norton now nowruz
    nowtv np nr nra nrw ntt nu nyc nz obi observer office okinawa olayan olayangroup ollo om omega
    one ong onion onl online ooo open oracle orange org organic origins osaka otsuka ott ovh pa page
    panasonic paris pars partners parts party pay pccw pe pet pf pfizer pg ph pharmacy phd philips
    phone photo photography photos physio pics pictet pictures pid pin ping pink pioneer pizza pk pl
    place play playstation plumbing plus pm pn pnc pohl poker politie porn post pr pramerica praxi
    press prime pro prod productions prof progressive promo properties property protection pru
    prudential ps pt pub pw pwc py qa qpon quebec quest racing radio re read realestate realtor
    realty recipes red redstone redumbrella rehab reise reisen reit reliance ren rent rentals repair
    report republican rest restaurant review reviews rexroth rich richardli ricoh ril rio rip ro
    rocks rodeo rogers room rs rsvp ru rugby ruhr run rw rwe ryukyu sa saarland safe safety sakura
    sale salon samsclub samsung sandvik sandvikcoromant sanofi sap sarl sas save saxo sb sbi sbs sc
    scb schaeffler schmidt scholarships school schule schwarz science scot sd se search seat secure
    security seek select sener services seven sew sex sexy sfr sg sh shangrila sharp shell shia
    shiksha shoes shop shopping shouji show si silk sina singles site sj sk ski skin sky skype sl
    sling sm smart smile sn sncf so soccer social softbank software sohu solar solutions song sony
    soy spa space sport spot sr srl ss st stada staples star statebank statefarm stc stcgroup
    stockholm storage store stream studio study style su sucks supplies supply support surf surgery
    suzuki sv swatch swiss sx sy sydney systems sz tab taipei talk taobao target tatamotors tatar
    tattoo tax taxi tc tci td tdk team tech technology tel temasek tennis teva tf tg th thd theater
    theatre tiaa tickets tienda tips tires tirol tj tjmaxx tjx tk tkmaxx tl tm tmall tn to today
    tokyo ton tools top toray toshiba total tours town toyota toys tr trade trading training travel
    travelers travelersinsurance trust trv tt tube tui tunes tushu tv tvs tw tz ua ubank ubs ug uk
    unicom university uno uol ups us uy uz va vacations vana vanguard vc ve vegas ventures verisign
    vermögensberater vermögensberatung versicherung vet vg vi viajes video vig viking villas vin vip
    virgin visa vision viva vivo vlaanderen vn vodka volvo vote voting voto voyage vu wales walmart
    walter wang wanggou watch watches weather weatherchannel webcam weber website wed wedding weibo
    weir wf whoswho wien wiki williamhill win windows wine winners wme wolterskluwer woodside work
    works world wow ws wtc wtf xbox xerox xihuan xin xxx xyz yachts yahoo yamaxun yandex ye
    yodobashi yoga yokohama you youtube yt yun za zappos zara zero zip zm zone zuerich zw ελ ευ бг
    бел дети ею католик ком мкд мон москва онлайн орг рус рф сайт срб укр қаз հայ ישראל קום ابوظبي
    ارامكو الاردن البحرين الجزائر السعودية العليان المغرب امارات ایران بارت بازار بيتك بھارت تونس
    سودان سورية شبكة عراق عرب عمان فلسطين قطر كاثوليك كوم مصر مليسيا موريتانيا موقع همراه پاکستان
    ڀارت कॉम नेट भारत भारतम् भारोत संगठन বাংলা ভারত ভাৰত ਭਾਰਤ ભારત ଭାରତ இந்தியா இலங்கை சிங்கப்பூர்
    భారత్ ಭಾರತ ഭാരതം ලංකා คอม ไทย ລາວ გე みんな アマゾン クラウド グーグル コム ストア セール ファッション ポイント 世界 中信 中国 中國 中文网
    亚马逊 企业 佛山 信息 健康 八卦 公司 公益 台湾 台灣 商城 商店 商标 嘉里 嘉里大酒店 在线 大拿 天主教 娱乐 家電 广东 微博 慈善 我爱你 手机 招聘 政务 政府 新加坡 新闻
    时尚 書籍 机构 淡马锡 游戏 澳門 点看 移动 组织机构 网址 网店 网站 网络 联通 谷歌 购物 通販 集团 電訊盈科 飞利浦 食品 餐厅 香格里拉 香港 닷넷 닷컴 삼성 한국
";

static COMMON_TLD_SET: std::sync::LazyLock<Vec<&'static str>> =
    std::sync::LazyLock::new(|| COMMON_TLDS.split_ascii_whitespace().collect());

fn is_common_tld(chars: &[char]) -> bool {
    let original: String = chars.iter().collect();
    let tlds = &*COMMON_TLD_SET;
    if original.bytes().all(|b| b.is_ascii_lowercase()) {
        return tlds.binary_search(&original.as_str()).is_ok();
    }
    let lower = original.to_lowercase();
    if lower != original {
        let mut original_tail = original.chars();
        let mut lower_tail = lower.chars();
        original_tail.next();
        lower_tail.next();
        if lower_tail.eq(original_tail) {
            return false;
        }
    }
    tlds.binary_search(&lower.as_str()).is_ok()
}

fn fix_url(chars: &[char]) -> Option<usize> {
    let mut shift = 0;
    let mut has_protocol = false;
    let head = &chars[..chars.len().min(10)];
    if ascii_prefix_ci(head, "http://")
        || ascii_prefix_ci(head, "https://")
        || ascii_prefix_ci(head, "ftp://")
        || ascii_prefix_ci(head, "tonsite://")
    {
        let colon = find_char(chars, 0, ':')?;
        shift = colon + 3;
        has_protocol = true;
    }

    let rest = &chars[shift..];
    let mut domain_end = rest.len();
    for delimiter in ['/', '?', '#'] {
        if let Some(pos) = rest.iter().position(|&c| c == delimiter) {
            domain_end = domain_end.min(pos);
        }
    }
    let mut domain = &rest[..domain_end];
    let path = &rest[domain_end..];

    if let Some(at_pos) = domain.iter().position(|&c| c == '@') {
        domain = &domain[at_pos + 1..];
    }
    if let Some(colon) = domain.iter().rposition(|&c| c == ':') {
        domain = &domain[..colon];
    }

    if byte_len(domain) == 12 && (domain[0] == 't' || domain[0] == 'T') && ascii_eq_ci(domain, "teiegram.org") {
        return None;
    }

    let mut balance = [0i32; 3];
    let mut path_pos = 0;
    while path_pos < path.len() {
        match path[path_pos] {
            '(' => balance[0] += 1,
            '[' => balance[1] += 1,
            '{' => balance[2] += 1,
            ')' => balance[0] -= 1,
            ']' => balance[1] -= 1,
            '}' => balance[2] -= 1,
            _ => {}
        }
        if balance[0] < 0 || balance[1] < 0 || balance[2] < 0 {
            break;
        }
        path_pos += 1;
    }
    while path_pos > 0 && is_bad_path_end_char(path[path_pos - 1]) {
        path_pos -= 1;
    }
    let kept = chars.len() - (path.len() - path_pos);

    let mut prev = 0;
    let mut domain_part_count = 0;
    let mut has_non_digit = false;
    let mut is_ipv4 = true;
    for i in 0..=domain.len() {
        if i == domain.len() || domain[i] == '.' {
            let part_size = byte_len(&domain[prev..i]);
            if part_size == 0 || part_size >= 64 || domain[i - 1] == '-' {
                return None;
            }
            if is_ipv4 {
                if part_size > 3 {
                    is_ipv4 = false;
                }
                if part_size == 3
                    && (domain[prev] >= '3'
                        || (domain[prev] == '2'
                            && (domain[prev + 1] >= '6'
                                || (domain[prev + 1] == '5' && domain[prev + 2] >= '6'))))
                {
                    is_ipv4 = false;
                }
                if domain[prev] == '0' && part_size >= 2 {
                    is_ipv4 = false;
                }
            }
            domain_part_count += 1;
            if i != domain.len() {
                prev = i + 1;
            }
        } else if !domain[i].is_ascii_digit() {
            is_ipv4 = false;
            has_non_digit = true;
        }
    }
    if domain_part_count == 1 {
        return None;
    }
    if is_ipv4 && domain_part_count == 4 {
        return Some(kept);
    }
    if !has_non_digit {
        return None;
    }

    let tld = &domain[prev..];
    if tld.len() <= 1 {
        return None;
    }
    if tld.starts_with(&['x', 'n', '-', '-']) {
        if byte_len(tld) <= 5 {
            return None;
        }
        if !tld[4..].iter().all(|&c| c.is_ascii_alphanumeric()) {
            return None;
        }
    } else {
        if tld.contains(&'_') || tld.contains(&'-') {
            return None;
        }
        if !has_protocol && !is_common_tld(tld) {
            return None;
        }
    }

    let mut pos = prev - 1;
    while pos > 0 {
        pos -= 1;
        if domain[pos] == '_' {
            return None;
        }
        if domain[pos] == '.' {
            break;
        }
    }

    Some(kept)
}

fn find_tg_urls(chars: &[char], out: &mut Vec<(usize, usize, Kind)>) {
    for (start, end) in match_tg_urls(chars) {
        out.push((start, end - start, Kind::Url));
    }
}

fn find_urls(chars: &[char], out: &mut Vec<(usize, usize, Kind)>) {
    for (start, end) in match_urls(chars) {
        let url = &chars[start..end];
        if is_email_address(url) {
            out.push((start, url.len(), Kind::EmailAddress));
        } else if url.starts_with(&['m', 'a', 'i', 'l', 't', 'o', ':']) && is_email_address(&url[7..]) {
            out.push((start + 7, url.len() - 7, Kind::EmailAddress));
        } else if let Some(len) = fix_url(url) {
            out.push((start, len, Kind::Url));
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

fn is_base64any_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/')
}

fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

fn base64_decode_strict(s: &[u8], url_alphabet: bool) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &c in s {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' if url_alphabet => 62,
            b'+' if !url_alphabet => 62,
            b'_' if url_alphabet => 63,
            b'/' if !url_alphabet => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn check_ton_address(address: &[u8]) -> bool {
    if address.len() != 48 {
        return false;
    }
    let buffer = match base64_decode_strict(address, true).or_else(|| base64_decode_strict(address, false)) {
        Some(buffer) => buffer,
        None => return false,
    };
    if buffer.len() != 36 {
        return false;
    }
    let expected = ((buffer[34] as u16) << 8) | buffer[35] as u16;
    if crc16(&buffer[..34]) != expected {
        return false;
    }
    buffer[0] & 0x3f == 0x11
}

fn find_ton_addresses(chars: &[char], out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = 0;
    while i < chars.len() {
        if !is_base64any_char(chars[i]) {
            i += 1;
            continue;
        }
        let begin = i;
        while i < chars.len() && is_base64any_char(chars[i]) {
            i += 1;
        }
        let run: Vec<u8> = chars[begin..i].iter().map(|c| *c as u8).collect();
        if check_ton_address(&run) {
            out.push((begin, i - begin, Kind::TonAddress));
        }
    }
}

fn find_entities(chars: &[char]) -> Vec<(usize, usize, Kind)> {
    let mut all = Vec::new();
    find_mentions(chars, &mut all);
    find_bot_commands(chars, &mut all);
    find_hashtags(chars, &mut all);
    find_cashtags(chars, &mut all);
    find_bank_cards(chars, &mut all);
    find_ton_addresses(chars, &mut all);
    find_tg_urls(chars, &mut all);
    find_urls(chars, &mut all);
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

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Json {
    Null,
    Bool(bool),
    // numbers keep their literal, like JsonValue::get_number in TDLib, because
    // integer fields are parsed from the literal with to_integer_safe
    Int(i64, String),
    Float(f64, String),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn parse(text: &str) -> Option<Json> {
        let bytes = text.as_bytes();
        let mut pos = 0usize;
        let value = parse_value(bytes, &mut pos)?;
        skip_json_ws(bytes, &mut pos);
        if pos != bytes.len() {
            return None;
        }
        Some(value)
    }

    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(members) => members.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(value) => Some(value),
            _ => None,
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(value, _) => Some(*value),
            Json::Float(value, _) => Some(*value as i64),
            _ => None,
        }
    }
}

fn skip_json_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }
}

fn parse_value(bytes: &[u8], pos: &mut usize) -> Option<Json> {
    skip_json_ws(bytes, pos);
    match *bytes.get(*pos)? {
        b'{' => {
            *pos += 1;
            let mut members = Vec::new();
            skip_json_ws(bytes, pos);
            if bytes.get(*pos) == Some(&b'}') {
                *pos += 1;
                return Some(Json::Obj(members));
            }
            loop {
                skip_json_ws(bytes, pos);
                let key = match parse_value(bytes, pos)? {
                    Json::Str(value) => value,
                    _ => return None,
                };
                skip_json_ws(bytes, pos);
                if bytes.get(*pos) != Some(&b':') {
                    return None;
                }
                *pos += 1;
                let value = parse_value(bytes, pos)?;
                members.push((key, value));
                skip_json_ws(bytes, pos);
                match bytes.get(*pos) {
                    Some(b',') => *pos += 1,
                    Some(b'}') => {
                        *pos += 1;
                        return Some(Json::Obj(members));
                    }
                    _ => return None,
                }
            }
        }
        b'[' => {
            *pos += 1;
            let mut items = Vec::new();
            skip_json_ws(bytes, pos);
            if bytes.get(*pos) == Some(&b']') {
                *pos += 1;
                return Some(Json::Arr(items));
            }
            loop {
                items.push(parse_value(bytes, pos)?);
                skip_json_ws(bytes, pos);
                match bytes.get(*pos) {
                    Some(b',') => *pos += 1,
                    Some(b']') => {
                        *pos += 1;
                        return Some(Json::Arr(items));
                    }
                    _ => return None,
                }
            }
        }
        b'"' => parse_json_string(bytes, pos).map(Json::Str),
        b't' => {
            if bytes.len() >= *pos + 4 && &bytes[*pos..*pos + 4] == b"true" {
                *pos += 4;
                Some(Json::Bool(true))
            } else {
                None
            }
        }
        b'f' => {
            if bytes.len() >= *pos + 5 && &bytes[*pos..*pos + 5] == b"false" {
                *pos += 5;
                Some(Json::Bool(false))
            } else {
                None
            }
        }
        b'n' => {
            if bytes.len() >= *pos + 4 && &bytes[*pos..*pos + 4] == b"null" {
                *pos += 4;
                Some(Json::Null)
            } else {
                None
            }
        }
        _ => parse_json_number(bytes, pos),
    }
}

fn parse_json_number(bytes: &[u8], pos: &mut usize) -> Option<Json> {
    let start = *pos;
    let mut is_float = false;
    while *pos < bytes.len() {
        match bytes[*pos] {
            b'-' | b'+' => *pos += 1,
            b'0'..=b'9' => *pos += 1,
            b'.' | b'e' | b'E' => {
                is_float = true;
                *pos += 1;
            }
            _ => break,
        }
    }
    if start == *pos {
        return None;
    }
    let text = std::str::from_utf8(&bytes[start..*pos]).ok()?;
    if is_float {
        text.parse::<f64>()
            .ok()
            .map(|value| Json::Float(value, text.to_string()))
    } else {
        text.parse::<i64>()
            .ok()
            .map(|value| Json::Int(value, text.to_string()))
    }
}

fn parse_json_string(bytes: &[u8], pos: &mut usize) -> Option<String> {
    if bytes.get(*pos) != Some(&b'"') {
        return None;
    }
    *pos += 1;
    let mut out = String::new();
    let mut high_surrogate: Option<u32> = None;
    loop {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        match byte {
            b'"' => return Some(out),
            b'\\' => {
                let escape = *bytes.get(*pos)?;
                *pos += 1;
                match escape {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = std::str::from_utf8(bytes.get(*pos..*pos + 4)?).ok()?;
                        *pos += 4;
                        let code = u32::from_str_radix(hex, 16).ok()?;
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
                    _ => return None,
                }
            }
            _ => {
                let start = *pos - 1;
                let width = utf8_width(byte);
                let slice = bytes.get(start..start + width)?;
                out.push(std::str::from_utf8(slice).ok()?.chars().next()?);
                *pos = start + width;
            }
        }
    }
}

fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn json_escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{:04x}", unit));
                }
            }
        }
    }
}

fn text_object(text: &str) -> String {
    let mut body = String::new();
    json_escape(text, &mut body);
    format!("{{\"@type\":\"text\",\"text\":\"{}\"}}", body)
}

fn error_object(code: i64, message: &str) -> String {
    let mut body = String::new();
    json_escape(message, &mut body);
    format!("{{\"@type\":\"error\",\"code\":{},\"message\":\"{}\"}}", code, body)
}

fn ok_object() -> String {
    "{\"@type\":\"ok\"}".to_string()
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

// ---------------------------------------------------------------------------
// getFileMimeType / getFileExtension: tdutils/td/utils/MimeType.cpp over the
// gperf tables that tdutils/generate/generate_mime_types_gperf.cpp builds from
// tdutils/generate/mime_types.txt. MIME_TYPES_DATA is that 786-line input
// ("mime_type extension...", whitespace separated); both lookup tables are
// derived from it with the generator's exact rules.
// ---------------------------------------------------------------------------

fn mime_lookup(table: &[(&'static str, &'static str)], key: &str) -> &'static str {
    if key.is_empty() {
        return "";
    }
    let lowered = key.to_ascii_lowercase();
    match table.binary_search_by(|(candidate, _)| (*candidate).cmp(lowered.as_str())) {
        Ok(index) => table[index].1,
        Err(_) => "",
    }
}

fn file_mime_type(file_name: &str) -> &'static str {
    mime_lookup(&mime_tables::EXTENSION_TO_MIME, path_extension(file_name))
}

fn mime_type_extension(mime_type: &str) -> &'static str {
    mime_lookup(&mime_tables::MIME_TO_EXTENSION, mime_type)
}

const UNICODE_SIMPLE_CATEGORY_RANGES: [u32; 1655] = [
    0,1028,1056,1538,1856,2081,2912,3105,3936,5124,5152,5441,5472,5699,5760,5793,
    5824,5923,5953,5984,6019,6112,6145,6880,6913,7904,7937,22592,22721,23104,23553,23712,
    23937,23968,24001,24032,28161,28320,28353,28416,28481,28608,28641,28672,28865,28896,28929,29024,
    29057,29088,29121,29760,29793,32448,32481,36928,37185,42496,42529,43744,43777,43840,44033,45344,
    45409,45472,47617,48480,48609,48736,50177,51552,52226,52544,52673,52736,52769,55936,55969,56000,
    56481,56544,56769,56834,57153,57248,57313,57344,57857,57888,57921,58880,59809,62656,63009,63040,
    63490,63809,64864,65153,65216,65345,65376,65537,66240,66369,66400,66689,66720,66817,66848,67585,
    68384,68609,68960,69121,69888,69921,70144,70657,72000,73857,75584,75681,75712,76289,76320,76545,
    76864,76994,77312,77345,77856,77985,78240,78305,78368,78433,79136,79169,79392,79425,79456,79553,
    79680,79777,79808,80321,80352,80769,80832,80865,80960,81090,81409,81472,81539,81728,81793,81824,
    82081,82272,82401,82464,82529,83232,83265,83488,83521,83584,83617,83680,83713,83776,84769,84896,
    84929,84960,85186,85504,85569,85664,86177,86464,86497,86592,86625,87328,87361,87584,87617,87680,
    87713,87872,87969,88000,88577,88608,89089,89152,89282,89600,89889,89920,90273,90528,90593,90656,
    90721,91424,91457,91680,91713,91776,91809,91968,92065,92096,93057,93120,93153,93248,93378,93696,
    93729,93763,93952,94305,94336,94369,94560,94657,94752,94785,94912,95009,95072,95105,95136,95169,
    95232,95329,95392,95489,95584,95681,96064,96769,96800,97474,97795,97888,98465,98720,98753,98848,
    98881,99616,99649,100160,100257,100288,101121,101216,101249,101312,101377,101440,101570,101888,102147,102368,
    102401,102432,102561,102816,102849,102944,102977,103712,103745,104064,104097,104256,104353,104384,105345,105440,
    105473,105536,105666,105984,106017,106080,106625,106912,106945,107040,107073,108384,108449,108480,108993,109024,
    109185,109280,109315,109537,109632,109762,110083,110368,110401,110592,110753,111328,111425,112192,112225,112512,
    112545,112576,112641,112864,113858,114176,114721,116256,116289,116352,116737,116960,117250,117568,118817,118880,
    118913,118944,118977,119136,119169,119936,119969,120000,120033,120352,120385,120448,120737,120768,120833,120992,
    121025,121056,121346,121664,121729,121856,122881,122912,123906,124227,124544,124929,125184,125217,126368,127233,
    127392,131073,132448,133089,133122,133440,133633,133824,133953,134080,134177,134208,134305,134368,134593,134688,
    134817,135232,135617,135648,135682,136000,136193,137408,137441,137472,137633,137664,137729,139104,139137,149792,
    149825,149952,150017,150240,150273,150304,150337,150464,150529,151840,151873,152000,152065,153120,153153,153280,
    153345,153568,153601,153632,153665,153792,153857,154336,154369,156192,156225,156352,156417,158560,159011,159648,
    159745,160256,160769,163520,163585,163776,163873,183712,183777,184324,184353,185184,185345,187744,187843,187937,
    188192,188417,188992,189409,190016,190465,191040,191489,191904,191937,192032,192513,194176,195297,195328,195457,
    195488,195586,195904,196099,196416,197122,197440,197633,200480,200705,200864,200929,202016,202049,202080,202241,
    204480,204801,205792,207042,207361,208320,208385,208544,208897,210304,210433,211264,211458,211779,211808,212993,
    213728,214017,215712,217090,217408,217602,217920,218337,218368,221345,222848,223393,223648,223746,224064,225377,
    226336,226753,226818,227137,228544,229377,230528,231426,231744,231841,231938,232257,233408,233473,233824,233985,
    235360,235425,235520,236833,236960,236993,237184,237217,237280,237377,237408,237569,243712,245761,254656,254721,
    254912,254977,256192,256257,256448,256513,256768,256801,256832,256865,256896,256929,256960,256993,257984,258049,
    259744,259777,260000,260033,260064,260161,260256,260289,260512,260609,260736,260801,260992,261121,261536,261697,
    261792,261825,262048,262148,262496,263428,263488,263652,263680,265188,265216,265731,265761,265792,265859,266048,
    266209,266243,266560,266721,267264,270401,270432,270561,270592,270657,270976,271009,271040,271137,271296,271489,
    271520,271553,271584,271617,271648,271681,271808,271841,272192,272257,272384,272545,272704,272833,272864,272899,
    274529,274595,274752,297987,299904,302403,303104,323267,324224,360449,367776,367969,368096,368193,368256,368547,
    368576,368641,369856,369889,369920,370081,370112,370177,371968,372193,372224,372737,373472,373761,373984,374017,
    374240,374273,374496,374529,374752,374785,375008,375041,375264,375297,375520,375553,375776,378337,378368,393220,
    393248,393377,393443,393472,394275,394560,394785,394944,395011,395105,395168,395297,398048,398241,398336,398369,
    401248,401281,401408,401569,402944,402977,405984,406083,406208,406529,407552,409089,409600,410627,410944,411907,
    412160,412195,412672,413699,414016,415267,415744,425985,636928,638977,1348000,1350145,1351616,1351681,1360288,1360385,
    1360898,1361217,1361280,1361921,1363424,1363937,1364928,1364993,1367235,1367552,1368801,1369088,1369153,1372448,1372513,1375168,
    1375297,1375328,1375777,1376320,1376353,1376448,1376481,1376608,1376641,1377376,1377795,1377984,1378305,1379968,1380417,1382016,
    1382914,1383232,1384001,1384192,1384289,1384320,1384353,1384416,1384450,1384769,1385664,1385985,1386720,1387521,1388448,1388673,
    1390176,1391073,1391106,1391424,1391617,1391776,1391809,1392130,1392449,1392608,1392641,1393952,1394689,1394784,1394817,1395072,
    1395202,1395520,1395713,1396448,1396545,1396576,1396673,1398272,1398305,1398336,1398433,1398496,1398561,1398720,1398785,1398816,
    1398849,1398880,1399649,1399744,1399809,1400160,1400385,1400480,1400865,1401056,1401121,1401312,1401377,1401568,1401857,1402080,
    1402113,1402336,1402369,1403744,1403777,1404224,1404289,1404352,1404417,1408096,1408514,1408832,1409025,1766528,1766913,1767648,
    1767777,1769344,2039809,2051520,2051585,2054976,2056193,2056416,2056801,2056960,2057121,2057152,2057185,2057504,2057537,2057952,
    2057985,2058144,2058177,2058208,2058241,2058304,2058337,2058400,2058433,2061888,2062945,2074560,2075137,2077184,2077249,2078976,
    2080257,2080640,2084353,2084512,2084545,2088864,2089474,2089792,2090017,2090848,2091041,2091872,2092225,2095072,2095169,2095360,
    2095425,2095616,2095681,2095872,2095937,2096032,2097153,2097536,2097569,2098400,2098433,2099040,2099073,2099136,2099169,2099648,
    2099713,2100160,2101249,2105184,2105571,2107008,2107395,2109216,2109763,2109824,2117633,2118560,2118657,2120224,2120739,2121600,
    2121729,2122755,2122880,2123169,2123811,2123841,2124099,2124128,2124289,2125504,2125825,2126784,2126849,2128000,2128129,2128384,
    2128419,2128576,2129921,2134976,2135042,2135360,2135553,2136704,2136833,2137984,2138113,2139392,2139649,2141312,2141697,2142048,
    2142081,2142560,2142593,2142816,2142849,2142912,2142945,2143296,2143329,2143808,2143841,2144064,2144097,2144160,2144257,2145920,
    2146305,2156256,2156545,2157248,2157569,2157824,2158593,2158784,2158817,2160160,2160193,2160640,2162689,2162880,2162945,2162976,
    2163009,2164416,2164449,2164512,2164609,2164640,2164705,2165440,2165507,2165761,2166496,2166563,2166785,2167776,2168035,2168320,
    2169857,2170464,2170497,2170560,2170723,2170881,2171587,2171776,2171905,2172736,2172929,2173760,2174977,2176768,2176899,2176961,
    2177027,2177536,2177603,2179073,2179104,2179585,2179712,2179745,2179840,2179873,2180800,2181123,2181408,2182145,2183075,2183136,
    2183169,2184099,2184192,2185217,2185472,2185505,2186400,2186595,2186752,2187265,2188992,2189313,2190016,2190083,2190337,2190944,
    2191107,2191361,2191936,2192675,2192896,2195457,2197792,2199553,2201184,2201601,2203232,2203459,2203649,2204800,2205186,2205504,
    2205698,2206017,2206912,2207201,2207936,2214915,2215904,2215937,2217280,2217473,2217536,2218049,2218240,2218785,2219488,2220033,
    2220963,2221281,2221312,2221569,2222272,2222627,2222752,2223617,2224192,2225665,2226339,2226560,2227201,2227936,2228321,2230016,
    2230851,2231490,2231808,2231841,2231904,2231969,2232000,2232417,2233856,2234881,2235680,2235906,2236224,2236513,2237664,2238146,
    2238464,2238593,2238624,2238689,2238720,2238977,2240096,2240193,2240224,2240609,2242144,2242593,2242720,2243074,2243393,2243424,
    2243457,2243488,2243619,2244256,2244609,2245184,2245217,2246016,2246625,2246688,2248705,2248928,2248961,2248992,2249025,2249152,
    2249185,2249664,2249697,2250016,2250241,2251744,2252290,2252608,2252961,2253216,2253281,2253344,2253409,2254112,2254145,2254368,
    2254401,2254464,2254497,2254656,2254753,2254784,2255361,2255392,2255777,2255936,2256897,2257216,2257249,2257280,2257345,2257376,
    2257409,2258624,2258657,2258688,2259489,2259520,2259553,2259584,2260993,2262688,2263265,2263392,2263554,2263872,2264033,2264128,
    2265089,2266624,2267265,2267328,2267361,2267392,2267650,2267968,2273281,2274784,2276097,2276224,2277377,2278912,2279553,2279584,
    2279938,2280256,2281473,2282848,2283265,2283296,2283522,2283840,2284034,2284672,2285569,2286432,2287106,2287427,2287488,2287617,
    2287840,2293761,2295168,2298881,2300930,2301251,2301536,2301921,2302176,2302241,2302272,2302337,2302592,2302625,2302688,2302721,
    2303488,2303969,2304000,2304033,2304064,2304514,2304832,2307073,2307328,2307393,2308640,2309153,2309184,2309217,2309248,2310145,
    2310176,2310497,2311776,2312001,2312032,2312705,2312736,2313089,2314560,2315169,2315200,2315777,2318112,2318657,2318688,2324481,
    2325536,2326018,2326336,2326529,2326816,2326849,2328032,2328577,2328608,2329090,2329411,2330016,2330177,2331136,2334721,2334944,
    2334977,2335040,2335073,2336288,2336961,2336992,2337282,2337600,2337793,2337984,2338017,2338080,2338113,2339136,2339585,2339616,
    2339842,2340160,2340353,2341760,2341890,2342208,2342433,2342464,2350081,2350688,2351169,2351200,2351233,2351648,2351681,2352768,
    2353666,2353984,2356737,2356768,2357251,2357920,2359297,2388800,2392067,2395648,2395811,2396161,2402432,2402819,2412768,2486785,
    2489888,2490369,2524672,2525217,2525408,2526209,2654048,2654209,2672864,2891777,2892736,2893314,2893632,2949121,2967328,2967553,
    2968544,2968578,2968896,2969089,2971616,2971650,2971968,2972161,2973120,2973697,2975232,2975745,2975872,2976258,2976576,2976611,
    2976832,2976865,2977536,2977697,2978304,2992129,2993568,2993666,2993984,3000321,3002371,3003104,3003393,3004192,3004257,3005056,
    3006465,3008864,3009025,3009056,3011169,3011584,3013633,3013696,3013729,3013760,3014209,3014275,3014368,3014657,3251040,3252193,
    3253280,3256321,3260000,3260417,3289664,3290113,3291744,3538433,3538560,3538593,3538816,3538849,3538912,3538945,3548448,3548737,
    3548768,3549697,3549792,3549857,3549888,3550337,3550496,3550721,3563392,3637249,3640672,3640833,3641248,3641345,3641632,3641857,
    3642176,3776002,3776320,3823619,3824256,3824643,3825280,3828739,3829536,3833857,3836576,3836609,3838880,3838913,3838976,3839041,
    3839072,3839137,3839200,3839265,3839392,3839425,3839808,3839841,3839872,3839905,3840128,3840161,3842240,3842273,3842400,3842465,
    3842720,3842753,3842976,3843009,3843904,3843937,3844064,3844097,3844256,3844289,3844320,3844417,3844640,3844673,3855584,3855617,
    3856416,3856449,3857248,3857281,3858272,3858305,3859104,3859137,3860128,3860161,3860960,3860993,3861984,3862017,3862816,3862849,
    3863840,3863873,3864672,3864705,3864960,3865026,3866624,3923969,3928128,3928577,3928800,3930529,3932160,3933697,3935680,3940353,
    3941792,3942113,3942336,3942402,3942720,3942849,3942880,3953153,3954112,3954689,3956096,3956226,3956544,3971585,3972480,3972610,
    3972928,3979777,3980736,3980801,3980834,3981152,3987457,3988448,3988481,3988576,3988609,3988672,3988705,3988928,3988993,3989152,
    3989441,3989504,3996673,3996896,3996929,3997056,3997089,3997152,3997185,3997664,3997697,4004000,4004067,4004352,4005889,4008064,
    4008289,4008320,4008450,4008768,4034083,4035968,4036003,4036096,4036131,4036256,4038691,4040128,4040163,4040640,4046849,4046976,
    4047009,4047872,4047905,4047968,4048001,4048032,4048097,4048128,4048161,4048480,4048513,4048640,4048673,4048704,4048737,4048768,
    4048961,4048992,4049121,4049152,4049185,4049216,4049249,4049280,4049313,4049408,4049441,4049504,4049537,4049568,4049633,4049664,
    4049697,4049728,4049761,4049792,4049825,4049856,4049889,4049920,4049953,4050016,4050049,4050080,4050145,4050272,4050305,4050528,
    4050561,4050688,4050721,4050848,4050881,4050912,4050945,4051264,4051297,4051840,4052001,4052096,4052129,4052288,4052321,4052864,
    4071427,4071840,4161026,4161344,4194305,5561344,5562369,5702624,5702657,5887424,5887489,6126624,6127105,6147008,6225921,6243264,
    6291457,6449504,6449665,6721344,7995393,8357888,4294967295,
];
const UNICODE_SIMPLE_CATEGORY_TABLE: [u8; 128] = [
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    4,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2,2,2,2,2,2,2,2,2,2,0,0,0,0,0,0,
    0,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,0,0,0,0,0,
    0,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,0,0,0,0,0,
];

// Code points for which prepare_search_character() returns 0, i.e. which
// clean_filename_part() drops. Only that distinction is observable there.
const PREPARE_SEARCH_DELETED_RANGES: [(u32, u32); 353] = [
    (0, 0),(173, 173),(183, 183),(700, 700),(702, 702),
    (768, 836),(838, 879),(1155, 1161),(1425, 1469),(1471, 1471),
    (1473, 1474),(1476, 1477),(1479, 1481),(1536, 1541),(1552, 1562),
    (1564, 1564),(1611, 1631),(1648, 1648),(1750, 1757),(1759, 1764),
    (1767, 1768),(1770, 1773),(1807, 1807),(1809, 1809),(1840, 1866),
    (1958, 1968),(2027, 2035),(2045, 2045),(2070, 2073),(2075, 2083),
    (2085, 2087),(2089, 2093),(2137, 2139),(2192, 2193),(2199, 2207),
    (2250, 2307),(2362, 2364),(2366, 2383),(2385, 2391),(2402, 2403),
    (2433, 2435),(2492, 2492),(2494, 2500),(2503, 2504),(2507, 2509),
    (2519, 2519),(2530, 2531),(2558, 2558),(2561, 2563),(2620, 2620),
    (2622, 2626),(2631, 2632),(2635, 2637),(2641, 2641),(2672, 2673),
    (2677, 2677),(2689, 2691),(2748, 2748),(2750, 2757),(2759, 2761),
    (2763, 2765),(2786, 2787),(2810, 2815),(2817, 2819),(2876, 2876),
    (2878, 2884),(2887, 2888),(2891, 2893),(2899, 2903),(2914, 2915),
    (2946, 2946),(3006, 3010),(3014, 3016),(3018, 3021),(3031, 3031),
    (3072, 3076),(3132, 3132),(3134, 3140),(3142, 3144),(3146, 3149),
    (3157, 3158),(3170, 3171),(3201, 3203),(3260, 3260),(3262, 3268),
    (3270, 3272),(3274, 3277),(3285, 3286),(3298, 3299),(3315, 3315),
    (3328, 3331),(3387, 3388),(3390, 3396),(3398, 3400),(3402, 3405),
    (3415, 3415),(3426, 3427),(3457, 3459),(3530, 3530),(3535, 3540),
    (3542, 3542),(3544, 3551),(3570, 3571),(3633, 3633),(3636, 3642),
    (3655, 3662),(3761, 3761),(3764, 3772),(3784, 3790),(3864, 3865),
    (3893, 3893),(3895, 3895),(3897, 3897),(3902, 3903),(3953, 3972),
    (3974, 3975),(3981, 3991),(3993, 4028),(4038, 4038),(4139, 4158),
    (4182, 4185),(4190, 4192),(4194, 4196),(4199, 4205),(4209, 4212),
    (4226, 4237),(4239, 4239),(4250, 4253),(4957, 4959),(5906, 5909),
    (5938, 5940),(5970, 5971),(6002, 6003),(6068, 6099),(6109, 6109),
    (6155, 6159),(6277, 6278),(6313, 6313),(6432, 6443),(6448, 6459),
    (6679, 6683),(6741, 6750),(6752, 6780),(6783, 6783),(6832, 6896),
    (6912, 6916),(6964, 6980),(7019, 7027),(7040, 7042),(7073, 7085),
    (7142, 7155),(7204, 7223),(7376, 7378),(7380, 7400),(7405, 7405),
    (7412, 7412),(7415, 7417),(7616, 7679),(8203, 8207),(8234, 8238),
    (8288, 8292),(8294, 8303),(8400, 8432),(11503, 11505),(11647, 11647),
    (11744, 11775),(12330, 12335),(12441, 12442),(12540, 12540),(42607, 42610),
    (42612, 42621),(42654, 42655),(42736, 42737),(43010, 43010),(43014, 43014),
    (43019, 43019),(43043, 43047),(43052, 43052),(43136, 43137),(43188, 43205),
    (43232, 43249),(43263, 43263),(43302, 43309),(43335, 43347),(43392, 43395),
    (43443, 43456),(43493, 43493),(43561, 43574),(43587, 43587),(43596, 43597),
    (43643, 43645),(43696, 43696),(43698, 43700),(43703, 43704),(43710, 43711),
    (43713, 43713),(43755, 43759),(43765, 43766),(44003, 44010),(44012, 44013),
    (55296, 57343),(64286, 64286),(65024, 65039),(65056, 65071),(65279, 65279),
    (65529, 65531),(66045, 66045),(66272, 66272),(66422, 66426),(68097, 68099),
    (68101, 68102),(68108, 68111),(68152, 68154),(68159, 68159),(68325, 68326),
    (68900, 68903),(68969, 68973),(69291, 69292),(69323, 69327),(69360, 69375),
    (69446, 69456),(69506, 69509),(69632, 69634),(69688, 69702),(69744, 69744),
    (69747, 69748),(69759, 69762),(69808, 69818),(69821, 69821),(69826, 69826),
    (69837, 69837),(69888, 69890),(69927, 69940),(69957, 69958),(70003, 70003),
    (70016, 70018),(70067, 70080),(70089, 70092),(70094, 70095),(70188, 70199),
    (70206, 70206),(70209, 70209),(70367, 70378),(70400, 70403),(70459, 70460),
    (70462, 70468),(70471, 70472),(70475, 70477),(70487, 70487),(70498, 70499),
    (70502, 70508),(70512, 70516),(70584, 70592),(70594, 70594),(70597, 70597),
    (70599, 70602),(70604, 70608),(70610, 70610),(70625, 70626),(70709, 70726),
    (70750, 70750),(70832, 70851),(71087, 71093),(71096, 71104),(71132, 71133),
    (71216, 71232),(71339, 71351),(71453, 71467),(71724, 71738),(71984, 71989),
    (71991, 71992),(71995, 71998),(72000, 72000),(72002, 72003),(72145, 72151),
    (72154, 72160),(72164, 72164),(72193, 72202),(72243, 72249),(72251, 72254),
    (72263, 72263),(72273, 72283),(72330, 72345),(72544, 72551),(72751, 72758),
    (72760, 72767),(72850, 72871),(72873, 72886),(73009, 73014),(73018, 73018),
    (73020, 73021),(73023, 73029),(73031, 73031),(73098, 73102),(73104, 73105),
    (73107, 73111),(73200, 73200),(73459, 73462),(73472, 73473),(73475, 73475),
    (73524, 73530),(73534, 73538),(73562, 73562),(78896, 78912),(78919, 78933),
    (90398, 90415),(92912, 92916),(92976, 92982),(94031, 94031),(94033, 94087),
    (94095, 94098),(94180, 94180),(94192, 94193),(113821, 113822),(113824, 113827),
    (118528, 118573),(118576, 118598),(119079, 119080),(119141, 119145),(119149, 119170),
    (119173, 119179),(119210, 119213),(119362, 119364),(119376, 119378),(119387, 119388),
    (119391, 119391),(119424, 119425),(121344, 121398),(121403, 121452),(121461, 121461),
    (121476, 121476),(121499, 121503),(121505, 121519),(122880, 122886),(122888, 122904),
    (122907, 122913),(122915, 122916),(122918, 122922),(123023, 123023),(123184, 123190),
    (123566, 123566),(123628, 123631),(124140, 124143),(124398, 124399),(124643, 124643),
    (124646, 124646),(124654, 124655),(124661, 124661),(125136, 125142),(125252, 125258),
    (917505, 917505),(917536, 917631),(917760, 917999),
];

const CATEGORY_LETTER: u8 = 1;
const CATEGORY_DECIMAL_NUMBER: u8 = 2;
const CATEGORY_NUMBER: u8 = 3;

fn unicode_simple_category(code: u32) -> u8 {
    if code < 128 {
        return UNICODE_SIMPLE_CATEGORY_TABLE[code as usize];
    }
    // The original jump table only narrows the search start; the boundary found
    // this way is the same one its forward scan stops at.
    let packed = (code << 5) + 30;
    let index = UNICODE_SIMPLE_CATEGORY_RANGES.partition_point(|&boundary| boundary <= packed);
    (UNICODE_SIMPLE_CATEGORY_RANGES[index - 1] & 31) as u8
}

fn prepare_search_character_deletes(code: u32) -> bool {
    let index = PREPARE_SEARCH_DELETED_RANGES.partition_point(|&(begin, _)| begin <= code);
    index > 0 && code <= PREPARE_SEARCH_DELETED_RANGES[index - 1].1
}

fn is_filename_char(code: u32) -> bool {
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

fn clean_filename_part(name: &str, max_length: usize) -> String {
    let mut result = String::new();
    let mut size = 0;
    for character in name.chars() {
        if size >= max_length {
            break;
        }
        let mut code = character;
        if !is_filename_char(code as u32) {
            if prepare_search_character_deletes(code as u32) {
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

fn path_view(path: &str) -> (usize, usize) {
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

fn path_extension(path: &str) -> &str {
    let (_, last_dot) = path_view(path);
    if last_dot == path.len() {
        return "";
    }
    &path[last_dot + 1..]
}

fn path_file_stem(path: &str) -> &str {
    let (file_name_start, last_dot) = path_view(path);
    &path[file_name_start..last_dot]
}

fn clean_filename(name: &str) -> String {
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

fn is_emoji(prefix: &str) -> bool {
    emoji_table::EMOJIS.binary_search(&prefix).is_ok()
}

fn match_emoji_at(text: &str, pos: usize) -> Option<usize> {
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

fn skip_emoji_selector(bytes: &[u8], pos: usize) -> usize {
    if pos + 3 <= bytes.len() && bytes[pos] == 0xEF && bytes[pos + 1] == 0xB8 && bytes[pos + 2] == 0x8F {
        pos + 3
    } else {
        pos
    }
}

fn find_emoji_ranges(text: &str) -> Vec<(usize, usize)> {
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

fn emoji_ranges_json(text: &str) -> String {
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

fn number_value(value: Option<&Json>) -> f64 {
    match value {
        Some(Json::Float(number, _)) => *number,
        Some(Json::Int(number, _)) => *number as f64,
        _ => 0.0,
    }
}

fn emit_typed_json_value(value: &Json, out: &mut String) {
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
        Json::Str(text) => {
            out.push_str("{\"@type\":\"jsonValueString\",\"value\":\"");
            json_escape(text, out);
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

fn emit_json_text(value: &Json, out: &mut String) {
    match value.get("@type").and_then(Json::as_str) {
        Some("jsonValueNull") => out.push_str("null"),
        Some("jsonValueBoolean") => {
            let flag = matches!(value.get("value"), Some(Json::Bool(true)));
            out.push_str(if flag { "true" } else { "false" });
        }
        Some("jsonValueNumber") => {
            out.push_str(&format!("{:.6}", number_value(value.get("value"))));
        }
        Some("jsonValueString") => {
            out.push('"');
            if let Some(text) = value.get("value").and_then(Json::as_str) {
                json_escape(text, out);
            }
            out.push('"');
        }
        Some("jsonValueArray") => {
            out.push('[');
            if let Some(Json::Arr(items)) = value.get("values") {
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    emit_json_text(item, out);
                }
            }
            out.push(']');
        }
        Some("jsonValueObject") => {
            out.push('{');
            if let Some(Json::Arr(members)) = value.get("members") {
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push('"');
                    if let Some(key) = member.get("key").and_then(Json::as_str) {
                        json_escape(key, out);
                    }
                    out.push_str("\":");
                    if let Some(member_value) = member.get("value") {
                        emit_json_text(member_value, out);
                    }
                }
            }
            out.push('}');
        }
        _ => out.push_str("null"),
    }
}

fn color_text(color: i64) -> String {
    format!("#{:06x}", color as u32 & 0xFF_FF_FF)
}

fn theme_parameters_json_string(theme: Option<&Json>) -> String {
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

const FOLDER_ICON_NAMES: [&str; 30] = [
    "All", "Unread", "Unmuted", "Bots", "Channels", "Groups", "Private", "Custom", "Setup", "Cat", "Crown",
    "Favorite", "Flower", "Game", "Home", "Love", "Mask", "Party", "Sport", "Study", "Trade", "Travel", "Work",
    "Airplane", "Book", "Light", "Like", "Money", "Note", "Palette",
];

fn json_array_len(value: Option<&Json>) -> usize {
    match value {
        Some(Json::Arr(items)) => items.len(),
        _ => 0,
    }
}

fn json_flag(value: Option<&Json>) -> bool {
    matches!(value, Some(Json::Bool(true)))
}

fn chat_folder_default_icon_name(folder: Option<&Json>) -> Option<String> {
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

fn base64url_prefix_value(text: &str) -> Option<i64> {
    let prefix: &[u8] = text.as_bytes().get(..12)?;
    let decoded = base64_decode_strict(prefix, true)?;
    if decoded.len() != 9 {
        return None;
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&decoded[..8]);
    Some(i64::from_le_bytes(bytes))
}

fn push_receiver_id(payload: &str) -> Result<i64, String> {
    if payload == "{}" {
        return Ok(0);
    }
    let Some(Json::Obj(_)) = Json::parse(payload) else {
        return Err("Failed to parse payload as JSON object".to_string());
    };
    let mut root = Json::parse(payload).unwrap_or(Json::Null);
    if let Some(data) = root.get("data") {
        root = data.clone();
    }
    if let Some(encrypted) = root.get("p").and_then(Json::as_str) {
        if encrypted.len() < 12 {
            return Err("Encrypted payload is too small".to_string());
        }
        return base64url_prefix_value(encrypted)
            .ok_or_else(|| "Failed to base64url-decode payload".to_string());
    }
    if let Some(user_id) = root.get("user_id").and_then(Json::as_i64) {
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

const MAX_USER_ID: i64 = (1 << 40) - 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EType {
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
    fn index(self) -> u32 {
        self as u32
    }

    fn type_name(self) -> &'static str {
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
    fn message_name(self) -> &'static str {
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
    fn priority(self) -> i32 {
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
struct TextEntity {
    etype: EType,
    offset: i32,
    length: i32,
    argument: String,
    user_id: i64,
    custom_emoji_id: i64,
    date: i32,
    date_flags: i32,
    media_timestamp: i32,
}

impl TextEntity {
    fn new(etype: EType, offset: i32, length: i32) -> TextEntity {
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

fn sort_entities(entities: &mut [TextEntity]) {
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

fn entity_type_mask(etype: EType) -> u32 {
    1 << etype.index()
}

fn splittable_entities_mask() -> u32 {
    entity_type_mask(EType::Bold)
        | entity_type_mask(EType::Italic)
        | entity_type_mask(EType::Underline)
        | entity_type_mask(EType::Strikethrough)
        | entity_type_mask(EType::Spoiler)
}

fn blockquote_entities_mask() -> u32 {
    entity_type_mask(EType::BlockQuote) | entity_type_mask(EType::ExpandableBlockQuote)
}

fn continuous_entities_mask() -> u32 {
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

fn pre_entities_mask() -> u32 {
    entity_type_mask(EType::Pre)
        | entity_type_mask(EType::Code)
        | entity_type_mask(EType::PreCode)
        | entity_type_mask(EType::FormattedDate)
}

fn user_entities_mask() -> u32 {
    splittable_entities_mask()
        | blockquote_entities_mask()
        | entity_type_mask(EType::TextUrl)
        | entity_type_mask(EType::MentionName)
        | entity_type_mask(EType::CustomEmoji)
        | pre_entities_mask()
}

fn is_splittable_entity(etype: EType) -> bool {
    entity_type_mask(etype) & splittable_entities_mask() != 0
}

fn is_blockquote_entity(etype: EType) -> bool {
    entity_type_mask(etype) & blockquote_entities_mask() != 0
}

fn is_continuous_entity(etype: EType) -> bool {
    entity_type_mask(etype) & continuous_entities_mask() != 0
}

fn is_pre_entity(etype: EType) -> bool {
    entity_type_mask(etype) & pre_entities_mask() != 0
}

fn is_user_entity(etype: EType) -> bool {
    entity_type_mask(etype) & user_entities_mask() != 0
}

const SPLITTABLE_ENTITY_TYPE_COUNT: usize = 5;

fn splittable_entity_type_index(etype: EType) -> usize {
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
fn text_length(text: &[u8]) -> i32 {
    let mut result = 0;
    for &c in text {
        if utf8_is_first_code_unit(c) {
            result += 1 + i32::from((c & 0xf8) == 0xf0);
        }
    }
    result
}

/// td/utils/utf8.cpp utf8_utf16_truncate
fn utf16_truncate(text: &[u8], mut length: i32) -> &[u8] {
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
fn utf8_utf16_substr(text: &[u8], offset: i32, length: i32) -> &[u8] {
    let rest = if offset == 0 {
        text
    } else {
        let cut = utf16_truncate(text, offset).len();
        &text[cut..]
    };
    utf16_truncate(rest, length)
}

fn is_valid_user_id(user_id: i64) -> bool {
    0 < user_id && user_id <= MAX_USER_ID
}

fn remove_empty_entities(entities: &mut Vec<TextEntity>) {
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

fn are_entities_valid(entities: &[TextEntity]) -> bool {
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
fn remove_intersecting_entities(entities: &mut Vec<TextEntity>) {
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
fn remove_entities_intersecting_blockquote(
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

struct SplitEntitiesState<'a> {
    entities: &'a [TextEntity],
    result: Vec<TextEntity>,
    it: usize,
    begin_pos: [i32; SPLITTABLE_ENTITY_TYPE_COUNT],
    end_pos: [i32; SPLITTABLE_ENTITY_TYPE_COUNT],
}

impl SplitEntitiesState<'_> {
    fn flush_entities(&mut self, offset: i32) {
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

    fn add_entities(&mut self, end_offset: i32) {
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
fn split_entities(entities: &mut Vec<TextEntity>, other_entities: &[TextEntity]) {
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

fn resplit_entities(splittable_entities: Vec<TextEntity>, entities: Vec<TextEntity>) -> Vec<TextEntity> {
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

fn fix_entities(entities: &mut Vec<TextEntity>) {
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
fn replace_offending_characters(text: &mut [u8]) {
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

const TEXT_LENGTH_LIMIT: usize = 35000; // server-side limit

fn clean_input_string(text: &mut Vec<u8>) -> bool {
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
fn clean_input_string_with_entities(text: &[u8], entities: &mut Vec<TextEntity>) -> Result<Vec<u8>, String> {
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
fn remove_invalid_entities(text: &[u8], entities: &mut Vec<TextEntity>) -> bool {
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
fn fix_formatted_text(text: &mut String, entities: &mut Vec<TextEntity>) -> Result<(), String> {
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

fn find_text_url_entities_v3(text: &[u8]) -> Vec<[usize; 4]> {
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

struct TextUrlState {
    result_text: Vec<u8>,
    result_entities: Vec<TextEntity>,
    result_text_utf16_length: i32,
    part_entities: Vec<TextEntity>,
    part_splittable_entities: [Vec<TextEntity>; SPLITTABLE_ENTITY_TYPE_COUNT],
    part_begin: i32,
    max_end: i32,
    skipped_length: i32,
}

fn text_url_add_part(state: &mut TextUrlState, text: &mut &[u8], part_end: i32) {
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
fn parse_text_url_entities_v3(text: &[u8], entities: &[TextEntity]) -> (Vec<u8>, Vec<TextEntity>) {
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

fn find_splittable_entities_v3(text: &[u8], entities: &[TextEntity]) -> Vec<TextEntity> {
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

fn parse_pre_entities_v3(text: &[u8]) -> (Vec<u8>, Vec<TextEntity>) {
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

struct PreEntitiesState<'a> {
    text: &'a [u8],
    result_text: Vec<u8>,
    result_entities: Vec<TextEntity>,
    result_text_utf16_length: i32,
    part_begin: i32,
    max_end: i32,
    skipped_length: i32,
}

impl PreEntitiesState<'_> {
    fn add_part(&mut self, part_end: i32) {
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
fn parse_pre_entities_v3_with_entities(
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

type MarkdownParser = fn(&[u8], Vec<TextEntity>) -> (Vec<u8>, Vec<TextEntity>);

struct MarkdownPartParser<'a> {
    left_text: &'a [u8],
    result_text: Vec<u8>,
    result_entities: Vec<TextEntity>,
    part_entities: Vec<TextEntity>,
    result_text_utf16_length: i32,
    part_begin: i32,
}

impl<'a> MarkdownPartParser<'a> {
    fn new(text: &'a [u8]) -> MarkdownPartParser<'a> {
        MarkdownPartParser {
            left_text: text,
            result_text: Vec::new(),
            result_entities: Vec::new(),
            part_entities: Vec::new(),
            result_text_utf16_length: 0,
            part_begin: 0,
        }
    }

    fn add_part(&mut self, part_end: i32, parse: MarkdownParser) {
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
fn parse_markdown_v3_without_pre(text: &[u8], entities: Vec<TextEntity>) -> (Vec<u8>, Vec<TextEntity>) {
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
fn parse_markdown_v3_without_blockquote(
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
fn parse_markdown_v3(text: &[u8], entities: Vec<TextEntity>) -> (Vec<u8>, Vec<TextEntity>) {
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

fn is_valid_language_code(code: &[u8]) -> bool {
    code.iter().all(|&c| (33..=126).contains(&c))
}

// text entities must be valid
fn get_markdown_v3(text: String, entities: Vec<TextEntity>) -> (String, Vec<TextEntity>) {
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

fn byte_at(bytes: &[u8], pos: usize) -> u8 {
    bytes.get(pos).copied().unwrap_or(0)
}

fn is_space_byte(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 0 | 0x0b)
}

fn is_alpha_byte(c: u8) -> bool {
    let c = c | 0x20;
    c >= b'a' && c <= b'z'
}

fn is_digit_byte(c: u8) -> bool {
    c >= b'0' && c <= b'9'
}

fn is_alnum_byte(c: u8) -> bool {
    is_alpha_byte(c) || is_digit_byte(c)
}

fn hex_digit_value(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some(u32::from(c - b'0')),
        b'a'..=b'f' => Some(u32::from(c - b'a') + 10),
        b'A'..=b'F' => Some(u32::from(c - b'A') + 10),
        _ => None,
    }
}

fn is_hex_digit_byte(c: u8) -> bool {
    hex_digit_value(c).is_some()
}

fn utf8_is_first_code_unit(c: u8) -> bool {
    c & 0xc0 != 0x80
}

fn to_lower_bytes(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .map(|&c| if c.is_ascii_uppercase() { c | 0x20 } else { c })
        .collect()
}

fn tolower_begins_with(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len()
        && prefix
            .iter()
            .enumerate()
            .all(|(index, &expected)| bytes[index].to_ascii_lowercase() == expected)
}

fn bytes_to_string(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    }
}

fn to_integer_i64(bytes: &[u8]) -> i64 {
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

fn to_integer_i32(bytes: &[u8]) -> i32 {
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

fn to_integer_safe_i64(bytes: &[u8]) -> Option<i64> {
    let value = to_integer_i64(bytes);
    if value.to_string().as_bytes() == bytes {
        Some(value)
    } else {
        None
    }
}

fn to_integer_safe_i32(bytes: &[u8]) -> Option<i32> {
    let value = to_integer_i32(bytes);
    if value.to_string().as_bytes() == bytes {
        Some(value)
    } else {
        None
    }
}

/// td/utils/HttpUrl.cpp
struct HttpUrl {
    https: bool,
    userinfo: Vec<u8>,
    host: Vec<u8>,
    is_ipv6: bool,
    specified_port: i32,
    query: Vec<u8>,
}

impl HttpUrl {
    fn get_url(&self) -> Vec<u8> {
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
fn is_valid_ipv6(text: &[u8]) -> bool {
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

fn check_url_part(part: &[u8], name: &str, allow_colon: bool) -> Result<(), String> {
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

fn parse_url(url: &[u8]) -> Result<HttpUrl, String> {
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

fn check_link_impl(link: &[u8]) -> Result<Vec<u8>, String> {
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
fn check_link(link: &[u8]) -> Result<Vec<u8>, String> {
    check_link_impl(link).map_err(|reason| match std::str::from_utf8(link) {
        Ok(text) => format!("URL '{}' is invalid: {}", text, reason),
        Err(_) => format!("URL is invalid: {}", reason),
    })
}

fn get_checked_link(link: &[u8]) -> Option<Vec<u8>> {
    check_link_impl(link).ok()
}

fn split_parameter(parameter: &[u8]) -> (&[u8], &[u8]) {
    match parameter.iter().position(|&c| c == b'=') {
        Some(pos) => (&parameter[..pos], &parameter[pos + 1..]),
        None => (parameter, &[]),
    }
}

fn get_link_user_id(url: &[u8]) -> Option<i64> {
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

fn check_tg_url_host(url: &[u8], host: &[u8]) -> Option<Vec<u8>> {
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

fn get_link_custom_emoji_id(url: &[u8]) -> Option<i64> {
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
const DATE_RELATIVE: i32 = 1;
const DATE_SHORT_TIME: i32 = 2;
const DATE_LONG_TIME: i32 = 4;
const DATE_SHORT_DATE: i32 = 8;
const DATE_LONG_DATE: i32 = 16;
const DATE_DAY_OF_WEEK: i32 = 32;

fn get_formatted_date(date: i32, date_format: &[u8]) -> Result<(i32, i32), String> {
    let date_flags = get_date_flags(date_format)?;
    Ok((date, date_flags))
}

fn get_date_flags(date_format: &[u8]) -> Result<i32, String> {
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

fn get_link_formatted_date(url: &[u8]) -> Option<(i32, i32)> {
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

fn encode_code(code: u32, units: &mut [u8; 4]) -> usize {
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

fn append_utf8_code(out: &mut Vec<u8>, code: u32) {
    let mut units = [0u8; 4];
    let len = encode_code(code, &mut units);
    out.extend_from_slice(&units[..len]);
}

fn write_utf8_code(buf: &mut [u8], pos: usize, code: u32) -> usize {
    let mut units = [0u8; 4];
    let len = encode_code(code, &mut units);
    buf[pos..pos + len].copy_from_slice(&units[..len]);
    len
}

fn decode_html_entity(bytes: &[u8], pos: usize) -> Option<(u32, usize)> {
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

fn reserved_character_error(c: u8) -> String {
    format!(
        "Character '{}' is reserved and must be escaped with the preceding '\\'",
        c as char
    )
}

fn parse_markdown(text: &[u8]) -> Result<(String, Vec<TextEntity>), String> {
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

struct NestedEntity {
    etype: EType,
    argument: Vec<u8>,
    entity_offset: i32,
    entity_byte_offset: usize,
    entity_begin_pos: usize,
}

fn parse_markdown_v2(text: &[u8]) -> Result<(String, Vec<TextEntity>), String> {
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

struct HtmlNestedEntity {
    tag_name: Vec<u8>,
    argument: Vec<u8>,
    date: i32,
    entity_offset: i32,
    entity_begin_pos: usize,
}

fn parse_html(text: &[u8]) -> Result<(String, Vec<TextEntity>), String> {
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

fn formatting_type_json(date_flags: i32) -> Option<String> {
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

fn entity_type_json(entity: &TextEntity) -> String {
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

fn formatted_text_json(text: &str, entities: &[TextEntity]) -> String {
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

fn json_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "Null",
        Json::Bool(_) => "Boolean",
        Json::Int(..) | Json::Float(..) => "Number",
        Json::Str(_) => "String",
        Json::Arr(_) => "Array",
        Json::Obj(_) => "Object",
    }
}

fn json_int64_field(value: &Json) -> Result<i64, String> {
    let literal = match value {
        Json::Null => return Ok(0),
        Json::Int(_, literal) | Json::Float(_, literal) => literal,
        Json::Str(text) => text,
        other => return Err(format!("Expected Number, but receive {}", json_type_name(other))),
    };
    match to_integer_safe_i64(literal.as_bytes()) {
        Some(number) => Ok(number),
        None => Err(format!("Can't parse as an integer string \"{}\"", literal)),
    }
}

/// td_api constructor identifiers, which the JSON layer also accepts as "@type"
const TEXT_ENTITY_TYPE_CONSTRUCTORS: [(i32, &str); 24] = [
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

const DATE_TIME_FORMATTING_CONSTRUCTORS: [(i32, &str); 5] = [
    (-648483424, "dateTimeFormattingTypeRelative"),
    (47463317, "dateTimeFormattingTypeAbsolute"),
    (1790302111, "dateTimePartPrecisionNone"),
    (1290158159, "dateTimePartPrecisionShort"),
    (-105378824, "dateTimePartPrecisionLong"),
];

fn json_class_name<'a>(value: &'a Json, constructors: &[(i32, &'static str)]) -> Result<&'a str, String> {
    match value.get("@type") {
        Some(Json::Str(name)) => Ok(name),
        Some(Json::Int(_, literal)) | Some(Json::Float(_, literal)) => {
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

fn json_nullable_class_name<'a>(
    value: Option<&'a Json>,
    constructors: &[(i32, &'static str)],
) -> Result<Option<&'a str>, String> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(value) => json_class_name(value, constructors).map(Some),
    }
}

/// FormattedDate::get_formatted_date(date, DateTimeFormattingType)
fn formatting_type_flags(value: Option<&Json>) -> Result<i32, String> {
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
fn get_message_entities(entities: Option<&Json>, allow_all: bool) -> Result<Vec<TextEntity>, String> {
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
                let argument = entity_type.get("language").and_then(Json::as_str).unwrap_or("");
                let mut language = argument.as_bytes().to_vec();
                if !clean_input_string(&mut language) {
                    return Err("MessageEntityPreCode.language must be encoded in UTF-8".to_string());
                }
                let mut entity = TextEntity::new(EType::PreCode, offset, length);
                entity.argument = bytes_to_string(language);
                entity
            }
            "textEntityTypeTextUrl" => {
                let url = entity_type.get("url").and_then(Json::as_str).unwrap_or("");
                let mut url = url.as_bytes().to_vec();
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

fn json_int32_field(value: &Json) -> Result<i32, String> {
    let literal = match value {
        Json::Null => return Ok(0),
        Json::Int(_, literal) | Json::Float(_, literal) => literal,
        Json::Str(text) => text,
        other => return Err(format!("Expected Number, but receive {}", json_type_name(other))),
    };
    match to_integer_safe_i32(literal.as_bytes()) {
        Some(number) => Ok(number),
        None => Err(format!("Can't parse as an integer string \"{}\"", literal)),
    }
}

fn parse_entities_error(message: &str) -> String {
    error_object(400, &format!("Can't parse entities: {}", message))
}

/// the formattedText argument shared by parseMarkdown and getMarkdownText
fn read_formatted_text(request: &Json, allow_all: bool) -> Result<(String, Vec<TextEntity>), String> {
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
        Some(Json::Str(value)) => value.clone(),
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

fn parse_markdown_request(request: &Json) -> String {
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

fn get_markdown_text_request(request: &Json) -> String {
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
const TEXT_PARSE_MODE_HTML_ID: i32 = 1660208627;
const TEXT_PARSE_MODE_MARKDOWN_ID: i32 = 360073407;

fn parse_text_entities(request: &Json) -> String {
    let format_error = |message: String| {
        error_object(
            400,
            &format!("Failed to parse JSON object as TDLib request: {}", message),
        )
    };
    let text = match request.get("text") {
        None | Some(Json::Null) => "",
        Some(Json::Str(text)) => text,
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
        Some(Json::Str(name)) => match name.as_str() {
            "textParseModeHTML" => true,
            "textParseModeMarkdown" => false,
            other => return format_error(format!("Unknown class \"{}\"", other)),
        },
        Some(Json::Int(_, literal)) | Some(Json::Float(_, literal)) => {
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

fn execute(request: &str) -> String {
    let Some(json) = Json::parse(request) else {
        return error_object(400, "tdjson_rs: malformed request");
    };
    let Some(kind) = json.get("@type").and_then(Json::as_str) else {
        return error_object(400, "tdjson_rs: malformed request");
    };
    match kind {
        "getTextEntities" => match json.get("text").and_then(Json::as_str) {
            Some(text) => entities_json(text),
            None => error_object(400, "Text must be encoded in UTF-8"),
        },
        "checkQuickReplyShortcutName" => match json.get("name").and_then(Json::as_str) {
            Some(name) => check_shortcut_name(name),
            None => error_object(400, "Strings must be encoded in UTF-8"),
        },
        "getCountryFlagEmoji" => match json.get("country_code").and_then(Json::as_str) {
            Some(code) => text_object(&country_flag_emoji(code)),
            None => text_object(""),
        },
        "testReturnError" => match json.get("error") {
            Some(error) => error_object(
                error.get("code").and_then(Json::as_i64).unwrap_or(0),
                error.get("message").and_then(Json::as_str).unwrap_or(""),
            ),
            None => error_object(404, "Not Found"),
        },
        "getFileMimeType" => text_object(file_mime_type(
            json.get("file_name").and_then(Json::as_str).unwrap_or(""),
        )),
        "getFileExtension" => text_object(mime_type_extension(
            json.get("mime_type").and_then(Json::as_str).unwrap_or(""),
        )),
        "cleanFileName" => text_object(&clean_filename(
            json.get("file_name").and_then(Json::as_str).unwrap_or(""),
        )),
        "getTextEmojiRanges" => match json.get("text").and_then(Json::as_str) {
            Some(text) => emoji_ranges_json(text),
            None => error_object(400, "Text must be encoded in UTF-8"),
        },
        "parseTextEntities" => parse_text_entities(&json),
        "parseMarkdown" => parse_markdown_request(&json),
        "getMarkdownText" => get_markdown_text_request(&json),
        "getJsonValue" => match json.get("json").and_then(Json::as_str) {
            Some(text) => match Json::parse(text) {
                Some(value) => {
                    let mut out = String::new();
                    emit_typed_json_value(&value, &mut out);
                    out
                }
                None => error_object(400, "Failed to parse JSON object"),
            },
            None => error_object(400, "Failed to parse JSON object"),
        },
        "getJsonString" => match json.get("json_value") {
            Some(value) => {
                let mut out = String::new();
                emit_json_text(value, &mut out);
                text_object(&out)
            }
            None => text_object(""),
        },
        "getThemeParametersJsonString" => {
            text_object(&theme_parameters_json_string(json.get("theme")))
        }
        "getChatFolderDefaultIconName" => match chat_folder_default_icon_name(json.get("folder")) {
            Some(name) => {
                let mut out = String::from("{\"@type\":\"chatFolderIcon\",\"name\":\"");
                json_escape(&name, &mut out);
                out.push_str("\"}");
                out
            }
            None => error_object(400, "Chat folder must be non-empty"),
        },
        "getPushReceiverId" => {
            let payload = json.get("payload").and_then(Json::as_str).unwrap_or("");
            match push_receiver_id(payload) {
                Ok(id) => format!("{{\"@type\":\"pushReceiverId\",\"id\":\"{}\"}}", id),
                Err(message) => error_object(400, &message),
            }
        }
        "getOption" => {
            let name = json.get("name").and_then(Json::as_str).unwrap_or("");
            let value = match name {
                "version" => "1.8.71",
                "commit_hash" => env!("TD_GIT_COMMIT_HASH"),
                _ => return error_object(400, "The option can't be get synchronously"),
            };
            let mut body = String::from("{\"@type\":\"optionValueString\",\"value\":\"");
            json_escape(value, &mut body);
            body.push_str("\"}");
            body
        }
        other => error_object(
            400,
            &format!("tdjson_rs: unimplemented request type {}", other),
        ),
    }
}

fn check_shortcut_name(name: &str) -> String {
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

fn country_flag_emoji(country_code: &str) -> String {
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