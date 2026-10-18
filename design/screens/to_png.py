#!/usr/bin/env python3
"""Converts the renderer's binary PPM frames to PNG. Standard library only."""
import struct
import sys
import zlib
from pathlib import Path


def ppm_to_png(source: Path, target: Path) -> None:
    data = source.read_bytes()
    header, rest = data.split(b"\n", 3)[:3], data.split(b"\n", 3)[3]
    width, height = (int(v) for v in header[1].split())
    stride = width * 3
    raw = b"".join(b"\x00" + rest[row * stride:(row + 1) * stride] for row in range(height))

    def chunk(kind: bytes, payload: bytes) -> bytes:
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    target.write_bytes(png)


if __name__ == "__main__":
    folder = Path(sys.argv[1])
    for frame in sorted(folder.glob("*.ppm")):
        ppm_to_png(frame, frame.with_suffix(".png"))
        frame.unlink()
    print(f"PNG frames in {folder}")
