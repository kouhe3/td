"""Compare tdutils Slice::find / rfind and the comparison operators with TDLib.

    uv run python tools/slice_cmp_diff.py
    uv run python tools/slice_cmp_diff.py --selftest

All four functions are pure value transforms, so the whole corpus goes through
one script and `npos` is printed as its unsigned value (18446744073709551615)
instead of a special token. The interesting edges are: first vs last hit,
missing byte, empty slice, the unsigned byte order in `<` (0x7f < 0x80), and the
prefix rule (the shorter slice is smaller).
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/slice_cmp_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/slice_cmp_probe.exe"
WORK = REPO / "work" / "slice"

PAYLOADS = {
    "e": "-",
    "a": "61",
    "b": "62",
    "ab": "6162",
    "ba": "6261",
    "abc": "616263",
    "abd": "616264",
    "aab": "616162",
    "aba": "616261",
    "abcabc": "616263616263",
    "nul": "00",
    "nula": "0061",
    "lo": "7f",
    "hi": "80",
    "lohi": "7f80",
    "hiff": "80ff",
    "ff": "ff",
}

# 0x41 and 0x65 are absent from every payload; 0x61/0x62 appear more than once.
CHARS = [0x00, 0x41, 0x61, 0x62, 0x63, 0x65, 0x7F, 0x80, 0xFF]

NPOS = "18446744073709551615"


def script() -> str:
    lines = ["# find/rfind over every payload, then all ordered pairs"]
    for name, payload in PAYLOADS.items():
        lines.append(f"new {name} {payload}")
    for name in PAYLOADS:
        for byte in CHARS:
            lines.append(f"find {name} {byte}")
            lines.append(f"rfind {name} {byte}")
    for left in PAYLOADS:
        for right in PAYLOADS:
            lines.append(f"cmp {left} {right}")
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


def line_of(output: str, prefix: str) -> str:
    for line in output.splitlines():
        if line.startswith(prefix):
            return line
    return ""


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
    script_path = WORK / "slice_cmp_script.txt"
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
            ("find aba c=97 pos=0", "first hit is not index 0"),
            ("rfind aba c=97 pos=2", "last hit is not the final index"),
            (f"find a c=98 pos={NPOS}", "a missing byte did not return npos"),
            (f"find e c=97 pos={NPOS}", "an empty slice did not return npos"),
            ("cmp lo hi eq=0 ne=1 lt=1 gt=0", "0x7f is not ordered below 0x80 (unsigned bytes)"),
            ("cmp ab abc eq=0 ne=1 lt=1 gt=0", "the shorter prefix is not smaller"),
            ("cmp e e eq=1 ne=0 lt=0 gt=0", "two empty slices are not equal"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        # One flipped ordering bit must be caught by the same comparator.
        lines = cpp_out.splitlines()
        index = next(index for index, line in enumerate(lines) if line.startswith("cmp ") and " lt=1 " in line)
        lines[index] = lines[index].replace(" lt=1 ", " lt=0 ", 1)
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = compare(0, "\n".join(lines) + "\n", 0, cpp_out)
        if status != 1:
            print(f"SELFTEST FAIL: the comparator missed a flipped ordering bit\n{captured.getvalue()}")
            return 1
        print("SELFTEST OK: first/last/npos/empty, unsigned order, prefix rule, flipped bit caught")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())