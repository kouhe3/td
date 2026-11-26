"""Build the language pack fixtures for `getLanguagePackString` (B7).

`LanguagePackManager::get_language_pack_string` reads a caller-supplied SQLite database
(`td/telegram/LanguagePackManager.cpp:1261`): one `kv_<localization_target>_<language_code>`
table per language (`get_database_table_name`, `SqliteKeyValue`), blobs
`1<ordinary>`, `2<zero>\\0<one>\\0<two>\\0<few>\\0<many>\\0<other>` or anything else for "deleted",
and the meta keys `!version`, `!key_count`, `!base_language_code`.

No external data is needed: TDLib's `tdlib-data` language packs are just this database, and the
reader is in this repository. The fixtures below therefore cover the loader's own branches.

Both sides get their own copy of the directory (`work/langpack/cpp`, `work/langpack/rs`), because
the C++ side writes `!key_count` back into the database on first read.

Values must be valid UTF-8: `td::JsonBuilder` CHECK-fails on invalid UTF-8 when it serializes a
`languagePackStringValueOrdinary`, so an invalid byte in a pack would abort the oracle process.
"""

from __future__ import annotations

import sqlite3
import sys
from pathlib import Path

SIDES = ("cpp", "rs")
TARGET = "android"
CODE = "en"


def _db(root: Path, name: str, rows: dict[str, bytes], target: str = TARGET, code: str = CODE) -> None:
    path = root / name
    # 前一版的 -wal/-shm 留在旁边会让 sqlite 拿旧日志套新库
    for stale in (path, path.with_name(path.name + "-wal"), path.with_name(path.name + "-shm")):
        stale.unlink(missing_ok=True)
    db = sqlite3.connect(str(path))
    table = f'"kv_{target}_{code}"'
    db.execute(f"CREATE TABLE IF NOT EXISTS {table} (k BLOB PRIMARY KEY, v BLOB)")
    for key, value in rows.items():
        db.execute(f"INSERT INTO {table} (k, v) VALUES (?, ?)", (key.encode(), value))
    db.commit()
    db.close()


def build(root: Path) -> None:
    root.mkdir(parents=True, exist_ok=True)
    # 有版本的语言包：缺失的 key 记为 deleted（Load string ... from database 之后走 deleted_strings_）
    _db(
        root,
        "full.db",
        {
            "!version": b"7",
            "!base_language_code": b"en",
            "Continue": b"1Continue",
            "Plural": b"2\x00one\x00two\x00few\x00many\x00other",
            "TrailingNul": b"2\x00a\x00b\x00c\x00d\x00",
            "TooFew": b"2a\x00b",
            "ManyParts": b"2\x00a\x00b\x00c\x00d\x00e\x00f",
            "OnePart": b"2solo",
            "EmptyOrdinary": b"1",
            "Deleted": b"3",
            "Zero": b"0zero",
            "Utf8": "1你好 😀".encode(),
            "Escapes": b'1a"b\\c\nd\te',
            "Controls": b"1\x01\x1f\x7f",
        },
    )
    # 有表但没有 !version：命中 → 返回值，未命中 → 404（不是 deleted）
    _db(root, "no-version.db", {"Continue": b"1Continue", "Gone": b"3"})
    # !version 非数字：td::to_integer<int32> 给 0，于是「缺失即 deleted」
    _db(root, "bad-version.db", {"!version": b"abc", "Present": b"1abc"})
    # !version 是空 blob：按 -1 处理，与「没有 !version」一致
    _db(root, "empty-version.db", {"!version": b"", "Present": b"1abc"})
    # 换一个 localization target：另一张表，互不可见
    _db(
        root,
        "other-target.db",
        {"!version": b"1", "Continue": b"1ios-continue"},
        target="ios",
        code="de",
    )
    # 自定义语言包 ID（X 前缀）
    _db(root, "custom.db", {"!version": b"1", "Continue": b"1custom"}, target=TARGET, code="Xy")
    # 不是 SQLite 的文件：open 失败，按空库处理
    (root / "garbage.db").write_bytes(b"not a sqlite database, just bytes\x00\x01\x02")
    # 不存在的库：C++ 会建出空库，返回 404
    (root / "absent.db").unlink(missing_ok=True)


def main(argv: list[str]) -> int:
    root = Path(argv[0]) if argv else Path("work/langpack")
    for side in SIDES:
        build(root / side)
        print(f"built {root / side}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))