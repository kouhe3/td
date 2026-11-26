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
    超 DBL_MAX 的浮点字面量（B10，作 CONTROL）、请求与字符串里的非法 UTF-8（两侧表示不同）。
    逐格覆盖状态见 docs/coverage-domain.md。

失效条件:
    以下任一变化都必须重判 gate 并更新 docs/facts.md：
      - oracle 的 md5 不等于 EXPECTED_CPP_MD5（换了构建、改了源码、改了编译器）；
      - Rust 侧换了 target / linker（C ABI 兼容性重新验证）；
      - `td_api.tl` 变更导致 action/object 序列化格式变化。

CONTROL 组的纪律:
    CONTROL 放**记录在案、带回流条件**的已知差异，行内必须写明它是缺陷（目前：B10 的
    `1e999` 超 DBL_MAX 浮点）。差异一旦被修复，这个 case 必然转为相同——届时把它移进
    MATCH，并如实标注「已无已知差异」，不要为了凑负例去伪造一条。
    历史上的两次踩坑：`getOption` / `searchStringsByPrefix` 曾靠「Rust 未实现」制造差异，
    实现正确后判据自相矛盾；T19 修完类型错配后，那两条也已移进 MATCH。

自检（每次运行都跑）:
    两个 dll 的 md5 必须不同（防止拿 oracle 跟自己比）、每条 MATCH 输出非空、
    MATCH 语料条数不低于下限（防止语料被误删后「全绿」）。
"""

from __future__ import annotations

import ctypes
import hashlib
import os
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
    '{"@type":"getLanguagePackString","language_pack_database_path":"","localization_target":"android","language_pack_id":"en","key":"Continue"}',
    '{"@type":"getLanguagePackString","language_pack_database_path":"","localization_target":"ios","language_pack_id":"x","key":"k"}',
    '{"@type":"getLanguagePackString","language_pack_database_path":"","localization_target":"","language_pack_id":"","key":""}',
    '{"@type":"getLanguagePackString","language_pack_database_path":"","localization_target":"android","language_pack_id":"en","key":"bad key"}',
    '{"@type":"getLanguagePackString","language_pack_database_path":"","localization_target":"android","language_pack_id":"Xx","key":"Continue"}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":""}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"+"}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"abc"}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"+1555"}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"+1416"}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"+44"}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"+7"}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"+86"}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"+999"}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"abcdef","entities":[]},"quote":{"@type":"formattedText","text":"cd","entities":[]},"quote_position":0}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"abcabc","entities":[]},"quote":{"@type":"formattedText","text":"abc","entities":[]},"quote_position":4}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"abcabc","entities":[]},"quote":{"@type":"formattedText","text":"abc","entities":[]},"quote_position":0}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"abcdef","entities":[]},"quote":{"@type":"formattedText","text":"zz","entities":[]},"quote_position":0}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"abcdef","entities":[]},"quote":{"@type":"formattedText","text":"","entities":[]},"quote_position":0}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"\\ud83d\\ude00x","entities":[]},"quote":{"@type":"formattedText","text":"x","entities":[]},"quote_position":0}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"abcdef","entities":[]},"quote":{"@type":"formattedText","text":"ab","entities":[{"@type":"textEntity","offset":0,"length":2,"type":{"@type":"textEntityTypeBold"}}]},"quote_position":0}',
    '{"@type":"searchQuote","text":null,"quote":{"@type":"formattedText","text":"a","entities":[]},"quote_position":0}',
    '{"@type":"getTextEntities","text":123}',
    '{"@type":"getTextEntities","text":null}',
    '{"@type":"getTextEntities"}',
    '{"@type":"getTextEntities","text":["a"]}',
    '{"@type":"getTextEntities","text":"a","bogus":1}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":123}',
    '{"@type":"getPhoneNumberInfoSync","phone_number_prefix":"+1"}',
    '{"@type":"getFileMimeType","file_name":99999999999999999999}',
    '{"@type":"getFileMimeType","file_name":"a\\u4e2d\\u6587.txt"}',
    '{"@type":"searchStringsByPrefix","strings":["a"],"query":"a","limit":99999999999999999999,"return_sorted":true}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"ab","entities":[]},"quote":{"@type":"formattedText","text":"a","entities":[]},"quote_position":99999999999999999999}',
    '{}',
    '[]',
    '{"@type":123}',
    '{"@type":-341490693,"text":"@a"}',
    '{"@type":"noSuchType"}',
    '{"@type":null}',
    '{"@type":"getMe"}',
    '{"@type":"getTextEntities","text":"@a","@extra":7}',
    '{"@type":"getTextEntities","text":123,"@extra":7}',
    '{"@type":"getTextEntities","text":"a","@extra":{"k":[1,"x",null]}}',
    '{"@type":"getTextEntities","text":"a","@extra":1e999}',
    '{"@type":"getChatFolderDefaultIconName","folder":null}',
    '{"@type":"setLogVerbosityLevel","new_verbosity_level":"3x"}',
    '{"@type":"getJsonValue","json":123}',
    '{"@type":"getThemeParametersJsonString","theme":123}',
    '{"@type":"getTextEmojiRanges","text":123}',
    '{"@type":"testReturnError","error":123}',
    '{"@type":"getLanguagePackString","language_pack_database_path":123,"localization_target":"android","language_pack_id":"en","key":"Continue"}',
    '{"@type":"getOption","name":true}',
    '{"@type":"getTextEntities","text":"/start@somebot"}',
    '{"@type":"getTextEntities","text":"/help@бот"}',
    '{"@type":"getTextEntities","text":"/启动"}',
    '{"@type":"getTextEntities","text":"$USDT"}',
    '{"@type":"getTextEntities","text":"$USD@somebot"}',
    '{"@type":"getTextEntities","text":"$1INCH"}',
    '{"@type":"getTextEntities","text":"$btc"}',
    '{"@type":"getTextEntities","text":"$ABCDEFGHI"}',
    '{"@type":"getTextEntities","text":"$АБВ"}',
    '{"@type":"getTextEntities","text":"4111111111111111111"}',
    '{"@type":"getTextEntities","text":"378282246310005"}',
    '{"@type":"getTextEntities","text":"30569309025904"}',
    '{"@type":"getTextEntities","text":"4222222222222"}',
    '{"@type":"getTextEntities","text":"4111111111111112"}',
    '{"@type":"getTextEntities","text":"1234567890123"}',
    '{"@type":"getTextEntities","text":"0:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"}',
    '{"@type":"getTextEntities","text":"EQAAAQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHx2j"}',
    '{"@type":"getTextEntities","text":"http://[::1]/"}',
    '{"@type":"getTextEntities","text":"https://例え.jp/x"}',
    '{"@type":"getTextEntities","text":"https://пример.рф/path"}',
    '{"@type":"getTextEntities","text":"https://e.com/' + 'a' * 200 + '"}',
    '{"@type":"getTextEntities","text":"www.abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz.com"}',
    '{"@type":"getTextEntities","text":"пример@example.com"}',
    '{"@type":"getTextEntities","text":"a@пример.рф"}',
    '{"@type":"getTextEntities","text":"at 59:59"}',
    '{"@type":"getTextEntities","text":"at 60:00"}',
    '{"@type":"getTextEntities","text":"at 1:60"}',
    '{"@type":"getTextEntities","text":"at 99:99:99"}',
    '{"@type":"getTextEntities","text":"at 0:00"}',
    '{"@type":"getTextEntities","text":"at 1:2:3"}',
    '{"@type":"getTextEntities","text":"@username_"}',
    '{"@type":"getTextEntities","text":"@_"}',
    '{"@type":"getTextEntities","text":"@a_b_c"}',
    '{"@type":"getTextEntities","text":"#标签@somebot"}',
    '{"@type":"checkQuickReplyShortcutName","name":"\\u542f\\u52a8"}',
    '{"@type":"checkQuickReplyShortcutName","name":"a b"}',
    '{"@type":"checkQuickReplyShortcutName","name":"_"}',
    '{"@type":"checkQuickReplyShortcutName","name":"' + 'a' * 32 + '"}',
    '{"@type":"checkQuickReplyShortcutName","name":"' + 'a' * 33 + '"}',
    '{"@type":"checkQuickReplyShortcutName","name":"a\\u200c"}',
    '{"@type":"checkQuickReplyShortcutName","name":"\\u00f1"}',
    '{"@type":"getCountryFlagEmoji","country_code":"\\u00fcS"}',
    '{"@type":"getCountryFlagEmoji","country_code":"US "}',
    '{"@type":"getCountryFlagEmoji","country_code":""}',
    '{"@type":"getFileMimeType","file_name":"\\u56fe\\u7247.JPG"}',
    '{"@type":"getFileMimeType","file_name":"a.b.c.tar.gz"}',
    '{"@type":"getFileMimeType","file_name":"noext"}',
    '{"@type":"getFileMimeType","file_name":".hidden"}',
    '{"@type":"getFileMimeType","file_name":"trail."}',
    '{"@type":"getFileMimeType","file_name":"' + 'a' * 100 + '.txt"}',
    '{"@type":"getFileMimeType","file_name":""}',
    '{"@type":"getFileExtension","mime_type":"IMAGE/JPEG"}',
    '{"@type":"getFileExtension","mime_type":"application/x-empty"}',
    '{"@type":"getFileExtension","mime_type":"audio/mpeg"}',
    '{"@type":"getFileExtension","mime_type":"text/x-python"}',
    '{"@type":"getFileExtension","mime_type":""}',
    '{"@type":"cleanFileName","file_name":"' + 'a' * 64 + '.txt"}',
    '{"@type":"cleanFileName","file_name":"' + 'a' * 65 + '.txt"}',
    '{"@type":"cleanFileName","file_name":"a.' + 'b' * 16 + '"}',
    '{"@type":"cleanFileName","file_name":"a.' + 'b' * 17 + '"}',
    '{"@type":"cleanFileName","file_name":"!!!.???"}',
    '{"@type":"cleanFileName","file_name":"..."}',
    '{"@type":"cleanFileName","file_name":"\\u6587\\u4ef6\\u540d.\\u6269\\u5c55\\u540d"}',
    '{"@type":"getTextEmojiRanges","text":"\\ud83c\\uddfa\\ud83c\\uddf8\\ud83c\\uddf8"}',
    '{"@type":"getTextEmojiRanges","text":"\\ud83d\\udc68\\u200d\\ud83d\\udc69"}',
    '{"@type":"getTextEmojiRanges","text":"\\ud83d\\udc4d\\ud83c\\udffb\\ud83d\\udc4d\\ud83c\\udfff"}',
    '{"@type":"getTextEmojiRanges","text":"#\\ufe0f\\u20e3"}',
    '{"@type":"getTextEmojiRanges","text":"\\u2764"}',
    '{"@type":"getTextEmojiRanges","text":"\\ud83c\\udff4\\udb40\\udc67\\udb40\\udc62\\udb40\\udc73\\udb40\\udc63\\udb40\\udc74\\udb40\\udc7f"}',
    '{"@type":"getJsonValue","json":"{\\"a\\":1,\\"a\\":2}"}',
    '{"@type":"getJsonValue","json":"{\\"b\\":1,\\"a\\":2}"}',
    '{"@type":"getJsonValue","json":"[[]]"}',
    '{"@type":"getJsonValue","json":"{\\"a\\":null}"}',
    '{"@type":"getJsonValue","json":"  {\\"a\\" : 1 }  "}',
    '{"@type":"getJsonValue","json":"{\\"a\\":\\"\\\\u0000\\"}"}',
    '{"@type":"getJsonValue","json":"{\\"a\\":true,\\"b\\":false}"}',
    '{"@type":"getJsonString","json_value":{"@type":"jsonValueObject","members":[{"@type":"jsonObjectMember","key":"a","value":{"@type":"jsonValueArray","values":[{"@type":"jsonValueNull"}]}}]}}',
    '{"@type":"getJsonString","json_value":{"@type":"nope"}}',
    '{"@type":"getThemeParametersJsonString","theme":{"@type":"themeParameters","background_color":16777216}}',
    '{"@type":"getThemeParametersJsonString","theme":{"@type":"themeParameters","background_color":-1}}',
    '{"@type":"getThemeParametersJsonString","theme":{}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"include_contacts":true,"include_groups":true,"include_bots":true,"exclude_muted":true,"exclude_archived":true}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":"x"},"is_shareable":false,"pinned_chat_ids":[-1001234567890],"include_contacts":true}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder","title":{"@type":"formattedText","text":""},"is_shareable":true}}',
    '{"@type":"getChatFolderDefaultIconName","folder":{"@type":"chatFolder"}}',
    '{"@type":"getPushReceiverId","payload":"{\\"p\\":\\"AQEBAQEBAQ\\"}"}',
    '{"@type":"getPushReceiverId","payload":"{\\"p\\":\\"!!!!!!!!\\"}"}',
    '{"@type":"getPushReceiverId","payload":"{\\"user_id\\":-5}"}',
    '{"@type":"getPushReceiverId","payload":"{\\"data\\":{}}"}',
    '{"@type":"getPushReceiverId","payload":"[]"}',
    '{"@type":"parseTextEntities","text":"<tg-time unix=\\"1\\">x</tg-time>","parse_mode":{"@type":"textParseModeHTML"}}',
    '{"@type":"parseTextEntities","text":"<tg-emoji emoji-id=\\"1\\">x</tg-emoji>","parse_mode":{"@type":"textParseModeHTML"}}',
    '{"@type":"parseTextEntities","text":"<b>unclosed","parse_mode":{"@type":"textParseModeHTML"}}',
    '{"@type":"parseTextEntities","text":"x","parse_mode":{"@type":"textParseModeMarkdown","version":0}}',
    '{"@type":"parseTextEntities","text":"x","parse_mode":{"@type":"textParseModeMarkdown"}}',
    '{"@type":"parseTextEntities","text":"x","parse_mode":{}}',
    '{"@type":"parseMarkdown","text":{"@type":"formattedText","text":"a**b**c","entities":[]}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"x","entities":[{"@type":"textEntity","offset":0,"length":1,"type":{"@type":"textEntityTypePreCode","language":"py"}}]}}',
    '{"@type":"searchStringsByPrefix","strings":["\\u00c4pfel","apfel"],"query":"a","limit":10,"return_sorted":true}',
    '{"@type":"searchStringsByPrefix","strings":["caf\\u00e9","cafe"],"query":"CAF","limit":10,"return_sorted":true}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"abcdef","entities":[{"@type":"textEntity","offset":-1,"length":3,"type":{"@type":"textEntityTypeBold"}},{"@type":"textEntity","offset":4,"length":99,"type":{"@type":"textEntityTypeItalic"}}]},"quote":{"@type":"formattedText","text":"cd","entities":[]},"quote_position":-3}',
    '{"@type":"searchQuote","text":{"@type":"formattedText","text":"abc","entities":[]},"quote":{"@type":"formattedText","text":"abc","entities":[]},"quote_position":99}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"de","phone_number_prefix":"+49"}',
    '{"@type":"getPhoneNumberInfoSync","language_code":"en","phone_number_prefix":"88812345678"}',
    '{"@type":"getTextEntities","text":"41111111111111113"}',
    '{"@type":"getTextEntities","text":"411111111111111118"}',
    '{"@type":"getTextEntities","text":"4111111111111111110"}',
    '{"@type":"getTextEntities","text":"EQAAAQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHx2k"}',
    '{"@type":"getTextEntities","text":"UQAAAQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eH0Bl"}',
    '{"@type":"getTextEntities","text":"EQAAAQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHx2"}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"abc","entities":[{"@type":"textEntity","offset":0,"length":3,"type":{"@type":"textEntityTypeCode"}},{"@type":"textEntity","offset":1,"length":1,"type":{"@type":"textEntityTypeBold"}}]}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"abc","entities":[{"@type":"textEntity","offset":0,"length":2,"type":{"@type":"textEntityTypePreCode","language":"py"}},{"@type":"textEntity","offset":1,"length":1,"type":{"@type":"textEntityTypeBold"}}]}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"a*b","entities":[{"@type":"textEntity","offset":0,"length":3,"type":{"@type":"textEntityTypeItalic"}}]}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"a`b","entities":[{"@type":"textEntity","offset":0,"length":3,"type":{"@type":"textEntityTypeCode"}}]}}',
    '{"@type":"getMarkdownText","text":{"@type":"formattedText","text":"ab","entities":[{"@type":"textEntity","offset":0,"length":2,"type":{"@type":"textEntityTypeTextUrl","url":"https://e.com"}}]}}',
    '{"@type":"searchStringsByPrefix","strings":["ab","ab"],"query":"ab","limit":10,"return_sorted":true}',
    '{"@type":"searchStringsByPrefix","strings":["ba","ab","ab"],"query":"a","limit":10,"return_sorted":true}',
    'not json',
    '{',
    'null',
    '"x"',
    '{"a":1}',
    '{"@type":"getTextEntities",}',
    '{"@type":"getTextEntities" "text":"a"}',
    '{"@type":"getTextEntities","text":"a"',
    "{'a':1}",
    'tru',
    'falsey',
    'nul',
    '{"@type":"getTextEntities","text":"a"} x',
    '{"@type":"getTextEntities","text":"a"}}',
    '{"a":1,,}',
    '[,1]',
    '[1,]',
    '{"a"}',
    '{"a":}',
    '{"@type":"getTextEntities","text":"\\\\ud83d"}',
    '{"@type":"getTextEntities","text":"\\\\u12"}',
    '{"@type":"getTextEntities","text":"\\\\uzzzz"}',
    '{"@type":"getTextEntities","text":"caf\\u00e9"}',
    '{"@type":"getTextEntities","text":"a\\\\/b"}',
    '{"@type":"getTextEntities","text":"a\\\\u0000b"}',
    '{"@type":"getJsonValue","json":"1-2"}',
    '{"@type":"getJsonValue","json":"1.2.3"}',
    '{"@type":"getJsonValue","json":"+"}',
    '{"@type":"getJsonValue","json":"-"}',
    '{"@type":"getJsonValue","json":"[1,]"}',
    '{"@type":"getJsonValue","json":""}',
    '{"@type":"getJsonValue","json":"{\\"a\\":1,}"}',
    '{"@type":"getPushReceiverId","payload":"[]"}',
    '{"@type":"getPushReceiverId","payload":"not json"}',
]

# 已知差异（backlog B10）：超 DBL_MAX 的浮点字面量，C++ 给 DBL_MAX 的十进制、Rust 给 inf。
CONTROL = [
    '{"@type":"getJsonValue","json":"1e999"}',
]

# 有状态 action 只在有序序列里有意义：每段序列对两个 dll 各跑一遍，逐项比结果。
# 每段序列自己负责把状态恢复原值，避免污染其它段落。
SEQUENCES = [
    [
        '{"@type":"getLogVerbosityLevel"}',
        '{"@type":"setLogVerbosityLevel","new_verbosity_level":3}',
        '{"@type":"getLogVerbosityLevel"}',
        '{"@type":"setLogVerbosityLevel","new_verbosity_level":5}',
        '{"@type":"getLogVerbosityLevel"}',
    ],
    [
        '{"@type":"getLogTagVerbosityLevel","tag":"td_requests"}',
        '{"@type":"setLogTagVerbosityLevel","tag":"td_requests","new_verbosity_level":7}',
        '{"@type":"getLogTagVerbosityLevel","tag":"td_requests"}',
        '{"@type":"setLogTagVerbosityLevel","tag":"td_requests","new_verbosity_level":3}',
        '{"@type":"getLogTagVerbosityLevel","tag":"td_requests"}',
    ],
    [
        '{"@type":"getLogStream"}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamEmpty"}}',
        '{"@type":"getLogStream"}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamDefault"}}',
        '{"@type":"getLogStream"}',
        '{"@type":"getLogTagVerbosityLevel","tag":"no_such_tag"}',
        '{"@type":"setLogTagVerbosityLevel","tag":"no_such_tag","new_verbosity_level":2}',
        '{"@type":"addLogMessage","verbosity_level":1,"text":"seq-probe"}',
        '{"@type":"getLogTags"}',
    ],
    [
        '{"@type":"getLogStream"}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamFile","path":"work/logs/seq-a.log","max_file_size":4096,"redirect_stderr":true}}',
        '{"@type":"getLogStream"}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamFile","path":"work/logs/seq-b.log","max_file_size":1024}}',
        '{"@type":"getLogStream"}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamFile","path":"work/logs/sub/../seq-c.log","max_file_size":2048,"redirect_stderr":false}}',
        '{"@type":"getLogStream"}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamFile","path":"work/logs/seq-d.log","max_file_size":0}}',
        '{"@type":"getLogStream"}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamFile","path":"","max_file_size":4096}}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamFile","path":"C:/no_such_dir_xyz/a.log","max_file_size":4096}}',
        '{"@type":"getLogStream"}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamEmpty"}}',
        '{"@type":"getLogStream"}',
        '{"@type":"setLogStream","log_stream":{"@type":"logStreamDefault"}}',
        '{"@type":"getLogStream"}',
    ],
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


def add_runtime_dirs() -> None:
    # CPython 3.8+ 的 CDLL 走 LOAD_LIBRARY_SEARCH_DEFAULT_DIRS，PATH 不参与依赖查找
    for candidate in (CPP.parent, Path("C:/msys64/ucrt64/bin")):
        if candidate.is_dir():
            os.add_dll_directory(str(candidate))


LOG_DIR = Path("work/logs")


def ensure_log_dir() -> bool:
    # logStreamFile 序列要往这里写文件；目录不在就会静默走成「打不开」分支
    LOG_DIR.mkdir(parents=True, exist_ok=True)
    return LOG_DIR.is_dir()


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

    add_runtime_dirs()
    if not ensure_log_dir():
        print(f"FAIL log dir missing: {LOG_DIR}")
        return 1
    cpp = load(CPP)
    rs = load(RS)

    print("\n== self-test: 管道不能空转 ==")
    self_checks = [
        ("两个 dll 必须不同", actual != md5(RS), "oracle 与 rust 的 md5 相同，等于拿自己比自己"),
        ("MATCH 语料下限", len(MATCH) >= 300, f"只有 {len(MATCH)} 条，语料可能被误删"),
        ("CONTROL 非空", len(CONTROL) >= 1, "没有已知差异作负例时，至少要有一条记录在案的差异"),
    ]
    empty_cells = 0
    for case in MATCH:
        output = run(cpp, case)
        if output is None or output == "":
            empty_cells += 1
    self_checks.append(("MATCH 输出非空", empty_cells == 0, f"{empty_cells} 条没有任何输出"))
    for name, ok, detail in self_checks:
        if not ok:
            failures.append(f"self-test failed: {name} ({detail})")
        print(f"{'PASS' if ok else 'FAIL'} {name}{'' if ok else ': ' + detail}")

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

    print("\n== SEQUENCE: 有状态 action 按有序序列比 ==  ")
    for index, sequence in enumerate(SEQUENCES):
        cpp_outputs = [run(cpp, step) for step in sequence]
        rs_outputs = [run(rs, step) for step in sequence]
        ok = cpp_outputs == rs_outputs
        if not ok:
            failures.append(f"sequence {index} differs")
        print(f"{'PASS' if ok else 'FAIL'} sequence {index} ({len(sequence)} steps)")
        for step, cpp_out, rs_out in zip(sequence, cpp_outputs, rs_outputs):
            marker = " " if cpp_out == rs_out else ">"
            print(f" {marker} {step}")
            print(f"   cpp: {cpp_out}")
            if cpp_out != rs_out:
                print(f"   rs : {rs_out}")

    print(f"\nfailures={len(failures)}")
    for f in failures:
        print(f"  - {f}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))