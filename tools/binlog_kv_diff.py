"""Judge td::BinlogKeyValue<td::Binlog> + td::SeqKeyValue: C++ oracle vs the Rust port.

    uv run python tools/binlog_kv_diff.py
    uv run python tools/binlog_kv_diff.py --selftest

Both sides run the same script against the same output path (work/binlog-kv/),
and every case is judged on all four criteria:

  (a) the two written binlogs are byte-identical;
  (b) the C++ BinlogKeyValue opens the *Rust* file (`attach` script) and the
      resulting dump equals the C++ dump of the C++ file;
  (c) the Rust side opens the C++ file and its dump equals the Rust dump of the
      Rust file -- i.e. each implementation's replay accepts the other's bytes;
  (d) the narration (op results, sequence numbers, sorted map dumps, file size
      and sha256) is identical line by line;
  plus FLOOR: a second oracle run must produce the same file and the same lines.

What the corpus pins down (measured; docs/facts.md F191/F192):
  * the map keeps `(value, event_id)`, not the sequence number: a changed value
    writes `create_raw(previous_event_id, magic, Rewrite, key+value TL strings)`
    and returns the new sequence number, while a first-time `set` writes
    `id = seq_no` because there is no previous id;
  * an unchanged `set` returns 0 and writes nothing (`same_twice`);
  * `erase` writes `type = -2`, `flags = 1` and *no data at all* (32 bytes), so it
    is distinguishable from `set key ""` (`empty_value`), whose frame still
    carries two TL strings; `get` cannot tell them apart, `isset` can;
  * `erase_batch` returns `last + 1` (its `next_event_id(0)` does not move the
    counter) and then burns one id per erased key, writing frames in script
    order -- so the returned number stays behind the counter;
  * `erase_by_prefix` uses `next_event_id(count)` (the counter advances by the
    match count) and leaves the returned number unused;
  * `init` replays every *live* event into the map with no type/magic filter
    (TdDb's own switch is the filter, TdDb.cpp:170-181), and the id counter is
    rebuilt from that replay, so it drops back to the highest *appended* id
    across a reopen (`reopen_counter`);
  * `init` begins with `close()` and `close()` is `*this = BinlogKeyValue()`, so
    *every* init without an override resets `magic_` to MAGIC (0x2a280000) as
    well as dropping the map: only `init(..., override_magic = n)` installs an
    override, and it lasts exactly until the next `reopen` (`magic 17191` then
    `reopen` then `set` writes 0x2a280000, and so does `magic 0`);
  * `set` of an empty key is `CHECK(!key.empty())` on the oracle; the probe prints
    `reject` and both sides must agree instead of dying;
  * an open Binlog holds a whole-file write lock, so `dumpfile` is only legal
    after `close` (`reject dumpfile reason=open` otherwise);
  * `SeqKeyValue` is memory only: unchanged `set` -> 0, every other effective
    mutation burns one id, and `erase_batch` returns `current_id_ + 1` while
    *adding* the count (`sq_*` family, judged from the same script).

Coverage boundary and invalidation: the lanes below the KV are judged by the two
neighbouring instruments (`tools/binlog_write_diff.py` owns framing/crc32/ids,
the read lane owns the replay), and this corpus only re-checks them end to end
through the KV. Not covered: encryption (the KV is opened with `DbKey::empty()`),
`external_init_*` (the path TdDb actually uses, which needs an actor-driven
Binlog), and a multi-match `eraseprefix` -- `remove_if` walks a
`WaitFreeHashMap`, i.e. an unordered_map, so the *order* of the frames it writes
is a hash-table artifact that cannot be reproduced without porting the table;
`erase_batch` covers multi-key removal with a script-defined order instead. Any
edit to BinlogKeyValue.h, SeqKeyValue.h, Binlog.cpp's id accounting or
BinlogEventsProcessor must be re-measured before this gate is trusted.

`--selftest` adds hand-computed facts next to the two implementations: the body
of a single `set`, the TL string length forms for 253 / 254 / 65536 bytes, one
map dump, the empty body of an erase frame, and a zlib recomputation of the first
frame's crc32.

Exit codes: 0 = PASS, 1 = diff / self-test failure, 2 = missing binary.

`bytes` in the PASS line counts every file byte the comparisons touched: the two
files a case produced (oracle + rust) plus the four cross-read copies, which are
the same bytes a second time.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import struct
import subprocess
import sys
import time
import zlib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/binlog_kv_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/binlog_kv_probe.exe"
WORK = REPO / "work" / "binlog-kv"

HEADER_SIZE = 4 + 8 + 4 + 4 + 8
TAIL_SIZE = 4
MIN_SIZE = HEADER_SIZE + TAIL_SIZE
FLAG_REWRITE = 1
SERVICE_EMPTY = -2
MAGIC = 0x2A280000

# Fixed value shapes. The keys are ASCII so a failure is readable in the
# narration; the 253/254-byte keys and the 253/254/65536-byte values are the
# three TL string length forms (1-byte, 0xFE + 3-byte LE, no 0xFF form).
V0 = b""
V1 = b"x"
V253 = bytes([0x33]) * 253
V254 = bytes([0x34]) * 254
BIG = bytes(range(256)) * 256  # 65536 bytes
K253 = b"T" * 253
K254 = b"U" * 254


def hexed(data: bytes) -> str:
    return data.hex() if data else "-"


def mid(fill: int) -> bytes:
    return bytes([fill]) * 12288


CASES = [
    (
        "single",
        [
            "set 61 62",
            "get 61",
            "isset 61",
            "isset 62",
            "getall",
            "prefix -",
            "foreach",
            "sync",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "overwrite",
        [
            "set 6b 7631",
            "set 6b 7632",
            "get 6b",
            "getall",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "same_twice",
        [
            "set 6b 7631",
            "set 6b 7631",
            "set 6b 7631",
            "seq",
            "getall",
            "close",
            "dumpfile",
        ],
    ),
    (
        "erase",
        [
            "set 61 31",
            "set 62 32",
            "erase 61",
            "isset 61",
            "get 61",
            "erase 61",
            "erase 7a",
            "set 61 33",
            "getall",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "erase_batch",
        [
            "set 61 31",
            "set 62 32",
            "set 63 33",
            "erase_batch 61 63 7a 61",
            "getall",
            "seq",
            "erase_batch 7a",
            "seq",
            "set 64 34",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "erase_prefix",
        [
            "set 7072652f61 31",
            "set 7072652f62 32",
            "set 6f74686572 33",
            "eraseprefix 7072652f61",
            "getall",
            "eraseprefix 7a7a",
            "getall",
            "erase 7072652f62",
            "erase 6f74686572",
            "set 7072652f63 34",
            "eraseprefix -",
            "getall",
            "set 61 35",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "empty_value",
        [
            "set 65 -",
            "isset 65",
            "get 65",
            "set 65 -",
            "set 65 62",
            "set 65 -",
            "isset 65",
            "get 65",
            "erase 65",
            "isset 65",
            "get 65",
            "getall",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "empty_key",
        [
            "set - 61",
            "get -",
            "isset -",
            "erase -",
            "erase_batch -",
            "eraseprefix -",
            "getall",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "key_lengths",
        [
            f"set {hexed(K253)} 78",
            f"set {hexed(K254)} {hexed(V254)}",
            f"get {hexed(K253)}",
            f"get {hexed(K254)}",
            "getall",
            "prefix 54",
            "foreach",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "value_lengths",
        [
            f"set 7630 {hexed(V0)}",
            f"set 7631 {hexed(V1)}",
            f"set 7632 {hexed(V253)}",
            f"set 7633 {hexed(V254)}",
            "get 7632",
            "getall",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "big_value",
        [
            f"set 626967 {hexed(BIG)}",
            f"set 6b {hexed(BIG)}",
            "get 626967",
            "getall",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "reindex",
        # Six distinct 12 KiB versions of one key push the file past the 50 KiB /
        # 5:1 rule, so Binlog::add_event regenerates it as the live set only.
        [f"set 626967 {hexed(mid(0x41 + index))}" for index in range(6)]
        + ["set 6b 78", "getall", "seq", "close", "dumpfile"],
    ),
    (
        "magic_pmc",
        [
            "magic 17191",
            "set 61 62",
            "set 61 63",
            "erase 61",
            "set 64 65",
            "reopen",
            "set 66 67",
            "close",
            "reopen",
            "set 68 69",
            "getall",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "magic_config",
        [
            "magic 7951",
            "set 61 62",
            "magic 0",
            "set 63 64",
            "close",
            "dumpfile",
            "reopen",
            "set 65 66",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "magic_unknown",
        [
            "magic 424242",
            "set 61 62",
            "close",
            "dumpfile",
            "reopen",
            "getall",
            "set 61 63",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "magic_negative",
        # A negative override is a service type: an appended frame is dropped by
        # the replay, but a *rewrite* of a live id still folds into the live set
        # (BinlogEventsProcessor::do_event checks Rewrite before `type < 0`), so
        # the negative-typed entry is what the KV parses on the next load.
        [
            "set 61 62",
            "magic -5",
            "set 61 63",
            "set 64 65",
            "getall",
            "close",
            "dumpfile",
            "reopen",
            "getall",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "reopen_counter",
        [
            "set 61 31",
            "set 62 32",
            "set 61 33",
            "reopen",
            "seq",
            "set 63 34",
            "set 61 35",
            "set 63 34",
            "erase 62",
            "reopen",
            "getall",
            "seq",
            "set 64 36",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "foreign_event",
        # A KV loader has no type filter: type 0 (which TdDb's switch would
        # LOG(FATAL) on) and type 12345 both become live entries and both bodies
        # are parsed as `[key][value] TL strings` -- the 12345 body's last three
        # bytes are non-zero *padding* of the empty value, which `fetch_string`
        # discards without checking, so that entry loads as `62=-`. A later `set`
        # of the foreign key rewrites the foreign event's own id. `raw` while the
        # KV is open, and an unaligned body, must be refused identically.
        [
            "set 61 62",
            "raw 12345 017a000001680000",
            "close",
            "raw 12345 017a00000168",
            "raw 0 017a000001680000",
            "raw 12345 0162000000310000",
            "reopen",
            "getall",
            "set 7a 6e",
            "reopen",
            "getall",
            "seq",
            "close",
            "dumpfile",
        ],
    ),
    (
        "closed_ops",
        # Every op that needs an open binlog must refuse identically once the KV
        # has been closed (`close()` is `*this = BinlogKeyValue()`: no binlog_).
        [
            "set 61 62",
            "close",
            "set 61 62",
            "erase 61",
            "erase_batch 61",
            "eraseprefix 61",
            "get 61",
            "isset 61",
            "getall",
            "prefix 61",
            "foreach",
            "sync",
            "seq",
            "dumpfile",
        ],
    ),
    (
        "seqkeyvalue",
        [
            "sq_set 61 31",
            "sq_set 61 31",
            "sq_set 61 32",
            "sq_erase 7a",
            "sq_erase 61",
            "sq_erase 61",
            "sq_getall",
            "sq_seq",
            "sq_size",
            "sq_set 62 32",
            "sq_set 63 33",
            "sq_erase_batch 62 63 7a 62",
            "sq_getall",
            "sq_seq",
            "sq_get 62",
            "sq_isset 62",
            "sq_set - 31",
            "close",
            "dumpfile",
        ],
    ),
    (
        "ordering",
        [
            "set 62 31",
            "set 61 32",
            "set 6161 33",
            "set 6162 34",
            "set 63 35",
            "set 6b 36",
            "getall",
            "foreach",
            "prefix 61",
            "prefix 6161",
            "prefix -",
            "prefix 7a7a",
            "seq",
            "set 61 37",
            "getall",
            "close",
            "dumpfile",
        ],
    ),
]

# (b)/(c): open the file the *other* implementation wrote, in a fresh copy so
# neither side can disturb the other's input.
CROSS = ["attach", "getall", "prefix -", "foreach", "close", "dumpfile"]


def window(data: bytes, offset: int, span: int = 24) -> str:
    low = max(0, offset - span)
    return data[low : offset + span].hex() or "-"


def compare_files(name: str, cpp: bytes, rs: bytes) -> bool:
    if cpp == rs:
        return True
    if len(cpp) != len(rs):
        print(f"DIFF {name} size: oracle={len(cpp)} rust={len(rs)}")
    offset = next((i for i in range(min(len(cpp), len(rs))) if cpp[i] != rs[i]), min(len(cpp), len(rs)))
    print(f"DIFF {name} offset={offset} oracle={window(cpp, offset)} rust={window(rs, offset)}")
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
                "crc": crc,
                "data": data[offset + HEADER_SIZE : offset + size - TAIL_SIZE],
            }
        )
        offset += size
    return rows, offset


def tl_string(data: bytes, offset: int):
    """One TL string: length prefix, payload, zero padding. Returns (value, next)."""
    first = data[offset]
    if first < 254:
        length, prefix = first, 1
    elif first == 254:
        length, prefix = int.from_bytes(data[offset + 1 : offset + 4], "little"), 4
    else:
        raise AssertionError("the 0xFF TL string form is out of scope")
    start = offset + prefix
    end = start + length
    padded = end + (-end) % 4
    return data[start:end], padded


def script_text(cases) -> str:
    lines = []
    for name, body in cases:
        lines.append(f"# {name}")
        lines.extend(body)
    return "\n".join(lines) + "\n"


def case_text(body) -> str:
    return "\n".join(body) + "\n"


def run(exe: Path, script: Path, out: Path, env: dict):
    proc = subprocess.run([str(exe), str(script), str(out)], capture_output=True, env=env)
    return (
        proc.returncode,
        proc.stdout.decode("utf-8", "replace").splitlines(),
        proc.stderr.decode("utf-8", "replace").strip(),
    )


def narration(lines, prefix: str):
    return [line for line in lines if line.startswith(prefix)]


def selftest(cpp_files: dict[str, bytes], cpp_lines: dict[str, list[str]]) -> bool:
    def want(condition: bool, message: str) -> bool:
        if not condition:
            print(f"SELFTEST FAIL: {message}")
        return condition

    ok = True

    # 1. hand-computed body of one `set 61 62`: two TL strings, 1-byte lengths,
    #    padding to 4, so 8 data bytes and a 40-byte frame.
    single = cpp_files["single"]
    rows, consumed = frames(single)
    ok &= want(len(single) == 40 and consumed == 40, f"single must be one 40-byte frame, got {len(single)}")
    ok &= want(len(rows) == 1, "single must hold exactly one frame")
    row = rows[0]
    ok &= want(row["id"] == 1, f"first set takes id 1, got {row['id']}")
    ok &= want(row["type"] == MAGIC, f"default magic 0x2a280000, got {row['type']:#x}")
    ok &= want(row["flags"] == 0 and row["extra"] == 0, "a first set has flags 0 and extra 0")
    ok &= want(row["data"] == bytes.fromhex("0161000001620000"), f"set body is {row['data'].hex()}")
    body, next_offset = tl_string(row["data"], 0)
    ok &= want(body == b"a" and next_offset == 4, "TL string 'a' occupies 4 bytes")
    # 2. third opinion: crc32 over size-4 bytes, recomputed with zlib
    source = single[: row["size"] - TAIL_SIZE]
    ok &= want(row["crc"] == zlib.crc32(source) & 0xFFFFFFFF, "the first frame's crc32 is zlib's")
    ok &= want(struct.unpack_from("<I", single, row["size"] - TAIL_SIZE)[0] == zlib.crc32(source) & 0xFFFFFFFF,
               "the stored crc sits in the last 4 bytes")
    # 3. one map dump, hand-computed
    ok &= want(narration(cpp_lines["single"], "getall") == ["getall n=1 61=62"], "single's getall line")
    ok &= want(narration(cpp_lines["single"], "seq") == ["seq 1"], "single's last sequence number")

    # 4. the three TL string length forms seen from the file
    rows, _ = frames(cpp_files["key_lengths"])
    long_key = rows[0]["data"]
    ok &= want(long_key[0] == 253, f"a 253-byte string keeps the 1-byte form, got {long_key[0]}")
    ok &= want(long_key[1:254] == K253, "the 253-byte key payload")
    ok &= want(long_key[254:256] == b"\x00\x00", "the 253-byte key pads to a multiple of 4")
    wider = rows[1]["data"]
    ok &= want(wider[0] == 254 and wider[1:4] == (254).to_bytes(3, "little"), "a 254-byte string uses 0xFE + 3 bytes")
    ok &= want(wider[4:258] == K254, "the 254-byte key payload")
    big_rows, _ = frames(cpp_files["big_value"])
    ok &= want(big_rows[0]["size"] == 32 + 4 + 4 + 65536, f"64 KiB value frame is 65576 bytes, got {big_rows[0]['size']}")
    big_data = big_rows[0]["data"]
    # key "big" is 4 bytes of TL (1-byte length + pad), so the value's prefix is at 4
    ok &= want(big_data[4] == 254 and big_data[5:8] == (65536).to_bytes(3, "little"),
               "a 64 KiB string uses 0xFE + 3 bytes")
    ok &= want(big_data[8:] == BIG, "the 64 KiB payload")

    # 5. an erase frame carries no data at all, unlike `set key ""`
    rows, _ = frames(cpp_files["erase"])
    erases = [row for row in rows if row["type"] == SERVICE_EMPTY]
    # `erase 61` on a present key writes one frame; the two following erases are
    # no-ops (absent key, absent key), so the file keeps exactly one
    ok &= want(len(erases) == 1, f"only the erasure of a live key writes a frame, got {len(erases)}")
    ok &= want(all(row["size"] == MIN_SIZE and row["data"] == b"" for row in erases), "erase frames are 32 empty bytes")
    ok &= want(all(row["flags"] == FLAG_REWRITE for row in erases), "erase frames are rewrites")
    ok &= want(all(row["id"] == 1 for row in erases), "erase targets the key's own event id")
    empty_rows, _ = frames(cpp_files["empty_value"])
    ok &= want(empty_rows[0]["type"] == MAGIC and len(empty_rows[0]["data"]) == 8,
               "set of an empty value is a magic-typed frame with two TL strings")

    # 6. magic overrides change the event type, and close() resets it
    types = [row["type"] for row in frames(cpp_files["magic_pmc"])[0]]
    # 17191 lasts until the first `reopen`: init starts with close(), and close()
    # is `*this = BinlogKeyValue()`, i.e. magic_ = MAGIC again
    ok &= want(types == [17191, 17191, SERVICE_EMPTY, 17191, MAGIC, MAGIC], f"magic_pmc types {types}")
    config_types = [row["type"] for row in frames(cpp_files["magic_config"])[0]]
    ok &= want(config_types == [7951, MAGIC, MAGIC], f"magic_config types {config_types}")
    ok &= want(narration(cpp_lines["magic_config"], "magic") == ["magic 7951 ok=1 size=0", "magic 0 ok=1 size=1"],
               "both magic commands report the map they just replayed")

    # 7. a foreign event type is loaded like any other KV event, and the rewrite
    #    of that key targets the foreign event's id
    ok &= want(narration(cpp_lines["foreign_event"], "getall")[0] == "getall n=3 61=62 62=- 7a=68",
               "type 0 and type 12345 are parsed as KV events")
    foreign = frames(cpp_files["foreign_event"])[0]
    ok &= want([row["type"] for row in foreign] == [MAGIC, 0, 12345, MAGIC], "the foreign types stay in the file")
    ok &= want(foreign[-1]["id"] == 2 and foreign[-1]["flags"] == FLAG_REWRITE,
               "the rewrite of the foreign key targets the foreign event id")
    return ok


def cross_read(args, env, name: str, cpp_bytes: Path, rs_bytes: Path, total_bytes: int) -> tuple[bool, int]:
    """(b), (c) and the cross-read half of (d): four fresh copies.

    Every run and every comparison is reported, even after the file comparison
    already failed: the cross narration then says what each loader makes of the
    other side's bytes (a folded rewrite, a dropped erase frame, a refused file),
    which is the part (a) cannot see.
    """
    read_dir = WORK / "read"
    paths = {
        "cpp_own": read_dir / f"{name}.cpp.cpp.binlog",
        "cpp_rs": read_dir / f"{name}.rs.cpp.binlog",
        "rs_own": read_dir / f"{name}.rs.rs.binlog",
        "rs_cpp": read_dir / f"{name}.cpp.rs.binlog",
    }
    shutil.copyfile(cpp_bytes, paths["cpp_own"])
    shutil.copyfile(rs_bytes, paths["cpp_rs"])
    shutil.copyfile(rs_bytes, paths["rs_own"])
    shutil.copyfile(cpp_bytes, paths["rs_cpp"])
    script = WORK / "scripts" / "cross.txt"

    results = {}
    ok = True
    for key, exe in (
        ("cpp_own", Path(args.cpp)),
        ("cpp_rs", Path(args.cpp)),
        ("rs_own", Path(args.rs)),
        ("rs_cpp", Path(args.rs)),
    ):
        rc, lines, stderr = run(exe, script, paths[key], env)
        if rc != 0:
            print(f"DIFF cross={key}/{name} exit={rc} stderr={stderr!r}")
            ok = False
            continue
        results[key] = lines
        total_bytes += paths[key].stat().st_size

    for label, left, right in (
        ("cpp-on-rust", "cpp_own", "cpp_rs"),
        ("rust-on-cpp", "rs_own", "rs_cpp"),
        ("cross-narration", "cpp_own", "rs_own"),
    ):
        if left in results and right in results:
            if not compare_lines(f"{name} {label}", results[left], results[right]):
                ok = False
    return ok, total_bytes


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

    text = script_text(CASES) + "# cross\n" + "\n".join(CROSS) + "\n"
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    for sub in ("scripts", "cpp", "rs", "read"):
        shutil.rmtree(WORK / sub, ignore_errors=True)
        (WORK / sub).mkdir(parents=True, exist_ok=True)
    (WORK / "scripts" / "cross.txt").write_text(case_text(CROSS))

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    started = time.time()
    total_bytes = 0
    cpp_files: dict[str, bytes] = {}
    cpp_lines: dict[str, list[str]] = {}

    for name, body in CASES:
        script = WORK / "scripts" / f"{name}.txt"
        script.write_text(case_text(body))
        cpp_out = WORK / "cpp" / f"{name}.binlog"
        rs_out = WORK / "rs" / f"{name}.binlog"
        cpp_rc, cpp_lines[name], cpp_err = run(Path(args.cpp), script, cpp_out, env)
        rs_rc, rs_lines, rs_err = run(Path(args.rs), script, rs_out, env)
        if cpp_rc != rs_rc:
            print(f"DIFF {name} exit code: oracle={cpp_rc} rust={rs_rc} stderr={cpp_err!r} {rs_err!r}")
            return 1
        if cpp_rc != 0:
            print(f"FAIL {name}: both sides exited {cpp_rc}: {cpp_err!r} {rs_err!r}")
            return 1
        cpp_bytes = cpp_out.read_bytes()
        rs_bytes = rs_out.read_bytes()
        cpp_files[name] = cpp_bytes
        total_bytes += len(cpp_bytes) + len(rs_bytes)

        case_ok = compare_files(name, cpp_bytes, rs_bytes)
        case_ok &= compare_lines(f"{name} narration", cpp_lines[name], rs_lines)

        # FLOOR: the oracle must produce the same file and the same lines twice
        floor_out = WORK / "cpp" / f"{name}.floor.binlog"
        floor_rc, floor_lines, _ = run(Path(args.cpp), script, floor_out, env)
        if floor_rc != cpp_rc or floor_lines != cpp_lines[name] or floor_out.read_bytes() != cpp_bytes:
            print(f"FLOOR FAIL: the oracle is not deterministic on {name}")
            return 1
        floor_out.unlink()

        ok, total_bytes = cross_read(args, env, name, cpp_out, rs_out, total_bytes)
        if not (case_ok and ok):
            return 1

    if args.selftest:
        if not selftest(cpp_files, cpp_lines):
            return 1
        print(
            "SELFTEST OK: set body, TL string forms, map dump, erase frame, magic types, "
            "foreign types, zlib crc32"
        )

    elapsed = time.time() - started
    print(f"PASS cases={len(CASES)} bytes={total_bytes} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())