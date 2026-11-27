"""Compare td::mtproto::ProxySecret with TDLib's own implementation.

Both probes take the same argv and must print the same bytes: accepted secrets
carry raw / proxy / encoded / padding / tls (and domain for TLS), rejections
carry the status code and message.

Usage:
    uv run python tools/proxy_secret_diff.py
    uv run python tools/proxy_secret_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import random
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/proxy_secret_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/proxy_secret_probe.exe"

HEX = "0123456789abcdefABCDEF"
BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
BASE64URL = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"


def h(byte: str, count: int) -> str:
    return byte * count


def hand_cases() -> list[list[str]]:
    domain = "6578616d706c652e636f6d"  # "example.com"
    return [
        # link: hex path, length rules, uppercase digits
        ["link", ""],
        ["link", h("00", 16)],
        ["link", h("00", 15)],
        ["link", "dd" + h("00", 16)],
        ["link", h("00", 17)],
        ["link", "ee" + h("11", 16)],
        ["link", "ff" + h("11", 16)],
        ["link", "EE" + h("11", 16)],
        ["link", "ee" + h("11", 16) + domain],
        ["link", "dd" + h("00", 16) + domain],
        ["link", "0" * 31],
        ["link", "A"],
        ["link", "AAAA="],
        ["link", "AA=="],
        ["link", "A*AA"],
        ["link", "ee" + h("11", 199)],
        ["link", "ee" + h("11", 199), "--truncate"],
        ["link", "ee" + h("11", 200)],
        ["link", "ee" + h("11", 200), "--truncate"],
        ["link", "dd" + h("00", 16) + "616263"],
        # link: base64url path (unpadded lengths 2 and 3 modulo 4)
        ["link", "8Y7f8Y7f8Y7f8Y7f8Y7f8A"],
        ["link", "8Y7f8Y7f8Y7f8Y7f8Y7f8A=="],
        # binary: every accepted and rejected shape
        ["binary", ""],
        ["binary", h("00", 16)],
        ["binary", "dd" + h("00", 16)],
        ["binary", "ee" + h("00", 17)],
        ["binary", h("00", 17)],
        ["binary", h("00", 15)],
        ["binary", "dd" + h("00", 17)],
        ["binary", "ee" + h("00", 198)],
        ["binary", "ee" + h("00", 199)],
        ["binary", "ee" + h("00", 199), "--truncate"],
        # encoded / derived: from_raw accepts anything
        ["encoded", ""],
        ["encoded", h("00", 16)],
        ["encoded", "dd" + h("11", 16)],
        ["encoded", "ee" + h("11", 16) + domain],
        ["derived", ""],
        ["derived", h("00", 16)],
        ["derived", "dd" + h("11", 16)],
        ["derived", "ee" + h("11", 16)],
        ["derived", "ee" + h("11", 16) + domain],
        ["derived", h("ab", 200)],
    ]


def random_cases(rng: random.Random, count: int) -> list[list[str]]:
    cases: list[list[str]] = []
    for _ in range(count):
        kind = rng.random()
        if kind < 0.6:
            length = rng.choice([0, 1, 15, 16, 17, 18, 19, 40, 198, 199, 200, rng.randrange(0, 205)])
            first = rng.choice([0x00, 0xdd, 0xee, 0xff, 0x11, rng.randrange(256)])
            body = bytes([first]) + bytes(rng.randrange(256) for _ in range(max(0, length - 1)))
            hexed = body.hex()
            truncate = ["--truncate"] if rng.random() < 0.5 else []
            cases.append(["binary", hexed, *truncate])
            cases.append(["derived", hexed])
            continue
        alphabet = HEX + BASE64 + BASE64URL + "+/=*."
        length = rng.choice([0, 1, 2, 3, 4, 16, 22, 24, 32, 34, 36, 40, rng.randrange(0, 64)])
        text = "".join(rng.choice(alphabet) for _ in range(length))
        cases.append(["link", text])
    return cases


def corpus_md5(cases: list[list[str]]) -> str:
    digest = hashlib.md5()
    for case in cases:
        digest.update(("\x1f".join(case) + "\x1e").encode("utf-8"))
    return digest.hexdigest()


def run(binary: str, argv: list[str], env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, *argv], capture_output=True, env=env)
    # printf on Windows translates \n to \r\n; Rust writes bytes as-is.
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--random-cases", type=int, default=60)
    parser.add_argument("--seed", type=int, default=20261127)
    args = parser.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    rng = random.Random(args.seed)
    cases = hand_cases() + random_cases(rng, args.random_cases)
    print(f"corpus md5={corpus_md5(cases)} cases={len(cases)}")

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    for argv in cases:
        cpp_rc, cpp_out = run(args.cpp, argv, env)
        rs_rc, rs_out = run(args.rs, argv, env)
        if cpp_rc != rs_rc or cpp_out != rs_out:
            print(f"DIFF {' '.join(argv)[:120]}: oracle={cpp_out.strip()!r} rust={rs_out.strip()!r}")
            return 1

    # A/A floor: the same argv twice on the oracle, independent processes.
    single = cases[-1]
    rc, again = run(args.cpp, single, env)
    rc2, again2 = run(args.cpp, single, env)
    if rc != 0 or rc2 != 0 or again != again2:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        _, left = run(args.cpp, ["derived", h("00", 16)], env)
        _, right = run(args.cpp, ["derived", h("00", 15)], env)
        if left == right:
            print("SELFTEST FAIL: different secrets produced the same output")
            return 1
        print("SELFTEST OK: comparator sees a one-byte change")

    print(f"PASS cases={len(cases)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())