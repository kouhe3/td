use std::cell::RefCell;
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
    Int(i64),
    Float(f64),
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
            Json::Int(value) => Some(*value),
            Json::Float(value) => Some(*value as i64),
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
        text.parse::<f64>().ok().map(Json::Float)
    } else {
        text.parse::<i64>().ok().map(Json::Int)
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
        Some(Json::Float(number)) => *number,
        Some(Json::Int(number)) => *number as f64,
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
        Json::Int(number) => out.push_str(&format!(
            "{{\"@type\":\"jsonValueNumber\",\"value\":{:.6}}}",
            *number as f64
        )),
        Json::Float(number) => out.push_str(&format!(
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