#!/usr/bin/env bash
# midline - 《中线》安卓 APK 组包链（壳 .so → aapt2 link → zipalign → apksigner）
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
# 组包链照 ~/rust/rust-beast-sdk-android/scripts/build_apk.sh 的骨架借（那仓真装过机、真签过名），
# 但**不搬它的 dex 那一段**：本壳的入口是 `ANativeActivity_onCreate`，Activity 用框架自带的
# `android.app.NativeActivity`，APK 里没有一行 Java ⇒ manifest 上 `android:hasCode="false"`，
# classes.dex 整段消失（代价与验证见 README 的"第十七步"条目）。
#
# 口径三条（都是这仓已经付过学费的地方，逐条有牙）：
# ① 输出目录必须是全新的——复用目录会让"产物存在"与"本次产物"分家（byte-baseline 那条同款坑）。
# ② 签名键缺失时**不自动生成**：换键 = 已装机的包再也覆盖不了，得卸载重装。
# ③ 指纹每次实测比对，不匹配就停——指错一把"也存在"的键是最容易发生的静默失败。

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SHELL_DIR="$ROOT/shell-android"
SDK="${ANDROID_HOME:-/opt/android-sdk}"
BUILD_TOOLS="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
PLATFORM="$SDK/platforms/android-34/android.jar"
OUT="$ROOT/dist"
# 字体子集的落点（与 scripts/android-font.sh 同一条约定）：**刻意不在 dist/ 里**——dist 有"必须是
# 全新目录"那道闸，资产若预先躺在里面，每次组包都会先被自己那道闸毙掉。
ASSETS="${MIDLINE_ASSETS:-$ROOT/build/assets}"
ABI_DIR="arm64-v8a"
RUST_TARGET="aarch64-linux-android"
LIB_NAME="midline_shell_android"

# 版本号单一来源：改这里，manifest 与产物名一起变。
VERSION="0.1.0"
VERSION_CODE="1"
PACKAGE="com.flexiatom.midline"
LABEL="中线"

# 发布签名键：刻意不在仓库里，也不在 dist/（dist 会被清，键跟着没了就覆盖不了老包）。
KS="${MIDLINE_KEYSTORE:-$HOME/.midline-android/release.keystore}"
KS_ALIAS="midline"
KS_PASS_FILE="${MIDLINE_KEYSTORE_PASS_FILE:-$KS.pass}"
KEY_PASS_FILE="${MIDLINE_KEY_PASS_FILE:-$KS.$KS_ALIAS.pass}"
KS_CERT_SHA256="ad5bf6d2ef5b7a51bf69d76b7c5f281cc7313aac36e048c99773a557e6c243e4"

die() { echo "$*" >&2; exit 1; }

[ -x "$BUILD_TOOLS/aapt2" ] || die "找不到 aapt2（ANDROID_HOME=$SDK）"
[ -f "$PLATFORM" ] || die "找不到 $PLATFORM"
[ -d "$SHELL_DIR" ] || die "找不到壳目录 $SHELL_DIR"
# 字体是**必需的**，不是可选装饰：没有 ui.otf 的包能签能装，但屏上永远不会有字（壳的 text 面
# 全靠资产通道里那 310 KB）。缺它就停，而不是"先出一个没字的包"——后者在真机上看着像画坏了，
# 会把人引向错误的那格（上一帧 fmt=4 写花缓冲就是这种"能跑但错"的代价）。
[ -f "$ASSETS/fonts/ui.otf" ] || die "找不到字体子集 $ASSETS/fonts/ui.otf ⇒ 先跑 bash scripts/android-font.sh
（它现抽码点、出三档字节数，并把选中的一档放到这里）。换目录用 MIDLINE_ASSETS。"

# ① 全新目录闸（闸的是**实际要写的那个目录**，所以先解析 MIDLINE_DIST 再判空）
DIST="${MIDLINE_DIST:-$OUT}"
if [ -e "$DIST" ] && [ ! -d "$DIST" ]; then
  die "输出路径 $DIST 被一个同名文件占着 ⇒ 那不是本脚本能收拾的目录。"
fi
if [ -d "$DIST" ] && [ -n "$(ls -A "$DIST")" ]; then
  die "输出目录 $DIST 不是全新的（里面有东西）⇒ 停。清空它，或用 MIDLINE_DIST 换个目录；
不清空就重跑，'产物在'这句话证明不了它是这次编出来的。"
fi
mkdir -p "$DIST"

# ② 缺键不自动生成
if [ ! -f "$KS" ] || [ ! -f "$KS_PASS_FILE" ] || [ ! -f "$KEY_PASS_FILE" ]; then
  die "找不到发布签名键：$KS（库口令 $KS_PASS_FILE，键口令 $KEY_PASS_FILE）。
键不存在时不自动生成——换签名键会让已装机的包无法覆盖升级。
要么把键放回来，要么用 MIDLINE_KEYSTORE / MIDLINE_KEYSTORE_PASS_FILE / MIDLINE_KEY_PASS_FILE 指路，
要么明确换键并同步改本脚本钉住的 KS_CERT_SHA256。"
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cat > "$STAGE/AndroidManifest.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="${PACKAGE}"
    android:versionCode="${VERSION_CODE}" android:versionName="${VERSION}">
    <uses-sdk android:minSdkVersion="21" android:targetSdkVersion="34"/>
    <application android:label="${LABEL}" android:hasCode="false">
        <activity android:name="android.app.NativeActivity"
                  android:label="${LABEL}"
                  android:theme="@style/MidlineNative"
                  android:exported="true"
                  android:screenOrientation="sensorLandscape"
                  android:configChanges="keyboardHidden|orientation|screenSize|screenLayout|uiMode|density|navigation">
            <meta-data android:name="android.app.lib_name" android:value="${LIB_NAME}"/>
            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>
        </activity>
    </application>
</manifest>
EOF

# 挖孔区这一条只许有**一处**声明，且必须声明在**主题**里，不是 manifest 那条属性。
# 这是本帧两次装机量出来的，不是查文档查出来的：manifest 上写 `android:windowLayoutInDisplayCutoutMode=
# "shortEdges"` 那一版，产物里确实存成了枚举整数（`aapt2 dump xmltree` 现读 `windowLayoutInDisplayCutoutMode
# (0x01010586)=1`），可 `dumpsys window windows` 里本壳的 mAttrs 是
# `…sim={adjust=resize forwardNavigation} disableCutout=false…`——**没有** `layoutInDisplayCutoutMode` 这个
# token（同一份 dump 里 com.dragon.read 那行明明白白带着 `layoutInDisplayCutoutMode=shortEdges`），
# 窗口帧 `frame=[Rect(0, 52 - 720, 1600)]`，另有一只 `Letterbox_left_…`(contentSize=52x720) 替我们填那道缝，
# 壳里读到的仍是 `onNativeWindowCreated 1548x720` ⇒ **值没走到 WindowManager**。
# 换成主题这一条：mAttrs 出现 `layoutInDisplayCutoutMode=shortEdges`，`onNativeWindowCreated 1600x720`，
# `onContentRectChanged l=52 t=0 r=1600 b=720`。主题走的是客户端 `Window#generateLayout` 读
# `R.styleable.Window` 的路，LayoutParams 的值本来就在那儿写进去的——manifest 那条留着只会让人以为它在管事，
# 所以摘掉（一处声明，一条路）。
mkdir -p "$STAGE/res/values"
cat > "$STAGE/res/values/theme.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<resources>
    <style name="MidlineNative" parent="@android:style/Theme.Black.NoTitleBar.Fullscreen">
        <item name="android:windowLayoutInDisplayCutoutMode">shortEdges</item>
    </style>
</resources>
EOF

echo "== cargo build --release --target $RUST_TARGET（壳 crate；自本帧起带**一件**第三方：ab_glyph 字体链）"
( cd "$SHELL_DIR" && cargo build --release --target "$RUST_TARGET" )
SO="$SHELL_DIR/target/$RUST_TARGET/release/lib${LIB_NAME}.so"
[ -f "$SO" ] || die "缺产物 $SO"

mkdir -p "$STAGE/lib/$ABI_DIR"
cp "$SO" "$STAGE/lib/$ABI_DIR/lib${LIB_NAME}.so"

echo "== aapt2 compile res/（本帧起包里有**一份**资源：那份主题；manifest 指 @style/MidlineNative，缺它 link 直接失败）"
"$BUILD_TOOLS/aapt2" compile --dir "$STAGE/res" -o "$STAGE/res.zip"

echo "== aapt2 link（含 -A 资产目录：字体走 assets/ 通道，不走 include_bytes!）"
# 资源走**位置参数**而不是 `-R`：`-R` 是 overlay 语义，新造一份 style 会被它当场毙
# （现读：`error: resource style/MidlineNative does not override an existing resource`）。
"$BUILD_TOOLS/aapt2" link -o "$STAGE/unaligned.apk" -I "$PLATFORM" \
  --manifest "$STAGE/AndroidManifest.xml" -A "$ASSETS" "$STAGE/res.zip"

# 只装 lib/，不装 classes.dex（hasCode=false）
( cd "$STAGE" && zip -q -r unaligned.apk "lib" )

echo "== zipalign"
"$BUILD_TOOLS/zipalign" -f -p 4 "$STAGE/unaligned.apk" "$STAGE/aligned.apk"

APK="$DIST/${PACKAGE##*.}-${VERSION}-${ABI_DIR}.apk"
cp "$STAGE/aligned.apk" "$APK"

echo "== apksigner sign（口令走 file:，值不进命令行、不进日志）"
"$BUILD_TOOLS/apksigner" sign --ks "$KS" --ks-key-alias "$KS_ALIAS" \
  --ks-pass "file:$KS_PASS_FILE" --key-pass "file:$KEY_PASS_FILE" "$APK"
"$BUILD_TOOLS/apksigner" verify "$APK"

# ③ 指纹比对：核的是产物里实际落下的证书，不是"我以为用了哪把键"
got="$("$BUILD_TOOLS/apksigner" verify --print-certs "$APK" \
      | sed -n 's/.*certificate SHA-256 digest: //p' | head -1 | tr 'A-Z' 'a-z')"
[ "$got" = "$KS_CERT_SHA256" ] || die "签名指纹与钉住的发布键不符：产物 $got ≠ 预期 $KS_CERT_SHA256"
echo "签名指纹核对通过 $got"

echo "== zip 条目实价（§3.8.5 那笔按 41.35% 代理比的推算，就地换成产物上的现量）"
command -v unzip >/dev/null || die "找不到 unzip：这一格只量**已签名的产物**，没它就只能继续推算。"
# 数据行 8 列：1 Length／2 Method／3 Size／4 压缩比／5 Date／6 Time／7 CRC-32／8 Name。
# 只收 Method 是 Defl／Stored 的行——表头与分隔线**同样**是 8 列，第一版把这两行也当条目打了出来，
# 而 ratio 那格我误取了 $6（Time），读出来满屏 "00:00"。
unzip -v "$APK" | awk '$2 ~ /^(Defl:N|Stored)$/ {printf "   %-38s %9d B → %9d B  %4s\n", $8, $1, $3, $4}'
set -- $(unzip -v "$APK" | awk '$NF=="assets/fonts/ui.otf" {print $1, $3}')
raw="${1:-}"; comp="${2:-}"
[ -n "$raw" ] || die "产物里没有 assets/fonts/ui.otf ⇒ -A 那格没生效，字体没进包（装了也是没字的一只）。"
awk -v r="$raw" -v c="$comp" -v a="$(stat -c%s "$APK")" \
  'BEGIN{printf "   字体条目现量：%d B 未压 → %d B 实存（%.1f%% 的原价）／整只 APK %d B\n", r, c, c*100/r, a}'
# 反向一格：本壳 manifest 写 hasCode="false"，dex 出现就说明有人偷偷加了 Java 面。
unzip -l "$APK" | grep -q 'classes\.dex' && die "产物里出现 classes.dex（本壳不该有 Java）" || true

# 产物回读：主题里那条 item 到底存成了什么。三层判据，缺一不装：
# ① **属性 id 从 -I 的那份 android.jar 现取**（`javap` 读 `android.R$attr`），不手写十六进制、
#    也不从我们的源里抄——"包里的这个 id 就是平台所说的那个 attr"这件事必须有一个**外部证人**。
# ② 值必须是枚举整数 `=1`（shortEdges）。框架类型是 TYPE_ENUM、系统取的是 `getInt`，所以
#    "aapt2 把 `shortEdges` 当字符串塞进包"与"塞成整数 1"两种产物，名字都在、装机都不报错——
#    **只有后者画面会到边**。前者是静默失效，读值才咬得住。
# ③ parent 必须仍是那只全屏主题：换主题等于把整套餐具（标题栏、全屏位、状态栏）一起换掉，
#    指错 parent 时画面会安静地变成别的样子，而这一帧问的恰好是"到不到边"。
command -v javap >/dev/null || die "找不到 javap：属性 id 就没有外部证人，这一格不许按我抄的数走。"
CUTOUT_DEC="$(javap -constants -cp "$PLATFORM" 'android.R$attr' \
             | sed -n 's/.*windowLayoutInDisplayCutoutMode = \([0-9]*\);/\1/p' | head -1)"
[ -n "$CUTOUT_DEC" ] || die "从 $PLATFORM 里读不出 windowLayoutInDisplayCutoutMode 的值 ⇒ 换 android.jar 或改名了，别按旧 id 判。"
CUTOUT_ID="$(printf '0x%08x' "$CUTOUT_DEC")"
RES_DUMP="$("$BUILD_TOOLS/aapt2" dump resources "$APK" 2>/dev/null | grep -A6 'style/MidlineNative' || true)"
[ -n "$RES_DUMP" ] || die "产物 resources.arsc 里没有 style/MidlineNative ⇒ manifest 指的 @style 是个空名，装机直接起不来。"
printf '%s\n' "$RES_DUMP" | grep -q "${CUTOUT_ID}=1" \
  || die "MidlineNative 里没有 ${CUTOUT_ID}=1（现读：$(printf '%s' "$RES_DUMP" | tr '\n' ' ')）⇒ 系统 getInt 会退回默认值，画面照样不到边。"
printf '%s\n' "$RES_DUMP" | grep -q 'parent=0x0103000a' \
  || die "MidlineNative 的 parent 不是 0x0103000a（Theme.Black.NoTitleBar.Fullscreen）⇒ 换主题换了别的东西。"
echo "   主题那条落下：style/MidlineNative 里 $CUTOUT_ID=1（id 现取自 $PLATFORM），parent 仍是那只全屏主题"

abis="$("$BUILD_TOOLS/aapt2" dump badging "$APK" 2>/dev/null | sed -n "s/^native-code: //p" | tr -d "'")"
echo "OK: $APK  [${abis:-无 native-code 声明}]  $(stat -c %s "$APK") 字节"
