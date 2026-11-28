"""Single gate for the Rust rewrite: build, differential harness, generated tables.

命令:
    python tools/verify.py            # 人读的 PASS/FAIL 汇总
    python tools/verify.py --json     # 同一结果 + 基座自证（commit / worktree / oracle md5）

判据:
    退出码 0 = 全部门禁全绿：`cargo build`（0 warning）、`tools/tdjson_diff.py`
    （failures=0）、`tools/binlog_diff.py`（binlog 四栏与 `binlog_dump` 逐行相同）、
    协议/RPC lane（`auth_key`、`msg_layer`、`transport`、`framing`、`msgid`、`rpc`、
    `rpc_error`、`auth_data`、`svc_msg`、`proxy_secret`、`header`、`session`、`web_proxy`、
    `misc` 的 `--selftest`）、`tools/tdjson_fuzz.py`
    （随机语料零分歧）、`tools/client_diff.py`（客户端 lane 对齐）、八个生成器
    `--check` 与仓库内容逐字节相同。

覆盖边界:
    覆盖 `tdjson` C ABI 的 `td_execute` 面（29 个同步 action + 全部 1114 个函数的参数解析，
    含 `getLanguagePackString` 的成功路径）、客户端 lane（一个 client、固定
    `setTdlibParameters`：首次运行逐行、同一 `database_directory` 的第二次运行按集合、
    `waitPhoneNumber` 下的未授权状态机逐行）与生成表的一致性。
    覆盖不到：第二次运行的 option 顺序（不可判，F078）、真实登录（需实时码 + 2FA，F079）、
    非空号码的 `setAuthenticationPhoneNumber`、8 个有状态日志 action 的随机输入（它们只能
    按有序序列判，见 SEQUENCE 段）。

失效条件:
    oracle 换了构建（md5 变）、Rust 换了 target/linker、或 `td/` 的 schema/数据表更新——
    任一条都要重判 gate 并更新 docs/facts.md。
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
UCrt = "C:/msys64/ucrt64/bin"
PYTHON = sys.executable

GATES = [
    (
        "build",
        [
            "cargo",
            "build",
            "--release",
            "--target",
            "x86_64-pc-windows-gnu",
            "--manifest-path",
            "rust/Cargo.toml",
        ],
    ),
    ("instrument", [PYTHON, "tools/tdjson_diff.py"]),
    ("binlog", [PYTHON, "tools/binlog_diff.py", "--all", "--selftest"]),
    ("binlog_write", [PYTHON, "tools/binlog_write_diff.py", "--selftest"]),
    ("binlog_kv", [PYTHON, "tools/binlog_kv_diff.py", "--selftest"]),
    ("tddb_paths", [PYTHON, "tools/tddb_paths_diff.py", "--selftest"]),
    ("sqlite_kv", [PYTHON, "tools/sqlite_kv_diff.py", "--selftest"]),
    ("tqueue", [PYTHON, "tools/tqueue_diff.py", "--selftest"]),
    ("crypto", [PYTHON, "tools/crypto_diff.py", "--selftest"]),
    ("auth_key", [PYTHON, "tools/auth_key_diff.py", "--selftest"]),
    ("msg_layer", [PYTHON, "tools/msg_layer_diff.py", "--selftest"]),
    ("transport", [PYTHON, "tools/transport_diff.py", "--selftest"]),
    ("framing", [PYTHON, "tools/framing_diff.py", "--selftest"]),
    ("msgid", [PYTHON, "tools/msgid_diff.py", "--selftest"]),
    ("rpc", [PYTHON, "tools/rpc_diff.py", "--selftest"]),
    ("rpc_error", [PYTHON, "tools/rpc_error_diff.py", "--selftest"]),
    ("auth_data", [PYTHON, "tools/auth_data_diff.py", "--selftest"]),
    ("svc_msg", [PYTHON, "tools/svc_msg_diff.py", "--selftest"]),
    ("proxy_secret", [PYTHON, "tools/proxy_secret_diff.py", "--selftest"]),
    ("header", [PYTHON, "tools/header_diff.py", "--selftest"]),
    ("session", [PYTHON, "tools/session_diff.py", "--selftest"]),
    ("web_proxy", [PYTHON, "tools/web_proxy_diff.py", "--selftest"]),
    ("misc", [PYTHON, "tools/misc_diff.py", "--selftest"]),
    ("tls_init", [PYTHON, "tools/tls_init_diff.py", "--selftest"]),
    ("dh_handshake", [PYTHON, "tools/dh_handshake_diff.py", "--selftest"]),
    ("rsa", [PYTHON, "tools/rsa_diff.py", "--selftest"]),
    ("public_rsa", [PYTHON, "tools/public_rsa_diff.py", "--selftest"]),
    ("public_rsa_cdn", [PYTHON, "tools/public_rsa_cdn_diff.py", "--selftest"]),
    ("buffer_slice", [PYTHON, "tools/buffer_slice_diff.py", "--selftest"]),
    ("shared_slice", [PYTHON, "tools/shared_slice_diff.py", "--selftest"]),
    ("dc_id", [PYTHON, "tools/dc_id_diff.py", "--selftest"]),
    ("slice", [PYTHON, "tools/slice_diff.py", "--selftest"]),
    ("slice_cmp", [PYTHON, "tools/slice_cmp_diff.py", "--selftest"]),
    ("slice_prefix", [PYTHON, "tools/slice_prefix_diff.py", "--selftest"]),
    ("slice_ptr", [PYTHON, "tools/slice_ptr_diff.py", "--selftest"]),
    ("hash", [PYTHON, "tools/hash_diff.py", "--selftest"]),
    ("slice_hash", [PYTHON, "tools/slice_hash_diff.py", "--selftest"]),
    ("path_view", [PYTHON, "tools/path_view_diff.py", "--selftest"]),
    ("string", [PYTHON, "tools/string_diff.py", "--selftest"]),
    ("number", [PYTHON, "tools/number_diff.py", "--selftest"]),
    ("base64", [PYTHON, "tools/base64_diff.py", "--selftest"]),
    ("utf8", [PYTHON, "tools/utf8_diff.py", "--selftest"]),
    ("hints", [PYTHON, "tools/hints_diff.py", "--selftest"]),
    ("unicode", [PYTHON, "tools/unicode_diff.py", "--selftest"]),
    ("string_builder", [PYTHON, "tools/string_builder_diff.py", "--selftest"]),
    ("translit", [PYTHON, "tools/translit_diff.py", "--selftest"]),
    ("fuzz", [PYTHON, "tools/tdjson_fuzz.py", "--seeds", "20261126,7,99,2027"]),
    ("client_options", [PYTHON, "tools/gen_client_options.py", "--check"]),
    ("client_lane", [PYTHON, "tools/client_diff.py"]),
    ("tl_schema", [PYTHON, "tools/gen_tl_schema.py", "--check"]),
    ("mime_tables", [PYTHON, "tools/gen_mime_tables.py", "--check"]),
    ("emoji_table", [PYTHON, "tools/gen_emoji_table.py", "--check"]),
    ("country_table", [PYTHON, "tools/gen_country_table.py", "--check"]),
    ("ton_address", [PYTHON, "tools/gen_ton_address.py", "--check"]),
    ("api_functions", [PYTHON, "tools/gen_api_functions.py", "--check"]),
]


def run(name: str, command: list[str]) -> dict:
    env = dict(os.environ)
    env["PATH"] = UCrt + ";" + env.get("PATH", "")
    # TDLib 的日志会把请求里的原始字节写进 stderr（corpus 有非法 UTF-8 的 cell），
    # 所以按字节收再宽松解码，别让 gate 自己因为解码炸掉
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True)
    output = ((result.stdout or b"") + (result.stderr or b"")).decode("utf-8", "replace")
    warnings = sum(1 for line in output.splitlines() if line.startswith("warning:"))
    return {
        "gate": name,
        "command": " ".join(command),
        "exit_code": result.returncode,
        "warnings": warnings,
        "tail": [line for line in output.strip().splitlines()[-3:]],
    }


def snapshot_meta() -> dict:
    """快照自证：这批 gate 跑在哪个 commit、哪个 oracle 二进制上。"""
    sys.path.insert(0, str(ROOT / "tools"))
    import tdjson_diff  # oracle 路径与 md5 pin 只此一份

    def git(*args: str) -> str | None:
        try:
            done = subprocess.run(
                ["git", *args], cwd=ROOT, capture_output=True, encoding="utf-8", errors="replace"
            )
        except OSError:
            return None
        return done.stdout.strip() if done.returncode == 0 else None

    oracle = Path(tdjson_diff.CPP)
    return {
        "commit": git("rev-parse", "HEAD"),
        # 工作树脏时 commit 不代表被测源码，快照不能自证
        "worktree_dirty": bool(git("status", "--porcelain")),
        "cpp_oracle": str(oracle),
        "cpp_oracle_md5": tdjson_diff.md5(oracle) if oracle.is_file() else None,
        "cpp_oracle_expected_md5": tdjson_diff.EXPECTED_CPP_MD5,
    }


def main(argv: list[str]) -> int:
    records = [run(name, command) for name, command in GATES]
    failures = [record for record in records if record["exit_code"] != 0]
    if "--json" in argv:
        snapshot = snapshot_meta()
        snapshot["gates"] = records
        snapshot["failures"] = len(failures)
        print(json.dumps(snapshot, indent=2, ensure_ascii=False))
    else:
        for record in records:
            mark = "PASS" if record["exit_code"] == 0 else "FAIL"
            extra = f" warnings={record['warnings']}" if record["gate"] == "build" else ""
            print(f"{mark} {record['gate']}{extra}")
            if record["exit_code"] != 0:
                for line in record["tail"]:
                    print(f"    {line}")
        print(f"\nfailures={len(failures)}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))