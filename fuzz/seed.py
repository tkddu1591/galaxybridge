"""Create synthetic, non-device-derived seeds in ignored fuzz corpus folders."""
from pathlib import Path
import struct

root = Path(__file__).resolve().parent / "corpus"
protocol = root / "protocol"
usb = root / "usb-layout"
protocol.mkdir(parents=True, exist_ok=True)
usb.mkdir(parents=True, exist_ok=True)


def words(*values):
    return struct.pack("<" + "I" * len(values), *values)


for length in [14, 60, 100, 468, 980, 1514]:
    message = words(1, 44 + length, 36, length, 0, 0, 0, 0, 0, 0, 0) + bytes([0x42]) * length
    (protocol / f"packet-{length}").write_bytes(message)
    (protocol / f"packet-padded-{length}").write_bytes(message + bytes(512))
(protocol / "initialize").write_bytes(words(0x80000002, 52, 1, 0, 1, 0, 1, 0, 1, 16384, 0, 0, 0))
(protocol / "query").write_bytes(words(0x80000004, 30, 1, 0, 6, 16) + bytes([2, 1, 2, 3, 4, 5]))
(protocol / "set").write_bytes(words(0x80000005, 16, 1, 0))
(protocol / "keepalive").write_bytes(words(0x80000008, 16, 1, 0))
(protocol / "notification").write_bytes(words(1, 0))
for status in [0x4001000b, 0x4001000c, 0xc0010015]:
    (protocol / f"status-{status}").write_bytes(words(7, 20, status, 0, 0))
(protocol / "ipc-ready").write_bytes(bytes([1, 2, 1, 2, 3, 4, 5]))
(protocol / "ipc-frame").write_bytes(bytes([2]) + bytes([0x42]) * 60)

records = [
    bytes([8, 11, 0, 2, 0xe0, 1, 3, 0]),
    bytes([9, 4, 0, 0, 1, 0xe0, 1, 3, 0]),
    bytes([5, 0x24, 0, 0x10, 1]),
    bytes([5, 0x24, 6, 0, 1]),
    bytes([7, 5, 0x81, 3, 8, 0, 9]),
    bytes([9, 4, 1, 0, 2, 0x0a, 0, 0, 0]),
    bytes([7, 5, 0x82, 2, 0, 2, 0]),
    bytes([7, 5, 3, 2, 0, 2, 0]),
]
for name, body in [
    ("rndis", b"".join(records)),
    ("no-union", b"".join(records[:3] + records[4:])),
    ("no-association", b"".join(records[1:])),
    ("alternate-data", b"".join(records[:5]) + bytes([9, 4, 1, 0, 0, 0x0a, 0, 0, 0])
     + bytes([9, 4, 1, 1, 2, 0x0a, 0, 0, 0]) + b"".join(records[6:])),
]:
    header = bytes([9, 2]) + struct.pack("<H", 9 + len(body)) + bytes([2, 1, 0, 0x80, 50])
    (usb / name).write_bytes(header + body)
