"""Compare td::get_word_transliterations (tdutils/translit.cpp) with the Rust port.

    uv run python tools/translit_diff.py
    uv run python tools/translit_diff.py --selftest

Criterion: byte-exact stdout of the C++ oracle and the Rust probe over one frozen
script. The script is ASCII (payloads travel as hex) and every payload is fed
twice, once per `allow_partial` value. The `tr` output keeps the returned order
(`tr in=... partial=... n=<k>` plus ` out<i>=<hex>` lines), so ordering and the
`td::unique` dedup are judged, not just the set. A FLOOR rerun of the oracle must
reproduce the first run byte for byte.

Covered: all four rule tables, the two-pass structure (simple-table pass then the
complex-rule pass with its `s + rule.second` partial appends), first-match order
over the complex list, the `if (!s.empty())` push guards, the `td::unique` sort by
bytes, and the function-local `static const` tables (the `once` lines repeat a
word five times inside one process). Payloads: the 26 latin letters alone, in
pairs and in triples; all 17 en->ru and all 5 ru->en complex rules; overlapping
prefixes (shch/sch/sh/s, yo/jo, yu/ju, ya/ja, ks/kappa-sigma, ii at the end and in
the middle); uppercase and mixed script; all 33 Russian letters in both cases;
short-i and yo; punctuation, digits and spaces; the empty word; words of 32+ chars; and
script-neutral payloads whose two passes coincide.

Boundary: `next_utf8_unsafe` takes the sequence length from the lead byte and
never validates it, so a payload whose last sequence would run past the end would
read out of bounds (and a 0xF8..0xFF lead is UNREACHABLE). `is_scannable` models
that rule and drops such payloads from the corpus; the only deliberately invalid
payload, `c0af` (an overlong "/"), has a length-valid shape and judges the
re-encoding through `append_utf8_character`.

Fails if: the Rust port and the oracle disagree on any byte of the script output,
the oracle is not deterministic, or the corpus hash changes under the frozen
port (the mutations of the RED rounds live in the Rust file, never here).
"""

from __future__ import annotations

import argparse
import hashlib
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/translit_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/translit_probe.exe"
WORK = REPO / "work" / "translit"

LATIN = "abcdefghijklmnopqrstuvwxyz"

EN_COMPLEX = ["ch", "ei", "ey", "ia", "iy", "jo", "ju", "ja", "kh", "shch", "sh", "sch", "ts", "yo", "yu", "ya", "zh"]
RU_COMPLEX = ["\u0438\u0439", "\u0438\u044f", "\u043a\u0441", "yo", "jo"]

# all 33 letters, lowercase then uppercase (yo = U+0451 / U+0401 after ie)
RU_LOWER = (
    "\u0430\u0431\u0432\u0433\u0434\u0435\u0451\u0436\u0437\u0438\u0439\u043a\u043b\u043c\u043d"
    "\u043e\u043f\u0440\u0441\u0442\u0443\u0444\u0445\u0446\u0447\u0448\u0449\u044a\u044b\u044c"
    "\u044d\u044e\u044f"
)
RU_UPPER = (
    "\u0410\u0411\u0412\u0413\u0414\u0415\u0401\u0416\u0417\u0418\u0419\u041a\u041b\u041c\u041d"
    "\u041e\u041f\u0420\u0421\u0422\u0423\u0424\u0425\u0426\u0427\u0428\u0429\u042a\u042b\u042c"
    "\u042d\u042e\u042f"
)

# shch/sch/sh/s, yo/jo, yu/ju, ya/ja, ks/ks and rules that are prefixes of the remainder
OVERLAP_LATIN = [
    "shch", "sch", "sh", "s", "sc", "scc", "shc", "shh", "shcha", "shchka", "shchsh", "shchyo",
    "shchshch", "ssch", "shsh", "ch", "c", "h", "chch", "ts", "tsh", "tss", "zh", "kh", "khh",
    "yo", "y", "o", "jo", "j", "yu", "u", "ju", "ya", "a", "ja", "ei", "ey", "eiy", "ia", "iy",
    "iya", "yiy", "ks", "kss", "kx", "x", "yoyo", "jy", "jj", "yy", "yoyuya", "shchts", "schz",
]
OVERLAP_RU = [
    "\u0438\u0439", "\u0438\u0439\u0439", "\u043a\u0438\u0439", "\u0438\u0438\u0439",
    "\u0439\u0438\u0439", "\u0438\u044f", "\u0438\u044f\u0438\u044f", "\u043a\u0438\u044f",
    "\u043a\u0441", "\u043a\u0441\u043a\u0441", "\u043a\u0441\u0441", "\u0441\u043a\u0441",
    "\u043a\u0441\u0439\u043e", "\u0439\u043e", "\u0439\u043e\u0439\u043e", "\u043c\u043e\u0439",
    "\u043c\u043e\u0438\u0439", "\u0438\u0439\u043c\u043e", "\u043c\u0438\u0439",
    "\u043c\u043e\u0439\u0439", "\u043a\u0441yo", "yo\u043a\u0441", "\u0438\u0439yo",
]
MIXED = [
    "A", "Z", "CH", "Sh", "SHCH", "Yo", "YO", "ScH",
    "\u041a\u0421", "\u0418\u0419", "a\u0411", "Ab", "\u0428ch", "ch\u0428",
    "\u0419o", "\u0416", "\u0401", "\u0419", "\u042c", "\u042a", "\u042b", "\u042d",
    "\u042e", "\u042f", "\u0414a", "a\u0414", "\u043f\u0420\u0438\u0412\u0435\u0422",
]
PUNCT = [
    "", "-", "_", "'", ".", ",", " ", "  ", "\t", "a b", "a-b", "a_b", "..", "...", "()", "@#",
    "a@b.c", "123", "1a", "a1", "a.a", "1-2-3", "0", "9", "12",
]
# whole lowercase words: a full ru->en ride, ий at the end and in the middle, кс inside a word
RU_WORDS = [
    "\u043f\u0440\u0438\u0432\u0435\u0442",
    "\u043c\u043e\u0439",
    "\u0439\u043e\u0434",
    "\u0451\u0436",
    "\u0451\u043b\u043a\u0430",
    "\u0441\u044a\u0435\u0437\u0434",
    "\u043c\u043e\u0441\u043a\u0432\u0430",
    "\u043a\u0441\u0435\u043d\u0438\u044f",
    "\u0438\u0438\u0439",
    "\u043a\u0438\u0439",
]
LONG = [
    "a" * 32, "z" * 40, "ch" * 16, "shch" * 8, "yo" * 16, "sch" * 11, "abc" * 11,
    RU_LOWER * 2, "\u0438\u0439" * 16, "\u043a\u0441" * 16, "\u0448ch" * 11,
]
RAW_HEX = ["c0af"]

ONCE_WORDS = ["yo", "y", "shch", "sc", "\u0438\u0439", "\u043f\u0440\u0438\u0432\u0435\u0442", "a"]


def sequence_length(lead: int) -> int:
    """`next_utf8_unsafe` picks the length from the lead byte alone (0 = UNREACHABLE)."""
    if lead < 0x80:
        return 1
    if lead & 0x20 == 0:
        return 2
    if lead & 0x10 == 0:
        return 3
    if lead & 0x08 == 0:
        return 4
    return 0


def is_scannable(payload: bytes) -> bool:
    """No sequence may run past the end and no 0xF8..0xFF lead may appear."""
    pos = 0
    while pos < len(payload):
        need = sequence_length(payload[pos])
        if need == 0 or pos + need > len(payload):
            return False
        pos += need
    return True


def tok(payload: bytes) -> str:
    return payload.hex() if payload else "-"


def corpus() -> tuple[list[bytes], int]:
    """The unique payloads that can be scanned, plus how many were dropped as unsafe."""
    candidates: list[bytes] = []
    candidates.extend(letter.encode() for letter in LATIN)
    for first in LATIN:
        for second in LATIN:
            candidates.append((first + second).encode())
    for first in LATIN:
        for second in LATIN:
            for third in LATIN:
                candidates.append((first + second + third).encode())
    candidates.extend(rule.encode() for rule in EN_COMPLEX)
    candidates.extend(rule.encode() for rule in RU_COMPLEX)
    candidates.extend(text.encode() for text in OVERLAP_LATIN)
    candidates.extend(text.encode() for text in OVERLAP_RU)
    candidates.extend(text.encode() for text in MIXED)
    candidates.extend(letter.encode() for letter in RU_LOWER)
    candidates.extend(letter.encode() for letter in RU_UPPER)
    candidates.extend(text.encode() for text in PUNCT)
    candidates.extend(text.encode() for text in RU_WORDS)
    candidates.extend(text.encode() for text in LONG)
    candidates.extend(bytes.fromhex(text) for text in RAW_HEX)
    unique = list(dict.fromkeys(candidates))
    safe = [payload for payload in unique if is_scannable(payload)]
    return safe, len(unique) - len(safe)


def script() -> str:
    items, skipped = corpus()
    lines = [f"# get_word_transliterations: {len(items)} payloads, both allow_partial values"]
    for payload in items:
        for partial in (0, 1):
            lines.append(f"tr {tok(payload)} {partial}")
    for word in ONCE_WORDS:
        for partial in (0, 1):
            lines.append(f"once {word.encode().hex()} {partial}")
    if skipped:
        lines.append(f"# skipped {skipped} payloads whose last sequence would run past the end")
    return "\n".join(lines) + "\n"


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


def records(output: str) -> dict[str, list[str]]:
    """Group the ` out<i>=...` lines under the header line that introduced them."""
    grouped: dict[str, list[str]] = {}
    header = ""
    for line in output.splitlines():
        if line.startswith(" "):
            grouped.setdefault(header, []).append(line[1:])
        else:
            header = line
            grouped.setdefault(header, [])
    return grouped


def compare(cpp_rc: int, cpp_out: str, rs_rc: int, rs_out: str) -> int:
    if cpp_rc == rs_rc and cpp_out == rs_out:
        return 0
    for index, (left, right) in enumerate(zip(cpp_out.splitlines(), rs_out.splitlines())):
        if left != right:
            print(f"DIFF line {index + 1}: oracle={left!r} rust={right!r}")
            return 1
    if cpp_rc != rs_rc:
        print(f"DIFF exit code: oracle={cpp_rc} rust={rs_rc}")
        return 1
    print(f"DIFF line count: oracle={len(cpp_out.splitlines())} rust={len(rs_out.splitlines())}")
    return 1


def cells() -> list[tuple[str, list[str], str]]:
    def cell(header: str, outs: list[str], reason: str) -> tuple[str, list[str], str]:
        return header, [f"out{index}={value}" for index, value in enumerate(outs)], reason

    checks = [
        cell("tr in=- partial=0 n=0", [], "empty word: both `if (!s.empty())` guards drop their pass"),
        cell("tr in=- partial=1 n=0", [], "no scan position exists, so no rule can be a partial"),
        cell("tr in=79 partial=0 n=2", ["79", "d0b8"],
             "y: the ru->en pass keeps it verbatim, the en->ru pass maps it to U+0438"),
        cell("tr in=79 partial=1 n=6", ["65", "79", "d0b5", "d0b8", "d18e", "d18f"],
             "partials: e from the ru->en yo rule, U+0435/U+044E/U+044F from en->ru yo/yu/ya"),
        cell("tr in=796f partial=0 n=4", ["65", "796f", "d0b5", "d0b8d0be"],
             "yo: both tables match it fully (U+0435 and e) besides the two simple passes"),
        cell("tr in=796f partial=1 n=4", ["65", "796f", "d0b5", "d0b8d0be"],
             "allow_partial adds nothing once a rule matches: no duplicate of the full one"),
        cell("tr in=73686368 partial=0 n=3", ["73686368", "d181d185d0bad185", "d189"],
             "shch wins over sh by table order, so the complex pass gives U+0449"),
        cell("tr in=73686368 partial=1 n=3", ["73686368", "d181d185d0bad185", "d189"],
             "a full shch match never adds its own partial"),
        cell("tr in=736368 partial=0 n=3", ["736368", "d181d0bad185", "d189"],
             "sch: sh cannot match it (the second byte differs), so the sch rule is live"),
        cell("tr in=736863 partial=0 n=3", ["736863", "d181d185d0ba", "d188d0ba"],
             "shc: sh matches the first two bytes, so the complex pass gives U+0448 U+043A"),
        cell("tr in=7363 partial=1 n=4", ["7363", "d181d0ba", "d181d187", "d189"],
             "sc: U+0449 from the sch partial, U+0441 U+0447 from the ch partial at position 1"),
        cell("tr in=6368 partial=0 n=3", ["6368", "d0bad185", "d187"],
             "ch: the simple pass spells U+043A U+0445, the complex pass U+0447"),
        cell("tr in=d0b8d0b9 partial=0 n=3", ["6979", "79", "d0b8d0b9"],
             "ii (U+0438 U+0439): iy from the simple pass, y from the ru->en complex rule"),
        cell("tr in=d0bfd180d0b8d0b2d0b5d182 partial=0 n=2",
             ["707269766574", "d0bfd180d0b8d0b2d0b5d182"],
             "a whole Cyrillic word keeps itself and gains privet; ASCII sorts first"),
        cell("tr in=4348 partial=0 n=1", ["4348"],
             "uppercase has no rule and passes through; unique collapses the duplicate pass"),
        cell("tr in=313233 partial=0 n=1", ["313233"],
             "digits: every table leaves them verbatim, so the two passes dedup"),
        cell("tr in=d18c partial=0 n=1", ["d18c"],
             "a lone soft sign: the ru->en pass maps it to the empty string and the guard drops it"),
        cell("tr in=c0af partial=0 n=1", ["2f"],
             "an overlong UTF-8 slash is decoded without validation and re-encoded as 2f"),
        (
            "once in=796f partial=0 repeats=5 identical=1 n=4",
            ["out=65,796f,d0b5,d0b8d0be"],
            "the function-local static tables do not drift between calls",
        ),
    ]
    return checks


def main() -> int:
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
    script_path = WORK / "translit_script.txt"
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
        grouped = records(cpp_out)
        for header, expected, reason in cells():
            actual = grouped.get(header)
            if actual is None:
                print(f"SELFTEST FAIL: {reason} (missing {header!r})")
                return 1
            if actual != expected:
                print(f"SELFTEST FAIL: {reason}\n  header={header!r}\n  expected={expected}\n  actual={actual}")
                return 1
        print(f"SELFTEST OK: {len(cells())} hand-checked cells (table order, partials, dedup, guards)")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())