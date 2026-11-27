"""Compare the built-in RSA key table with TDLib.

    uv run python tools/public_rsa_diff.py
    uv run python tools/public_rsa_diff.py --selftest

PublicRsaKeySharedMain holds one PKCS#1 key per mode (`main` / `test`); the
lookup walks the requested fingerprints outer loop and the keys inner loop, and
the failure message prints the list with braces.

The two known fingerprints are read from the rsa probe (the oracle for
from_pem_public_key), so this lane never hard-codes them.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/public_rsa_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/public_rsa_probe.exe"
RSA_CPP = "C:/tmp/td-probe/rsa_probe.exe"
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


def fingerprint_of(pem_text: str, env: dict[str, str]) -> int:
    PEM_DIR.mkdir(parents=True, exist_ok=True)
    path = PEM_DIR / "public_rsa_probe.pem"
    path.write_text(pem_text)
    proc = subprocess.run([RSA_CPP, str(path), "fingerprint"], capture_output=True, env=env)
    match = re.search(r"fingerprint=(-?\d+)", proc.stdout.decode("utf-8", "replace"))
    if match is None:
        raise RuntimeError(f"rsa_probe gave no fingerprint: {proc.stdout!r}")
    return int(match.group(1))


def run(binary: str, provider: str, fingerprints: list[int], env: dict[str, str]) -> tuple[int, str]:
    text = ",".join(str(value) for value in fingerprints)
    proc = subprocess.run([binary, provider, text], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").replace("\r\n", "\n")


def cases(main_fp: int, test_fp: int) -> list[tuple[str, list[int]]]:
    return [
        ("main", [main_fp]),
        ("main", [test_fp]),
        ("main", [test_fp, main_fp]),
        ("main", [main_fp, test_fp]),
        ("main", [0, 1]),
        ("main", []),
        ("test", [test_fp]),
        ("test", [main_fp]),
        ("test", []),
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    for path in (args.cpp, args.rs, RSA_CPP):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    main_fp = fingerprint_of(MAIN_PEM, env)
    test_fp = fingerprint_of(TEST_PEM, env)
    table = cases(main_fp, test_fp)
    digest = hashlib.md5(
        "\x1f".join(f"{provider}:{','.join(map(str, fps))}" for provider, fps in table).encode()
    ).hexdigest()
    print(f"corpus md5={digest} cases={len(table)} main_fp={main_fp} test_fp={test_fp}")

    for provider, fingerprints in table:
        cpp_rc, cpp_out = run(args.cpp, provider, fingerprints, env)
        rs_rc, rs_out = run(args.rs, provider, fingerprints, env)
        if cpp_rc != rs_rc or cpp_out != rs_out:
            print(f"DIFF {provider} {fingerprints}: oracle={cpp_out.strip()!r} rust={rs_out.strip()!r}")
            return 1

    if run(args.cpp, "main", [main_fp], env)[1] != run(args.cpp, "main", [main_fp], env)[1]:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        if run(args.cpp, "main", [main_fp], env)[1] == run(args.cpp, "main", [test_fp], env)[1]:
            print("SELFTEST FAIL: the test fingerprint matched the main key")
            return 1
        if run(args.cpp, "main", [main_fp], env)[1] == run(args.cpp, "test", [main_fp], env)[1]:
            print("SELFTEST FAIL: main and test providers answered the same")
            return 1
        print("SELFTEST OK: comparator sees a fingerprint change and a provider change")

    print(f"PASS cases={len(table)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())