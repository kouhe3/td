"""Compare td/mtproto/RSA.cpp with the Rust side.

    uv run python tools/rsa_diff.py
    uv run python tools/rsa_diff.py --selftest

The corpus builds SPKI PEM files here (no openssl dependency) and feeds the same
file to both probes. `encrypt`/`decrypt` are raw x^e mod n, so every output line
must match byte for byte; `load` covers the three validation failures.
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
DEFAULT_CPP = "C:/tmp/td-probe/rsa_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/rsa_probe.exe"
PEM_DIR = REPO / "work" / "rsa"

# RFC 3526 group 14: any 2048-bit modulus is fine for a public key.
MODULUS = int(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74"
    "020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437"
    "4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED"
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF05"
    "98DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB"
    "9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B"
    "E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF695581718"
    "3995497CEA956AE515D2261898FA051015728E5A8AACAA68FFFFFFFFFFFFFFFF",
    16,
)

RSA_OID = bytes.fromhex("2a864886f70d010101")
EC_OID = bytes.fromhex("2a8648ce3d0201")

# A real EC P-256 public key generated once with openssl; the point has to be on
# the curve for OpenSSL to reach the "not an RSA key" branch.
EC_PEM = """-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEvLG2y3TXLJiBlJ9H2I2YFio5IN4P
wFLff52URPYOv6IBXy0VN/ZLTduzDdJdAtI4983hwFbTV/KNGmiHGlN0pw==
-----END PUBLIC KEY-----
"""

# TDLib's own key table uses the PKCS#1 wrapper, so the lane has to cover it.
MAIN_PKCS1 = """-----BEGIN RSA PUBLIC KEY-----
MIIBCgKCAQEA6LszBcC1LGzyr992NzE0ieY+BSaOW622Aa9Bd4ZHLl+TuFQ4lo4g
5nKaMBwK/BIb9xUfg0Q29/2mgIR6Zr9krM7HjuIcCzFvDtr+L0GQjae9H0pRB2OO
62cECs5HKhT5DZ98K33vmWiLowc621dQuwKWSQKjWf50XYFw42h21P2KXUGyp2y/
+aEyZ+uVgLLQbRA1dEjSDZ2iGRy12Mk5gpYc397aYp438fsJoHIgJ2lgMv5h7WY9
t6N/byY9Nw9p21Og3AoXSL2q/2IJ1WRUhebgAdGVMlV1fkuOQoEzR7EdpqtQD9Cs
5+bfo3Nhmcyvk5ftB0WkJ9z6bNZ7yxrP8wIDAQAB
-----END RSA PUBLIC KEY-----
"""

TEST_PKCS1 = """-----BEGIN RSA PUBLIC KEY-----
MIIBCgKCAQEAyMEdY1aR+sCR3ZSJrtztKTKqigvO/vBfqACJLZtS7QMgCGXJ6XIR
yy7mx66W0/sOFa7/1mAZtEoIokDP3ShoqF4fVNb6XeqgQfaUHd8wJpDWHcR2OFwv
plUUI1PLTktZ9uW2WE23b+ixNwJjJGwBDJPQEQFBE+vfmH0JP503wr5INS1poWg/
j25sIWeYPHYeOrFp/eXaqhISP6G+q2IeTaWTXpwZj4LzXq5YOpk4bYEQ6mvRq7D1
aHWfYmlEGepfaYR8Q0YqvvhYtMte3ITnuSJs171+GDqpdKcSwHnd6FudwGO4pcCO
j4WcDuXc2CTHgH8gFTNhp/Y8/SpDOhvn9QIDAQAB
-----END RSA PUBLIC KEY-----
"""


def der_length(length: int) -> bytes:
    if length < 0x80:
        return bytes([length])
    body = length.to_bytes((length.bit_length() + 7) // 8, "big")
    return bytes([0x80 | len(body)]) + body


def tlv(tag: int, content: bytes) -> bytes:
    return bytes([tag]) + der_length(len(content)) + content


def der_integer(value: int) -> bytes:
    body = value.to_bytes(max(1, (value.bit_length() + 7) // 8), "big")
    if body[0] & 0x80:
        body = b"\x00" + body
    return tlv(0x02, body)


def spki(n: int, e: int, oid: bytes = RSA_OID) -> bytes:
    algorithm = tlv(0x30, tlv(0x06, oid) + b"\x05\x00")
    public = tlv(0x30, der_integer(n) + der_integer(e))
    return tlv(0x30, algorithm + tlv(0x03, b"\x00" + public))


def pem(n: int, e: int) -> str:
    text = base64.b64encode(spki(n, e)).decode()
    lines = [text[i:i + 64] for i in range(0, len(text), 64)]
    return "-----BEGIN PUBLIC KEY-----\n" + "\n".join(lines) + "\n-----END PUBLIC KEY-----\n"


def pkcs1_pem(n: int, e: int) -> str:
    der = tlv(0x30, der_integer(n) + der_integer(e))
    text = base64.b64encode(der).decode()
    lines = [text[i:i + 64] for i in range(0, len(text), 64)]
    return "-----BEGIN RSA PUBLIC KEY-----\n" + "\n".join(lines) + "\n-----END RSA PUBLIC KEY-----\n"


def payload(value: int) -> str:
    return value.to_bytes(256, "big").hex()


def cases() -> list[tuple[str, str, str, str | None]]:
    good = pem(MODULUS, 65537)
    table: list[tuple[str, str, str, str | None]] = [
        ("ok", good, "load", None),
        ("ok", good, "fingerprint", None),
        ("ok", good, "encrypt", payload(1)),
        ("ok", good, "encrypt", payload(MODULUS - 1)),
        ("ok", good, "encrypt", payload(MODULUS)),
        ("ok", good, "encrypt", payload(0xFFFFFFFFFFFFFFFF)),
        ("ok", good, "encrypt", payload(0)),
        ("ok", good, "decrypt", payload(1)),
        ("ok", good, "decrypt", payload(MODULUS + 12345)),
        ("n2040", pem(MODULUS >> 8, 65537), "load", None),
        ("n2049", pem(MODULUS | (1 << 2048), 65537), "load", None),
        ("e1", pem(MODULUS, 1), "load", None),
        ("e1", pem(MODULUS, 1), "fingerprint", None),
        ("e3", pem(MODULUS, 3), "fingerprint", None),
        ("badb64", "-----BEGIN PUBLIC KEY-----\n!!!!\n-----END PUBLIC KEY-----\n", "load", None),
        ("empty", "", "load", None),
        ("ec", EC_PEM, "load", None),
        ("ec", EC_PEM, "fingerprint", None),
        ("pkcs1_main", MAIN_PKCS1, "load", None),
        ("pkcs1_main", MAIN_PKCS1, "fingerprint", None),
        ("pkcs1_test", TEST_PKCS1, "fingerprint", None),
        ("pkcs1_2040", pkcs1_pem(MODULUS >> 8, 65537), "load", None),
        ("pkcs1_bad", "-----BEGIN RSA PUBLIC KEY-----\n!!!!\n-----END RSA PUBLIC KEY-----\n", "load", None),
    ]
    return table


def run(binary: str, pem_path: Path, mode: str, data: str | None, env: dict[str, str]) -> tuple[int, str]:
    argv = [binary, str(pem_path), mode]
    if data is not None:
        argv.append(data)
    proc = subprocess.run(argv, capture_output=True, env=env)
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
    digest = hashlib.md5(
        "\x1f".join(f"{name}:{mode}:{data or ''}" for name, _, mode, data in table).encode()
    ).hexdigest()
    print(f"corpus md5={digest} cases={len(table)}")

    PEM_DIR.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    for name, text, mode, data in table:
        pem_path = PEM_DIR / f"{name}.pem"
        pem_path.write_text(text)
        cpp_rc, cpp_out = run(args.cpp, pem_path, mode, data, env)
        rs_rc, rs_out = run(args.rs, pem_path, mode, data, env)
        if cpp_rc != rs_rc or cpp_out != rs_out:
            print(f"DIFF {name} {mode}: oracle={cpp_out.strip()[:80]!r} rust={rs_out.strip()[:80]!r}")
            return 1

    floor = table[0]
    floor_path = PEM_DIR / f"{floor[0]}.pem"
    if run(args.cpp, floor_path, floor[2], floor[3], env)[1] != run(args.cpp, floor_path, floor[2], floor[3], env)[1]:
        print("FLOOR FAIL: the oracle is not deterministic")
        return 1

    if args.selftest:
        good_path = PEM_DIR / "ok.pem"
        if run(args.cpp, good_path, "encrypt", payload(1), env)[1] == run(
            args.cpp, good_path, "encrypt", payload(2), env
        )[1]:
            print("SELFTEST FAIL: two payloads produced the same ciphertext")
            return 1
        ec_path = PEM_DIR / "ec.pem"
        if run(args.cpp, ec_path, "load", None, env)[1] == run(args.cpp, good_path, "load", None, env)[1]:
            print("SELFTEST FAIL: an EC key loaded like an RSA key")
            return 1
        print("SELFTEST OK: comparator sees a payload change and a key-type change")

    print(f"PASS cases={len(table)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())