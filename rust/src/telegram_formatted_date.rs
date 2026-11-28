//! `td::FormattedDate` — `td/telegram/FormattedDate.{h,cpp}`.
//!
//! The class is a pair `(date, date_flags)` plus two decoders: a `date_format`
//! string (`r`/`R` for Relative, otherwise the per-character flags `t T d D w W`)
//! and a `td_api::DateTimeFormattingType` object. Everything else is a mapping
//! back out to flags for `td_api` / `telegram_api`.
//!
//! Two asymmetries the C++ implementation really has (docs/facts.md F282):
//!   * `get_date_flags("dD")` ORs both bits (24), while the bool constructor's
//!     `if short_date else if long_date` keeps only ShortDate (8);
//!   * only the `td_api` overload rejects `date <= 0` (`Invalid date specified`);
//!     the string overload happily builds `date 0 with flags 1`.

pub const RELATIVE: i32 = 1;
pub const SHORT_TIME: i32 = 2;
pub const LONG_TIME: i32 = 4;
pub const SHORT_DATE: i32 = 8;
pub const LONG_DATE: i32 = 16;
pub const DAY_OF_WEEK: i32 = 32;

/// The `td_api::DateTimeFormattingType` variants the C++ switch accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    None,
    Short,
    Long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiType {
    /// `type_ptr == nullptr`
    Null,
    Relative,
    Absolute {
        time: Precision,
        date: Precision,
        day_of_week: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormattedDate {
    pub date: i32,
    pub flags: i32,
}

impl FormattedDate {
    /// The seven-bool constructor: `relative` wins over every other flag, and the
    /// time/date flags are `if short else if long`.
    pub fn new(
        date: i32,
        relative: bool,
        short_time: bool,
        long_time: bool,
        short_date: bool,
        long_date: bool,
        day_of_week: bool,
    ) -> FormattedDate {
        let mut flags = 0;
        if relative {
            flags = RELATIVE;
        } else {
            if short_time {
                flags |= SHORT_TIME;
            } else if long_time {
                flags |= LONG_TIME;
            }
            if short_date {
                flags |= SHORT_DATE;
            } else if long_date {
                flags |= LONG_DATE;
            }
            if day_of_week {
                flags |= DAY_OF_WEEK;
            }
        }
        FormattedDate { date, flags }
    }

    /// The `(date, date_flags)` constructor, public in the C++ header as "tests only".
    pub fn with_flags(date: i32, flags: i32) -> FormattedDate {
        FormattedDate { date, flags }
    }

    pub fn is_valid(&self) -> bool {
        self.date > 0
    }

    /// `get_formatted_date(date, const string &date_format)`; no `date <= 0` check.
    pub fn from_format(date: i32, format: &str) -> Result<FormattedDate, String> {
        if format == "r" || format == "R" {
            return Ok(FormattedDate { date, flags: RELATIVE });
        }
        let mut flags = 0;
        for c in format.chars() {
            flags |= match c {
                't' => SHORT_TIME,
                'T' => LONG_TIME,
                'd' => SHORT_DATE,
                'D' => LONG_DATE,
                'w' | 'W' => DAY_OF_WEEK,
                _ => return Err("Invalid date format used".to_owned()),
            };
        }
        Ok(FormattedDate { date, flags })
    }

    /// `get_formatted_date(date, const td_api::object_ptr<DateTimeFormattingType>&)`.
    pub fn from_api(date: i32, kind: ApiType) -> Result<FormattedDate, String> {
        if date <= 0 {
            return Err("Invalid date specified".to_owned());
        }
        let flags = match kind {
            ApiType::Null => 0,
            ApiType::Relative => RELATIVE,
            ApiType::Absolute {
                time,
                date: date_precision,
                day_of_week,
            } => {
                let mut flags = 0;
                match time {
                    Precision::None => {}
                    Precision::Short => flags |= SHORT_TIME,
                    Precision::Long => flags |= LONG_TIME,
                }
                match date_precision {
                    Precision::None => {}
                    Precision::Short => flags |= SHORT_DATE,
                    Precision::Long => flags |= LONG_DATE,
                }
                if day_of_week {
                    flags |= DAY_OF_WEEK;
                }
                flags
            }
        };
        Ok(FormattedDate { date, flags })
    }

    /// `get_date_time_formatting_type_object()` rendered the way the oracle's
    /// `get_id()` + field dump reads.
    pub fn type_object(&self) -> String {
        if self.flags == 0 {
            return "null".to_owned();
        }
        if self.flags & RELATIVE != 0 {
            return "rel".to_owned();
        }
        let time = if self.flags & SHORT_TIME != 0 {
            "short"
        } else if self.flags & LONG_TIME != 0 {
            "long"
        } else {
            "none"
        };
        let date = if self.flags & SHORT_DATE != 0 {
            "short"
        } else if self.flags & LONG_DATE != 0 {
            "long"
        } else {
            "none"
        };
        format!(
            "abs:{time}:{date}:{}",
            i32::from(self.flags & DAY_OF_WEEK != 0)
        )
    }

    /// `get_input_message_entity_formatted_date(offset, length)`.
    pub fn input_entity(&self, offset: i32, length: i32) -> String {
        format!(
            "rel={} st={} lt={} sd={} ld={} dow={} off={} len={} date={}",
            i32::from(self.flags & RELATIVE != 0),
            i32::from(self.flags & SHORT_TIME != 0),
            i32::from(self.flags & LONG_TIME != 0),
            i32::from(self.flags & SHORT_DATE != 0),
            i32::from(self.flags & LONG_DATE != 0),
            i32::from(self.flags & DAY_OF_WEEK != 0),
            offset,
            length,
            self.date
        )
    }

    /// `get_input_text_date(std::move(text))` with a null `text`.
    pub fn input_text_date(&self) -> String {
        format!(
            "rel={} st={} lt={} sd={} ld={} dow={} text=0 date={}",
            i32::from(self.flags & RELATIVE != 0),
            i32::from(self.flags & SHORT_TIME != 0),
            i32::from(self.flags & LONG_TIME != 0),
            i32::from(self.flags & SHORT_DATE != 0),
            i32::from(self.flags & LONG_DATE != 0),
            i32::from(self.flags & DAY_OF_WEEK != 0),
            self.date
        )
    }
}

impl std::fmt::Display for FormattedDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "date {} with flags {}", self.date, self.flags)
    }
}