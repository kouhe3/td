"""Generate rust/src/client_options.rs from the C++ oracle's client runs.

命令:
    uv run python tools/gen_client_options.py            # 重新生成
    uv run python tools/gen_client_options.py --check    # 只校验（逐字节比较）

判据:
    退出码 0 = 生成内容与 rust/src/client_options.rs 逐字节相同；非 0 = 漂移或 oracle 不可用。

覆盖边界:
    两张表都来自 `use_test_dc:true` + 固定 `setTdlibParameters` + 全新 `database_directory`：
    ① STARTUP_TEMPLATE 是首次运行（129 条，路径相关值运行时填）；
    ② SECOND_RUN_TEMPLATE 是同一目录的第二次运行（125 个 option，按名字排序——oracle 的
       第二次运行顺序不稳定（F078），所以这张表只保证集合）。
    每个 client 都必须在新进程里跑：同进程的第二个 client 会多出 6 条全局 update（F077）。

失效条件:
    oracle md5 变、TDLib 版本变（option 表随版本变）、驱动参数变——任一条都要重跑并重判 gate。
"""

from __future__ import annotations

import argparse
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
ORACLE = Path("C:/tmp/td-build/libtdjson.dll")
EXPECTED_MD5 = "0398814038978aaeef7f8d6ac62bb8ff"
OUT = ROOT / "rust" / "src" / "client_options.rs"
DB = ROOT / "work" / "gen-client-db"

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

DYNAMIC = {
    "unix_time": "{UNIX_TIME}",
    "utc_time_offset": "{UTC_OFFSET}",
    "commit_hash": "{COMMIT_HASH}",
}


def md5(path: Path) -> str:
    digest = hashlib.md5()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def capture_once(db: Path, out: Path, fresh: bool) -> None:
    """Worker mode: one client in this process, exactly like the judged lane."""
    if not ORACLE.is_file():
        raise SystemExit(f"FAIL oracle missing: {ORACLE}")
    actual = md5(ORACLE)
    if actual != EXPECTED_MD5:
        raise SystemExit(f"FAIL oracle md5 changed: {actual} != {EXPECTED_MD5}")

    os.add_dll_directory(r"C:\msys64\ucrt64\bin")
    lib = ctypes.CDLL(str(ORACLE))
    lib.td_create_client_id.restype = ctypes.c_int
    lib.td_send.argtypes = [ctypes.c_int, ctypes.c_char_p]
    lib.td_receive.restype = ctypes.c_char_p
    lib.td_receive.argtypes = [ctypes.c_double]
    lib.td_execute.restype = ctypes.c_char_p
    lib.td_execute.argtypes = [ctypes.c_char_p]

    if fresh and db.exists():
        shutil.rmtree(db)
    db.mkdir(parents=True, exist_ok=True)

    lib.td_execute(json.dumps({"@type": "setLogVerbosityLevel", "new_verbosity_level": 0}).encode())
    client_id = lib.td_create_client_id()
    params = dict(PARAMS, database_directory=str(db), files_directory=str(db / "files"))
    lib.td_send(client_id, json.dumps({"@type": "setTdlibParameters", **params}).encode())

    rows: list[str] = []
    deadline = time.time() + 30
    while time.time() < deadline:
        raw = lib.td_receive(0.3)
        if not raw:
            continue
        text = raw.decode()
        rows.append(text)
        message = json.loads(text)
        if (
            message.get("@type") == "updateAuthorizationState"
            and message["authorization_state"]["@type"] == "authorizationStateWaitPhoneNumber"
        ):
            break
    out.write_text("\n".join(rows), encoding="utf-8")
    print(f"capture {out}: {len(rows)} lines")


def capture(fresh: bool) -> list[str]:
    """Run one client in a child process so the lane matches what is judged (F077)."""
    out = DB.parent / ("gen-client-first.jsonl" if fresh else "gen-client-second.jsonl")
    command = [sys.executable, str(Path(__file__).resolve()), "--capture", str(DB), str(out)]
    if fresh:
        command.append("--fresh")
    process = subprocess.run(command, capture_output=True)
    if process.returncode != 0:
        text = ((process.stdout or b"") + (process.stderr or b"")).decode("utf-8", "replace")
        raise SystemExit(f"FAIL capture: rc={process.returncode}\n{text[-2000:]}")
    return out.read_text(encoding="utf-8").split("\n")


def template(row: str) -> str:
    message = json.loads(row)
    name = message.get("name")
    if message.get("@type") == "updateOption" and name in DYNAMIC:
        placeholder = DYNAMIC[name]
        pattern = re.compile(
            r'("name":"' + re.escape(name) + r'","value":\{"@type":"optionValue\w+","value":")[^"]*(")'
        )
        row, count = pattern.subn(lambda m: m.group(1) + placeholder + m.group(2), row)
        if count != 1:
            raise SystemExit(f"FAIL could not template option {name}: {count} replacements")
    row, count = re.subn(r'"@client_id":\d+', '"@client_id":{CLIENT_ID}', row)
    if count != 1:
        raise SystemExit(f"FAIL missing @client_id in: {row[:120]}")
    return row


def rust_raw(text: str) -> str:
    hashes = 1
    while f'"{"#" * hashes}' in text:
        hashes += 1
    fence = "#" * hashes
    return f'r{fence}"{text}"{fence}'


def order_second(rows: list[str]) -> list[str]:
    """The oracle's second-run order is unstable (F078), so pin it by option name."""
    head: list[str] = []
    options: list[str] = []
    tail: list[str] = []
    for row in rows:
        message = json.loads(row)
        if message.get("@type") == "updateOption":
            options.append(row)
        elif message["authorization_state"]["@type"] == "authorizationStateWaitTdlibParameters":
            head.append(row)
        else:
            tail.append(row)
    names = [json.loads(row)["name"] for row in options]
    if len(names) != len(set(names)):
        raise SystemExit(f"FAIL duplicated option names in the second run: {len(names)} vs {len(set(names))}")
    options.sort(key=lambda row: json.loads(row)["name"])
    return head + options + tail


def render(first: list[str], second: list[str]) -> str:
    first_options = sum(1 for row in first if json.loads(row).get("@type") == "updateOption")
    second_options = sum(1 for row in second if json.loads(row).get("@type") == "updateOption")
    if len(first) != 129 or first_options != 127:
        raise SystemExit(f"FAIL unexpected first-run shape: {len(first)} lines, {first_options} options")
    if len(second) != 127 or second_options != 125:
        raise SystemExit(f"FAIL unexpected second-run shape: {len(second)} lines, {second_options} options")

    ordered_second = order_second(second)
    lines = [
        "// @generated by tools/gen_client_options.py from the C++ oracle; do not edit.",
        "//",
        f"// Source: {ORACLE.as_posix()} (md5 {EXPECTED_MD5}), one client per process",
        "// with a fresh database_directory and fixed setTdlibParameters.",
        "// Placeholders filled at runtime: {CLIENT_ID}, {UNIX_TIME}, {UTC_OFFSET}, {COMMIT_HASH}.",
        "//",
        f"// First run: {len(first)} messages ({first_options} updateOption, {len(first) - first_options} state).",
        f"pub const STARTUP_TEMPLATE: [&str; {len(first)}] = [",
    ]
    lines += [f"    {rust_raw(template(row))}," for row in first]
    lines += [
        "];",
        "",
        "// Second run on the same database_directory: 125 options, pinned by name because the",
        "// oracle's order is unstable there (F078). Judged as a set, not line by line.",
        f"pub const SECOND_RUN_TEMPLATE: [&str; {len(ordered_second)}] = [",
    ]
    lines += [f"    {rust_raw(template(row))}," for row in ordered_second]
    lines += ["];", ""]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--capture", nargs=2, metavar=("DB", "OUT"))
    parser.add_argument("--fresh", action="store_true")
    args = parser.parse_args()

    if args.capture:
        capture_once(Path(args.capture[0]), Path(args.capture[1]), args.fresh)
        return 0

    first = capture(fresh=True)
    second = capture(fresh=False)
    content = render(first, second)

    if args.check:
        if not OUT.is_file():
            print(f"FAIL missing {OUT.relative_to(ROOT)}")
            return 1
        if OUT.read_bytes() != content.encode():
            print(f"FAIL {OUT.relative_to(ROOT)} differs from the oracle's client runs")
            return 1
        print(f"PASS {OUT.relative_to(ROOT)} matches the oracle")
        return 0

    OUT.write_bytes(content.encode())
    print(f"wrote {OUT.relative_to(ROOT)} ({len(content)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())