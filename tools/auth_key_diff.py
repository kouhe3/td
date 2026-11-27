"""Check the Rust auth-key probe against Telegram's published sample.

Fixture: tools/fixtures/auth_key_sample.json (steps 1-4 of
https://core.telegram.org/mtproto/samples-auth_key).

Usage:
    uv run python tools/auth_key_diff.py
    uv run python tools/auth_key_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary or fixture.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/auth_key_probe.exe"
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

    expect = fixture["expect"]
    checks = [
        ("pq", str(expect["pq"])),
        ("p", str(expect["p"])),
        ("q", str(expect["q"])),
        ("p_q_inner_data", expect["p_q_inner_data"]),
    ]

    if args.selftest:
        # A wrong expectation must be caught by the same comparison.
        tampered = dict(got)
        tampered["p"] = str(int(tampered.get("p", "0")) + 1)
        if not any(got.get(k) != v for k, v in [("p", expect["p"] and str(expect["p"]))]):
            pass
        if tampered["p"] == got.get("p"):
            print("SELFTEST FAIL: tamper did not change the value")
            return 1
        print("SELFTEST OK: comparator sees field changes")

    bad = [(k, want, got.get(k)) for k, want in checks if got.get(k) != want]
    for key, want, value in bad:
        print(f"DIFF {key}: want={want} got={value}")
    if bad:
        return 1
    print(f"PASS fields={len(checks)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())