"""Compare tdutils SliceHash with TDLib.

    uv run python tools/slice_hash_diff.py
    uv run python tools/slice_hash_diff.py --selftest

The hash is `result: uint32 = 0` accumulated as `result * 123456789 + c`, so it
wraps modulo 2^32 and the byte is a signed `char` (0x80 contributes 0xFFFFFF80).
The script hashes each payload twice — directly and through a mutable view — and
the selftest recomputes every value with an independent Python formula, so the
oracle is checked against the documented algorithm and not just against Rust.
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
DEFAULT_CPP = "C:/tmp/td-probe/slice_hash_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/slice_hash_probe.exe"
WORK = REPO / "work" / "hash"

MUL = 123456789

PAYLOADS = {
    "e": "-",
    "nul": "00",
    "one": "01",
    "lo": "7f",
    "hi": "80",
    "ff": "ff",
    "a": "61",
    "ab": "6162",
    "ba": "6261",
    "abc": "616263",
    "abcabc": "616263616263",
    "hi_seq": "808080",
    "zeros32": "00" * 32,
    "ff100": "ff" * 100,
}


def expected_hash(payload: str) -> int:
    data = b"" if payload == "-" else bytes.fromhex(payload)
    result = 0
    for byte in data:
        signed = byte - 256 if byte >= 128 else byte
        result = (result * MUL + signed) & 0xFFFFFFFF
    return result


def script() -> str:
    lines = ["# every payload twice: std::map-style Slice, then MutableSlice -> Slice"]
    for name, payload in PAYLOADS.items():
        lines.append(f"new {name} {payload}")
    for name in PAYLOADS:
        lines.append(f"hash {name}")
        lines.append(f"mhash {name}")
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


def hash_by_name(output: str) -> dict[str, list[int]]:
    values: dict[str, list[int]] = {}
    for line in output.splitlines():
        parts = line.split(" ")
        if len(parts) < 4 or " h=" not in line:
            continue
        name = parts[1]
        value = int(line.split("h=")[1])
        values.setdefault(name, []).append(value)
    return values


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
    script_path = WORK / "slice_hash_script.txt"
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
        observed = hash_by_name(cpp_out)
        for name, payload in PAYLOADS.items():
            wanted = expected_hash(payload)
            got = observed.get(name, [])
            if got != [wanted, wanted]:
                print(f"SELFTEST FAIL: {name} hashed to {got!r}, the formula says {wanted}")
                return 1
        if expected_hash("-") != 0:
            print("SELFTEST FAIL: the empty payload does not hash to 0")
            return 1
        # One flipped hash must be caught by the same comparator.
        lines = cpp_out.splitlines()
        index = next(index for index, line in enumerate(lines) if " h=" in line)
        head, _, tail = lines[index].rpartition("h=")
        lines[index] = head + "h=" + ("0" if tail != "0" else "1")
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = compare(0, "\n".join(lines) + "\n", 0, cpp_out)
        if status != 1:
            print(f"SELFTEST FAIL: the comparator missed a flipped hash\n{captured.getvalue()}")
            return 1
        print("SELFTEST OK: oracle matches an independent formula on all payloads, flipped hash caught")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())