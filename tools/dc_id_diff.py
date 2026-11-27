"""Compare td/telegram/net/DcId with TDLib.

    uv run python tools/dc_id_diff.py
    uv run python tools/dc_id_diff.py --selftest

DcId is a value type: a raw id, the main sentinel (-1), the invalid sentinel
(-2), the empty sentinel (0), and an external flag. `create` validates its
input, `from_value` does not, and `operator<<` picks one of five texts by
branch order. The script walks the int32 boundary values plus a fixed random
sample, then the comparison operators in three constructions.
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
DEFAULT_CPP = "C:/tmp/td-probe/dc_id_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/dc_id_probe.exe"
WORK = REPO / "work" / "dcid"

INT32_MIN = -(2**31)
INT32_MAX = 2**31 - 1

BOUNDARY = [INT32_MIN, -1001, -1000, -3, -2, -1, 0, 1, 2, 999, 1000, 1001, INT32_MAX]
VALID = [1, 2, 3, 999, 1000]
ORDER = [-2, -1, 0, 1, 2, 1000, 1001]

# Every printer branch must be reachable from the corpus.
BRANCHES = ["str=DcId{invalid}", "str=DcId{empty}", "str=DcId{is_empty}", "str=DcId{main}", "str=DcId{2 external}"]


def script() -> str:
    values = list(BOUNDARY)
    rng = random.Random(20261127)
    values += [rng.randint(INT32_MIN, INT32_MAX) for _ in range(40)]
    lines = ["# static is_valid, then the three constructors"]
    for value in values:
        lines.append(f"is_valid {value}")
    for value in values:
        lines.append(f"create {value}")
    for value in values:
        lines.append(f"from_value {value}")
    for value in VALID:
        lines.append(f"internal {value}")
        lines.append(f"external {value}")
    lines.append("# comparisons, including the external flag")
    for left in ORDER:
        for right in ORDER:
            lines.append(f"cmp {left} {right}")
    for left in VALID:
        for right in VALID:
            lines.append(f"cmp_external {left} {right}")
            lines.append(f"cmp_mixed {left} {right}")
    lines.append("# get_raw_id on exact ids only")
    for value in [1, 2, 999, 1000, INT32_MAX]:
        lines.append(f"raw {value}")
    return "\n".join(lines) + "\n"


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    # printf on Windows writes \r\n; Rust writes bytes as-is.
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
    script_path = WORK / "dcid_script.txt"
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
        for branch in BRANCHES:
            if branch not in cpp_out:
                print(f"SELFTEST FAIL: no corpus line reaches {branch}")
                return 1
        # One flipped predicate must be caught by the same comparator.
        lines = cpp_out.splitlines()
        index = next(index for index, line in enumerate(lines) if "static=1" in line)
        lines[index] = lines[index].replace("static=1", "static=0")
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = compare(0, "\n".join(lines) + "\n", 0, cpp_out)
        if status != 1:
            print(f"SELFTEST FAIL: the comparator missed a flipped is_valid result\n{captured.getvalue()}")
            return 1
        print(f"SELFTEST OK: {len(BRANCHES)} printer branches present, flipped predicate caught")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())