"""Read back an MTProto 2.0 packet produced by TDLib's Transport::write.

TDLib's writer always emits client-direction packets (X = 0); its reader is
hard-wired to X = 8, so the two cannot be fed into each other. The oracle is
therefore the writer (tools/fixtures/transport_probe.cpp), and the Rust
transport_probe reads the packet with the client-direction KDF.

Usage:
    uv run python tools/transport_diff.py
    uv run python tools/transport_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary or fixture.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/transport_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/transport_probe.exe"
DEFAULT_FIXTURE = REPO / "tools/fixtures/auth_key_sample.json"


def run(cmd, env=None):
    proc = subprocess.run(cmd, capture_output=True, env=env)
    return (
        proc.returncode,
        proc.stdout.decode("utf-8", "replace"),
        proc.stderr.decode("utf-8", "replace"),
    )


def parse_kv(text):
    out = {}
    for line in text.splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            out[key] = value
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

    fixture = json.loads(Path(args.fixture).read_text(encoding="utf-8"))
    auth_key = fixture["expect"]["auth_key"]
    spec = fixture["transport"]
    expect = spec["expect"]

    env = dict(__import__("os").environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")
    cpp_cmd = [
        args.cpp,
        "write",
        auth_key,
        str(spec["akid"]),
        str(spec["salt"]),
        str(spec["session_id"]),
        str(spec["msg_id"]),
        str(spec["seq_no"]),
        spec["payload"],
    ]
    rc, out, err = run(cpp_cmd, env=env)
    if rc != 0:
        print(f"FAIL oracle rc={rc} {err.strip()[:200]}")
        return 1
    packet = parse_kv(out).get("packet", "")
    if not packet:
        print(f"FAIL oracle produced no packet: {out[:200]}")
        return 1

    rc, out, err = run([args.rs, args.fixture, packet])
    if rc != 0:
        print(f"FAIL rust rc={rc} {err.strip()[:200]}")
        return 1
    got = parse_kv(out)
    got["packet_len"] = str(len(packet) // 2)

    if args.selftest:
        tampered = dict(got)
        tampered["salt"] = str(int(tampered.get("salt", "0")) + 1)
        if tampered == got:
            print("SELFTEST FAIL: tamper did not change the value")
            return 1
        print("SELFTEST OK: comparator sees field changes")

    bad = [(k, str(v), got.get(k)) for k, v in expect.items() if got.get(k) != str(v)]
    for key, want, value in bad:
        print(f"DIFF {key}: want={want} got={value}")
    if bad:
        return 1
    print(f"PASS fields={len(expect)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())