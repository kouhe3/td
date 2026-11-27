"""Compare the string helpers in tdutils misc.h with TDLib.

    uv run python tools/string_diff.py
    uv run python tools/string_diff.py --selftest

Covered: `to_lower` / `to_upper` (ASCII only), `trim` (its `is_space` set is
{' ', '\\t', '\\r', '\\n', '\\0', '\\v'} and excludes '\\f'), `lpad` / `lpad0` /
`rpad` (no-op when the target is not larger), `oneline` (collapse line breaks to
one space, drop spaces right after them and at the end), `replace_with_spaces`,
`implode`, `split` (always two parts), `full_split` (max_parts), and the byte
predicates. Every corpus entry is already hex, so NUL and high bytes stay
visible; "-" marks the empty string.
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
DEFAULT_CPP = "C:/tmp/td-probe/string_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/string_probe.exe"
WORK = REPO / "work" / "string"

CASE_INPUTS = ["", "41", "4142", "61626358595a", "6142635f3039", "633066666565", "e9", "80ff", "4180ff62"]
TRIM_INPUTS = ["", "2020616220", "090d0a", "0b610b", "0c610c", "006100", "20", "0c20610c", "090a20616263200a09"]
ONELINE_INPUTS = ["", "61", "2020610a2020622020", "610a0a62", "610d0a62", "0a61", "610920", "0a", "610a62200a0a6320"]
PAD_CASES = [("616263", 5, "0"), ("616263", 3, "0"), ("616263", 2, "0"), ("", 3, "x"), ("616263", 0, "x"),
             ("41", 4, " ")]
LPAD0_CASES = [("3432", 4), ("", 2)]
RPAD_CASES = [("6162", 4, "x"), ("6162", 1, "x"), ("", 2, " ")]
REPLACE_CASES = [("612d622d63", "2d"), ("612e622c63", "2c2e"), ("616263", ""), ("616263", "616263"), ("", "616263")]
IMPLODE_CASES = [(32, ["61", "62", "63"]), (0, ["61", "62"]), (44, ["61"]), (44, [])]
SPLIT_CASES = [("612c62", 44), ("616263", 44), ("", 44), ("2c61", 44), ("612c", 44), ("612c2c62", 44)]
FULLSPLIT_CASES = [
    ("612c622c", 44, 4294967295),
    ("", 44, 5),
    ("612c622c63", 44, 2),
    ("612c622c63", 44, 1),
    ("612c622c63", 44, 4),
    ("2c", 44, 3),
    ("616263", 44, 2),
]
PRED_BYTES = "00090b0c200d0a5f30415a617a67804646ff7f"


def token(value: str) -> str:
    return value if value else "-"


def script() -> str:
    lines = ["# case mapping, trimming, padding, line folding"]
    for value in CASE_INPUTS:
        lines.append(f"lower {token(value)}")
        lines.append(f"upper {token(value)}")
    for value in TRIM_INPUTS:
        lines.append(f"trim {token(value)}")
    for value in ONELINE_INPUTS:
        lines.append(f"oneline {token(value)}")
    for value, size, fill in PAD_CASES:
        lines.append(f"lpad {token(value)} {size} {ord(fill)}")
    for value, size in LPAD0_CASES:
        lines.append(f"lpad0 {token(value)} {size}")
    for value, size, fill in RPAD_CASES:
        lines.append(f"rpad {token(value)} {size} {ord(fill)}")
    for value, chars in REPLACE_CASES:
        lines.append(f"replace {token(value)} {token(chars)}")
    for delimiter, parts in IMPLODE_CASES:
        lines.append(" ".join(["implode", str(delimiter), *parts]).strip())
    for value, delimiter in SPLIT_CASES:
        lines.append(f"split {token(value)} {delimiter}")
    for value, delimiter, max_parts in FULLSPLIT_CASES:
        lines.append(f"fullsplit {token(value)} {delimiter} {max_parts}")
    lines.append(f"pred {PRED_BYTES}")
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
    script_path = WORK / "string_script.txt"
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
            ("lower in=4142 out=6162", "to_lower of AB"),
            ("lower in=80ff out=80ff", "to_lower must not touch bytes >= 0x80"),
            ("upper in=61626358595a out=41424358595a", "to_upper of abcXYZ"),
            ("trim in=0c610c out=0c610c", "form feed is not in is_space, so nothing is trimmed"),
            ("trim in=0b610b out=61", "vertical tab is in is_space"),
            ("trim in=006100 out=61", "NUL is in is_space"),
            ("trim in=20 out=-", "a single space trims to nothing"),
            ("trim in=090a20616263200a09 out=616263", "tab/newline/space padding"),
            ("oneline in=2020610a2020622020 out=612062", "oneline collapse"),
            ("oneline in=610d0a62 out=612062", "CRLF is one break"),
            ("oneline in=0a out=-", "a lone newline folds to nothing"),
            ("lpad in=616263 size=5 out=3030616263", "lpad pads on the left"),
            ("lpad in=616263 size=2 out=616263", "lpad never truncates"),
            ("lpad0 in=3432 size=4 out=30303432", "lpad0"),
            ("rpad in=6162 size=4 out=61627878", "rpad pads on the right"),
            ("replace in=612d622d63 chars=2d out=6120622063", "replace_with_spaces"),
            ("replace in=616263 chars=616263 out=202020", "every byte replaced"),
            ("replace in=616263 chars=- out=616263", "an empty character set changes nothing"),
            ("implode delim=32 parts=61|62|63 out=6120622063", "implode"),
            ("implode delim=44 parts=none out=-", "implode of nothing"),
            ("split in=612c62 delim=44 a=61 b=62", "split at the delimiter"),
            ("split in=616263 delim=44 a=616263 b=-", "split without a delimiter"),
            ("split in=2c61 delim=44 a=- b=61", "split at position 0"),
            ("fullsplit in=612c622c delim=44 max=4294967295 parts=61|62|-", "trailing delimiter adds an empty part"),
            ("fullsplit in=- delim=44 max=5 parts=none", "an empty input has no parts"),
            ("fullsplit in=612c622c63 delim=44 max=2 parts=61|622c63", "max_parts keeps the rest"),
            ("fullsplit in=612c622c63 delim=44 max=1 parts=612c622c63", "max_parts 1 means no split"),
            ("pred b=0b space=1 alpha=0 digit=0 alnum=0 hex=0", "vertical tab"),
            ("pred b=0c space=0 alpha=0 digit=0 alnum=0 hex=0", "form feed"),
            ("pred b=5f space=0 alpha=0 digit=0 alnum=0 hex=0", "underscore is not alnum"),
            ("pred b=80 space=0 alpha=0 digit=0 alnum=0 hex=0", "0x80 is not alpha"),
            ("pred b=46 space=0 alpha=1 digit=0 alnum=1 hex=1", "F is alpha and a hex digit"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        print(f"SELFTEST OK: {len(checks)} hand-checked cells, including the \\f/\\v/\\0 trim set")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())