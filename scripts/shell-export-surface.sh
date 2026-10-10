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
# 壳的导出面守门检（android-rust-shell-fused-source §6 的 P7，本文件是它的落地）。
#
# 为什么必须是第二条检：主仓那条机检 `this_program_ships_no_reusable_api_surface` 扫的是
# main.rs 的 mod 表 + `src/*.rs` 两样，`shell-android/` 整目录在它的盲区里。所以
# **「主仓绿」证明不了「壳没拓宽」**——这条检守的是那条盲区。它按符号表量，不按源码文本猜：
# 导出面是什么，看链接器实际留下了什么。
#
# 用法：bash scripts/shell-export-surface.sh
# 退出码：0＝六格全过；非 0＝任一格红（并打印红在哪格与当时的读数）。
set -u
cd "$(dirname "$0")/.." || exit 1
SHELL_DIR=shell-android
NDK="${ANDROID_NDK_HOME:-/opt/android-ndk}"
CLANG="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang"
SYSROOT="$NDK/toolchains/llvm/prebuilt/linux-x86_64/sysroot"
TARGET=aarch64-linux-android

# 白名单＝平台强制的入口符号，一个不多。多出来的任何一条都是"壳自己开的对外把手"，
# 那正是防绕过立场要挡的东西；少一条（入口没导出）则 APK 根本起不来。
ALLOWED='ANativeActivity_onCreate'

fail=0
say() { printf '%s\n' "$*"; }
red() { say "  [红] $*"; fail=1; }
ok()  { say "  [过] $*"; }

say "== 1. 构建（host ＋ $TARGET）=="
# 先 clean 再编，因为第 4 格数的是 `target/` 里的文件，而 cargo **增量构建不删旧产物**：
# 上一轮拿"临时给 crate-type 加 rlib"验这一格的牙，那 4 个 .rlib 留在目录里，此后 cdylib-only
# 的配置照样被读出 .rlib=4——那时检的是目录的历史，不是本次构建。壳零第三方依赖，整目录重编约十秒。
if ! (cd "$SHELL_DIR" && cargo clean --quiet) >/tmp/shell-guard-clean.log 2>&1; then
  red "cargo clean 失败，见 /tmp/shell-guard-clean.log"; say "$(tail -3 /tmp/shell-guard-clean.log)"; exit 1
fi
# 必须在 shell-android 里跑 cargo：`.cargo/config.toml` 是按**当前工作目录**发现的，
# 用 `--manifest-path` 从仓库根调它读不到那份 NDK linker 配置，链接器会退回宿主 `/usr/bin/ld`
# 并报 `file in wrong format`（本机实测踩过一次，所以这行不是 precaution 而是修案）。
if ! (cd "$SHELL_DIR" && cargo build --release --offline) >/tmp/shell-guard-host.log 2>&1; then
  red "host 构建失败，见 /tmp/shell-guard-host.log"; say "$(tail -5 /tmp/shell-guard-host.log)"; exit 1
fi
if ! (cd "$SHELL_DIR" && cargo build --release --offline --target "$TARGET") >/tmp/shell-guard-target.log 2>&1; then
  red "$TARGET 交叉构建失败，见 /tmp/shell-guard-target.log"; say "$(tail -5 /tmp/shell-guard-target.log)"; exit 1
fi
LIBNAME=libmidline_shell_android.so
HOST_SO="$SHELL_DIR/target/release/$LIBNAME"
TGT_SO="$SHELL_DIR/target/$TARGET/release/$LIBNAME"
ok "两份产物都出来了：$(stat -c%s "$HOST_SO") B（host）／$(stat -c%s "$TGT_SO") B（$TARGET）"

say "== 2. 导出面＝白名单（整张已定义动态符号表，不只 T 段）=="
for so in "$HOST_SO" "$TGT_SO"; do
  # 量的是 nm -D --defined-only 的全部行；类型分布实测就是这一条 T，所以白名单收整表而非收 T。
  got=$(nm -D --defined-only "$so" | awk '{print $3}' | sort)
  want=$(printf '%s\n' "$ALLOWED" | sort)
  case "$so" in "$HOST_SO") where=host;; *) where=$TARGET;; esac
  if [ "$got" = "$want" ]; then
    ok "$where：已定义动态符号恰好 1 条 = ANativeActivity_onCreate"
  else
    red "$so 导出面不等于白名单。实际：$(printf '%s ' $got | tr -d '\n')"
    red "  多出：$(comm -13 <(printf '%s\n' "$want") <(printf '%s\n' "$got") | tr '\n' ' ')"
    red "  缺失：$(comm -23 <(printf '%s\n' "$want") <(printf '%s\n' "$got") | tr '\n' ' ')"
  fi
done

say "== 3. 引擎内部面不得出现在动态表里（按名字点收）=="
# 这一格是"反向"证：白名单相等已经蕴含它为 0，但白名单会随入口需求增长（下一条真壳要加
# JNI 回调时 ANativeActivity_onCreate 那格会变），而引擎字段名不该跟着变——所以单独钉一次。
hits3=0
for so in "$HOST_SO" "$TGT_SO"; do
  for name in Battle CardInst run_level Board svg_frame Session Host; do
    n=$(nm -D --defined-only "$so" | grep -c "$name")
    [ "$n" = 0 ] || { red "$so 里 $name 命中 $n 条（应为 0）"; hits3=$((hits3+1)); }
  done
done
[ "$hits3" = 0 ] && ok "Battle／CardInst／run_level／Board／svg_frame／Session／Host 在两份产物里均 0 命中"

say "== 4. 不产出可被 \`use\` 的元数据构件 =="
# cdylib 不发 .rmeta（上一档 P2 实测：拿它当 Rust 依赖 ⇒ error[E0786]）。
# 本格防的是"有人为了方便把 crate-type 加上 rlib"——加了就当场红，而不是等审计发现。
# 本格的可信度全靠 §1 那次 clean：没有它，这里数到的是历史上某次构建的剩物。
n_rmeta=$(find "$SHELL_DIR/target" -maxdepth 4 -name '*.rmeta' | wc -l)
n_rlib=$(find "$SHELL_DIR/target" -maxdepth 4 -name 'libmidline_shell_android.rlib' | wc -l)
if [ "$n_rmeta" = 0 ] && [ "$n_rlib" = 0 ]; then
  ok "target 下 .rmeta=0、本 crate 的 .rlib=0"
else
  red "出现了可复用元数据：.rmeta=$n_rmeta、.rlib=$n_rlib（crate-type 应当只有 cdylib）"
fi

say "== 5. 平台库确实挂上（NEEDED 面）=="
need=$(readelf -d "$TGT_SO" | sed -n 's/.*NEEDED.*\[\(.*\)\]/\1/p' | sort | tr '\n' ' ')
for lib in libandroid.so liblog.so; do
  case " $need " in *" $lib "*) ok "$TARGET 产物 NEEDED 含 $lib";; *) red "$TARGET 产物 NEEDED 缺 $lib（入口胶水没被真正链上）";; esac
done
allowed_need='libc.so libandroid.so libdl.so liblog.so'
for lib in $need; do
  case " $allowed_need " in *" $lib "*) :;; *) red "NEEDED 出现白名单外的库：$lib";; esac
done
say "  NEEDED 现量：$need"

say "== 6. 布局尺（第二道）：拿 NDK 真头做 _Static_assert =="
# entry.rs 那批 offset_of! 只能证明"我自己的声明自洽"；换序漏字段一样全绿。
# 这一格的外部证人是 /opt/android-ndk 里的那张 native_activity.h。只编不链不跑。
if [ -x "$CLANG" ]; then
  if "$CLANG" --sysroot="$SYSROOT" -c -o /tmp/shell-layout-check.o "$SHELL_DIR/layout_check.c" 2>/tmp/shell-layout-check.log; then
    ok "layout_check.c 对 NDK 头通过（ANativeActivity=80B、ANativeActivityCallbacks=128B、16 个回调偏移逐个对上）"
  else
    red "layout_check.c 与 NDK 头对不上——抄写错了或 NDK 换了 ABI："; sed 's/^/      /' /tmp/shell-layout-check.log | head -12
  fi
else
  red "找不到 NDK clang：$CLANG（这一格不许静默跳过，跳过就等于布局没尺）"
fi

say "== 结论 =="
if [ "$fail" = 0 ]; then say "导出面守门检全过：壳只露平台入口符号，引擎面 0 命中，无元数据外泄，布局有外部证人。"
else say "守门检红。按禁忌 3：以上是实跑读数，不是推断。"; fi
exit "$fail"
