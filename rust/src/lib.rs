use std::cell::RefCell;
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

fn extract_string_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let colon = rest.find(':')? + 1;
    let mut it = rest[colon..].chars().peekable();
    while matches!(it.peek(), Some(c) if c.is_whitespace()) {
        it.next();
    }
    if it.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    let mut high_surrogate: Option<u32> = None;
    while let Some(c) = it.next() {
        match c {
            '"' => return Some(out),
            '\\' => {
                let e = it.next()?;
                match e {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let hex: String = (0..4).filter_map(|_| it.next()).collect();
                        let code = u32::from_str_radix(&hex, 16).ok()?;
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
                    other => out.push(other),
                }
            }
            c => out.push(c),
        }
    }
    None
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

fn execute(request: &str) -> String {
    match extract_string_field(request, "@type").as_deref() {
        Some("getTextEntities") => match extract_string_field(request, "text") {
            Some(text) => entities_json(&text),
            None => "{\"@type\":\"error\",\"code\":400,\"message\":\"Text must be encoded in UTF-8\"}".to_string(),
        },
        Some(other) => format!(
            "{{\"@type\":\"error\",\"code\":400,\"message\":\"tdjson_rs: unimplemented request type {}\"}}",
            other.replace('"', "'")
        ),
        None => "{\"@type\":\"error\",\"code\":400,\"message\":\"tdjson_rs: malformed request\"}".to_string(),
    }
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