"""Generate the TON address cells used by tools/tdjson_diff.py.

命令:
    python tools/gen_ton_address.py            # 打印两个地址
    python tools/gen_ton_address.py --check    # 与仪器里的字面量逐字节比对，不一致退出 1

判据:
    地址 = base64url(tag ‖ workchain ‖ 32 字节 hash ‖ crc16)（36 字节 → 48 字符，无 padding），
    tag 0x11 = bounceable（首字符 E），tag 0x51 = non-bounceable（首字符 U）。
    hash 取固定的 bytes(range(32))，所以输出是确定的；`--check` 证明仪器里的字面量没有被手改。

覆盖边界:
    只覆盖 `getTextEntities` 认的 user-friendly 形式（48 字符 base64url + crc16）。
    覆盖不到：raw 形式（`0:<hex>`）、31/63 字节的 masterchain 地址、非法校验和样本。

失效条件:
    TDLib 改了 `TonWalletManager::check_ton_address` 的 tag/workchain/crc 规则，
    或 `tools/tdjson_diff.py` 里的字面量被替换。
"""

from __future__ import annotations

import base64
import sys
from pathlib import Path

INSTRUMENT = Path(__file__).resolve().parent / "tdjson_diff.py"
HASH = bytes(range(32))


def crc16(data: bytes) -> int:
    crc = 0
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) if crc & 0x8000 else (crc << 1)
            crc &= 0xFFFF
    return crc


def address(tag: int) -> str:
    body = bytes([tag, 0x00]) + HASH
    crc = crc16(body)
    return base64.urlsafe_b64encode(body + bytes([crc >> 8, crc & 0xFF])).decode().rstrip("=")


def main(argv: list[str]) -> int:
    bounceable = address(0x11)
    non_bounceable = address(0x51)
    if "--check" in argv:
        text = INSTRUMENT.read_text(encoding="utf-8")
        missing = [a for a in (bounceable, non_bounceable) if a not in text]
        if missing:
            print("FAIL instrument does not carry:", *missing)
            return 1
        print("PASS instrument carries both generated addresses")
        print(f"{bounceable}\n{non_bounceable}")
        return 0
    print(bounceable)
    print(non_bounceable)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))