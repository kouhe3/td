//! `td::MessageId`: the packed message identifier.
//!
//! Moved verbatim out of `bin/message_id_probe.rs` when the `td_api_objects`
//! lane needed `MessageId::get_message_id(telegram_api::Message*)`; the probe
//! keeps its narration (facts F208/F209).

#![allow(dead_code)]

pub const SERVER_ID_SHIFT: i64 = 20;
pub const SHORT_TYPE_MASK: i64 = 3;
pub const TYPE_MASK: i64 = 7;
pub const FULL_TYPE_MASK: i64 = (1 << SERVER_ID_SHIFT) - 1;
pub const SCHEDULED_MASK: i64 = 4;
pub const TYPE_YET_UNSENT: i64 = 1;
pub const TYPE_LOCAL: i64 = 2;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum MessageType {
    None = 0,
    Server = 1,
    YetUnsent = 2,
    Local = 3,
}

#[derive(Clone, Copy, Default)]
pub struct MessageId {
    pub id: i64,
}

impl MessageId {
    pub fn from_int64(id: i64) -> MessageId {
        MessageId { id }
    }

    pub fn from_server(id: i32) -> MessageId {
        MessageId { id: (id as i64) << SERVER_ID_SHIFT }
    }

    pub fn from_scheduled(server_id: i32, send_date: i32, force: bool) -> MessageId {
        if send_date <= (1 << 30) {
            // "Scheduled message send date ... is in the past": the object stays 0.
            return MessageId { id: 0 };
        }
        // ScheduledServerMessageId::is_valid() is 0 < id < 2^18
        if !(server_id > 0 && server_id < (1 << 18)) && !force {
            return MessageId { id: 0 };
        }
        MessageId {
            id: ((send_date as i64 - (1 << 30)) << 21) | ((server_id as i64) << 3) | SCHEDULED_MASK,
        }
    }

    pub fn max() -> MessageId {
        MessageId { id: ((i32::MAX as i64) << SERVER_ID_SHIFT) }
    }

    pub fn is_valid(&self) -> bool {
        if self.id <= 0 || self.id > MessageId::max().id {
            return false;
        }
        if (self.id & FULL_TYPE_MASK) == 0 {
            return true;
        }
        let ty = self.id & TYPE_MASK;
        ty == TYPE_YET_UNSENT || ty == TYPE_LOCAL
    }

    pub fn is_valid_scheduled(&self) -> bool {
        if self.id <= 0 || self.id > (1i64 << 51) {
            return false;
        }
        let ty = self.id & TYPE_MASK;
        ty == SCHEDULED_MASK || ty == (SCHEDULED_MASK | TYPE_YET_UNSENT) || ty == (SCHEDULED_MASK | TYPE_LOCAL)
    }

    pub fn is_valid_sponsored(&self) -> bool {
        if self.id <= MessageId::max().id || self.id > (1i64 << 51) {
            return false;
        }
        (self.id & TYPE_MASK) == TYPE_LOCAL
    }

    pub fn get_type(&self) -> MessageType {
        if self.id <= 0 || self.id > (1i64 << 51) {
            return MessageType::None;
        }
        if self.is_scheduled() {
            return match self.id & TYPE_MASK {
                x if x == SCHEDULED_MASK | TYPE_YET_UNSENT => MessageType::YetUnsent,
                x if x == SCHEDULED_MASK | TYPE_LOCAL => MessageType::Local,
                x if x == SCHEDULED_MASK => MessageType::Server,
                _ => MessageType::None,
            };
        }
        if (self.id & FULL_TYPE_MASK) == 0 {
            return MessageType::Server;
        }
        match self.id & TYPE_MASK {
            TYPE_YET_UNSENT => MessageType::YetUnsent,
            TYPE_LOCAL => MessageType::Local,
            _ => MessageType::None,
        }
    }

    pub fn is_scheduled(&self) -> bool {
        (self.id & SCHEDULED_MASK) != 0
    }

    pub fn is_yet_unsent(&self) -> bool {
        assert!(self.is_valid() || self.is_scheduled());
        (self.id & SHORT_TYPE_MASK) == TYPE_YET_UNSENT
    }

    pub fn is_local(&self) -> bool {
        assert!(self.is_valid() || self.is_scheduled());
        (self.id & SHORT_TYPE_MASK) == TYPE_LOCAL
    }

    pub fn is_server(&self) -> bool {
        (self.id & FULL_TYPE_MASK) == 0 && self.id > 0 && self.id <= MessageId::max().id
    }

    pub fn is_scheduled_server(&self) -> bool {
        assert!(self.is_valid_scheduled());
        (self.id & SHORT_TYPE_MASK) == 0
    }

    pub fn is_any_server(&self) -> bool {
        if self.is_scheduled() {
            self.is_scheduled_server()
        } else {
            self.is_server()
        }
    }

    pub fn get_server_message_id_force(&self) -> i32 {
        assert!(!self.is_scheduled());
        (self.id >> SERVER_ID_SHIFT) as i32
    }

    pub fn get_scheduled_server_message_id_force(&self) -> i32 {
        assert!(self.is_scheduled());
        ((self.id >> 3) & ((1 << 18) - 1)) as i32
    }

    pub fn get_scheduled_server_message_id(&self) -> i32 {
        assert!(self.is_scheduled_server());
        self.get_scheduled_server_message_id_force()
    }

    pub fn get_prev_server_message_id(&self) -> MessageId {
        assert!(!self.is_scheduled());
        MessageId { id: self.id & !FULL_TYPE_MASK }
    }

    pub fn get_next_server_message_id(&self) -> MessageId {
        assert!(!self.is_scheduled());
        MessageId { id: (self.id + FULL_TYPE_MASK) & !FULL_TYPE_MASK }
    }

    pub fn get_scheduled_message_date(&self) -> i32 {
        assert!(self.is_valid_scheduled());
        (self.id >> 21) as i32 + (1 << 30)
    }

    pub fn get_next_message_id(&self, ty: MessageType) -> MessageId {
        if self.is_scheduled() {
            assert!(self.is_valid_scheduled());
            let current_type = self.get_type();
            if (current_type as i32) < (ty as i32) {
                return MessageId { id: self.id - (current_type as i64) + (ty as i64) };
            }
            let base_id = (self.id & !TYPE_MASK) + TYPE_MASK + 1 + SCHEDULED_MASK;
            return match ty {
                MessageType::Server => MessageId { id: base_id },
                MessageType::YetUnsent => MessageId { id: base_id + TYPE_YET_UNSENT },
                MessageType::Local => MessageId { id: base_id + TYPE_LOCAL },
                MessageType::None => unreachable!("get_next_message_id(None)"),
            };
        }
        match ty {
            MessageType::Server => {
                if self.is_server() {
                    MessageId::from_server(self.get_server_message_id_force() + 1)
                } else {
                    self.get_next_server_message_id()
                }
            }
            MessageType::YetUnsent => MessageId {
                id: ((self.id + TYPE_MASK + 1 - TYPE_YET_UNSENT) & !TYPE_MASK) + TYPE_YET_UNSENT,
            },
            MessageType::Local => MessageId {
                id: ((self.id + TYPE_MASK + 1 - TYPE_LOCAL) & !TYPE_MASK) + TYPE_LOCAL,
            },
            MessageType::None => unreachable!("get_next_message_id(None)"),
        }
    }

    pub fn render(&self) -> String {
        let formatted = |value: i32| value.to_string();
        if self.is_scheduled() {
            if !self.is_valid_scheduled() {
                return format!("scheduled invalid message {}", self.id);
            }
            if self.is_scheduled_server() {
                return format!("scheduled server message {}", formatted(self.get_scheduled_server_message_id_force()));
            }
            if self.is_local() {
                return format!("scheduled local message {}", formatted(self.get_scheduled_server_message_id_force()));
            }
            if self.is_yet_unsent() {
                return format!(
                    "scheduled yet unsent message {}",
                    formatted(self.get_scheduled_server_message_id_force())
                );
            }
            return format!("scheduled bugged message {}", self.id);
        }
        if !self.is_valid() {
            return format!("invalid message {}", self.id);
        }
        if self.is_server() {
            return format!("server message {}", formatted(self.get_server_message_id_force()));
        }
        if self.is_local() {
            return format!(
                "local message {}.{}",
                formatted(self.get_server_message_id_force()),
                self.id & FULL_TYPE_MASK
            );
        }
        if self.is_yet_unsent() {
            return format!(
                "yet unsent message {}.{}",
                formatted(self.get_server_message_id_force()),
                self.id & FULL_TYPE_MASK
            );
        }
        format!("bugged message {}", self.id)
    }
}

/// Which `telegram_api` constructor the probe built; the mapping itself reads
/// only `id_` and `date_` (td/telegram/MessageId.cpp:28).
pub enum TlMessageKind {
    Message,
    Empty,
    Service,
}

impl MessageId {
    /// `MessageId::get_message_id(const telegram_api::Message *, bool)`.
    /// `messageEmpty` under `is_scheduled` is the only branch that comes back
    /// invalid; the other two go through `ScheduledServerMessageId`, which
    /// rejects a send date at or below `2^30` and a server id outside `2^18`.
    pub fn from_tl_message(kind: TlMessageKind, id: i32, date: i32, scheduled: bool) -> MessageId {
        match kind {
            TlMessageKind::Empty => {
                if scheduled {
                    MessageId::default()
                } else {
                    MessageId::from_server(id)
                }
            }
            _ => {
                if scheduled {
                    MessageId::from_scheduled(id, date, false)
                } else {
                    MessageId::from_server(id)
                }
            }
        }
    }
}
