"""Compare AuthData seq_no / message_id behaviour with TDLib's oracle.

`next_seq_no` is deterministic, so the whole sequence must match. `next_message_id`
deliberately mixes in a secure random word, so only its invariants are compared:
high 32 bits equal `now << 32`, `% 4 == 0`, strictly increasing.

Usage:
    uv run python tools/msgid_diff.py
    uv run python tools/msgid_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary or fixture.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/msgid_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/msgid_probe.exe"
DEFAULT_FIXTURE = REPO / "tools/fixtures/auth_key_sample.json"

CPP_LINE = re.compile(r"^seq=(-?\d+) msg_id=(\d+) content=([01])$")


def run(cmd, env=None):
    proc = subprocess.run(cmd, capture_output=True, env=env)
    return (
        proc.returncode,
        proc.stdout.decode("utf-8", "replace"),
        proc.stderr.decode("utf-8", "replace"),
    )


def invariants_ok(ids, now):
    high = int(now * (1 << 32)) >> 32
    prev = 0
    for mid in ids:
        if mid % 4 != 0 or mid <= prev or (mid >> 32) != high:
            return False
        prev = mid
    return True


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cpp", default=DEFAULT_CPP)
    ap.add_argument("--rs", default=str(DEFAULT_RS))
    ap.add_argument("--fixture", default=str(DEFAULT_FIXTURE))
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    for path in (args.cpp, args.rs, args.fixture):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    fixture = json.loads(Path(args.fixture).read_text(encoding="utf-8"))
    spec = fixture["msgid"]
    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    rc, out, err = run([args.cpp, str(spec["now"]), str(spec["count"])], env=env)
    if rc != 0:
        print(f"FAIL oracle rc={rc} {err.strip()[:200]}")
        return 1
    cpp_seqs, cpp_ids = [], []
    for line in out.splitlines():
        m = CPP_LINE.match(line.strip())
        if m:
            cpp_seqs.append(int(m.group(1)))
            cpp_ids.append(int(m.group(2)))

    rc, out, err = run([args.rs, args.fixture])
    if rc != 0:
        print(f"FAIL rust rc={rc} {err.strip()[:200]}")
        return 1
    rs = {}
    for line in out.splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            rs[key] = value
    rs_seqs = [int(v) for v in rs.get("seq", "").split(",") if v]

    if args.selftest:
        mutated = list(rs_seqs)
        if mutated:
            mutated[0] += 2
        if mutated == cpp_seqs:
            print("SELFTEST FAIL: mutated sequence was accepted")
            return 1
        print("SELFTEST OK: mutated sequence rejected")

    if cpp_seqs != rs_seqs:
        print(f"DIFF seq: oracle={cpp_seqs} rust={rs_seqs}")
        return 1
    if rs.get("invariants_ok") != "true":
        print("DIFF msg_id: rust invariants failed")
        return 1
    if not invariants_ok(cpp_ids, spec["now"]):
        print(f"DIFF msg_id: oracle invariants failed (ids={cpp_ids[:3]}...)")
        return 1

    print(f"PASS seq={len(cpp_seqs)} msg_id_invariants=ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())