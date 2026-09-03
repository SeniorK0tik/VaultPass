#!/usr/bin/env python3
"""Иконка приложения «Сейф» без внешних зависимостей.

В системе нет ни Pillow, ни ImageMagick, а тянуть их ради одной картинки
незачем: PNG-контейнер и ICO собираются из zlib и struct за сотню строк.
Рисунок — знак сейфа из макета: круглая дверь с ручкой-крестовиной,
акцент #9184d9 на фоне #161826.
"""
import math
import struct
import zlib
from pathlib import Path

BG = (0x16, 0x18, 0x26)
ACCENT = (0x91, 0x84, 0xD9)
ACCENT_DIM = (0x42, 0x3A, 0x6A)
SS = 4  # сглаживание избыточной выборкой

OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"


def draw(size: int):
    """Возвращает RGBA-буфер size×size, нарисованный в SS раз крупнее."""
    n = size * SS
    px = [[(0, 0, 0, 0)] * n for _ in range(n)]

    r_corner = n * 0.22
    cx = cy = n / 2.0

    def blend(dst, src, a):
        return tuple(round(d + (s - d) * a) for d, s in zip(dst, src))

    for y in range(n):
        for x in range(n):
            fx, fy = x + 0.5, y + 0.5

            # фон: квадрат со скруглёнными углами
            dx = max(r_corner - fx, fx - (n - r_corner), 0.0)
            dy = max(r_corner - fy, fy - (n - r_corner), 0.0)
            if math.hypot(dx, dy) > r_corner:
                continue
            px[y][x] = (*BG, 255)

            d = math.hypot(fx - cx, fy - cy)
            ring_r = n * 0.30
            ring_w = n * 0.055

            # тусклое кольцо-подложка
            if abs(d - ring_r) < ring_w * 1.9:
                px[y][x] = (*blend(px[y][x][:3], ACCENT_DIM, 0.55), 255)
            # само кольцо двери
            if abs(d - ring_r) < ring_w:
                px[y][x] = (*ACCENT, 255)
            # ступица
            if d < n * 0.085:
                px[y][x] = (*ACCENT, 255)

            # четыре спицы крестовины
            if n * 0.06 < d < n * 0.235:
                ang = math.atan2(fy - cy, fx - cx)
                for k in range(4):
                    target = k * math.pi / 2 + math.pi / 4
                    diff = abs((ang - target + math.pi) % (2 * math.pi) - math.pi)
                    if diff * d < n * 0.030:
                        px[y][x] = (*ACCENT, 255)
                        break

    # усреднение блоков SS×SS
    out = bytearray()
    for y in range(size):
        out.append(0)  # фильтр строки: None
        for x in range(size):
            acc = [0, 0, 0, 0]
            for j in range(SS):
                for i in range(SS):
                    p = px[y * SS + j][x * SS + i]
                    for c in range(4):
                        acc[c] += p[c]
            out.extend(bytes(v // (SS * SS) for v in acc))
    return bytes(out)


def png(size: int) -> bytes:
    raw = draw(size)

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8 бит, RGBA
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(raw, 9))
            + chunk(b"IEND", b""))


def ico(sizes) -> bytes:
    """ICO как контейнер готовых PNG — так его читают все Windows начиная с Vista."""
    images = [(s, png(s)) for s in sizes]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = len(header) + 16 * len(images)
    entries, blobs = b"", b""
    for s, data in images:
        entries += struct.pack("<BBBBHHII", s if s < 256 else 0, s if s < 256 else 0,
                               0, 0, 1, 32, len(data), offset)
        blobs += data
        offset += len(data)
    return header + entries + blobs


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for name, size in [("32x32.png", 32), ("128x128.png", 128),
                       ("128x128@2x.png", 256), ("icon.png", 512),
                       ("tray.png", 32)]:
        (OUT / name).write_bytes(png(size))
        print(f"  {name}")
    (OUT / "icon.ico").write_bytes(ico([16, 32, 48, 64, 128, 256]))
    print("  icon.ico")


if __name__ == "__main__":
    main()
