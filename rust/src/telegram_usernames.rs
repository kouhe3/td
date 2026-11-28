//! `td::Usernames`: the value type behind `td_api::usernames`.
//!
//! Moved verbatim out of `bin/usernames_probe.rs` when the `td_api_objects`
//! lane needed `get_usernames_object()` on top of the same model; the probe
//! keeps its narration, the model keeps its behaviour (facts F205/F206).

#![allow(dead_code)]

use crate::script_tokens::{parse_flag, parse_name, split};

#[derive(Clone)]
pub struct Usernames {
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

pub struct Item {
    name: Vec<u8>,
    active: bool,
    editable: bool,
    deletable: bool,
    expired: bool,
}

pub fn parse_items(token: &[u8]) -> Vec<Item> {
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

pub fn check_utf8(name: &[u8]) -> bool {
    std::str::from_utf8(name).is_ok()
}

impl Usernames {
    pub fn new(first_username: Vec<u8>, items: Vec<Item>) -> Self {
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

    pub fn is_empty(&self) -> bool {
        self.editable_pos == -1 && self.active.is_empty() && self.disabled.is_empty()
    }

    pub fn has_first_username(&self) -> bool {
        !self.active.is_empty()
    }

    pub fn first_username(&self) -> &[u8] {
        if !self.has_first_username() {
            return b"";
        }
        &self.active[0]
    }

    pub fn has_editable_username(&self) -> bool {
        self.editable_pos != -1
    }

    pub fn editable_username(&self) -> &[u8] {
        if !self.has_editable_username() {
            return b"";
        }
        if self.editable_disabled {
            &self.disabled[self.editable_pos as usize]
        } else {
            &self.active[self.editable_pos as usize]
        }
    }

    pub fn check_validness(&mut self) {
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

    pub fn change_editable_username(&self, new_username: Vec<u8>) -> Usernames {
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

    pub fn can_toggle(&self, for_bot: bool, username: &[u8]) -> bool {
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

    pub fn toggle(&self, for_bot: bool, username: &[u8], is_active: bool) -> Usernames {
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

    pub fn can_add_secondary(&self, username: &[u8]) -> bool {
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

    pub fn add_secondary(&self, username: &[u8]) -> Usernames {
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

    pub fn can_delete_secondary(&self, username: &[u8]) -> bool {
        self.deletable.iter().any(|n| n == username) && self.editable_username() != username
    }

    pub fn delete_secondary(&self, username: &[u8]) -> Usernames {
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

    pub fn deactivate_all(&self) -> Usernames {
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

    pub fn can_reorder_to(&self, new_order: &[Vec<u8>]) -> bool {
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

    pub fn reorder_to(&self, new_order: Vec<Vec<u8>>) -> Usernames {
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

    pub fn render(&self) -> Vec<u8> {
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

/// The `td_api::usernames` fields, in the order the JSON printer emits them.
pub struct UsernamesObject {
    pub active: Vec<Vec<u8>>,
    pub disabled: Vec<Vec<u8>>,
    pub editable: Vec<u8>,
    pub collectible: Vec<Vec<u8>>,
    pub secondary: Vec<u8>,
}

impl Usernames {
    /// `Usernames::get_usernames_object()`: `None` is the C++ `nullptr`.
    ///
    /// Collectible usernames are every active/disabled name that is none of the
    /// editable, the other editable or the deletable ones, and the "secondary"
    /// field is the **first deletable username** (not a stored field).
    pub fn get_usernames_object(&self) -> Option<UsernamesObject> {
        if self.is_empty() {
            return None;
        }
        let editable = self.editable_username();
        let mut collectible = Vec::new();
        for name in self.active.iter().chain(self.disabled.iter()) {
            if name.as_slice() != editable
                && !self.other_editable.iter().any(|n| n == name)
                && !self.deletable.iter().any(|n| n == name)
            {
                collectible.push(name.clone());
            }
        }
        Some(UsernamesObject {
            active: self.active.clone(),
            disabled: self.disabled.clone(),
            editable: editable.to_vec(),
            collectible,
            secondary: self.deletable.first().cloned().unwrap_or_default(),
        })
    }
}
