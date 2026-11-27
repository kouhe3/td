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
    # binlog_dump prints through LOG(PLAIN), which goes to stderr.
    rows = [line for line in (out + "\n" + err).splitlines() if line.strip()]
    return rc, rows, err


def rs_rows(exe, binlog):
    rc, out, err = run([str(exe), str(binlog)])
    rows = [line for line in out.splitlines() if line.strip()]
    return rc, rows, err


def compare(cpp, rs):
    diffs = []
    for i in range(max(len(cpp), len(rs))):
        a = cpp[i] if i < len(cpp) else None
        b = rs[i] if i < len(rs) else None
        if a != b:
            diffs.append((i, a, b))
    return diffs


def squeeze(a, b, pad=20, span=60):
    """Show the first differing character with a small window around it."""
    if a is None or b is None:
        return f"cpp={a!r} rust={b!r}"
    n = min(len(a), len(b))
    i = next((k for k in range(n) if a[k] != b[k]), n)
    lo = max(0, i - pad)
    return f"@{i} cpp={a[lo:i + span]!r} rust={b[lo:i + span]!r}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cpp", default=DEFAULT_CPP)
    ap.add_argument("--rs", default=str(DEFAULT_RS))
    ap.add_argument("--binlog", default=str(DEFAULT_BINLOG))
    ap.add_argument("--all", action="store_true", help="compare every work/*/td_test.binlog")
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2
    files = sorted(REPO.glob("work/*/td_test.binlog")) if args.all else [Path(args.binlog)]
    if not files or not all(Path(f).exists() for f in files):
        print(f"MISSING {args.binlog}")
        return 2

    failed = 0
    for binlog in files:
        if not compare_one(args, str(binlog)):
            failed += 1

    if failed:
        print(f"FAIL {failed}/{len(files)} binlogs differ")
        return 1
    if len(files) > 1:
        print(f"PASS {len(files)} binlogs diff_lines=0")
    return 0


def compare_one(args, binlog) -> bool:
    label = Path(binlog).parent.name
    cpp_rc, cpp, cpp_err = cpp_rows(args.cpp, binlog)
    rs_rc, rs, rs_err = rs_rows(args.rs, binlog)

    if cpp_rc != 0:
        print(f"FAIL {label}: cpp rc={cpp_rc} {cpp_err.strip()[:200]}")
        return False
    if rs_rc != 0:
        print(f"FAIL {label}: rust rc={rs_rc} {rs_err.strip()[:200]}")
        return False

    diffs = compare(cpp, rs)
    print(f"{label}: lines cpp={len(cpp)} rust={len(rs)}")

    if args.selftest:
        if not rs:
            print(f"SELFTEST FAIL {label}: no rust rows")
            return False
        tampered = list(rs)
        tampered[0] = tampered[0].replace("[type:", "[type:9", 1)
        fake = compare(cpp, tampered)
        if not fake or fake[0][0] != 0:
            print(f"SELFTEST FAIL {label}: comparator did not catch a mutated type")
            return False
        print(f"SELFTEST OK {label}: mutated row 0 caught")
        args.selftest = False  # once is enough

    if diffs:
        print(f"DIFF {label} {len(diffs)}")
        for i, a, b in diffs[:5]:
            print(f"  row {i}: {squeeze(a, b)}")
        return False

    print(f"PASS {label} diff_lines=0")
    return True


if __name__ == "__main__":
    sys.exit(main())