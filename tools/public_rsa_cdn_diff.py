"""Compare td/telegram/net/PublicRsaKeySharedCdn with TDLib.

    uv run python tools/public_rsa_cdn_diff.py
    uv run python tools/public_rsa_cdn_diff.py --selftest

The CDN table is dynamic: keys are added at runtime, deduplicated by
fingerprint, dropped on request, and the failure message names the provider.
Both probes take the same script and print one line per op.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/public_rsa_cdn_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/public_rsa_cdn_probe.exe"
PEM_DIR = REPO / "work" / "rsa"

MAIN_PEM = """-----BEGIN RSA PUBLIC KEY-----
MIIBCgKCAQEA6LszBcC1LGzyr992NzE0ieY+BSaOW622Aa9Bd4ZHLl+TuFQ4lo4g
5nKaMBwK/BIb9xUfg0Q29/2mgIR6Zr9krM7HjuIcCzFvDtr+L0GQjae9H0pRB2OO
62cECs5HKhT5DZ98K33vmWiLowc621dQuwKWSQKjWf50XYFw42h21P2KXUGyp2y/
+aEyZ+uVgLLQbRA1dEjSDZ2iGRy12Mk5gpYc397aYp438fsJoHIgJ2lgMv5h7WY9
t6N/byY9Nw9p21Og3AoXSL2q/2IJ1WRUhebgAdGVMlV1fkuOQoEzR7EdpqtQD9Cs
5+bfo3Nhmcyvk5ftB0WkJ9z6bNZ7yxrP8wIDAQAB
-----END RSA PUBLIC KEY-----
"""

TEST_PEM = """-----BEGIN RSA PUBLIC KEY-----
MIIBCgKCAQEAyMEdY1aR+sCR3ZSJrtztKTKqigvO/vBfqACJLZtS7QMgCGXJ6XIR
yy7mx66W0/sOFa7/1mAZtEoIokDP3ShoqF4fVNb6XeqgQfaUHd8wJpDWHcR2OFwv
plUUI1PLTktZ9uW2WE23b+ixNwJjJGwBDJPQEQFBE+vfmH0JP503wr5INS1poWg/
j25sIWeYPHYeOrFp/eXaqhISP6G+q2IeTaWTXpwZj4LzXq5YOpk4bYEQ6mvRq7D1
aHWfYmlEGepfaYR8Q0YqvvhYtMte3ITnuSJs171+GDqpdKcSwHnd6FudwGO4pcCO
j4WcDuXc2CTHgH8gFTNhp/Y8/SpDOhvn9QIDAQAB
-----END RSA PUBLIC KEY-----
"""

BAD_PEM = "-----BEGIN PUBLIC KEY-----\n!!!!\n-----END PUBLIC KEY-----\n"

# Two known fingerprints, filled in by main() from the rsa probe.
MAIN_FP = 0
TEST_FP = 0


def script() -> str:
    main_path = PEM_DIR / "cdn_main.pem"
    test_path = PEM_DIR / "cdn_test.pem"
    bad_path = PEM_DIR / "cdn_bad.pem"
    missing = PEM_DIR / "cdn_missing.pem"
    if missing.exists():
        missing.unlink()
    lines = [
        "# dynamic CDN key table",
        "has",
        f"get {MAIN_FP}",
        f"add {main_path}",
        "has",
        f"get {MAIN_FP}",
        f"add {main_path}",
        "has",
        f"add {test_path}",
        "has",
        f"get {TEST_FP}",
        f"get {TEST_FP},{MAIN_FP}",
        f"get {MAIN_FP},{TEST_FP}",
        "get 1,2",
        "get ",
        "drop",
        "has",
        f"get {MAIN_FP}",
        f"add {bad_path}",
        f"add {missing}",
        f"add {main_path}",
        "has",
    ]
    return "\n".join(lines) + "\n"


def fingerprint_of(pem_text: str, env: dict[str, str]) -> int:
    import re

    path = PEM_DIR / "cdn_probe_fingerprint.pem"
    path.write_text(pem_text)
    proc = subprocess.run(["C:/tmp/td-probe/rsa_probe.exe", str(path), "fingerprint"], capture_output=True, env=env)
    match = re.search(r"fingerprint=(-?\d+)", proc.stdout.decode("utf-8", "replace"))
    if match is None:
        raise RuntimeError(f"rsa_probe gave no fingerprint: {proc.stdout!r}")
    return int(match.group(1))


def run(binary: str, script_path: Path, env: dict[str, str]) -> tuple[int, str]:
    proc = subprocess.run([binary, str(script_path)], capture_output=True, env=env)
    # printf on Windows writes \r\n; Rust writes bytes as-is.
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


def main() -> int:
    global MAIN_FP, TEST_FP
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    PEM_DIR.mkdir(parents=True, exist_ok=True)
    (PEM_DIR / "cdn_main.pem").write_text(MAIN_PEM)
    (PEM_DIR / "cdn_test.pem").write_text(TEST_PEM)
    (PEM_DIR / "cdn_bad.pem").write_text(BAD_PEM)
    MAIN_FP = fingerprint_of(MAIN_PEM, env)
    TEST_FP = fingerprint_of(TEST_PEM, env)

    text = script()
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())} main_fp={MAIN_FP} test_fp={TEST_FP}")

    script_path = PEM_DIR / "cdn_script.txt"
    script_path.write_text(text)

    cpp_rc, cpp_out = run(args.cpp, script_path, env)
    rs_rc, rs_out = run(args.rs, script_path, env)
    if cpp_rc != rs_rc or cpp_out != rs_out:
        for index, (left, right) in enumerate(zip(cpp_out.splitlines(), rs_out.splitlines())):
            if left != right:
                print(f"DIFF line {index + 1}: oracle={left!r} rust={right!r}")
                return 1
        print(f"DIFF line count: oracle={len(cpp_out.splitlines())} rust={len(rs_out.splitlines())}")
        return 1

    if run(args.cpp, script_path, env)[1] != cpp_out:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        # Adding a key must change the first `get` of that key from a failure to
        # a hit; the comparator has to see the difference.
        alone = ["# only the get", f"get {MAIN_FP}"]
        alone_path = PEM_DIR / "cdn_alone.txt"
        alone_path.write_text("\n".join(alone) + "\n")
        no_key = run(args.cpp, alone_path, env)[1]
        with_key = run(args.cpp, script_path, env)[1]
        if no_key.splitlines()[0] == with_key.splitlines()[4]:
            print("SELFTEST FAIL: get with and without the key produced the same line")
            return 1
        if "Unknown CDN fingerprints" not in no_key:
            print("SELFTEST FAIL: the empty table did not report the unknown-fingerprint message")
            return 1
        print("SELFTEST OK: comparator sees a table change and the CDN message")

    print(f"PASS lines={len(cpp_out.splitlines())}")
    return 0


if __name__ == "__main__":
    sys.exit(main())