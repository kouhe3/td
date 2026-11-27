"""Compare TlsInit's ClientHello with TDLib by structure, not by bytes.

The oracle (tools/fixtures/tls_init_probe.cpp) and rust/src/bin/tls_init_probe.rs
both print `hello len=<n> <hex>`. Two runs never agree byte for byte: the
session id, both X25519 keys, the 1184-byte ML-KEM blob and the ClientHello
random (an HMAC over the buffer) are fresh, `Permutation` shuffles the
extension order, and ech_payload picks one of four lengths. So the comparison:

  * both sides must satisfy the same invariants (HMAC self-consistency, the
    grease shape, the extension count and lengths, the four legal totals);
  * after normalising the grease words and the random stretches, every sample
    on one side must produce the same structure, and the two sides' structures
    must be equal.

`--samples N` runs N fresh processes per side (the op tree is a function-local
static in TDLib, so one process only ever shows one permutation).
"""

from __future__ import annotations

import argparse
import hashlib
import hmac
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/tls_init_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/tls_init_probe.exe"

DOMAIN = "proxy.example.com"
SECRET = bytes(range(16))
UNIX_TIME = 0x12345678

ECH_LENGTHS = {186, 218, 250, 282}  # 42 + {144, 176, 208, 240}
# 1716 bytes of everything except the server name; ech_payload adds 32 per step.
TOTAL_LENGTHS = {1716 + len(DOMAIN) + 32 * step for step in range(4)}
FIXED_EXTENSIONS = {0x0000, 0x0005, 0x000A, 0x000B, 0x000D, 0x0010, 0x0012, 0x0017, 0x001B, 0x0023,
                    0x002B, 0x002D, 0x0033, 0x44CD, 0xFE0D, 0xFF01}


def is_grease_word(word: bytes) -> bool:
    return len(word) == 2 and word[0] == word[1] and (word[0] & 0x0F) == 0x0A


def parse_client_hello(data: bytes) -> dict:
    if data[:3] != b"\x16\x03\x01":
        raise ValueError("record header is not 16 03 01")
    record_length = int.from_bytes(data[3:5], "big")
    if record_length != len(data) - 5:
        raise ValueError(f"record length {record_length} != {len(data) - 5}")
    body = data[5:]
    if body[:2] != b"\x01\x00":
        raise ValueError("handshake is not ClientHello")
    handshake_length = int.from_bytes(body[2:4], "big")
    if handshake_length != len(body) - 4:
        raise ValueError(f"handshake length {handshake_length} != {len(body) - 4}")
    body = body[4:]
    if body[:2] != b"\x03\x03":
        raise ValueError("client version is not 03 03")
    random = body[2:34]
    offset = 34
    session_id_length = body[offset]
    if session_id_length != 32:
        raise ValueError(f"session id length {session_id_length} != 32")
    session_id = body[offset + 1:offset + 33]
    offset += 33
    cipher_length = int.from_bytes(body[offset:offset + 2], "big")
    offset += 2
    ciphers = body[offset:offset + cipher_length]
    offset += cipher_length
    compression_count = body[offset]
    offset += 1
    compression = body[offset:offset + compression_count]
    offset += compression_count
    extensions_length = int.from_bytes(body[offset:offset + 2], "big")
    offset += 2
    end = offset + extensions_length
    if end != len(body):
        raise ValueError(f"extension block ends at {end}, body is {len(body)}")
    extensions = []
    while offset < end:
        ext_type = int.from_bytes(body[offset:offset + 2], "big")
        ext_length = int.from_bytes(body[offset + 2:offset + 4], "big")
        extensions.append((ext_type, body[offset + 4:offset + 4 + ext_length]))
        offset += 4 + ext_length
    return {
        "length": len(data),
        "random": random,
        "session_id": session_id,
        "ciphers": ciphers,
        "compression": compression,
        "extensions": extensions,
    }


def check_hmac(data: bytes) -> bool:
    zeroed = bytearray(data)
    zeroed[11:43] = b"\x00" * 32
    digest = bytearray(hmac.new(SECRET, bytes(zeroed), hashlib.sha256).digest())
    tail = int.from_bytes(digest[28:32], "little", signed=True) ^ UNIX_TIME
    digest[28:32] = (tail & 0xFFFFFFFF).to_bytes(4, "little")
    return data[11:43] == bytes(digest)


def shape_of(ext_type: int, content: bytes):
    """Everything that is fixed by construction, with the grease words zeroed
    and the random stretches dropped."""
    if ext_type == 0x0000:
        return (0x0000, content)
    if ext_type == 0x000A:
        return (0x000A, content[:2] + b"\x00\x00" + content[4:])
    if ext_type == 0x002B:
        return (0x002B, content[:1] + b"\x00\x00" + content[3:])
    if ext_type == 0x0033:
        return (0x0033, (content[:2], content[4:11]))
    if ext_type == 0xFE0D:
        return (0xFE0D, (content[:5], content[6:8]))
    return (ext_type, content)


def signature(parsed: dict) -> tuple:
    extensions = []
    grease_count = 0
    for ext_type, content in parsed["extensions"]:
        word = ext_type.to_bytes(2, "big")
        if is_grease_word(word):
            grease_count += 1
            continue
        extensions.append(shape_of(ext_type, content))
    ciphers = parsed["ciphers"]
    if not is_grease_word(ciphers[:2]):
        raise ValueError(f"cipher suite list does not start with grease: {ciphers[:2].hex()}")
    return (
        tuple(sorted(extensions)),
        grease_count,
        ciphers[2:],
        parsed["compression"],
    )


def problems(parsed: dict, data: bytes) -> list[str]:
    found = []
    if not check_hmac(data):
        found.append("ClientHello random is not the HMAC of the buffer")
    if parsed["length"] not in TOTAL_LENGTHS:
        found.append(f"unexpected total length {parsed['length']}")
    seen = {ext_type for ext_type, _ in parsed["extensions"]}
    grease = {t for t in seen if is_grease_word(t.to_bytes(2, "big"))}
    if len(grease) != 2:
        found.append(f"expected two grease extensions, saw {len(grease)}")
    missing = FIXED_EXTENSIONS - seen
    if missing:
        found.append("missing extensions " + ", ".join(hex(t) for t in sorted(missing)))
    extra = seen - FIXED_EXTENSIONS - grease
    if extra:
        found.append("unexpected extensions " + ", ".join(hex(t) for t in sorted(extra)))
    for ext_type, content in parsed["extensions"]:
        if ext_type == 0xFE0D and len(content) not in ECH_LENGTHS:
            found.append(f"ech_payload length {len(content)} not in {sorted(ECH_LENGTHS)}")
        if ext_type == 0x0000:
            # server_name: list length, name type 0, name length, name
            name_length = int.from_bytes(content[3:5], "big")
            name = content[5:5 + name_length]
            if name != DOMAIN.encode():
                found.append(f"server name {name!r} != {DOMAIN!r}")
    if parsed["ciphers"][2:] != bytes.fromhex(
        "130113021303c02bc02fc02cc030cca9cca8c013c014009c009d002f0035"
    ):
        found.append("cipher suite list differs from TDLib's")
    return found


def run(binary: str, env: dict[str, str]) -> str:
    proc = subprocess.run(
        [binary, DOMAIN, SECRET.hex(), str(UNIX_TIME), "1"], capture_output=True, env=env
    )
    if proc.returncode != 0:
        raise RuntimeError(f"{binary} exited {proc.returncode}: {proc.stderr.decode('utf-8', 'replace')}")
    line = proc.stdout.decode("utf-8", "replace").strip().splitlines()[0]
    parts = line.split()
    if parts[0] != "hello" or not parts[1].startswith("len="):
        raise RuntimeError(f"unexpected probe output: {line[:80]}")
    return parts[2]


def collect(binary: str, samples: int, env: dict[str, str]) -> list[tuple[bytes, dict]]:
    results = []
    for _ in range(samples):
        data = bytes.fromhex(run(binary, env))
        results.append((data, parse_client_hello(data)))
    return results


def mutate(data: bytes) -> bytes:
    """Flip one byte of the signature_algorithms extension so the comparator
    has to notice."""
    parsed = parse_client_hello(data)
    offset = 5 + 4 + 34 + 33
    offset += 2 + int.from_bytes(data[offset:offset + 2], "big") + 2
    end = offset + int.from_bytes(data[offset:offset + 2], "big")
    offset += 2
    while offset < end:
        ext_type = int.from_bytes(data[offset:offset + 2], "big")
        ext_length = int.from_bytes(data[offset + 2:offset + 4], "big")
        if ext_type == 0x000D:
            mutated = bytearray(data)
            mutated[offset + 4] ^= 0x01
            return bytes(mutated)
        offset += 4 + ext_length
    raise RuntimeError("no signature_algorithms extension to mutate")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--samples", type=int, default=8)
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    sides = {}
    for label, binary in (("oracle", args.cpp), ("rust", args.rs)):
        try:
            samples = collect(binary, args.samples, env)
        except (RuntimeError, ValueError) as error:
            print(f"FAIL {label}: {error}")
            return 1
        found = []
        for data, parsed in samples:
            found.extend(problems(parsed, data))
        if found:
            print(f"FAIL {label} invariants:")
            for line in sorted(set(found)):
                print(f"    {line}")
            return 1
        shapes = {signature(parsed) for _, parsed in samples}
        if len(shapes) != 1:
            print(f"FAIL {label}: {len(shapes)} distinct structures across {len(samples)} samples")
            for shape in list(shapes)[:3]:
                print(f"    {shape}")
            return 1
        sides[label] = shapes.pop()
        lengths = sorted({parsed["length"] for _, parsed in samples})
        print(f"{label}: {len(samples)} samples, length set {lengths}")

    if sides["oracle"] != sides["rust"]:
        print("DIFF structure")
        for a, b in zip(sides["oracle"], sides["rust"]):
            if a != b:
                print(f"    oracle={a!r}")
                print(f"    rust  ={b!r}")
        return 1

    if args.selftest:
        data, _ = collect(args.cpp, 1, env)[0]
        original = signature(parse_client_hello(data))
        changed = signature(parse_client_hello(mutate(data)))
        if original == changed:
            print("SELFTEST FAIL: the comparator did not see a signature_algorithms change")
            return 1
        if check_hmac(mutate(data)):
            print("SELFTEST FAIL: the HMAC check accepted a mutated buffer")
            return 1
        print("SELFTEST OK: comparator sees a fixed-byte change and the HMAC check rejects it")

    print("PASS extensions=%d total_lengths=%s" % (len(FIXED_EXTENSIONS), sorted(TOTAL_LENGTHS)))
    return 0


if __name__ == "__main__":
    sys.exit(main())