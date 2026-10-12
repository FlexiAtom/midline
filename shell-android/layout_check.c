/* midline - 《中线》安卓薄壳的布局尺（第二道）
 * Copyright (C) 2026 FlexiAtom
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */
/* 拿 **真头** 校 entry.rs 里那张数字表。它不链进任何产物，也不执行——只被
 * scripts/shell-export-surface.sh 用 NDK clang `-c` 编一次：编过＝布局对得上，
 * 编不过＝NDK 头里的偏移与我抄写的不一致（或换了 ABI）。
 *
 * 为什么要有这一道：entry.rs 那批 offset_of! 断言只能证明"我自己的声明自洽"，
 * 换序、漏字段一样能全绿。真正的外部证人只有 NDK 头本身。
 */
#include <android/native_activity.h>
#include <stddef.h>

#if !defined(__aarch64__)
#error "this layout table is for the aarch64 ABI (8-byte pointers); armv7/x86 need their own table"
#endif

_Static_assert(sizeof(ANativeActivityCallbacks) == 128, "ANativeActivityCallbacks size");
_Static_assert(sizeof(ANativeActivity) == 80, "ANativeActivity size");
_Static_assert(offsetof(ANativeActivity, callbacks) == 0, "callbacks");
_Static_assert(offsetof(ANativeActivity, vm) == 8, "vm");
_Static_assert(offsetof(ANativeActivity, env) == 16, "env");
_Static_assert(offsetof(ANativeActivity, clazz) == 24, "clazz");
_Static_assert(offsetof(ANativeActivity, internalDataPath) == 32, "internalDataPath");
_Static_assert(offsetof(ANativeActivity, externalDataPath) == 40, "externalDataPath");
_Static_assert(offsetof(ANativeActivity, sdkVersion) == 48, "sdkVersion");
_Static_assert(offsetof(ANativeActivity, instance) == 56, "instance");
_Static_assert(offsetof(ANativeActivity, assetManager) == 64, "assetManager");
_Static_assert(offsetof(ANativeActivity, obbPath) == 72, "obbPath");
_Static_assert(offsetof(ANativeActivityCallbacks, onStart) == 0, "onStart");
_Static_assert(offsetof(ANativeActivityCallbacks, onResume) == 8, "onResume");
_Static_assert(offsetof(ANativeActivityCallbacks, onSaveInstanceState) == 16, "onSaveInstanceState");
_Static_assert(offsetof(ANativeActivityCallbacks, onPause) == 24, "onPause");
_Static_assert(offsetof(ANativeActivityCallbacks, onStop) == 32, "onStop");
_Static_assert(offsetof(ANativeActivityCallbacks, onDestroy) == 40, "onDestroy");
_Static_assert(offsetof(ANativeActivityCallbacks, onWindowFocusChanged) == 48, "onWindowFocusChanged");
_Static_assert(offsetof(ANativeActivityCallbacks, onNativeWindowCreated) == 56, "onNativeWindowCreated");
_Static_assert(offsetof(ANativeActivityCallbacks, onNativeWindowResized) == 64, "onNativeWindowResized");
_Static_assert(offsetof(ANativeActivityCallbacks, onNativeWindowRedrawNeeded) == 72, "onNativeWindowRedrawNeeded");
_Static_assert(offsetof(ANativeActivityCallbacks, onNativeWindowDestroyed) == 80, "onNativeWindowDestroyed");
_Static_assert(offsetof(ANativeActivityCallbacks, onInputQueueCreated) == 88, "onInputQueueCreated");
_Static_assert(offsetof(ANativeActivityCallbacks, onInputQueueDestroyed) == 96, "onInputQueueDestroyed");
_Static_assert(offsetof(ANativeActivityCallbacks, onContentRectChanged) == 104, "onContentRectChanged");
_Static_assert(offsetof(ANativeActivityCallbacks, onConfigurationChanged) == 112, "onConfigurationChanged");
_Static_assert(offsetof(ANativeActivityCallbacks, onLowMemory) == 120, "onLowMemory");

/* 呈现面这一格新抄的两张表：`ANativeWindow_Buffer` 的偏移、像素格式与资产读法的枚举值。
 * 它们与 entry.rs 里那批 offset_of!/const 断言共用同一张数字表——NDK 头改了而这边没改，
 * 或这边抄错了而那边自洽，都会有一边编不过。
 * 像素枚举在 NDK 里住 `android/hardware_buffer.h`；`system/graphics.h` 是平台源码那侧的路径，
 * NDK sysroot **没有它**（第一版按平台源码抄了 include，结果整格根本没编译，见 §6 那次红）。 */
#include <android/native_window.h>
#include <android/asset_manager.h>
#include <android/hardware_buffer.h>

/* 字段序 width,height,stride,format,bits,reserved[6]：四个整数先排满 16 字节，指针自然落在 16。
 * 上一版把 bits 记成 @8（即"指针在两个整数之前"），两侧一起自洽、一起错——**外部证人只有一次机会，
 * 抄错的时候它必须开口**，所以这几行的价值就在它跟 entry.rs 那份是各自独立抄的。 */
_Static_assert(sizeof(ANativeWindow_Buffer) == 48, "ANativeWindow_Buffer size");
_Static_assert(offsetof(ANativeWindow_Buffer, width) == 0, "buffer width");
_Static_assert(offsetof(ANativeWindow_Buffer, height) == 4, "buffer height");
_Static_assert(offsetof(ANativeWindow_Buffer, stride) == 8, "buffer stride");
_Static_assert(offsetof(ANativeWindow_Buffer, format) == 12, "buffer format");
_Static_assert(offsetof(ANativeWindow_Buffer, bits) == 16, "buffer bits");
_Static_assert(offsetof(ANativeWindow_Buffer, reserved) == 24, "buffer reserved");
/* NDK 的像素枚举与 legacy HAL 的旧编号不是一套（这里 3=RGB_888、4=RGB_565）。上一帧按旧编号
 * 把真机报的 fmt=4 当 4 字节写，整块缓冲写花——那三格数字就是那次的证人。 */
_Static_assert(AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM == 1, "RGBA_8888");
_Static_assert(AHARDWAREBUFFER_FORMAT_R8G8B8X8_UNORM == 2, "RGBX_8888");
_Static_assert(AHARDWAREBUFFER_FORMAT_R8G8B8_UNORM == 3, "RGB_888");
_Static_assert(AHARDWAREBUFFER_FORMAT_R5G6B5_UNORM == 4, "RGB_565");
_Static_assert(AASSET_MODE_BUFFER == 3, "AASSET_MODE_BUFFER");
/* 两条返回宽度：头里 `AAsset_getLength` 写的是 **off_t**（本机 NDK 现量 8 字节），`AAsset_read`
 * 写的是 `int`。entry.rs 那两行分别声明成 `c_long`／`c_int`——第一版把 getLength 抄成了 `c_int`，
 * 那次这条开口了，所以这格的全部意义就在这。`typeof` 只取类型不求值。 */
_Static_assert(sizeof(off_t) == 8, "off_t width on this ABI");
_Static_assert(__builtin_types_compatible_p(typeof(AAsset_getLength(NULL)), off_t), "getLength returns off_t");
_Static_assert(__builtin_types_compatible_p(typeof(AAsset_read(NULL, NULL, 0)), int), "read returns int");
