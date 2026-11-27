"""Compare the structural part of tdutils utf8.h with TDLib.

    uv run python tools/utf8_diff.py
    uv run python tools/utf8_diff.py --selftest

Covered: `check_utf8` (the shift-based DFA), `utf8_length`, the first-code-unit
predicate, `next_utf8_unsafe`, `append_utf8_character`, `utf8_truncate` /
`utf8_substr`, and the UTF-16 variants. Two quirks are deliberately in the
corpus: `utf8_truncate` compares the *byte* size with the character budget, and
`utf8_utf16_length` counts F8..FF as one unit while `utf8_utf16_truncate`
subtracts two.
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
DEFAULT_CPP = "C:/tmp/td-probe/utf8_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/utf8_probe.exe"
WORK = REPO / "work" / "utf8"

STRINGS = [
    "", "61", "c3a9", "e4bda0", "f09f9880", "616263", "e4bda061", "f09f988061",
    "80", "bf", "c080", "c1bf", "e08080", "e09fbf", "e0a080", "eda080", "ed9fbf",
    "f0808080", "f08fbfbf", "f0908080", "f48fbfbf", "f4908080", "f5808080",
    "c3", "e4bd", "f09f98", "ff", "fe", "61ff62", "c328", "f8808080",
]
SHORT_STRINGS = ["", "61", "c3a9", "e4bda0", "f09f9880", "616263", "e4bda061", "f8808080"]
NEXT_INPUTS = ["61", "c3a9", "e4bda0", "f09f9880", "e4bda061"]
APP_CODES = [0, 0x7F, 0x80, 0x7FF, 0x800, 0xD7FF, 0xD800, 0xFFFF, 0x10000, 0x1F600, 0x10FFFF, 0x110000,
             0xFFFFFFFF]
FIRST_BYTES = [0x00, 0x7F, 0x80, 0xBF, 0xC0, 0xFF]


def tok(value: str) -> str:
    return value if value else "-"


def script() -> str:
    lines = ["# DFA, lengths, decode/encode, truncation"]
    for value in STRINGS:
        lines.append(f"check {tok(value)}")
    for value in STRINGS:
        lines.append(f"len {tok(value)}")
    for value in STRINGS:
        lines.append(f"u16len {tok(value)}")
    for byte in FIRST_BYTES:
        lines.append(f"first {byte}")
    for value in NEXT_INPUTS:
        lines.append(f"next {value}")
    for code in APP_CODES:
        lines.append(f"app {code}")
    for value in SHORT_STRINGS:
        for n in (0, 1, 2, 3):
            lines.append(f"trunc {tok(value)} {n}")
            lines.append(f"u16trunc {tok(value)} {n}")
    for value in SHORT_STRINGS:
        for offset in (0, 1, 2, 3):
            lines.append(f"substr {tok(value)} {offset}")
            lines.append(f"u16substr {tok(value)} {offset}")
    for value in SHORT_STRINGS:
        for offset in (1, 2):
            for length in (1, 2):
                lines.append(f"substr2 {tok(value)} {offset} {length}")
                lines.append(f"u16substr2 {tok(value)} {offset} {length}")
    return "\n".join(lines) + "\n"


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


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
    script_path = WORK / "utf8_script.txt"
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
            ("check in=61 r=1", "ASCII"),
            ("check in=c3a9 r=1", "2-byte character"),
            ("check in=e4bda0 r=1", "3-byte character"),
            ("check in=f09f9880 r=1", "4-byte character"),
            ("check in=c080 r=0", "overlong C0 80"),
            ("check in=c1bf r=0", "overlong C1 BF"),
            ("check in=e08080 r=0", "overlong E0 80 80"),
            ("check in=e0a080 r=1", "the first valid 3-byte character"),
            ("check in=eda080 r=0", "a surrogate is rejected"),
            ("check in=ed9fbf r=1", "the last code point before the surrogates"),
            ("check in=f0808080 r=0", "overlong F0 80 80 80"),
            ("check in=f0908080 r=1", "the first valid 4-byte character"),
            ("check in=f48fbfbf r=1", "U+10FFFF"),
            ("check in=f4908080 r=0", "above U+10FFFF"),
            ("check in=c3 r=0", "truncated sequence"),
            ("check in=80 r=0", "a lone continuation byte"),
            ("len in=e4bda0 n=1", "utf8_length of one 3-byte character"),
            ("len in=80 n=0", "a lone continuation byte is not a character"),
            ("len in=616263 n=3", "three ASCII characters"),
            ("first c=128 r=0", "a continuation byte"),
            ("first c=192 r=1", "a two-byte lead"),
            ("next in=e4bda0 code=20320 used=3", "U+4F60"),
            ("next in=f09f9880 code=128512 used=4", "U+1F600"),
            ("app code=128512 out=f09f9880", "append a 4-byte character"),
            ("app code=1114112 out=f4908080", "append does not validate the code point"),
            ("trunc in=e4bda0 n=0 out=-", "truncating to zero characters"),
            ("trunc in=e4bda0 n=2 out=e4bda0",
             "a 3-byte character survives a budget of 2 (the fast path compares bytes)"),
            ("trunc in=616263 n=2 out=6162", "two ASCII characters"),
            ("trunc in=616263 n=3 out=616263", "the byte size equals the budget, so nothing is cut"),
            ("substr in=e4bda061 off=1 out=61", "offset 1 skips the whole 3-byte character"),
            ("u16len in=f09f9880 n=2", "a 4-byte character is two UTF-16 units"),
            ("u16len in=f8808080 n=1", "F8 is not counted as a surrogate pair by the length function"),
            ("u16trunc in=f09f9880 n=0 out=-", "zero units"),
            ("u16trunc in=616263 n=2 out=6162", "two ASCII units"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        print(f"SELFTEST OK: {len(checks)} hand-checked cells (DFA edges, byte/char budget, UTF-16 units)")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())