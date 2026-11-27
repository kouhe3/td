"""Compare the Rust intermediate-transport framing with TDLib's own writer.

Oracle: tools/fixtures/framing_probe.cpp (tcp::IntermediateTransport).
The unpadded variants are compared byte for byte; the padded one only on the
magic, because its padding is random.

Usage:
    uv run python tools/framing_diff.py
    uv run python tools/framing_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary or fixture.
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/framing_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/framing_probe.exe"
DEFAULT_FIXTURE = REPO / "tools/fixtures/auth_key_sample.json"

CASES = [
    ("plain", 0, 0),
    ("quick_ack", 0, 1),
    ("padded", 1, 0),
]


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


def check_case(args, spec, label, with_padding, quick_ack, env, mutate=False):
    rc, out, err = run([args.cpp, spec["frame_payload"], str(with_padding), str(quick_ack)], env=env)
    if rc != 0:
        print(f"FAIL {label}: oracle rc={rc} {err.strip()[:160]}")
        return False
    cpp = parse_kv(out)

    fixture = json.loads(Path(args.fixture).read_text(encoding="utf-8"))
    fixture["framing"] = dict(spec, with_padding=with_padding, quick_ack=quick_ack)
    tmp_path = REPO / "work/framing-case.json"
    tmp_path.parent.mkdir(exist_ok=True)
    tmp_path.write_text(json.dumps(fixture), encoding="utf-8")

    rc, out, err = run([args.rs, str(tmp_path)])
    if rc != 0:
        print(f"FAIL {label}: rust rc={rc} {err.strip()[:160]}")
        return False
    rs = parse_kv(out)

    if mutate:
        packet = rs.get("packet", "")
        rs["packet"] = ("0" if packet[:1] != "0" else "1") + packet[1:]

    if cpp.get("magic") != rs.get("magic"):
        print(f"DIFF {label}.magic: want={cpp.get('magic')} got={rs.get('magic')}")
        return False
    if with_padding == 0 and cpp.get("packet") != rs.get("packet"):
        want, got = cpp.get("packet", ""), rs.get("packet", "")
        i = next((k for k in range(min(len(want), len(got))) if want[k] != got[k]), min(len(want), len(got)))
        print(f"DIFF {label}.packet @{i}: want={want[max(0, i - 8):i + 32]} got={got[max(0, i - 8):i + 32]}")
        return False
    print(f"PASS {label}")
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

    spec = json.loads(Path(args.fixture).read_text(encoding="utf-8"))["framing"]
    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    failed = 0
    for label, with_padding, quick_ack in CASES:
        if not check_case(args, spec, label, with_padding, quick_ack, env):
            failed += 1

    if args.selftest:
        if check_case(args, spec, "selftest-mutant", 0, 0, env, mutate=True):
            print("SELFTEST FAIL: a mutated packet was accepted")
            return 1
        print("SELFTEST OK: mutated packet rejected")

    if failed:
        print(f"FAIL {failed}/{len(CASES)} cases")
        return 1
    print(f"PASS cases={len(CASES)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())