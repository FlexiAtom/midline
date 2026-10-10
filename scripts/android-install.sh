#!/usr/bin/env bash
# midline - 《中线》安卓装机通路（解锁 → install → 轮询「继续安装」→ 回读）
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
# 这条脚本存在的全部理由：MIUI 上 `adb install` 会弹一个**不会第一时间出现**的「继续安装」对话框，
# 不点它就一直挂着；而手机不在手边时没人能点。人 2026-10-11 给了通路并授权「2D版本的做好直接装」。
#
# 三格判据全部实测过（读数与推导见 README 的第十七步 + pitfalls 那四条）：
# ① **任何像素判据之前先读屏幕状态**——息屏时 screencap 回传的是上一帧缓存，看着像桌面其实是假的。
# ② 「密码」气泡单独点一下就解锁（实测 4~6s），不需要 swipe；等待窗口按"每 2s 轮询、最多 20s"写，
#    别 sleep 一个固定值就判失败（那次我 sleep 6 回读仍锁着，差点误判通路不通）。
# ③ 「继续安装」的坐标 (207,1495) 在桌面态正好落在 dock 的 QQ 图标上 ⇒ **无条件 tap 等于替人开 App**，
#    所以三条颜色判据缺一不点。
# 所有坐标是这台 720x1600 上量的，脚本按 screencap 实际宽高换算——换算只救形状，不救"换机后判据色
# 是否还是那三个色"，那格未验，换设备得重量。

set -euo pipefail

PKG="com.flexiatom.midline"
REF_W=720
REF_H=1600
BUBBLE_X=678; BUBBLE_Y=789          # Touch Tool 的「密码」气泡中心（实测 crop 定位）
BUBBLE_PROBE_X=660; BUBBLE_PROBE_Y=770  # 气泡内一点，用来验"气泡在位"
GATE1_X=307; GATE1_Y=1503; GATE1_C=#F0F0F0
GATE2_X=619; GATE2_Y=1192; GATE2_C=#FFFFFF
GATE3_X=613; GATE3_Y=1480; GATE3_C=#3482FF
CONTINUE_X=207; CONTINUE_Y=1495     # 「继续安装」按钮中央附近
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

die() { echo "android-install: $*" >&2; exit 1; }
PX="$(dirname "$0")/png-pixel.py"

APK="${1:-}"
[ -n "$APK" ] && [ -f "$APK" ] || die "用法：$0 <apk>"
command -v adb >/dev/null || die "找不到 adb"
[ -f "$PX" ] || die "找不到像素读取器 $PX"

# 只认单设备：多设备下每个 adb 命令都要 -s，漏一个就装到别的机器上。
DEVICES="$(adb devices | awk 'NR>1 && $2=="device"{print $1}')"
COUNT="$(printf '%s\n' "$DEVICES" | grep -c . || true)"
[ "$COUNT" = "1" ] || die "需要恰好一台在线设备，现在 $COUNT 台（多设备本脚本不支持，别猜装到哪台）。"
SERIAL="$DEVICES"
adb() { command adb -s "$SERIAL" "$@"; }

keyguard() { adb shell dumpsys window 2>/dev/null | grep -m1 -oE 'isKeyguardShowing=(true|false)' | cut -d= -f2; }
wake_state() { adb shell dumpsys power 2>/dev/null | grep -m1 -oE 'mWakefulness=[A-Za-z]+' | cut -d= -f2; }
display_state() { adb shell dumpsys display 2>/dev/null | grep -m1 -oE 'mState=[A-Z]+' | cut -d= -f2; }

cap() { adb exec-out screencap -p > "$STAGE/shot.png"; }
# 换算：量的时候是 720x1600，落到实际分辨率
scale_x() { awk -v x="$1" -v w="$(python3 "$PX" size "$STAGE/shot.png" | cut -dx -f1)" 'BEGIN{printf "%d", x*w/'"$REF_W"'}'; }
scale_y() { awk -v y="$1" -v h="$(python3 "$PX" size "$STAGE/shot.png" | cut -dx -f2)" 'BEGIN{printf "%d", y*h/'"$REF_H"'}'; }

echo "== 屏幕状态（判据之前先读这三格）：wakefulness=$(wake_state) display=$(display_state) keyguard=$(keyguard)"
if [ "$(wake_state)" != "Awake" ]; then
  adb shell input keyevent KEYCODE_WAKEUP
  sleep 1.5
  echo "   唤醒后：wakefulness=$(wake_state) display=$(display_state) keyguard=$(keyguard)"
fi

if [ "$(keyguard)" != "false" ]; then
  cap
  python3 "$PX" hex "$STAGE/shot.png" 60 "$(scale_x $BUBBLE_PROBE_X),$(scale_y $BUBBLE_PROBE_Y),#FFFFFF" \
    || die "锁屏了，但「密码」气泡不在位（探测点不是近白）。气泡是悬浮窗，可能被拖走了——把探测坐标重新量一遍，别改成「无条件点」。"
  # 气泡内部已含上划（人 2026-10-11 原话），所以这里**只点、不 swipe**；
  # MIUI 的注入许可是间歇的（同坐标有时全数 failed 有时 0 次），故失败后重点一次，最多两轮。
  for attempt in 1 2; do
    echo "== 点「密码」气泡解锁（第 $attempt 轮）"
    adb shell input tap "$(scale_x $BUBBLE_X)" "$(scale_y $BUBBLE_Y)"
    for i in $(seq 1 10); do
      sleep 2
      kg="$(keyguard)"
      [ "$kg" = "false" ] && break
    done
    echo "   第 $attempt 轮结束：keyguard=$kg，注入失败计数=$(adb logcat -d -v brief 2>/dev/null | grep -cE 'injection from pid.*failed')"
    [ "$kg" = "false" ] && break
  done
  [ "$(keyguard)" = "false" ] || die "点了两轮气泡 20s×2 仍未解锁。停在这里，不 swipe、不猜密码、不盲点——要么人在手机上点一下气泡，要么查开发者选项里那两格 USB 调试授权。"
fi

echo "== adb install -r（MIUI 会弹「继续安装」，且**「拒绝 (N)」那颗倒计时是自动拒绝**）
⇒ 胜负手是"弹窗出现→tap 出去"的延迟，不是坐标。热路径因此一次 magick 读三点（C 实现，~0.15s），
   轮询之间不 sleep；纯 Python 那份 PNG 解码器 720x1600 逐行反 filter 要秒级，放热路径上会把十秒倒计时耗光。"
command -v magick >/dev/null || die "热路径需要 magick（一次进程读三个像素）；本机没有就别用本脚本。"

# 分辨率在整个装机期间不变，坐标只换算一次
cap
SW="$(python3 "$PX" size "$STAGE/shot.png" | cut -dx -f1)"
SH="$(python3 "$PX" size "$STAGE/shot.png" | cut -dx -f2)"
sx() { awk -v x="$1" -v w="$SW" 'BEGIN{printf "%d", x*w/'"$REF_W"'}'; }
sy() { awk -v y="$1" -v h="$SH" 'BEGIN{printf "%d", y*h/'"$REF_H"'}'; }
G1="$(sx $GATE1_X),$(sy $GATE1_Y)"; G2="$(sx $GATE2_X),$(sy $GATE2_Y)"; G3="$(sx $GATE3_X),$(sy $GATE3_Y)"
CX="$(sx $CONTINUE_X)"; CY="$(sy $CONTINUE_Y)"

# 一次 magick 读三点 → "RRGGBBAA RRGGBBAA RRGGBBAA"，逐点比通道差之和 <= tol
gate() {
  local got tol want h r g b wr wg wb d all=1
  got="$(magick "$STAGE/shot.png" -format "%[hex:p{${G1}}] %[hex:p{${G2}}] %[hex:p{${G3}}]" info:)"
  tol="$1"
  set -- $got
  for pair in "$1:$GATE1_C" "$2:$GATE2_C" "$3:$GATE3_C"; do
    h="${pair%%:*}"; want="${pair##*:}"
    r=$((16#${h:0:2})); g=$((16#${h:2:2})); b=$((16#${h:4:2}))
    wr=$((16#${want:1:2})); wg=$((16#${want:3:2})); wb=$((16#${want:5:2}))
    d=$(( (r>wr?r-wr:wr-r) + (g>wg?g-wg:wg-g) + (b>wb?b-wb:wb-b) ))
    [ "$d" -le "$tol" ] || all=0
  done
  printf '%s [%s]' "$all" "$got"
}

adb install -r "$APK" > "$STAGE/install.log" 2>&1 &
INSTALL_PID=$!
tapped=0
for i in $(seq 1 120); do
  if ! kill -0 "$INSTALL_PID" 2>/dev/null; then break; fi   # 安装自己结束了（无需点／或已失败）
  cap
  read -r hit detail <<<"$(gate 24)"
  if [ "$hit" = "1" ]; then
    adb shell input tap "$CX" "$CY"          # 命中就地立刻点，中间不插任何 sleep
    tapped=1
    cp "$STAGE/shot.png" "/tmp/midline-install-dialog.png"   # 判据命中的那一帧留证
    echo "   第 $i 轮判据全中 → 已点「继续安装」($CX,$CY)；读数 $detail"
    break
  fi
  [ $((i % 10)) = 0 ] && echo "   第 $i 轮未命中，读数 $detail"
done
wait "$INSTALL_PID" 2>/dev/null || true
cat "$STAGE/install.log"
grep -q 'Success' "$STAGE/install.log" || {
  cp "$STAGE/shot.png" "/tmp/midline-install-last.png" 2>/dev/null || true
  echo "android-install: install 没回 Success（tapped=$tapped）。别重跑同一命令当没事发生——先看上面那份日志。" >&2
  echo "最后一帧：/tmp/midline-install-last.png（弹窗态可查 /tmp/midline-install-dialog.png）" >&2
  exit 1
}

echo "== 回读"
PATH_OUT="$(adb shell pm path "$PKG" 2>/dev/null | tr -d '\r' | head -1)"
VER="$(adb shell dumpsys package "$PKG" 2>/dev/null | grep -m1 -oE 'versionName=[^ ]+' | tr -d '\r')"
[ -n "$PATH_OUT" ] || die "install 报 Success 但 pm path 读不到 $PKG ⇒ 装上的不是它。"
echo "包名 $PKG  $VER"
echo "路径 $PATH_OUT"
