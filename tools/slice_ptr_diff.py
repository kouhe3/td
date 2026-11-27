"""Compare tdutils Slice/MutableSlice accessors and pointer consistency with TDLib.

    uv run python tools/slice_ptr_diff.py
    uv run python tools/slice_ptr_diff.py --selftest

Two criteria again, because `back()` is CHECKed on an empty slice:

* values and pointer arithmetic go through a script (byte for byte, including
  the byte offsets of data/begin/end/ubegin/uend after `remove_prefix`);
* `back()` on an empty slice aborts, so those cases run one process each and
  only "did it return" is compared. In-range `n == size` controls keep the
  abort lane honest.

`operator[]` out of range is UB in td, not a contract, so it is never fed.
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
DEFAULT_CPP = "C:/tmp/td-probe/slice_ptr_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/slice_ptr_probe.exe"
WORK = REPO / "work" / "slice"

PAYLOADS = {
    "e": "-",
    "a": "61",
    "ab": "6162",
    "abc": "616263",
    "abcabc": "616263616263",
    "nul": "00",
    "hi": "80",
    "ff00": "ff00",
}

# (op, len, must_abort). len == 0 is the CHECK edge; the rest are controls.
CHECK_CASES = [("back", 0, True), ("mback", 0, True), ("back", 1, False), ("back", 8, False), ("mback", 5, False)]


def length_of(payload: str) -> int:
    return 0 if payload == "-" else len(payload) // 2


def script() -> str:
    lines = ["# values, then the same window after remove_prefix"]
    for name, payload in PAYLOADS.items():
        length = length_of(payload)
        lines.append(f"new p_{name} {payload}")
        lines.append(f"mnew q_{name} {payload}")
        lines.append(f"ptr p_{name}")
        lines.append(f"mptr q_{name}")
        if length == 0:
            continue
        lines.append(f"back p_{name}")
        lines.append(f"mback q_{name}")
        for index in (0, length - 1):
            lines.append(f"index p_{name} {index}")
            lines.append(f"mindex q_{name} {index}")
        if length < 2:
            continue
        for shift in (1, length - 1):
            lines.append(f"new s_{name}_{shift} {payload}")
            lines.append(f"shift s_{name}_{shift} {shift}")
            lines.append(f"ptr s_{name}_{shift}")
            lines.append(f"back s_{name}_{shift}")
            lines.append(f"index s_{name}_{shift} 0")
            lines.append(f"mnew t_{name}_{shift} {payload}")
            lines.append(f"mshift t_{name}_{shift} {shift}")
            lines.append(f"mptr t_{name}_{shift}")
            lines.append(f"mback t_{name}_{shift}")
            lines.append(f"mindex t_{name}_{shift} 0")
    return "\n".join(lines) + "\n"


def run_script(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


def run_check(binary: str, case: tuple, env: dict[str, str]) -> tuple[bool, str]:
    op, length, _ = case
    proc = subprocess.run([binary, "--check", op, str(length)], capture_output=True, env=env)
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
    script_path = WORK / "slice_ptr_script.txt"
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

    for case in CHECK_CASES:
        op, length, must_abort = case
        cpp_aborted, cpp_text = run_check(args.cpp, case, env)
        rs_aborted, rs_text = run_check(args.rs, case, env)
        if cpp_aborted != must_abort:
            print(f"FLOOR FAIL: oracle {op} len={length} aborted={cpp_aborted}, expected {must_abort}")
            return 1
        if cpp_aborted != rs_aborted or cpp_text != rs_text:
            print(
                f"DIFF check {op} len={length}: "
                f"oracle aborted={cpp_aborted} out={cpp_text!r} / rust aborted={rs_aborted} out={rs_text!r}"
            )
            return 1

    if args.selftest:
        checks = [
            ("back p_abcabc size=6 value=99", "back() is not the last byte"),
            ("index p_abcabc i=0 value=97", "operator[](0) is not the first byte"),
            ("index p_abcabc i=5 value=99", "operator[](size-1) is not the last byte"),
            (
                "ptr s_abcabc_1 size=5 off=1 endoff=6 uboff=1 ueoff=6 same_db=1 same_ub=1",
                "the pointer four-tuple did not move with remove_prefix",
            ),
            ("index s_abcabc_1 i=0 value=98", "operator[] ignores the moved window"),
            ("back s_abcabc_5 size=1 value=99", "a one-byte tail window is wrong"),
        ]
        for expected, failure in checks:
            if expected not in cpp_out:
                print(f"SELFTEST FAIL: {failure} (missing {expected!r})")
                return 1
        # One flipped value must be caught by the same comparator.
        lines = cpp_out.splitlines()
        index = next(index for index, line in enumerate(lines) if " value=" in line)
        lines[index] = lines[index].replace(" value=", " value=9", 1)
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            status = compare(0, "\n".join(lines) + "\n", 0, cpp_out)
        if status != 1:
            print(f"SELFTEST FAIL: the comparator missed a flipped value\n{captured.getvalue()}")
            return 1
        print("SELFTEST OK: back/index values, moved window, flipped value caught, abort lane has controls")

    print(f"PASS lines={len(cpp_out.splitlines())} checks={len(CHECK_CASES)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())