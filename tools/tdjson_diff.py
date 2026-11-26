"""Same-process differential harness: C++ libtdjson.dll vs Rust tdjson_rs.dll over the C ABI.

命令:
    cargo build --release --target x86_64-pc-windows-gnu --manifest-path rust/Cargo.toml
    python tools/tdjson_diff.py [--skip-md5]

判据:
    退出码 0 = MATCH 组逐字节相同 且 CONTROL 组判为不同 且 floor 段稳定；
    任一不符即退出码 1，并打印两侧原文。两侧输出都为空的 case 不算证据。

覆盖边界:
    只覆盖 `td_execute` 这一条纯函数路径（不登录、不联网）。
    覆盖不到的：`td_send` / `td_receive` / `td_json_client_*` 的有状态客户端路径；
    `tddb` / `td/mtproto` / `tdactor` / `tdutils` 在 C ABI 之下，不在本仪器范围内；
    非 ASCII 文本（emoji/中文）的 UTF-16 偏移尚未入 corpus（F007 待证）。

失效条件:
    以下任一变化都必须重判 gate 并更新 docs/facts.md：
      - oracle 的 md5 不等于 EXPECTED_CPP_MD5（换了构建、改了源码、改了编译器）；
      - Rust 侧换了 target / linker（C ABI 兼容性重新验证）；
      - `td_api.tl` 变更导致 action/object 序列化格式变化。

CONTROL 组的纪律:
    CONTROL 只能放**长期不实现**的动作。它靠「Rust 未实现」制造差异，一旦该动作被实现，
    这个 case 必然转为相同，`failures=0` 与「实现正确」就自相矛盾——把它移进 MATCH，
    并换一个仍未实现的动作进 CONTROL。`getOption` 与 `searchStringsByPrefix` 各踩过一次。
"""

from __future__ import annotations

import ctypes
import hashlib
import sys
from pathlib import Path

CPP = Path("C:/tmp/td-build/libtdjson.dll")
RS = Path(__file__).resolve().parents[1] / "rust/target/x86_64-pc-windows-gnu/release/tdjson_rs.dll"
EXPECTED_CPP_MD5 = "0398814038978aaeef7f8d6ac62bb8ff"

MATCH = [
    '{"@type":"getTextEntities","text":"@abc @a #bd"}',
    '{"@type":"getTextEntities","text":"plain"}',
    '{"@type":"getTextEntities","text":"#one #two @abcd"}',
    '{"@type":"getTextEntities","text":"@a"}',
    '{"@type":"getTextEntities","text":"\\ud83d\\ude00 @abcd"}',
    '{"@type":"getTextEntities","text":"\\u4e2d\\u6587 #\\u6807\\u7b7e"}',
    '{"@type":"getTextEntities","text":"#abc\\ud83d\\ude00"}',
    '{"@type":"getTextEntities","text":"$AAPL and $btc"}',
    '{"@type":"getTextEntities","text":"/start@somebot and /help"}',
    '{"@type":"getTextEntities","text":"4111111111111111"}',
    '{"@type":"getTextEntities","text":"4276 3800 1234 5678"}',
    '{"@type":"getTextEntities","text":"watch 01:23:45 now"}',
    '{"@type":"getTextEntities","text":"at 1:23 ok"}',
    '{"@type":"getTextEntities","text":"#sometag@somebot"}',
    '{"@type":"getTextEntities","text":"see https://example.com/path?x=1 ok"}',
    '{"@type":"getTextEntities","text":"tg://resolve?domain=durov"}',
    '{"@type":"getTextEntities","text":"visit www.example.com now"}',
    '{"@type":"getTextEntities","text":"mail me at a@b.com"}',
    '{"@type":"getTextEntities","text":"mailto:a@b.com"}',
    '{"@type":"getTextEntities","text":"@abcd #tag https://x.com a@b.com $USD"}',
    '{"@type":"getTextEntities","text":"pay to EQAAAQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHx2j now"}',
    '{"@type":"getTextEntities","text":"pay to UQAAAQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eH0Bm now"}',
    '{"@type":"getTextEntities","text":"#b# mailto:"}',
    '{"@type":"getTextEntities","text":"#' + 'a' * 300 + '"}',
    '{"@type":"checkQuickReplyShortcutName","name":"my_shortcut"}',
    '{"@type":"checkQuickReplyShortcutName","name":"my shortcut"}',
    '{"@type":"checkQuickReplyShortcutName","name":""}',
    '{"@type":"checkQuickReplyShortcutName","name":"' + 'a' * 33 + '"}',
    '{"@type":"getCountryFlagEmoji","country_code":"US"}',
    '{"@type":"getCountryFlagEmoji","country_code":"yl"}',
    '{"@type":"getCountryFlagEmoji","country_code":"U1"}',
    '{"@type":"testReturnError","error":{"@type":"error","code":400,"message":"boom \\"quoted\\" \\u4f60\\u597d"}}',
    '{"@type":"testReturnError"}',
    '{"@type":"getFileMimeType","file_name":"photo.JPG"}',
    '{"@type":"getFileMimeType","file_name":"archive.tar.gz"}',
    '{"@type":"getFileMimeType","file_name":".gitignore"}',
    '{"@type":"getFileExtension","mime_type":"image/jpeg"}',
    '{"@type":"getFileExtension","mime_type":"video/mp4"}',
    '{"@type":"getFileExtension","mime_type":"application/x-unknown"}',
    '{"@type":"cleanFileName","file_name":"bad<>name?.txt"}',
    '{"@type":"cleanFileName","file_name":"../../etc/passwd"}',
    '{"@type":"cleanFileName","file_name":"\\u00dcn\\u00efcode \\u2026 name.jpg"}',
    '{"@type":"getTextEmojiRanges","text":"hi \\ud83d\\ude00"}',
    '{"@type":"getTextEmojiRanges","text":"\\ud83d\\udc68\\u200d\\ud83d\\udc69\\u200d\\ud83d\\udc67"}',
    '{"@type":"getTextEmojiRanges","text":"1\\ufe0f\\u20e3 x"}',
    '{"@type":"getTextEmojiRanges","text":"\\ud83c\\uddfa\\ud83c\\uddf8"}',
    '{"@type":"getTextEmojiRanges","text":"ok \\ud83d\\udc4d\\ud83c\\udffd go"}',
    '{"@type":"getTextEmojiRanges","text":"\\u2764\\ufe0f"}',
    '{"@type":"getTextEmojiRanges","text":"a\\ud83d\\ude00b\\ud83d\\udc68\\u200d\\ud83d\\udc69\\u200d\\ud83d\\udc67c"}',
    '{"@type":"getJsonValue","json":"{}"}',
    '{"@type":"getJsonValue","json":"[]"}',
    '{"@type":"getJsonValue","json":"{\\"a\\":1,\\"b\\":[true,false,null],\\"c\\":{\\"d\\":\\"x\\ny\\"}}"}',
    '{"@type":"getJsonValue","json":"{\\"n\\": 1.5, \\"big\\": 1234567890123, \\"neg\\": -0.25}"}',
    '{"@type":"getJsonValue","json":"{\\"u\\":\\"\\u4f60\\u597d\\"}"}',
    '{"@type":"getJsonString","json_value":{"@type":"jsonValueNull"}}',
    '{"@type":"getJsonString","json_value":{"@type":"jsonValueBoolean","value":true}}',
    '{"@type":"getJsonString","json_value":{"@type":"jsonValueNumber","value":1.5}}',
    '{"@type":"getJsonString","json_value":{"@type":"jsonValueString","value":"hi \\u4f60"}}',
    '{"@type":"getJsonString","json_value":{"@type":"jsonValueArray","values":[{"@type":"jsonValueNumber","value":2},{"@type":"jsonValueString","value":"a"}]}}',
    '{"@type":"getJsonString","json_value":{"@type":"jsonValueObject","members":[{"@type":"jsonObjectMember","key":"k","value":{"@type":"jsonValueNumber","value":3}}]}}',
    '{"@type":"getThemeParametersJsonString","theme":{"@type":"themeParameters","background_color":1122867,"secondary_background_color":11189196,"header_background_color":65280,"bottom_bar_background_color":1,"section_background_color":16777215,"section_separator_color":0,"text_color":1193046,"accent_text_color":6636321,"section_header_text_color":11259375,"subtitle_text_color":16702650,"destructive_text_color":16711680,"hint_color":8421504,"link_color":255,"button_color":65535,"button_text_color":16776960}}',
    '{"@type":"getThemeParametersJsonString","theme":null}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"include_contacts":true}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"include_groups":true}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"include_channels":true}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"include_bots":true}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"include_groups":true,"include_channels":true,"exclude_read":true}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"include_groups":true,"include_channels":true,"exclude_muted":true}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"pinned_chat_ids":[-1001234567890]}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"icon":{"@type":"chatFolderIcon","name":"Cat"}}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"icon":{"@type":"chatFolderIcon","name":"nature"}}}',
    '{"@type":"getPushReceiverId","payload":"{}"}',
    '{"@type":"getPushReceiverId","payload":"{\\"user_id\\":123}"}',
    '{"@type":"getPushReceiverId","payload":"{\\"p\\":\\"AQEBAQEBAQEB\\"}"}',
    '{"@type":"getPushReceiverId","payload":"{\\"data\\":{\\"user_id\\":7}}"}',
    '{"@type":"getPushReceiverId","payload":"{\\"x\\":1}"}',
    '{"@type":"getPushReceiverId","payload":"not json"}',
    '{"@type":"getOption","name":"version"}',
    '{"@type":"getOption","name":"my_id"}',
    '{"@type":"getOption","name":""}',
    '{"@type":"parseTextEntities","text":"<b>bold</b> <i>it</i> <code>c</code>","parse_mode":{"@type":"textParseModeHTML"}}',
    '{"@type":"parseTextEntities","text":"<a href=\\"https://e.com/x\\">link</a> &amp; more","parse_mode":{"@type":"textParseModeHTML"}}',
    '{"@type":"parseTextEntities","text":"plain <unknown>text</unknown>","parse_mode":{"@type":"textParseModeHTML"}}',
    '{"@type":"parseTextEntities","text":"<pre><code class=\\"language-py\\">x=1</code></pre>","parse_mode":{"@type":"textParseModeHTML"}}',
    '{"@type":"parseTextEntities","text":"*bold* _it_ `code`","parse_mode":{"@type":"textParseModeMarkdown","version":1}}',
    '{"@type":"parseTextEntities","text":"**bold** __it__ `code` [link](https://e.com)","parse_mode":{"@type":"textParseModeMarkdown","version":2}}',
    '{"@type":"parseTextEntities","text":"escaped \\\\*not bold\\\\*","parse_mode":{"@type":"textParseModeMarkdown","version":2}}',
    '{"@type":"parseTextEntities","text":"~~strike~~ ||spoiler||","parse_mode":{"@type":"textParseModeMarkdown","version":2}}',
    '{"@type":"parseTextEntities","text":"x","parse_mode":null}',
    '{"@type":"parseTextEntities","text":"[https://e.com]","parse_mode":{"@type":"textParseModeMarkdown","version":1}}',
    '{"@type":"parseTextEntities","text":"**b**","parse_mode":{"@type":"textParseModeMarkdown","version":2}}',
    '{"@type":"parseTextEntities","text":"> quote\\nline","parse_mode":{"@type":"textParseModeMarkdown","version":2}}',
    '{"@type":"parseTextEntities","text":"<code class=\\"language-py\\">x</code>","parse_mode":{"@type":"textParseModeHTML"}}',
    '{"@type":"parseTextEntities","text":"x","parse_mode":{"@type":"textParseModeMarkdown","version":3}}',
    '{"@type":"parseTextEntities","text":"&nbsp; &amp","parse_mode":{"@type":"textParseModeHTML"}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"hi there","entities":[]}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"bold","entities":[{"@type":"textEntity","offset":0,"length":4,"type":{"@type":"textEntityTypeBold"}}]}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"x=1","entities":[{"@type":"textEntity","offset":0,"length":3,"type":{"@type":"textEntityTypeCode"}}]}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"bi","entities":[{"@type":"textEntity","offset":0,"length":2,"type":{"@type":"textEntityTypeBold"}},{"@type":"textEntity","offset":0,"length":2,"type":{"@type":"textEntityTypeItalic"}}]}}',
    '{"@type":"parseMarkdown","text":{"@type":"formattedText","text":"hi *there*","entities":[]}}',
    '{"@type":"parseMarkdown","text":{"@type":"formattedText","text":"**b** and *i*","entities":[]}}',
    '{"@type":"parseMarkdown","text":null}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"see t.me","entities":[{"@type":"textEntity","offset":4,"length":4,"type":{"@type":"textEntityTypeUrl"}}]}}',
    '{"@type":"parseMarkdown","text":{"@type":"formattedText","text":"0123456789","entities":[{"@type":"textEntity","offset":4,"length":10,"type":{"@type":"textEntityTypeBold"}}]}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"a\\rb","entities":[{"@type":"textEntity","offset":0,"length":3,"type":{"@type":"textEntityTypeBold"}}]}}',
    '{"@type":"searchStringsByPrefix","strings":["alpha","beta","alphabet"],"query":"alp","limit":10,"return_sorted":true}',
    '{"@type":"searchStringsByPrefix","strings":["b","a","c"],"query":"","limit":10,"return_sorted":true}',
    '{"@type":"searchStringsByPrefix","strings":["x"],"query":"","limit":10,"return_sorted":false}',
    '{"@type":"searchStringsByPrefix","strings":["Alpha","alphabet","ALP"],"query":"al","limit":10,"return_sorted":true}',
    '{"@type":"searchStringsByPrefix","strings":["alpha","alphabet"],"query":"alp","limit":1,"return_sorted":true}',
    '{"@type":"searchStringsByPrefix","strings":["alpha"],"query":"pha","limit":10,"return_sorted":true}',
    '{"@type":"searchStringsByPrefix","strings":["John Smith","Smith John"],"query":"smith","limit":10,"return_sorted":true}',
    '{"@type":"searchStringsByPrefix","strings":["cafe","caf\\u00e9"],"query":"cafe","limit":10,"return_sorted":true}',
    '{"@type":"searchStringsByPrefix","strings":["alpha","beta"],"query":"a","limit":2,"return_sorted":true}',
]

CONTROL = [
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"+1555"}',
    '{"@type":"getLanguagePackString","language_pack_database_path":"","localization_target":"android","language_pack_id":"en","key":"Continue"}',
]


def md5(path: Path) -> str:
    h = hashlib.md5()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def load(path: Path):
    lib = ctypes.CDLL(str(path))
    lib.td_execute.restype = ctypes.c_char_p
    lib.td_execute.argtypes = [ctypes.c_char_p]
    return lib


def run(lib, case: str) -> str | None:
    raw = lib.td_execute(case.encode())
    return None if raw is None else raw.decode()


def main(argv: list[str]) -> int:
    failures: list[str] = []

    if not CPP.is_file():
        print(f"FAIL oracle missing: {CPP}")
        return 1
    if not RS.is_file():
        print(f"FAIL rust dll missing: {RS}")
        return 1

    actual = md5(CPP)
    if "--skip-md5" not in argv and actual != EXPECTED_CPP_MD5:
        print(f"FAIL oracle md5 changed: {actual} != {EXPECTED_CPP_MD5}")
        print("     重判 gate 后再跑；确认无误时加 --skip-md5")
        return 1
    print(f"oracle md5={actual}")

    cpp = load(CPP)
    rs = load(RS)

    print("\n== floor: oracle 重复 3 次必须逐字节稳定 ==")
    for case in MATCH:
        outs = {run(cpp, case) for _ in range(3)}
        ok = len(outs) == 1
        if not ok:
            failures.append(f"floor unstable: {case}")
        print(f"{'PASS' if ok else 'FAIL'} {case}")
        if not ok:
            print(f"  outs: {sorted(outs)}")

    print("\n== MATCH: 两侧必须逐字节相同 ==")
    for case in MATCH:
        a = run(cpp, case)
        b = run(rs, case)
        ok = a == b and a is not None and a != ""
        if not ok:
            failures.append(f"match failed: {case}")
        print(f"{'PASS' if ok else 'FAIL'} {case}")
        print(f"  out: {a}")
        if a != b:
            print(f"  rs : {b}")

    print("\n== CONTROL: 两侧必须判为不同（证明仪器能区分） ==")
    for case in CONTROL:
        a = run(cpp, case)
        b = run(rs, case)
        ok = a != b
        if not ok:
            failures.append(f"control not discriminated: {case}")
        print(f"{'PASS' if ok else 'FAIL'} {case}")
        print(f"  cpp: {a}")
        print(f"  rs : {b}")

    print(f"\nfailures={len(failures)}")
    for f in failures:
        print(f"  - {f}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))