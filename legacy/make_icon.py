#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
生成应用图标：app.png（32x32，供窗口图标参考）与 app.ico（多尺寸，供打包用）。
纯标准库实现（struct + zlib），不依赖 PIL。

图标含义：一个圆角徽标内左侧绿色、右侧红色两个对比面板，代表"对拍/对比"。
用法：python3 make_icon.py
"""

import struct
import zlib

ACCENT = (47, 111, 237)      # 蓝
GREEN = (62, 180, 137)       # 绿
RED = (224, 85, 97)          # 红
WHITE = (255, 255, 255)
BG = (223, 230, 240)         # 浅灰蓝背景


def blank(w, h):
    """创建一个 w x h 的 RGBA 画布，全部填背景色。"""
    return [[list(BG) + [255] for _ in range(w)] for _ in range(h)]


def rounded_rect(px, x0, y0, x1, y1, r, color):
    """在画布上绘制一个填充的圆角矩形。"""
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            dx = max(x0 + r - x, 0, x - (x1 - r))
            dy = max(y0 + r - y, 0, y - (y1 - r))
            if dx * dx + dy * dy <= r * r:
                px[y][x] = list(color) + [255]


def draw(size):
    """按 size 尺寸绘制图标，返回 RGBA 像素行列表。"""
    px = blank(size, size)
    s = size / 32.0
    def rect(x0, y0, x1, y1, r, color):
        rounded_rect(px, int(x0 * s), int(y0 * s), int(x1 * s), int(y1 * s),
                     max(1, int(r * s)), color)
    rect(2, 2, 29, 29, 7, ACCENT)          # 外圈
    rect(5, 5, 26, 26, 5, BG)              # 掏空形成圆环
    rect(7, 9, 14, 23, 3, GREEN)           # 左绿面板
    rect(17, 9, 24, 23, 3, RED)            # 右红面板
    for y in range(10, 23):                # 中间白色分隔
        for x in (15, 16):
            px[int(y * s)][int(x * s)] = list(WHITE) + [255]
    return px


def png_bytes(px):
    """把 RGBA 像素画布编码为 PNG 字节。"""
    h = len(px)
    w = len(px[0])
    raw = b"".join(b"\x00" + bytes(v for pixel in row for v in pixel) for row in px)
    def chunk(tag, data):
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xffffffff))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)  # 8bit RGBA
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(raw, 9))
            + chunk(b"IEND", b""))


def ico_bytes(png_list):
    """把各尺寸 PNG 打包成 ICO 容器。"""
    sizes = [len(px) for px in png_list]
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    data = b""
    for w, px in zip(sizes, png_list):
        png = png_bytes(px)
        entries += struct.pack("<BBBBHHII",
                               w if w < 256 else 0,
                               w if w < 256 else 0,
                               0, 0, 1, 32, len(png), offset)
        data += png
        offset += len(png)
    return header + entries + data


def main():
    sizes = [16, 32, 48, 64, 128, 256]
    png_list = [draw(s) for s in sizes]

    with open("app.png", "wb") as f:
        f.write(png_bytes(draw(32)))
    with open("app.ico", "wb") as f:
        f.write(ico_bytes(png_list))
    print("已生成 app.png 与 app.ico")


if __name__ == "__main__":
    main()
