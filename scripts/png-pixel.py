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


def load_ppm(path: str):
    """读 P6（二进制 RGB）PPM，返回 (width, height, channels, rows)，与 `load` 同形。

    host 侧对撞工具 `probes/host-frame` 出的就是这张格式：它只要 stdlib 能写、能读，
    不引 PNG 编码器（zlib 压缩对"逐像素是否相同"这句判据不提供任何东西）。
    """
    data = open(path, "rb").read()
    if not data.startswith(b"P6"):
        raise SystemExit(f"{path} 不是 P6 PPM")
    # 头是 "P6\n<w> <h>\n<max>\n"，按空白切四段（本项目产出的那份不含注释，故不实现 # 注释）。
    tok, i, out = [], 0, []
    while len(out) < 4:
        while i < len(data) and data[i : i + 1].isspace():
            i += 1
        j = i
        while j < len(data) and not data[j : j + 1].isspace():
            j += 1
        out.append(data[i:j].decode())
        i = j
    w, h, maxv = int(out[1]), int(out[2]), int(out[3])
    if maxv != 255:
        raise SystemExit(f"只认 8 位通道（读到 maxval={maxv}）")
    body = data[i + 1 :]
    need = w * h * 3
    if len(body) < need:
        raise SystemExit(f"{path} 像素体短了：{len(body)} < {need}")
    stride = w * 3
    return w, h, 3, [body[y * stride : y * stride + stride] for y in range(h)]


def pixel(rows, channels, x, y):
    o = x * channels
    row = rows[y]
    if channels >= 3:
        return row[o], row[o + 1], row[o + 2]
    v = row[o]
    return v, v, v


def corner_size(rows_p, ch_p):
    """量 PPM 左上角那块纯色正方形的边长（不写死 8：写死就等于尺跟着代码跑，改了不报）。"""
    corner = rows_p[0][:ch_p]
    w = len(rows_p[0]) // ch_p
    xr = 0
    while xr < w and bytes(rows_p[0][xr * ch_p : xr * ch_p + ch_p]) == corner:
        xr += 1
    yr = 0
    while yr < len(rows_p) and bytes(rows_p[yr][:ch_p]) == corner:
        yr += 1
    return min(xr, yr)


def _mode_color(rows, ch, w, h):
    """采样取众数色（每 5 格取 1）：对撞要比的是"有墨的格子"，底色就是那块正方形的多数派。"""
    from collections import Counter

    c = Counter()
    for y in range(0, h, 5):
        r = rows[y]
        for x in range(0, min(w, len(r) // ch), 5):
            c[r[x * ch : x * ch + 3]] += 1
    return c.most_common(1)[0][0] if c else b"\x00\x00\x00"


def _in_any(dx, dy, exclude):
    return any(x0 <= dx <= x1 and y0 <= dy <= y1 for x0, y0, x1, y1 in exclude)


def ink_ratio(rowsp, rowsd, chp, chd, pw, ph, ox, oy, exclude):
    """两侧"非底色格数"之比。逐像素全等是强判据，这一格是**归因**判据：
    比值为 0 说明设备侧一片空白（没到屏），>1.2 多半是行错位或多画了一层。"""
    exclude = exclude or []
    bgp = _mode_color(rowsp, chp, pw, ph)
    bgd = _mode_color([rowsd[oy + y] for y in range(min(ph, len(rowsd) - oy))], chd, pw, ph)
    hp = hd = 0
    for y in range(ph):
        dy = oy + y
        if dy >= len(rowsd):
            break
        pr, dr = rowsp[y], rowsd[dy]
        for x in range(pw):
            dx = ox + x
            if dx >= len(dr) // chd:
                break
            if _in_any(dx, dy, exclude):
                continue
            if pr[x * chp : x * chp + 3] != bgp:
                hp += 1
            if dr[dx * chd : dx * chd + 3] != bgd:
                hd += 1
    return (hd / hp) if hp else float("nan")


def locate(rows_p, rows_d, ch_p, ch_d, w, h, size):
    """在设备图里找出 PPM 左上角那块 `size×size` 色块的**左上角坐标**。

    为什么不让人的截图偏移量手填：那块色块正是 `text::fill_bg` 画在缓冲 (0,0) 的定位点，
    它的存在就是为了回答"我们的缓冲第一格落在屏幕哪一格"。手填偏移等于把这把尺交给人眼。

    两边的通道数可以不同（host 的 PPM 是 3，screencap 的 PNG 常是 4），所以比对走"每格取前三字节"，
    先把设备每行的 R 分量抽成一条平面做粗筛，命中再逐格验——直接按字节 `find` 整行会把 alpha 卷进来。
    """

    def ppx(x, y):
        o = x * ch_p
        return rows_p[y][o : o + 3]

    def dpx(x, y):
        o = x * ch_d
        return rows_d[y][o : o + 3]

    want = ppx(0, 0)
    for y, row in enumerate(rows_d):
        rplane = row[0::ch_d]
        base = 0
        while True:
            x = rplane.find(want[:1], base)
            if x < 0:
                break
            base = x + 1
            if x + size > w or y + size > len(rows_d) or dpx(x, y) != want:
                continue
            if all(dpx(x + k, y + j) == ppx(k, j) for j in range(size) for k in range(size)):
                return x, y
    return None


def diff(rows_p, rows_d, ch_p, ch_d, pw, ph, ox, oy, exclude):
    """逐像素对撞：返回 (比对格数, 全等差档计数, 最大单通道差, 差异 bbox, 被排除格数)。"""
    exclude = exclude or []
    n = exact = le1 = le2 = worst = skipped = 0
    box = [1 << 30, 1 << 30, -1, -1]
    for y in range(ph):
        dy = oy + y
        if dy >= len(rows_d):
            break
        pr, dr = rows_p[y], rows_d[dy]
        for x in range(pw):
            dx = ox + x
            if dx >= len(dr) // ch_d:
                break
            if _in_any(dx, dy, exclude):
                skipped += 1
                continue
            i, j = x * ch_p, dx * ch_d
            if ch_p == ch_d == 3:
                d = max(abs(pr[i] - dr[j]), abs(pr[i + 1] - dr[j + 1]), abs(pr[i + 2] - dr[j + 2]))
            else:
                d = max(abs(a - b) for a, b in zip(pr[i : i + 3], dr[j : j + 3]))
            n += 1
            worst = max(worst, d)
            if d == 0:
                exact += 1
            elif d <= 1:
                le1 += 1
            elif d <= 2:
                le2 += 1
            else:
                box[0], box[1] = min(box[0], dx), min(box[1], dy)
                box[2], box[3] = max(box[2], dx), max(box[3], dy)
    return n, (exact, le1, le2), worst, (None if box[2] < 0 else tuple(box)), skipped


def blocks(rows_p, rows_d, ch_p, ch_d, pw, ph, ox, oy, tol, exclude):
    """差 > tol 的格子做 **4-邻域连通块**，每块带两侧各自的众数色。

    为什么要有这一格：排除矩形必须由**测量**给出，手填等于"把没解释的东西写进尺里"。
    第十八步·其一 那六块是手量的矩形＋事后逐块数格；这里把"量"这一步收进尺：
    下一帧截屏换了形状（悬浮件挪位、挖孔到边、我们画漏一块），先跑 blocks，读数自己会指到块上。
    返回 (差格总数, [(格数, x0, y0, x1, y1, host 众数色, 设备众数色), …]) 按格数降序。"""
    from collections import Counter

    exclude = exclude or []
    bad = set()
    for y in range(ph):
        dy = oy + y
        if dy >= len(rows_d):
            break
        pr, dr = rows_p[y], rows_d[dy]
        for x in range(pw):
            dx = ox + x
            if dx >= len(dr) // ch_d:
                break
            if _in_any(dx, dy, exclude):
                continue
            i, j = x * ch_p, dx * ch_d
            d = max(abs(a - b) for a, b in zip(pr[i : i + 3], dr[j : j + 3]))
            if d > tol:
                bad.add((dx, dy))
    out = []
    seen = set()
    for start in bad:
        if start in seen:
            continue
        stack = [start]
        seen.add(start)
        blk = []
        while stack:
            x, y = stack.pop()
            blk.append((x, y))
            for nb in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
                if nb in bad and nb not in seen:
                    seen.add(nb)
                    stack.append(nb)
        xs = [p[0] for p in blk]
        ys = [p[1] for p in blk]
        cp = Counter()
        cd = Counter()
        for x, y in blk:
            # blk 存的是**设备**坐标 (dx,dy)：host 那侧行列都要各自减掉定位偏移，
            # 否则整块读数会平移一格（其一 那次偏移是 (52,0)，正是最容易把这条读错的情形）。
            cp[rows_p[y - oy][(x - ox) * ch_p : (x - ox) * ch_p + 3]] += 1
            cd[rows_d[y][x * ch_d : x * ch_d + 3]] += 1
        out.append(
            (len(blk), min(xs), min(ys), max(xs), max(ys),
             "#%02x%02x%02x" % tuple(cp.most_common(1)[0][0]),
             "#%02x%02x%02x" % tuple(cd.most_common(1)[0][0]))
        )
    out.sort(reverse=True)
    return len(bad), out


def main(argv):
    if len(argv) < 3:
        raise SystemExit(
            "用法：png-pixel.py size <png> | px <png> x,y [x,y ...] | hex <png> <tol> x,y,#RRGGBB [...] "
            "| diff <设备.png> <host.ppm> [exclude=X0,Y0,X1,Y1]... [dot=边长] "
            "| blocks <设备.png> <host.ppm> [tol=N] [exclude=...]（差>tol 的格子分连通块，含两侧众数色）"
        )
    cmd, path = argv[1], argv[2]
    width, height, channels, rows = load(path)
    if cmd == "diff":
        ppm = argv[3]
        exclude = []
        dot = None
        for extra in argv[4:]:
            if extra.startswith("exclude="):
                exclude.append(tuple(int(v) for v in extra[len("exclude=") :].split(",")))
            elif extra.startswith("dot="):
                dot = int(extra[len("dot=") :])
        pw, ph, chp, rowsp = load_ppm(ppm)
        dw, dh, chd, rowsd = load(path)
        size = dot or corner_size(rowsp, chp)
        at = locate(rowsp, rowsd, chp, chd, pw, ph, size)
        if at is None:
            raise SystemExit(
                f"设备图里找不到那块 {size}×{size} 定位点——两种可能：壳没画（logcat 里 painted=0），"
                f"或这条对撞尺的定位点口径已随 fill_bg 改动（那就先改尺，别先改画面）"
            )
        ox, oy = at
        n, (eq, le1, le2), worst, box, skipped = diff(rowsp, rowsd, chp, chd, pw, ph, ox, oy, exclude)
        ratio = ink_ratio(rowsp, rowsd, chp, chd, pw, ph, ox, oy, exclude)
        print(
            f"diff 设备 {dw}x{dh} ← PPM {pw}x{ph} @({ox},{oy}) 比对 {n} 格"
            f"{'，排除 %d 格（%d 个矩形）' % (skipped, len(exclude)) if exclude else ''}\n"
            f"  全等 {eq}（{100.0 * eq / max(n, 1):.4f}%）  差≤1 {le1}  差≤2 {le2}  其余 {n - eq - le1 - le2}"
            f"  单通道最大差 {worst}\n"
            f"  墨量比 {ratio:.4f}（设备/ host，>1 或 <1 都说明有一侧整片没画或画多了）"
            f"  差异 bbox {box if box else '无'}"
        )
        if exclude and skipped < sum((x1 - x0 + 1) * (y1 - y0 + 1) for x0, y0, x1, y1 in exclude):
            print(
                "  [注意] 实际排除格数 < 各矩形面积之和——要么矩形互相重叠，要么有矩形超出比对画面。"
                "记账按现量格数，别拿矩形面积当排除量"
            )
        return 0 if n and eq == n else 1

    if cmd == "blocks":
        ppm = argv[3]
        tol = 2
        exclude = []
        for extra in argv[4:]:
            if extra.startswith("exclude="):
                exclude.append(tuple(int(v) for v in extra[len("exclude=") :].split(",")))
            elif extra.startswith("tol="):
                tol = int(extra[len("tol=") :])
        pw, ph, chp, rowsp = load_ppm(ppm)
        dw, dh, chd, rowsd = load(path)
        size = corner_size(rowsp, chp)
        at = locate(rowsp, rowsd, chp, chd, pw, ph, size)
        if at is None:
            raise SystemExit(f"设备图里找不到那块 {size}×{size} 定位点，分块无从对齐")
        ox, oy = at
        n, comps = blocks(rowsp, rowsd, chp, chd, pw, ph, ox, oy, tol, exclude)
        print(
            f"blocks 设备 {dw}x{dh} ← PPM {pw}x{ph} @({ox},{oy})  tol={tol}"
            + (f"  排除 {len(exclude)} 个矩形" if exclude else "")
        )
        print(f"  差 > {tol} 共 {n} 格，连通块 {len(comps)} 个")
        for c in comps[:15]:
            cnt, x0, y0, x1, y1, hp, hd = c
            print(
                f"  {cnt:>7} 格  ({x0},{y0})-({x1},{y1})  {x1 - x0 + 1}x{y1 - y0 + 1}"
                f"  host={hp}  设备={hd}"
            )
        if len(comps) > 15:
            print(f"  …另有 {len(comps) - 15} 块未列（都计入上面的总数）")
        tot = sum(c[0] for c in comps)
        print(f"  列出的块合计 {tot} 格／全部 {n} 格（差≤tol 的格子不参与分块，所以这个和应等于 n）")
        return 0

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
