// (Unofficial) Rust side of the `photo_size_source` lane; the oracle is
// `tools/fixtures/photo_size_source_probe.cpp` over the real
// `td/telegram/PhotoSizeSource.cpp`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin photo_size_source_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/photo_size_source_probe.exe <script.txt>
//        ... --check filetype_full_legacy | name_ss_legacy | unique_thumb_wide | type_default
//
// Rules this side reproduces (docs/facts.md F254):
//   * only eight of the ten variant offsets can be built; a default-constructed
//     source has offset `-1` and aborts on every type-based query;
//   * `get_compare_type` folds thumbnails into `type + 5` with `'a'` -> 0 and
//     `'c'` -> 1, and rejects a thumbnail type outside `0..127`;
//   * `get_unique` is one byte unless the compare type is 2 (13 bytes, with a
//     leading `0x02`) or 3 (12 bytes), both TL little-endian;
//   * `get_unique_name` prints the thumbnail type in decimal and the sticker-set
//     version as `uint32`;
//   * `get_input_peer` drops the access hash for chats and returns null for
//     secret chats.

use std::io::Write;

#[allow(dead_code)]
#[path = "../script_tokens.rs"]
mod script_tokens;

#[allow(dead_code)]
#[path = "../telegram_dialog_id.rs"]
mod telegram_dialog_id;

use script_tokens::{split_tokens, trim_line};
use telegram_dialog_id::{DialogId, DialogType};

#[derive(Clone)]
enum Source {
    Thumbnail { file_type: i32, thumb_type: i32 },
    DialogPhotoSmall { dialog: DialogId, hash: i64 },
    DialogPhotoBig { dialog: DialogId, hash: i64 },
    FullLegacy { volume: i64, local: i32, secret: i64 },
    DialogPhotoSmallLegacy { dialog: DialogId, hash: i64, volume: i64, local: i32 },
    DialogPhotoBigLegacy { dialog: DialogId, hash: i64, volume: i64, local: i32 },
    StickerSetThumbnailLegacy { set_id: i64, set_hash: i64, volume: i64, local: i32 },
    StickerSetThumbnailVersion { set_id: i64, set_hash: i64, version: i32 },
    /// `PhotoSizeSource()`: the Variant holds offset -1.
    Unset,
}

impl Source {
    fn type_index(&self) -> i32 {
        match self {
            Source::Thumbnail { .. } => 1,
            Source::DialogPhotoSmall { .. } => 2,
            Source::DialogPhotoBig { .. } => 3,
            Source::FullLegacy { .. } => 5,
            Source::DialogPhotoSmallLegacy { .. } => 6,
            Source::DialogPhotoBigLegacy { .. } => 7,
            Source::StickerSetThumbnailLegacy { .. } => 8,
            Source::StickerSetThumbnailVersion { .. } => 9,
            Source::Unset => panic!("get_type: LOG_CHECK(offset >= 0)"),
        }
    }

    fn render(&self) -> String {
        match self {
            // `file_type` and the photo size type are printed by their own
            // operators: the size type as a char when it is 'a'..'z'.
            Source::Thumbnail { file_type, thumb_type } => format!(
                "PhotoSizeSourceThumbnail[{}, type = {}]",
                file_type_name(*file_type),
                photo_size_type_name(*thumb_type)
            ),
            Source::DialogPhotoSmall { .. } => "PhotoSizeSourceChatPhotoSmall[]".to_string(),
            Source::DialogPhotoBig { .. } => "PhotoSizeSourceChatPhotoBig[]".to_string(),
            Source::FullLegacy { .. } => "PhotoSizeSourceFullLegacy[]".to_string(),
            Source::DialogPhotoSmallLegacy { .. } => "PhotoSizeSourceChatPhotoSmallLegacy[]".to_string(),
            Source::DialogPhotoBigLegacy { .. } => "PhotoSizeSourceChatPhotoBigLegacy[]".to_string(),
            Source::StickerSetThumbnailLegacy { set_id, .. } => {
                format!("PhotoSizeSourceStickerSetThumbnailLegacy[{}]", set_id)
            }
            Source::StickerSetThumbnailVersion { set_id, version, .. } => {
                format!("PhotoSizeSourceStickerSetThumbnailVersion[{}_{}]", set_id, version)
            }
            Source::Unset => panic!("operator<<: get_type LOG_CHECK(offset >= 0)"),
        }
    }

    fn file_type(&self) -> i32 {
        match self {
            Source::Thumbnail { file_type, .. } => *file_type,
            Source::DialogPhotoSmall { .. }
            | Source::DialogPhotoBig { .. }
            | Source::DialogPhotoSmallLegacy { .. }
            | Source::DialogPhotoBigLegacy { .. } => 1, // FileType::ProfilePhoto
            Source::StickerSetThumbnailLegacy { .. } | Source::StickerSetThumbnailVersion { .. } => 0, // FileType::Thumbnail
            Source::FullLegacy { .. } | Source::Unset => panic!("get_file_type: UNREACHABLE"),
        }
    }

    fn compare_type(&self) -> i32 {
        match self {
            Source::Thumbnail { thumb_type, .. } => {
                let kind = *thumb_type;
                if !(0..=127).contains(&kind) {
                    panic!("get_compare_type: LOG(FATAL) on thumbnail type");
                }
                if kind == i32::from(b'a') {
                    return 0;
                }
                if kind == i32::from(b'c') {
                    return 1;
                }
                kind + 5
            }
            Source::DialogPhotoSmall { .. } => 0,
            Source::DialogPhotoBig { .. } => 1,
            Source::FullLegacy { .. }
            | Source::DialogPhotoSmallLegacy { .. }
            | Source::DialogPhotoBigLegacy { .. }
            | Source::StickerSetThumbnailLegacy { .. } => 3,
            Source::StickerSetThumbnailVersion { .. } => 2,
            Source::Unset => panic!("get_compare_type: get_type LOG_CHECK"),
        }
    }

    fn compare_volume_id(&self) -> i64 {
        match self {
            Source::FullLegacy { volume, .. } => *volume,
            Source::DialogPhotoSmallLegacy { volume, .. } | Source::DialogPhotoBigLegacy { volume, .. } => *volume,
            Source::StickerSetThumbnailLegacy { volume, .. } => *volume,
            Source::StickerSetThumbnailVersion { set_id, .. } => *set_id,
            _ => panic!("get_compare_volume_id: UNREACHABLE"),
        }
    }

    fn compare_local_id(&self) -> i32 {
        match self {
            Source::FullLegacy { local, .. } => *local,
            Source::DialogPhotoSmallLegacy { local, .. } | Source::DialogPhotoBigLegacy { local, .. } => *local,
            Source::StickerSetThumbnailLegacy { local, .. } => *local,
            Source::StickerSetThumbnailVersion { version, .. } => *version,
            _ => panic!("get_compare_local_id: UNREACHABLE"),
        }
    }

    fn unique(&self) -> Vec<u8> {
        let compare_type = self.compare_type();
        if compare_type != 2 && compare_type != 3 {
            return vec![compare_type as u8];
        }
        let mut out = Vec::with_capacity(13);
        if compare_type == 2 {
            out.push(0x02);
        }
        out.extend_from_slice(&self.compare_volume_id().to_le_bytes());
        out.extend_from_slice(&self.compare_local_id().to_le_bytes());
        assert!(out.len() <= 13, "get_unique CHECK(size <= 13)");
        out
    }

    fn unique_less(&self, rhs: &Source) -> bool {
        let lhs_type = self.compare_type();
        let rhs_type = rhs.compare_type();
        if lhs_type != rhs_type {
            return lhs_type < rhs_type;
        }
        if lhs_type != 2 && lhs_type != 3 {
            return false;
        }
        let lhs_volume = self.compare_volume_id();
        let rhs_volume = rhs.compare_volume_id();
        if lhs_volume != rhs_volume {
            return lhs_volume < rhs_volume;
        }
        self.compare_local_id() < rhs.compare_local_id()
    }

    fn unique_equal(&self, rhs: &Source) -> bool {
        let lhs_type = self.compare_type();
        let rhs_type = rhs.compare_type();
        if lhs_type != rhs_type {
            return false;
        }
        if lhs_type != 2 && lhs_type != 3 {
            return true;
        }
        self.compare_volume_id() == rhs.compare_volume_id() && self.compare_local_id() == rhs.compare_local_id()
    }

    fn unique_name(&self, photo_id: i64) -> String {
        match self {
            Source::Thumbnail { thumb_type, .. } => {
                assert!((0..=127).contains(thumb_type), "get_unique_name CHECK(0 <= type && type <= 127)");
                format!("{}_{}", photo_id, thumb_type)
            }
            Source::DialogPhotoSmall { .. } => format!("{}", photo_id),
            Source::DialogPhotoBig { .. } => format!("{}_{}", photo_id, 1),
            Source::StickerSetThumbnailVersion { set_id, version, .. } => {
                format!("{}_{}", set_id, *version as u32)
            }
            _ => panic!("get_unique_name: UNREACHABLE"),
        }
    }

    fn equals(&self, rhs: &Source) -> bool {
        match (self, rhs) {
            (Source::Unset, Source::Unset) => false, // offset -1 never compares equal
            (Source::Unset, _) | (_, Source::Unset) => false,
            (Source::Thumbnail { file_type: lf, thumb_type: lt }, Source::Thumbnail { file_type: rf, thumb_type: rt }) => {
                lf == rf && lt == rt
            }
            (
                Source::DialogPhotoSmall { dialog: ld, hash: lh },
                Source::DialogPhotoSmall { dialog: rd, hash: rh },
            )
            | (Source::DialogPhotoBig { dialog: ld, hash: lh }, Source::DialogPhotoBig { dialog: rd, hash: rh }) => {
                ld.id == rd.id && lh == rh
            }
            (
                Source::StickerSetThumbnailLegacy { set_id: li, set_hash: lh, .. },
                Source::StickerSetThumbnailLegacy { set_id: ri, set_hash: rh, .. },
            ) => li == ri && lh == rh,
            (
                Source::FullLegacy { volume: lv, local: ll, secret: ls },
                Source::FullLegacy { volume: rv, local: rl, secret: rs },
            ) => lv == rv && ll == rl && ls == rs,
            (
                Source::DialogPhotoSmallLegacy { dialog: ld, hash: lh, volume: lv, local: ll },
                Source::DialogPhotoSmallLegacy { dialog: rd, hash: rh, volume: rv, local: rl },
            )
            | (
                Source::DialogPhotoBigLegacy { dialog: ld, hash: lh, volume: lv, local: ll },
                Source::DialogPhotoBigLegacy { dialog: rd, hash: rh, volume: rv, local: rl },
            ) => ld.id == rd.id && lh == rh && lv == rv && ll == rl,
            (
                Source::StickerSetThumbnailVersion { set_id: li, set_hash: lh, version: lv },
                Source::StickerSetThumbnailVersion { set_id: ri, set_hash: rh, version: rv },
            ) => li == ri && lh == rh && lv == rv,
            _ => false,
        }
    }

    fn dialog_photo(&self) -> (&DialogId, i64) {
        match self {
            Source::DialogPhotoSmall { dialog, hash }
            | Source::DialogPhotoBig { dialog, hash }
            | Source::DialogPhotoSmallLegacy { dialog, hash, .. }
            | Source::DialogPhotoBigLegacy { dialog, hash, .. } => (dialog, *hash),
            _ => panic!("dialog_photo(): UNREACHABLE"),
        }
    }

    fn input_sticker_set(&self) -> (i64, i64) {
        match self {
            Source::StickerSetThumbnailLegacy { set_id, set_hash, .. }
            | Source::StickerSetThumbnailVersion { set_id, set_hash, .. } => (*set_id, *set_hash),
            _ => panic!("sticker_set_thumbnail(): UNREACHABLE"),
        }
    }
}

/// StringBuilder's `operator<<` for the two types that appear in the corpus.
fn file_type_name(value: i32) -> String {
    match value {
        0 => "Thumbnail".to_string(),
        1 => "ProfilePhoto".to_string(),
        2 => "Photo".to_string(),
        5 => "Document".to_string(),
        _ => format!("FileType({})", value),
    }
}

fn photo_size_type_name(value: i32) -> String {
    if (i32::from(b'a')..=i32::from(b'z')).contains(&value) {
        return (value as u8 as char).to_string();
    }
    value.to_string()
}

// ------------------------------------------------------------------- parsing

fn parse_i64(token: &[u8]) -> i64 {
    std::str::from_utf8(token).unwrap().parse().unwrap()
}

fn parse_i32(token: &[u8]) -> i32 {
    std::str::from_utf8(token).unwrap().parse().unwrap()
}

fn parse_dialog(token: &[u8]) -> DialogId {
    if token == b"z" || token.is_empty() {
        return DialogId::default();
    }
    match token[0] {
        b'u' => DialogId::from_user(parse_i64(&token[1..])),
        b'c' => DialogId::from_chat(parse_i64(&token[1..])),
        b'n' => DialogId::from_channel(parse_i64(&token[1..])),
        b's' => DialogId::from_secret(parse_i32(&token[1..])),
        other => panic!("bad dialog spec {}", other as char),
    }
}

fn parse_source(spec: &[u8]) -> Source {
    let parts: Vec<&[u8]> = spec.split(|c| *c == b':').collect();
    let kind = parts[0];
    match kind {
        b"l" => Source::Unset,
        b"t" if parts.len() == 3 => Source::Thumbnail {
            file_type: parse_i32(parts[1]),
            thumb_type: parse_i32(parts[2]),
        },
        b"d" if parts.len() == 4 => {
            let dialog = parse_dialog(parts[1]);
            let hash = parse_i64(parts[2]);
            if parts[3] == b"1" {
                Source::DialogPhotoBig { dialog, hash }
            } else {
                Source::DialogPhotoSmall { dialog, hash }
            }
        }
        b"f" if parts.len() == 4 => Source::FullLegacy {
            volume: parse_i64(parts[1]),
            local: parse_i32(parts[2]),
            secret: parse_i64(parts[3]),
        },
        b"dl" if parts.len() == 6 => {
            let dialog = parse_dialog(parts[1]);
            let hash = parse_i64(parts[2]);
            let volume = parse_i64(parts[4]);
            let local = parse_i32(parts[5]);
            if parts[3] == b"1" {
                Source::DialogPhotoBigLegacy { dialog, hash, volume, local }
            } else {
                Source::DialogPhotoSmallLegacy { dialog, hash, volume, local }
            }
        }
        b"sl" if parts.len() == 5 => Source::StickerSetThumbnailLegacy {
            set_id: parse_i64(parts[1]),
            set_hash: parse_i64(parts[2]),
            volume: parse_i64(parts[3]),
            local: parse_i32(parts[4]),
        },
        b"sv" if parts.len() == 4 => Source::StickerSetThumbnailVersion {
            set_id: parse_i64(parts[1]),
            set_hash: parse_i64(parts[2]),
            version: parse_i32(parts[3]),
        },
        _ => panic!("bad source spec"),
    }
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    if args.len() == 3 && args[1] == "--check" {
        let _ = writeln!(out, "before");
        out.flush().unwrap();
        match args[2].as_str() {
            "filetype_full_legacy" => {
                let _ = writeln!(
                    out,
                    "after filetype={}",
                    parse_source(b"f:1:2:3").file_type()
                );
            }
            "name_ss_legacy" => {
                let _ = writeln!(out, "after={}", parse_source(b"sl:1:2:3:4").unique_name(1));
            }
            "unique_thumb_wide" => {
                let _ = writeln!(out, "after={}", hex(&parse_source(b"t:0:200").unique()));
            }
            "type_default" => {
                let _ = writeln!(out, "after type={}", Source::Unset.type_index());
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
        eprintln!("usage: photo_size_source_probe <script.txt>");
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
            b"print" => {
                let source = parse_source(tokens[1]);
                let _ = writeln!(out, "print type={} text={}", source.type_index(), source.render());
            }
            b"filetype" => {
                let source = parse_source(tokens[1]);
                let _ = writeln!(out, "filetype={}", source.file_type());
            }
            b"unique" => {
                let source = parse_source(tokens[1]);
                let unique = source.unique();
                let _ = writeln!(out, "unique len={} hex={}", unique.len(), hex(&unique));
            }
            b"less" => {
                let lhs = parse_source(tokens[1]);
                let rhs = parse_source(tokens[2]);
                let _ = writeln!(out, "less={}", lhs.unique_less(&rhs) as u8);
            }
            b"ueq" => {
                let lhs = parse_source(tokens[1]);
                let rhs = parse_source(tokens[2]);
                let _ = writeln!(out, "ueq={}", lhs.unique_equal(&rhs) as u8);
            }
            b"name" => {
                let photo_id = parse_i64(tokens[1]);
                let source = parse_source(tokens[2]);
                let _ = writeln!(out, "name={}", source.unique_name(photo_id));
            }
            b"eq" => {
                let lhs = parse_source(tokens[1]);
                let rhs = parse_source(tokens[2]);
                let equal = lhs.equals(&rhs);
                let _ = writeln!(out, "eq eq={} ne={}", equal as u8, !equal as u8);
            }
            b"peer" => {
                let source = parse_source(tokens[1]);
                let (dialog, hash) = source.dialog_photo();
                let text = match dialog.get_type() {
                    DialogType::User => format!("peer=user id={} hash={}", dialog.get_user_id(), hash),
                    DialogType::Chat => format!("peer=chat id={}", dialog.get_chat_id()),
                    DialogType::Channel => format!("peer=channel id={} hash={}", dialog.get_channel_id(), hash),
                    DialogType::SecretChat => "peer=null".to_string(),
                    DialogType::None => "peer=empty".to_string(),
                };
                let _ = writeln!(out, "{}", text);
            }
            b"sset" => {
                let source = parse_source(tokens[1]);
                let (set_id, set_hash) = source.input_sticker_set();
                let _ = writeln!(out, "sset id={} hash={}", set_id, set_hash);
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}