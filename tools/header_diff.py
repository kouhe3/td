"""Compare MtprotoHeader's option matrix with TDLib (default + anonymous).

Both probes take the same argv and must print the same two hex headers.

Usage:
    uv run python tools/header_diff.py
    uv run python tools/header_diff.py --selftest

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
DEFAULT_CPP = "C:/tmp/td-probe/header_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/header_probe.exe"

BASE = ["94575", "probe", "Windows 10", "1, TDLib 1.8.71", "en", "", "", "0", "0", "none"]


def case(**changes: str) -> list[str]:
    values = list(BASE)
    index = {
        "api_id": 0,
        "device": 1,
        "system": 2,
        "app": 3,
        "system_lang": 4,
        "language_pack": 5,
        "language_code": 6,
        "tz_offset": 7,
        "emulator": 8,
        "proxy": 9,
    }
    extra: list[str] = []
    for key, value in changes.items():
        if key in ("server", "port"):
            continue
        values[index[key]] = value
    if "server" in changes:
        extra.append(changes["server"])
    if "port" in changes:
        if "server" not in changes:
            extra.append("127.0.0.1")
        extra.append(changes["port"])
    return values + extra


def hand_cases() -> list[list[str]]:
    return [
        case(),
        case(tz_offset="3600"),
        case(tz_offset="-3600"),
        case(tz_offset="900"),
        case(tz_offset="86400"),
        case(tz_offset="-1"),
        case(emulator="1"),
        case(proxy="socks5", server="10.0.0.1", port="1080"),
        case(proxy="http", server="10.0.0.2", port="3128"),
        case(proxy="mtproto", server="mt.example.com", port="8080"),
        case(proxy="web", server="web.example.com", port="9999"),
        case(language_pack="android_x"),
        case(language_pack="android_x", language_code="de"),
        case(language_pack="android_x", language_code="X-custom"),
        case(language_pack="", language_code="de"),
        case(language_pack="x", language_code="X"),
        case(device="", system="", app=""),
        case(device="设备😀", system="系统", app="应用"),
        case(device="a" * 253),
        case(device="a" * 254),
        case(device="a" * 300),
        case(api_id="0"),
        case(api_id="-1"),
        case(emulator="1", proxy="mtproto", server="1.2.3.4", port="443"),
        case(emulator="1", language_pack="android_x", language_code="de"),
    ]


def random_cases(rng: random.Random, count: int) -> list[list[str]]:
    devices = ["", "probe", "设备", "a" * 253, "a" * 254, "x" * 300, "Pixel 9"]
    systems = ["", "Windows 10", "Linux", "macOS 15", "系统"]
    apps = ["", "1, TDLib 1.8.71", "app 0.0.1"]
    langs = ["", "en", "de", "zh-hans"]
    packs = ["", "android_x", "ios", "web", "x"]
    codes = ["", "en", "de", "X-custom", "X"]
    proxies = ["none", "socks5", "http", "mtproto", "web"]
    servers = ["", "127.0.0.1", "mt.example.com", "1.2.3.4", "web.example.com"]
    cases = []
    for _ in range(count):
        cases.append(
            [
                rng.choice(["94575", "0", "-1", "1"]),
                rng.choice(devices),
                rng.choice(systems),
                rng.choice(apps),
                rng.choice(langs),
                rng.choice(packs),
                rng.choice(codes),
                str(rng.choice([0, 3600, -3600, 900, 86400, -1, rng.randrange(-12 * 3600, 12 * 3600)])),
                rng.choice(["0", "1"]),
                rng.choice(proxies),
                rng.choice(servers),
                str(rng.randrange(1, 65536)),
            ]
        )
    return cases


def corpus_md5(cases: list[list[str]]) -> str:
    digest = hashlib.md5()
    for argv in cases:
        digest.update(("\x1f".join(argv) + "\x1e").encode("utf-8"))
    return digest.hexdigest()


def run(binary: str, argv: list[str], env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, *argv], capture_output=True, env=env)
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
            print(f"DIFF {' '.join(argv)[:160]}")
            cpp_lines = cpp_out.splitlines()
            rs_lines = rs_out.splitlines()
            for index, (left, right) in enumerate(zip(cpp_lines, rs_lines)):
                if left != right:
                    print(f"  line {index + 1}: oracle={left[:120]} rust={right[:120]}")
                    break
            return 1

    single = cases[0]
    rc, first = run(args.cpp, single, env)
    rc2, second = run(args.cpp, single, env)
    if rc != 0 or rc2 != 0 or first != second:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        _, left = run(args.rs, case(tz_offset="0"), env)
        _, right = run(args.rs, case(tz_offset="3600"), env)
        if left == right:
            print("SELFTEST FAIL: tz_offset did not change the header")
            return 1
        print("SELFTEST OK: comparator sees a tz_offset change")

    print(f"PASS cases={len(cases)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())