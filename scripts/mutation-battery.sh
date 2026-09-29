#!/usr/bin/env bash
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
echo "### M14 收尾：全部复原后整族应全绿"; restore; run
