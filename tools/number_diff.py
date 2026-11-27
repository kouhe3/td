"""Compare the integer parsing helpers in tdutils misc.h with TDLib.

    uv run python tools/number_diff.py
    uv run python tools/number_diff.py --selftest

The signed parser accumulates in the unsigned type (wrapping modulo 2^bits),
negates once when the value exceeds T::max, and clamps to T::min if the negated
value is still too large — so `to_integer<int32>("4294967296")` is 0 and
`("2147483649")` is -2147483647. `+` is not a sign, parsing stops at the first
non-digit, and the `*_safe` variants re-print the value and reject anything that
does not round-trip. The hex family adds three exact error messages.
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
DEFAULT_CPP = "C:/tmp/td-probe/number_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/number_probe.exe"
WORK = REPO / "work" / "number"

PARSE_INPUTS = [
    "0", "1", "42", "-1", "-42", "-", "", "2147483647", "2147483648", "2147483649",
    "4294967295", "4294967296", "4294967297", "-2147483648", "-2147483649",
    "-4294967296", "9223372036854775807", "9223372036854775808",
    "18446744073709551615", "18446744073709551616", "000123", "12abc", " 5",
    "+5", "007", "abc", "99999999999999999999999",
]
HEX_INPUTS = [
    "", "0", "f", "ff", "ffffffff", "FFFFFFFF", "100000000", "ffffffffffffffff",
    "10000000000000000", "1G", "g", "abc", "0x10", "ab", " 1", "-1",
]
DECODE_INPUTS = ["", "00", "ff", "a5", "abc", "g0", "GG", "0x", "ABCDEF"]
ENCODE_INPUTS = ["", "00", "0f", "a5", "ff", "00ff"]


def token(text: str) -> str:
    return text.encode().hex() if text else "-"


def script() -> str:
    lines = ["# parse (unsafe), parse (safe), hex parse, hex decode/encode"]
    for value in PARSE_INPUTS:
        for op in ("i32", "i64", "u32", "u64"):
            lines.append(f"{op} {token(value)}")
    for value in PARSE_INPUTS:
        for op in ("i32s", "i64s", "u32s", "u64s"):
            lines.append(f"{op} {token(value)}")
    for value in HEX_INPUTS:
        for op in ("hx32", "hx64", "hxs32", "hxs64"):
            lines.append(f"{op} {token(value)}")
    for value in DECODE_INPUTS:
        lines.append(f"hdec {token(value)}")
    for value in ENCODE_INPUTS:
        # henc takes the bytes to encode, so the corpus entry is already hex.
        lines.append(f"henc {value if value else '-'}")
    # 0xFF followed by '5': the unsafe parser returns 0 and the safe one reports
    # the UTF-8 branch of get_to_integer_safe_error instead of the "Can't parse"
    # message.
    lines.append("i32s ff35")
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


def err(message: str) -> str:
    return message.encode().hex()


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
    script_path = WORK / "number_script.txt"
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
            (f"i32 in={token('2147483647')} out=2147483647", "the int32 maximum"),
            (f"i32 in={token('2147483648')} out=-2147483648", "2^31 clamps to INT32_MIN"),
            (f"i32 in={token('2147483649')} out=-2147483647", "2^31+1 wraps to -2147483647"),
            (f"i32 in={token('4294967296')} out=0", "2^32 wraps to 0"),
            (f"i32 in={token('-2147483649')} out=2147483647", "negative overflow flips the sign"),
            (f"i32 in={token('+5')} out=0", "'+' is not a sign character"),
            (f"i32 in={token(' 5')} out=0", "a leading space stops the parse"),
            (f"i32 in={token('12abc')} out=12", "parsing stops at the first non-digit"),
            (f"i32 in={token('-')} out=0", "a lone minus is zero"),
            (f"u32 in={token('-1')} out=0", "the unsigned parser has no sign"),
            (f"i64 in={token('18446744073709551616')} out=0", "2^64 wraps to 0"),
            (f"i32s in={token('0')} ok=1 out=0", "safe parse of 0"),
            (f"i32s in={token('007')} ok=0 err={err('Can\'t parse as an integer string \"007\"')}",
             "leading zeros do not round-trip"),
            (f"i32s in={token('2147483649')} ok=0 err={err('Can\'t parse as an integer string \"2147483649\"')}",
             "the safe parser rejects the wrap"),
            (f"i32s in=ff35 ok=0 err={err('Strings must be encoded in UTF-8')}",
             "the UTF-8 branch of the error"),
            (f"hx32 in={token('1G')} out=1", "hex parsing stops at the first non-hex digit"),
            (f"hx32 in={token('FFFFFFFF')} out=4294967295", "full hex word"),
            (f"hxs32 in={token('ffffffff')} ok=1 out=4294967295", "safe hex accepts the maximum"),
            (f"hxs32 in={token('100000000')} ok=0 err={err('String hex number overflows')}", "hex overflow"),
            (f"hxs32 in=- ok=0 err={err('String is empty')}", "empty hex"),
            (f"hxs32 in={token('1G')} ok=0 err={err('String contains non-hex digit')}", "non-hex digit"),
            (f"hdec in={token('a5')} ok=1 out=a5", "hex decode"),
            (f"hdec in={token('abc')} ok=0 err={err('Wrong hex string length')}", "odd length"),
            (f"hdec in={token('g0')} ok=0 err={err('Wrong hex string')}", "bad digit"),
            ("henc in=a5 out=a5", "hex encode is lowercase"),
            ("henc in=- out=-", "hex encode of nothing"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        print(f"SELFTEST OK: {len(checks)} hand-checked cells (wrap, clamp, safe round-trip, hex errors)")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())