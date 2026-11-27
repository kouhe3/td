"""Compare tdutils begins_with / ends_with with TDLib.

    uv run python tools/slice_prefix_diff.py
    uv run python tools/slice_prefix_diff.py --selftest

Both are free functions in misc.h, not Slice members. The corpus is every
ordered pair of payloads, so it covers: longer affix (rejected by the length
guard), empty affix (always true — for `ends_with` that means a zero-length
slice at the one-past-end pointer), equal strings, and binary payloads.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/slice_prefix_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/slice_prefix_probe.exe"
WORK = REPO / "work" / "slice"

PAYLOADS = {
    "e": "-",
    "a": "61",
    "b": "62",
    "ab": "6162",
    "ba": "6261",
    "abc": "616263",
    "abcabc": "616263616263",
    "xabc": "78616263",
    "abcx": "61626378",
    "abcd": "61626364",
    "abab": "61626162",
    "nul": "00",
    "nula": "0061",
    "lohi": "7f80",
    "hi": "80",
    "ff": "ff",
    "ff00": "ff00",
}


def script() -> str:
    lines = ["# every ordered pair, both predicates"]
    for name, payload in PAYLOADS.items():
        lines.append(f"new {name} {payload}")
    for hay in PAYLOADS:
        for affix in PAYLOADS:
            lines.append(f"begins {hay} {affix}")
            lines.append(f"ends {hay} {affix}")
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
    script_path = WORK / "slice_prefix_script.txt"
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
            ("begins e e result=1", "empty begins empty"),
            ("begins ab e result=1", "the empty prefix is missing"),
            ("ends ab e result=1", "the empty suffix (one-past-end slice) is missing"),
            ("begins e a result=0", "a longer affix was not rejected"),
            ("ends e a result=0", "a longer suffix was not rejected"),
            ("begins abc abc result=1", "the equal case is missing"),
            ("ends xabc abc result=1", "the tail window is missing"),
            ("begins xabc abc result=0", "the head window is missing"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        # One flipped result bit must be caught by the same comparator.
        lines = cpp_out.splitlines()
        index = next(index for index, line in enumerate(lines) if line.endswith("result=1"))
        lines[index] = lines[index][:-1] + "0"
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = compare(0, "\n".join(lines) + "\n", 0, cpp_out)
        if status != 1:
            print(f"SELFTEST FAIL: the comparator missed a flipped result bit\n{captured.getvalue()}")
            return 1
        print("SELFTEST OK: empty/longer/equal affixes and both windows present, flipped bit caught")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())