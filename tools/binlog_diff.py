"""Compare binlog event headers: C++ ``binlog_dump`` vs Rust ``binlog_head``.

Columns compared: event_id, type, flags, data length.  The C++ tool prints the
data length in its ``size`` tag (``get_data().size()``), not the frame size.

Usage:
    uv run python tools/binlog_diff.py
    uv run python tools/binlog_diff.py --binlog work/tdlib-oracle/td_test.binlog
    uv run python tools/binlog_diff.py --selftest

Exit codes: 0 = PASS, 1 = diff / bad output, 2 = missing binary or fixture.
"""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-build/tddb/binlog_dump.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/binlog_head.exe"
DEFAULT_BINLOG = REPO / "work/tdlib-oracle/td_test.binlog"

CPP_RE = re.compile(
    r"\[event_id:0x([0-9a-f]{16})\]\[type:(-?\d+)\]\[flags:(-?\d+)\]\[size:(\d+)\]"
)
RS_RE = re.compile(r"^0x([0-9a-f]{16}) (-?\d+) (-?\d+) (\d+)$")


def run(cmd, env=None):
    proc = subprocess.run(cmd, capture_output=True, env=env)
    return (
        proc.returncode,
        proc.stdout.decode("utf-8", "replace"),
        proc.stderr.decode("utf-8", "replace"),
    )


def cpp_rows(exe, binlog):
    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")
    rc, out, err = run([str(exe), str(binlog)], env=env)
    rows = []
    # binlog_dump prints through LOG(PLAIN), which goes to stderr.
    for line in (out + "\n" + err).splitlines():
        m = CPP_RE.search(line)
        if m:
            rows.append(m.groups())
    return rc, rows, err


def rs_rows(exe, binlog):
    rc, out, err = run([str(exe), str(binlog)])
    rows = []
    for line in out.splitlines():
        m = RS_RE.match(line)
        if m:
            rows.append(m.groups())
    return rc, rows, err


def compare(cpp, rs):
    diffs = []
    for i in range(max(len(cpp), len(rs))):
        a = cpp[i] if i < len(cpp) else None
        b = rs[i] if i < len(rs) else None
        if a != b:
            diffs.append((i, a, b))
    return diffs


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cpp", default=DEFAULT_CPP)
    ap.add_argument("--rs", default=str(DEFAULT_RS))
    ap.add_argument("--binlog", default=str(DEFAULT_BINLOG))
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    for path in (args.cpp, args.rs, args.binlog):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    cpp_rc, cpp, cpp_err = cpp_rows(args.cpp, args.binlog)
    rs_rc, rs, rs_err = rs_rows(args.rs, args.binlog)

    if cpp_rc != 0:
        print(f"FAIL cpp rc={cpp_rc} {cpp_err.strip()[:200]}")
        return 1
    if rs_rc != 0:
        print(f"FAIL rust rc={rs_rc} {rs_err.strip()[:200]}")
        return 1

    diffs = compare(cpp, rs)
    print(f"events cpp={len(cpp)} rust={len(rs)}")

    if args.selftest:
        if not rs:
            print("SELFTEST FAIL: no rust rows")
            return 1
        tampered = [list(r) for r in rs]
        tampered[0][1] = str(int(tampered[0][1]) + 1)
        fake = compare(cpp, tampered)
        if not fake or fake[0][0] != 0:
            print("SELFTEST FAIL: comparator did not catch a mutated type")
            return 1
        print("SELFTEST OK: mutated row 0 caught")

    if diffs:
        print(f"DIFF {len(diffs)}")
        for i, a, b in diffs[:5]:
            print(f"  row {i}: cpp={a} rust={b}")
        return 1

    if len(cpp) != 147:
        print(f"WARN expected 147 events, got {len(cpp)}")
    print("PASS diff_lines=0")
    return 0


if __name__ == "__main__":
    sys.exit(main())