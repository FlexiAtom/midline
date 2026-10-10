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
                  android:theme="@android:style/Theme.Black.NoTitleBar.Fullscreen"
                  android:exported="true"
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

echo "== cargo build --release --target $RUST_TARGET（壳 crate，零第三方依赖）"
( cd "$SHELL_DIR" && cargo build --release --target "$RUST_TARGET" )
SO="$SHELL_DIR/target/$RUST_TARGET/release/lib${LIB_NAME}.so"
[ -f "$SO" ] || die "缺产物 $SO"

mkdir -p "$STAGE/lib/$ABI_DIR"
cp "$SO" "$STAGE/lib/$ABI_DIR/lib${LIB_NAME}.so"

echo "== aapt2 link"
"$BUILD_TOOLS/aapt2" link -o "$STAGE/unaligned.apk" -I "$PLATFORM" \
  --manifest "$STAGE/AndroidManifest.xml"

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

abis="$("$BUILD_TOOLS/aapt2" dump badging "$APK" 2>/dev/null | sed -n "s/^native-code: //p" | tr -d "'")"
echo "OK: $APK  [${abis:-无 native-code 声明}]  $(stat -c %s "$APK") 字节"
