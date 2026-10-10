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
