"""Compare tdutils' unicode tables with TDLib.

    uv run python tools/unicode_diff.py
    uv run python tools/unicode_diff.py --selftest

Covered: `get_unicode_simple_category`, `prepare_search_character`, `unicode_to_lower`,
`remove_diacritics` (the int16 tables under TABLE_SIZE and `binary_search_ranges` above it,
including the negative-value linear mapping and the three bit toggles at 0x200000), plus
`utf8_to_lower` and `utf8_get_search_words` from utf8.cpp on top of them. The corpus sweeps
every code point 0..=0x10FFFF (`sweep 0 1114111`) so a difference names a single code point,
then re-tests every range-table endpoint -1/+0/+1.

覆盖边界:
    `get_unicode_simple_category` has no `> 0x10ffff` guard: it indexes
    `unicode_simple_category_jump_pos` by `0x400 - 2 + (code >> 16)`, and that table holds
    1040 entries, so it is only defined up to 0x11FFFF. It is judged on 0..=0x10FFFF plus
    the single code 0x110000 (the first code outside the last plane, still inside the jump
    table); 0x120000 and above read outside the table and are excluded. `prep`/`low`/`dia`
    do have the early return, so they are additionally tested at 0x110000, 0x7FFFFFFF and
    0xFFFFFFFF.
    `next_utf8_unsafe` checks neither the length nor the validity of a lead byte: it reads
    up to four bytes and the two callers advance by that step, so a truncated tail makes
    them step past the end of the slice and scan out of bounds (measured: the oracle dies
    with 0xC0000409 on `utfl c3`, and again on `utfl 80`). The corpus therefore only feeds
    payloads whose scan lands exactly on the end (`scan_lands_on_end`), which still covers
    a continuation byte in 0x80..0x9F used as a lead and a surrogate encoded as ED A0 80.
    0xA0..0xFF as a lead, and every truncated tail, hit UNREACHABLE or read past the end,
    so they are never fed.

失效条件:
    TDLib 改了 unicode.cpp 的表 / binary_search_ranges 的映射 / utf8.cpp 的两个函数，
    或 C++ 侧的 std::string 不再以 '\\0' 收尾（孤立延续字节的判据会失效）。
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/unicode_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/unicode_probe.exe"
WORK = REPO / "work" / "unicode"
SOURCE = REPO / "tdutils" / "td" / "utils" / "unicode.cpp"

SWEEP_LIMIT = 0x10FFFF
CAT_LIMIT = 0x11FFFF
PAST_RANGE = [0x110000, 0x7FFFFFFF, 0xFFFFFFFF]

# Cyrillic ПРИВЕТ, Greek ΑΘΗΝΑ, CJK 中文, Japanese 日本語, full-width ＡＢＣａｂｃ,
# Turkish İ ı ẞ, German ß/ẞ, combining acute on "e", precomposed é, emoji, a surrogate
# encoded as ED A0 80, and lone continuation bytes / a lone two-byte lead.
UTFL_INPUTS = [
    "d09fd0a0d098d092d095d0a2",
    "d0bfd180d0b8d0b2d0b5d182",
    "ce91ce98ce97ce9dce91",
    "ceacceadceaecf8acea0",
    "e4b8ade69687",
    "e697a5e69cace8aa9e",
    "efbca1efbca2efbca3efbd81efbd82efbd83",
    "c4b0c4b1e1ba9e",
    "c39fe1ba9e",
    "65cc81",
    "c3a9c3a7c3b1",
    "f09f9880f09f9881",
    "eda080",
    "eda080edb080",
    "808061",
    "80806162",
    "802061",
    "9f61",
    "-",
]
WORDS_INPUTS = [
    "d09fd0a0d098d092d095d0a2",
    "d09fd180d0b8d0b2d0b5d1822c20d0bcd0b8d180",
    "d0bfd180d0b8d0b2d0b5d182",
    "d0bfd180d0b8d0b2d0b5d18220d0bcd0b8d180",
    "ce91ce98ce97ce9dce91",
    "ceacceadceaecf8a",
    "e4b8ade69687",
    "e4b8ade696872065e697a5e69cac",
    "e697a5e69cace8aa9e",
    "efbca1efbca2efbc90efbc91",
    "c4b0c4b1",
    "c4b020c4b1",
    "c39f",
    "53545241535345",
    "65cc81",
    "c3a9c3a7c3b1",
    "c0c1c2c3c4c5",
    "f09f988020f09f9881",
    "eda080",
    "612c62",
    "612d62",
    "612062",
    "202061",
    "6120",
    "20",
    "808061",
    "-",
]


def scan_lands_on_end(data: bytes) -> bool:
    """Mirror next_utf8_unsafe's step sizes: only payloads that end exactly stay in bounds."""
    pos = 0
    while pos < len(data):
        first = data[pos]
        if first & 0x80 == 0:
            used = 1
        elif first & 0x20 == 0:
            used = 2
        elif first & 0x10 == 0:
            used = 3
        elif first & 0x08 == 0:
            used = 4
        else:
            return False
        if pos + used > len(data):
            return False
        pos += used
    return True


def payload(value: str) -> str:
    if value != "-" and not scan_lands_on_end(bytes.fromhex(value)):
        raise SystemExit(
            f"{value!r} is not decodable without reading past the end; "
            "utf8_to_lower/utf8_get_search_words would scan out of bounds"
        )
    return value


def table(name: str) -> list[int]:
    text = SOURCE.read_text(encoding="utf-8")
    match = re.search(re.escape(name) + r"\s*(?:\[[^\]]*\])?\s*=\s*\{([^}]*)\}", text)
    if match is None:
        raise SystemExit(f"cannot find {name} in {SOURCE}")
    body = re.sub(r"//[^\n]*", "", match.group(1))
    return [int(value) for value in re.findall(r"-?\d+", body)]


def category_codes() -> list[int]:
    codes: set[int] = set()
    for entry in table("unicode_simple_category_ranges"):
        if entry == 0xFFFFFFFF:
            continue
        code = entry >> 5
        for candidate in (code - 1, code, code + 1):
            if 0 <= candidate <= CAT_LIMIT:
                codes.add(candidate)
    return sorted(codes)


def pair_codes(name: str) -> list[int]:
    values = table(name)
    if len(values) % 2 != 0:
        raise SystemExit(f"{name} does not hold whole pairs")
    codes: set[int] = set()
    for begin in values[0::2]:
        for candidate in (begin - 1, begin, begin + 1):
            if 0 <= candidate <= 0xFFFFFFFF:
                codes.add(candidate)
    return sorted(codes)


def script() -> str:
    lines = ["# four table functions, one code point per line above TABLE_SIZE"]
    lines.append(f"sweep 0 {SWEEP_LIMIT}")
    for code in category_codes():
        lines.append(f"cat {code}")
    for op, name in (
        ("prep", "prepare_search_character_ranges"),
        ("low", "to_lower_ranges"),
        ("dia", "without_diacritics_ranges"),
    ):
        for code in pair_codes(name):
            lines.append(f"{op} {code}")
    lines.append(f"cat {PAST_RANGE[0]}")
    for op in ("prep", "low", "dia"):
        for code in PAST_RANGE:
            lines.append(f"{op} {code}")
    lines.append("# utf8.cpp users")
    for value in UTFL_INPUTS:
        lines.append(f"utfl {payload(value)}")
    for value in WORDS_INPUTS:
        lines.append(f"words {payload(value)}")
    return "\n".join(lines) + "\n"


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, bytes]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.replace(b"\r\n", b"\n")


def compare(cpp_rc: int, cpp_out: bytes, rs_rc: int, rs_out: bytes) -> int:
    if cpp_rc == rs_rc and cpp_out == rs_out:
        return 0
    for index, (left, right) in enumerate(zip(cpp_out.splitlines(), rs_out.splitlines())):
        if left != right:
            print(f"DIFF line {index + 1}: oracle={left!r} rust={right!r}")
            return 1
    if cpp_rc != rs_rc:
        print(f"DIFF exit code: oracle={cpp_rc} rust={rs_rc}")
        return 1
    print(
        f"DIFF line count: oracle={len(cpp_out.splitlines())} rust={len(rs_out.splitlines())}"
    )
    return 1


def main() -> int:
    started = time.perf_counter()
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    text = script()
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    WORK.mkdir(parents=True, exist_ok=True)
    script_path = WORK / "unicode_script.txt"
    script_path.write_text(text)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    cpp_rc, cpp_out = run(args.cpp, script_path, env)
    rs_rc, rs_out = run(args.rs, script_path, env)
    status = compare(cpp_rc, cpp_out, rs_rc, rs_out)
    if status != 0:
        return status

    if run(args.cpp, script_path, env) != (cpp_rc, cpp_out):
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        checks = [
            (b"c=0x41 cat=1 prep=97 low=97 dia=65", "'A' is a Letter and lowercases to 'a'"),
            (b"c=0x30 cat=2 prep=48 low=48 dia=48", "'0' is a DecimalNumber, kept as a digit"),
            (b"c=0x20 cat=4 prep=32 low=32 dia=32", "space is a Separator and survives as a word break"),
            (b"c=0x2c cat=0 prep=32", "a comma is Unknown and becomes a space"),
            (b"c=0x1f600 cat=0 prep=32 low=128512 dia=128512",
             "an emoji is Unknown, so prepare turns it into a word break instead of deleting it"),
            (b"c=0xd800 cat=0 prep=0 low=55296 dia=0",
             "a surrogate is deleted for search but unicode_to_lower passes it through"),
            (b"c=0x130 cat=1 prep=105 low=105 dia=73", "İ folds to ASCII 'i', not to dotless 'ı'"),
            (b"c=0xdf cat=1 prep=223 low=223 dia=223", "ß is already lowercase and keeps itself"),
            (b"c=0x1e9e cat=1 prep=223 low=223 dia=7838", "capital ẞ folds to ß"),
            (b"c=0xff41 cat=1 prep=65345 low=65345 dia=97",
             "full-width ａ survives prepare_search_character but remove_diacritics narrows it to 'a'"),
            (b"c=0x10ffff cat=0 prep=32 low=1114111 dia=1114111",
             "the last code point is not a letter and only becomes a word break"),
            (b"cat c=0x110000 v=0", "the first code past the last plane is Unknown"),
            (b"low c=0x110000 v=0", "lowercasing answers 0 above the last plane"),
            (b"dia c=0x110000 v=0", "diacritic removal answers 0 above the last plane"),
            (b"prep c=0xffffffff v=0", "the early return covers the whole uint32 range"),
            (b"prep c=0x7fffffff v=0", "the early return is not limited to 0x110000"),
            (b"utfl in=d09fd0a0d098d092d095d0a2 out=d0bfd180d0b8d0b2d0b5d182",
             "ПРИВЕТ lowercases to привет"),
            (b"utfl in=c4b0c4b1e1ba9e out=69c4b1c39f", "İ ı ẞ lowercases to i ı ß"),
            (b"utfl in=efbca1efbca2efbca3efbd81efbd82efbd83 out=efbd81efbd82efbd83efbd81efbd82efbd83",
             "full-width Latin lowercases inside the full-width block"),
            (b"utfl in=808061 out=0061", "a stray continuation byte decodes to U+0000 before 'a'"),
            (b"utfl in=9f61 out=dfa1", "0x9F is accepted as a two-byte lead"),
            (b"words in=ceacceadceaecf8a n=1\nceb1ceb5ceb7ceb9",
             "Greek tonos is stripped down to the base letters"),
            (b"words in=efbca1efbca2efbc90efbc91 n=1\n61623031",
             "full-width Latin and digits narrow to ASCII for search"),
            (b"words in=c4b020c4b1 n=2\n69\nc4b1", "İ becomes a one-letter word, dotless ı stays"),
            (b"words in=c0c1c2c3c4c5 n=1\n61", "À..Å all collapse to a single 'a'"),
            (b"words in=f09f988020f09f9881 n=0", "emoji-only text has no search words"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        if f"sweep 0 {SWEEP_LIMIT}" not in text:
            print("SELFTEST FAIL: the full-range sweep is missing from the corpus")
            return 1
        print(f"SELFTEST OK: {len(checks)} hand-checked cells (categories, lowercasing, search)")

    print(f"PASS lines={len(cpp_out.splitlines())} wall={time.perf_counter() - started:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())