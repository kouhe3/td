// (Unofficial) Rust side of the `dialog_photo` lane; the oracle is
// `tools/fixtures/dialog_photo_probe.cpp` over the FileManager-free half of
// `td/telegram/DialogPhoto.{h,cpp}` plus `need_update_dialog_photo_minithumbnail`
// from `td/telegram/PhotoSize.cpp`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin dialog_photo_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/dialog_photo_probe.exe <script.txt>
//
// Rules this side reproduces (docs/facts.md F270):
//   * `need_update_dialog_photo` ignores `minithumbnail`, `operator==` compares it,
//     so "equal ids, different minithumbnail" is `need=0` but `eq=0`;
//   * `as_fake_dialog_photo` returns an empty `DialogPhoto` as soon as either the
//     'a' or the 'c' file id is invalid, dropping minithumbnail/animation/personal;
//   * `need_update_dialog_photo_minithumbnail` compares decoded sizes (a leading
//     0x01 gives `max(byte1, byte2)`), not bytes;
//   * `convert_photo_to_profile_photo` needs both an 'a' and a 'c' photoSize and
//     reports `has_video` from the video sizes.
//
// The FileManager half of the header is `blocked` (F270): `FileManager` is final
// and its constructor LOG_CHECKs without a `Td`, so it is not modelled here.

use std::io::Write;

#[allow(dead_code)]
#[path = "../script_tokens.rs"]
mod script_tokens;

use script_tokens::{split_tokens, trim_line};

/// `td::FileId`: only the local id matters for printing, equality and validity.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct FileId(i32);

impl FileId {
    fn is_valid(self) -> bool {
        self.0 > 0
    }
}

impl std::fmt::Display for FileId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}(0)", self.0)
    }
}

#[derive(Clone, Default, PartialEq)]
struct DialogPhoto {
    small_file_id: FileId,
    big_file_id: FileId,
    minithumbnail: Vec<u8>,
    has_animation: bool,
    is_personal: bool,
}

impl std::fmt::Display for DialogPhoto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<small_file_id = {}, big_file_id = {}, has_animation = {}, is_personal = {}>",
            self.small_file_id, self.big_file_id, self.has_animation, self.is_personal
        )
    }
}

#[derive(Clone, Default, PartialEq)]
struct ProfilePhoto {
    photo: DialogPhoto,
    id: i64,
}

impl std::fmt::Display for ProfilePhoto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<ID = {}, small_file_id = {}, big_file_id = {}, has_animation = {}, is_personal = {}>",
            self.id,
            self.photo.small_file_id,
            self.photo.big_file_id,
            self.photo.has_animation,
            self.photo.is_personal
        )
    }
}

/// `td::Photo`, only the fields this lane touches.
#[derive(Clone)]
struct Photo {
    id: i64,
    minithumbnail: Vec<u8>,
    sizes: Vec<(u8, i32)>,
    animations: usize,
}

impl Photo {
    fn is_empty(&self) -> bool {
        self.id == -2
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn hex_decode(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut index = 0;
    while index + 1 < text.len() {
        match (hex_value(text[index]), hex_value(text[index + 1])) {
            (Some(high), Some(low)) => out.push(high << 4 | low),
            _ => break,
        }
        index += 2;
    }
    out
}

fn hex_encode(data: &[u8]) -> String {
    if data.is_empty() {
        return "-".to_string();
    }
    data.iter().map(|byte| format!("{:02x}", byte)).collect()
}

/// One `key=value` field list from a spec token (`s=1,b=2,m=aa`).
struct Fields(Vec<(Vec<u8>, Vec<u8>)>);

impl Fields {
    fn parse(spec: &[u8]) -> Fields {
        let mut fields = Vec::new();
        for item in spec.split(|c| *c == b',') {
            if item.is_empty() {
                continue;
            }
            match item.iter().position(|c| *c == b'=') {
                Some(index) => fields.push((item[..index].to_vec(), item[index + 1..].to_vec())),
                None => fields.push((item.to_vec(), Vec::new())),
            }
        }
        Fields(fields)
    }

    fn raw(&self, key: &[u8]) -> &[u8] {
        for (name, value) in &self.0 {
            if name == key {
                return value;
            }
        }
        b""
    }

    fn int(&self, key: &[u8], fallback: i64) -> i64 {
        let value = self.raw(key);
        if value.is_empty() {
            fallback
        } else {
            std::str::from_utf8(value).unwrap().parse().unwrap()
        }
    }

    fn hex(&self, key: &[u8]) -> Vec<u8> {
        hex_decode(self.raw(key))
    }
}

fn parse_dialog_photo(spec: &[u8]) -> DialogPhoto {
    let fields = Fields::parse(spec);
    DialogPhoto {
        small_file_id: FileId(fields.int(b"s", 0) as i32),
        big_file_id: FileId(fields.int(b"b", 0) as i32),
        minithumbnail: fields.hex(b"m"),
        has_animation: fields.int(b"a", 0) != 0,
        is_personal: fields.int(b"p", 0) != 0,
    }
}

fn parse_profile_photo(spec: &[u8]) -> ProfilePhoto {
    let fields = Fields::parse(spec);
    ProfilePhoto { photo: parse_dialog_photo(spec), id: fields.int(b"id", 0) }
}

fn parse_photo(spec: &[u8]) -> Photo {
    let fields = Fields::parse(spec);
    let mut sizes = Vec::new();
    for item in fields.raw(b"sz").split(|c| *c == b';') {
        if item.len() < 2 {
            continue;
        }
        sizes.push((item[0], std::str::from_utf8(&item[1..]).unwrap().parse().unwrap()));
    }
    Photo {
        id: fields.int(b"id", -2),
        minithumbnail: fields.hex(b"m"),
        sizes,
        animations: fields.int(b"an", 0) as usize,
    }
}

/// `as_fake_dialog_photo`: any invalid side collapses the whole struct.
fn as_fake_dialog_photo(photo: &Photo, is_personal: bool) -> DialogPhoto {
    let mut result = DialogPhoto::default();
    if !photo.is_empty() {
        for &(kind, file_id) in &photo.sizes {
            if kind == b'a' {
                result.small_file_id = FileId(file_id);
            } else if kind == b'c' {
                result.big_file_id = FileId(file_id);
            }
        }
        result.minithumbnail = photo.minithumbnail.clone();
        result.has_animation = photo.animations > 0;
        result.is_personal = is_personal;
        if !result.small_file_id.is_valid() || !result.big_file_id.is_valid() {
            return DialogPhoto::default();
        }
    }
    result
}

fn dialog_photo_get_file_ids(photo: &DialogPhoto) -> Vec<FileId> {
    let mut ids = Vec::new();
    if photo.small_file_id.is_valid() {
        ids.push(photo.small_file_id);
    }
    if photo.big_file_id.is_valid() {
        ids.push(photo.big_file_id);
    }
    ids
}

/// Deliberately does not look at `minithumbnail`.
fn need_update_dialog_photo(from: &DialogPhoto, to: &DialogPhoto) -> bool {
    from.small_file_id != to.small_file_id
        || from.big_file_id != to.big_file_id
        || from.has_animation != to.has_animation
        || from.is_personal != to.is_personal
}

fn need_update_profile_photo(from: &ProfilePhoto, to: &ProfilePhoto) -> bool {
    from.id != to.id || need_update_dialog_photo(&from.photo, &to.photo)
}

fn get_minithumbnail_size(packed: &[u8]) -> i32 {
    if packed.len() < 3 || packed[0] != 0x01 {
        return 0;
    }
    i32::from(packed[1].max(packed[2]))
}

fn need_update_dialog_photo_minithumbnail(from: &[u8], to: &[u8]) -> bool {
    if from == to {
        return false;
    }
    let from_size = get_minithumbnail_size(from);
    let to_size = get_minithumbnail_size(to);
    to_size != 0 && (to_size <= 8 || from_size > 8)
}

fn parse_tphoto(spec: &[u8]) -> (i64, i32, Vec<u8>, usize) {
    let fields = Fields::parse(spec);
    let mut sizes = Vec::new();
    for item in fields.raw(b"sz").split(|c| *c == b';') {
        if let Some(kind) = item.first() {
            sizes.push(*kind);
        }
    }
    (fields.int(b"id", 0), fields.int(b"dc", 0) as i32, sizes, fields.int(b"v", 0) as usize)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    if args.len() != 2 {
        eprintln!("usage: dialog_photo_probe <script.txt>");
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
            b"pid" => {
                let spec = tokens[1];
                let value = if spec == b"none" || spec == b"empty" {
                    0
                } else {
                    Fields::parse(spec).int(b"photo", 0)
                };
                let _ = writeln!(out, "pid={}", value);
            }
            b"fake" => {
                let photo = parse_photo(tokens[1]);
                let is_personal = std::str::from_utf8(tokens[3]).unwrap().parse::<i64>().unwrap() != 0;
                let result = as_fake_dialog_photo(&photo, is_personal);
                let _ = writeln!(
                    out,
                    "fake=s={} b={} mini={} an={} p={}",
                    result.small_file_id.0,
                    result.big_file_id.0,
                    hex_encode(&result.minithumbnail),
                    result.has_animation as u8,
                    result.is_personal as u8
                );
            }
            b"ids" => {
                let photo = parse_photo(tokens[1]);
                let is_personal = std::str::from_utf8(tokens[3]).unwrap().parse::<i64>().unwrap() != 0;
                let ids = dialog_photo_get_file_ids(&as_fake_dialog_photo(&photo, is_personal));
                let mut text = format!("ids={}", ids.len());
                for id in ids {
                    text.push_str(&format!(":{}", id.0));
                }
                let _ = writeln!(out, "{}", text);
            }
            b"need" => {
                let from = parse_dialog_photo(tokens[1]);
                let to = parse_dialog_photo(tokens[2]);
                let _ = writeln!(out, "need={}", need_update_dialog_photo(&from, &to) as u8);
            }
            b"needp" => {
                let from = parse_profile_photo(tokens[1]);
                let to = parse_profile_photo(tokens[2]);
                let _ = writeln!(out, "needp={}", need_update_profile_photo(&from, &to) as u8);
            }
            b"mneed" => {
                let from = hex_decode(tokens[1]);
                let to = hex_decode(tokens[2]);
                let _ = writeln!(
                    out,
                    "mneed={}",
                    need_update_dialog_photo_minithumbnail(&from, &to) as u8
                );
            }
            b"print" => {
                let _ = writeln!(out, "print={}", parse_dialog_photo(tokens[1]));
            }
            b"printp" => {
                let _ = writeln!(out, "printp={}", parse_profile_photo(tokens[1]));
            }
            b"eq" => {
                let lhs = parse_dialog_photo(tokens[1]);
                let rhs = parse_dialog_photo(tokens[2]);
                let equal = lhs == rhs;
                let _ = writeln!(out, "eq={} ne={}", equal as u8, !equal as u8);
            }
            b"eqp" => {
                let lhs = parse_profile_photo(tokens[1]);
                let rhs = parse_profile_photo(tokens[2]);
                let equal = lhs.id == rhs.id && lhs.photo == rhs.photo;
                let _ = writeln!(out, "eqp={} nep={}", equal as u8, !equal as u8);
            }
            b"conv" => {
                let (id, dc, sizes, videos) = parse_tphoto(tokens[1]);
                let is_personal = std::str::from_utf8(tokens[2]).unwrap().parse::<i64>().unwrap() != 0;
                let have_small = sizes.contains(&b'a');
                let have_big = sizes.contains(&b'c');
                if !have_small || !have_big {
                    let _ = writeln!(out, "conv=null");
                } else {
                    let _ = writeln!(
                        out,
                        "conv=flags=0 video={} personal={} photo={} stripped=0 dc={}",
                        (videos > 0) as u8,
                        is_personal as u8,
                        id,
                        dc
                    );
                }
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}
