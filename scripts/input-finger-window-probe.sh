#!/usr/bin/env bash
# midline - 真人手指撞冷启动首点空窗（H3）：每轮 force-stop→冷启动，人**连点**若干下，三方对质
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
# 这条脚本存在的理由：`input-lifecycle-probe.sh tap` 量的是 **adb 注入**，而注入走 InputDispatcher、
# 不落 /dev/input（10-13 18:17 现标定：两下全中的 tap 在 getevent 里一条事件都没有，整场 12 行全是设备抬头）。
# 所以"本机注入通路有 ≤0.6 s 启动空窗"那句话**从来没说真人手指算不算**——真手指走 /dev/input，是另一条通路。
# 这里 getevent 反过来当唯一可用的证人：只有真手指才在里面有行，它给的正是"人确实按了"这一格。
#
# 两个钟不同轴这件事（10-14 16:01 现量，本脚本按此重写）：`getevent -t` 打的是 **CLOCK_MONOTONIC**，
# 而 `/proc/uptime` 含休眠，本机现读差 **6400 s**（uptime 269702 对 getevent 263302）⇒ 拿 uptime 去换算
# getevent 时戳会读出"按下发生在 launch 前 6400 秒"这种假数。正解不猜休眠，而是**用同一轮里的事件把两个钟钉起来**：
# 人每轮**连点**多下，第 k 下在 getevent 里有单调时戳、在我们 logcat（`-v epoch`）里有 epoch 时戳，
# 取**末对**（最后一次按下必在应用已起之后，不会丢）算 offset = 末次down_epoch − 末次press_mono，
# 再把它套到**首下**上 ⇒ 首下epoch = 首下mono + offset。误差量级 = 两次的派发延迟之差（毫秒级），不是 6400 秒。
#
# 三把尺（一轮三份原文）：
#   ① 按下侧：`getevent -t -l` 的 EV_KEY BTN_TOUCH DOWN 名单（个数＝他按了几下，这是"他确实按了"的证人）。
#   ② 我们侧：`input touch down` 名单（个数＝收到几下）。①②个数不等 ⇒ 少的就是丢的，且**首下最可能丢**。
#   ③ 顺序：首下epoch − `ANativeActivity_onCreate`epoch 的正负号 ⇒ 他第一下落在我们建好队列之前还是之后。
#   注：设备侧 `date +%s` 与 `date -u +%s` **同值**（epoch 与时区无关），所以本脚本全程不引"UTC 偏移"；
#   墙钟 `-v threadtime` 只做可读副本，判据一律 `-v epoch`。
#
# 口径三条：判据前读屏三格（坑 L108/L125，不替你解锁）；读数落持久盘（人 2026-10-13「要持久的写磁盘，别写tmp」）；
# 提示坐标 (447,514) 取 10-13 装机读数里现量到 region=Hand(0) 那一格，不拿换算现算（M170 那课）。

set -uo pipefail

PKG="com.flexiatom.midline"
TAP_X=447; TAP_Y=514
ROUNDS="${1:-5}"
GATE="${2:-30}"          # 开跑前准备秒
WAIT="${WAIT:-6}"        # 每轮给手的窗口秒（要容得下连点多下）
LOGDIR="${LOGDIR:-$HOME/logs/midline/$(date +%Y-%m-%d)-finger}"

say() { printf '%s\n' "$*"; }
die() { printf 'input-finger-window-probe: %s\n' "$*" >&2; exit 1; }
keyguard() { adb shell dumpsys window 2>/dev/null | grep -m1 -oE 'isKeyguardShowing=(true|false)' | cut -d= -f2; }
wake_state() { adb shell dumpsys power 2>/dev/null | grep -m1 -oE 'mWakefulness=[A-Za-z]+' | cut -d= -f2; }
# getevent 行首是 "[  263302.068711]"——方括号与数字之间有空格，awk 的 $1 只是 "["，只能按正则抠
mono_of() { sed -n 's/^\[ *\([0-9][0-9.]*\)\].*/\1/p' | head -1; }
mono_all() { sed -n 's/^\[ *\([0-9][0-9.]*\)\].*/\1/p'; }

DEV=$(adb devices | tail -n +2 | grep -c -E '\s+device$') || DEV=0
[ "$DEV" = 1 ] || die "只认单设备，现读 $DEV 台在线。"
[ "$(keyguard)" = "false" ] || die "锁屏着。本脚本不替你解锁——请在手机上解锁后重跑。"
[ "$(wake_state)" = "Awake" ] || { adb shell input keyevent KEYCODE_WAKEUP; sleep 1.5; }

mkdir -p "$LOGDIR" || die "建不了读数目录 $LOGDIR"
STAMP=$(date +%H%M%S)
SUM="$LOGDIR/finger-summary-$STAMP.txt"

{
  say "== 屏幕：wakefulness=$(wake_state) keyguard=$(keyguard)"
  say "== 提示坐标 ($TAP_X,$TAP_Y)｜判据轴=epoch；墙钟只作可读副本"
  printf '轮  按下数  收到数  create_epoch  首下mono  末下mono  末down_epoch  offset(s)  首下epoch  首下−create(ms)  末down−create(ms)  判定\n'
} > "$SUM"

say "每轮：关进程 → 冷启动 → 你**一看到画面就连点** ($TAP_X,$TAP_Y) 那颗手牌，越快越好、连点 4~6 下；共 $ROUNDS 轮，每轮 ${WAIT}s。"
say "== $GATE 秒后开始，把手放到那一格上"
sleep "$GATE"

for i in $(seq 1 "$ROUNDS"); do
  adb shell am force-stop "$PKG" >/dev/null 2>&1; sleep 1.2
  adb logcat -c >/dev/null 2>&1
  GF="$LOGDIR/finger-r$i-getevent-$STAMP.txt"
  LT="$LOGDIR/finger-r$i-logcat-wall-$STAMP.txt"
  LE="$LOGDIR/finger-r$i-logcat-epoch-$STAMP.txt"
  adb shell getevent -t -l > "$GF" 2>&1 &
  GE=$!
  sleep 0.4
  adb shell monkey -p "$PKG" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1
  sleep "$WAIT"
  kill "$GE" 2>/dev/null; wait "$GE" 2>/dev/null
  adb shell pkill getevent >/dev/null 2>&1
  adb logcat -d -v threadtime -s midline:V > "$LT" 2>/dev/null
  adb logcat -d -v epoch -s midline:V > "$LE" 2>/dev/null

  # -l 会把值翻成名字（DOWN），也可能仍是 00000001；两种都认，别在格式上赌。
  PALL=$(grep -aE 'EV_KEY[[:space:]]+BTN_TOUCH[[:space:]]+(DOWN|00000001|1)' "$GF" | mono_all)
  P1=$(printf '%s\n' "$PALL" | head -1); PL=$(printf '%s\n' "$PALL" | tail -1)
  NP=$(printf '%s\n' "$PALL" | grep -c . || true)
  DALL=$(grep -a 'input touch down' "$LE" | awk '{print $1}')
  DN=$(printf '%s\n' "$DALL" | grep -c . || true)
  DL=$(printf '%s\n' "$DALL" | tail -1)
  CRE=$(grep -m1 'ANativeActivity_onCreate' "$LE" | awk '{print $1}')
  say "   轮$i：按下 $NP 下／收到 $DN 下（getevent $(wc -l < "$GF") 行／我们 $(grep -ac 'midline :' "$LT") 行）"

  OFF=""; P1E=""; P1C=""; DLC=""
  if [ -n "${PL:-}" ] && [ -n "${DL:-}" ]; then
    OFF=$(awk -v d="$DL" -v p="$PL" 'BEGIN{printf "%.6f", d-p}')
    [ -n "${P1:-}" ] && P1E=$(awk -v p="$P1" -v o="$OFF" 'BEGIN{printf "%.6f", p+o}')
  fi
  if [ -n "${CRE:-}" ] && [ -n "${P1E:-}" ]; then P1C=$(awk -v p="$P1E" -v c="$CRE" 'BEGIN{printf "%d", (p-c)*1000}'); fi
  if [ -n "${CRE:-}" ] && [ -n "${DL:-}" ]; then DLC=$(awk -v d="$DL" -v c="$CRE" 'BEGIN{printf "%d", (d-c)*1000}'); fi

  if [ -z "${CRE:-}" ]; then verdict="连 ANativeActivity_onCreate 都没起来 ⇒ 作废"
  elif [ "$NP" = 0 ]; then verdict="你这一轮**没按**（getevent 零条 BTN_TOUCH）⇒ 作废，不是谁的锅"
  elif [ "$DN" = 0 ]; then verdict="按了 $NP 下而一条 down 边都没有 ⇒ 全丢（首下至少在内）"
  elif [ "$NP" = 1 ] && [ "$DN" = 1 ]; then verdict="按 1 下收 1 下｜只有一对，offset 由这对自己钉 ⇒ **首下位置不可定**（退化，别看首下−create）"
  elif [ "$NP" != "$DN" ]; then verdict="按 $NP 下、收到 $DN 下 ⇒ **少了 $((NP-DN)) 下**；首下−create=${P1C:-?}ms（前提：丢的不是末下，否则 offset 失效）"
  else verdict="按 $NP 下全收到；首下−create=${P1C:-?}ms（正=建好之后，负=撞在空窗里）"
  fi
  printf '%s  %s  %s  %s  %s  %s  %s  %s  %s  %s  %s  %s\n' \
    "$i" "$NP" "$DN" "${CRE:-无}" "${P1:-无}" "${PL:-无}" "${DL:-无}" "${OFF:-无}" "${P1E:-无}" \
    "${P1C:-无}" "${DLC:-无}" "$verdict" >> "$SUM"
done

adb shell am force-stop "$PKG" >/dev/null 2>&1
adb shell input keyevent KEYCODE_HOME >/dev/null 2>&1
say "== 读数：$SUM（getevent 与两份 logcat 原文同目录）"
cat "$SUM"
