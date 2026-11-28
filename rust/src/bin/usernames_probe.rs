// (Unofficial) Rust side of the `Usernames` lane; the oracle is
// `tools/fixtures/usernames_probe.cpp` over the real `td::Usernames`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin usernames_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/usernames_probe.exe <script.txt>
//
// Same script language and same stdout bytes as the C++ probe; `--check
// <case>` exists only so the harness can compare "did the process abort".

use std::io::Write;

#[derive(Clone)]
struct Usernames {
    active: Vec<Vec<u8>>,
    disabled: Vec<Vec<u8>>,
    other_editable: Vec<Vec<u8>>,
    deletable: Vec<Vec<u8>>,
    expired: Vec<Vec<u8>>,
    editable_pos: i32,
    editable_disabled: bool,
}

impl Default for Usernames {
    fn default() -> Self {
        Usernames {
            active: Vec::new(),
            disabled: Vec::new(),
            other_editable: Vec::new(),
            deletable: Vec::new(),
            expired: Vec::new(),
            editable_pos: -1,
            editable_disabled: false,
        }
    }
}

struct Item {
    name: Vec<u8>,
    active: bool,
    editable: bool,
    deletable: bool,
    expired: bool,
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

fn parse_name(token: &[u8]) -> Vec<u8> {
    if token == b"-" {
        return Vec::new();
    }
    if token.len() > 4 && &token[..4] == b"hex-" {
        let hex = &token[4..];
        assert!(hex.len() % 2 == 0, "bad hex");
        let mut out = Vec::with_capacity(hex.len() / 2);
        for pair in hex.chunks(2) {
            let hi = hex_digit(pair[0]).expect("bad hex digit");
            let lo = hex_digit(pair[1]).expect("bad hex digit");
            out.push((hi << 4) | lo);
        }
        return out;
    }
    token.to_vec()
}

fn parse_flag(token: &[u8]) -> bool {
    token == b"1"
}

fn split<'a>(line: &'a [u8], sep: u8) -> Vec<&'a [u8]> {
    if line.is_empty() {
        return vec![];
    }
    let mut out: Vec<&[u8]> = line.split(|c| *c == sep).collect();
    if line.last() == Some(&sep) {
        out.pop();
    }
    out
}

fn parse_csv(token: &[u8]) -> Vec<Vec<u8>> {
    if token == b"-" {
        return Vec::new();
    }
    split(token, b',').into_iter().map(parse_name).collect()
}

fn parse_items(token: &[u8]) -> Vec<Item> {
    if token == b"-" {
        return Vec::new();
    }
    split(token, b',')
        .into_iter()
        .map(|item| {
            let parts = split(item, b':');
            assert!(parts.len() == 5, "bad item");
            let mut name = parse_name(parts[0]);
            let active = parse_flag(parts[1]);
            let editable = parse_flag(parts[2]);
            let deletable = parse_flag(parts[3]);
            let expired = parse_flag(parts[4]);
            Item { name: std::mem::take(&mut name), active, editable, deletable, expired }
        })
        .collect()
}

fn check_utf8(name: &[u8]) -> bool {
    std::str::from_utf8(name).is_ok()
}

impl Usernames {
    fn new(first_username: Vec<u8>, items: Vec<Item>) -> Self {
        if items.is_empty() {
            let mut result = Usernames::default();
            if !first_username.is_empty() {
                result.active.push(first_username);
                result.editable_pos = 0;
            }
            return result;
        }
        if !first_username.is_empty() {
            // "Receive first username with usernames": the object stays empty.
            return Usernames::default();
        }

        let mut result = Usernames::default();
        let mut was_editable = false;
        let mut editable_disabled = false;
        let mut received: Vec<Vec<u8>> = Vec::new();
        let mut items = items;
        for item in items.drain(..) {
            let mut item = item;
            if item.name.is_empty() {
                continue;
            }
            if received.iter().any(|n| *n == item.name) {
                continue;
            }
            received.push(item.name.clone());
            if item.deletable {
                item.editable = false;
                result.deletable.push(item.name.clone());
            }
            if item.expired {
                result.expired.push(item.name.clone());
            }
            if item.editable {
                if was_editable {
                    item.editable = false;
                    result.other_editable.push(item.name.clone());
                } else {
                    was_editable = true;
                }
            }
            if item.active {
                result.active.push(item.name.clone());
                if item.editable {
                    editable_disabled = false;
                    result.editable_pos = result.active.len() as i32 - 1;
                }
            } else {
                result.disabled.push(item.name.clone());
                if item.editable {
                    editable_disabled = true;
                    result.editable_pos = result.disabled.len() as i32 - 1;
                }
            }
        }
        result.editable_disabled = editable_disabled;
        assert_eq!(result.has_editable_username(), was_editable);
        result.check_validness();
        result
    }

    fn is_empty(&self) -> bool {
        self.editable_pos == -1 && self.active.is_empty() && self.disabled.is_empty()
    }

    fn has_first_username(&self) -> bool {
        !self.active.is_empty()
    }

    fn first_username(&self) -> &[u8] {
        if !self.has_first_username() {
            return b"";
        }
        &self.active[0]
    }

    fn has_editable_username(&self) -> bool {
        self.editable_pos != -1
    }

    fn editable_username(&self) -> &[u8] {
        if !self.has_editable_username() {
            return b"";
        }
        if self.editable_disabled {
            &self.disabled[self.editable_pos as usize]
        } else {
            &self.active[self.editable_pos as usize]
        }
    }

    fn check_validness(&mut self) {
        let mut seen: Vec<&Vec<u8>> = Vec::new();
        for name in self.active.iter().chain(self.disabled.iter()) {
            if name.is_empty() || !check_utf8(name) || seen.iter().any(|n| *n == name) {
                *self = Usernames::default();
                return;
            }
            seen.push(name);
        }
        if self.editable_disabled {
            if self.editable_pos < 0 || self.editable_pos as usize >= self.disabled.len() {
                *self = Usernames::default();
            }
        } else if self.editable_pos != -1 && self.editable_pos as usize >= self.active.len() {
            *self = Usernames::default();
        }
    }

    fn change_editable_username(&self, new_username: Vec<u8>) -> Usernames {
        let mut result = self.clone();
        if result.has_editable_username() {
            if result.editable_disabled {
                return result;
            }
            if new_username.is_empty() {
                result.active.remove(result.editable_pos as usize);
                result.editable_pos = -1;
            } else {
                result.active[result.editable_pos as usize] = new_username;
            }
        } else if !new_username.is_empty() {
            assert!(!result.editable_disabled);
            result.active.insert(0, new_username);
            result.editable_pos = 0;
        }
        result.check_validness();
        result
    }

    fn can_toggle(&self, for_bot: bool, username: &[u8]) -> bool {
        if self.deletable.iter().any(|n| n == username) {
            return false;
        }
        if self.active.iter().any(|n| n == username) {
            if !self.has_editable_username()
                || self.editable_disabled
                || self.active[self.editable_pos as usize] != username
            {
                return true;
            }
            if for_bot {
                return self.active.len() >= 2;
            }
            return false;
        }
        if self.disabled.iter().any(|n| n == username) {
            return true;
        }
        false
    }

    fn toggle(&self, for_bot: bool, username: &[u8], is_active: bool) -> Usernames {
        let mut result = self.clone();
        if !result.can_toggle(for_bot, username) {
            return result;
        }
        if let Some(i) = result.active.iter().position(|n| n == username) {
            if !is_active {
                result.active.remove(i);
                result.disabled.insert(0, username.to_vec());
                if result.has_editable_username() {
                    if result.editable_disabled {
                        result.editable_pos += 1;
                        if for_bot && result.active.is_empty() {
                            let pos = result.editable_pos as usize;
                            let editable = result.disabled.remove(pos);
                            result.active.push(editable);
                            result.editable_disabled = false;
                            result.editable_pos = 0;
                        }
                    } else if i <= result.editable_pos as usize {
                        if i == result.editable_pos as usize {
                            assert!(for_bot);
                            result.editable_pos = 0;
                            result.editable_disabled = true;
                        } else {
                            assert!(result.editable_pos > 0);
                            result.editable_pos -= 1;
                        }
                    }
                }
            }
            result.check_validness();
            return result;
        }
        if let Some(i) = result.disabled.iter().position(|n| n == username) {
            if is_active {
                result.disabled.remove(i);
                result.active.push(username.to_vec());
                if result.editable_disabled {
                    assert!(result.has_editable_username());
                    if result.editable_pos as usize == i {
                        result.editable_disabled = false;
                        result.editable_pos = result.active.len() as i32 - 1;
                    } else if i <= result.editable_pos as usize {
                        assert!(result.editable_pos > 0);
                        result.editable_pos -= 1;
                    }
                }
            }
            result.check_validness();
            return result;
        }
        unreachable!("toggle of an unknown username");
    }

    fn can_add_secondary(&self, username: &[u8]) -> bool {
        if !self.deletable.is_empty() {
            return false;
        }
        if username.is_empty() {
            return false;
        }
        if self.active.iter().any(|n| n == username) || self.disabled.iter().any(|n| n == username) {
            return false;
        }
        true
    }

    fn add_secondary(&self, username: &[u8]) -> Usernames {
        let mut result = self.clone();
        if !result.can_add_secondary(username) {
            return result;
        }
        result.deletable.push(username.to_vec());
        result.active.insert(0, username.to_vec());
        if result.has_editable_username() && !result.editable_disabled {
            result.editable_pos += 1;
        }
        result.check_validness();
        result
    }

    fn can_delete_secondary(&self, username: &[u8]) -> bool {
        self.deletable.iter().any(|n| n == username) && self.editable_username() != username
    }

    fn delete_secondary(&self, username: &[u8]) -> Usernames {
        let mut result = self.clone();
        if !result.can_delete_secondary(username) {
            return result;
        }
        result.deletable.retain(|n| n != username);
        let mut i = 0;
        while i < result.active.len() {
            if result.active[i] == username {
                result.active.remove(i);
                if result.has_editable_username()
                    && !result.editable_disabled
                    && i <= result.editable_pos as usize
                {
                    assert!(i < result.editable_pos as usize);
                    result.editable_pos -= 1;
                }
            } else {
                i += 1;
            }
        }
        let mut i = 0;
        while i < result.disabled.len() {
            if result.disabled[i] == username {
                result.disabled.remove(i);
                if result.has_editable_username()
                    && result.editable_disabled
                    && i <= result.editable_pos as usize
                {
                    assert!(i < result.editable_pos as usize);
                    result.editable_pos -= 1;
                }
            } else {
                i += 1;
            }
        }
        result.check_validness();
        if result.active.is_empty() && result.has_editable_username() {
            let editable = result.editable_username().to_vec();
            result = result.toggle(true, &editable, true);
        }
        result
    }

    fn deactivate_all(&self) -> Usernames {
        let mut result = Usernames::default();
        for (i, name) in self.active.iter().enumerate() {
            if !self.editable_disabled && i == self.editable_pos as usize {
                result.active.push(name.clone());
                result.editable_pos = 0;
            } else {
                result.disabled.push(name.clone());
            }
        }
        if self.editable_disabled {
            assert!(self.has_editable_username());
            result.editable_pos = self.editable_pos + result.disabled.len() as i32;
        }
        result.disabled.extend(self.disabled.iter().cloned());
        assert_eq!(result.has_editable_username(), self.has_editable_username());
        result.check_validness();
        result
    }

    fn can_reorder_to(&self, new_order: &[Vec<u8>]) -> bool {
        if new_order.len() != self.active.len() {
            return false;
        }
        let mut remaining: Vec<&Vec<u8>> = self.active.iter().collect();
        for name in new_order {
            match remaining.iter().position(|n| *n == name) {
                Some(i) => {
                    remaining.remove(i);
                }
                None => return false,
            }
        }
        assert!(remaining.is_empty());
        true
    }

    fn reorder_to(&self, new_order: Vec<Vec<u8>>) -> Usernames {
        let mut result = Usernames::default();
        result.active = new_order;
        result.disabled = self.disabled.clone();
        if self.has_editable_username() {
            if self.editable_disabled {
                result.editable_pos = self.editable_pos;
            } else {
                let editable = self.active[self.editable_pos as usize].clone();
                for (i, name) in result.active.iter().enumerate() {
                    if *name == editable {
                        result.editable_pos = i as i32;
                        break;
                    }
                }
            }
            assert!(result.has_editable_username());
        }
        result.check_validness();
        result
    }

    fn render(&self) -> Vec<u8> {
        let mut out = b"Usernames[".to_vec();
        if self.has_editable_username() {
            out.extend_from_slice(self.editable_username());
        }
        let push_vec = |out: &mut Vec<u8>, label: &str, names: &Vec<Vec<u8>>| {
            if names.is_empty() {
                return;
            }
            out.extend_from_slice(label.as_bytes());
            out.push(b'{');
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    out.extend_from_slice(b", ");
                }
                out.extend_from_slice(name);
            }
            out.push(b'}');
        };
        push_vec(&mut out, ", active ", &self.active);
        push_vec(&mut out, ", disabled ", &self.disabled);
        push_vec(&mut out, ", other editable ", &self.other_editable);
        push_vec(&mut out, ", deletable ", &self.deletable);
        push_vec(&mut out, ", expired ", &self.expired);
        out.push(b']');
        out
    }
}

fn hex_encode(bytes: &[u8]) -> Vec<u8> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize]);
        out.push(DIGITS[(b & 15) as usize]);
    }
    out
}

fn print_state(usernames: &Usernames, hex: bool, out: &mut std::io::Stdout) {
    let rendered = usernames.render();
    if hex {
        let mut line = b"statehex=".to_vec();
        line.extend_from_slice(&hex_encode(&rendered));
        line.push(b'\n');
        out.write_all(&line).unwrap();
    } else {
        let mut line = b"state=".to_vec();
        line.extend_from_slice(&rendered);
        line.push(b'\n');
        out.write_all(&line).unwrap();
    }
}

fn print_accessors(usernames: &Usernames, out: &mut std::io::Stdout) {
    let first = usernames.first_username();
    let editable = usernames.editable_username();
    let mut line = format!(
        "acc empty={} hasfirst={} first=",
        usernames.is_empty() as u8,
        usernames.has_first_username() as u8
    )
    .into_bytes();
    if first.is_empty() {
        line.push(b'-');
    } else {
        line.extend_from_slice(&hex_encode(first));
    }
    line.extend_from_slice(
        format!(" haseditable={} editable=", usernames.has_editable_username() as u8).as_bytes(),
    );
    if editable.is_empty() {
        line.push(b'-');
    } else {
        line.extend_from_slice(&hex_encode(editable));
    }
    line.push(b'\n');
    out.write_all(&line).unwrap();
}

fn check_case(name: &str) -> i32 {
    if name == "reorder_missing" {
        let usernames = Usernames::new(Vec::new(), parse_items(b"a:1:1:0:0,b:1:0:0:0"));
        let mut out = std::io::stdout();
        let mut line = b"before=".to_vec();
        line.extend_from_slice(&usernames.render());
        line.push(b'\n');
        out.write_all(&line).unwrap();
        out.flush().unwrap();
        let _ = usernames.reorder_to(vec![b"b".to_vec()]);
        return 0;
    }
    2
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--check" {
        std::process::exit(check_case(&args[2]));
    }
    if args.len() != 2 {
        eprintln!("usage: usernames_probe <script.txt>");
        std::process::exit(2);
    }
    let script = std::fs::read(&args[1]).expect("cannot read script");
    let mut current = Usernames::default();
    let mut hex_mode = false;
    let mut out = std::io::stdout();
    for raw_line in script.split(|c| *c == b'\n') {
        let line: &[u8] = raw_line.trim_ascii();
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens: Vec<&[u8]> = line.split(|c| *c == b' ').filter(|t| !t.is_empty()).collect();
        match tokens[0] {
            b"new" => {
                current = Usernames::new(parse_name(tokens[1]), parse_items(tokens[2]));
                print_state(&current, hex_mode, &mut out);
            }
            b"print" => print_state(&current, false, &mut out),
            b"printhex" => print_state(&current, true, &mut out),
            b"hexmode" => hex_mode = parse_flag(tokens[1]),
            b"acc" => print_accessors(&current, &mut out),
            b"canadd" => {
                out.write_all(
                    format!("canadd={}\n", current.can_add_secondary(&parse_name(tokens[1])) as u8).as_bytes(),
                )
                .unwrap();
            }
            b"candel" => {
                out.write_all(
                    format!("candel={}\n", current.can_delete_secondary(&parse_name(tokens[1])) as u8).as_bytes(),
                )
                .unwrap();
            }
            b"cantoggle" => {
                out.write_all(
                    format!(
                        "cantoggle={}\n",
                        current.can_toggle(parse_flag(tokens[1]), &parse_name(tokens[2])) as u8
                    )
                    .as_bytes(),
                )
                .unwrap();
            }
            b"canreorder" => {
                out.write_all(
                    format!("canreorder={}\n", current.can_reorder_to(&parse_csv(tokens[1])) as u8).as_bytes(),
                )
                .unwrap();
            }
            b"change" => {
                current = current.change_editable_username(parse_name(tokens[1]));
                print_state(&current, hex_mode, &mut out);
            }
            b"toggle" => {
                let for_bot = parse_flag(tokens[1]);
                let name = parse_name(tokens[2]);
                let is_active = parse_flag(tokens[3]);
                if !current.can_toggle(for_bot, &name) {
                    out.write_all(b"toggle=skip\n").unwrap();
                    continue;
                }
                current = current.toggle(for_bot, &name, is_active);
                print_state(&current, hex_mode, &mut out);
            }
            b"addsec" => {
                current = current.add_secondary(&parse_name(tokens[1]));
                print_state(&current, hex_mode, &mut out);
            }
            b"delsec" => {
                current = current.delete_secondary(&parse_name(tokens[1]));
                print_state(&current, hex_mode, &mut out);
            }
            b"deact" => {
                current = current.deactivate_all();
                print_state(&current, hex_mode, &mut out);
            }
            b"reorder" => {
                current = current.reorder_to(parse_csv(tokens[1]));
                print_state(&current, hex_mode, &mut out);
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}