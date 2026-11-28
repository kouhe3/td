"""Judge the td::Binlog write path: C++ oracle vs the Rust writer, byte for byte.

    uv run python tools/binlog_write_diff.py
    uv run python tools/binlog_write_diff.py --selftest

Both sides drive the same script against the same output path; the comparison is
the two *files* (bytes) plus the two narrations (stdout, line by line), so a
difference in either the framing or the returned ids shows up.

Unencrypted only: every run uses `DbKey::empty()`, so no AesCtrEncryption header
event exists and the file is plaintext. The encrypted writer (key derivation,
AES-CTR state across flush boundaries) is a different lane.

What the corpus pins down (measured against tddb/td/db/binlog/Binlog.cpp):
  * frame `[size][id][type][flags][extra][data][crc32]`, little endian,
    `size` = 32 + data length, `extra` always 0, crc32 = zlib's crc32 over the
    frame minus its own 4 bytes (BinlogEvent.cpp:66-70);
  * `add` takes the next id, `rewrite` burns a sequence number but writes the
    *target* id with flags=1, `erase` writes type=-2/flags=1 with no data;
  * `size % 4 != 0` is a LOG(FATAL) precondition (Binlog.cpp:232), so a raw
    payload must be 4-byte aligned -- the probe prints `skip … unaligned` where
    the oracle would abort, and both sides must agree on the refusal;
  * only flush/sync/close reach the disk, and how often you flush never changes
    the bytes (the `flushmany`/`flushonce` pair is compared to each other);
  * `last_event_id` is not persisted: a reopen rebuilds it from the replay, which
    only counts appended events, so ids burned by `rewrite`/`erase` and any
    service-type frame are gone -- the counter can even go backwards.
  * the file is not append-only: past 50 KiB with a 5:1 dead ratio `add_event`
    regenerates it from the live set (100 KiB at 4:1, 300 KiB at 3:1, 500 KiB at
    2:1), and each surviving frame keeps the bytes it was written with.

With `--key raw` the same corpus runs under AES-256-CTR. Two encrypted files can
never be byte equal (the header's 32-byte key_salt and 16-byte iv are random per
run, so the keystream -- and therefore every ciphertext byte -- differs), so the
criteria there are:
  * structure: same total size, same first-frame header (size/id/type/flags/extra)
    and the same per-frame size sequence, recovered from the decoders' `size`
    tags (frame size = data + 32) and checked to sum to the file length;
  * td's own decoder reads both files: `binlog_dump.exe` (which validates every
    frame's crc32 while decrypting) and the read lane's `binlog_head.exe` print
    identical `LogEvent[…]` lines for the C++ file and the Rust file -- this is
    what makes ciphertext equality unnecessary: a single wrong keystream byte
    breaks the crc;
  * randomness stays local: two oracle runs of the same script agree on every
    byte up to the salt's length prefix (measured: 33 shared bytes -- the 28-byte
    frame header, the zeroed flags word and the `0x20` salt length) and diverge
    inside key_salt, so nothing structural is random;
  * Python recomputes the key and `key_hash` from the file's own key_salt with
    `hashlib.pbkdf2_hmac` / `hmac` and replays the header-event layout, plus the
    crc32 of that plaintext frame.
`change_key` is driven through `Binlog::change_key`; it regenerates the file with
a fresh header, and the Python check uses the key the script ends with, so a
`change_key raw …` that is then reverted is judged end to end. `change_key empty`
drops encryption and the file is then compared byte for byte again. Opening an
encrypted file with `DbKey::empty()` is `WrongPassword`; neither probe can be
driven there from a script (the file is destroyed at startup), so that path is a
measured fact, not a case.

The Python side recomputes one frame's crc32 with zlib and reads the header
fields back, which is the third opinion next to the two implementations.

Coverage boundary and invalidation: only the unencrypted writer is judged, so a
change to `reset_encryption` / `AesCtrEncryptionEvent` / `change_key` cannot be
seen here. The corpus deliberately touches only live ids, because rewriting or
erasing an unknown id makes `BinlogEventsProcessor` return an error that
`Binlog` turns into LOG(FATAL) on the oracle side; the two `skip …` narrations
cover the alignment precondition instead. Any edit to the frame layout,
`create_raw`, the `size % 4` precondition or the replay's id accounting in
tddb/td/db/binlog/ must be re-measured before this gate is trusted. stdout is
compared; the C++ side's LOG output goes to stderr and is ignored.

Exit codes: 0 = PASS, 1 = diff / self-test failure, 2 = missing binary.

`--encrypted` runs only the encrypted corpus (same entry point, for iteration);
the default and `--selftest` run both halves.
"""

from __future__ import annotations

import argparse
import hashlib
import hmac
import io
import os
import re
import shutil
import struct
import subprocess
import sys
import time
import zlib
from contextlib import redirect_stdout
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/binlog_write_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/binlog_write_probe.exe"
DUMP = Path("C:/tmp/td-build/tddb/binlog_dump.exe")
HEAD = REPO / "rust/target/x86_64-pc-windows-gnu/release/binlog_head.exe"
WORK = REPO / "work" / "binlog-write"

HEADER_SIZE = 4 + 8 + 4 + 4 + 8
TAIL_SIZE = 4
MIN_SIZE = HEADER_SIZE + TAIL_SIZE
HEADER_FLAGS_SIZE = 4  # END_STORE_FLAGS' zeroed uint32 at the start of the header event
SERVICE_AES_CTR_ENCRYPTION = -3
KEY_CHECK = b"cucumbers everywhere"
DEFAULT_KEY = b"cucumber"


def hexed(data: bytes) -> str:
    return data.hex() if data else "-"


BIG = bytes(range(256)) * 256  # 65536 bytes, exactly 64 KiB

CASES = [
    # an empty script: init() creates the file, nothing is ever written
    ("emptyscript", ["# nothing at all"]),
    ("single", ["add 7 41424344", "close"]),
    (
        "multi",
        [
            "add 7 41424344",
            "add 42 4142434445464748",
            "add 1000 4142434445464748494a4b4c",
            "close",
        ],
    ),
    # an empty payload still writes a full 32-byte frame
    ("zerodata", ["add 42 -", "add 7 41424344", "close"]),
    # a 1-byte payload is impossible as a raw Slice (size 33 is not 4-aligned),
    # but a TL `string` pads it to 4 bytes on disk
    ("onebyte", ["add 7 41", "addtl 7 41", "close"]),
    ("big", [f"add 7 {hexed(BIG)}", "close"]),
    (
        "rewrite",
        [
            "add 7 41424344",
            "add 7 45464748",
            "rewrite 1 9 0102030405060708",
            "close",
            "reopen",
            "nextid",
            "close",
        ],
    ),
    (
        "erase",
        [
            "add 7 41424344",
            "add 8 45464748",
            "erase 1",
            "close",
            "reopen",
            "nextid",
            "close",
        ],
    ),
    (
        "interleaved",
        [
            "add 7 41424344",
            "add 8 45464748",
            "rewrite 2 9 00000000",
            "add 10 11121314",
            "erase 1",
            "rewrite 4 11 20212224",
            "add 12 31323334",
            "close",
        ],
    ),
    ("closereopen", ["add 7 41424344", "close", "reopen", "add 7 45464748", "nextid", "close"]),
    ("flushmany", ["add 7 41424344", "flush", "add 7 45464748", "flush", "add 7 48474645", "close"]),
    ("flushonce", ["add 7 41424344", "add 7 45464748", "add 7 48474645", "flush", "close"]),
    # the sequence number spent by a rewrite is never written, so the reopened
    # counter goes backwards (5 -> 3 here) and the next add reuses it
    (
        "reopenids",
        [
            "add 7 41424344",
            "add 8 45464748",
            "nextid",
            "rewrite 2 9 00000000",
            "nextid",
            "close",
            "reopen",
            "nextid",
            "add 7 01020304",
            "close",
        ],
    ),
    ("lazy", ["add 7 41424344", "lazy", "add 8 45464748", "lazy", "add 9 48474645", "close"]),
    (
        "erasebatch",
        [
            "add 7 41424344",
            "add 8 45464748",
            "add 9 48474645",
            "erasebatch 1,3",
            "close",
            "reopen",
            "nextid",
            "close",
        ],
    ),
    # TL string padding: 1 / 10 / 253 / 254 bytes -> 4 / 12 / 256 / 260 on disk
    (
        "strpad",
        [
            f"addtl 7 {hexed(bytes([0x41]))}",
            f"addtl 7 {hexed(bytes(range(10)))}",
            f"addtl 7 {hexed(bytes([0x62]) * 253)}",
            f"addtl 7 {hexed(bytes([0x63]) * 254)}",
            "close",
        ],
    ),
    # a negative type is written but skipped by the replay: nextid does not move
    ("svc", ["add -5 00000000", "add -1 00000000", "add 7 41424344", "close", "reopen", "nextid", "close"]),
    # a Rewrite frame whose target is past everything seen so far is appended as
    # a *live* event with flags=1, while a service frame past it is skipped --
    # and the later add then repeats id 9 in the file
    (
        "quirk",
        [
            "add 7 41424344",
            "rewrite 7 9 41414141",
            "erase 9",
            "close",
            "reopen",
            "nextid",
            "add 7 02020202",
            "close",
        ],
    ),
    ("unaligned", ["add 7 41", "add 7 414243", "rewrite 1 9 41", "add 7 41424344", "close"]),
    # the writer is not append-only: 1500 rewrites of one id push the file past
    # 50 KiB with a 5:1 dead ratio, so add_event reindexes and the final file is
    # the folded live set (measured: 114 frames, 4104 bytes)
    ("compact", ["add 7 41424344"] + ["rewrite 1 7 45464748"] * 1500 + ["add 7 48474645", "close"]),
]

# (case, the key the script ends with, script). Every entry is opened with
# `--key raw` = DbKey::raw_key("cucumber"); `change_key` may move it elsewhere.
ENCRYPTED_CASES = [
    (
        "enc_basic",
        DEFAULT_KEY,
        ["add 7 41424344", "add 8 45464748", "rewrite 1 9 0102030405060708", "erase 2", "close"],
    ),
    # three flushes: the keystream has to continue across flush boundaries
    (
        "enc_flush",
        DEFAULT_KEY,
        ["add 7 41424344", "flush", "add 8 45464748", "flush", "add 9 48474645", "close"],
    ),
    (
        "enc_reopen",
        DEFAULT_KEY,
        ["add 7 41424344", "add 8 45464748", "close", "reopen", "nextid", "add 7 01020304", "close"],
    ),
    # 64 KiB in one frame crosses the lazy_flush threshold mid-stream
    ("enc_big", DEFAULT_KEY, [f"add 7 {hexed(BIG)}", "add 8 45464748", "close"]),
    # change_key to the same key still regenerates the header (new salt and iv)
    (
        "enc_change_same",
        DEFAULT_KEY,
        ["add 7 41424344", "add 8 45464748", "change_key raw 637563756d626572", "add 9 48474645", "close"],
    ),
    # away to another key and back: the final read only works if the second
    # change_key re-encrypted the frames written under the first one
    (
        "enc_rekey",
        DEFAULT_KEY,
        [
            "add 7 41424344",
            "change_key raw 6f74686572206b6579",
            "add 8 45464748",
            "change_key raw 637563756d626572",
            "add 9 48474645",
            "close",
        ],
    ),
    # dropping the key regenerates the file as plaintext: full byte comparison
    (
        "enc_then_plain",
        None,
        ["add 7 41424344", "add 8 45464748", "change_key empty", "add 9 48474645", "close"],
    ),
]

EVENT_RE = re.compile(r"LogEvent\[\[event_id:0x([0-9a-f]+)\]\[type:(-?\d+)\]\[flags:(\d+)\]\[size:(\d+)\]")

# Measured with binlog_head on the oracle's own file: the decrypted frame
# sequence every script must produce, as (id, type, flags, data length). This is
# what the ciphertext has to say once decrypted, and it was copied from a run --
# no AES anywhere in the table. The payload bytes themselves are pinned by the
# cpp-vs-rust decoder comparison.
EXPECTED_EVENTS = {
    "enc_basic": [(0, -3, 0, 96), (1, 7, 0, 4), (2, 8, 0, 4), (1, 9, 1, 8), (2, -2, 1, 0)],
    "enc_flush": [(0, -3, 0, 96), (1, 7, 0, 4), (2, 8, 0, 4), (3, 9, 0, 4)],
    "enc_reopen": [(0, -3, 0, 96), (1, 7, 0, 4), (2, 8, 0, 4), (4, 7, 0, 4)],
    "enc_big": [(0, -3, 0, 96), (1, 7, 0, 65536), (2, 8, 0, 4)],
    "enc_change_same": [(0, -3, 0, 96), (1, 7, 0, 4), (2, 8, 0, 4), (3, 9, 0, 4)],
    "enc_rekey": [(0, -3, 0, 96), (1, 7, 0, 4), (2, 8, 0, 4), (3, 9, 0, 4)],
    "enc_then_plain": [(1, 7, 0, 4), (2, 8, 0, 4), (3, 9, 0, 4)],
}


def frame_prefix(data: bytes):
    """First frame's size and header when the file starts with a valid frame."""
    if len(data) < MIN_SIZE:
        return None
    size = struct.unpack_from("<I", data, 0)[0]
    if size < MIN_SIZE or size > len(data):
        return None
    return size


def is_encrypted(data: bytes) -> bool:
    size = frame_prefix(data)
    return size is not None and struct.unpack_from("<i", data, 12)[0] == SERVICE_AES_CTR_ENCRYPTION


def common_prefix(left: bytes, right: bytes) -> int:
    limit = min(len(left), len(right))
    index = 0
    while index < limit and left[index] == right[index]:
        index += 1
    return index


def tl_string(data: bytes, offset: int):
    """One TL string: length prefix, payload, zero padding. Returns (value, next)."""
    first = data[offset]
    if first < 254:
        length, prefix = first, 1
    elif first == 254:
        length = data[offset + 1] | data[offset + 2] << 8 | data[offset + 3] << 16
        prefix = 4
    else:
        length = int.from_bytes(data[offset + 1 : offset + 6], "little")
        prefix = 8
    start = offset + prefix
    end = start + length
    padded = end + (4 - end % 4) % 4
    return data[start:end], padded


def decoder_lines(exe: Path, path: Path, env) -> tuple[int, list[str]]:
    """`binlog_dump` narrates through LOG(PLAIN) on stderr, `binlog_head` on stdout."""
    proc = subprocess.run([str(exe), str(path)], capture_output=True, env=env)
    text = proc.stdout.decode("utf-8", "replace") + "\n" + proc.stderr.decode("utf-8", "replace")
    return proc.returncode, [line for line in text.splitlines() if line.strip()]


def event_rows(lines: list[str]):
    rows = []
    for line in lines:
        match = EVENT_RE.search(line)
        if match:
            rows.append((int(match.group(1), 16), int(match.group(2)), int(match.group(3)), int(match.group(4))))
    return rows


def mask_header(lines: list[str]) -> list[str]:
    """Hide the one legitimately random line: the header event's salt/iv/hash.

    Everything else a decoder prints is plaintext and must match byte for byte.
    The header payload itself is judged by check_encryption_header and by td
    accepting the file at all (a wrong key makes the decoder fail).
    """
    masked = []
    for line in lines:
        match = EVENT_RE.search(line)
        if match and int(match.group(1), 16) == 0 and int(match.group(2)) == SERVICE_AES_CTR_ENCRYPTION:
            head, _, _ = line.partition("[data:")
            line = head + "[data:<random salt|iv|key_hash>]"
        masked.append(line)
    return masked


def check_encryption_header(name: str, data: bytes, key: bytes):
    """Third opinion: replay the header layout and the key derivation in Python.

    Returns the derived key as hex, or None if any check failed.
    """
    ok = True

    def want(condition: bool, message: str) -> bool:
        nonlocal ok
        if not condition:
            print(f"SELFTEST FAIL: {name}: {message}")
            ok = False
        return condition

    size = frame_prefix(data)
    if not want(size is not None and is_encrypted(data), "the file must start with a type -3 frame"):
        return None
    size = int(size)
    event_id, event_type, flags, extra = struct.unpack_from("<QiiQ", data, 4)
    want(size == 128, f"header frame is 128 bytes, got {size}")
    want(
        (event_id, event_type, flags, extra) == (0, SERVICE_AES_CTR_ENCRYPTION, 0, 0),
        f"header id/type/flags/extra = {event_id}/{event_type}/{flags}/{extra}",
    )
    want(
        struct.unpack_from("<I", data, size - TAIL_SIZE)[0] == zlib.crc32(data[: size - TAIL_SIZE]),
        "header frame crc32",
    )
    payload = data[HEADER_SIZE : size - TAIL_SIZE]
    want(payload[:4] == b"\x00\x00\x00\x00", "END_STORE_FLAGS writes one zeroed u32")
    offset = 4
    salt, offset = tl_string(payload, offset)
    iv, offset = tl_string(payload, offset)
    stored_hash, offset = tl_string(payload, offset)
    want((len(salt), len(iv), len(stored_hash)) == (32, 16, 32), f"salt/iv/hash sizes {len(salt)}/{len(iv)}/{len(stored_hash)}")
    want(offset == len(payload), f"header payload is exactly four fields, {len(payload)} bytes")
    derived = hashlib.pbkdf2_hmac("sha256", key, salt, 2, dklen=32)
    want(
        hmac.new(derived, KEY_CHECK, "sha256").digest() == stored_hash,
        f"key_hash mismatch for key {key!r}: {hmac.new(derived, KEY_CHECK, 'sha256').hexdigest()} != {stored_hash.hex()}",
    )
    return derived.hex() if ok else None


def script_text(cases) -> str:
    lines = []
    for case in cases:
        name, body = case[0], case[-1]
        lines.append(f"# case {name}")
        lines.extend(body)
    return "\n".join(lines) + "\n"


def case_text(body) -> str:
    return "\n".join(body) + "\n"


def run(exe: Path, script: Path, out: Path, env: dict, key: str = "empty"):
    proc = subprocess.run([str(exe), "--key", key, str(script), str(out)], capture_output=True, env=env)
    return proc.returncode, proc.stdout.decode("utf-8", "replace").splitlines()


def window(data: bytes, offset: int, span: int = 24) -> str:
    low = max(0, offset - span)
    return data[low : offset + span].hex() or "-"


def compare_files(name: str, cpp: bytes, rs: bytes) -> bool:
    if cpp == rs:
        return True
    if len(cpp) != len(rs):
        print(f"DIFF {name} size: oracle={len(cpp)} rust={len(rs)}")
    offset = next((i for i in range(min(len(cpp), len(rs))) if cpp[i] != rs[i]), min(len(cpp), len(rs)))
    print(
        f"DIFF {name} offset={offset} "
        f"oracle={window(cpp, offset)} rust={window(rs, offset)}"
    )
    return False


def compare_lines(name: str, cpp, rs) -> bool:
    if cpp == rs:
        return True
    for index in range(max(len(cpp), len(rs))):
        left = cpp[index] if index < len(cpp) else None
        right = rs[index] if index < len(rs) else None
        if left != right:
            print(f"DIFF {name} line {index + 1}: oracle={left!r} rust={right!r}")
            return False
    return True


def frames(data: bytes):
    offset = 0
    rows = []
    while offset + 4 <= len(data):
        size = struct.unpack_from("<I", data, offset)[0]
        if size < MIN_SIZE or offset + size > len(data):
            break
        event_id, event_type, flags, extra = struct.unpack_from("<QiiQ", data, offset + 4)
        crc = struct.unpack_from("<I", data, offset + size - TAIL_SIZE)[0]
        rows.append(
            {
                "offset": offset,
                "size": size,
                "id": event_id,
                "type": event_type,
                "flags": flags,
                "extra": extra,
                "data": data[offset + HEADER_SIZE : offset + size - TAIL_SIZE],
                "crc": crc,
            }
        )
        offset += size
    return rows, offset


def selftest(cpp_files: dict[str, bytes]) -> bool:
    def want(condition: bool, message: str) -> bool:
        if not condition:
            print(f"SELFTEST FAIL: {message}")
        return condition

    ok = True

    # 1. hand-computed frame geometry: add 7 41424344 -> 32 header + 4 data
    single = cpp_files["single"]
    ok &= want(len(single) == 36, f"single must be 36 bytes, got {len(single)}")
    rows, consumed = frames(single)
    ok &= want(len(rows) == 1 and consumed == 36, "single must hold exactly one frame")
    row = rows[0]
    ok &= want(row["size"] == 36, f"first frame size must be 36, got {row['size']}")
    ok &= want(struct.unpack_from("<I", single, 0)[0] == 36, "size field is the first u32")
    ok &= want(row["id"] == 1, f"first add takes id 1, got {row['id']}")
    ok &= want(row["type"] == 7 and row["flags"] == 0 and row["extra"] == 0, "type/flags/extra of a plain add")
    ok &= want(row["data"] == bytes.fromhex("41424344"), "the payload sits right after the 24-byte header")
    # 2. third opinion: crc32 over size-4 bytes, recomputed with zlib
    expected_crc = zlib.crc32(single[: row["size"] - TAIL_SIZE])
    ok &= want(
        row["crc"] == expected_crc,
        f"crc32 over the first {row['size'] - TAIL_SIZE} bytes: file={row['crc']:08x} zlib={expected_crc:08x}",
    )
    ok &= want(
        struct.unpack_from("<I", single, 32)[0] == expected_crc,
        "the crc is the last 4 bytes and equals zlib's crc32 of the frame minus them",
    )
    # recorded once from the oracle, so the check does not rest on zlib alone
    ok &= want(row["crc"] == 0x48903662, f"the single frame's crc is 0x48903662, got {row['crc']:08x}")

    # 3. an empty payload still writes a 32-byte frame
    zero = cpp_files["zerodata"]
    zero_rows, _ = frames(zero)
    ok &= want(len(zero_rows) == 2, "zerodata has two frames")
    ok &= want(zero_rows[0]["size"] == 32 and zero_rows[0]["data"] == b"", "an empty add is a 32-byte frame")
    ok &= want(zero_rows[0]["type"] == 42, "the empty add keeps its type")
    ok &= want(
        zero_rows[0]["crc"] == zlib.crc32(zero[:28]), f"empty-frame crc {zero_rows[0]['crc']:08x} != zlib"
    )

    # 4. the TL string padding rule: 1-byte payload -> 01 41 00 00
    one_rows, _ = frames(cpp_files["onebyte"])
    ok &= want(len(one_rows) == 1, "onebyte writes a single frame (the raw 1-byte add is refused)")
    ok &= want(
        one_rows[0]["data"] == bytes.fromhex("01410000"),
        f"TL string(1 byte) is 01410000, got {one_rows[0]['data'].hex()}",
    )

    # 5. the empty file has the well-known sha256 of nothing
    ok &= want(cpp_files["emptyscript"] == b"", "an empty script leaves a 0-byte binlog")
    ok &= want(
        hashlib.sha256(cpp_files["emptyscript"]).hexdigest()
        == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "sha256 of the empty binlog",
    )

    # 6. flush boundaries never change the bytes
    ok &= want(
        cpp_files["flushmany"] == cpp_files["flushonce"],
        "three flushes and one flush must produce identical files",
    )

    # 7. the dead-ratio rule really compacted: append-only would be ~54 KiB
    compact = cpp_files["compact"]
    compact_rows, compact_consumed = frames(compact)
    ok &= want(len(compact) == 4104, f"compact must shrink to 4104 bytes, got {len(compact)}")
    ok &= want(compact_consumed == len(compact), "compact has no partial frame")
    ok &= want(len(compact_rows) == 114, f"compact keeps 114 frames, got {len(compact_rows)}")
    ok &= want(
        all(row["size"] == 36 for row in compact_rows),
        "every surviving frame is a 36-byte rewrite",
    )

    # 8. one flipped byte must be caught by the same comparator
    corrupted = bytearray(cpp_files["single"])
    corrupted[16] ^= 0x01  # the `extra` field
    captured = io.StringIO()
    with redirect_stdout(captured):
        caught = not compare_files("single", bytes(corrupted), cpp_files["single"])
    ok &= want(caught, "the comparator must catch a flipped `extra` byte")
    ok &= want("offset=16" in captured.getvalue(), f"the diff must name offset 16: {captured.getvalue()!r}")

    return ok


def plaintext_block(args, env) -> tuple[int, int]:
    """Both sides write every case; compare the files and the narration."""
    cpp_files: dict[str, bytes] = {}
    total_bytes = 0
    for name, body in CASES:
        script = WORK / "scripts" / f"{name}.txt"
        script.write_text(case_text(body))
        cpp_out = WORK / "cpp" / f"{name}.binlog"
        rs_out = WORK / "rs" / f"{name}.binlog"
        cpp_rc, cpp_lines = run(Path(args.cpp), script, cpp_out, env)
        rs_rc, rs_lines = run(Path(args.rs), script, rs_out, env)
        if cpp_rc != rs_rc:
            print(f"DIFF {name} exit code: oracle={cpp_rc} rust={rs_rc}")
            return 1
        if cpp_rc != 0:
            print(f"FAIL {name}: both sides exited {cpp_rc}")
            return -1, 0
        cpp_bytes = cpp_out.read_bytes()
        rs_bytes = rs_out.read_bytes()
        cpp_files[name] = cpp_bytes
        total_bytes += len(cpp_bytes) + len(rs_bytes)
        if not compare_files(name, cpp_bytes, rs_bytes):
            return -1, 0
        if not compare_lines(name, cpp_lines, rs_lines):
            return -1, 0

        # FLOOR: the oracle must produce the same file twice
        floor_out = WORK / "cpp" / f"{name}.floor.binlog"
        floor_rc, floor_lines = run(Path(args.cpp), script, floor_out, env)
        if floor_rc != cpp_rc or floor_lines != cpp_lines or floor_out.read_bytes() != cpp_bytes:
            print(f"FLOOR FAIL: the oracle is not deterministic on {name}")
            return -1, 0
        floor_out.unlink()

    if args.selftest:
        if not selftest(cpp_files):
            return -1, 0
        print("SELFTEST OK: frame geometry, zlib crc32, empty payload, TL string padding, flush equivalence")
    return len(CASES), total_bytes


def encrypted_block(args, env) -> tuple[int, int, dict]:
    """AES-256-CTR cases: structure, cross-reads, randomness locality, Python key."""
    # The decoders may regenerate the file they are given (binlog_dump opens with
    # the default `cucumber` key and reindexes a file that is not encrypted that
    # way), so they always read throwaway copies.
    read_dir = WORK / "read"
    shutil.rmtree(read_dir, ignore_errors=True)
    read_dir.mkdir(parents=True, exist_ok=True)

    total_bytes = 0
    measurements = {"frame0": 0, "first_diff": [], "keys": []}
    for name, final_key, body in ENCRYPTED_CASES:
        script = WORK / "scripts" / f"{name}.txt"
        script.write_text(case_text(body))
        cpp_out = WORK / "cpp" / f"{name}.binlog"
        cpp_second = WORK / "cpp" / f"{name}.2.binlog"
        rs_out = WORK / "rs" / f"{name}.binlog"
        cpp_rc, cpp_lines = run(Path(args.cpp), script, cpp_out, env, key="raw")
        second_rc, second_lines = run(Path(args.cpp), script, cpp_second, env, key="raw")
        rs_rc, rs_lines = run(Path(args.rs), script, rs_out, env, key="raw")
        if cpp_rc != rs_rc:
            print(f"DIFF {name} exit code: oracle={cpp_rc} rust={rs_rc}")
            return -1, 0, measurements
        if cpp_rc != 0:
            print(f"FAIL {name}: both sides exited {cpp_rc}")
            return -1, 0, measurements
        if second_rc != 0:
            print(f"FLOOR FAIL: the oracle exited {second_rc} on the second run of {name}")
            return -1, 0, measurements
        cpp = cpp_out.read_bytes()
        again = cpp_second.read_bytes()
        rust = rs_out.read_bytes()
        total_bytes += len(cpp) + len(again) + len(rust)

        # the narration carries no salt/iv/hash, so it must match exactly
        if not compare_lines(f"{name} narration", cpp_lines, rs_lines):
            return -1, 0, measurements
        if second_lines != cpp_lines:
            print(f"FLOOR FAIL: the oracle narration changed on the second run of {name}")
            return -1, 0, measurements

        # 1. two independent decoders read both implementations' output; the
        #    decrypted frame sequence must be the one the script asked for
        cpp_copy = read_dir / f"{name}.cpp.binlog"
        rust_copy = read_dir / f"{name}.rs.binlog"
        second_copy = read_dir / f"{name}.second.binlog"
        for tool, label in ((DUMP, "binlog_dump"), (HEAD, "binlog_head")):
            if not Path(tool).exists():
                print(f"MISSING {tool}")
                return -1, 0, measurements
            # fresh copies per tool: binlog_dump reindexes (and therefore rewrites)
            # a file that is not encrypted with the key it opens with
            cpp_copy.write_bytes(cpp)
            rust_copy.write_bytes(rust)
            second_copy.write_bytes(again)
            cpp_status, cpp_view = decoder_lines(Path(tool), cpp_copy, env)
            rust_status, rust_view = decoder_lines(Path(tool), rust_copy, env)
            _, second_view = decoder_lines(Path(tool), second_copy, env)
            if cpp_status != 0 or rust_status != 0:
                detail = rust_view[-1] if rust_view else ""
                print(
                    f"DIFF {name} {label} exit: oracle={cpp_status} rust={rust_status} "
                    f"last={detail[:160]!r}"
                )
                return -1, 0, measurements
            cpp_view = mask_header(cpp_view)
            rust_view = mask_header(rust_view)
            second_view = mask_header(second_view)
            if not compare_lines(f"{name} {label} cpp-vs-rust", cpp_view, rust_view):
                return -1, 0, measurements
            if not compare_lines(f"{name} {label} cpp-vs-second", cpp_view, second_view):
                return -1, 0, measurements
            rows = event_rows(cpp_view)
            decoded_size = sum(row[3] + MIN_SIZE for row in rows)
            if decoded_size != len(cpp):
                print(f"DIFF {name} {label} structure: decoded {decoded_size} bytes, file has {len(cpp)}")
                return -1, 0, measurements
            if label == "binlog_dump":
                # identical per-frame size sequence on both implementations
                rust_rows = event_rows(rust_view)
                if [row[3] for row in rows] != [row[3] for row in rust_rows]:
                    print(f"DIFF {name} frame size sequence: {[row[3] for row in rows]} != {[row[3] for row in rust_rows]}")
                    return -1, 0, measurements
                if rows != EXPECTED_EVENTS[name]:
                    print(f"DIFF {name} plaintext: decoded {rows} != expected {EXPECTED_EVENTS[name]}")
                    return -1, 0, measurements

        if not is_encrypted(cpp):
            # `change_key empty` dropped encryption: back to full byte equality
            if not compare_files(name, cpp, rust) or cpp != again:
                return -1, 0, measurements
            continue

        # 2. structure: same total size and same first-frame header
        if len(cpp) != len(rust) or len(cpp) != len(again):
            print(f"DIFF {name} size: oracle={len(cpp)} second={len(again)} rust={len(rust)}")
            return -1, 0, measurements
        if cpp[:HEADER_SIZE] != rust[:HEADER_SIZE] or cpp[:HEADER_SIZE] != again[:HEADER_SIZE]:
            print(
                f"DIFF {name} header offset=4 "
                f"oracle={cpp[4:HEADER_SIZE].hex()} rust={rust[4:HEADER_SIZE].hex()}"
            )
            return -1, 0, measurements

        # 3. randomness locality: identical up to the salt, then random
        divergence = common_prefix(cpp, again)
        measurements["first_diff"].append(divergence)
        measurements["frame0"] = frame_prefix(cpp) or 0
        if divergence < HEADER_SIZE + HEADER_FLAGS_SIZE:
            print(f"DIFF {name} randomness leaked: two oracle runs share only {divergence} bytes")
            return -1, 0, measurements
        if cpp == again:
            print(f"DIFF {name} randomness: two oracle runs are byte identical")
            return -1, 0, measurements

        # 4. Python: header layout, PBKDF2 key and HMAC key_hash from the file's salt
        if final_key is None:
            print(f"SELFTEST FAIL: {name}: an encrypted file cannot have a null final key")
            return -1, 0, measurements
        derived = check_encryption_header(name, cpp, final_key)
        if derived is None:
            return -1, 0, measurements
        if check_encryption_header(f"{name} rust", rust, final_key) is None:
            return -1, 0, measurements
        measurements["keys"].append(derived)

    return len(ENCRYPTED_CASES), total_bytes, measurements


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpp", default=DEFAULT_CPP)
    parser.add_argument("--rs", default=str(DEFAULT_RS))
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument("--encrypted", action="store_true", help="run only the AES-256-CTR corpus")
    args = parser.parse_args()

    for path in (args.cpp, args.rs):
        if not Path(path).exists():
            print(f"MISSING {path}")
            return 2

    text = script_text(CASES) + script_text(ENCRYPTED_CASES)
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    for sub in ("scripts", "cpp", "rs"):
        shutil.rmtree(WORK / sub, ignore_errors=True)
        (WORK / sub).mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    started = time.time()
    cases = 0
    total_bytes = 0
    if not args.encrypted:
        plain_cases, plain_bytes = plaintext_block(args, env)
        if plain_cases < 0:
            return 1
        cases += plain_cases
        total_bytes += plain_bytes
    enc_cases, enc_bytes, measurements = encrypted_block(args, env)
    if enc_cases < 0:
        return 1
    cases += enc_cases
    total_bytes += enc_bytes
    if args.selftest and measurements["first_diff"]:
        print(
            f"SELFTEST ENC: cases={enc_cases} header={measurements['frame0']}B "
            f"first_diff={measurements['first_diff']} key[0]={measurements['keys'][0] if measurements['keys'] else '-'}"
        )

    elapsed = time.time() - started
    print(f"PASS cases={cases} bytes={total_bytes} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())