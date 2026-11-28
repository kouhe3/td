// (Unofficial) Rust side of the `media_geometry` lane; the oracle is
// `tools/fixtures/media_geometry_probe.cpp` over the real `td::Dimensions` and
// `td::MediaAreaCoordinates`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin media_geometry_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/media_geometry_probe.exe <script.txt>
//        ... --check obj_invalid | input_invalid
//
// The narration carries the 64-bit pattern of every double that reaches the
// object, so clamping and normalisation stay exact even where the printed form
// rounds to six decimals.
//
// Rules this side reproduces (docs/facts.md F239):
//   * `get_dimensions` clamps each side to `[0, 65535]` and zeroes **both** when
//     either side is 0;
//   * `MediaAreaCoordinates::init` clamps `x/y/width/height/radius` to
//     `[0, 100]`, clamps `rotation` to `[-360, 360]` and then adds 360 when it
//     is negative, and maps every non-finite double to 0 first;
//   * `is_valid()` is `width > 0 && height > 0` only, and both accessors CHECK it;
//   * the TL constructor drops the incoming flags, and the input getter
//     recomputes them from `radius > 0`;
//   * `operator==` compares each field with `|delta| < 1e-6`.

use std::io::Write;

#[allow(dead_code)]
#[path = "../script_tokens.rs"]
mod script_tokens;

use script_tokens::{split_tokens, trim_line};

struct Dimensions {
    width: u16,
    height: u16,
}

fn get_dimension(size: i32) -> u16 {
    if size < 0 || size > 65535 {
        // "Wrong image dimension": the object keeps 0.
        return 0;
    }
    size as u16
}

fn get_dimensions(width: i32, height: i32) -> Dimensions {
    let mut result = Dimensions { width: get_dimension(width), height: get_dimension(height) };
    if result.width == 0 || result.height == 0 {
        result.width = 0;
        result.height = 0;
    }
    result
}

fn pixel_count(dimensions: &Dimensions) -> u32 {
    u32::from(dimensions.width) * u32::from(dimensions.height)
}

struct Coordinates {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    rotation: f64,
    radius: f64,
}

fn fix(value: f64, min_value: f64, max_value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(min_value, max_value)
}

impl Coordinates {
    fn init(x: f64, y: f64, width: f64, height: f64, rotation: f64, radius: f64) -> Coordinates {
        let mut rotation = fix(rotation, -360.0, 360.0);
        if rotation < 0.0 {
            rotation += 360.0;
        }
        Coordinates {
            x: fix(x, 0.0, 100.0),
            y: fix(y, 0.0, 100.0),
            width: fix(width, 0.0, 100.0),
            height: fix(height, 0.0, 100.0),
            rotation,
            radius: fix(radius, 0.0, 100.0),
        }
    }

    fn is_valid(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }

    fn render(&self) -> String {
        format!(
            "AreaPosition[{:.6}, {:.6}, {:.6}, {:.6}, {:.6}, {:.6}]",
            self.x, self.y, self.width, self.height, self.rotation, self.radius
        )
    }

    fn json(&self) -> String {
        format!(
            "{{\"@type\":\"areaPosition\",\"x_percentage\":{:.6},\"y_percentage\":{:.6},\"width_percentage\":{:.6},\"height_percentage\":{:.6},\"rotation_angle\":{:.6},\"corner_radius_percentage\":{:.6}}}",
            self.x, self.y, self.width, self.height, self.rotation, self.radius
        )
    }

    /// `get_input_media_area_coordinates()`: RADIUS_MASK (bit 0) is recomputed
    /// from `radius > 0`, the incoming flags are gone by then.
    fn input_flags(&self) -> i32 {
        if self.radius > 0.0 { 1 } else { 0 }
    }

    /// `operator==`: every field within 1e-6, not exact equality.
    fn equal(&self, other: &Coordinates) -> bool {
        (self.x - other.x).abs() < 1e-6
            && (self.y - other.y).abs() < 1e-6
            && (self.width - other.width).abs() < 1e-6
            && (self.height - other.height).abs() < 1e-6
            && (self.rotation - other.rotation).abs() < 1e-6
            && (self.radius - other.radius).abs() < 1e-6
    }
}

fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

fn bits_of(values: [f64; 6]) -> String {
    let parts: Vec<String> = values.iter().map(|value| bits(*value)).collect();
    parts.join(" ")
}

/// The C++ prints the **inputs** of the object, not the normalised fields, so
/// the narration shows both the clamped result and what came in.
fn print_coordinates(coordinates: &Coordinates, input: [f64; 6], out: &mut std::io::Stdout) {
    let _ = writeln!(
        out,
        "area in=[{}] valid={} print={}",
        bits_of(input),
        coordinates.is_valid() as u8,
        coordinates.render()
    );
    if coordinates.is_valid() {
        let _ = writeln!(out, "  json={}", coordinates.json());
        let _ = writeln!(
            out,
            "  input flags={} bits=[{}]",
            coordinates.input_flags(),
            bits_of([
                coordinates.x,
                coordinates.y,
                coordinates.width,
                coordinates.height,
                coordinates.rotation,
                coordinates.radius
            ])
        );
    }
}

fn parse_double(token: &[u8]) -> f64 {
    match token {
        b"nan" => f64::NAN,
        b"inf" => f64::INFINITY,
        b"-inf" => f64::NEG_INFINITY,
        _ => std::str::from_utf8(token).unwrap().parse().unwrap(),
    }
}

fn parse_int(token: &[u8]) -> i32 {
    std::str::from_utf8(token).unwrap().parse().unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    if args.len() == 3 && args[1] == "--check" {
        let coordinates = Coordinates::init(10.0, 20.0, 0.0, 40.0, 0.0, 0.0);
        let _ = writeln!(out, "before valid={}", coordinates.is_valid() as u8);
        out.flush().unwrap();
        match args[2].as_str() {
            "obj_invalid" => {
                assert!(coordinates.is_valid(), "get_area_position_object CHECK(is_valid())");
                let _ = writeln!(out, "after={}", coordinates.json());
            }
            "input_invalid" => {
                assert!(coordinates.is_valid(), "get_input_media_area_coordinates CHECK(is_valid())");
                let _ = writeln!(out, "after flags={}", coordinates.input_flags());
            }
            _ => {
                eprintln!("unknown check case");
                std::process::exit(2);
            }
        }
        out.flush().unwrap();
        return;
    }
    if args.len() != 2 {
        eprintln!("usage: media_geometry_probe <script.txt>");
        std::process::exit(2);
    }

    let script = std::fs::read(&args[1]).expect("cannot read script");
    for raw_line in script.split(|c| *c == b'\n') {
        let line = trim_line(raw_line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let tokens = split_tokens(line);
        match tokens[0] {
            b"dim" => {
                let dimensions = get_dimensions(parse_int(tokens[1]), parse_int(tokens[2]));
                let _ = writeln!(
                    out,
                    "dim w={} h={} px={} print=({}, {})",
                    dimensions.width,
                    dimensions.height,
                    pixel_count(&dimensions),
                    dimensions.width,
                    dimensions.height
                );
            }
            b"dimcmp" => {
                let lhs = get_dimensions(parse_int(tokens[1]), parse_int(tokens[2]));
                let rhs = get_dimensions(parse_int(tokens[3]), parse_int(tokens[4]));
                let equal = lhs.width == rhs.width && lhs.height == rhs.height;
                let _ = writeln!(out, "dimcmp eq={} ne={}", equal as u8, !equal as u8);
            }
            b"area" => {
                let input = [
                    parse_double(tokens[1]),
                    parse_double(tokens[2]),
                    parse_double(tokens[3]),
                    parse_double(tokens[4]),
                    parse_double(tokens[5]),
                    parse_double(tokens[6]),
                ];
                let coordinates =
                    Coordinates::init(input[0], input[1], input[2], input[3], input[4], input[5]);
                print_coordinates(&coordinates, input, &mut out);
            }
            b"tarea" => {
                // The TL constructor ignores the flags; whatever the radius says
                // is recomputed on the way out.
                let _flags = parse_int(tokens[1]);
                let input = [
                    parse_double(tokens[2]),
                    parse_double(tokens[3]),
                    parse_double(tokens[4]),
                    parse_double(tokens[5]),
                    parse_double(tokens[6]),
                    parse_double(tokens[7]),
                ];
                let coordinates =
                    Coordinates::init(input[0], input[1], input[2], input[3], input[4], input[5]);
                print_coordinates(&coordinates, input, &mut out);
            }
            b"eq" => {
                let lhs = Coordinates::init(
                    parse_double(tokens[1]),
                    parse_double(tokens[2]),
                    parse_double(tokens[3]),
                    parse_double(tokens[4]),
                    parse_double(tokens[5]),
                    parse_double(tokens[6]),
                );
                let rhs = Coordinates::init(
                    parse_double(tokens[7]),
                    parse_double(tokens[8]),
                    parse_double(tokens[9]),
                    parse_double(tokens[10]),
                    parse_double(tokens[11]),
                    parse_double(tokens[12]),
                );
                let equal = lhs.equal(&rhs);
                let _ = writeln!(out, "eq eq={} ne={}", equal as u8, !equal as u8);
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}