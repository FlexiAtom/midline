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
# 电池预检：照跑整族、只把 run() 换成不跑 cargo 的空壳——于是每条变异的**补丁真打上没有**在半分钟里量完。
# 立这条尺的由头是 §九 帧撞上的那条静默假绿：M46 的 `assert s.count(n)==1` 因为 §九 推导器照抄了同一行守卫
# 而变成 2 ⇒ python 抛 AssertionError、补丁根本没打上，而那次整族日志里 cargo 照旧全绿，只多一行 traceback
# （这一族记录从没有程序查过它）。`bash -n` 验得到 shell、验不到内嵌 Python，所以判据不能靠人读日志。
# 落点仍在原仓库里：电池用 `$(dirname "$0")/..` 认 src，副本必须待在 scripts/ 下才解析得对
# （写成 /tmp 里的副本 ⇒ SRC 指空，cp //Cargo.toml 当场失败）。产物带 PID 且退出即删，故 .gitignore 收了一条。
# 用法：bash scripts/battery-preflight.sh
set -u
B="$(cd "$(dirname "$0")" && pwd)/mutation-battery.sh"
[ -f "$B" ] || { echo "找不到电池：$B"; exit 1; }
REAL="${MIDLINE_REAL_DOC:-$HOME/中线.MD}"
[ -f "$REAL" ] || { echo "找不到规格文档 $REAL（可用 MIDLINE_REAL_DOC 指定）"; exit 1; }

# 判据唯一化：电池里那六个"跑 cargo／跑 python 检／跑 clang"的入口，定义行必须各恰有一处，否则这条尺的替换就是空替换——
# 静默预检出"全过"最坑。run() 是引擎侧 167 条的落点；srun/crun/frun/irun/grun 是壳侧五条 lane 的落点
# （M167 走 host 探针、M168 走守门检 §8 那份谓词、M169 走 frame:: 测面、M170–M171 走 input:: 测面、M172 走 NDK clang 单编），
# 它们各自真跑一次都是**几十秒到几分钟**的编译，预检要量的是"补丁有没有打上"，所以六个全换空壳——少换一个，预检就退化成一次整族。
for f in run srun crun frun irun grun; do
    n=$(grep -c "^$f() {" "$B")
    [ "$n" -eq 1 ] || { echo "电池里 $f() 的定义行应有且只有一处，实测 $n 处 ⇒ 本预检的替换判据已失效，请同步本脚本"; exit 1; }
done

T="$(dirname "$B")/.battery-preflight.$$.sh"
trap 'rm -f "$T"' EXIT
python3 - "$B" "$T" <<'PY'
import sys
src, dst = sys.argv[1], sys.argv[2]
# 空壳只做一件事：把 py()/pys() 记下的 PATCH_FAIL 喊出来，然后清掉（一条记录只喊一次）。
NEUTRAL = ('run', 'srun', 'crun', 'frun', 'irun', 'grun')
def noop(name):
    return ('%s() { if [ -n "${PATCH_FAIL:-}" ]; then '
            "printf '!! 补丁失败（%%s）⇒ 这条没有打到码\\n' \"$PATCH_FAIL\"; PATCH_FAIL=\"\"; fi; return 0; }" % name)
lines = open(src, encoding='utf8').read().split('\n')
out = []
for l in lines:
    hit = next((f for f in NEUTRAL if l.startswith(f + '() {')), None)
    out.append(noop(hit) if hit else l)
for f in NEUTRAL:
    assert sum(1 for l in out if l.startswith(f + '() {')) == 1, f
open(dst, 'w', encoding='utf8').write('\n'.join(out))
PY

echo "### 预检（只打补丁、不跑 cargo／不跑检）｜红＝某条变异的补丁没打上或内嵌 Python 有语法错"
bash "$T"
# 记录条数钉死（本帧 D19）：以前这行只打印条数、没人核对，一条记录静默丢失（echo 头被误删、或整条误并）
# 就会让"预检全过"变成假绿。现在断言 == 期望数；加一条牙就同步改 EXPECTED_RECORDS 这一处。
# 174 ＝ M0 引擎对照 + M0b 壳侧对照 + M1…M172（其中 M167–M172 走上面那五个壳侧入口）。
EXPECTED_RECORDS=174
N="$(grep -c '^echo "### ' "$B")"
if [ "$N" -ne "$EXPECTED_RECORDS" ]; then
    printf '!! 记录条数对不上：期望 %s 条，实测 %s 条 ⇒ 有 echo "### " 头被删／多写／整条误并，请核对电池\n' "$EXPECTED_RECORDS" "$N"
    exit 1
fi
# 末条抬头直接取文件里最后一行，不做 EXPECTED_RECORDS-1 那种算术——M0b 插进来之后那种算术就已经是错的了。
LAST="$(grep '^echo "### ' "$B" | tail -1 | sed 's/^echo "### \([A-Za-z0-9]*\).*/\1/')"
echo "### 预检结束｜记录共 $N 条（含两条对照，末条 $LAST，与 EXPECTED_RECORDS 相符）。上面没有「补丁失败」行＝逐条补丁都真打到了码。"

