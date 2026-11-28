//! `td::Outline` (td/telegram/Outline.cpp): the compact path decoder and both
//! of its faces.
//!
//! `get_outline_svg_path` renders the decoded path back into a normalized SVG
//! string (`int`-truncated zoom, one trailing `z`); `get_outline_object` renders
//! the same decode into `td_api::closedVectorPath` objects (full doubles, no
//! start point, one object per closed subpath, and a subpath without commands is
//! dropped). The C++ has two parsers with identical grammar; this module keeps
//! one parser and two renderers (facts F225/F226, and T109 for the object face).
//!
//! The compact form: `>= 192` indexes the table, `>= 128` inserts `,`, `>= 64`
//! inserts `-`, otherwise the byte is `c & 63`. The decoder prepends `M` and
//! appends `z` itself.

#[allow(dead_code)]
const TABLE: &[u8; 64] = b"AACAAAAHAAALMAAAQASTAVAAAZaacaaaahaaalmaaaqastava.az0123456789-,";

fn table_index(c: u8) -> Option<u8> {
    TABLE.iter().position(|&t| t == c).map(|index| index as u8)
}

pub fn encode(svg: &[u8]) -> Vec<u8> {
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

#[derive(Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub enum PathCommand {
    Line(Point),
    CubicBezier {
        start_control: Point,
        end_control: Point,
        end: Point,
    },
}

pub struct ClosedPath {
    pub start: Point,
    pub commands: Vec<PathCommand>,
}

/// `Empty`, `Unclosed` and `Invalid` all make the C++ return `nullptr` / the
/// empty string; the distinction is kept only for the log messages it prints.
pub enum OutlineError {
    Empty,
    Unclosed,
    Invalid,
}

fn point(x: f64, y: f64, zoom: f64) -> Point {
    Point { x: x * zoom, y: y * zoom }
}

/// The object face: `Ok(vec![])` is an empty (non-null) outline, `Err` is the
/// C++ `nullptr`.
pub fn outline_paths(encoded: &[u8], zoom: f64) -> Result<Vec<ClosedPath>, OutlineError> {
    if encoded.is_empty() {
        return Err(OutlineError::Empty);
    }
    let path = decode(encoded);
    let mut parser = Parser { path: &path, pos: 0 };
    let mut result = Vec::new();

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

        let start_x = x;
        let start_y = y;
        let mut commands = Vec::new();
        let mut have_last_end_control_point = false;
        let mut last_end_control_point_x = 0.0f64;
        let mut last_end_control_point_y = 0.0f64;
        let mut is_closed = false;
        let mut command = b'-';
        while !is_closed {
            parser.skip_commas();
            if parser.byte() == 0 {
                // "Receive unclosed path": the real functions log and give up.
                return Err(OutlineError::Unclosed);
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
                    commands.push(PathCommand::Line(point(x, y, zoom)));
                    have_last_end_control_point = false;
                }
                b'C' | b'c' | b'S' | b's' => {
                    let (start_control_x, start_control_y);
                    if command == b'S' || command == b's' {
                        if have_last_end_control_point {
                            start_control_x = 2.0 * x - last_end_control_point_x;
                            start_control_y = 2.0 * y - last_end_control_point_y;
                        } else {
                            start_control_x = x;
                            start_control_y = y;
                        }
                    } else {
                        let mut sx = parser.get_number();
                        let mut sy = parser.get_number();
                        if command == b'c' {
                            sx += x;
                            sy += y;
                        }
                        start_control_x = sx;
                        start_control_y = sy;
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

                    commands.push(PathCommand::CubicBezier {
                        start_control: point(start_control_x, start_control_y, zoom),
                        end_control: point(last_end_control_point_x, last_end_control_point_y, zoom),
                        end: point(x, y, zoom),
                    });
                }
                b'm' | b'M' | b'z' | b'Z' => {
                    if command == b'm' || command == b'M' {
                        // The byte belongs to the next round's `m/M` run.
                        parser.pos -= 1;
                    }
                    if x != start_x || y != start_y {
                        x = start_x;
                        y = start_y;
                        commands.push(PathCommand::Line(point(x, y, zoom)));
                    }
                    is_closed = true;
                }
                _ => {
                    // "Receive invalid command": the real functions log and give up.
                    return Err(OutlineError::Invalid);
                }
            }
        }
        result.push(ClosedPath { start: point(start_x, start_y, zoom), commands });
    }
    Ok(result)
}

/// The string face: `""` is what the C++ returns for an empty path, an unclosed
/// subpath and an invalid command.
pub fn outline_svg_path(encoded: &[u8], zoom: f64) -> String {
    let paths = match outline_paths(encoded, zoom) {
        Ok(paths) => paths,
        Err(_) => return String::new(),
    };
    if paths.is_empty() {
        return String::new();
    }
    let mut result = String::new();
    let push = |p: Point, out: &mut String| {
        out.push_str(&format!(" {} {}", p.x as i32, p.y as i32));
    };
    for path in &paths {
        result.push('M');
        push(path.start, &mut result);
        for command in &path.commands {
            match command {
                PathCommand::Line(p) => {
                    result.push_str(" L");
                    push(*p, &mut result);
                }
                PathCommand::CubicBezier { start_control, end_control, end } => {
                    result.push_str(" C");
                    push(*start_control, &mut result);
                    push(*end_control, &mut result);
                    push(*end, &mut result);
                }
            }
        }
    }
    result.push('z');
    result
}