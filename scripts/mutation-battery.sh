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
# 想先验补丁写法而不必等整族：bash scripts/battery-preflight.sh（十几秒量完 99 条记录的补丁有没有真打上；
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
# 静默失效共有三种，run() 对前两种各有一句喊话（本帧 M33／M54 各撞一种）：
#   ① 补丁没打上（assert 炸）⇒ PATCH_FAIL 喊。
#   ② 补丁打上了、却没产出任何 `test result:` 行＝编译失败 ⇒ run() 喊"这条没有落点"。
#      M54 是抠掉注释**中间一段**，把后面续写的 `；§五:200 …` 留成裸代码，整条记录在日志里成了空白。
#   ③ 补丁打上了、编译也过，但**这一族本来就看不见它**——M33 抹掉 §七:266 的锚点却整族全绿，
#      因为 §五 帧新写了一句「见 §七:265／§七:266」，而交叉引用也算锚（M39 定的口径），多出来的那处把抹除吸收了。
#      这种 preflight 量不到（补丁确实打上了），只有整族复跑会露。所以：凡改动过注释的帧，末了必须整族复跑一次；
#      记录里也一律断言"命中次数 == 预期"，多一处提及就当场喊，而不是静默转绿。
run() { local o r; if [ -n "$PATCH_FAIL" ]; then printf '!! 补丁失败（%s）⇒ 这条没有打到码，下面的读数一律不算落点\n' "$PATCH_FAIL"; fi; o="$(cd "$D" && MIDLINE_DOC="${DOC:-$REAL}" cargo test -q 2>&1)"; r="$(printf '%s\n' "$o" | grep -E -- "--- FAILED|test result:")"; if [ -z "$r" ]; then printf '!! cargo 没有产出 test result ⇒ 编译失败（或输出形状变了）——这条**没有落点**，别当"没红"读\n'; printf '%s\n' "$o" | grep -E -- '^error' | head -4; else printf '%s\n' "$r"; fi; printf '%s\n' "$o" | sed -n '/panicked at/,+3p' | head -12; printf '%s\n' "$o" | grep -E -- "^Traceback|AssertionError" | head -3; }
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
n=len(re.findall(r'§七:266(?![0-9])', s))
assert n==3, '§七:266 现有 %d 处（本记录预期 3 处）' % n + ' ⇒ 有人新增或删掉了一处提及：交叉引用也算锚，多出来的那一处会吸收掉这条抹除，整族静默转绿（M33 就曾被 §五 帧新写的「见 §七:265／§七:266」吸收过一次）'
s2=re.sub(r'§七:266(?![0-9])','',s)
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
echo "### M49 只改融合价的一侧（progress.rs 的 -1→-2，ai.rs 不动）⇒ 应红在 ⑤ 逐字同式计数 2→1 ＋ 三条扣费测（§三 入列后实测 **6 红**：既有那 4 条＋§三 复现测（红字落在 md:115「融合 → 消耗业力 = 副牌费用-1」）＋§三 推导器 ③ 的融合价同式 grep——同一条变异多两条红，见其五 状态文档）"
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
import re
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
# 删**整条行尾注释**，不删中间一段：这两行的注释后来被 §五 帧续了「；§五:200 …」，
# 只抠掉 §十二 那一段会把 §五 那一段留成裸代码 ⇒ 编译失败（M54 因此在整族里成了一条没落点的空记录）。
n=len(re.findall(r'§十二:452', s))
assert n==2, 'battle.rs 里 §十二:452 现有 %d 处（本记录预期 2 处）' % n + ' ⇒ 锚点搬家了，本条要按新位置重写'
s2=re.sub(r'[ \t]*//[^\n]*§十二:452[^\n]*','',s)
assert s2!=s, '没抹到 §十二:452'
open(p,'w',encoding='utf8').write(s2)
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
# 判据多带一行（§九 那条「不以全角冒号收尾」的否决）：§五 帧把这台 walk 照了一遍，那 4 行成了两处，
# 单靠前 4 行 replace 会同时打进两章——落点就读不出是谁红的了。§五 的同一降级是 M71。
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
n='            if t.is_empty() || fence {'+chr(10)+'                continue;'+chr(10)+'            }'+chr(10)+'            let nb_text = at(next_non_blank(n)).trim();'+chr(10)+\"            if arity(n) == 1 && !t.ends_with('：')\"
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'            if t.is_empty() {'+chr(10)+'                continue;'+chr(10)+'            }'+chr(10)+'            let nb_text = at(next_non_blank(n)).trim();'+chr(10)+\"            if arity(n) == 1 && !t.ends_with('：')\"))"
run
echo "### M19 收尾：全部复原后整族应全绿"; restore; run
echo "### M58 生产码里抹掉一枚真锚点的冒号（ai.rs 的 §五:210 写成 §五210）⇒ 实测双红：隐形锚点测（model.rs:3680）＋ §五 推导器的认领路（model.rs:3072）——本帧 §五 立了推导器，md:210 那行从此两把尺都看得见，这条的落点由一红变两红"
# 本帧 bug 类的反证，也是那条测存在的理由：同一个补丁打在 HEAD（6bb2725，收成语法之前）**151 全绿**——
# 臂A 实测。少冒号的写法反向覆盖看不见、正向尺当时还替它作证（冒号写成可选），全仓唯一的信号是"绿"。
restore; py "
p='$D/src/ai.rs'; s=open(p,encoding='utf8').read()
assert s.count('§五:210')==1, s.count('§五:210')
open(p,'w',encoding='utf8').write(s.replace('§五:210','§五210'))"
run
echo "### M59 悄悄放宽语法：§ 支的冒号由必需改成可选（正例不受损，只有反例被认进来）⇒ 应红在 语法验收单的反例列"
# 这条撞的是"共用"最隐蔽的失效方式：anchor_at 现在两把尺共用，把冒号放回去，反向覆盖就开始拿
# 「章号紧跟行号」的形状替文档行作证＝虚覆盖。验收单里那两条 format! 拼出来的反例正是为这一刻准备。
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
a='                _ => return None,'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'                _ => j,'))"
run
echo "### M60 语法放过「.」前缀 ⇒ README.md:12 会被当成 中线 第 12 行 ⇒ 应红在 语法验收单（文件坐标那条反例）"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
a=\"let prev_ok = i == 0 || (!cs[i - 1].is_ascii_alphanumeric() && cs[i - 1] != '.');\"
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'let prev_ok = i == 0 || !cs[i - 1].is_ascii_alphanumeric();'))"
run
echo "### M61 § 支砍掉全角冒号（只认半角）⇒ 应红在 语法验收单的正例列（全角那条今天全仓 0 个真锚点，只有表格钉着它）"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
a=\"                Some(&':' | &'：') => j + 1,\"
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,chr(32)*16+\"Some(&':') => j + 1,\"))"
run

# ===== §五 开端·核心起始牌（第十三步·其三）：表体逐字段等值／特性↔围栏对撞／否定式 cell 封闭名单 =====
echo "### M62 抹掉 battle.rs 里 §五:199 那枚锚（放置后场上那 1 张卡）⇒ 应只红在 §五 推导器「19 行全有锚」那层"
# 配比 锚点8／挂债0／实测11 把围栏行按实测优先记，摘掉围栏行的锚它看不见。§十四 的 M39 量过这条路，
# 这里逐章补量一次：双记账那层靠的是各章自己的行名单，一章验过不等于别的章也验过。
restore; py "
import re
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
s2=re.sub(r'\s*//\s*§五:199[^\n]*','',s)
assert s2!=s, s.count('§五:199')
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M63 文档副本 md:200「每关最多2次」→3（md:182 与 §十二:452 都不动）⇒ 应红在 ③ 特性行↔围栏 对撞 ＋ 复现测"
# 按行号打，不按整篇 replace：'（每关最多2次）' 在文档里有两处（md:200 与 §十二:452 那条流程步），
# 整篇替换会顺手把 §十二 那一处也改掉——那就不再是"§五 一处改动"，落点读不出是谁红的。
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert '（每关最多2次）' in ls[199] and '每回合结束获得1业力' in ls[199], ls[199]
ls[199]=ls[199].replace('（每关最多2次）','（每关最多3次）')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M64 文档副本把 md:182 与 md:200 两处上限同步改成 3 ⇒ ③ 平了，应红在 ④ 码面闸与复现测（正证 ④ 不是 ③ 的重复）"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert '（每关上限2次）' in ls[181], ls[181]
assert '（每关最多2次）' in ls[199], ls[199]
ls[181]=ls[181].replace('（每关上限2次）','（每关上限3次）')
ls[199]=ls[199].replace('（每关最多2次）','（每关最多3次）')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M65 文档副本 md:180「数值 1（血量=伤害=1）」两处一起 →2 ⇒ 应红在 ② 逐字段等值（文档内部自洽那条照样绿，正是它该绿的地方）"
restore; py "
p='$D/doc.md'; s=open(p,encoding='utf8').read()
a='数值 1（血量=伤害=1）'
assert s.count(a)==1
open(p,'w',encoding='utf8').write(s.replace(a,'数值 2（血量=伤害=2）'))"
DOC="$D/doc.md" run
echo "### M66 文档副本删掉 md:185 括号那句「每关固定发放」⇒ 应红在 ③ 那句与 md:191 手牌行的连线（❌ 那半截仍绿）"
restore; py "
p='$D/doc.md'; s=open(p,encoding='utf8').read()
a='入继承堆 ❌（每关固定发放）'
assert s.count(a)==1
open(p,'w',encoding='utf8').write(s.replace(a,'入继承堆 ❌'))"
DOC="$D/doc.md" run
echo "### M67 引擎侧把继承堆那行的开端剔除换成恒真 ⇒ 应红在 ⑤ 否定式 cell 的封闭名单 ＋ meta.rs 那条行为测（§六 入列后实测 **5 红**：⑤ 名单＋ai.rs:907＋meta.rs:590＋session 的 EOF 行为测＋§六 复现测——同一条变异的红字数绑定「哪几章有推导器／复现测」这个前提，章数增加时既有读数会漂）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='v.retain(|c| !c.is_starter());'
assert s.count(a)==1
open(p,'w',encoding='utf8').write(s.replace(a,'v.retain(|c| c.hp >= 0);'))"
run
echo "### M68 摘掉 s5_classify 回合末分支的数字个数守卫（expect(2)→expect(nums.len())，文档不动）⇒ 实测全绿：这条是盲区登记"
# 记录形状本身：守卫单独降级、不打文档配合，四把尺量不到——它的牙只在"文档那一行多写第三数"时才出（M69／M70 就是那一对）。
# 与 M33 同一类（抹除被别的层吸收），差别是这条**根本不红**，所以必须留字，别等到哪天把它当成有牙的尺。
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='        expect(2, \"回合末行（每次多少＋上限几次）\");'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'        expect(nums.len(), \"回合末行（每次多少＋上限几次）\");'))"
run
echo "### M69 M68 那层守卫降级 ＋ 文档 md:200 在同一行**追加第三个数**（前缀「每回合结束获得」不动）⇒ 实测全绿 155：第三个数被无声吞掉，fold 仍读 gain=nums[0]／cap=nums[1]——盲区由此从「一句推断」变成「一对读数」"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='        expect(2, \"回合末行（每次多少＋上限几次）\");'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'        expect(nums.len(), \"回合末行（每次多少＋上限几次）\");'))
q='$D/doc.md'; d=open(q,encoding='utf8').read()
b='每回合结束获得1业力（每关最多2次）'
assert d.count(b)==1, d.count(b)
open(q,'w',encoding='utf8').write(d.replace(b,'每回合结束获得1业力（每关最多2次，每次1点）'))"
DOC="$D/doc.md" run
echo "### M70 M69 的那处文档改动、守卫**不**降级 ⇒ 实测红 2 条（复现测＋§五 推导器），落点就是 expect(2) 那句人话「解析出 3 个数字（[1, 2, 1]），回合末行该有 2 个」：与 M69 对照才读出那道守卫的价格——同一处文档改动，有守卫红、没守卫全绿"
restore; py "
q='$D/doc.md'; d=open(q,encoding='utf8').read()
b='每回合结束获得1业力（每关最多2次）'
assert d.count(b)==1, d.count(b)
open(q,'w',encoding='utf8').write(d.replace(b,'每回合结束获得1业力（每关最多2次，每次1点）'))"
DOC="$D/doc.md" run
echo "### M71 §五 那台推导器的围栏内不判标签（M57 的姊妹条）⇒ 实测红在哪一层是量出来的：rows 与标签名单一起长，围栏那 15 行不再只由解析器给"
restore; py "
p='$D/src/model.rs'; s=open(p,encoding='utf8').read()
n='            if t.is_empty() || fence {'+chr(10)+'                continue;'+chr(10)+'            }'+chr(10)+'            let nb_text = at(next_non_blank(n)).trim();'+chr(10)+'            if arity(n) == 1 && (nb_text == '
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n,'            if t.is_empty() {'+chr(10)+'                continue;'+chr(10)+'            }'+chr(10)+'            let nb_text = at(next_non_blank(n)).trim();'+chr(10)+'            if arity(n) == 1 && (nb_text == '))"
run

# ===== §六 开局手牌与双牌堆（第十三步·其四）：双记账那层／张数三处对账／文档数字↔码面字面／封闭写点名单／分类 panic 的牙 =====
echo "### M72 抹掉 battle.rs 里 §六:243 那枚锚（手牌满额 FIFO 的落点注释）⇒ 应只红在 §六 推导器「12 行全有锚」那层"
# 配比 锚点0／挂债0／实测12 把 12 行全记成实测，摘锚它看不见。本章是第一条实测全走的章，双记账那层有没有牙只能这样量一次
# ——§五 M62／§十四 M39 各量过自己章的那一份，一章验过不等于别的章也验过。
restore; py "
import re
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
assert s.count('§六:243')==1, s.count('§六:243')
s2=re.sub(r'    /// §六:243[^\n]*\n','',s)
assert s2!=s, '正则没命中那行注释'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M73 文档副本只改 md:220 那张表（3张→4张，md:223／md:224 不动）⇒ 应红在 ③ 那句「三处张数不齐」＋复现测"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[219].startswith('继承堆抽牌 3张'), ls[219]
ls[219]=ls[219].replace('继承堆抽牌 3张','继承堆抽牌 4张')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M74 文档副本把张数家族四处（md:220／223／224／225）一起改到 4 ⇒ ③ 的文档内部对账全平，应红在码面 grep（开局发牌那个循环的字面「for _ in 0..4」实测 0 处）＋复现测：正证码面那一层不是文档自洽的重复"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[219].startswith('继承堆抽牌 3张') and '抽3张' in ls[222] and '抽3张' in ls[223] and '不足3张' in ls[224], (ls[219], ls[222], ls[223], ls[224])
ls[219]=ls[219].replace('继承堆抽牌 3张','继承堆抽牌 4张')
ls[222]=ls[222].replace('抽3张','抽4张'); ls[223]=ls[223].replace('抽3张','抽4张'); ls[224]=ls[224].replace('不足3张','不足4张')
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M75 文档副本把 md:226 换成一句不含任何已登记内容的话（原地换字、行号不漂移）⇒ 落点应是 s6_classify 那句 panic「登记过的八种内容一种都不匹配」：文档往围栏里加第五种写法时，实测路必须当场喊「这行我没读法」，不能安静走过去"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert '不计入' in ls[225], ls[225]
ls[225]='开局手牌由系统直接给出'
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M76 生产码面新写一条文档里根本没有的主动额度扣减（dead_code 方法，行为不变）⇒ 应只红在 ④ 封闭写点名单 6→7（M41 的姊妹条）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
n='impl Battle {'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n, n+chr(10)+'    #[allow(dead_code)]'+chr(10)+'    fn s6_probe_charge(&mut self) { self.pf.manual_draws -= 1; }'))"
run
echo "### M77 把开端堆那道闸从 <=0 挪到 <0（每回合能多抽一次，真行为回归）⇒ 应红在 ③ 码面 grep（「starter_draws <= 0」实测 0 处）＋复现测⑤ 那条 Err 断言"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='if self.pf.starter_draws <= 0 {'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'if self.pf.starter_draws < 0 {'))"
run
echo "### M78 摘掉 s6_table_row 表2 继承堆行的数字个数守卫（文档不动）⇒ 读数：这条**预期全绿**，是 §六 自己的 M68——守卫单独降级、四把尺都看不见，它的牙只在文档那一格多写第四个数时才出（M79 就是那一条）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='            if nums.len() != 3 {'+chr(10)+'                expect(3, \"表2 继承堆行（可抽几次＋自动几次＋可选几次）\");'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'            if false {'+chr(10)+'                expect(3, \"表2 继承堆行（可抽几次＋自动几次＋可选几次）\");'))"
run
echo "### M79 M78 那层守卫降级 ＋ 文档 md:232 同一格追加第四个数（「牌库上限10」，arity 仍 3）⇒ 读数：只剩 §六 推导器 ② 那份独立计数在喊 ⇒ 两处各数一遍不是重复，一处降级另一处仍红"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='            if nums.len() != 3 {'+chr(10)+'                expect(3, \"表2 继承堆行（可抽几次＋自动几次＋可选几次）\");'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'            if false {'+chr(10)+'                expect(3, \"表2 继承堆行（可抽几次＋自动几次＋可选几次）\");'))
q='$D/doc.md'; ls=open(q,encoding='utf8').read().split('\n')
b='（1自动+2可选）'
assert ls[231].endswith(b), ls[231]
ls[231]=ls[231][:-1]+'，牌库上限10）'
open(q,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run

# ===== §三 业力系统（第十三步·其五）：双记账那层／档位数字↔码面字面／互斥与闸的**执行**／封闭业力写点名单／标签判据那道「不带数字」／分类守卫降级＋独立计数 =====
echo "### M80 抹掉 battle.rs 里 §三:83 那枚锚（表2「第二次 50%」那档的行尾注释）⇒ 应只红在 §三 推导器「31 行全有锚」那层（双记账）"
# 配比 锚点0／挂债0／实测31 把整章记成实测，摘锚它看不见。M72 量的是 §六 那一份，一章验过不等于别的章也验过——
# 这条与 M72 的差别只在章号与行数：文档没动，所以复现测照样绿（引擎返的仍是码面那个 50）。
restore; py "
import re
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
assert s.count('§三:83')==1, s.count('§三:83')
s2=re.sub(r'  // §三:83[^\n]*','',s)
assert s2!=s, '正则没命中那行注释'
open(p,'w',encoding='utf8').write(s2)"
run
echo "### M81 文档副本只改 md:83 那一档（第二次 50%→40%，其余三档不动）⇒ 实测 2 红：③ 码面 grep（「1 => 40,」0 处 @ model.rs:3805）＋复现测。复现测那条红在**示例↔表2 的文档内部对账**（battle.rs:3642「md:89 那行说第 2 次返 50%，表2 同一档却写着 40」），比拿档位去驱动引擎那一句更早。② 那层的数字个数仍是 1，拦不住换了值的数"
restore; py "
p='$D/doc.md'; ls=open(p,encoding='utf8').read().split('\n')
assert ls[82]=='第二次 50%', ls[82]
ls[82]='第二次 40%'
open(p,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M82 把献祭那一支从裸费用改成乘**当前**档位（摘掉 md:75「全额，不递减」的机器形式；档位推进仍留着不碰）⇒ 实测 3 红：复现测④ 第一段（battle.rs:3728，档位已掉到 10% 的那张牌献祭仍该拿全额 5，引擎给 0）＋两条**既有**献祭行为测（battle.rs:3097／4036）。本章的牙不是新长的，是把既有盲区接上账（§四 入列后实测 **4 红**：多出来那条是 §四 复现测 @battle.rs:4320——两章共读 `on_death` 里同一支「全额」match 臂，所以这一改现在两侧都喊；行号是当时的快照，按测名读）"
# 这条特意不写成 `c.def.cost * pct / 100`——那个式子是 ③ 那份「恰有 2 处」的 needle（另一处是终影吞名，出处速查:985），
# 写成它会把「互斥被摘」报成「有人硬编了同一个式子」，读数就分不清是哪一句话坏了。
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='                    c.def.cost  // §三:75'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'                    c.def.cost * refund_pct(c.deaths) / 100  // §三:75'))"
run
echo "### M83 摘掉同名牌那道闸（条件写成 false，报错文案、名单维护与回合开始清空都留着）⇒ 读数：这条只红在复现测⑤——③ 咬的是那句「本回合献祭过同名」还在不在，不是闸到底执行没执行（§四 入列后实测 **2 红**：§三 复现测 @battle.rs:4146 ＋ §四 复现测 @battle.rs:4406，两章各撞一遍这条行为；§四 推导器**没喊**——本章 ③ 那批 grep 钉的是三道闸的**形状**，同名牌那条的文案没被摘、只是不执行，与待办 (e)「字面串可绕」同族，照登不改）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='if self.pf.sacrificed_names.contains(&self.hand[hand_idx].def.name) {'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'if false {'))"
run
echo "### M84 保底补开端多触发一次（把那个调用点复制一行）⇒ 应红在 ③「self.grant_free_starter(); 实测 1 处」＋复现测⑦ 那条手牌张数（每回合最多1次 ⇒ 一次给 2 张）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='            self.grant_free_starter();'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a, a+chr(10)+a))"
run
echo "### M85 生产码面新写一条文档里根本没有的业力增补（dead_code 方法，行为不变）⇒ 应只红在 §三 ④ 那份封闭业力写点名单 12→13（M76 的姊妹条，换个章各量一遍）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
n='impl Battle {'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n, n+chr(10)+'    #[allow(dead_code)]'+chr(10)+'    fn s3_probe_charge(&mut self) { self.p_karma += 1; }'))"
run
echo "### M86 让那张免费补牌吃掉开端堆额度（在 grant_free_starter 开头扣一次）⇒ 实测 2 红：§三 ④「体内零额度写点」名单由空变非空（model.rs:3841）＋复现测⑦ 那句「不消耗每回合抽牌次数」（battle.rs:3821，额度 (2,0)→(2,-1)）。原设计预测的第三把尺（§六 ④）**没有喊**：那份封闭名单只管 manual_draws，starter_draws 的写点全章只有 §三 ④ 在点名——这条红字里那句「与 §六:226 一起落空」是口径连带，不是另一条测"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='        if self.starter_pile > 0 {'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'        self.pf.starter_draws -= 1;'+chr(10)+a))"
run
echo "### M87 摘掉标签判据里那道「不带数字」（M71 的姊妹条）⇒ §三 多这一道是因为 md:128「若玩家手牌为0且场上无卡牌：」那句头里那个 0 **就是触发条件**：吞成标签等于把这条规则从实测里删掉。实测 2 红：复现测红在解析器自己那句「缺 md:128 这一行」（battle.rs:2967，走不到那两组「非空不补」的断言）＋推导器红在 ① 的围栏行集对账（model.rs:3660，第六道围栏 5 行变 4 行）——不是预测的那条 assert_ne。§四 帧起 needle 带上「return S3Claim::Label;」那一行：「s4_classify」抄了同一条判据，单行 needle 变成 2 处（预检当场喊出来的，不是人读出来的）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a=\"    if t.ends_with('：') && s15_digits(t).is_empty() {\"+chr(10)+'        return S3Claim::Label;'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,\"    if t.ends_with('：') {\"+chr(10)+'        return S3Claim::Label;'))"
run
echo "### M88 摘掉「初始业力那格该有 2 个数字」那道分类守卫 ＋ 文档 md:64 同一格追加第三个数（键名与 arity 都不动）⇒ 实测 1 红：只剩 ② 那份**独立**计数在喊（model.rs:3771，红字直接报出实测 [0, 0, 3] 对登记口径 2）——M78＋M79 那对成对姊妹条的 §三 版本，两处各数一遍不是重复，一处降级另一处仍红"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='            if nums.len() != 2 {'+chr(10)+'                expect(2, \"初始业力那格（我方起点＋普通关敌方起点）\");'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'            if false {'+chr(10)+'                expect(2, \"初始业力那格（我方起点＋普通关敌方起点）\");'))
q='$D/doc.md'; ls=open(q,encoding='utf8').read().split('\n')
b='开场脚本预算）'
assert ls[63].endswith(b), ls[63]
ls[63]=ls[63][:-1]+' 3）'
open(q,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M89 只摘「初始业力那格该有 2 个数字」那道分类守卫、文档不动 ⇒ **预期全绿**：§三 自己的 M68／M78——守卫单独降级、四把尺都看不见，它的牙只在文档那一格多写第三个数时才出（M88 就是那一条）。这一半不测就只是推断，所以单独立一条"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='            if nums.len() != 2 {'+chr(10)+'                expect(2, \"初始业力那格（我方起点＋普通关敌方起点）\");'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'            if false {'+chr(10)+'                expect(2, \"初始业力那格（我方起点＋普通关敌方起点）\");'))"
run
echo "### M90 抹掉 battle.rs 里 §四:154 那枚行尾锚（md:154 走的是实测路，锚是双记账那一半）⇒ 应只红在 §四 推导器 ②「14 行全指回」那层，配比 0／0／14 不动（M47／M80 的姊妹条，换个章各量一遍）"
restore; py "
import re,glob
hits=0
for p in glob.glob('$D/src/*.rs'):
    s=open(p,encoding='utf8').read()
    s2,n=re.subn(r'\s*//\s*§四:154[^\n]*','',s)
    hits+=n
    if n: open(p,'w',encoding='utf8').write(s2)
assert hits==1, hits"
run
echo "### M91 文档副本把 md:152 行首那个「获得2业力」→3（括号里那个「死亡获2业力」不动）⇒ 应红在复现测① 那句「两处说的是同一笔钱」＋推导器 ③ 那个 door pin（gain=3≠2）；①′ 那份普查仍绿（数字个数没变，只有值变了）——arity 拦不住换值，这条与 M81 同形"
restore; py "
q='$D/doc.md'; ls=open(q,encoding='utf8').read().split('\n')
assert ls[151].count('获得2业力')==1, ls[151]
ls[151]=ls[151].replace('获得2业力','获得3业力',1)
open(q,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M92 把在场闸那道「< 1」改成「< 0」（我方＋敌方两支场上入口一起，真行为回归：入场 0 回合也能献）⇒ 应红在复现测③ 那条边界扫描＋推导器 ③ 那句「码面恰有 2 处」实测 0 处"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='if self.turn - c.placed_turn < 1 {'
assert s.count(a)==2, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'if self.turn - c.placed_turn < 0 {'))"
run
echo "### M93 摘掉开端那半道豁免（两支手牌入口的「if !is_starter {」一起改成「if true {」：开端也吃每回合1次额度）⇒ 实测 3 红：复现测⑤（battle.rs:4336「献完开端那位仍是 false」）＋**一条既有**手牌额度测 hand_sacrifice_quota_only_starter_exempt（battle.rs:4634）＋推导器 ③ 那句「码面恰有 2 处」实测 0 处（model.rs:4151）。预测只列了后两条，多出来的那条正向测与其五 §3.5 的 M82 同形——本章的牙把既有盲区接上账，不是新造；④ 那份名单确实不动（置位行数没变，变的是它被谁执行）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='        if !is_starter {'
assert s.count(a)==2, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'        if true {'))"
run
echo "### M94 往玩家手牌献祭入口塞一道在场闸（md:153 那条否定式被摘：手牌献开端从此要看 placed_turn）⇒ 实测 **8 红**（预测 3 红，差额全是这条塞进去的闸在**行为侧**炸出的连锁）：7 条测死在同一处减法溢出 battle.rs:550:12（「self.turn - c.placed_turn」在 placed_turn=i64::MIN 上减穿下界），死者含 §三／§五 两章的复现测、既有手牌额度测，以及 boss／command／meta 三把**与本章无关**的既有尺；第 8 条是 §四 推导器，红在 ③ 那句闸计数「恰有 2 处」实测 3（model.rs:4145）——**④ 那份「体内零在场闸」的点名没读到**：同一条测里 assert 是串行的，③ 先炸就把④ 遮在后面（与 M95 合起来读出这条口径：红字数数的是**测**，不是层）。溢出这一形把池里 placed-turn-min-overflow 那个坑从**实测侧**也照了出来"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='        let c = self.hand.remove(idx);'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a, a+chr(10)+'        if self.turn - c.placed_turn < 1 { return Err(\"在场不足1回合，不可献祭\".into()); }'))"
run
echo "### M95 生产码面新写一条文档里根本没有的献祭额度消耗（dead_code 方法，行为不变）⇒ 实测 **1 红**（预测 2 红）：推导器红在 ③ 那句「self.pf.sacrifice_used = true; 恰有 2 处」实测 3（model.rs:4157），而 ④ 那份封闭写点名单 14→15 **没被读到**——两层在同一条测里，assert 串行，先炸的那层就是这次的全部读数。M85 的姊妹条，但落点比它少一层：M85 那次 ③ 的 needle 恰好不覆盖新写点，所以 ④ 是唯一喊的人；这里两处都覆盖，③ 抢先。合起来（与 M94）读出的口径是：**红字数测不数层**，「两层的红字互相把对方指出来」这句话在单条测内不成立"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
n='impl Battle {'
assert s.count(n)==1, s.count(n)
open(p,'w',encoding='utf8').write(s.replace(n, n+chr(10)+'    #[allow(dead_code)]'+chr(10)+'    fn s4_probe_sac(&mut self) { self.pf.sacrifice_used = true; }'))"
run
echo "### M96 摘掉 s4_table_row「在场限制那格该有 1 个数字」那道 arity 守卫、文档不动 ⇒ **预期全绿**（M68／M78／M89 的姊妹条）：守卫单独降级，四把尺都看不见，它的牙只在文档那一格多写一个数时才出（M97 就是那一条）"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='            if nums.len() != 1 {'+chr(10)+'                expect(1, \"在场限制那格（「需在场N回合以上」那一个数）\");'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'            if false {'+chr(10)+'                expect(1, \"在场限制那格（「需在场N回合以上」那一个数）\");'))"
run
echo "### M97 M96 那层守卫降级 ＋ 文档 md:161 同一格追加第二个数（「（牌库上限10）」，键名与行号都不动）⇒ 实测只剩 §四 推导器 ①′ 那份**独立**普查在喊（该 1 个、实测 2）：fold 仍取 nums[0]=1，复现测一路绿——与 M96 合起来才读出 ①′ 这一层的价钱：M78／M79 那对读数在 §四 的重做"
restore; py "
p='$D/src/battle.rs'; s=open(p,encoding='utf8').read()
a='            if nums.len() != 1 {'+chr(10)+'                expect(1, \"在场限制那格（「需在场N回合以上」那一个数）\");'
assert s.count(a)==1, s.count(a)
open(p,'w',encoding='utf8').write(s.replace(a,'            if false {'+chr(10)+'                expect(1, \"在场限制那格（「需在场N回合以上」那一个数）\");'))
q='$D/doc.md'; ls=open(q,encoding='utf8').read().split('\n')
b='手牌献祭不受此限制'
assert ls[160].endswith(b), ls[160]
ls[160]=ls[160]+'（牌库上限10）'
open(q,'w',encoding='utf8').write('\n'.join(ls))"
DOC="$D/doc.md" run
echo "### M98 只摘掉 §四 复现测上方的 #[test]（函数体一字不动）⇒ 应红在推导器 ⑤（正证：定义还在≠测还在，161 项会掉成 160；M44 的姊妹条，§十四 之外再量一次那把尺本身）"
restore; py "
p='$D/src/battle.rs'; ls=open(p,encoding='utf8').read().split('\n')
i=[k for k,l in enumerate(ls) if l.strip().startswith('fn section4_sacrifice_rules_reproduce_on_the_engine(')]
assert len(i)==1, i
assert ls[i[0]-1].strip()=='#[test]', repr(ls[i[0]-1])
ls[i[0]-1]='// 变异试验：把 #[test] 摘掉'
open(p,'w',encoding='utf8').write('\n'.join(ls))"
run
