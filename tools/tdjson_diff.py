"""Same-process differential harness: C++ libtdjson.dll vs Rust tdjson_rs.dll over the C ABI.

命令:
    cargo build --release --target x86_64-pc-windows-gnu --manifest-path rust/Cargo.toml
    python tools/tdjson_diff.py [--skip-md5]

判据:
    退出码 0 = MATCH 组逐字节相同 且 CONTROL 组判为不同 且 floor 段稳定；
    任一不符即退出码 1，并打印两侧原文。两侧输出都为空的 case 不算证据。

覆盖边界:
    只覆盖 `td_execute` 这一条纯函数路径（不登录、不联网）。
    覆盖不到的：`td_send` / `td_receive` / `td_json_client_*` 的有状态客户端路径；
    `tddb` / `td/mtproto` / `tdactor` / `tdutils` 在 C ABI 之下，不在本仪器范围内；
    非 ASCII 文本（emoji/中文）的 UTF-16 偏移尚未入 corpus（F007 待证）。

失效条件:
    以下任一变化都必须重判 gate 并更新 docs/facts.md：
      - oracle 的 md5 不等于 EXPECTED_CPP_MD5（换了构建、改了源码、改了编译器）；
      - Rust 侧换了 target / linker（C ABI 兼容性重新验证）；
      - `td_api.tl` 变更导致 action/object 序列化格式变化。
"""

from __future__ import annotations

import ctypes
import hashlib
import sys
from pathlib import Path

CPP = Path("C:/tmp/td-build/libtdjson.dll")
RS = Path(__file__).resolve().parents[1] / "rust/target/x86_64-pc-windows-gnu/release/tdjson_rs.dll"
EXPECTED_CPP_MD5 = "0398814038978aaeef7f8d6ac62bb8ff"

MATCH = [
    '{"@type":"getTextEntities","text":"@abc @a #bd"}',
    '{"@type":"getTextEntities","text":"plain"}',
    '{"@type":"getTextEntities","text":"#one #two @abcd"}',
    '{"@type":"getTextEntities","text":"@a"}',
    '{"@type":"getTextEntities","text":"\\ud83d\\ude00 @abcd"}',
    '{"@type":"getTextEntities","text":"\\u4e2d\\u6587 #\\u6807\\u7b7e"}',
    '{"@type":"getTextEntities","text":"#abc\\ud83d\\ude00"}',
]

CONTROL = [
    '{"@type":"getOption","name":"version"}',
    '{"@type":"getTextEntities","text":"mail me at a@b.com"}',
]


def md5(path: Path) -> str:
    h = hashlib.md5()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def load(path: Path):
    lib = ctypes.CDLL(str(path))
    lib.td_execute.restype = ctypes.c_char_p
    lib.td_execute.argtypes = [ctypes.c_char_p]
    return lib


def run(lib, case: str) -> str | None:
    raw = lib.td_execute(case.encode())
    return None if raw is None else raw.decode()


def main(argv: list[str]) -> int:
    failures: list[str] = []

    if not CPP.is_file():
        print(f"FAIL oracle missing: {CPP}")
        return 1
    if not RS.is_file():
        print(f"FAIL rust dll missing: {RS}")
        return 1

    actual = md5(CPP)
    if "--skip-md5" not in argv and actual != EXPECTED_CPP_MD5:
        print(f"FAIL oracle md5 changed: {actual} != {EXPECTED_CPP_MD5}")
        print("     重判 gate 后再跑；确认无误时加 --skip-md5")
        return 1
    print(f"oracle md5={actual}")

    cpp = load(CPP)
    rs = load(RS)

    print("\n== floor: oracle 重复 3 次必须逐字节稳定 ==")
    for case in MATCH:
        outs = {run(cpp, case) for _ in range(3)}
        ok = len(outs) == 1
        if not ok:
            failures.append(f"floor unstable: {case}")
        print(f"{'PASS' if ok else 'FAIL'} {case}")
        if not ok:
            print(f"  outs: {sorted(outs)}")

    print("\n== MATCH: 两侧必须逐字节相同 ==")
    for case in MATCH:
        a = run(cpp, case)
        b = run(rs, case)
        ok = a == b and a is not None and a != ""
        if not ok:
            failures.append(f"match failed: {case}")
        print(f"{'PASS' if ok else 'FAIL'} {case}")
        print(f"  out: {a}")
        if a != b:
            print(f"  rs : {b}")

    print("\n== CONTROL: 两侧必须判为不同（证明仪器能区分） ==")
    for case in CONTROL:
        a = run(cpp, case)
        b = run(rs, case)
        ok = a != b
        if not ok:
            failures.append(f"control not discriminated: {case}")
        print(f"{'PASS' if ok else 'FAIL'} {case}")
        print(f"  cpp: {a}")
        print(f"  rs : {b}")

    print(f"\nfailures={len(failures)}")
    for f in failures:
        print(f"  - {f}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))