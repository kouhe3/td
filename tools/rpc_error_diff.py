"""Compare rpc_error encoding and parsing with TDLib's generated class."""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/rpc_probe2.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/rpc_error_probe.exe"
DEFAULT_FIXTURE = REPO / "tools/fixtures/auth_key_sample.json"
RPC_ERROR_ID = "19ca4421"


def run(cmd, env=None):
    proc = subprocess.run(cmd, capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace"), proc.stderr.decode("utf-8", "replace")


def kv(text):
    out = {}
    for line in text.splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            out[k] = v
    return out


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

    spec = json.loads(Path(args.fixture).read_text(encoding="utf-8"))["rpc_error"]
    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    rc, out, err = run([args.cpp, "store", str(spec["code"]), spec["message"]], env=env)
    if rc != 0:
        print(f"FAIL oracle store rc={rc} {err.strip()[:160]}")
        return 1
    oracle_fields = kv(out).get("bytes", "")

    rc, out, err = run([args.rs, args.fixture])
    if rc != 0:
        print(f"FAIL rust rc={rc} {err.strip()[:160]}")
        return 1
    rs = kv(out)

    if args.selftest:
        mutated = ("0" if rs.get("full", "0")[:1] != "0" else "1") + rs.get("full", "0")[1:]
        if mutated == rs.get("full"):
            print("SELFTEST FAIL: tamper did not change the value")
            return 1
        print("SELFTEST OK: comparator sees field changes")

    if rs.get("fields") != oracle_fields:
        print(f"DIFF fields: oracle={oracle_fields} rust={rs.get('fields')}")
        return 1
    if rs.get("full") != RPC_ERROR_ID + oracle_fields:
        print(f"DIFF full: want={RPC_ERROR_ID}{oracle_fields} got={rs.get('full')}")
        return 1

    # Reverse direction: TDLib must parse the fields we produced.
    rc, out, err = run([args.cpp, "parse", rs["fields"]], env=env)
    if rc != 0:
        print(f"FAIL oracle parse rc={rc} {err.strip()[:160]}")
        return 1
    parsed = kv(out)
    if parsed.get("code") != str(spec["code"]) or parsed.get("message") != spec["message"]:
        print(f"DIFF roundtrip: {parsed}")
        return 1

    print(f"PASS fields_bytes={len(oracle_fields) // 2}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
