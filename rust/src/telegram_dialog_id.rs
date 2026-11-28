//! `td::DialogId`: the packed dialog identifier.
//!
//! Moved verbatim out of `bin/dialog_id_probe.rs` when the
//! `photo_size_source` lane needed the dialog types for `get_input_peer`;
//! the probe keeps its narration (facts F212/F213).

#![allow(dead_code)]

pub const ZERO_SECRET_CHAT_ID: i64 = -2000000000000;
pub const ZERO_CHANNEL_ID: i64 = -1000000000000;
pub const MAX_USER_ID: i64 = (1i64 << 40) - 1;
pub const MAX_CHAT_ID: i64 = 999999999999;
pub const MAX_CHANNEL_ID: i64 = 1000000000000 - (1i64 << 31);
pub const MIN_MONOFORUM_CHANNEL_ID: i64 = 1000000000000 + (1i64 << 31) + 1;
pub const MAX_MONOFORUM_CHANNEL_ID: i64 = 3000000000000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogType {
    None,
    User,
    Chat,
    Channel,
    SecretChat,
}

#[derive(Clone, Copy, Default)]
pub struct DialogId {
    pub id: i64,
}

impl DialogId {
    pub fn from_int64(id: i64) -> DialogId {
        DialogId { id }
    }

    pub fn from_user(id: i64) -> DialogId {
        if 0 < id && id <= MAX_USER_ID {
            DialogId { id }
        } else {
            DialogId { id: 0 }
        }
    }

    pub fn from_chat(id: i64) -> DialogId {
        if 0 < id && id <= MAX_CHAT_ID {
            DialogId { id: -id }
        } else {
            DialogId { id: 0 }
        }
    }

    pub fn from_channel(id: i64) -> DialogId {
        let is_regular = 0 < id && id < MAX_CHANNEL_ID;
        let is_monoforum = MIN_MONOFORUM_CHANNEL_ID <= id && id < MAX_MONOFORUM_CHANNEL_ID;
        if is_regular || is_monoforum {
            DialogId { id: ZERO_CHANNEL_ID - id }
        } else {
            DialogId { id: 0 }
        }
    }

    pub fn from_secret(id: i32) -> DialogId {
        if id != 0 {
            DialogId { id: ZERO_SECRET_CHAT_ID + id as i64 }
        } else {
            DialogId { id: 0 }
        }
    }

    pub fn get_type(&self) -> DialogType {
        let id = self.id;
        if id < 0 {
            if -MAX_CHAT_ID <= id {
                return DialogType::Chat;
            }
            if ZERO_CHANNEL_ID - MAX_CHANNEL_ID <= id && id != ZERO_CHANNEL_ID {
                return DialogType::Channel;
            }
            if ZERO_SECRET_CHAT_ID + i32::MIN as i64 <= id && id != ZERO_SECRET_CHAT_ID {
                return DialogType::SecretChat;
            }
            if ZERO_CHANNEL_ID - MAX_MONOFORUM_CHANNEL_ID <= id {
                return DialogType::Channel;
            }
        } else if 0 < id && id <= MAX_USER_ID {
            return DialogType::User;
        }
        DialogType::None
    }

    pub fn is_valid(&self) -> bool {
        self.get_type() != DialogType::None
    }

    pub fn get_user_id(&self) -> i64 {
        assert!(self.get_type() == DialogType::User);
        self.id
    }

    pub fn get_chat_id(&self) -> i64 {
        assert!(self.get_type() == DialogType::Chat);
        -self.id
    }

    pub fn get_channel_id(&self) -> i64 {
        assert!(self.get_type() == DialogType::Channel);
        ZERO_CHANNEL_ID - self.id
    }

    pub fn get_secret_chat_id(&self) -> i32 {
        assert!(self.get_type() == DialogType::SecretChat);
        (self.id - ZERO_SECRET_CHAT_ID) as i32
    }

    pub fn render(&self) -> String {
        format!("chat {}", self.id)
    }
}

