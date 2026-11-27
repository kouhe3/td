"""Compare the Web-proxy helpers (td/telegram/net/Proxy.cpp) with TDLib.

Both probes take the same argv and must print the same bytes: the chosen secret,
the derived address / link secret / bridge capability, or the rejection text.

Usage:
    uv run python tools/web_proxy_diff.py
    uv run python tools/web_proxy_diff.py --selftest

Exit codes: 0 = PASS, 1 = mismatch, 2 = missing binary.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/web_proxy_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/web_proxy_probe.exe"


def b64url(data: bytes) -> str:
    """TDLib's base64url_encode: unpadded."""
    return base64.urlsafe_b64encode(data).decode().rstrip("=")


def cases() -> list[list[str]]:
    plain16 = bytes(16)
    dd17 = b"\xdd" + bytes(16)
    tls33 = b"\xee" + bytes(32)
    return [
        # get_web_proxy_secret
        ["secret", plain16.hex()],
        ["secret", dd17.hex()],
        ["secret", tls33.hex()],
        ["secret", ""],
        ["secret", "zz"],
        ["secret", b64url(plain16)],
        ["secret", "00" * 15],
        # get_web_proxy_link_secret: marked (0x70) and plain forms
        ["link", b64url(b"\x70" + plain16), "0"],
        ["link", b64url(b"\x70" + plain16), "1"],
        ["link", b64url(b"\x70" + dd17), "0"],
        ["link", b64url(b"\x70" + tls33), "0"],
        ["link", b64url(b"\x70" + tls33), "1"],
        ["link", b64url(plain16), "0"],
        ["link", b64url(plain16), "1"],
        ["link", plain16.hex(), "0"],
        ["link", plain16.hex(), "1"],
        ["link", "zz", "0"],
        ["link", "zz", "1"],
        ["link", "", "0"],
        # web proxy derived strings
        ["proxy", "example.com", "", plain16.hex()],
        ["proxy", "example.com", "a-b_c", plain16.hex()],
        ["proxy", "例子.com", "path9", plain16.hex()],
        ["proxy", "", "", plain16.hex()],
        ["proxy", "example.com", "", dd17.hex()],
        ["proxy", "example.com", "p", dd17.hex()],
        ["proxy", "example.com", "p", (b"\xee" + bytes(32)).hex()],
    ]


def run(binary: str, argv: list[str], env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, *argv], capture_output=True, env=env)
    # printf on Windows translates \n to \r\n; Rust writes bytes as-is.
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    table = cases()
    digest = hashlib.md5("\x1f".join("\x1e".join(case) for case in table).encode()).hexdigest()
    print(f"corpus md5={digest} cases={len(table)}")

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    for argv in table:
        cpp_rc, cpp_out = run(args.cpp, argv, env)
        rs_rc, rs_out = run(args.rs, argv, env)
        if cpp_rc != rs_rc or cpp_out != rs_out:
            print(f"DIFF {' '.join(argv)[:120]}: oracle={cpp_out.strip()!r} rust={rs_out.strip()!r}")
            return 1

    single = table[0]
    _, first = run(args.cpp, single, env)
    _, second = run(args.cpp, single, env)
    if first != second:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        # Swap the bridge context version: both outputs must move.
        left = run(args.cpp, ["proxy", "example.com", "", bytes(16).hex()], env)[1]
        right = run(args.cpp, ["proxy", "example.com", "p", bytes(16).hex()], env)[1]
        if left == right:
            print("SELFTEST FAIL: path did not change the derived strings")
            return 1
        print("SELFTEST OK: comparator sees the path change")

    print(f"PASS cases={len(table)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())