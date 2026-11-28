//! Judge `td::FormattedDate` (td/telegram/FormattedDate.{h,cpp}) against the C++
//! oracle (`tools/fixtures/formatted_date_probe.cpp`).
//!
//!   formatted_date_probe <script.txt>
//!
//! Commands (one per line; blank lines and '#' comments ignored):
//!   fmt <date> <format>              get_formatted_date(date, string date_format)
//!                                    `-` means the empty format string
//!   api <date> <spec>                get_formatted_date(date, td_api type)
//!                                    spec = none | rel | abs:<time>:<date>:<dow>
//!                                           time/date are n|s|l, dow is 0|1
//!   ctr <date> <rel> <st> <lt> <sd> <ld> <dow>    the seven-bool constructor
//!   typeobj <date> <flags>           get_date_time_formatting_type_object
//!   entity <date> <flags> <off> <len>  get_input_message_entity_formatted_date
//!   textdate <date> <flags>          get_input_text_date (text is passed as null)
//!   eq <date_a> <flags_a> <date_b> <flags_b>
//!   print <date> <flags>             operator<<

#[allow(dead_code)]
#[path = "../telegram_formatted_date.rs"]
mod telegram_formatted_date;

use std::io::BufRead;

use telegram_formatted_date::{ApiType, FormattedDate, Precision};

fn parse_i32(text: &str) -> Option<i32> {
    // `strtoll` + `static_cast<int32_t>` on the C++ side: out-of-range values
    // truncate instead of being rejected.
    text.parse::<i64>().ok().map(|value| value as i32)
}

fn precision(byte: u8) -> Option<Precision> {
    match byte {
        b'n' => Some(Precision::None),
        b's' => Some(Precision::Short),
        b'l' => Some(Precision::Long),
        _ => None,
    }
}

fn parse_api(spec: &str) -> Option<ApiType> {
    if spec == "none" {
        return Some(ApiType::Null);
    }
    if spec == "rel" {
        return Some(ApiType::Relative);
    }
    let bytes = spec.as_bytes();
    if bytes.len() != 9 || !spec.starts_with("abs:") || bytes[5] != b':' || bytes[7] != b':' {
        return None;
    }
    let time = precision(bytes[4])?;
    let date = precision(bytes[6])?;
    let day_of_week = match bytes[8] {
        b'0' => false,
        b'1' => true,
        _ => return None,
    };
    Some(ApiType::Absolute {
        time,
        date,
        day_of_week,
    })
}

fn run_script(script_path: &str) -> i32 {
    let file = match std::fs::File::open(script_path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("cannot read {script_path}: {error}");
            return 2;
        }
    };
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() || tokens[0].starts_with('#') {
            continue;
        }
        match tokens[0] {
            "fmt" => {
                let (Some(date), Some(raw)) = (
                    tokens.get(1).and_then(|t| parse_i32(t)),
                    tokens.get(2).copied(),
                ) else {
                    println!("reject fmt argc={}", tokens.len());
                    continue;
                };
                if tokens.len() != 3 {
                    println!("reject fmt argc={}", tokens.len());
                    continue;
                }
                let format = if raw == "-" { "" } else { raw };
                match FormattedDate::from_format(date, format) {
                    Ok(value) => println!(
                        "fmt {date} {raw} valid={} {value}",
                        i32::from(value.is_valid())
                    ),
                    Err(error) => println!("fmt {date} {raw} err={error}"),
                }
            }
            "api" => {
                let (Some(date), Some(spec)) = (
                    tokens.get(1).and_then(|t| parse_i32(t)),
                    tokens.get(2).copied(),
                ) else {
                    println!("reject api argc={}", tokens.len());
                    continue;
                };
                if tokens.len() != 3 {
                    println!("reject api argc={}", tokens.len());
                    continue;
                }
                let Some(kind) = parse_api(spec) else {
                    println!("reject api spec={spec}");
                    continue;
                };
                match FormattedDate::from_api(date, kind) {
                    Ok(value) => println!("api {date} {spec} {value}"),
                    Err(error) => println!("api {date} {spec} err={error}"),
                }
            }
            "ctr" => {
                if tokens.len() != 8 {
                    println!("reject ctr argc={}", tokens.len());
                    continue;
                }
                let mut values = [0i32; 6];
                let mut ok = true;
                for (index, slot) in values.iter_mut().enumerate() {
                    match parse_i32(tokens[index + 2]) {
                        Some(value) => *slot = value,
                        None => ok = false,
                    }
                }
                let Some(date) = parse_i32(tokens[1]) else {
                    println!("reject ctr argc={}", tokens.len());
                    continue;
                };
                if !ok {
                    println!("reject ctr argc={}", tokens.len());
                    continue;
                }
                let bools = [values[0] != 0, values[1] != 0, values[2] != 0, values[3] != 0, values[4] != 0, values[5] != 0];
                let value = FormattedDate::new(
                    date, bools[0], bools[1], bools[2], bools[3], bools[4], bools[5],
                );
                let bits: String = values
                    .iter()
                    .map(|v| if *v != 0 { '1' } else { '0' })
                    .collect();
                println!(
                    "ctr {date} {bits} valid={} {value}",
                    i32::from(value.is_valid())
                );
            }
            "typeobj" => {
                let (Some(date), Some(flags)) = (
                    tokens.get(1).and_then(|t| parse_i32(t)),
                    tokens.get(2).and_then(|t| parse_i32(t)),
                ) else {
                    println!("reject typeobj argc={}", tokens.len());
                    continue;
                };
                if tokens.len() != 3 {
                    println!("reject typeobj argc={}", tokens.len());
                    continue;
                }
                let value = FormattedDate::with_flags(date, flags);
                println!("typeobj {date} {flags} {}", value.type_object());
            }
            "entity" => {
                if tokens.len() != 5 {
                    println!("reject entity argc={}", tokens.len());
                    continue;
                }
                let (Some(date), Some(flags), Some(offset), Some(length)) = (
                    parse_i32(tokens[1]),
                    parse_i32(tokens[2]),
                    parse_i32(tokens[3]),
                    parse_i32(tokens[4]),
                ) else {
                    println!("reject entity argc={}", tokens.len());
                    continue;
                };
                let value = FormattedDate::with_flags(date, flags);
                println!(
                    "entity {date} {flags} {offset} {length} {}",
                    value.input_entity(offset, length)
                );
            }
            "textdate" => {
                let (Some(date), Some(flags)) = (
                    tokens.get(1).and_then(|t| parse_i32(t)),
                    tokens.get(2).and_then(|t| parse_i32(t)),
                ) else {
                    println!("reject textdate argc={}", tokens.len());
                    continue;
                };
                if tokens.len() != 3 {
                    println!("reject textdate argc={}", tokens.len());
                    continue;
                }
                let value = FormattedDate::with_flags(date, flags);
                println!("textdate {date} {flags} {}", value.input_text_date());
            }
            "eq" => {
                if tokens.len() != 5 {
                    println!("reject eq argc={}", tokens.len());
                    continue;
                }
                let (Some(date_a), Some(flags_a), Some(date_b), Some(flags_b)) = (
                    parse_i32(tokens[1]),
                    parse_i32(tokens[2]),
                    parse_i32(tokens[3]),
                    parse_i32(tokens[4]),
                ) else {
                    println!("reject eq argc={}", tokens.len());
                    continue;
                };
                let a = FormattedDate::with_flags(date_a, flags_a);
                let b = FormattedDate::with_flags(date_b, flags_b);
                println!(
                    "eq {date_a} {flags_a} {date_b} {flags_b} eq={} ne={}",
                    i32::from(a == b),
                    i32::from(a != b)
                );
            }
            "print" => {
                let (Some(date), Some(flags)) = (
                    tokens.get(1).and_then(|t| parse_i32(t)),
                    tokens.get(2).and_then(|t| parse_i32(t)),
                ) else {
                    println!("reject print argc={}", tokens.len());
                    continue;
                };
                if tokens.len() != 3 {
                    println!("reject print argc={}", tokens.len());
                    continue;
                }
                let value = FormattedDate::with_flags(date, flags);
                println!("print {date} {flags} {value}");
            }
            other => println!("reject unknown={other}"),
        }
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: formatted_date_probe <script.txt>");
        std::process::exit(2);
    }
    std::process::exit(run_script(&args[1]));
}