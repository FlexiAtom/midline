#!/usr/bin/env python3
# midline - 《中线》核心规则引擎 CLI
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
# 用法：python3 scripts/coverage-probe.py [中线.MD 路径]｜不给则取 $MIDLINE_DOC，再退到 ~/中线.MD
# 进度盘点探针：把 src/*.rs 里所有文档锚点（§X:N 与 md:N／速查:N）按**文档行区间**归到章，
# 再与挂债数、推导器有无并列。
# 面口径与 src/model.rs 的 production_face() **同规则、独立实现**：跳过 //! 行；遇 `#[cfg(test)]`+`mod`
# 或 `mod anchor_tests` 截断＝其后全算测试面；`#[cfg(test)]` 挂在**普通 item** 上时只跳那一个 item 的区间
# （按"与属性行同缩进的收尾"定界）。这一条是 2026-10-01 §九 帧补的：探针当时只认 `+mod`，于是把解析器
# 那些测试专用 item 里的 `/// md:311` 也算成生产锚点——一章的锚点数虚高，盲区就从这张表上看不见了。
# 两边各数一遍是刻意的：同一串字两套口径，偏移只会露出来，不会互相圆过去。
import glob, re, os, sys

CN = '零一二三四五六七八九'
def cn2int(s):
    if s == '十': return 10
    if s in ('二十', '卅'): return 20
    if s.startswith('二十'): return 20 + CN.index(s[2:])
    if s.startswith('廿'): return 20 + (0 if s[1:] == '' else CN.index(s[1:]))
    if s.startswith('十'): return 10 + CN.index(s[1:])
    return CN.index(s)

doc = sys.argv[1] if len(sys.argv) > 1 else os.environ.get('MIDLINE_DOC', os.path.expanduser('~/中线.MD'))
lines = open(os.path.expanduser(doc), encoding='utf8').read().split('\n')
chs = []
for i, l in enumerate(lines, 1):
    m = re.match(r'^([一二三四五六七八九十廿]+)、(.+)$', l.strip())
    if m:
        chs.append((cn2int(m.group(1)), i, m.group(2).strip()))
chs.sort(key=lambda x: x[1])
rng = []
for k, (n, start, title) in enumerate(chs):
    end = chs[k + 1][1] - 1 if k + 1 < len(chs) else len(lines)
    rng.append((n, start, end, title))

def chapter_of(lineno):
    for n, s, e, t in rng:
        if s <= lineno <= e:
            return n
    return 0

prod, testf = {}, {}
item_tested = 0  # 只数「测试专用 item 被跳掉」那部分锚点出现次数：口径改动的可观测副作用，与 mod 截断不同源
near_miss = []
# 锚点语法的三份式样——与 `src/model.rs` 里的 `anchor_at`（两把尺共用的唯一定义）**逐条对齐**：
#   § 支冒号半角全角都认；md／速查 支只认半角冒号，且前面不许紧贴字母数字或 `.`
#   （`README.md:12` 是文件坐标，不是 中线 的第 12 行——放过它就是拿文件名替规则行作证＝虚覆盖）。
# 这里是**独立实现**，不是抄那份代码：两边同错的可能性由此下降，读数不一致时就知道其中一把漂了。
SEC_RE = r'§([一二三四五六七八九十廿]+)[:：](\d+)'
MD_RE = r'(?<![0-9A-Za-z.])md:(\d+)'
QUICK_RE = r'(?<![0-9A-Za-z.])速查:(\d+)'


def indent_of(s):
    return len(s) - len(s.lstrip())


def face_of(raw):
    """每行标 prod／test。规则同 production_face：看见测试 mod 就到此为止；看见挂在普通 item 上的
    #[cfg(test)] 只跳那一个 item——按与属性行同缩进的收尾（'}' 或无花括号时的 ';' 行）定界。"""
    ls = [x.strip() for x in raw]
    face = ['prod'] * len(ls)
    i, tail = 0, False
    while i < len(ls):
        t = ls[i]
        nxt = next((x for x in ls[i + 1:] if x), '')
        if t.startswith('mod anchor_tests') or (t.startswith('#[cfg(test)]') and nxt.startswith('mod ')):
            tail = True
            break
        if t.startswith('#[cfg(test)]'):
            ind, j, opened = indent_of(raw[i]), i + 1, False
            while j < len(raw):
                lt = raw[j].strip()
                if indent_of(raw[j]) == ind and lt:
                    if lt == '}':
                        j += 1
                        break
                    if not opened and lt.endswith(';'):
                        j += 1
                        break
                    opened |= '{' in lt
                j += 1
            for k in range(i, min(j, len(ls))):
                face[k] = 'item'
            i = j
            continue
        i += 1
    if tail:
        for k in range(i, len(ls)):
            face[k] = 'tail'
    return face


for p in sorted(glob.glob('src/*.rs')):
    raw = open(p, encoding='utf8').read().split('\n')
    ls = [x.strip() for x in raw]
    face = face_of(raw)
    for i, t in enumerate(ls):
        if t.startswith('//!') or not t:
            continue
        d = prod if face[i] == 'prod' else testf
        if face[i] == 'item':
            item_tested += len(re.findall(SEC_RE + r'|' + MD_RE + r'|' + QUICK_RE, t))
        for mm in re.finditer(SEC_RE, t):
            d.setdefault(cn2int(mm.group(1)), set()).add(int(mm.group(2)))
        for mm in re.finditer(MD_RE, t):
            d.setdefault(chapter_of(int(mm.group(1))), set()).add(int(mm.group(1)))
        for mm in re.finditer(QUICK_RE, t):
            d.setdefault(23, set()).add(int(mm.group(1)))
    # 隐形锚点：形似锚点、语法不认的连排（少冒号／阿拉伯章号）。Rust 侧 `no_doc_anchor_is_written_without_its_colon`
    # 是拦人的那一把，这里数同一件事是**对账**——两把尺各写一遍同一条规则，数字不一致就是其中一把漂了。
    # 扫描口径与那条测一致：逐**原文行**，不看生产面、不跳 `//!`（注释里的锚点也是给人的承诺）。
    for i, t in enumerate(raw):
        for mm in re.finditer(r'§[一二三四五六七八九十廿]+(?=[0-9])|§[0-9]+[:：](?=[0-9])', t):
            near_miss.append(f'{p}:{i + 1} `{t[mm.start():mm.end() + 4].strip()}`')

src = open('src/model.rs', encoding='utf8').read()
blk = src[src.index('const NOT_IMPLEMENTED'):]
blk = blk[:blk.index('\n    ];')]
debt = {}
for mm in re.finditer(r'doc: (\d+),\s*\n\s*section: (\d+),', blk):
    debt.setdefault(int(mm.group(2)), []).append(int(mm.group(1)))
# 挂债数一旦静默少算，"某章 0 债"就是假绿：字段顺序变了、或条目被上面的切片截断了，都必须当场响。
raw = len(re.findall(r'\n\s*doc: \d+,', blk))
if raw != sum(len(v) for v in debt.values()):
    sys.exit(f'✖ 债表解析对不上：文本里 {raw} 个 doc: 字段，只配对出 {sum(len(v) for v in debt.values())} 条'
             '（section 字段顺序变了？还是 const NOT_IMPLEMENTED 的收尾 `];` 找错了位置？）')

deriv = set(int(m.group(1)) for m in re.finditer(r'fn every_[a-z0-9_]*?section(\d+)_', src))

print('章   文档行区间     生产锚点  挂债  推导器  标题')
for n, s, e, t in rng:
    print(f'{n:>3}   {s:>4}-{e:<5}      {len(prod.get(n, ())):>4}   {len(debt.get(n, ())):>3}    {"有" if n in deriv else "—"}    {t}')
print()
print(f'生产面锚点唯一文档行合计 = {sum(len(v) for v in prod.values())}')
ov = sum(len(v & testf.get(n, set())) for n, v in prod.items())
print(f'测试面锚点唯一文档行合计 = {sum(len(v) for v in testf.values())}（其中 {ov} 行生产面也锚过 ⇒ 两行不可相加当总覆盖）')
print(f'测试专用 item 里被剔出生产面的锚点出现次数 = {item_tested}（那些是解析器／夹具的形状注释，不是实现锚点）')
print(f'隐形锚点（形似锚点、语法不认）= {len(near_miss)}'
      + ('' if not near_miss else '：拦人的那把是 `cargo test` 的 no_doc_anchor_is_written_without_its_colon，这里只负责对账')
      + ''.join('\n  ' + x for x in near_miss[:12]))
print(f'挂债合计 = {sum(len(v) for v in debt.values())}')
print(f'有推导器的章 = {sorted(deriv)}')
print(f'生产锚点=0 且 挂债=0 的章：{[n for n, s, e, t in rng if not prod.get(n) and not debt.get(n)]}')
print(f'生产锚点=0 但挂了债的章：{[n for n, s, e, t in rng if not prod.get(n) and debt.get(n)]}')
