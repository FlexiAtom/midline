#!/usr/bin/env python3
# midline - 截图像素读取器（只用 stdlib：zlib + struct）
# Copyright (C) 2026 FlexiAtom
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU Affero General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU Affero General Public License for more details.
#
# You should have received a copy of the GNU Affero General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.
"""读 PNG 像素，不引 PIL（本机 python3 没有 PIL，实测 import 直接失败）。

为什么自己解 PNG 而不是 shell 出去调 magick：装机脚本要每 2s 采四个点，
`magick -format %[pixel:...]` 一次一个点、还要跟 srgba() 字符串格式搏斗，
而一次解码全图只要一个进程。

坑（这次真踩了，见 pitfalls）：PNG 每个 scanline 是 **1 字节 filter type + stride 字节数据**，
把 filter 字节读在行尾会让所有颜色整体错位一格——错位时 (207,1495) 读成 #0001FF，
magick 同一处读成 #DF2E2E。两份读数打架时先怀疑自己这把尺。
"""
import struct
import sys
import zlib

FILTER_NONE, FILTER_SUB, FILTER_UP, FILTER_AVERAGE, FILTER_PAETH = range(5)


def _paeth(a: int, b: int, c: int) -> int:
    p = a + b - c
    pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    return b if pb <= pc else c


def load(path: str):
    """返回 (width, height, channels, rows)，rows[y] 是该行解压后的字节。"""
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path} 不是 PNG（签名不符）")
    pos, idat, size, depth, ctype = 8, bytearray(), 0, 0, 0
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length  # 4 长度 + 4 类型 + body + 4 CRC
        if kind == b"IHDR":
            size, height, depth, ctype = struct.unpack(">IIBB", body[:10])[:4]
            interlace = body[12] if len(body) > 12 else 0
            if interlace:
                raise SystemExit("不支持隔行 PNG（screencap 不产这种）")
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    if depth != 8:
        raise SystemExit(f"只支持 8 位通道，这份是 {depth}")
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ctype]
    raw = zlib.decompress(bytes(idat))
    stride = size * channels
    rows, prev = [], bytearray(stride)
    for y in range(height):
        f = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)])
        if f == FILTER_SUB:
            for x in range(channels, stride):
                line[x] = (line[x] + line[x - channels]) & 0xFF
        elif f == FILTER_UP:
            for x in range(stride):
                line[x] = (line[x] + prev[x]) & 0xFF
        elif f == FILTER_AVERAGE:
            for x in range(stride):
                left = line[x - channels] if x >= channels else 0
                line[x] = (line[x] + ((left + prev[x]) >> 1)) & 0xFF
        elif f == FILTER_PAETH:
            for x in range(stride):
                left = line[x - channels] if x >= channels else 0
                upleft = prev[x - channels] if x >= channels else 0
                line[x] = (line[x] + _paeth(left, prev[x], upleft)) & 0xFF
        elif f != FILTER_NONE:
            raise SystemExit(f"第 {y} 行的 filter 类型未知：{f}")
        rows.append(bytes(line))
        prev = line
    return size, height, channels, rows


def pixel(rows, channels, x, y):
    o = x * channels
    row = rows[y]
    if channels >= 3:
        return row[o], row[o + 1], row[o + 2]
    v = row[o]
    return v, v, v


def main(argv):
    if len(argv) < 3:
        raise SystemExit("用法：png-pixel.py size <png> | px <png> x,y[,x,y...] | hex <png> x,y #RRGGBB[,x,y,#RRGGBB...]")
    cmd, path = argv[1], argv[2]
    width, height, channels, rows = load(path)
    if cmd == "size":
        print(f"{width}x{height}")
        return 0
    if cmd == "px":
        out = []
        for spec in argv[3:]:
            x, y = (int(v) for v in spec.split(","))
            out.append(f"({x},{y})=#{'%02X%02X%02X' % pixel(rows, channels, x, y)}")
        print(" ".join(out))
        return 0
    if cmd == "hex":
        # 逐点比对：|实际-期望| 之和 <= tol 算中；打印每点读数，错了能直接看出错在哪。
        tol = int(argv[3])
        all_ok = True
        report = []
        for spec in argv[4:]:
            xs, ys, want = spec.split(",")
            x, y = int(xs), int(ys)
            r, g, b = pixel(rows, channels, x, y)
            wr, wg, wb = int(want[1:3], 16), int(want[3:5], 16), int(want[5:7], 16)
            delta = abs(r - wr) + abs(g - wg) + abs(b - wb)
            ok = delta <= tol
            all_ok &= ok
            report.append(f"({x},{y}) got #{'%02X%02X%02X' % (r, g, b)} want {want} d={delta} {'OK' if ok else 'NO'}")
        print(" | ".join(report))
        return 0 if all_ok else 1
    raise SystemExit(f"未知子命令 {cmd}（size|px|hex）")


if __name__ == "__main__":
    sys.exit(main(sys.argv))
