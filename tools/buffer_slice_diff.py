"""Compare tdutils BufferSlice ownership semantics with TDLib.

    uv run python tools/buffer_slice_diff.py
    uv run python tools/buffer_slice_diff.py --selftest

The interesting part is aliasing: `clone()` shares the allocation and copies
only the window, `copy()` allocates fresh storage, and `as_mutable_slice()`
does not copy on write. The script exercises both, plus window movement
(`confirm_read`, `truncate`, `after`) and the empty slice.
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
DEFAULT_CPP = "C:/tmp/td-probe/buffer_slice_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/buffer_slice_probe.exe"
WORK = REPO / "work" / "buffer"


def script(alias_is_clone: bool = True) -> str:
    second = "clone" if alias_is_clone else "copy"
    return "\n".join(
        [
            "# alias vs deep copy",
            "new s 6162636465",
            "print s",
            f"{second} c s",
            "print c",
            "mutate c 0 120",
            "print s",
            "print c",
            "copy d s",
            "mutate d 1 121",
            "print s",
            "print d",
            "clone e c",
            "mutate e 2 122",
            "print s",
            "print e",
            "# window movement",
            "read s 1",
            "print s",
            "mutate s 0 88",
            "print s",
            "print c",
            "read s 1",
            "print s",
            "truncate s 1",
            "print s",
            "truncate s 100",
            "print s",
            "after s 0",
            "after s 1",
            "# empty slice",
            "new z",
            "print z",
            "clone z2 z",
            "print z2",
            "copy z3 z",
            "print z3",
        ]
    ) + "\n"


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    # printf on Windows writes \r\n; Rust writes bytes as-is.
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

    text = script()
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    WORK.mkdir(parents=True, exist_ok=True)
    script_path = WORK / "slice_script.txt"
    script_path.write_text(text)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    cpp_rc, cpp_out = run(args.cpp, script_path, env)
    rs_rc, rs_out = run(args.rs, script_path, env)
    if cpp_rc != rs_rc or cpp_out != rs_out:
        for index, (left, right) in enumerate(zip(cpp_out.splitlines(), rs_out.splitlines())):
            if left != right:
                print(f"DIFF line {index + 1}: oracle={left!r} rust={right!r}")
                return 1
        print(f"DIFF line count: oracle={len(cpp_out.splitlines())} rust={len(rs_out.splitlines())}")
        return 1

    if run(args.cpp, script_path, env)[1] != cpp_out:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        # Turning the first clone() into copy() must change the aliasing lines.
        other = WORK / "slice_script_copy.txt"
        other.write_text(script(alias_is_clone=False))
        other_out = run(args.cpp, other, env)[1]
        if other_out == cpp_out:
            print("SELFTEST FAIL: clone and copy produced the same trace")
            return 1
        print("SELFTEST OK: comparator sees an aliasing change")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())