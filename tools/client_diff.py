"""CLIENT lane: first-run `td_receive` sequence, C++ oracle vs the Rust rewrite.

命令:
    uv run python tools/client_diff.py
    uv run python tools/client_diff.py --capture <dll> <dbdir> <out.jsonl>   # worker（仪器自用）

判据:
    退出码 0 = ① A/A 地板：oracle 两次独立进程、规范化后逐行相同；
               ② MATCH：两侧逐行相同（白名单只有 unix_time / utc_time_offset / commit_hash）；
               ③ second run：同一 database_directory 的第二次运行按**集合**相等
                  （125 个 option + 2 状态；顺序不参与，F078）；
               ④ 变异自检：篡改一条 option 后比较器必须报差异；
               ⑤ 白名单值自身可判：unix_time 与采集时刻相差 ≤5s、两条 commit_hash 各自可解释。

覆盖边界:
    只覆盖「一个 client、先 `td_send(setTdlibParameters)` 再 drain 到
    `authorizationStateWaitPhoneNumber`」这一条驱动，分首次运行（逐行）与同一目录的第二次
    运行（按集合）。第二个 client（F077）、登录状态机、其它 `setTdlibParameters` 参数 lane 不在内。

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

AUTH_REQUESTS = [
    {"@type": "setAuthenticationPhoneNumber", "phone_number": ""},
    {"@type": "checkAuthenticationCode", "code": "12345"},
    {"@type": "checkAuthenticationPassword", "password": "x"},
    {"@type": "getMe"},
    {"@type": "logOut"},
]

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


def worker(dll: str, db: str, out: str, fresh: bool, auth: bool, legacy: bool) -> int:
    add_runtime_dirs()
    lib = ctypes.CDLL(dll)
    lib.td_execute.restype = ctypes.c_char_p
    lib.td_execute.argtypes = [ctypes.c_char_p]
    # 先静音再建 client：否则 oracle 的 "Create client" 日志会混进 stderr
    lib.td_execute(json.dumps({"@type": "setLogVerbosityLevel", "new_verbosity_level": 0}).encode())

    if legacy:
        # 遗留入口（td_json_client_*）：句柄是 ClientJson*，回包不带 @client_id
        lib.td_json_client_create.restype = ctypes.c_void_p
        lib.td_json_client_send.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        lib.td_json_client_receive.restype = ctypes.c_char_p
        lib.td_json_client_receive.argtypes = [ctypes.c_void_p, ctypes.c_double]
        lib.td_json_client_destroy.argtypes = [ctypes.c_void_p]
        handle = lib.td_json_client_create()
        if not handle:
            print(json.dumps({"error": "td_json_client_create returned NULL"}))
            return 1

        def send(request: bytes) -> None:
            lib.td_json_client_send(handle, request)

        def receive(timeout: float):
            return lib.td_json_client_receive(handle, timeout)

        client_id = 0
    else:
        lib.td_create_client_id.restype = ctypes.c_int
        lib.td_send.argtypes = [ctypes.c_int, ctypes.c_char_p]
        lib.td_receive.restype = ctypes.c_char_p
        lib.td_receive.argtypes = [ctypes.c_double]
        client_id = lib.td_create_client_id()

        def send(request: bytes) -> None:
            lib.td_send(client_id, request)

        def receive(timeout: float):
            return lib.td_receive(timeout)

    database = Path(db)
    if fresh and database.exists():
        shutil.rmtree(database)
    database.mkdir(parents=True, exist_ok=True)

    started = int(time.time())
    params = dict(PARAMS, database_directory=str(database), files_directory=str(database / "files"))
    send(json.dumps({"@type": "setTdlibParameters", **params}).encode())

    rows: list[str] = []
    deadline = time.time() + 30
    while time.time() < deadline:
        raw = receive(0.3)
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
    if auth:
        for request in AUTH_REQUESTS:
            send(json.dumps(request).encode())
            end = time.time() + 0.8
            while time.time() < end:
                raw = receive(0.2)
                if raw:
                    rows.append(raw.decode("utf-8", "replace"))
    if legacy:
        # ClientJson::send 把解析不了的请求变成 testReturnError，所以得到的是一条 error 回包
        send(b"not json")
        end = time.time() + 2
        while time.time() < end:
            raw = receive(0.2)
            if raw:
                rows.append(raw.decode("utf-8", "replace"))
        lib.td_json_client_destroy(handle)

    Path(out).write_text("\n".join(rows), encoding="utf-8")
    print(json.dumps({"client_id": client_id, "lines": len(rows), "started": started}))
    return 0


def normalize(row: str) -> str:
    for pattern, replacement in DYNAMIC:
        row = pattern.sub(replacement, row)
    return row


def strip_client_id(row: str) -> str:
    # 现代表在最后追加 ,"@client_id":N；遗留表不追加（ClientJson::receive 传 client_id 0）
    return re.sub(r',"@client_id":\d+\}$', "}", row)


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


def compare_sets(left: list[str], right: list[str]) -> str | None:
    if len(left) != len(right):
        return f"line count {len(left)} != {len(right)}"
    a = sorted(normalize(row) for row in left)
    b = sorted(normalize(row) for row in right)
    if a == b:
        return None
    missing = [row for row in a if row not in b]
    extra = [row for row in b if row not in a]
    detail = f"{len(missing)} only in cpp, {len(extra)} only in rs"
    if missing:
        detail += f"\n    only cpp: {missing[0][:220]}"
    if extra:
        detail += f"\n    only rs : {extra[0][:220]}"
    return detail


def option_count(rows: list[str]) -> int:
    return sum(1 for row in rows if json.loads(row).get("@type") == "updateOption")


def mutate(rows: list[str]) -> list[str]:
    changed = list(rows)
    for index, row in enumerate(changed):
        if '"name":"version"' in row:
            changed[index] = row.replace('"1.8.71"', '"1.8.72"')
            return changed
    raise SystemExit("FAIL self-test: no version option to mutate")


def capture(
    dll: Path, tag: str, database: str, fresh: bool = True, auth: bool = False, legacy: bool = False
) -> tuple[list[str], dict]:
    base = ROOT / "work" / "client" / tag
    database_path = ROOT / "work" / "client" / database / "db"
    out = base / "rows.jsonl"
    base.mkdir(parents=True, exist_ok=True)
    command = [PYTHON, str(Path(__file__).resolve()), "--capture", str(dll), str(database_path), str(out)]
    if not fresh:
        command.append("--reuse")
    if auth:
        command.append("--auth")
    if legacy:
        command.append("--legacy")
    process = subprocess.run(
        command,
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
        return worker(argv[1], argv[2], argv[3], "--reuse" not in argv, "--auth" in argv, "--legacy" in argv)

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

    cpp_a, meta_a = capture(CPP, "cpp-a", "cpp", fresh=True)
    cpp_b, _ = capture(CPP, "cpp-b", "cpp-b", fresh=True)
    rs, meta_rs = capture(RS, "rs", "rs", fresh=True)

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

    print("\n== second run: 同一 database_directory，按集合比较（顺序不参与，F078） ==")
    cpp_2, _ = capture(CPP, "cpp-2", "cpp", fresh=False)
    cpp_3, _ = capture(CPP, "cpp-3", "cpp", fresh=False)
    rs_2, _ = capture(RS, "rs-2", "rs", fresh=False)

    shapes = [
        ("cpp 第二次运行是 127 行 / 125 个 option", len(cpp_2) == 127 and option_count(cpp_2) == 125, f"cpp {len(cpp_2)}/{option_count(cpp_2)}"),
        ("rust 第二次运行同形状", len(rs_2) == 127 and option_count(rs_2) == 125, f"rs {len(rs_2)}/{option_count(rs_2)}"),
    ]
    for name, ok, detail in shapes:
        if not ok:
            failures.append(f"second-run shape: {name} ({detail})")
        print(f"{'PASS' if ok else 'FAIL'} {name}" + ("" if ok else f": {detail}"))

    diff = compare_sets(cpp_2, cpp_3)
    if diff:
        failures.append("second-run floor unstable: " + diff.split("\n")[0])
    print(f"{'PASS' if diff is None else 'FAIL'} cpp-2 vs cpp-3 (set)" + ("" if diff is None else f"\n{diff}"))

    diff = compare_sets(cpp_2, rs_2)
    if diff:
        failures.append("second-run lane differs: " + diff.split("\n")[0])
    print(f"{'PASS' if diff is None else 'FAIL'} cpp vs rs (second run, set)" + ("" if diff is None else f"\n{diff}"))

    diff = compare_sets(cpp_2, mutate(cpp_2))
    ok = diff is not None
    if not ok:
        failures.append("self-test failed: second-run mutation not detected")
    print(f"{'PASS' if ok else 'FAIL'} second-run mutation detected" + ("" if ok else ": 集合比较器对改过的 option 仍报相同"))

    print("\n== auth: 未授权状态机的响应（逐行，142 行 = 129 首启 + 13 响应） ==")
    cpp_auth, _ = capture(CPP, "cpp-auth", "cpp-auth", fresh=True, auth=True)
    cpp_auth2, _ = capture(CPP, "cpp-auth2", "cpp-auth2", fresh=True, auth=True)
    rs_auth, _ = capture(RS, "rs-auth", "rs-auth", fresh=True, auth=True)

    ok = len(cpp_auth) == 142 and len(rs_auth) == 142
    if not ok:
        failures.append(f"auth shape: cpp={len(cpp_auth)} rs={len(rs_auth)}")
    print(f"{'PASS' if ok else 'FAIL'} 两侧都是 142 行" + ("" if ok else f": cpp={len(cpp_auth)} rs={len(rs_auth)}"))

    diff = compare(cpp_auth, cpp_auth2)
    if diff:
        failures.append("auth floor unstable: " + diff.split("\n")[0])
    print(f"{'PASS' if diff is None else 'FAIL'} cpp-auth vs cpp-auth2" + ("" if diff is None else f"\n{diff}"))

    diff = compare(cpp_auth, rs_auth)
    if diff:
        failures.append("auth lane differs: " + diff.split("\n")[0])
    print(f"{'PASS' if diff is None else 'FAIL'} cpp vs rs (auth, 142 lines)" + ("" if diff is None else f"\n{diff}"))

    diff = compare(cpp_auth, mutate(cpp_auth))
    ok = diff is not None
    if not ok:
        failures.append("self-test failed: auth mutation not detected")
    print(f"{'PASS' if ok else 'FAIL'} auth mutation detected" + ("" if ok else ": 比较器对一条改过的 response 仍报相同"))

    print("\n== legacy: td_json_client_* 的同一条 auth lane（回包不带 @client_id） ==")
    cpp_legacy, _ = capture(CPP, "cpp-legacy", "cpp-legacy", fresh=True, auth=True, legacy=True)
    rs_legacy, _ = capture(RS, "rs-legacy", "rs-legacy", fresh=True, auth=True, legacy=True)

    has_id_cpp = sum(1 for row in cpp_legacy if "@client_id" in row)
    has_id_rs = sum(1 for row in rs_legacy if "@client_id" in row)
    has_id_modern = sum(1 for row in cpp_auth if "@client_id" in row)
    # 遗留回包一律不带 @client_id（ClientJson::receive 传 client_id 0），现代表每行都带
    ok = has_id_cpp == 0 and has_id_rs == 0 and has_id_modern == len(cpp_auth)
    if not ok:
        failures.append(f"legacy @client_id shape: cpp={has_id_cpp} rs={has_id_rs} modern={has_id_modern}")
    print(
        f"{'PASS' if ok else 'FAIL'} @client_id 只出现在现代表"
        + ("" if ok else f": cpp={has_id_cpp}/{len(cpp_legacy)} rs={has_id_rs}/{len(rs_legacy)} modern={has_id_modern}/{len(cpp_auth)}")
    )

    # oracle 内部一致性：遗留 lane 去掉末尾那条畸形请求回包，就是现代表去掉 @client_id
    modern_stripped = [strip_client_id(normalize(row)) for row in cpp_auth]
    legacy_normalized = [normalize(row) for row in cpp_legacy[:-1]]
    ok = legacy_normalized == modern_stripped
    if not ok:
        mismatch = next((i for i, (a, b) in enumerate(zip(legacy_normalized, modern_stripped)) if a != b), -1)
        failures.append(f"legacy != modern minus @client_id at line {mismatch}")
    print(
        f"{'PASS' if ok else 'FAIL'} cpp 遗留 lane = 现代 lane 去掉 @client_id"
        + ("" if ok else f": 首个差异在第 {mismatch} 行")
    )

    diff = compare(cpp_legacy, rs_legacy)
    if diff:
        failures.append("legacy lane differs: " + diff.split("\n")[0])
    print(f"{'PASS' if diff is None else 'FAIL'} cpp vs rs (legacy auth)" + ("" if diff is None else f"\n{diff}"))

    malformed = [row for row in cpp_legacy if '"code":400' in row and "Failed to parse request as JSON object" in row]
    ok = len(malformed) == 1 and len(rs_legacy) == len(cpp_legacy)
    if not ok:
        failures.append(f"legacy malformed send: cpp {len(malformed)} error rows, lens {len(cpp_legacy)}/{len(rs_legacy)}")
    print(
        f"{'PASS' if ok else 'FAIL'} 畸形请求回一条 400"
        + ("" if ok else f": cpp={len(malformed)} rows, lines {len(cpp_legacy)}/{len(rs_legacy)}")
    )

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