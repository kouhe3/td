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
        # username predicates
        ["valid_username", ""],
        ["valid_username", b"a".hex()],
        ["valid_username", b"_a".hex()],
        ["valid_username", b"a_".hex()],
        ["valid_username", b"a__b".hex()],
        ["valid_username", b"abc_def".hex()],
        ["valid_username", (b"a" * 32).hex()],
        ["valid_username", (b"a" * 33).hex()],
        ["valid_username", b"A_b9".hex()],
        ["valid_username", "Ж".encode().hex()],
        ["allowed_username", ""],
        ["allowed_username", b"abcd".hex()],
        ["allowed_username", b"abcde".hex()],
        ["allowed_username", b"Admin_1".hex()],
        ["allowed_username", b"telegram_x".hex()],
        ["allowed_username", b"support1".hex()],
        ["allowed_username", b"setting".hex()],
        ["allowed_username", b"settings_1".hex()],
        ["allowed_username", b"Admin".hex()],
        ["allowed_username", b"telegraph".hex()],
        # strip_empty_characters / clean_name
        ["strip_empty", b"  a   b  ".hex(), "100"],
        ["clean_name", b"  a   b  ".hex(), "100"],
        ["clean_name", b"a\n\nb".hex(), "100"],
        ["clean_name", ("a" + "\u00a0" + "b").encode().hex(), "100"],
        ["clean_name", ("a" + "\u2003" + "b").encode().hex(), "100"],
        ["clean_name", ("a" + "\u3000" + "b").encode().hex(), "100"],
        ["clean_name", ("a" + "\ufffc" + "b").encode().hex(), "100"],
        ["clean_name", ("a" + "\u202e" + "b").encode().hex(), "100"],
        ["clean_name", ("a" + "\U000f0000" + "b").encode().hex(), "100"],
        ["clean_name", ("中" * 10).encode().hex(), "1"],
        ["clean_name", ("中" * 10).encode().hex(), "3"],
        ["clean_name", ("中" * 10).encode().hex(), "9"],
        ["clean_name", ("中" * 10).encode().hex(), "30"],
        ["clean_name", b"abc".hex(), "0"],
        ["clean_name", b"abc".hex(), "3"],
        ["clean_name", "\u200b".encode().hex(), "100"],
        ["clean_name", "\ufeff".encode().hex(), "100"],
        ["strip_empty", "\u202e".encode().hex(), "100"],
        ["strip_empty", ("a" + "\u202e" + "\u202e" + "b").encode().hex(), "100"],
        # clean_input_string
        ["clean_input", ""],
        ["clean_input", b"a\x00b\x01c".hex()],
        ["clean_input", b"a\nb\rc".hex()],
        ["clean_input", b"\x1f\x20\x7f".hex()],
        ["clean_input", ("a" + "\u200b" + "\u200d" + "\u200e" + "\u200f" + "b").encode().hex()],
        ["clean_input", ("a" + "\u2028" + "\u202e" + "b").encode().hex()],
        ["clean_input", ("a" + "\u0333" + "\u033f" + "\u030a" + "b").encode().hex()],
        ["clean_input", ("a" + "\u0301" + "b").encode().hex()],
        ["clean_input", bytes([0xFF]).hex()],
        ["clean_input", bytes([0xE2, 0x80]).hex()],
        ["clean_input", (LRM + RLM).hex()],
    ]


def long_cases() -> list[tuple[list[str], str]]:
    """clean_input_string's 35000-byte limit does not fit in argv."""
    return [
        (["clean_input", "-"], (b"a" * 34996).hex()),
        (["clean_input", "-"], (b"a" * 34997).hex()),
        (["clean_input", "-"], (b"a" * 35000).hex()),
        (["clean_input", "-"], (b"\r\n" * 18000).hex()),
        (["clean_input", "-"], (("中" * 12000) + "ab").encode().hex()),
        (["clean_name", "-", "10"], ("中" * 20).encode().hex()),
    ]


def run(binary: str, argv: list[str], env: dict[str, str], stdin_data: str | None = None) -> tuple[int, str]:
    proc = subprocess.run(
        [binary, *argv],
        capture_output=True,
        env=env,
        input=(stdin_data.encode() if stdin_data is not None else None),
    )
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
    stdin_table = long_cases()
    shape = [":".join(case) for case in table]
    shape.extend(":".join(argv) + f"|stdin={len(data)}" for argv, data in stdin_table)
    digest = hashlib.md5("\x1f".join(shape).encode()).hexdigest()
    print(f"corpus md5={digest} cases={len(table) + len(stdin_table)}")

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    for argv in table:
        cpp_rc, cpp_out = run(args.cpp, argv, env)
        rs_rc, rs_out = run(args.rs, argv, env)
        if cpp_rc != rs_rc or cpp_out != rs_out:
            print(f"DIFF {' '.join(argv)}: oracle={cpp_out.strip()!r} rust={rs_out.strip()!r}")
            return 1
    for argv, data in stdin_table:
        cpp_rc, cpp_out = run(args.cpp, argv, env, data)
        rs_rc, rs_out = run(args.rs, argv, env, data)
        if cpp_rc != rs_rc or cpp_out != rs_out:
            print(f"DIFF {' '.join(argv)} stdin={len(data)} bytes: oracle={cpp_out.strip()!r} rust={rs_out.strip()!r}")
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