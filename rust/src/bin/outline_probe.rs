// (Unofficial) Rust side of the `outline` lane; the oracle is
// `tools/fixtures/outline_probe.cpp` over the real td::get_outline_svg_path.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin outline_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/outline_probe.exe <script.txt>
//
// The script carries the SVG *coordinate stream* (no leading M, no trailing z):
// the C++ function prepends 'M' and appends 'z' to the compact path itself.
// Both sides encode the stream into the compact form first (chars from the
// table, spaces turned into commas), exactly like the JNI layer does.

use std::io::Write;

const TABLE: &[u8; 64] = b"AACAAAAHAAALMAAAQASTAVAAAZaacaaaahaaalmaaaqastava.az0123456789-,";

fn table_index(c: u8) -> Option<u8> {
    TABLE.iter().position(|&t| t == c).map(|index| index as u8)
}

fn encode(svg: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(svg.len());
    for &raw in svg {
        let c = if raw == b' ' { b',' } else { raw };
        let index = table_index(c).unwrap_or_else(|| panic!("character {} is not in the compact table", c as char));
        encoded.push(192 + index);
    }
    encoded
}

fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

fn is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

struct Parser<'a> {
    path: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn byte(&self) -> u8 {
        *self.path.get(self.pos).unwrap_or(&0)
    }

    fn skip_commas(&mut self) {
        while self.byte() == b',' {
            self.pos += 1;
        }
    }

    fn get_number(&mut self) -> f64 {
        self.skip_commas();
        let mut sign = 1.0;
        if self.byte() == b'-' {
            sign = -1.0;
            self.pos += 1;
        }
        let mut res = 0.0f64;
        while is_digit(self.byte()) {
            res = res * 10.0 + (self.byte() - b'0') as f64;
            self.pos += 1;
        }
        if self.byte() == b'.' {
            self.pos += 1;
            let mut mul = 0.1f64;
            while is_digit(self.byte()) {
                res += (self.byte() - b'0') as f64 * mul;
                mul *= 0.1;
                self.pos += 1;
            }
        }
        sign * res
    }
}

fn decode(encoded: &[u8]) -> Vec<u8> {
    let mut path = Vec::with_capacity(encoded.len() + 2);
    path.push(b'M');
    for &c in encoded {
        if c >= 128 + 64 {
            path.push(TABLE[(c - 128 - 64) as usize]);
        } else {
            if c >= 128 {
                path.push(b',');
            } else if c >= 64 {
                path.push(b'-');
            }
            path.push(c & 63);
        }
    }
    path.push(b'z');
    path
}

fn outline_svg_path(encoded: &[u8], zoom: f64) -> String {
    if encoded.is_empty() {
        return String::new();
    }
    let path = decode(encoded);
    let mut parser = Parser { path: &path, pos: 0 };
    let mut result = String::new();

    let make_point = |x: f64, y: f64, result: &mut String| {
        result.push_str(&format!(" {} {}", (x * zoom) as i32, (y * zoom) as i32));
    };

    let mut x = 0.0f64;
    let mut y = 0.0f64;
    while parser.byte() != 0 {
        parser.skip_commas();
        if parser.byte() == 0 {
            break;
        }

        while parser.byte() == b'm' || parser.byte() == b'M' {
            let command = parser.byte();
            parser.pos += 1;
            loop {
                if command == b'm' {
                    x += parser.get_number();
                    y += parser.get_number();
                } else {
                    x = parser.get_number();
                    y = parser.get_number();
                }
                parser.skip_commas();
                if parser.byte() == 0 || is_alpha(parser.byte()) {
                    break;
                }
            }
        }
        result.push('M');
        make_point(x, y, &mut result);

        let start_x = x;
        let start_y = y;
        let mut have_last_end_control_point = false;
        let mut last_end_control_point_x = 0.0f64;
        let mut last_end_control_point_y = 0.0f64;
        let mut is_closed = false;
        let mut command = b'-';
        while !is_closed {
            parser.skip_commas();
            if parser.byte() == 0 {
                // "Receive unclosed path": the real function logs and returns "".
                return String::new();
            }
            if is_alpha(parser.byte()) {
                command = parser.byte();
                parser.pos += 1;
            }
            match command {
                b'l' | b'L' | b'h' | b'H' | b'v' | b'V' => {
                    if command == b'l' || command == b'h' {
                        x += parser.get_number();
                    } else if command == b'L' || command == b'H' {
                        x = parser.get_number();
                    }
                    if command == b'l' || command == b'v' {
                        y += parser.get_number();
                    } else if command == b'L' || command == b'V' {
                        y = parser.get_number();
                    }
                    result.push_str(" L");
                    make_point(x, y, &mut result);
                    have_last_end_control_point = false;
                }
                b'C' | b'c' | b'S' | b's' => {
                    let (start_control_point_x, start_control_point_y);
                    if command == b'S' || command == b's' {
                        if have_last_end_control_point {
                            start_control_point_x = 2.0 * x - last_end_control_point_x;
                            start_control_point_y = 2.0 * y - last_end_control_point_y;
                        } else {
                            start_control_point_x = x;
                            start_control_point_y = y;
                        }
                    } else {
                        let mut sx = parser.get_number();
                        let mut sy = parser.get_number();
                        if command == b'c' {
                            sx += x;
                            sy += y;
                        }
                        start_control_point_x = sx;
                        start_control_point_y = sy;
                    }

                    let mut ex = parser.get_number();
                    let mut ey = parser.get_number();
                    if command == b'c' || command == b's' {
                        ex += x;
                        ey += y;
                    }
                    last_end_control_point_x = ex;
                    last_end_control_point_y = ey;
                    have_last_end_control_point = true;

                    if command == b'c' || command == b's' {
                        x += parser.get_number();
                        y += parser.get_number();
                    } else {
                        x = parser.get_number();
                        y = parser.get_number();
                    }

                    result.push_str(" C");
                    make_point(start_control_point_x, start_control_point_y, &mut result);
                    make_point(last_end_control_point_x, last_end_control_point_y, &mut result);
                    make_point(x, y, &mut result);
                }
                b'm' | b'M' => {
                    parser.pos -= 1;
                    if x != start_x || y != start_y {
                        x = start_x;
                        y = start_y;
                        result.push_str(" L");
                        make_point(x, y, &mut result);
                    }
                    is_closed = true;
                }
                b'z' | b'Z' => {
                    if x != start_x || y != start_y {
                        x = start_x;
                        y = start_y;
                        result.push_str(" L");
                        make_point(x, y, &mut result);
                    }
                    is_closed = true;
                }
                _ => {
                    // "Receive invalid command": the real function logs and returns "".
                    return String::new();
                }
            }
        }
    }
    result.push('z');
    result
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: outline_probe <script.txt>");
        std::process::exit(2);
    }
    let script = std::fs::read(&args[1]).expect("cannot read script");
    let mut out = std::io::stdout();
    for raw_line in script.split(|c| *c == b'\n') {
        let mut line: &[u8] = raw_line;
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let mut parts = line.splitn(3, |c| *c == b' ');
        let tag = parts.next().unwrap_or(b"");
        let zoom_token = parts.next().unwrap_or(b"");
        let svg = parts.next().unwrap_or(b"");
        if tag != b"path" || zoom_token.is_empty() {
            eprintln!("bad line");
            std::process::exit(2);
        }
        let zoom: f64 = std::str::from_utf8(zoom_token).unwrap().parse().unwrap();
        let encoded = encode(svg);
        let result = outline_svg_path(&encoded, zoom);
        let shown = if result.is_empty() { "-".to_string() } else { result };
        let _ = writeln!(
            out,
            "path zoom={} out={}",
            std::str::from_utf8(zoom_token).unwrap(),
            shown
        );
    }
}