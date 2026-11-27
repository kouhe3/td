"""Compare tdutils Slice's view operations with TDLib.

    uv run python tools/slice_diff.py
    uv run python tools/slice_diff.py --selftest

Two criteria, because the surface has two kinds of edge:

* in-range ops (`remove_prefix`, `remove_suffix`, `substr`, `truncate`) are
  driven by a script and compared byte for byte, including `off=` (how far the
  view moved from the start of the buffer);
* the CHECK edges (`n > size`, `from > size`) abort the process, so each case
  runs as its own process and only "did it return" is compared. A control case
  with `n == size` must not abort — that is what makes the abort lane a lane
  rather than a constant.
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
DEFAULT_CPP = "C:/tmp/td-probe/slice_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/slice_probe.exe"
WORK = REPO / "work" / "slice"

# Length -> payload hex ("abcdefgh" prefixes).
PAYLOADS = {0: "-", 1: "61", 2: "6162", 5: "6162636465", 8: "6162636465666768"}

# (op, len, a, b, must_abort). n == len is the control: it must return.
CHECK_CASES = [
    ("prefix", 5, 5, 0, False),
    ("suffix", 5, 5, 0, False),
    ("substr", 5, 5, 0, False),
    ("substr2", 5, 5, 0, False),
    ("trunc", 5, 9, 0, False),
]
for _length in (0, 1, 5):
    CHECK_CASES += [
        ("prefix", _length, _length + 1, 0, True),
        ("suffix", _length, _length + 1, 0, True),
        ("substr", _length, _length + 1, 0, True),
        ("substr2", _length, _length + 1, 0, True),
    ]


def script() -> str:
    lines = ["# one `new` per case, because every op mutates in place"]
    for length, payload in PAYLOADS.items():
        # Script mode must stay inside the CHECK domain: n > size aborts.
        legal = sorted({n for n in (0, 1, length // 2, length - 1, length) if 0 <= n <= length})
        for n in legal:
            lines.append(f"new p{length}_{n} {payload}")
            lines.append(f"prefix p{length}_{n} {n}")
            lines.append(f"new q{length}_{n} {payload}")
            lines.append(f"suffix q{length}_{n} {n}")
        for n in sorted({n for n in (0, 1, length // 2, length - 1, length, length + 1, length + 3) if n >= 0}):
            lines.append(f"new t{length}_{n} {payload}")
            lines.append(f"trunc t{length}_{n} {n}")
        for frm in legal:
            lines.append(f"new s{length}_{frm} {payload}")
            lines.append(f"substr s{length}_{frm} {frm}")
        for frm in sorted({n for n in (0, 1, length) if 0 <= n <= length}):
            for size in sorted({0, 1, length, length + 3}):
                lines.append(f"new u{length}_{frm}_{size} {payload}")
                lines.append(f"substr2 u{length}_{frm}_{size} {frm} {size}")
    return "\n".join(lines) + "\n"


def run_script(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


def run_check(binary: str, case: tuple, env: dict[str, str]) -> tuple[bool, str]:
    op, length, a, b, _ = case
    command = [binary, "--check", op, str(length), str(a)]
    if op == "substr2":
        command.append(str(b))
    proc = subprocess.run(command, capture_output=True, env=env)
    return proc.returncode != 0, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


def suppress_crash_dialogs() -> None:
    """Without SEM_NOGPFAULTERRORBOX every abort() waits ~4 s for WerFault.exe."""
    if sys.platform != "win32":
        return
    import ctypes

    ctypes.windll.kernel32.SetErrorMode(0x0001 | 0x0002 | 0x8000)


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


def compare_checks(args, env: dict[str, str]) -> int:
    for case in CHECK_CASES:
        op, length, a, b, must_abort = case
        cpp_aborted, cpp_out = run_check(args.cpp, case, env)
        rs_aborted, rs_out = run_check(args.rs, case, env)
        if cpp_aborted != must_abort:
            print(f"FLOOR FAIL: oracle {op} len={length} a={a} aborted={cpp_aborted}, expected {must_abort}")
            return 1
        if cpp_aborted != rs_aborted or cpp_out != rs_out:
            print(
                f"DIFF check {op} len={length} a={a} b={b}: "
                f"oracle aborted={cpp_aborted} out={cpp_out!r} / rust aborted={rs_aborted} out={rs_out!r}"
            )
            return 1
    return 0


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
    print(f"corpus md5={digest} lines={len(text.splitlines())} checks={len(CHECK_CASES)}")

    WORK.mkdir(parents=True, exist_ok=True)
    script_path = WORK / "slice_script.txt"
    script_path.write_text(text)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")
    suppress_crash_dialogs()

    cpp_rc, cpp_out = run_script(args.cpp, script_path, env)
    rs_rc, rs_out = run_script(args.rs, script_path, env)
    status = compare(cpp_rc, cpp_out, rs_rc, rs_out)
    if status != 0:
        return status

    if run_script(args.cpp, script_path, env) != (cpp_rc, cpp_out):
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if compare_checks(args, env) != 0:
        return 1

    if args.selftest:
        # truncate must be a no-op above the current size: n > size keeps size.
        trunc = [line for line in cpp_out.splitlines() if line.startswith("trunc ")]
        big = [line for line in trunc if re.search(r" n=(\d+) size=(\d+) ", line) and
               int(re.search(r" n=(\d+)", line).group(1)) > int(re.search(r" size=(\d+)", line).group(1))]
        if not big:
            print("SELFTEST FAIL: no truncate case asks for more than the current size")
            return 1
        # substr2 must clamp: from=1, n=len+3 leaves len-1 bytes.
        clamped = [
            line for line in cpp_out.splitlines()
            if line.startswith("substr2 u8_1_11 ") and re.search(r" size=7 ", line)
        ]
        if not clamped:
            print("SELFTEST FAIL: the substr2 clamp case is missing from the oracle output")
            return 1
        # One flipped size must be caught by the same comparator.
        lines = cpp_out.splitlines()
        index = next(index for index, line in enumerate(lines) if " size=" in line)
        lines[index] = re.sub(r" size=(\d+)", lambda m: f" size={int(m.group(1)) + 1}", lines[index], count=1)
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = compare(0, "\n".join(lines) + "\n", 0, cpp_out)
        if status != 1:
            print(f"SELFTEST FAIL: the comparator missed a changed size\n{captured.getvalue()}")
            return 1
        print("SELFTEST OK: truncate no-op and substr2 clamp present, flipped size caught, abort lane has a control")

    print(f"PASS lines={len(cpp_out.splitlines())} checks={len(CHECK_CASES)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())