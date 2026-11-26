"""CLIENT lane: first-run `td_receive` sequence, C++ oracle vs the Rust rewrite.

命令:
    uv run python tools/client_diff.py
    uv run python tools/client_diff.py --capture <dll> <dbdir> <out.jsonl>   # worker（仪器自用）

判据:
    退出码 0 = ① A/A 地板：oracle 两次独立进程、规范化后逐行相同；
               ② MATCH：两侧逐行相同（白名单只有 unix_time / utc_time_offset / commit_hash）；
               ③ 变异自检：篡改一条 option 后比较器必须报差异；
               ④ 白名单值自身可判：unix_time 与采集时刻相差 ≤5s、两条 commit_hash 各自可解释。

覆盖边界:
    只覆盖「一个全新 database_directory、一个 client、先 `td_send(setTdlibParameters)` 再 drain
    到 `authorizationStateWaitPhoneNumber`」这一条驱动。第二个 client（F077）、已下发值 kv、
    登录状态机、其它 `setTdlibParameters` 参数 lane 不在内。

失效条件:
    oracle md5 变、TDLib 版本变（option 表变）、驱动参数变——任一条都要重判 gate。
"""

from __future__ import annotations

import ctypes
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CPP = Path("C:/tmp/td-build/libtdjson.dll")
RS = ROOT / "rust" / "target" / "x86_64-pc-windows-gnu" / "release" / "tdjson_rs.dll"
EXPECTED_CPP_MD5 = "0398814038978aaeef7f8d6ac62bb8ff"
ORACLE_COMMIT = "1b9efe84d6e9a2e2f6eadeeea32b0726d2b8c25d"
PYTHON = sys.executable
UCrt = "C:/msys64/ucrt64/bin"

EXPECTED_LINES = 129

PARAMS = {
    "use_test_dc": True,
    "database_encryption_key": "",
    "api_id": 94575,
    "api_hash": "a3406de8d171bb422bb6ddf3bbd800e2",
    "system_language_code": "en",
    "device_model": "probe",
    "system_version": "1",
    "application_version": "1",
}

DYNAMIC = [
    (
        re.compile(
            r'("name":"unix_time","value":\{"@type":"optionValueInteger","value":")\d+(")'
        ),
        r"\1<UNIX_TIME>\2",
    ),
    (
        re.compile(
            r'("name":"utc_time_offset","value":\{"@type":"optionValueInteger","value":")\d+(")'
        ),
        r"\1<UTC_OFFSET>\2",
    ),
    (
        re.compile(
            r'("name":"commit_hash","value":\{"@type":"optionValueString","value":")[0-9a-f]*(")'
        ),
        r"\1<COMMIT_HASH>\2",
    ),
]


def md5(path: Path) -> str:
    digest = hashlib.md5()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def add_runtime_dirs() -> None:
    for candidate in (CPP.parent, Path(ROOT / "rust" / "target" / "x86_64-pc-windows-gnu" / "release"), Path(UCrt)):
        if candidate.is_dir():
            os.add_dll_directory(str(candidate))


def worker(dll: str, db: str, out: str) -> int:
    add_runtime_dirs()
    lib = ctypes.CDLL(dll)
    lib.td_create_client_id.restype = ctypes.c_int
    lib.td_send.argtypes = [ctypes.c_int, ctypes.c_char_p]
    lib.td_receive.restype = ctypes.c_char_p
    lib.td_receive.argtypes = [ctypes.c_double]
    lib.td_execute.restype = ctypes.c_char_p
    lib.td_execute.argtypes = [ctypes.c_char_p]

    database = Path(db)
    if database.exists():
        shutil.rmtree(database)
    database.mkdir(parents=True)

    lib.td_execute(json.dumps({"@type": "setLogVerbosityLevel", "new_verbosity_level": 0}).encode())
    client_id = lib.td_create_client_id()
    started = int(time.time())
    params = dict(PARAMS, database_directory=str(database), files_directory=str(database / "files"))
    lib.td_send(client_id, json.dumps({"@type": "setTdlibParameters", **params}).encode())

    rows: list[str] = []
    deadline = time.time() + 30
    while time.time() < deadline:
        raw = lib.td_receive(0.3)
        if not raw:
            continue
        text = raw.decode("utf-8", "replace")
        rows.append(text)
        message = json.loads(text)
        if (
            message.get("@type") == "updateAuthorizationState"
            and message["authorization_state"]["@type"] == "authorizationStateWaitPhoneNumber"
        ):
            break

    Path(out).write_text("\n".join(rows), encoding="utf-8")
    print(json.dumps({"client_id": client_id, "lines": len(rows), "started": started}))
    return 0


def normalize(row: str) -> str:
    for pattern, replacement in DYNAMIC:
        row = pattern.sub(replacement, row)
    return row


def compare(left: list[str], right: list[str]) -> str | None:
    if len(left) != len(right):
        return f"line count {len(left)} != {len(right)}"
    for index, (a, b) in enumerate(zip(left, right)):
        if normalize(a) != normalize(b):
            return f"line {index} differs\n    cpp: {a[:220]}\n    rs : {b[:220]}"
    return None


def option_value(rows: list[str], name: str) -> str | None:
    for row in rows:
        message = json.loads(row)
        if message.get("@type") == "updateOption" and message.get("name") == name:
            return message["value"]["value"]
    return None


def mutate(rows: list[str]) -> list[str]:
    changed = list(rows)
    for index, row in enumerate(changed):
        if '"name":"version"' in row:
            changed[index] = row.replace('"1.8.71"', '"1.8.72"')
            return changed
    raise SystemExit("FAIL self-test: no version option to mutate")


def capture(dll: Path, tag: str) -> tuple[list[str], dict]:
    base = ROOT / "work" / "client" / tag
    database = base / "db"
    out = base / "rows.jsonl"
    base.mkdir(parents=True, exist_ok=True)
    process = subprocess.run(
        [PYTHON, str(Path(__file__).resolve()), "--capture", str(dll), str(database), str(out)],
        capture_output=True,
    )
    text = ((process.stdout or b"") + (process.stderr or b"")).decode("utf-8", "replace")
    if process.returncode != 0 or not out.is_file():
        raise SystemExit(f"FAIL capture {tag}: rc={process.returncode}\n{text[-2000:]}")
    meta = json.loads(text.strip().splitlines()[-1])
    rows = out.read_text(encoding="utf-8").split("\n")
    return rows, meta


def main(argv: list[str]) -> int:
    if argv[:1] == ["--capture"]:
        return worker(argv[1], argv[2], argv[3])

    failures: list[str] = []
    if not CPP.is_file():
        print(f"FAIL oracle missing: {CPP}")
        return 1
    if not RS.is_file():
        print(f"FAIL rust dll missing: {RS}")
        return 1
    actual = md5(CPP)
    if actual != EXPECTED_CPP_MD5:
        print(f"FAIL oracle md5 changed: {actual} != {EXPECTED_CPP_MD5}")
        return 1
    print(f"oracle md5={actual}")

    cpp_a, meta_a = capture(CPP, "cpp-a")
    cpp_b, _ = capture(CPP, "cpp-b")
    rs, meta_rs = capture(RS, "rs")

    print("\n== self-test: 管道不能空转 ==")
    checks = [
        ("行数是 129", len(cpp_a) == EXPECTED_LINES and len(rs) == EXPECTED_LINES, f"cpp={len(cpp_a)} rs={len(rs)}"),
        ("两侧 client id 都是 1", meta_a["client_id"] == 1 and meta_rs["client_id"] == 1, f"cpp={meta_a['client_id']} rs={meta_rs['client_id']}"),
        ("每行都是 JSON 对象", all(json.loads(row).get("@type") for row in cpp_a + rs), "有空行或非 JSON"),
    ]
    for name, ok, detail in checks:
        if not ok:
            failures.append(f"self-test failed: {name} ({detail})")
        print(f"{'PASS' if ok else 'FAIL'} {name}" + ("" if ok else f": {detail}"))

    print("\n== floor: oracle 两次独立进程规范化后逐行相同 ==")
    diff = compare(cpp_a, cpp_b)
    if diff:
        failures.append("floor unstable: " + diff.split("\n")[0])
    print(f"{'PASS' if diff is None else 'FAIL'} cpp-a vs cpp-b" + ("" if diff is None else f"\n{diff}"))

    print("\n== MATCH: 两侧规范化后逐行相同 ==")
    diff = compare(cpp_a, rs)
    if diff:
        failures.append("client lane differs: " + diff.split("\n")[0])
    print(f"{'PASS' if diff is None else 'FAIL'} cpp vs rs (129 lines)" + ("" if diff is None else f"\n{diff}"))

    print("\n== self-test: 变异必须被检出 ==")
    diff = compare(cpp_a, mutate(cpp_a))
    ok = diff is not None
    if not ok:
        failures.append("self-test failed: mutation not detected")
    print(f"{'PASS' if ok else 'FAIL'} mutation detected" + ("" if ok else ": 比较器对一条改过的 option 仍然报相同"))

    print("\n== 白名单值的语义校验 ==")
    cpp_unix = option_value(cpp_a, "unix_time")
    rs_unix = option_value(rs, "unix_time")
    cpp_offset = option_value(cpp_a, "utc_time_offset")
    rs_offset = option_value(rs, "utc_time_offset")
    cpp_commit = option_value(cpp_a, "commit_hash")
    rs_commit = option_value(rs, "commit_hash")

    semantic = [
        ("unix_time 与采集时刻相差 ≤5s", cpp_unix is not None and abs(int(cpp_unix) - meta_a["started"]) <= 5, f"cpp={cpp_unix} started={meta_a['started']}"),
        ("rust unix_time 与采集时刻相差 ≤5s", rs_unix is not None and abs(int(rs_unix) - meta_rs["started"]) <= 5, f"rs={rs_unix} started={meta_rs['started']}"),
        ("utc_time_offset 两侧相同", cpp_offset is not None and cpp_offset == rs_offset, f"cpp={cpp_offset} rs={rs_offset}"),
        ("oracle commit_hash 是构建 HEAD", cpp_commit == ORACLE_COMMIT, f"{cpp_commit}"),
        ("rust commit_hash 是 40 位 hex", isinstance(rs_commit, str) and len(rs_commit) == 40 and all(c in "0123456789abcdef" for c in rs_commit), f"{rs_commit}"),
    ]
    for name, ok, detail in semantic:
        if not ok:
            failures.append(f"semantic failed: {name} ({detail})")
        print(f"{'PASS' if ok else 'FAIL'} {name}" + ("" if ok else f": {detail}"))

    print(f"\nfailures={len(failures)}")
    for failure in failures:
        print(f"  - {failure}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))