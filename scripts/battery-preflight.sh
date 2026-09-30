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

# 判据唯一化：电池里 run() 的定义行必须恰有一处，否则这条尺的替换就是空替换——静默预检出"全过"最坑。
n=$(grep -c '^run() {' "$B")
[ "$n" -eq 1 ] || { echo "电池里 run() 的定义行应有且只有一处，实测 $n 处 ⇒ 本预检的替换判据已失效，请同步本脚本"; exit 1; }

T="$(dirname "$B")/.battery-preflight.$$.sh"
trap 'rm -f "$T"' EXIT
python3 - "$B" "$T" <<'PY'
import sys
src, dst = sys.argv[1], sys.argv[2]
# 空壳 run() 只做一件事：把 py() 记下的 PATCH_FAIL 喊出来，然后清掉（一条记录只喊一次）。
noop = ('run() { if [ -n "${PATCH_FAIL:-}" ]; then '
        "printf '!! 补丁失败（%s）⇒ 这条没有打到码\\n' \"$PATCH_FAIL\"; PATCH_FAIL=\"\"; fi; return 0; }")
lines = open(src, encoding='utf8').read().split('\n')
out = [noop if l.startswith('run() {') else l for l in lines]
assert sum(1 for l in out if l.startswith('run() {')) == 1
open(dst, 'w', encoding='utf8').write('\n'.join(out))
PY

echo "### 预检（只打补丁、不跑 cargo）｜红＝某条变异的补丁没打上或内嵌 Python 有语法错"
bash "$T"
echo "### 预检结束｜记录共 $(grep -c '^echo "### ' "$B") 条（含 M0 对照与 M19 收尾）。上面没有「补丁失败」行＝逐条补丁都真打到了码。"
