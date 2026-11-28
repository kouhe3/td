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
//
// The decoder lives in `telegram_outline.rs` because the `td_api_objects` lane
// drives `get_outline_object()` on top of the same parser.

use std::io::Write;

#[allow(dead_code)]
#[path = "../telegram_outline.rs"]
mod telegram_outline;

use telegram_outline::*;

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