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
# 面口径与 src/model.rs 的 referenced_doc_lines() 一致：跳过 //! 行；
# 遇 #[cfg(test)]+mod 或 mod anchor_tests 截断＝其后算测试面。
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
for p in sorted(glob.glob('src/*.rs')):
    ls = [x.strip() for x in open(p, encoding='utf8').read().split('\n')]
    face = 'prod'
    for i, t in enumerate(ls):
        nxt = next((x for x in ls[i + 1:] if x), '')
        if face == 'prod' and ((t.startswith('#[cfg(test)]') and nxt.startswith('mod ')) or t.startswith('mod anchor_tests')):
            face = 'test'
        if t.startswith('//!') or not t:
            continue
        d = prod if face == 'prod' else testf
        for mm in re.finditer(r'§([一二三四五六七八九十廿]+):(\d+)', t):
            d.setdefault(cn2int(mm.group(1)), set()).add(int(mm.group(2)))
        for mm in re.finditer(r'\bmd:(\d+)', t):
            d.setdefault(chapter_of(int(mm.group(1))), set()).add(int(mm.group(1)))
        for mm in re.finditer(r'速查:(\d+)', t):
            d.setdefault(23, set()).add(int(mm.group(1)))

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
print(f'挂债合计 = {sum(len(v) for v in debt.values())}')
print(f'有推导器的章 = {sorted(deriv)}')
print(f'生产锚点=0 且 挂债=0 的章：{[n for n, s, e, t in rng if not prod.get(n) and not debt.get(n)]}')
print(f'生产锚点=0 但挂了债的章：{[n for n, s, e, t in rng if not prod.get(n) and debt.get(n)]}')
