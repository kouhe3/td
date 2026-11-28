"""Judge `td::TQueueBinlog<td::Binlog>`: C++ oracle vs the Rust port.

    uv run python tools/tqueue_binlog_diff.py
    uv run python tools/tqueue_binlog_diff.py --selftest

Both sides run the same script against a real `td::Binlog` (plaintext,
`DbKey::empty()`) with `TQueueBinlog<Binlog>` as the queue's storage callback, so
every queue mutation becomes a binlog frame. Four criteria run to the end even
after one fails:

  (a) the two binlog files are **byte-identical** (the corpus stays far below
      the 50 KiB reindex threshold, so no regeneration happens);
  (b) the narration is line by line;
  (c) **cross replay**: the C++ side opens the Rust file and vice versa, and
      both report the same live frame count, head, tail and size;
  (d) FLOOR: a second oracle run produces the same file and the same lines.

What the corpus pins down (measured; docs/facts.md F202/F203):

  * the frame body is `TQueueLogEvent`: `queue_id:i64 ‖ event_id:i32 ‖
    expires_at:i32 ‖ data:TL-string ‖ [extra:i64 only when != 0]`, so `extra = 7`
    makes a 28-byte frame and `extra = 0` a 20-byte one;
  * `magic = BINLOG_EVENT_TYPE + (extra != 0)` -- 2314 or 2315 -- and a rewrite
    re-reads the *current* extra, so an event whose data was cleared rewrites as
    2314 even if it was written as 2315;
  * an erase frame is `type = -2`, `flags = 1`, no data (32 bytes), and its id is
    the erased event's log_event_id, never a sequence number;
  * `erase_batch` takes `next_event_id(0)` (the counter does not move) and then
    erases one id at a time;
  * replay goes through `BinlogEventsProcessor`, i.e. the *live* set: a folded
    rewrite replaces the earlier frame and an erased id disappears, so the frame
    count the queue sees is not the frame count on disk.

Encrypted generation (`--key raw` = `DbKey::raw_key("cucumber")`): the queue is
the same, so the whole narration except the file digest must stay identical and
the file must be exactly 128 bytes longer (the plaintext `type = -3` header;
AES-256-CTR is length preserving). The bytes themselves are never compared --
salt and iv are fresh per generation -- so four criteria replace them:

  (e) the encrypted file's structure: first frame `id=0 type=-3 size=128`, and
      Python re-derives `PBKDF2-HMAC-SHA256("cucumber", salt, 2)` and
      `HMAC(key, "cucumbers everywhere")` from the file's own salt;
  (f) cross replay with the key (C++ reads the Rust file and vice versa);
  (g) a wrong-key pair: `--key empty` on an encrypted file is `Wrong password`
      (exit 3) and the narration is identical on both sides;
  (h) FLOOR on the encrypted side: a second run agrees on size and on the
      narration with the digest masked.

Not covered: reindex under a key (the corpus is far below the 50 KiB
threshold), reopening a *plaintext* file with a key (`Binlog::init` would
regenerate it), and `TQueue::close(Promise)` -- each with a return condition in
F202/F277.
"""

from __future__ import annotations

import argparse
import hashlib
import hmac
import os
import shutil
import struct
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MSYS_BIN = "C:/msys64/ucrt64/bin"
DEFAULT_CPP = "C:/tmp/td-probe/tqueue_binlog_probe.exe"
DEFAULT_RS = REPO / "rust/target/x86_64-pc-windows-gnu/release/tqueue_binlog_probe.exe"
WORK = REPO / "work" / "tqueue-b"

BINLOG_EVENT_TYPE = 2314
MIN_SIZE = 4 + 8 + 4 + 4 + 8 + 4

LONG = "ab" * 300  # 300 bytes: the TL length form switches to 0xFE + 3 bytes

CASES = [
    (
        "basic",
        [
            "push 1 6161 1000 7 100",
            "push 1 6262 2000 0 -",
            "push 1 6363 3000 0 -",
            "head 1",
            "tail 1",
            "size 1",
            "close",
            "dumpfile",
            "replay 1",
        ],
    ),
    (
        "extra_flip",
        [
            "push 1 6161 1000 7 100",
            "clear 1 0",
            "close",
            "dumpfile",
            "replay 1",
        ],
    ),
    (
        "erase_kinds",
        [
            "push 1 6161 1000 0 100",
            "push 1 6262 2000 0 -",
            "push 1 6363 3000 0 -",
            "forget 1 101",
            "clear 1 0",
            "size 1",
            "close",
            "dumpfile",
            "replay 1",
        ],
    ),
    (
        "get_sweep",
        [
            "push 1 6161 500 0 100",
            "push 1 6262 1000 0 -",
            "get 1 100 0 700 8",
            "size 1",
            "close",
            "dumpfile",
            "replay 1",
        ],
    ),
    (
        "gc_sweep",
        [
            "push 1 6161 1000 0 100",
            "push 1 6262 500 0 -",
            "gc 5000",
            "size 1",
            "close",
            "dumpfile",
            "replay 1",
        ],
    ),
    (
        "queues",
        [
            "push 1 61 1000 0 100",
            "push 2 62 1000 5 500",
            "push 1 63 2000 0 -",
            "push 2 64 3000 0 -",
            "close",
            "dumpfile",
            "replay 1 2",
        ],
    ),
    (
        "rewrite_chain",
        [
            "push 1 61 1000 0 100",
            "push 1 61 2000 0 -",
            "push 1 62 3000 9 -",
            "push 1 61 4000 0 -",
            "close",
            "dumpfile",
            "replay 1",
        ],
    ),
    (
        "empty_tail",
        [
            "push 1 6161 1000 0 100",
            "push 1 6262 2000 0 -",
            "clear 1 0",
            "head 1",
            "size 1",
            "close",
            "dumpfile",
            "replay 1",
        ],
    ),
    (
        "long_data",
        [
            f"push 1 {LONG} 1000 0 100",
            "size 1",
            "close",
            "dumpfile",
            "replay 1",
        ],
    ),
    (
        "isolated_clear",
        [
            "push 1 61 1000 0 100",
            "push 2 62 1000 0 500",
            "clear 1 0",
            "size 1",
            "size 2",
            "close",
            "dumpfile",
            "replay 1 2",
        ],
    ),
]

READ_SCRIPT = [
    "attach",
    "head 1",
    "tail 1",
    "size 1",
    "head 2",
    "tail 2",
    "size 2",
    "close",
]


def frames(data: bytes):
    """Independent plaintext frame walk, so the oracle is not grading itself."""
    rows = []
    offset = 0
    while offset + 4 <= len(data):
        size = struct.unpack_from("<I", data, offset)[0]
        if size < MIN_SIZE or offset + size > len(data):
            break
        rows.append(
            {
                "id": struct.unpack_from("<Q", data, offset + 4)[0],
                "type": struct.unpack_from("<i", data, offset + 12)[0],
                "flags": struct.unpack_from("<i", data, offset + 16)[0],
                "extra": struct.unpack_from("<Q", data, offset + 20)[0],
                # [size:u32][id:u64][type:i32][flags:i32][extra:u64] = 28 bytes
                "data": data[offset + 28 : offset + size - 4],
                "size": size,
            }
        )
        offset += size
    return rows, offset


def tl_string(data: bytes, offset: int):
    """`TlStorer::store_string`: 1-byte length, or 0xFE + 3 LE bytes, padded to 4."""
    if offset >= len(data):
        return None, offset
    first = data[offset]
    if first <= 253:
        length = first
        start = offset + 1
    elif first == 254:
        if offset + 4 > len(data):
            return None, offset
        length = data[offset + 1] | (data[offset + 2] << 8) | (data[offset + 3] << 16)
        start = offset + 4
    else:
        return None, offset
    end = start + length
    if end > len(data):
        return None, offset
    # the padding is computed on (length prefix + payload), then rounded to 4
    total = (start - offset) + length
    padded = end + ((4 - total % 4) % 4)
    return data[start:end], min(padded, len(data))


def tunevent(payload: bytes):
    """Parse a `TQueueLogEvent` body: queue_id, event_id, expires_at, data, extra."""
    if len(payload) < 16:
        return None
    queue_id = struct.unpack_from("<q", payload, 0)[0]
    event_id = struct.unpack_from("<i", payload, 8)[0]
    expires_at = struct.unpack_from("<i", payload, 12)[0]
    data, offset = tl_string(payload, 16)
    if data is None:
        return None
    extra = 0
    if offset + 8 <= len(payload):
        extra = struct.unpack_from("<q", payload, offset)[0]
    return {
        "queue_id": queue_id,
        "event_id": event_id,
        "expires_at": expires_at,
        "data": data,
        "extra": extra,
    }


def narration(lines, command: str):
    return [line for line in lines if line.split(" ", 1)[0] == command]


def compare_lines(name: str, cpp, rs) -> bool:
    if len(cpp) != len(rs):
        print(f"DIFF {name}: line count oracle={len(cpp)} rust={len(rs)}")
        for index in range(min(len(cpp), len(rs))):
            if cpp[index] != rs[index]:
                print(f"  first mismatch at line {index + 1}: oracle={cpp[index]!r} rust={rs[index]!r}")
                break
        return False
    for index, (left, right) in enumerate(zip(cpp, rs)):
        if left != right:
            print(f"DIFF {name} line {index + 1}: oracle={left!r} rust={right!r}")
            return False
    return True


def compare_files(name: str, cpp: bytes, rs: bytes) -> bool:
    if cpp == rs:
        return True
    print(f"DIFF {name} binlog: oracle={len(cpp)} B rust={len(rs)} B")
    for offset in range(min(len(cpp), len(rs))):
        if cpp[offset] != rs[offset]:
            print(f"  first differing byte at {offset}: oracle={cpp[offset]:02x} rust={rs[offset]:02x}")
            break
    return False


def script_text(lines) -> str:
    return "\n".join(lines) + "\n"


def run(exe: Path, script: Path, binlog: Path, env: dict, key: str = "empty"):
    proc = subprocess.run([str(exe), "--key", key, str(script), str(binlog)], capture_output=True, env=env)
    return (
        proc.returncode,
        proc.stdout.decode("utf-8", "replace").splitlines(),
        proc.stderr.decode("utf-8", "replace").strip(),
    )


def cross_read(args, env, name: str, cpp_file: Path, rs_file: Path, key: str = "empty") -> bool:
    read_dir = WORK / "read"
    paths = {
        "cpp_own": read_dir / f"{name}.cpp.cpp.binlog",
        "cpp_rs": read_dir / f"{name}.rs.cpp.binlog",
        "rs_own": read_dir / f"{name}.rs.rs.binlog",
        "rs_cpp": read_dir / f"{name}.cpp.rs.binlog",
    }
    shutil.copyfile(cpp_file, paths["cpp_own"])
    shutil.copyfile(rs_file, paths["cpp_rs"])
    shutil.copyfile(rs_file, paths["rs_own"])
    shutil.copyfile(cpp_file, paths["rs_cpp"])
    script = WORK / "scripts" / "read.txt"

    results = {}
    for label, exe in (
        ("cpp_own", Path(args.cpp)),
        ("cpp_rs", Path(args.cpp)),
        ("rs_own", Path(args.rs)),
        ("rs_cpp", Path(args.rs)),
    ):
        rc, lines, stderr = run(exe, script, paths[label], env, key)
        if rc != 0:
            print(f"DIFF cross={label}/{name} exit={rc} stderr={stderr!r}")
            return False
        results[label] = lines

    for label, left, right in (
        ("cpp-on-rust", "cpp_own", "cpp_rs"),
        ("rust-on-cpp", "rs_own", "rs_cpp"),
        ("cross-narration", "cpp_own", "rs_own"),
    ):
        if not compare_lines(f"{name} {label}", results[left], results[right]):
            return False
    return True


HEADER_FRAME_SIZE = 128


def masked(lines):
    """Mask the file digest: salt and iv are fresh per generation, so the sha256
    of an encrypted file is not a criterion -- its length still is."""
    out = []
    for line in lines:
        if line.startswith("dumpfile size="):
            size = line.split("size=")[1].split(" ")[0]
            out.append(f"dumpfile size={size} sha256=<random>")
        else:
            out.append(line)
    return out


def encrypted_header(data: bytes):
    """Parse the plaintext `type = -3` header; everything behind it is ciphertext,
    so the frame walk stops right after that frame."""
    rows, _ = frames(data)
    if not rows:
        return None
    first = rows[0]
    if first["id"] != 0 or first["type"] != -3:
        return None
    body = first["data"]
    if len(body) < 4:
        return None
    salt, offset = tl_string(body, 4)
    iv, offset = tl_string(body, offset)
    stored, offset = tl_string(body, offset)
    if salt is None or iv is None or stored is None or offset != len(body):
        return None
    return {"size": first["size"], "salt": salt, "iv": iv, "hash": stored}


def header_key_matches(header) -> bool:
    derived = hashlib.pbkdf2_hmac("sha256", b"cucumber", header["salt"], 2, dklen=32)
    want = hmac.new(derived, b"cucumbers everywhere", "sha256").digest()
    return want == header["hash"]


def wrong_key(args, env, name: str, cpp_file: Path, rs_file: Path) -> bool:
    """Both sides must refuse an encrypted file opened with `DbKey::empty()`."""
    read_dir = WORK / "read"
    cpp_wrong = read_dir / f"{name}.cpp.wrong.binlog"
    rs_wrong = read_dir / f"{name}.rs.wrong.binlog"
    shutil.copyfile(cpp_file, cpp_wrong)
    shutil.copyfile(rs_file, rs_wrong)
    script = WORK / "scripts" / "read.txt"
    cpp_rc, cpp_out, _ = run(Path(args.cpp), script, cpp_wrong, env, "empty")
    rs_rc, rs_out, _ = run(Path(args.rs), script, rs_wrong, env, "empty")
    if (cpp_rc, cpp_out) != (rs_rc, rs_out):
        print(f"DIFF {name} wrong-key: oracle=({cpp_rc}, {cpp_out}) rust=({rs_rc}, {rs_out})")
        return False
    if cpp_rc != 3 or not any(line.startswith("init err=Wrong password") for line in cpp_out):
        print(f"DIFF {name} wrong-key: expected exit 3 and 'init err=Wrong password', got {cpp_out}")
        return False
    return True


def selftest(
    cpp_files: dict[str, bytes],
    cpp_lines: dict[str, list[str]],
    enc_files: dict[str, bytes],
) -> bool:
    def want(condition: bool, message: str) -> bool:
        if not condition:
            print(f"SELFTEST FAIL: {message}")
        return condition

    ok = True

    # 1. three pushes: two 52-byte 2314 frames and one 60-byte 2315 (extra = 7).
    #    A frame is 32 bytes of header+crc plus the body, so `binlog_dump`'s
    #    "size:28" is the body length of the 2315 one.
    rows, consumed = frames(cpp_files["basic"])
    ok &= want(consumed == len(cpp_files["basic"]), "the basic file is one whole frame run")
    ok &= want(
        [(row["type"], row["size"], row["flags"]) for row in rows]
        == [(BINLOG_EVENT_TYPE + 1, 60, 0), (BINLOG_EVENT_TYPE, 52, 0), (BINLOG_EVENT_TYPE, 52, 0)],
        "extra != 0 adds 8 bytes and switches the magic to 2315",
    )
    ok &= want(
        [tunevent(row["data"]) for row in rows]
        == [
            {"queue_id": 1, "event_id": 100, "expires_at": 1000, "data": b"aa", "extra": 7},
            {"queue_id": 1, "event_id": 101, "expires_at": 2000, "data": b"bb", "extra": 0},
            {"queue_id": 1, "event_id": 102, "expires_at": 3000, "data": b"cc", "extra": 0},
        ],
        "the body is queue_id, event_id, expires_at, TL-string data, optional extra",
    )

    # 2. clear(0) rewrites the last slot. The magic follows the *current* extra,
    #    and `clear_keep`/`pop` only drop the data -- the extra survives -- so a
    #    2315 event stays 2315 and its body keeps extra = 7.
    rows, _ = frames(cpp_files["extra_flip"])
    ok &= want(len(rows) == 2, "extra_flip writes the push and one rewrite")
    rewrite = rows[1]
    ok &= want(rewrite["flags"] == 1, "the rewrite carries flags = 1")
    ok &= want(
        rewrite["type"] == BINLOG_EVENT_TYPE + 1,
        "clearing the data does not clear the extra, so the rewrite stays 2315",
    )
    ok &= want(rewrite["id"] == 1, "the rewrite targets the erased event's log_event_id")
    body = tunevent(rewrite["data"])
    ok &= want(
        body is not None and body["data"] == b"" and body["extra"] == 7,
        "the rewritten body has empty data but keeps extra = 7",
    )

    # 3. forget -> an erase frame; clear -> erase_batch.
    rows, _ = frames(cpp_files["erase_kinds"])
    ok &= want(
        [(row["id"], row["type"], row["flags"], row["size"]) for row in rows]
        == [
            (1, BINLOG_EVENT_TYPE, 0, 52),
            (2, BINLOG_EVENT_TYPE, 0, 52),
            (3, BINLOG_EVENT_TYPE, 0, 52),
            (2, -2, 1, 32),
            (3, BINLOG_EVENT_TYPE, 1, 52),
            (1, -2, 1, 32),
        ],
        "an erase is a 32-byte -2/1 frame pointing at the erased id",
    )

    # 4. replay reports the processor's live set, not the raw frame count.
    ok &= want(
        narration(cpp_lines["erase_kinds"], "replay") == ["replay q=1 head=102 tail=103 size=0", "replay frames=1"],
        "six frames on disk, one live event to replay",
    )
    ok &= want(
        narration(cpp_lines["basic"], "replay") == ["replay q=1 head=100 tail=103 size=3", "replay frames=3"],
        "basic replays all three pushes",
    )

    # 5. a 300-byte payload switches the TL length to 0xFE + 3 bytes:
    #    32 (frame) + 16 (fixed body) + 4 (length) + 300 = 352.
    rows, _ = frames(cpp_files["long_data"])
    ok &= want(len(rows) == 1 and rows[0]["size"] == 352, "a long payload grows the frame to 352")
    ok &= want(rows[0]["data"][16] == 0xFE, "300 bytes use the 0xFE length form")

    # 6. two queues interleave but keep their own ids.
    rows, _ = frames(cpp_files["queues"])
    ok &= want(
        [(tunevent(row["data"])["queue_id"], tunevent(row["data"])["event_id"], row["type"]) for row in rows]
        == [(1, 100, 2314), (2, 500, 2315), (1, 101, 2314), (2, 501, 2314)],
        "each push keeps its queue id and its own magic",
    )

    # 7. dumpfile sizes agree with the independent walk.
    for name in ("basic", "erase_kinds", "long_data"):
        size_line = narration(cpp_lines[name], "dumpfile")[0]
        size = int(size_line.split("size=")[1].split(" ")[0])
        ok &= want(size == len(cpp_files[name]), f"{name}: dumpfile size matches the file")

    # 8. the encrypted generation: the same frames behind a 128-byte `-3` header,
    #    and the key_hash inside that header is PBKDF2 + HMAC of the raw key.
    for name in ("basic", "erase_kinds", "long_data"):
        header = encrypted_header(enc_files[name])
        ok &= want(header is not None, f"{name}: the encrypted file starts with an id=0 type=-3 frame")
        if header is None:
            continue
        ok &= want(header["size"] == HEADER_FRAME_SIZE, f"{name}: the header frame is 128 bytes")
        ok &= want(
            len(enc_files[name]) == len(cpp_files[name]) + HEADER_FRAME_SIZE,
            f"{name}: encrypted size is the plaintext size + 128",
        )
        ok &= want(len(header["salt"]) == 32 and len(header["iv"]) == 16, f"{name}: salt is 32 B, iv is 16 B")
        ok &= want(header_key_matches(header), f"{name}: key_hash is PBKDF2 + HMAC of the raw key")
    return ok


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

    text = script_text([f"# {name}" for name, _ in CASES] + sum((body for _, body in CASES), []))
    digest = hashlib.md5(text.encode()).hexdigest()
    print(f"corpus md5={digest} lines={len(text.splitlines())}")

    for sub in ("scripts", "cpp", "rs", "read"):
        shutil.rmtree(WORK / sub, ignore_errors=True)
        (WORK / sub).mkdir(parents=True, exist_ok=True)
    (WORK / "scripts" / "read.txt").write_text(script_text(READ_SCRIPT))

    env = dict(os.environ)
    env["PATH"] = MSYS_BIN + ";" + env.get("PATH", "")

    started = time.time()
    total_bytes = 0
    cpp_files: dict[str, bytes] = {}
    cpp_lines: dict[str, list[str]] = {}
    enc_files: dict[str, bytes] = {}

    for name, body in CASES:
        script = WORK / "scripts" / f"{name}.txt"
        script.write_text(script_text(body))
        cpp_file = WORK / "cpp" / f"{name}.binlog"
        rs_file = WORK / "rs" / f"{name}.binlog"
        cpp_rc, cpp_lines[name], cpp_err = run(Path(args.cpp), script, cpp_file, env)
        rs_rc, rs_out, rs_err = run(Path(args.rs), script, rs_file, env)
        if cpp_rc != rs_rc:
            print(f"DIFF {name} exit code: oracle={cpp_rc} rust={rs_rc} stderr={cpp_err!r} {rs_err!r}")
            return 1
        if cpp_rc != 0:
            print(f"FAIL {name}: both sides exited {cpp_rc}: {cpp_err!r} {rs_err!r}")
            return 1

        cpp_bytes = cpp_file.read_bytes()
        rs_bytes = rs_file.read_bytes()
        cpp_files[name] = cpp_bytes
        total_bytes += len(cpp_bytes) + len(rs_bytes)

        if not compare_files(name, cpp_bytes, rs_bytes):
            return 1
        if not compare_lines(f"{name} narration", cpp_lines[name], rs_out):
            return 1

        floor_rc, floor_out, _ = run(Path(args.cpp), script, cpp_file, env)
        if floor_rc != cpp_rc or floor_out != cpp_lines[name] or cpp_file.read_bytes() != cpp_bytes:
            print(f"FLOOR FAIL: the oracle is not deterministic on {name}")
            return 1

        if not cross_read(args, env, name, cpp_file, rs_file):
            return 1

        cpp_enc_file = WORK / "cpp" / f"{name}.enc.binlog"
        rs_enc_file = WORK / "rs" / f"{name}.enc.binlog"
        cpp_enc_rc, cpp_enc_out, cpp_enc_err = run(Path(args.cpp), script, cpp_enc_file, env, "raw")
        rs_enc_rc, rs_enc_out, rs_enc_err = run(Path(args.rs), script, rs_enc_file, env, "raw")
        if cpp_enc_rc != rs_enc_rc or cpp_enc_rc != 0:
            print(
                f"DIFF {name} encrypted exit: oracle={cpp_enc_rc} rust={rs_enc_rc} "
                f"{cpp_enc_err!r} {rs_enc_err!r}"
            )
            return 1
        cpp_enc = cpp_enc_file.read_bytes()
        rs_enc = rs_enc_file.read_bytes()
        enc_files[name] = cpp_enc
        total_bytes += len(cpp_enc) + len(rs_enc)
        if len(cpp_enc) != len(rs_enc):
            print(f"DIFF {name} encrypted size: oracle={len(cpp_enc)} rust={len(rs_enc)}")
            return 1
        if len(cpp_enc) != len(cpp_bytes) + HEADER_FRAME_SIZE:
            print(
                f"DIFF {name} encrypted size: {len(cpp_enc)} != plaintext {len(cpp_bytes)} "
                f"+ {HEADER_FRAME_SIZE}"
            )
            return 1
        if not compare_lines(f"{name} encrypted narration", masked(cpp_enc_out), masked(rs_enc_out)):
            return 1
        if not cross_read(args, env, f"{name}.enc", cpp_enc_file, rs_enc_file, "raw"):
            return 1
        if not wrong_key(args, env, name, cpp_enc_file, rs_enc_file):
            return 1
        floor_rc, floor_out, _ = run(Path(args.cpp), script, cpp_enc_file, env, "raw")
        if (
            floor_rc != cpp_enc_rc
            or masked(floor_out) != masked(cpp_enc_out)
            or len(cpp_enc_file.read_bytes()) != len(cpp_enc)
        ):
            print(f"FLOOR FAIL: the oracle is not deterministic on {name} (encrypted)")
            return 1

    if args.selftest and not selftest(cpp_files, cpp_lines, enc_files):
        return 1

    elapsed = time.time() - started
    print(f"PASS cases={len(CASES)} bytes={total_bytes} elapsed={elapsed:.2f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main())