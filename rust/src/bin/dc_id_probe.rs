//! td/telegram/net/DcId on the Rust side: the value-level surface around
//! is_valid / create / from_value plus the derived predicates, comparisons and
//! the stream printer.
//!
//! ```text
//! dc_id_probe <script.txt>
//! ```
//!
//! Script lines (`#` starts a comment): `is_valid <v>`, `create <v>`,
//! `from_value <v>`, `internal <v>`, `external <v>`, `cmp <a> <b>`,
//! `cmp_external <a> <b>`, `cmp_mixed <a> <b>`, `raw <v>`.
//!
//! The interesting part is `from_value`: it does not validate, so -1 is main,
//! -2 equals invalid(), and 1001 is "not exact but not empty". `create` does
//! validate and falls back to invalid() outside [1, 1000].

use std::env;
use std::fs;
use std::process::ExitCode;

const MAIN_DC: i32 = -1;
const INVALID_DC: i32 = -2;
const EMPTY_DC: i32 = 0;
const MAX_RAW_DC_ID: i32 = 1000;

#[derive(Clone, Copy)]
struct DcId {
    dc_id: i32,
    is_external: bool,
}

impl DcId {
    fn empty() -> Self {
        Self { dc_id: EMPTY_DC, is_external: false }
    }
    fn invalid() -> Self {
        Self { dc_id: INVALID_DC, is_external: false }
    }
    fn is_valid(dc_id: i32) -> bool {
        1 <= dc_id && dc_id <= MAX_RAW_DC_ID
    }
    fn create(dc_id: i32) -> Self {
        if Self::is_valid(dc_id) {
            Self { dc_id, is_external: false }
        } else {
            Self::invalid()
        }
    }
    fn from_value(dc_id: i32) -> Self {
        Self { dc_id, is_external: false }
    }
    fn internal(dc_id: i32) -> Self {
        assert!(Self::is_valid(dc_id));
        Self { dc_id, is_external: false }
    }
    fn external(dc_id: i32) -> Self {
        assert!(Self::is_valid(dc_id));
        Self { dc_id, is_external: true }
    }

    fn member_is_valid(self) -> bool {
        self.is_exact() || self.is_main()
    }
    fn is_empty(self) -> bool {
        !self.member_is_valid()
    }
    fn is_main(self) -> bool {
        self.dc_id == MAIN_DC
    }
    fn get_raw_id(self) -> i32 {
        assert!(self.is_exact());
        self.dc_id
    }
    fn get_value(self) -> i32 {
        self.dc_id
    }
    fn is_internal(self) -> bool {
        !self.is_external()
    }
    fn is_external(self) -> bool {
        self.is_external
    }
    fn is_exact(self) -> bool {
        self.dc_id > 0
    }
    fn equals(self, other: Self) -> bool {
        self.dc_id == other.dc_id && self.is_external == other.is_external
    }
    fn less(self, other: Self) -> bool {
        (self.dc_id, self.is_external) < (other.dc_id, other.is_external)
    }
    fn render(self) -> String {
        let mut out = String::from("DcId{");
        if self.equals(Self::invalid()) {
            out.push_str("invalid");
        } else if self.equals(Self::empty()) {
            out.push_str("empty");
        } else if self.is_empty() {
            out.push_str("is_empty");
        } else if self.is_main() {
            out.push_str("main");
        } else {
            out.push_str(&self.get_raw_id().to_string());
            if self.is_external() {
                out.push_str(" external");
            }
        }
        out.push('}');
        out
    }
}

fn dump(command: &str, value: i32, dc_id: DcId) {
    println!(
        "{} in={} value={} empty={} main={} exact={} internal={} external={} eq_invalid={} eq_empty={} str={}",
        command,
        value,
        dc_id.get_value(),
        dc_id.is_empty() as i32,
        dc_id.is_main() as i32,
        dc_id.is_exact() as i32,
        dc_id.is_internal() as i32,
        dc_id.is_external() as i32,
        dc_id.equals(DcId::invalid()) as i32,
        dc_id.equals(DcId::empty()) as i32,
        dc_id.render()
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: dc_id_probe <script.txt>");
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
        let command = parts[0];
        let number = |index: usize| -> i32 { parts.get(index).map_or(0, |text| text.parse().unwrap_or(0)) };
        match command {
            "is_valid" => {
                let value = number(1);
                println!("is_valid in={} static={}", value, DcId::is_valid(value) as i32);
            }
            "create" => {
                let value = number(1);
                dump(command, value, DcId::create(value));
            }
            "from_value" => {
                let value = number(1);
                dump(command, value, DcId::from_value(value));
            }
            "internal" => {
                let value = number(1);
                dump(command, value, DcId::internal(value));
            }
            "external" => {
                let value = number(1);
                dump(command, value, DcId::external(value));
            }
            "cmp" | "cmp_external" | "cmp_mixed" => {
                let a = number(1);
                let b = number(2);
                let (left, right) = if command == "cmp" {
                    (DcId::from_value(a), DcId::from_value(b))
                } else if command == "cmp_external" {
                    (DcId::external(a), DcId::external(b))
                } else {
                    (DcId::external(a), DcId::from_value(b))
                };
                println!(
                    "{} a={} b={} eq={} lt={} gt={}",
                    command,
                    a,
                    b,
                    left.equals(right) as i32,
                    left.less(right) as i32,
                    right.less(left) as i32
                );
            }
            "raw" => {
                let value = number(1);
                println!("raw in={} id={}", value, DcId::from_value(value).get_raw_id());
            }
            other => println!("unknown {other}"),
        }
    }
    ExitCode::SUCCESS
}