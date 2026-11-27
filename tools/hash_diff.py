"""Compare td/utils/Hash.h with TDLib, for the TD_HAVE_ABSL 0 build.

    uv run python tools/hash_diff.py
    uv run python tools/hash_diff.py --selftest

`AbslHash` is `TdHash` in this configuration, which reduces to `size_t(v)`:
a sign-extending integer conversion. The corpus walks the signed/unsigned
boundaries (int32(-1) vs uint32(0xFFFFFFFF)), the signed `char`, pairs (whose
halves XOR, so order does not matter), and the `Hasher` init/combine chain.

If the oracle is ever rebuilt with absl, `AbslHash` becomes `absl::Hash`, which
is seeded per process; the byte criterion then no longer applies.
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import io
import os
import random
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/hash_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/hash_probe.exe"
WORK = REPO / "work" / "hash"

INT32_MIN = -(2**31)
INT32_MAX = 2**31 - 1
INT64_MIN = -(2**63)
INT64_MAX = 2**63 - 1
UINT64_MAX = 2**64 - 1


def script() -> str:
    rng = random.Random(20261127)
    lines = ["# signed and unsigned boundaries, then pairs and the Hasher chain"]
    i32_values = [0, 1, -1, 2, -2, INT32_MAX, INT32_MIN, 12345, -12345]
    i32_values += [rng.randint(INT32_MIN, INT32_MAX) for _ in range(8)]
    i64_values = [0, 1, -1, INT64_MAX, INT64_MIN, 2**32, -(2**32)]
    i64_values += [rng.randint(INT64_MIN, INT64_MAX) for _ in range(6)]
    for value in i32_values:
        lines.append(f"i32 {value}")
    for value in i64_values:
        lines.append(f"i64 {value}")
    for value in [0, 1, 2147483648, 4294967295]:
        lines.append(f"u32 {value}")
    for value in [0, 1, 2**63, UINT64_MAX]:
        lines.append(f"u64 {value}")
    for value in [0, 1, 2]:
        lines.append(f"b {value}")
    for value in [0, 65, 127, 128, 255]:
        lines.append(f"ch {value}")
        lines.append(f"uch {value}")
    pair_i32 = [(1, 2), (2, 1), (0, 0), (-1, 1), (INT32_MAX, INT32_MIN)]
    pair_i32 += [(rng.randint(INT32_MIN, INT32_MAX), rng.randint(INT32_MIN, INT32_MAX)) for _ in range(4)]
    for first, second in pair_i32:
        lines.append(f"pair i32 {first} {second}")
    for first, second in [(1, 2), (1, 4294967296), (-1, 1)]:
        lines.append(f"pair i64 {first} {second}")
    lines.append("hasher 0")
    lines.append("hasher 7")
    lines.append("hasher 0 1")
    lines.append("hasher 5 1 2 3")
    lines.append(f"hasher {UINT64_MAX} 0")
    lines.append("hasherpair 0 1 2")
    lines.append("hasherpair 5 -1 1")
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


def hash_of_line(output: str, prefix: str) -> str:
    for line in output.splitlines():
        if line.startswith(prefix):
            return line.split("hash=")[1]
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
    script_path = WORK / "hash_script.txt"
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
            ("i32 v=-1 hash=18446744073709551615", "int32(-1) is not sign-extended"),
            ("i64 v=-1 hash=18446744073709551615", "int64(-1) is not sign-extended"),
            ("u32 v=4294967295 hash=4294967295", "uint32(0xFFFFFFFF) is not zero-extended"),
            ("ch v=-1 hash=18446744073709551615", "char is not signed in this build"),
            ("uch v=255 hash=255", "unsigned char(255) is not zero-extended"),
            ("hasher init=7 n=0 hash=7", "an empty Hasher does not return its init"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        swapped = [hash_of_line(cpp_out, "pair i32 a=1 b=2 "), hash_of_line(cpp_out, "pair i32 a=2 b=1 ")]
        if not swapped[0] or swapped[0] != swapped[1]:
            print(f"SELFTEST FAIL: pair hashes are not order-insensitive ({swapped!r})")
            return 1
        # One flipped hash must be caught by the same comparator.
        lines = cpp_out.splitlines()
        index = next(index for index, line in enumerate(lines) if " hash=" in line)
        lines[index] = lines[index][:-1] + ("0" if lines[index][-1] != "0" else "1")
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = compare(0, "\n".join(lines) + "\n", 0, cpp_out)
        if status != 1:
            print(f"SELFTEST FAIL: the comparator missed a flipped hash\n{captured.getvalue()}")
            return 1
        print("SELFTEST OK: sign extension, unsigned zero extension, pair order, Hasher init, flipped hash caught")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())