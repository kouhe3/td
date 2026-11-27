"""Compare TDLib's generated maintenance-message codec with the Rust probe.

Both binaries take the same argv and must print the same bytes. Store modes pin
`fields` and `full`; parse modes pin abstract dispatch (BadMsgNotification /
MsgDetailedInfo) and the parser error texts.

Usage:
    uv run python tools/svc_msg_diff.py
    uv run python tools/svc_msg_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/svc_msg_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/svc_msg_probe.exe"

I64_MAX = "9223372036854775807"
I64_MIN = "-9223372036854775808"

STORE_CASES = [
    ["bmn", "1234567890123", "7", "16"],
    ["bmn", "0", "0", "0"],
    ["bmn", "-1", "-2147483648", "2147483647"],
    ["bmn", I64_MAX, "0", "0"],
    ["bss", "1234567890123", "7", "48", "9876543210987"],
    ["bss", I64_MIN, "2147483647", "-1", I64_MAX],
    ["nsc", "111", "222", "333"],
    ["nsc", "0", "0", "-1"],
    ["pong", "444", "555"],
    ["pong", I64_MAX, I64_MIN],
    ["resend", ""],
    ["resend", "1"],
    ["resend", "1,2,3"],
    ["resend", f"-1,{I64_MAX},{I64_MIN}"],
    ["mdinfo", "10", "20", "30", "40"],
    ["mdinfo", I64_MIN, I64_MAX, "-2147483648", "2147483647"],
    ["mdnew", "21", "31", "41"],
]

# Negative parse cases: unknown constructor, short fields, trailing bytes.
NEGATIVE_CASES = [
    ["parse_bmn", "39300000"],
    ["parse_bmn", "a7eff811cb04"],
    ["parse_bmn", "a7eff811cb04fb711f010000070000001000000000"],
    ["parse_bmn", ""],
    ["parse_mdi", "dfe69d80"],
    ["parse_mdi", "c63e6d270a0000000000000014"],
]

PARSE_OF = {"bmn": "parse_bmn", "bss": "parse_bmn", "mdinfo": "parse_mdi", "mdnew": "parse_mdi"}


def run(binary: str, argv: list[str], env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, *argv], capture_output=True, env=env)
    # printf on Windows translates \n to \r\n; Rust writes bytes as-is.
    text = (proc.stdout + proc.stderr).decode("utf-8", "replace").replace("\r\n", "\n")
    return proc.returncode, text


def kv(text: str) -> dict[str, str]:
    out = {}
    for line in text.splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            out[key.strip()] = value.strip()
    return out


def store_record(text: str) -> dict[str, str]:
    """The store line is `id=… fields=… full=…` on a single line."""
    return dict(re.findall(r"(\w+)=(\S+)", text))


def compare(cpp: str, rs: str, argv: list[str], env: dict[str, str], expect_parse: str | None = None) -> str | None:
    """Returns None on match, otherwise a description of the first divergence."""
    cpp_rc, cpp_out = run(cpp, argv, env)
    rs_rc, rs_out = run(rs, argv, env)
    if cpp_rc != rs_rc:
        return f"exit code oracle={cpp_rc} rust={rs_rc}"
    if cpp_out != rs_out:
        cpp_lines = cpp_out.splitlines()
        rs_lines = rs_out.splitlines()
        for index, (left, right) in enumerate(zip(cpp_lines, rs_lines)):
            if left != right:
                return f"line {index + 1}: oracle={left!r} rust={right!r}"
        return f"line count oracle={len(cpp_lines)} rust={len(rs_lines)}"
    if expect_parse is not None:
        parsed = kv(cpp_out)
        wanted = {name: value for name, value in zip(expect_parse.split(","), argv[2:])}
        for name, value in wanted.items():
            if parsed.get(name) != value:
                return f"roundtrip field {name}: want={value} got={parsed.get(name)}"
    return None


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

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    checked = 0
    fields_seen = 0
    for argv in STORE_CASES:
        problem = compare(args.cpp, args.rs, argv, env)
        if problem:
            print(f"DIFF {' '.join(argv)}: {problem}")
            return 1
        checked += 1
        _, oracle_out = run(args.cpp, argv, env)
        record = store_record(oracle_out)
        for key in ("id", "fields", "full"):
            if key not in record:
                print(f"FAIL {' '.join(argv)}: oracle output lacks {key}")
                return 1
        fields_seen += len(record["fields"]) // 2

        # Both directions of the roundtrip through the abstract fetch.
        parse_mode = PARSE_OF.get(argv[0])
        if parse_mode is None:
            continue
        field_names = {
            "bmn": "bad_msg_id,bad_msg_seqno,error_code",
            "bss": "bad_msg_id,bad_msg_seqno,error_code,new_server_salt",
            "mdinfo": "msg_id,answer_msg_id,bytes,status_code",
            "mdnew": "answer_msg_id,bytes,status_code",
        }[argv[0]]
        problem = compare(args.cpp, args.rs, [parse_mode, record["full"]], env, expect_parse=field_names)
        if problem:
            print(f"DIFF {parse_mode} {' '.join(argv)}: {problem}")
            return 1
        checked += 1
        # The oracle must parse the bytes the Rust probe produced, too.
        rust_rc, rust_out = run(args.rs, argv, env)
        if rust_rc != 0:
            print(f"FAIL rust {' '.join(argv)}: rc={rust_rc}")
            return 1
        rust_record = store_record(rust_out)
        problem = compare(args.cpp, args.rs, [parse_mode, rust_record["full"]], env, expect_parse=field_names)
        if problem:
            print(f"DIFF rust->oracle {parse_mode} {' '.join(argv)}: {problem}")
            return 1
        checked += 1

    for argv in NEGATIVE_CASES:
        problem = compare(args.cpp, args.rs, argv, env)
        if problem:
            print(f"DIFF {' '.join(argv)}: {problem}")
            return 1
        checked += 1

    if args.selftest:
        # Change one field of a stored record: the comparator must see it.
        original = run(args.cpp, ["bss", "1", "2", "3", "4"], env)[1]
        tampered = run(args.cpp, ["bss", "1", "2", "3", "5"], env)[1]
        if original == tampered:
            print("SELFTEST FAIL: tamper did not change the encoded bytes")
            return 1
        print("SELFTEST OK: one-field change changes the bytes")

    print(f"PASS cases={checked} fields_bytes={fields_seen}")
    return 0


if __name__ == "__main__":
    sys.exit(main())