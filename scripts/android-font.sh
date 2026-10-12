#!/usr/bin/env bash
# midline - 《中线》安卓字体子集（码点现抽 → pyftsubset 两档 → 落 assets/）
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
#
# 子集**在打包时生成、不进 git**，两条理由大小不一但都够硬：
# ① 261 KB 的二进制进仓，每改一次码点就多一份历史；
# ② 更要紧的是清单会**过期**——仓里的字每天在变，一份写死的 `--unicodes=` 清单会在三个月后
#    少掉当天新加的那个字，而那种缺字在屏上只是"少一笔"，没人报修。所以清单的唯一来源是**源码本身**。
#
# 口径三条（逐条有牙）：
# ① 扫描面 = `src/*.rs` + `tests/*.rs` + `shell-android/src/*.rs`；抽的是**文件字节里出现过的
#    每一个可打印非 ASCII 字符**，不是"只取字符串字面量里的"。AST 级抽取要引语法解析器，
#    而多带注释里那几个码点的代价是几百字节，远小于漏一个字的风险。
# ② 三档各做一次：**留布局表**／**丢 GSUB·GPOS·GDEF**／**丢表＋展开 CFF 子程序**。`ab_glyph` 只读
#    cmap＋轮廓＋hmtx，用不到替换／字距特性，所以进包的是最后一档；三档字节数一起打出来，
#    §3.8.5 那笔推算就地作废。
# ③ 做完**回读子集的 cmap** 逐字对一遍需求码点。漏了当场红，而不是等真机上少一笔才发现。
#
# OFL 要求分发时随带许可证文本，所以产物旁边落一份 LICENSE（源字体来自系统包，不是仓里的东西）。

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_FONT="${MIDLINE_FONT_SRC:-/usr/share/fonts/adobe-source-han-sans/SourceHanSansCN-Regular.otf}"
SRC_LICENSE="${MIDLINE_FONT_LICENSE:-/usr/share/licenses/adobe-source-han-sans-cn-fonts/LICENSE.txt}"
ASSETS="${MIDLINE_ASSETS:-$ROOT/build/assets}"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

die() { echo "android-font: $*" >&2; exit 1; }

command -v pyftsubset >/dev/null || die "找不到 pyftsubset（fontTools 的 CLI）。子集这一格没有替代通路：
整份 8,429,224 B 的源字体直接进包，等于把 APK 撑到 MB 级。"
[ -f "$SRC_FONT" ] || die "找不到源字体 $SRC_FONT（可用 MIDLINE_FONT_SRC 指路）"

mkdir -p "$ASSETS/fonts"

say() { echo "== $*"; }
ok() { echo "   ✔ $*"; }

say "1. 源字体"
src_bytes=$(stat -c%s "$SRC_FONT")
ok "$SRC_FONT = $src_bytes B"

say "2. 现抽码点（扫描面：src/*.rs + tests/*.rs + shell-android/src/*.rs）"
python3 - "$ROOT" "$STAGE/chars.txt" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
files = sorted((root / "src").glob("*.rs")) + sorted((root / "tests").glob("*.rs")) \
      + sorted((root / "shell-android" / "src").glob("*.rs"))
seen = set()
for f in files:
    for ch in f.read_text(encoding="utf-8", errors="replace"):
        if ord(ch) > 0x7F and ch.isprintable():
            seen.add(ch)
chars = "".join(sorted(seen))
pathlib.Path(sys.argv[2]).write_text(chars, encoding="utf-8")
print(f"   {len(files)} 个文件 → 非 ASCII 码点 {len(seen)} 个（另加 ASCII U+0020–U+007E）")
PY
n_cjk=$(python3 -c 'import sys;print(len(set(open(sys.argv[1],encoding="utf-8").read())))' "$STAGE/chars.txt")
[ "$n_cjk" -gt 0 ] || die "一个非 ASCII 码点都没抽到——扫描面或 ROOT 不对，别把空子集当'这仓没中文'。"

say "3. 三档子集"
# 注意 `--unicodes` 只收裸十六进制区间：写成 `U+0020-U+007E` 会被 fontTools 的正则把 `+` 换成空格，
# 然后在 int(...,16) 上炸（本机 4.66.1 现测 ValueError）。
COMMON=(--text-file="$STAGE/chars.txt" --unicodes=20-7E)
pyftsubset "$SRC_FONT" "${COMMON[@]}" --output-file="$STAGE/keep-layout.otf"
pyftsubset "$SRC_FONT" "${COMMON[@]}" --drop-tables+=GSUB,GPOS,GDEF --output-file="$STAGE/no-layout.otf"
pyftsubset "$SRC_FONT" "${COMMON[@]}" --drop-tables+=GSUB,GPOS,GDEF --desubroutinize --output-file="$STAGE/shipped.otf"
keep=$(stat -c%s "$STAGE/keep-layout.otf")
drop=$(stat -c%s "$STAGE/no-layout.otf")
ship=$(stat -c%s "$STAGE/shipped.otf")
ok "留 GSUB/GPOS/GDEF（默认档）= $keep B"
ok "丢 GSUB/GPOS/GDEF = $drop B（省 $((keep - drop)) B；丢表顺带把只做闭包补进来的那些字形一起带走）"
ok "丢表＋--desubroutinize = $ship B（再省 $((drop - ship)) B）⇒ **进包走这一档**"
say "   选它的两条理由都实测过：① 本字符集上展开子程序反而更小（子程序调用簿记比省下的重复轮廓贵）；"
say "   ② 源字体骨架是 OTTO＝CFF／Type 2 栈机（§3.6），展开后栅格侧不必再解嵌套子程序——少一条只在这里成立的失败通路。"
[ "$ship" -le "$keep" ] || die "产物比默认档还大——口径变了，别继续往下走。"

cp "$STAGE/shipped.otf" "$ASSETS/fonts/ui.otf"
if [ -f "$SRC_LICENSE" ]; then
  cp "$SRC_LICENSE" "$ASSETS/fonts/LICENSE-OFL.txt"
  ok "许可证随包：$ASSETS/fonts/LICENSE-OFL.txt（$(stat -c%s "$ASSETS/fonts/LICENSE-OFL.txt") B）"
else
  die "找不到 OFL 许可证文本 $SRC_LICENSE ⇒ OFL 要求分发时随带许可证，缺它就不能把这档字体发出去。"
fi

say "4. 回读 cmap，把'子集漏了'与'源字体本来就没这个字'分开"
/usr/bin/python - "$SRC_FONT" "$ASSETS/fonts/ui.otf" "$STAGE/chars.txt" <<'PY' > "$STAGE/cover.log"
import sys
from fontTools.ttLib import TTFont
want = set(open(sys.argv[3], encoding="utf-8").read()) | {chr(c) for c in range(0x20, 0x7F)}
# getBestCmap() 的键是 **int 码点**，want 是字符集；直接取交集会永远得 0（第一版就是这么绿的）。
src = {chr(c) for c in TTFont(sys.argv[1]).getBestCmap()}
sub = {chr(c) for c in TTFont(sys.argv[2]).getBestCmap()}
lost = sorted(want & src - sub)          # 子集器丢的 ⇒ 硬失败
absent = sorted(want - src)              # 源字体没有 ⇒ 只报数（注释里的箭头、带圈数字这类）
print(f"COVER 需求={len(want)} 源={len(want & src)} 子集={len(want & sub)} 丢={len(lost)} 源无={len(absent)}")
if lost:
    print("LOST " + " ".join(f"U+{ord(c):04X}({c})" for c in lost[:20]))
if absent:
    print("ABSENT " + " ".join(f"U+{ord(c):04X}({c})" for c in absent[:20]))
PY
sed 's/^/   /' "$STAGE/cover.log"
grep -q '丢=0' "$STAGE/cover.log" || { echo "   ↑ 子集缺字（LOST 那行）。这是硬失败：漏的那个字上屏只会是空白，而'空白'没人报修。" >&2; exit 1; }
# 这道闸自己也要有牙：'丢=0' 在**两边键型不一致**时会恒成立（第一版就是这样：want 是字符、
# getBestCmap 的键是 int，交集永远空）。所以再钉一次"交集非空"。
nsrc=$(sed -n 's/.*源=\([0-9]\+\).*/\1/p' "$STAGE/cover.log")
[ "${nsrc:-0}" -gt 100 ] || die "回读交集只有 ${nsrc:-空} 个码点 ⇒ 这道比对没在做，别把恒等的 0 读成通过。"
grep -q '源无=0' "$STAGE/cover.log" || echo "   注：ABSENT 那些码点源字体里就没有（多在注释与文档符号里），子集器按 --ignore-missing-unicodes 默认丢掉。渲染用的字面量若落进这一列，得换字体，不是换子集参数。"

say "5. 产物"
out_bytes=$(stat -c%s "$ASSETS/fonts/ui.otf")
ok "$ASSETS/fonts/ui.otf = $out_bytes B（源字体的 $(awk -v a="$out_bytes" -v b="$src_bytes" 'BEGIN{printf "%.2f%%", a*100/b}')）"
say "  下一步：bash scripts/android-apk.sh（它把 $ASSETS 交给 aapt2 link -A），zip 条目实价在那之后现量。"
