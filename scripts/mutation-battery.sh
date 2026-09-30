#!/usr/bin/env bash
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
# 反向覆盖两把尺子的反证电池（禁忌6 的"尺子有牙"）：每条变异都必须把某一条测撞红，并记下**红在哪条测**。
# 铁律：绝不改 `~/中线.MD`——要动文档就 cp 一份副本，用 `MIDLINE_DOC=<副本>` 把机检指过去。
# 用法：bash scripts/mutation-battery.sh   （在 mktemp 出来的 src 副本里改，工作树只读）
set -u
REAL="${MIDLINE_REAL_DOC:-$HOME/中线.MD}"
SRC="$(cd "$(dirname "$0")/.." && pwd)/src"
[ -f "$REAL" ] || { echo "找不到规格文档 $REAL（可用 MIDLINE_REAL_DOC 指定）"; exit 1; }
D="$(mktemp -d)"; trap 'rm -rf "$D"' EXIT
cp "$(dirname "$SRC")/Cargo.toml" "$(dirname "$SRC")/Cargo.lock" "$D/"
restore() { cp "$SRC"/*.rs "$D/src/" 2>/dev/null || { mkdir -p "$D/src"; cp "$SRC"/*.rs "$D/src/"; }; cp "$REAL" "$D/doc.md"; }
run() { (cd "$D" && MIDLINE_DOC="${DOC:-$REAL}" cargo test -q 2>&1 | grep -E -- "--- FAILED|panicked at|test result:" | head -6); }
py() { python3 -c "$1"; }

echo "### M0 对照（不打补丁，应全绿）"; restore; run
echo "### M1 抹掉 md:452 的两处行尾锚点 ⇒ 应红在 §十二 推导器（漏登记）"
restore; py "
import re,glob
for p in glob.glob('$D/src/*.rs'):
    s=open(p,encoding='utf8').read()
    s2=re.sub(r'\s*//\s*§十二:452[^\n]*','',s)
    if s2!=s: open(p,'w',encoding='utf8').write(s2)"
run
echo "### M2 给债条 md:416 挂一个真锚点 ⇒ 应红在债表（债已偿，请删条目）"
restore; py "
p='$D/src/progress.rs'; ls=open(p,encoding='utf8').read().split('\n')
ls[30]+='  // §十二:416 升级在准备阶段也放开'; open(p,'w',encoding='utf8').write('\n'.join(ls))"
run
echo "### M3 文档副本里凭空多一条流程步 ⇒ 应红在 §十二 总数 72→73（正证：推导器跟着文档长）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
ls.insert(414,'8. 变异检验：凭空多出的流程步'); open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M4 成员偷换：剔掉 md:443（→ 子步）却放进 md:498（【…】标题），总数仍 72 ⇒ 应红在标题计数"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
s=s.replace(\"if t.starts_with('【') {\",\"if t.starts_with('【') && n != 498 {\")
s=s.replace('            steps.push(n);','            if n != 443 { steps.push(n); }')
open(p,'w',encoding='utf8').write(s)"
run
echo "### M5 单边洗白：删掉 md:452 的锚点、同时把 452 塞进 NOT_A_RULE ⇒ 应红在围栏认领断言"
restore; py "
import re,glob
for p in glob.glob('$D/src/*.rs'):
    s=open(p,encoding='utf8').read()
    s2=re.sub(r'\s*//\s*§十二:452[^\n]*','',s)
    if s2!=s: open(p,'w',encoding='utf8').write(s2)
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
s=s.replace('    const NOT_A_RULE: &[(usize, &str)] = &[','    const NOT_A_RULE: &[(usize, &str)] = &[\n        (452, \"变异检验：删锚后改口说这行不算规则\"),')
open(p,'w',encoding='utf8').write(s)"
run
echo "### M6 抹掉 §廿三:1026（平局条件）的锚点 ⇒ 应红在 §廿三 推导器（漏登记）"
restore; py "
import glob
for p in glob.glob('$D/src/*.rs'):
    s=open(p,encoding='utf8').read()
    s2=s.replace(' §廿三:1026 平局条件。','')
    if s2!=s: open(p,'w',encoding='utf8').write(s2)"
run
echo "### M7 摘掉 DEBT_RESTATEMENTS 那条登记 ⇒ §廿二:971 与 §廿三:1029 一起失去认领，两把尺应同时红"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
i=s.index('    const DEBT_RESTATEMENTS'); j=s.index('];', i)
s=s[:i]+'    const DEBT_RESTATEMENTS: &[(usize, &[usize], &str)] = &[];'+s[j+2:]
open(p,'w',encoding='utf8').write(s)"
run
echo "### M8 复述口塞进不相干行（把 §廿三:1013 持业者形象 挂到「激励」主债下）⇒ 应红在 ⑤ key 前缀 + 债已偿"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
s=s.replace('        &[971, 1029],','        &[971, 1029, 1013],')
open(p,'w',encoding='utf8').write(s)"
run
echo "### M9 否定行的牙：在生产码面真写出「评分系统」⇒ 应红在 §廿三 否定行 0 命中断言"
restore; py "
p='$D/src/render.rs'; ls=open(p,encoding='utf8').read().split('\n')
i=next(k for k,l in enumerate(ls) if l.startswith('pub fn'))
ls.insert(i,'#[allow(dead_code)]')
ls.insert(i+1,'const _SCORING_PROBE: &str = \"评分系统\";')
ls.insert(i+2,'')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
run
echo "### M10 抹掉 §廿一:932（Boss 表第一章）的锚点 ⇒ 应红在 §廿一 推导器（漏登记）"
restore; py "
import re
p='$D/src/boss.rs'; s=open(p,encoding='utf8').read()
s2=re.sub(r'\s*//\s*§廿一:932[^\n]*','',s)
assert s2!=s, '没抹到 §廿一:932'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M11 放宽块首判据（把单 token 的小节头也当规则行）⇒ 应红在 §廿一 的 14 行死数与 headings 钉死"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
s2=s.replace('if t.split_whitespace().count() == 1 {','if false {')
assert s2!=s, '没打到块首判据'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M12 把债条 md:915 塞进 NOT_A_RULE ⇒ 应同时红在 §廿一 围栏排除表断言 + 债表「两头下注」"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
s2=s.replace('&[(970,','&[(915, \"变异试验：把债条塞进排除表\"), (970,')
assert s2!=s, '没塞进 NOT_A_RULE'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M13 文档副本 §廿一 模式表凭空多一条模式 ⇒ 应红在 §廿一 总数 14→15（正证：推导器跟着文档长）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[915].startswith('每日挑战 '), '文档行 916 变了，副本口径不再对：'+ls[915]
ls.insert(916,'合作 双人同屏（变异：凭空多出的模式）')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M14 抹掉 §十五:628（超额按攻击顺序分配）的锚点 ⇒ 应红在 §十五 推导器（漏登记）"
restore; py "
import re
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
s2=re.sub(r'[ \t]*§十五:628(?![0-9])','',s)
assert s2!=s, '没抹到 §十五:628'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M15 给示例行 md:645 挂一个真锚点 ⇒ 应红在 §十五「示例行不许走锚点路」（把实测降级成指向）"
restore; py "
p='$D/src/battle.rs'; ls=open(p,encoding='utf8').read().split('\n')
i=[k for k,l in enumerate(ls) if 'let d = std::mem::take(&mut self.pending_candle_d);' in l]
assert len(i)==1, i
ls[i[0]]+='  // §十五:645 变异试验：给示例行挂锚'
open(p,'w',encoding='utf8').write('\n'.join(ls))"
run
echo "### M16 标签判据放宽到不分围栏内外 ⇒ 应红在 §十五 总数：闭围栏前那条真示例行 659 被吞成标签"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
Q = chr(34) + chr(96)*3 + chr(34) + ')'   # 源文本里的闭合部分；用 chr 拼出来，免得反引号在 bash 双引号里被当命令替换
s2 = s.replace('(!fence && next_non_blank(n) == ' + Q, '(next_non_blank(n) == ' + Q)
assert s2!=s, '没打到标签判据'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M17 文档副本 §十五 触发条件凭空多一条 ⇒ 应红在 §十五 总数 34→35 ＋ 示例行号名单"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[611].startswith('4. '), '文档行 612 变了，副本口径不再对：'+ls[611]
ls.insert(612,'5. 变异检验：凭空多出的触发条件')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M18 引擎侧把剩余伤害下限从 0 改成 1 ⇒ 应红在 §十五 示例实测（正证：实测抓得住行为回归，非只注释面）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
s2=s.replace('let remain = (d - a).max(0);','let remain = (d - a).max(1);')
assert s2!=s, '没打到 remain 口径'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M20 抹掉 §十七:742（前排空才推进）的锚点 ⇒ 应红在 §十七 推导器（漏登记）"
restore; py "
import re
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
s2=re.sub(r'；§十七:742(?![0-9])','',s)
assert s2!=s, '没抹到 §十七:742'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M21 摘掉 §十七 标签判据的第三档（围栏外单 token＋下一非空行以「·」开头）⇒ 761 落回 rows ⇒ 红在总数 19→20 与标签名单"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
needle = ' || next_non_blank(n).starts_with(' + chr(39) + '·' + chr(39) + ')'
assert needle in s, '没找到第三档判据'
open(p,'w',encoding='utf8').write(s.replace(needle, ''))"
run
echo "### M22 行为回归：摘掉推进的前排占用闸（前排有卡也推）⇒ 应红在 §十七 真测／既有规则测（正证：尺子抓得住行为，不止注释面）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
needle='            if self.e_front[col].is_none() {'
assert s.count(needle)==1, s.count(needle)
s=s.replace(needle, needle.replace('is_none()','is_none() || true'))
open(p,'w',encoding='utf8').write(s)"
run
echo "### M23 叙述顶锚：从实现里抹掉 §十七:756，只把行号写进 model.rs 的 //! 裁定登记区 ⇒ 应红（叙述不算锚点，§十七 同款判据）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
assert '§十七:756' in s
open(p,'w',encoding='utf8').write(s.replace('§十七:756',''))
q='$D/src/model.rs'; ls=open(q,encoding='utf8').read().split('\n')
k=0
while k<len(ls) and not ls[k].startswith('//!'): k+=1
ls.insert(k+1, '//! 变异检验：这一行把 §十七:756 写在叙述里，不该被算成锚点')
open(q,'w',encoding='utf8').write('\n'.join(ls))"
run
echo "### M24 抹掉 §十六:678（未达阈值不触发）的锚点 ⇒ 应红在 §十六 推导器（漏登记）"
restore; py "
import re
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
s2=re.sub(r'\s*//\s*§十六:678(?P<c>[^\n]*)','',s)
assert s2!=s, '没抹到 §十六:678'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M25 文档副本把示例行 689 的「触发1次」改成「触发2次」⇒ 应只红在 §十六 示例实测（正证：第三条路真读文档数字跑引擎，不是硬编码期望）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[688].startswith('阈值6，业火值12'), '文档行 689 变了，副本口径不再对：'+ls[688]
ls[688]=ls[688].replace('触发1次','触发2次')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M26 行为回归：摘掉触发上限闸（同回合可连发）⇒ 应红在 §十六 示例实测而推导器绿（正证：实测抓得住行为，尺子不止注释面）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
needle='if snap.hp <= 0 || snap.triggered_turn == turn || !is_threshold_trait(snap.def.tr) {'
assert s.count(needle)==1, s.count(needle)
s=s.replace('snap.triggered_turn == turn || ','')
open(p,'w',encoding='utf8').write(s)"
run
echo "### M27 两头下注：把已挂债的 §十六:705 塞进 NOT_A_RULE ⇒ 应双红（§十六 排除表断言 ＋ 债表交叉核对）"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
old='const NOT_A_RULE: &[(usize, &str)] = &['
assert old in s, '没找到排除表'
i=s.index(old)+len(old)
open(p,'w',encoding='utf8').write(s[:i]+'(705, \"变异检验：已挂债的行又想进排除表\"), '+s[i:])"
run
echo "### M28 抹掉 §八:295（同列友方攻击+1）的锚点 ⇒ 应红在 §八 推导器（漏登记）"
restore; py "
import re
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
s2=re.sub(r'\s*//\s*§八:295(?P<c>[^\n]*)','',s)
assert s2!=s, '没抹到 §八:295'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M29 文档副本只改 md:295 的「效果」列（+1→+2）⇒ 应红在 §八 推导器的槽位等值（正证：两列是真比对，不是同一串读两遍）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[294].startswith('5 同列友方攻击+1 同列友方攻击+1'), '文档行 295 变了，副本口径不再对：'+ls[294]
ls[294]='5 同列友方攻击+1 同列友方攻击+2'
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M30 文档副本把 md:295 **两列一起**改成 +2 ⇒ 槽位照样自洽，应红在「变体名尾数 ↔ 文档增减量」这一层（正证：等值尺之外还有一把对着代码名字的尺）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[294].startswith('5 同列友方攻击+1 同列友方攻击+1'), '文档行 295 变了，副本口径不再对：'+ls[294]
ls[294]='5 同列友方攻击+2 同列友方攻击+2'
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M31 枚举行内注释改一字（AllyColAtk1 的注释 +1→+2）⇒ 应红在 §八 推导器的「技能列 ↔ 注释」逐字节比对"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
n='AllyColAtk1,         // 5 同列友方攻击+1'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'AllyColAtk1,         // 5 同列友方攻击+2'))"
run
echo "### M32 行为回归：把「同列友方攻击+1」实现成 -1 ⇒ 应红在正向测 stacked_skills_count_per_copy 而 §八 推导器绿（正证：本帧这把尺不碰引擎，行为仍要靠正向测兜）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
n='dmg += Self::skill_count('
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'dmg -= Self::skill_count('))"
run
echo "### M19 收尾：全部复原后整族应全绿"; restore; run
