// (Unofficial) Rust side of the `message_search_filter` lane; the oracle is
// `tools/fixtures/message_search_filter_probe.cpp` over the real
// `td/telegram/MessageSearchFilter.{h,cpp}`.
//
// Build: cargo build --release --target x86_64-pc-windows-gnu --bin message_search_filter_probe
// Run:   rust/target/x86_64-pc-windows-gnu/release/message_search_filter_probe.exe <script.txt>
//        ... --check print_size | index_empty | callindex_none |
//            input_unread_mention | input_failed_to_send |
//            input_unread_reaction | input_unread_poll_vote
//
// Rules this side reproduces (docs/facts.md F259):
//   * `Size` is 21, so there are 20 filters and `index_mask` tops out at
//     `1 << 19` = 524288; `Empty` is the only mask-0 entry and has no index;
//   * `Call`/`MissedCall` are the two phone-call filters and share one TL
//     constructor with a different `missed` bit;
//   * `UnreadMention`, `FailedToSend`, `UnreadReaction` and `UnreadPollVote`
//     have no TL input filter at all, and `operator<<` on `Size` is a bug.

use std::io::Write;

#[allow(dead_code)]
#[path = "../script_tokens.rs"]
mod script_tokens;

use script_tokens::{split_tokens, trim_line};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    Empty,
    Animation,
    Audio,
    Document,
    Photo,
    Video,
    VoiceNote,
    PhotoAndVideo,
    Url,
    ChatPhoto,
    Call,
    MissedCall,
    VideoNote,
    VoiceAndVideoNote,
    Mention,
    UnreadMention,
    FailedToSend,
    Pinned,
    UnreadReaction,
    Poll,
    UnreadPollVote,
}

const FILTERS: [Filter; 20] = [
    Filter::Animation,
    Filter::Audio,
    Filter::Document,
    Filter::Photo,
    Filter::Video,
    Filter::VoiceNote,
    Filter::PhotoAndVideo,
    Filter::Url,
    Filter::ChatPhoto,
    Filter::Call,
    Filter::MissedCall,
    Filter::VideoNote,
    Filter::VoiceAndVideoNote,
    Filter::Mention,
    Filter::UnreadMention,
    Filter::FailedToSend,
    Filter::Pinned,
    Filter::UnreadReaction,
    Filter::Poll,
    Filter::UnreadPollVote,
];

impl Filter {
    fn from_int(value: i32) -> Filter {
        if value == 0 {
            return Filter::Empty;
        }
        FILTERS[(value - 1) as usize]
    }

    fn name(self) -> &'static str {
        match self {
            Filter::Empty => "Empty",
            Filter::Animation => "Animation",
            Filter::Audio => "Audio",
            Filter::Document => "Document",
            Filter::Photo => "Photo",
            Filter::Video => "Video",
            Filter::VoiceNote => "VoiceNote",
            Filter::PhotoAndVideo => "PhotoAndVideo",
            Filter::Url => "Url",
            Filter::ChatPhoto => "ChatPhoto",
            Filter::Call => "Call",
            Filter::MissedCall => "MissedCall",
            Filter::VideoNote => "VideoNote",
            Filter::VoiceAndVideoNote => "VoiceAndVideoNote",
            Filter::Mention => "Mention",
            Filter::UnreadMention => "UnreadMention",
            Filter::FailedToSend => "FailedToSend",
            Filter::Pinned => "Pinned",
            Filter::UnreadReaction => "UnreadReaction",
            Filter::Poll => "Poll",
            Filter::UnreadPollVote => "UnreadPollVote",
        }
    }

    fn index(self) -> i32 {
        if self == Filter::Empty {
            panic!("message_search_filter_index: CHECK(filter != Empty)");
        }
        FILTERS.iter().position(|f| *f == self).unwrap() as i32
    }

    fn mask(self) -> i32 {
        if self == Filter::Empty {
            return 0;
        }
        1 << self.index()
    }

    fn call_index(self) -> i32 {
        match self {
            Filter::Call => 0,
            Filter::MissedCall => 1,
            _ => panic!("call_message_search_filter_index: CHECK(is call filter)"),
        }
    }

    /// `get_input_messages_filter`: four filters have no TL form.
    fn input(self) -> Input {
        match self {
            Filter::Empty => Input::Id(1474492012),
            Filter::Animation => Input::Id(-3644025),
            Filter::Audio => Input::Id(928101534),
            Filter::Document => Input::Id(-1629621880),
            Filter::Photo => Input::Id(-1777752804),
            Filter::Video => Input::Id(-1614803355),
            Filter::VoiceNote => Input::Id(1358283666),
            Filter::PhotoAndVideo => Input::Id(1458172132),
            Filter::Url => Input::Id(2129714567),
            Filter::ChatPhoto => Input::Id(975236280),
            Filter::Call => Input::Calls { missed: false },
            Filter::MissedCall => Input::Calls { missed: true },
            Filter::VideoNote => Input::Id(-1253451181),
            Filter::VoiceAndVideoNote => Input::Id(2054952868),
            Filter::Mention => Input::Id(-1040652646),
            Filter::Pinned => Input::Id(464520273),
            Filter::Poll => Input::Id(-97793782),
            Filter::UnreadMention | Filter::FailedToSend | Filter::UnreadReaction | Filter::UnreadPollVote => {
                panic!("get_input_messages_filter: UNREACHABLE")
            }
        }
    }
}

enum Input {
    Id(i32),
    Calls { missed: bool },
}

/// `get_message_search_filter` over the 19 `td_api` classes (there is no
/// Call/MissedCall class) plus the null pointer.
fn from_td_api(name: &[u8]) -> Filter {
    match name {
        b"empty" => Filter::Empty,
        b"animation" => Filter::Animation,
        b"audio" => Filter::Audio,
        b"document" => Filter::Document,
        b"photo" => Filter::Photo,
        b"poll" => Filter::Poll,
        b"video" => Filter::Video,
        b"voicenote" => Filter::VoiceNote,
        b"photoandvideo" => Filter::PhotoAndVideo,
        b"url" => Filter::Url,
        b"chatphoto" => Filter::ChatPhoto,
        b"videonote" => Filter::VideoNote,
        b"voiceandvideonote" => Filter::VoiceAndVideoNote,
        b"mention" => Filter::Mention,
        b"unreadmention" => Filter::UnreadMention,
        b"unreadreaction" => Filter::UnreadReaction,
        b"unreadpollvote" => Filter::UnreadPollVote,
        b"failedtosend" => Filter::FailedToSend,
        b"pinned" => Filter::Pinned,
        other => panic!("bad td_api filter name {}", String::from_utf8_lossy(other)),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    if args.len() == 3 && args[1] == "--check" {
        let _ = writeln!(out, "before");
        out.flush().unwrap();
        match args[2].as_str() {
            "print_size" => panic!("operator<<(Size): UNREACHABLE"),
            "index_empty" => {
                let _ = writeln!(out, "after={}", Filter::Empty.index());
            }
            "callindex_none" => {
                let _ = writeln!(out, "after={}", Filter::Photo.call_index());
            }
            "input_unread_mention" => {
                let _ = writeln!(out, "after id={}", match Filter::UnreadMention.input() { Input::Id(id) => id, _ => 0 });
            }
            "input_failed_to_send" => {
                let _ = writeln!(out, "after id={}", match Filter::FailedToSend.input() { Input::Id(id) => id, _ => 0 });
            }
            "input_unread_reaction" => {
                let _ = writeln!(out, "after id={}", match Filter::UnreadReaction.input() { Input::Id(id) => id, _ => 0 });
            }
            "input_unread_poll_vote" => {
                let _ = writeln!(out, "after id={}", match Filter::UnreadPollVote.input() { Input::Id(id) => id, _ => 0 });
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
        eprintln!("usage: message_search_filter_probe <script.txt>");
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
            b"count" => {
                let _ = writeln!(out, "count={}", FILTERS.len());
            }
            b"print" => {
                let value: i32 = std::str::from_utf8(tokens[1]).unwrap().parse().unwrap();
                let _ = writeln!(out, "print={}", Filter::from_int(value).name());
            }
            b"index" => {
                let value: i32 = std::str::from_utf8(tokens[1]).unwrap().parse().unwrap();
                let _ = writeln!(out, "index={}", Filter::from_int(value).index());
            }
            b"mask" => {
                let value: i32 = std::str::from_utf8(tokens[1]).unwrap().parse().unwrap();
                let _ = writeln!(out, "mask={}", Filter::from_int(value).mask());
            }
            b"callindex" => {
                let value: i32 = std::str::from_utf8(tokens[1]).unwrap().parse().unwrap();
                let _ = writeln!(out, "callindex={}", Filter::from_int(value).call_index());
            }
            b"input" => {
                let value: i32 = std::str::from_utf8(tokens[1]).unwrap().parse().unwrap();
                match Filter::from_int(value).input() {
                    Input::Id(id) => {
                        let _ = writeln!(out, "input=id={}", id);
                    }
                    Input::Calls { missed } => {
                        let _ = writeln!(out, "input=calls silent={}", missed as u8);
                    }
                }
            }
            b"tdapi" => {
                let _ = writeln!(out, "tdapi={}", from_td_api(tokens[1]).name());
            }
            b"tdapinull" => {
                // A null `SearchMessagesFilter` is `Empty`.
                let _ = writeln!(out, "tdapinull={}", Filter::Empty.name());
            }
            other => {
                eprintln!("unknown command {}", String::from_utf8_lossy(other));
                std::process::exit(2);
            }
        }
    }
}