"""Compare tdutils SharedSlice ownership semantics with TDLib.

    uv run python tools/shared_slice_diff.py
    uv run python tools/shared_slice_diff.py --selftest

The interesting part is who owns the bytes: `clone()` shares the allocation and
only bumps the refcount, `UniqueSharedSlice` steals a unique buffer but
deep-copies a shared one, and `clone_as_buffer_slice()` makes an independent
BufferSlice. A default-constructed SharedSlice and `SharedSlice(Slice())` are
indistinguishable through the public API (Slice() points at "" and is_unique()
is private), so the script only checks that both clone and convert to empty.
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
DEFAULT_CPP = "C:/tmp/td-probe/shared_slice_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/shared_slice_probe.exe"
WORK = REPO / "work" / "shared_slice"


def script(shared_source: bool = True) -> str:
    lines = [
        "# null: clone and both conversions stay empty",
        "default dn",
        "print dn",
        "clone dnc dn",
        "print dnc",
        "unique dnu dn",
        "print dnu",
        "to_shared dsh dnu",
        "print dsh",
        "new em",
        "print em",
        "clone emc em",
        "print em",
        "print emc",
        "clear em",
        "print em",
        "print emc",
        "buffer emb em",
        "print emb",
        "# clone aliases the allocation, buffer copies it",
        "new s 6162636465",
        "print s",
        "clone c s",
        "print s",
        "print c",
        "buffer b s",
        "print b",
        "clear s",
        "print s",
        "print c",
        "print b",
        "# unique shared: deep copy from a shared source, steal from a unique one",
        "new n 01020304",
    ]
    if shared_source:
        lines.append("clone nc n")
    lines += [
        "unique mn n",
        "print n",
        "print mn",
        "mutate mn 0 255",
        "print n",
        "print mn",
    ]
    # the same line count in both variants: the last probe shows either the
    # shared sibling (deep copy) or the moved-from source (steal)
    lines.append("print nc" if shared_source else "print n")
    lines += [
        "new t 41424344",
        "unique mt t",
        "print t",
        "print mt",
        "mutate mt 3 0",
        "print mt",
        "to_shared back mt",
        "print mt",
        "print back",
        "clone back2 back",
        "print back",
        "clear back",
        "print back",
        "# UniqueSlice and SecureString",
        "unew us 4 65",
        "print us",
        "ucopy ud us",
        "print us",
        "print ud",
        "umutate ud 0 66",
        "print us",
        "print ud",
        "uclear us",
        "print us",
        "snew ss 3 122",
        "print ss",
        "scopy sd ss",
        "smutate sd 2 65",
        "print ss",
        "print sd",
        "sclear sd",
        "print sd",
    ]
    return "\n".join(lines) + "\n"


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
        # Dropping the clone must flip the constructor from the deep-copy branch
        # to the steal branch, which the trace has to show.
        other = WORK / "slice_script_unique.txt"
        other.write_text(script(shared_source=False))
        other_out = run(args.cpp, other, env)[1]
        if other_out == cpp_out:
            print("SELFTEST FAIL: the shared/unique split left no trace")
            return 1
        print("SELFTEST OK: comparator sees the copy/steal split")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
