#!/usr/bin/env python3
# midline - 壳侧字面量色值扫描器（守门检 §8 的谓词本体，抽出来只因为有两个调用方）
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
"""判据只有一个家：`scripts/shell-export-surface.sh` 第 8 节（守门）与
`scripts/mutation-battery.sh` 的 M168（量这条检有没有牙）都调本文件。

为什么不各写一遍：电池抄一份正则，就等于守门检改了而电池不知道——那正是本仓一路在消灭的
"两份真值"。第三条负样本（往生产面塞裸 `[u8;3]` 色值）原本只能手工跑完再 `git checkout` 回退，
抽成脚本后它才能在副本里被复跑。

用法：python3 scripts/color-literal-scan.py <目录>   （退出码 1＝扫到字面量色值或某文件生产面空转）
"""
import re, sys, pathlib
NUM = rb'(0x[0-9a-fA-F]{1,2}|\d{1,3})'
HEX = re.compile(rb'#[0-9a-fA-F]{6}(?:[0-9a-fA-F]{2})?(?![0-9a-zA-Z])')
NUMRGB = re.compile(rb'\brgb\(\s*\d')
NUM3 = re.compile(rb'[\[(]\s*' + NUM + rb'\s*,\s*' + NUM + rb'\s*,\s*' + NUM + rb'\s*(?:,\s*' + NUM + rb'\s*)?[\])]')
def val(tok):
    b = tok[2:] if tok[:2] == b'0x' else tok
    try:
        return int(b, 16) if tok[:2] == b'0x' else int(b)
    except ValueError:
        return 256
bad = 0
# 扫描范围与主仓那条检**逐字同口径**（`md3.rs:766`）：跳空行与 `//`（含 `///`）行、遇 `#[cfg(test)]` 即停。
# 不停在测区前会把夹具坐标 `[10, 20, 30]` 当色值报红，把注释里的 `#3482FF` 也算进去同样是误红——
# 一条天天误报的检，最后学会的是把它静音，那就等于没有检。
for p in sorted(pathlib.Path(sys.argv[1]).glob('*.rs')):
    seen = 0
    for n, line in enumerate(p.read_bytes().split(b'\n'), 1):
        s = line.lstrip()
        if s.startswith(b'#[cfg(test)]'):
            break
        if not s or s.startswith(b'//'):
            continue
        seen += 1
        hits = [m.group(0) for m in HEX.finditer(line)] + [m.group(0) for m in NUMRGB.finditer(line)]
        hits += [m.group(0) for m in NUM3.finditer(line)
                 if all(val(m.group(k)) <= 255 for k in (1, 2, 3)) and any(val(m.group(k)) > 2 for k in (1, 2, 3))]
        for m in hits:
            print(f"      {p.name}:{n} 出现字面量色值 {m.decode(errors='replace')}")
            bad += 1
    if seen == 0:
        print(f"      {p.name} 生产面一行都没扫到 ⇒ 这条检在它身上是空转")
        bad += 1
if bad:
    print(f"      共 {bad} 处")
sys.exit(1 if bad else 0)
