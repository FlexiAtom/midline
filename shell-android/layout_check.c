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
/* 挖孔区那一格：`onContentRectChanged` 递进来的、`ANativeWindow_lock` 回读的 dirty rect 都是
 * **同一个 `ARect`**（`android/rect.h`，四个 int32、无填充；语义 left 含、right 不含）。
 * entry.rs 里那份叫 `Area`，是照头**独立**抄的第二份——两边各自自洽就能一起错，所以这里让真头再开口一次。
 * 本帧问的正是"系统给的那条 52 到底是什么形状、在哪一边"，而唯一的现读证人就是这个结构。 */
#include <android/rect.h>

_Static_assert(sizeof(ARect) == 16, "ARect size");
_Static_assert(offsetof(ARect, left) == 0, "rect left");
_Static_assert(offsetof(ARect, top) == 4, "rect top");
_Static_assert(offsetof(ARect, right) == 8, "rect right");
_Static_assert(offsetof(ARect, bottom) == 12, "rect bottom");

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

/* ── 输入运输面（第十八步·其四）──────────────────────────────────────────────
 * entry.rs 这一帧新抄了 12 个函数与 20 来个数字。三个句柄 `AInputQueue`／`ALooper`／
 * `AInputEvent` 在头里都是**不完整类型**（`typedef struct AInputQueue AInputQueue;`），
 * 所以这里没有 `offsetof` 可量——能量的两样是：数字本身的值，和每条原型的**形状**
 * （返回类型＋参数表，整体比一次）。Rust 侧一律写成 `*mut c_void`，那是"不透明指针按指针传"
 * 的约定，宽度由下面那三行 sizeof 钉住；形状由 `typeof(&fn)` 钉住，二者合起来才覆盖
 * "我抄的那条声明"这件事。
 * 为什么必须在这格开口：`AINPUT_EVENT_TYPE_MOTION` 网上教程常写成 1（那是 KEY），
 * 抄错的话 `getType` 永远不等于我那个常数，症状是"点了没反应"——编译、链接、启动全绿，
 * 只有真机上什么都不动。那是最难自己发现的一种坏，而这里一行就能拦住。 */
#include <android/input.h>
#include <android/looper.h>

/* 三句柄都是不透明指针：本壳只转交、不解引用，所以尺寸唯一要紧（8 字节＝上面那条 aarch64 前提）。 */
_Static_assert(sizeof(AInputQueue*) == 8, "AInputQueue handle width");
_Static_assert(sizeof(ALooper*) == 8, "ALooper handle width");
_Static_assert(sizeof(AInputEvent*) == 8, "AInputEvent handle width");

/* 事件类型：1=KEY、2=MOTION。entry.rs 里 `EVENT_TYPE_MOTION` 那个 2 就靠这三行作证。 */
_Static_assert(AINPUT_EVENT_TYPE_KEY == 1, "event type KEY");
_Static_assert(AINPUT_EVENT_TYPE_MOTION == 2, "event type MOTION");
_Static_assert(AINPUT_EVENT_TYPE_FOCUS == 3, "event type FOCUS");
_Static_assert(AINPUT_EVENT_TYPE_CAPTURE == 4, "event type CAPTURE");
_Static_assert(AINPUT_EVENT_TYPE_DRAG == 5, "event type DRAG");
_Static_assert(AINPUT_EVENT_TYPE_TOUCH_MODE == 6, "event type TOUCH_MODE");

/* action 的低位掩码与那十三档。`ACTION_MASK` 抄成 0xff00 会把每条事件都读成 action 0，
 * 抄成 0xff00 之外的任何值都会让 DOWN/UP 分不清——这两行是本帧"按下/抬起"全部的证人。 */
_Static_assert(AMOTION_EVENT_ACTION_MASK == 0xff, "action mask");
_Static_assert(AMOTION_EVENT_ACTION_POINTER_INDEX_MASK == 0xff00, "pointer index mask");
_Static_assert(AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT == 8, "pointer index shift");
_Static_assert(AMOTION_EVENT_ACTION_DOWN == 0, "action DOWN");
_Static_assert(AMOTION_EVENT_ACTION_UP == 1, "action UP");
_Static_assert(AMOTION_EVENT_ACTION_MOVE == 2, "action MOVE");
_Static_assert(AMOTION_EVENT_ACTION_CANCEL == 3, "action CANCEL");
_Static_assert(AMOTION_EVENT_ACTION_OUTSIDE == 4, "action OUTSIDE");
_Static_assert(AMOTION_EVENT_ACTION_POINTER_DOWN == 5, "action POINTER_DOWN");
_Static_assert(AMOTION_EVENT_ACTION_POINTER_UP == 6, "action POINTER_UP");
_Static_assert(AMOTION_EVENT_ACTION_HOVER_MOVE == 7, "action HOVER_MOVE");
_Static_assert(AMOTION_EVENT_ACTION_SCROLL == 8, "action SCROLL");
_Static_assert(AMOTION_EVENT_ACTION_HOVER_ENTER == 9, "action HOVER_ENTER");
_Static_assert(AMOTION_EVENT_ACTION_HOVER_EXIT == 10, "action HOVER_EXIT");
_Static_assert(AMOTION_EVENT_ACTION_BUTTON_PRESS == 11, "action BUTTON_PRESS");
_Static_assert(AMOTION_EVENT_ACTION_BUTTON_RELEASE == 12, "action BUTTON_RELEASE");

/* looper 那四个负数与两个 1<<0：`POLL_ERROR` 若与 `POLL_WAKE` 撞档，日志里"looper 死了"
 * 那句就会在每次正常唤醒时打出来（那是假红）；`ID_INPUT` 则受头里那句「must be >= 0」约束，
 * 这里把它与 `ALOOPER_POLL_CALLBACK` 的负数档分开钉。 */
_Static_assert(ALOOPER_PREPARE_ALLOW_NON_CALLBACKS == (1 << 0), "prepare opts");
_Static_assert(ALOOPER_EVENT_INPUT == (1 << 0), "poll event INPUT");
_Static_assert(ALOOPER_POLL_WAKE == -1, "poll WAKE");
_Static_assert(ALOOPER_POLL_CALLBACK == -2, "poll CALLBACK");
_Static_assert(ALOOPER_POLL_TIMEOUT == -3, "poll TIMEOUT");
_Static_assert(ALOOPER_POLL_ERROR == -4, "poll ERROR");

/* 回调那一支的**类型**：本壳传的是 `None`（走 ident 轮询），所以这条现在没人调；
 * 但它钉的是"我那份 Rust fn-pointer 声明长得对不对"——下一帧若要改成回调式，
 * 这里已经有一条对得上的形状可用，不必再抄一遍。 */
_Static_assert(__builtin_types_compatible_p(ALooper_callbackFunc, int (*)(int, int, void*)),
               "ALooper_callbackFunc is int(int fd, int events, void* data)");

/* 12 条原型整体对质：返回类型＋参数表一次比完。写错任何一处（比如把 `getEvent` 的第二参
 * 抄成 `AInputEvent*` 而不是 `AInputEvent**`，或把 `pollOnce` 的返回从 `int` 记成 `int64_t`）
 * 都在这格编不过。`typeof(&f)` 不求值，所以这仍是纯编译期检查。 */
_Static_assert(__builtin_types_compatible_p(typeof(&AInputQueue_attachLooper),
               void (*)(AInputQueue*, ALooper*, int, ALooper_callbackFunc, void*)), "attachLooper shape");
_Static_assert(__builtin_types_compatible_p(typeof(&AInputQueue_detachLooper),
               void (*)(AInputQueue*)), "detachLooper shape");
_Static_assert(__builtin_types_compatible_p(typeof(&AInputQueue_hasEvents),
               int32_t (*)(AInputQueue*)), "hasEvents shape");
_Static_assert(__builtin_types_compatible_p(typeof(&AInputQueue_getEvent),
               int32_t (*)(AInputQueue*, AInputEvent**)), "getEvent shape");
_Static_assert(__builtin_types_compatible_p(typeof(&AInputQueue_preDispatchEvent),
               int32_t (*)(AInputQueue*, AInputEvent*)), "preDispatchEvent shape");
_Static_assert(__builtin_types_compatible_p(typeof(&AInputQueue_finishEvent),
               void (*)(AInputQueue*, AInputEvent*, int)), "finishEvent shape");
_Static_assert(__builtin_types_compatible_p(typeof(&ALooper_prepare),
               ALooper* (*)(int)), "ALooper_prepare shape");
_Static_assert(__builtin_types_compatible_p(typeof(&ALooper_wake),
               void (*)(ALooper*)), "ALooper_wake shape");
_Static_assert(__builtin_types_compatible_p(typeof(&ALooper_pollOnce),
               int (*)(int, int*, int*, void**)), "ALooper_pollOnce shape");
_Static_assert(__builtin_types_compatible_p(typeof(&AInputEvent_getType),
               int32_t (*)(const AInputEvent*)), "getType shape");
_Static_assert(__builtin_types_compatible_p(typeof(&AMotionEvent_getAction),
               int32_t (*)(const AInputEvent*)), "getAction shape");
_Static_assert(__builtin_types_compatible_p(typeof(&AMotionEvent_getPointerCount),
               size_t (*)(const AInputEvent*)), "getPointerCount shape");
/* 坐标是 `float` 且**带 pointer_index**：这条是本帧最容易静默失手的一格。把 `getX` 声明成
 * 无参，Rust 侧照样能编过（extern 块没人校参数），读回来的却是按 vararg 那档取的寄存器值。 */
_Static_assert(__builtin_types_compatible_p(typeof(&AMotionEvent_getX),
               float (*)(const AInputEvent*, size_t)), "getX shape");
_Static_assert(__builtin_types_compatible_p(typeof(&AMotionEvent_getY),
               float (*)(const AInputEvent*, size_t)), "getY shape");
