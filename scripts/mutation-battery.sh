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
# run() 现在把 panic 正文（前 3 行）也打出来：只看"红在哪条测"量不到"红在哪一层"，
# 而 §七／§八 这类多层等值的章，层号才是这条变异真正的落点。旧记录（M0–M32 那次整族）是 head -6 的截断口径。
# 想先验补丁写法而不必等整族：bash scripts/battery-preflight.sh（11 秒量完 58 条记录的补丁有没有真打上；
# 它就是把本文件的 run() 换成空壳、py() 原样保留来跑，所以电池改了判据行形状时要同步那条尺）。
# 本帧 M46 与 M57 各在这上面省了一次 15 分钟的白跑。
set -u
REAL="${MIDLINE_REAL_DOC:-$HOME/中线.MD}"
SRC="$(cd "$(dirname "$0")/.." && pwd)/src"
[ -f "$REAL" ] || { echo "找不到规格文档 $REAL（可用 MIDLINE_REAL_DOC 指定）"; exit 1; }
D="$(mktemp -d)"; trap 'rm -rf "$D"' EXIT
cp "$(dirname "$SRC")/Cargo.toml" "$(dirname "$SRC")/Cargo.lock" "$D/"
restore() { PATCH_FAIL=""; cp "$SRC"/*.rs "$D/src/" 2>/dev/null || { mkdir -p "$D/src"; cp "$SRC"/*.rs "$D/src/"; }; cp "$REAL" "$D/doc.md"; }
# PATCH_FAIL：补丁没打上时 run() 先喊出来，**不**假装"这条变异撞红了"。§九 帧整族复跑就撞上一条静默假绿——
# M46 的 `assert s.count(n)==1` 因 §九 推导器照抄了同一行守卫而变成 2，python 抛 AssertionError 后 cargo 照旧
# 151 全绿，日志里那行 traceback 只会被当成噪音（本帧之前从没查过它）。`bash -n` 也只验 shell 语法，验不到内嵌
# Python——所以判据不能靠人读，得让 run() 自己拒。
run() { local o; if [ -n "$PATCH_FAIL" ]; then printf '!! 补丁失败（%s）⇒ 这条没有打到码，下面的读数一律不算落点\n' "$PATCH_FAIL"; fi; o="$(cd "$D" && MIDLINE_DOC="${DOC:-$REAL}" cargo test -q 2>&1)"; printf '%s\n' "$o" | grep -E -- "--- FAILED|test result:"; printf '%s\n' "$o" | sed -n '/panicked at/,+3p' | head -12; printf '%s\n' "$o" | grep -E -- "^Traceback|AssertionError" | head -3; }
py() { local e rc; e="$(python3 -c "$1" 2>&1)"; rc=$?; [ -n "$e" ] && printf '%s\n' "$e" | sed "s#$D#<D>#g"; [ "$rc" -ne 0 ] && PATCH_FAIL="python exit=$rc"; return 0; }

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
echo "### M33 抹掉 §七:266（血量＝数值）的锚点 ⇒ 应红在 §七 推导器的认领路（漏登记）"
restore; py "
import re
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
s2=re.sub(r'；§七:266(?![0-9])[^\n]*','',s)
assert s2!=s, '没抹到 §七:266'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M34 字段行尾注释改一字（费用→花费）⇒ 应红在 §七 ①（注释与文档措辞逐字节等值）"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
n='    pub cost: i32, // 费用'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'    pub cost: i32, // 花费'))"
run
echo "### M35 文档副本把 md:255 的特性措辞换成技能那句（随机附加，可融合）⇒ ①②同红：② 抓的是「宿主错层」这个原因，不是又一个字面比对"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[254].strip().startswith('特性（固定，卡牌自带）'), '文档行 255 变了，副本口径不再对：'+ls[254]
ls[254]='  特性（随机附加，可融合）,'
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M36 文档副本把 md:277 技能数量「每张牌0~N个」改成「每张牌1个」⇒ 应红在 §七 ③（0~N 的措辞没了⇒容器形状的前提失效）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[276].startswith('数量 每张牌1个 每张牌0~N个'), '文档行 277 变了，副本口径不再对：'+ls[276]
ls[276]='数量 每张牌1个 每张牌1个'
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M37 文档副本改 md:280 示例行的技能引号串（+1累积→+9累积）⇒ 应红在 §七 ④（跨章等值：示例必须能在 §八 表体 12 行里原样找到）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert '攻击后自身+1累积' in ls[279], '文档行 280 变了，副本口径不再对：'+ls[279]
ls[279]=ls[279].replace('攻击后自身+1累积','攻击后自身+9累积')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M38 行为回归：融合时把副牌特性盖到主牌上（m.def.tr = s.def.tr）⇒ 应红在 §七 ⑤（剔注释后函数体仍出现 .tr）"
restore; py "
p='$D/src/progress.rs'; s=open(p,encoding='utf8').read()
n='    m.crafted = true;'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'    m.def.tr = s.def.tr;'+n))"
run
# ── M39–M46：§十四 帧。这一章是本仓第一条"同章并用两条路＋九行双记账"的章，所以每条变异都指定它红在**哪一层**：
#    双记账层（M39）／引擎实测的数字层（M40、M45）／② 封闭写点名单（M41）／③ 常量层（M42）／⑤ 对称层（M43）
#    ／④ 撤测层（M44）／围栏双口径层（M46）。M44 顺带把 `engine_repro_test_exists` 改了——只数 `fn 名字(` 拦不住
#    "摘掉 #[test]"，函数体一字不动而 cargo 再也不跑它；这条尺现在要求定义上方第一条非注释行恰是 #[test]。
echo "### M39 摘掉围栏行 md:592 的锚点（那行走的是实测路）⇒ 应红在「12 行全有锚」那一层，配比 3／0／9 看不见这层"
# 必须两处一起摘：592 在生产码面被提到两次——md:592 自己的锚点（battle.rs:929）＋另一侧写点上的交叉引用
# 「（与 §十四:592 同一条规则的另一侧）」（battle.rs:1015）。锚点扫描是**逐处独立取号**，只摘一处那行照旧"有锚"
# （本帧 M39 首跑实测：只摘 929 那处，整族 149 全绿）。交叉引用也算锚，这是这把尺现在的口径。
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='；§十四:592 我方每受到 1 伤害 → 蜡烛减短 1 单位'
b='（与 §十四:592 同一条规则的另一侧）'
assert s.count(a)==1 and s.count(b)==1, (s.count(a), s.count(b))
open(p,'w',encoding='utf8').write(s.replace(a,'').replace(b,''))"
run
echo "### M40 文档副本把两侧「每受到1伤害 → 蜡烛减短1单位」的减短量 1→2 ⇒ 应只红在 §十四 引擎实测（推导器只数行，不吃数字）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[591].strip()=='- 每受到1伤害 → 蜡烛减短1单位', ls[591]
assert ls[596].strip()=='- 每受到1伤害 → 蜡烛减短1单位', ls[596]
ls[591]=ls[591].replace('减短1单位','减短2单位'); ls[596]=ls[596].replace('减短1单位','减短2单位')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M41 生产码面新写一条文档里根本没有的减短路（每回合自动衰减）⇒ 应只红在 §十四 ② 封闭写点名单 10→11"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
n='impl Battle {'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n, n+chr(10)+'    #[allow(dead_code)]'+chr(10)+'    fn s14_probe_decay(&mut self) { self.p_candle -= 1; }'))"
run
echo "### M42 代码面把常量 CANDLE_HP 从 20 改成 21（文档不动）⇒ 应红在 §十四 ③（常量≠文档写的初始长度）＋引擎实测"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
n='pub const CANDLE_HP: i32 = 20;'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'pub const CANDLE_HP: i32 = 21;'))"
run
echo "### M43 摘掉敌方那一侧的画条调用（holder_line 改成不调 candle_bar）⇒ 应红在 §十四 ⑤ 计数 3→2 ＋ 行为面那条对称测"
restore; py "
p='$D/src/render.rs'; s=open(p,encoding='utf8').read()
n='candle_bar(h.hp, h.cap)'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'holder_label(h.name)'))"
run
echo "### M44 只摘掉复现测上方的 #[test]（函数体一字不动）⇒ 应红在 §十四 ④（正证：定义还在≠测还在，149 项会掉成 148）"
restore; py "
p='$D/src/battle.rs'; ls=open(p,encoding='utf8').read().split('\n')
i=[k for k,l in enumerate(ls) if l.strip().startswith('fn section14_candle_numbers_reproduce_on_the_engine(')]
assert len(i)==1, i
assert ls[i[0]-1].strip()=='#[test]', repr(ls[i[0]-1])
ls[i[0]-1]='// 变异试验：把 #[test] 摘掉'
open(p,'w',encoding='utf8').write('\n'.join(ls))"
run
echo "### M45 文档副本把两侧判死线 ≤0 改成 ≤5 ⇒ 应只红在 §十四 引擎实测的判死方向（推导器全绿：行数与名单没动）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert '蜡烛长度≤0' in ls[592] and '蜡烛长度≤0' in ls[597], (ls[592], ls[597])
ls[592]=ls[592].replace('≤0','≤5'); ls[597]=ls[597].replace('≤0','≤5')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M46 形状路的围栏守卫改成只看空行（围栏内的行也进 rows）⇒ 应红在「围栏外应得表体三行」（正证：同一串字两套口径必分叉）"
# 本条在 §九 帧复跑时**漂了**：那台推导器照抄了同一行守卫，`assert s.count(n)==1` 当场 AssertionError(2)，
# 补丁没打上而 cargo 照旧 151 全绿——电池自己的假绿，只有整族复跑＋逐条看 traceback 才露头。
# 现在把落点扩到"守卫＋它下面那一行"，两章那行字面同形、下一行不同，用它定唯一。
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
n='            if t.is_empty() || fence {'+chr(10)+'                continue;'+chr(10)+'            }'+chr(10)+'            let nb = next_non_blank(n);'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'            if t.is_empty() {'+chr(10)+'                continue;'+chr(10)+'            }'+chr(10)+'            let nb = next_non_blank(n);'))"
run
# ── M47–M56：§九 帧。本章是本仓第一条**四条路同章并用**的章（锚点／挂债／实测／示例），所以这十条里
#    有四条专门量"层与层不互替"：M47 只红双记账、M48 只红示例禁锚、M51 红三处、M52 只红债表；
#    M53–M56 四条动的是**判据与文档**（不是代码）——M54 更是本帧正证 `production_face` 那处机检自身缺陷：
#    修复前测试 item 的注释被算成生产锚点，摘掉两处真锚、在 cfg(test) 枚举里补一句假锚就能骗过整族。
echo "### M47 摘掉 §九:324 的锚点（那行走的是实测路）⇒ 应只红在双记账层，配比 6／1／20 不动"
restore; py "
p='$D/src/progress.rs'; s=open(p,encoding='utf8').read()
n='§九:324 新牌占用主牌那一格；'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,''))"
run
echo "### M48 给示例行 md:341 补一枚锚点 ⇒ 应只红在示例禁锚层（341 走实测路，配比看不见这层）"
restore; py "
p='$D/src/progress.rs'; s=open(p,encoding='utf8').read()
n='    *karma -= price;'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n, n+'  // §九:341 变异检验：给示例行补一枚锚'))"
run
echo "### M49 只改融合价的一侧（progress.rs 的 -1→-2，ai.rs 不动）⇒ 应红在 ⑤ 逐字同式计数 2→1 ＋ 三条扣费测"
restore; py "
p='$D/src/progress.rs'; s=open(p,encoding='utf8').read()
n='(inherit[sub].def.cost - 1).max(0)'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'(inherit[sub].def.cost - 2).max(0)'))"
run
echo "### M50 开端闸只查主牌一侧 ⇒ 应红在 ⑤ 体内 is_starter() 2→1 ＋ 复现测 ＋ 正向拒绝测"
restore; py "
p='$D/src/progress.rs'; s=open(p,encoding='utf8').read()
n='if inherit[main].is_starter() || inherit[sub].is_starter() {'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'if inherit[main].is_starter() {'))"
run
echo "### M51 从债表删掉 md:335 那条（实现一字不动）⇒ 应红在债表条数 26→25 ＋ §九 配比 1→0（335 落进实测桶）"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
i=s.index('Debt {'+chr(10)+'            doc: 335,')
k=s.rindex(chr(10), 0, i)+1
j=s.index(chr(10)+'        },'+chr(10), i)+len(chr(10)+'        },'+chr(10))
open(p,'w',encoding='utf8').write(s[:k]+s[j:])"
run
echo "### M52 给债行 md:335 补一枚锚点、债条不删 ⇒ 应只红在债表「债已偿，请删条目」（配比与双记账两层都不认它）"
restore; py "
p='$D/src/progress.rs'; s=open(p,encoding='utf8').read()
n='    m.crafted = true;'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n, n+' // §九:335 变异检验：拿锚点替债行作证'))"
run
echo "### M53 摘掉标签判据里那条「不以全角冒号收尾」的否决 ⇒ 应红在标签计数（354 又被当标签吞了）"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
n=\"if arity(n) == 1 && !t.ends_with('：') && (nb_text\"
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'if arity(n) == 1 && (nb_text'))"
run
echo "### M54 摘掉 battle.rs 两处 §十二:452 真锚，改在 cfg(test) 的枚举里补一句同名假锚 ⇒ 应仍红（正证 production_face 不把测试注释算成锚）"
# 修复前这条会**假绿**：`referenced_doc_lines` 当年看见 `#[cfg(test)]` 就跳后半份文件，把测试注释一并算进锚点面。
# 现在只跳那一个 item，所以 452 的锚在测试面就是没锚——本条是这把尺自己那颗牙的实证。
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='  // §十二:452 开端回合末业力每关上限 2 次'
b='  // §十二:452 开端在场→我方获得 1 业力'
assert s.count(a)==1 and s.count(b)==1, (s.count(a), s.count(b))
open(p,'w',encoding='utf8').write(s.replace(a,'').replace(b,''))
p='$D/src/progress.rs'; s=open(p,encoding='utf8').read()
n='    Definition,'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'    /// §十二:452 变异检验：测试注释里的锚点不算实现'+chr(10)+n))"
run
echo "### M55 文档副本把 md:324 保留清单里的「费用」写成「花费」（引擎与 md:328–331 不动）⇒ 应只红在 ④ 文档两处对撞"
# 曾试过改 md:328 那行的字段名（特性→特性值）：撞红的位置不是 ④，而是解析器的形状守卫 progress.rs:180
# 「『特性值』的右边不是「主牌特性值」而是「主牌特性」」——改一半的措辞根本进不到跨行对撞那一层。
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split(chr(10))
assert ls[323]=='1. 选择主牌（保留其特性+数值+阈值+费用）', ls[323]
ls[323]='1. 选择主牌（保留其特性+数值+阈值+花费）'
open(p,'w',encoding='utf8').write(chr(10).join(ls))"
DOC="$D/doc.md" run
echo "### M56 文档副本把 md:342 副牌费用 3→4 ⇒ 应红在 ④（344 被减的数≠342 的费用）＋ 复现测（引擎卡表那一侧没变）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split(chr(10))
assert ls[341].startswith('副牌：燎原（3费'), ls[341]
ls[341]=ls[341].replace('3费','4费',1)
open(p,'w',encoding='utf8').write(chr(10).join(ls))"
DOC="$D/doc.md" run
echo "### M57 §九 那台推导器的同一行围栏守卫做同样降级 ⇒ 实测红在**标签计数**（347／363 被吞成标签）而非 rows：同一处降级在两章落在不同层，这条落点是量出来的不是推的"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
n='            if t.is_empty() || fence {'+chr(10)+'                continue;'+chr(10)+'            }'+chr(10)+'            let nb_text = at(next_non_blank(n)).trim();'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'            if t.is_empty() {'+chr(10)+'                continue;'+chr(10)+'            }'+chr(10)+'            let nb_text = at(next_non_blank(n)).trim();'))"
run
echo "### M19 收尾：全部复原后整族应全绿"; restore; run
