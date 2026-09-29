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
run() { (cd "$D" && MIDLINE_DOC="${DOC:-$REAL}" cargo test -q 2>&1 | grep -E -- "--- FAILED|panicked at|test result:" | head -4); }
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
echo "### M6 收尾：全部复原后整族应全绿"; restore; run
