#!/usr/bin/env bash
# midline - 《中线》安卓输入生命周期取证（两条通路：HOME→重进 与 双实例换代）
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
# 这条脚本存在的全部理由：`entry.rs` 里那句 `input queue vanished mid-drain — stop` **说的比量到的多**
# ——它在"换代"与"真空"两格打的是同一句；而 `AInputQueue_detachLooper` 那条硬约束在 2026-10-13 之前
# 一条真机证人都没有（现读：当天全部 logcat 里 created 7／attach 7／destroyed 0／detach 0）。
# 本脚本不修任何东西，只把三件事量成数：
#   ① attach 与 detach 的**配对差**；
#   ② 系统那侧**当前活着**的输入通道（`dumpsys input` 的 live 段按 8hex 通道名去重——ANR 快照段里旧
#      通道名还留着，不切开就会把历史读数当成"现在还挂着"；那道分界是本尺唯一的尺身）；
#   ③ 每一格之后**手指还通不通**（判据是 logcat 里 down 边的增减与那条 `region=`，不是截屏）。
#
# 两条通路各自回答一个问题，2026-10-13 现读：这两条的**答案不一样**，所以不能只跑一条——
#   resume（默认）：用户那条路，HOME→从 launcher 重进 K 轮，会不会换队列？
#   dual          ：开发那条路，`am start -n` 造出同进程第二个实例。换代时旧队列去哪了？顶层销毁之后
#                   下层那条还挂着吗？点得到吗？系统自己怎么说（全量 logcat 抓 "Input dispatching
#                   timed out"，那行里带 8hex 通道名，能指到具体哪一条）。
#   tap           ：首点归因。冷启动后按**标称延迟档位**扫一遍，每档 force-stop 重来、只点一下，
#                   三方同母对质：注入侧（`InputManager: injectMotionEvent … ACTION_DOWN`）、
#                   系统派发名单（`MIUIInput: [MotionEvent] publisher … channel='<8hex> <pkg>/<act>`）、
#                   我们侧（`input touch down`）。哪一档开始三边都有 ⇒ 前面那些档丢在谁手里就量出来了。
#                   注：**getevent 不能当这一档的证人**——18:17 现标定，两下全中的 tap 在 getevent 里
#                   一条事件都没有（整场 12 行，全是设备抬头），因为注入走 InputDispatcher、不过 /dev/input。
#
# 口径三条（都是坑里躺着的）：
#   · 判据之前先读屏幕状态三格（坑 L108）；本脚本要求 keyguard=false 且**不替你解锁**——「密码」气泡是
#     悬浮窗、坐标没有长期真值（坑 L125），抄进来等于埋一颗会自己漂的雷。
#   · 读数一律落持久盘（人 2026-10-13 那句「要持久的写磁盘，别写tmp」），默认 LOGDIR=~/logs/midline/<日期>-lifecycle。
#   · 只认单设备（同 android-install.sh 的口径），多设备下一律拒。

set -uo pipefail

PKG="com.flexiatom.midline"
ACT="android.app.NativeActivity"
# (447,514) 是 2026-10-13 装机读数里现量到 `region=Hand(0)` 的那一格——用**读数**当坐标，
# 不拿换算现算（这条口径当天刚被 M170 上过一次课：期望值不许由被检函数现算）。
TAP_X=447; TAP_Y=514

say() { printf '%s\n' "$*"; }
die() { printf 'input-lifecycle-probe: %s\n' "$*" >&2; exit 1; }

MODE="resume"; K="3"; DELAYS="0.15 0.3 0.5 0.8 1.2 1.7 2.4 3.2"
case "${1:-}" in
  dual)      MODE="dual"; K="${2:-3}" ;;
  tap)       MODE="tap";  DELAYS="${2:-$DELAYS}" ;;       # 第二个参数是延迟档列表（秒，空格分隔）
  resume|"") K="${2:-3}" ;;                   # 显式 resume 或不带参数
  *[!0-9]*)  die "用法：$0 [resume|dual|tap] [轮数 K]（第一个参数拿到的是 '$1'）" ;;
  *)         K="$1" ;;                         # 老写法 `$0 5`：只给轮数，模式默认 resume
esac
case "$K" in (''|*[!0-9]*) die "K 要非负整数，现在拿到的是 '$K'";; esac
case "$DELAYS" in (*[!0-9.\ ]*|'') die "tap 的延迟档要写成『秒数 空格分隔』，现在拿到的是 '$DELAYS'";; esac

command -v adb >/dev/null || die "找不到 adb"
DEVICES="$(adb devices | awk 'NR>1 && $2=="device"{print $1}')"
COUNT="$(printf '%s\n' "$DEVICES" | grep -c . || true)"
[ "$COUNT" = "1" ] || die "需要恰好一台在线设备，现在 $COUNT 台（多设备本脚本不支持，别猜打到哪台）。"
SERIAL="$DEVICES"
adb() { command adb -s "$SERIAL" "$@"; }

LOGDIR="${LOGDIR:-$HOME/logs/midline/$(date +%Y-%m-%d)-lifecycle}"
mkdir -p "$LOGDIR" || die "建不出读数目录 $LOGDIR"
# 文件名带**本次会话的戳**：这不是整洁问题而是真值问题。15:24 那次 resume 读数（created=1、每轮 live=0）
# 事后查是**别的会话的 logcat 还在往同名文件里追加**造成的（见 stop_of_session 那条注释）。
# 名字唯一 ⇒ 陈旧写者写进的是它自己那份，永远混不进本轮；跨会话求和这个坑从此结构上不存在。
STAMP="$(date +%H%M%S)"
RAW="$LOGDIR/logcat-midline-$MODE-$STAMP.txt"
SYS="$LOGDIR/logcat-system-$MODE-$STAMP.txt"
SUM="$LOGDIR/probe-summary-$MODE-$STAMP.txt"

keyguard() { adb shell dumpsys window 2>/dev/null | grep -m1 -oE 'isKeyguardShowing=(true|false)' | cut -d= -f2; }
wake_state() { adb shell dumpsys power 2>/dev/null | grep -m1 -oE 'mWakefulness=[A-Za-z]+' | cut -d= -f2; }
focus() { adb shell dumpsys window 2>/dev/null | grep -m1 mCurrentFocus | tr -d '\r' | sed 's/^ *//'; }
pid() { adb shell pidof "$PKG" 2>/dev/null | tr -d '\r' | awk '{print $1}'; }
tap() { adb shell input tap "$TAP_X" "$TAP_Y"; }
key() { adb shell input keyevent "$1"; }

live_input() {
  adb shell dumpsys input 2>/dev/null \
    | awk -v stop='^Input Dispatcher State at time of last ANR:' '$0~stop{exit} /^Input Dispatcher State:/{f=1;next} f'
}
channels() { live_input | grep -oE "[0-9a-f]{7,8} ${PKG}/${ACT}" | sort -u | wc -l | tr -d ' '; }
# 尺自己也要被量一次：live 段的**行数**与通道数并排落进读数。"0 条通道"（应用真没挂）与
# "0 条通道因为这段 awk 一行都没读到"（尺失效）是两格，只报前一个数会把这两种分不开——
# 而这两种在 15:24 那次读数里恰好都长成一个 0。
gauge_shape() { live_input | wc -l | tr -d ' '; }
# 逐条通道现读：8hex 名＋status＋responsive。"挂着"与"挂着且已不回话"是两格，只有后者能指到 ANR。
channel_detail() {
  live_input | grep -oE "[0-9a-f]{7,8} ${PKG}/${ACT} \((server|client)\).*responsive=[a-z]+" | sort -u
}
# 派发器的焦点窗名单——**注入之前**就能读，所以它是一闸门而不是证人。
# 2026-10-14 现读：冷启动后头 0.6–1.1s 这一段是**空的**（闪屏窗正 EXITING，我们窗还没进 FocusedWindows），
# 期间注入的 touch 系统不派给任何 App 窗 ⇒ 那一发的"点不动"归时机，不归我们的队列。
focused_windows() {
  live_input | awk '/^  FocusedWindows:/{p=1;next} /^  FocusRequests:/{p=0} p'
}
focus_target_up() { focused_windows | grep -qE "${PKG}/${ACT}"; }
gate_shape() { focused_windows | wc -l | tr -d ' '; }   # 同 gauge_shape 的口径：空段与尺失效要分得开
wait_for_focus_target() { local bound="${1:-6}" i
  for i in $(seq 1 "$((bound * 5))"); do focus_target_up && return 0; sleep 0.2; done
  return 1; }
count_in_raw() { grep -c -- "$1" "$RAW" 2>/dev/null || true; }

MID_PID=""; SYS_PID=""
start_of_session() {
  adb shell am force-stop "$PKG" >/dev/null 2>&1
  adb logcat -c >/dev/null 2>&1
  # 抓取用**真 adb**、不经 `adb` 那个函数：把函数放进 `&` 里，bash 起的是**子 shell**，`$!` 拿到的是
  # 子 shell 的 PID，adb 本体随后被 reparent 继续活着（15:24 现读：三条已"收尾"的会话各有 2 个 adb
  # 存活 20 分钟以上，还在往同名文件里写字）。这里直接 `command adb` ⇒ `$!` 就是 adb 自己。
  command adb -s "$SERIAL" logcat -v threadtime -s midline:V > "$RAW" 2>&1 & MID_PID=$!
  # 全量那份是留给系统自己说话的：ANR 与 "Input dispatching timed out" 都不带我们的 tag。
  command adb -s "$SERIAL" logcat -v threadtime > "$SYS" 2>&1 & SYS_PID=$!
  sleep 0.6
  # 起手闸门：本轮 RAW 只许有 logcat 自己那行抬头。非空 = 有别的东西在写这里（陈旧写者或 logcat -c 失效），
  # 那种读数从第一条起就是两团日志的混合——**比没有数更坏**，所以当场停，不"打个折继续看"。
  local n0; n0="$(wc -l < "$RAW" | tr -d ' ')"
  [ "$n0" -le 1 ] || die "抓取刚起 RAW 就有 $n0 行（只该有 logcat 的 1 行抬头）——有陈旧写者或 logcat -c 没生效，本轮读数不可信，已停。RAW=$RAW"
}
stop_of_session() {
  local p left=""
  for p in "$MID_PID" "$SYS_PID"; do
    [ -n "$p" ] || continue
    kill "$p" 2>/dev/null
    for _ in 1 2 3 4 5 6 7 8; do kill -0 "$p" 2>/dev/null || break; sleep 0.25; done
    kill -9 "$p" 2>/dev/null
    kill -0 "$p" 2>/dev/null && left="$left $p"
  done
  # 停不干净就**当场说话**：静默失败等于下一轮读数是两团日志的混合，那种数比没有数更坏。
  [ -n "$left" ] && say "!! 抓取进程没停干净：$left —— 本轮之后的 RAW 会继续被写，读数别再复用上一次的"
  return 0
}
trap 'stop_of_session' EXIT INT TERM

wait_for() { local want="$1" bound="${2:-8}" i
  for i in $(seq 1 "$((bound * 4))"); do grep -q -- "$want" "$RAW" 2>/dev/null && return 0; sleep 0.25; done
  return 1; }
launch_via_launcher() { adb shell monkey -p "$PKG" -c android.intent.category.LAUNCHER 1 >/dev/null 2>&1; }
launch_via_start() { adb shell am start -n "$PKG/$ACT" >/dev/null 2>&1; }

say "== 屏幕状态（判据之前先读这三格）：wakefulness=$(wake_state) keyguard=$(keyguard)  模式=$MODE $( [ "$MODE" = tap ] && printf '档位=%s' "$DELAYS" || printf 'K=%s' "$K" )"
[ "$(keyguard)" = "false" ] || die "锁屏着。本脚本不替你解锁——「密码」气泡是悬浮窗、坐标无长期真值。请在手机上解锁后重跑，或走 scripts/android-install.sh 那条已验通路。"
[ "$(wake_state)" = "Awake" ] || { key KEYCODE_WAKEUP; sleep 1.5; }

start_of_session
BASE_CH="$(channels)"
say "== 基线：force-stop 之后 live 通道=$BASE_CH（应为 0；不为 0 就是上一轮留下的，本轮读数要打折看）／live 段现读 $(gauge_shape) 行"
if [ "$MODE" = tap ]; then
  say "== tap 模式：每档都 force-stop 重来，launch 之后按标称延迟点**一下**；『实点』是 launch→tap 的实测秒数（含中间那次闸门回读的 adb 往返），阈值以它为准"
  printf '标称s  实点s  pid  注入DOWN  进派发名单  我们的down边  attach到否  焦点gained到否  本档日志里出现过的我们通道名（不分哪种行，别读成"派给了它"）  点前闸门  点后可见  判定\n' > "$SUM"
else
  launch_via_launcher
  wait_for "input attach queue=" 8 || die "冷启动 8s 内没等到 'input attach queue='——线程没起来或队列没创建，后面量的东西不存在。RAW=$RAW"
  say "== 冷启动：pid=$(pid) created=$(count_in_raw onInputQueueCreated) attach=$(count_in_raw 'input attach queue=') live 通道=$(channels)"
  printf '阶段  pid  created  attach  destroyed  detach  live通道  live段行数  这一下点中\n' > "$SUM"
fi
row() { printf '%s\n' "$*" | tee -a "$SUM"; }
snap() { # snap <阶段名> [tap]
  local name="$1" hit="-" before after gate k nw
  if [ "${2:-no}" = "tap" ]; then
    # 点之前先过闸门。但**闸门不充分**：2026-10-14 扫频里 实0.56s 那档 FocusedWindows 已经指到我们窗
    # （点前闸门=1/空段1行），系统仍没把那一下派给我们、我们也没有 down 边。所以"落到才算点着"，
    # 没落就补点并重数——**补点次数本身就是读数**（0=一次就中，n=首下丢在启动空窗里）。
    if wait_for_focus_target 6; then gate="闸门到"; else gate="闸门超时(空段$(gate_shape)行)"; fi
    before="$(count_in_raw 'input touch down')"; k=0
    for k in 0 1 2 3; do
      tap; sleep 1.2
      nw="$(count_in_raw 'input touch down')"
      [ "$nw" -gt "$before" ] && { after="$nw"; break; }
      after="$nw"
    done
    hit="down 边 $before→$after｜$(grep -a 'input touch down' "$RAW" | tail -1 | grep -oE 'region=[A-Za-z(0-9]+' || echo '没等到新 down 边')｜$gate｜补点${k}次"
  fi
  row "$name  $(pid)  $(count_in_raw onInputQueueCreated)  $(count_in_raw 'input attach queue=')  $(count_in_raw onInputQueueDestroyed)  $(count_in_raw 'detachLooper done')  $(channels)  $(gauge_shape)  $hit"
}

case "$MODE" in
tap)
  # 三方对质都取**本次这一档的增量**（按行号切片），不取全程计数——全程计数会把上一档的下边算到这一档头上。
  n_raw() { wc -l < "$RAW" | tr -d ' '; }
  n_sys() { wc -l < "$SYS" | tr -d ' '; }
  seg()  { awk -v a="$1" -v b="$2" 'NR>a && NR<=b' "$3"; }
  # 派发名单的真文是 `channel '<8hex> <pkg>/<act> (server)'`（**空格**，不是等号）。18:17 那轮两下全中、
  # 名单里两次都有 d5c636f ⇒ 这是**正对照**；拿它当尺时"名单里没有"才说明得东西。
  ROUTED_RE="publisher action=0x0.*channel '[0-9a-f]{7,8} ${PKG}/${ACT}"
  for d in $DELAYS; do
    adb shell am force-stop "$PKG" >/dev/null 2>&1; sleep 0.8
    r0="$(n_raw)"; s0="$(n_sys)"
    launch_via_launcher
    tl="$(date +%s.%N)"
    sleep "$d"
    gwc0=$(focus_target_up && echo 1 || echo 0); gs0=$(gate_shape)   # 闸门要在**点之前**读，读晚了量到的是"这一档结束后才开的窗"
    tt="$(date +%s.%N)"; real="$(awk -v a="$tl" -v b="$tt" 'BEGIN{printf "%.2f", b-a}')"
    tap; sleep 1.6
    rw="$(seg "$r0" "$(n_raw)" "$RAW")"; sy="$(seg "$s0" "$(n_sys)" "$SYS")"
    inj=$(printf '%s\n' "$sy" | grep -cE 'injectMotionEvent: [0-9]+-\{action=ACTION_DOWN' || true)
    routed=$(printf '%s\n' "$sy" | grep -cE "$ROUTED_RE" || true)
    ours=$(printf '%s\n' "$rw" | grep -c 'input touch down' || true)
    att=$(printf '%s\n' "$rw" | grep -c 'input attach queue=' || true)
    gn=$(printf '%s\n' "$rw" | grep -c 'onWindowFocusChanged: gained' || true)
    gwc=$(focus_target_up && echo 1 || echo 0)   # 点完之后回读闸门：与"进派发名单"是两个独立读法，同向才算尺自洽
    ch=$(printf '%s\n' "$sy" | grep -oE "[0-9a-f]{7,8} ${PKG}/${ACT}" | head -1 || true)
    verdict="中"
    [ "$inj" = 0 ] && verdict="注入那一下没进系统 ⇒ 这一档作废（不是谁的锅）"
    [ "$inj" -ge 1 ] && [ "$routed" = 0 ] && [ "$ours" = 0 ] && verdict="系统没派我们窗 ⇒ 归平台/时机"
    [ "$inj" -ge 1 ] && [ "$routed" -ge 1 ] && [ "$ours" = 0 ] && verdict="系统派了、我们没收到 ⇒ 归我们（队列/线程）"
    # 作废标注放在**最后**：早先把它写在中间，后面那条更具体的判定把它整句盖掉，读数里就看不见"这档不算"了。
    [ "$gwc0" = 0 ] && verdict="$verdict｜点前闸门是空段 ⇒ 这档量的是启动空窗，不能当我们队列的证据"
    [ "$gwc0" = 1 ] && [ "$routed" = 0 ] && verdict="$verdict｜闸门已指到我们窗仍没派 ⇒ 闸门不充分，还有一段"
    row "${d}s  实${real}s  $(pid)  $inj  $routed  $ours  $att  $gn  ${ch:-无}  点前闸门=$gwc0/空段${gs0}行  点后可见=$gwc  $verdict"
  done
  ;;
resume)
  snap 冷启动 tap
  for k in $(seq 1 "$K"); do
    key KEYCODE_HOME; sleep 1.0
    launch_via_launcher; sleep 2.5
    snap "轮$k(HOME→重进)" tap
  done
  key 4; sleep 2.5
  snap "finish(BACK)" tap
  say "   BACK 之后的句：$(grep -aoE 'onInputQueueDestroyed|detachLooper done|onDestroy|queue vanished' "$RAW" | tr '\n' ' ')"
  ;;
dual)
  snap 实例A tap
  launch_via_start; sleep 2.5
  say "   第二个实例起来：焦点=$(focus)"
  snap "实例B(am-start-n)" tap
  for i in $(seq 1 "$K"); do sleep 5; snap "静置${i}×5s(A的destroy来不来)" no; done
  say "   此刻通道明细：$(channel_detail | tr '\n' ' ')"
  key 4; sleep 1
  snap "BACK收掉B" tap
  for i in $(seq 1 3); do sleep 5; snap "BACK后${i}×5s" tap; done
  say "   通道明细（末）：$(channel_detail | tr '\n' ' ')"
  say "   焦点（末）=$(focus)"
  say "   系统为这条包打了多少句超时／ANR：$(grep -acE 'Input dispatching timed out|ANR in '"$PKG" "$SYS" || true) 行"
  grep -aE "Input dispatching timed out|ANR in $PKG|Waited [0-9]+ms for MotionEvent" "$SYS" | sed 's/^/      /' | head -6
  ;;
esac

stop_of_session
{
  say "--- 本次会话的输入生命周期行（原样，含时间戳与 tid）---"
  grep -aE 'onInputQueue|input attach|detachLooper done|queue (vanished|replaced|cleared)|ANativeActivity_onCreate|onDestroy' "$RAW" || say "（无）"
} >> "$SUM"
say "== 读数：$SUM ／ midline 流 $RAW（$(wc -l < "$RAW") 行）／系统流 $SYS（$(wc -l < "$SYS") 行）"
say "   配对差 attach−detach=$(( $(count_in_raw 'input attach queue=') - $(count_in_raw 'detachLooper done') ))；live 通道末值=$(channels)（基线=$BASE_CH）"
