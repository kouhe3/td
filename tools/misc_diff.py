"""Compare the simple string sanitizers in td/telegram/misc.cpp with TDLib.

Usage:
    uv run python tools/misc_diff.py
    uv run python tools/misc_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary.
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
DEFAULT_CPP = "C:/tmp/td-probe/misc_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/misc_probe.exe"

LRM = bytes([0xE2, 0x80, 0x8E])
RLM = bytes([0xE2, 0x80, 0x8F])
ZWNJ = bytes([0xE2, 0x80, 0x8C])


def cases() -> list[list[str]]:
    utf8 = "ИМЯ Пример".encode()
    return [
        ["username", ""],
        ["username", b".".hex()],
        ["username", b"AbC".hex()],
        ["username", b"  .AbC.  ".hex()],
        ["username", b"A.B.C".hex()],
        ["username", b"\t\n.Nick Name\r\0".hex()],
        ["username", utf8.hex()],
        ["username", bytes([0xFF, 0x2E, 0x41]).hex()],
        ["phone", ""],
        ["phone", b"123".hex()],
        ["phone", b"+1 (23) 45-67".hex()],
        ["phone", "١٢٣٤".encode().hex()],
        ["phone", b"a1b2c3".hex()],
        ["offending", ""],
        ["offending", LRM.hex()],
        ["offending", RLM.hex()],
        ["offending", (LRM + RLM).hex()],
        ["offending", (LRM + RLM + LRM).hex()],
        ["offending", (b"A" + RLM + RLM + b"B").hex()],
        ["offending", (b"A" + LRM + RLM + b"B" + RLM).hex()],
        ["offending", (ZWNJ + LRM).hex()],
        ["offending", bytes([0xE2, 0x80]).hex()],
        ["offending", bytes([0xE2]).hex()],
        ["offending", (b"x" + LRM + RLM + LRM + RLM + b"y").hex()],
    ]


def run(binary: str, argv: list[str], env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, *argv], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


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

    table = cases()
    digest = hashlib.md5("\x1f".join(":".join(case) for case in table).encode()).hexdigest()
    print(f"corpus md5={digest} cases={len(table)}")

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    for argv in table:
        cpp_rc, cpp_out = run(args.cpp, argv, env)
        rs_rc, rs_out = run(args.rs, argv, env)
        if cpp_rc != rs_rc or cpp_out != rs_out:
            print(f"DIFF {' '.join(argv)}: oracle={cpp_out.strip()!r} rust={rs_out.strip()!r}")
            return 1

    single = table[0]
    if run(args.cpp, single, env)[1] != run(args.cpp, single, env)[1]:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        left = run(args.cpp, ["username", b"AbC".hex()], env)[1]
        right = run(args.cpp, ["username", b"AbD".hex()], env)[1]
        if left == right:
            print("SELFTEST FAIL: different inputs produced the same output")
            return 1
        print("SELFTEST OK: comparator sees a one-byte change")

    print(f"PASS cases={len(table)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())