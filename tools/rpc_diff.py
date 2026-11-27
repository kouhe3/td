"""Compare the Rust RPC request header with TDLib's MtprotoHeader bytes."""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/rpc_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/rpc_probe.exe"
DEFAULT_FIXTURE = REPO / "tools/fixtures/auth_key_sample.json"


def run(cmd, env=None):
    proc = subprocess.run(cmd, capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace"), proc.stderr.decode("utf-8", "replace")


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

    spec = json.loads(Path(args.fixture).read_text(encoding="utf-8"))["rpc"]
    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    rc, out, err = run(
        [
            args.cpp,
            str(spec["api_id"]),
            spec["device_model"],
            spec["system_version"],
            spec["application_version"],
            spec["system_language_code"],
        ],
        env=env,
    )
    if rc != 0:
        print(f"FAIL oracle rc={rc} {err.strip()[:160]}")
        return 1
    want = ""
    for line in out.splitlines():
        if line.startswith("header="):
            want = line.split("=", 1)[1]

    rc, out, err = run([args.rs, args.fixture])
    if rc != 0:
        print(f"FAIL rust rc={rc} {err.strip()[:160]}")
        return 1
    got = ""
    for line in out.splitlines():
        if line.startswith("header="):
            got = line.split("=", 1)[1]

    if args.selftest:
        mutated = ("0" if got[:1] != "0" else "1") + got[1:]
        if mutated == got:
            print("SELFTEST FAIL: tamper did not change the value")
            return 1
        print("SELFTEST OK: comparator sees field changes")

    if want != got:
        i = next((k for k in range(min(len(want), len(got))) if want[k] != got[k]), min(len(want), len(got)))
        print(f"DIFF header @{i}: want={want[max(0, i - 8):i + 32]} got={got[max(0, i - 8):i + 32]}")
        return 1
    print(f"PASS header_bytes={len(want) // 2}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
