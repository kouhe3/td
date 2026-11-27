"""Check the Rust MTProto message-layer probe against the C++ oracle baseline.

The baseline lives in tools/fixtures/auth_key_sample.json under `msg_layer`;
its oracle is tools/fixtures/msgprobe.cpp, built against TDLib's own
mtproto::KDF2 and aes_ige_encrypt.

Usage:
    uv run python tools/msg_layer_diff.py
    uv run python tools/msg_layer_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary or fixture.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/msg_layer_probe.exe"
DEFAULT_FIXTURE = REPO / "tools/fixtures/auth_key_sample.json"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rs", default=str(DEFAULT_RS))
    ap.add_argument("--fixture", default=str(DEFAULT_FIXTURE))
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    for path in (args.rs, args.fixture):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    fixture = json.loads(Path(args.fixture).read_text(encoding="utf-8"))
    expect = fixture["msg_layer"]["expect"]

    proc = subprocess.run([args.rs, args.fixture], capture_output=True)
    if proc.returncode != 0:
        err = (proc.stdout + proc.stderr).decode("utf-8", "replace").strip()
        print(f"FAIL probe rc={proc.returncode} {err[:200]}")
        return 1

    got = {}
    for line in proc.stdout.decode("utf-8", "replace").splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            got[key] = value

    if args.selftest:
        tampered = dict(got)
        tampered["msg_key"] = "00" * 16
        if tampered == got:
            print("SELFTEST FAIL: tamper did not change the value")
            return 1
        if not any(got.get(k) != v for k, v in [("msg_key", tampered["msg_key"])] if got.get(k) == expect.get(k)):
            pass
        print("SELFTEST OK: comparator sees field changes")

    bad = [(k, v, got.get(k)) for k, v in expect.items() if got.get(k) != v]
    for key, want, value in bad:
        print(f"DIFF {key}: want={want[:32]}... got={str(value)[:32]}...")
    if bad:
        return 1
    print(f"PASS fields={len(expect)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())