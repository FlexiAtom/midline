// midline - 《中线》安卓薄壳（丁′：壳也用 Rust，源码级融合）
// Copyright (C) 2026 FlexiAtom
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//! 平台入口胶水：`ANativeActivity_onCreate` 与它的生命周期回调表。
//!
//! **这一段是本 crate 唯一碰 libandroid/liblog 的地方**，也是唯一写 `unsafe extern` 的地方。
//! 它不引任何 crates.io 绑定（`ndk-sys`/`android-activity` 都不要），所以那两个结构体的
//! 内存布局是我从 NDK 头抄写来的——抄写会错，所以抄写之外另立两道尺：
//! - 本文件末尾那批 `offset_of!` 断言：钉住**我这份声明**的偏移，任何字段增删/换序当场编不过；
//! - `shell-android/layout_check.c`：拿 NDK clang 对**真头**做 `_Static_assert`，用同一张数字表。
//! 两道尺共用一张表，所以"我以为的布局"与"平台实际的布局"一旦分叉必有一边红。
//!
//! 本帧**不画任何东西、不读任何输入**。它只证明三件事：融合 crate 能编、入口符号在导出表里、
//! 生命周期回调被平台找到并回来过。呈现面要等人给 2D 设计，落盘时序还挂在 P8/P9 未测。

use std::os::raw::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};

/// NDK `ANativeActivityCallbacks`——16 个函数指针，框架已预置为 NULL，**只能就地填字段**。
/// 头里明写「The callbacks pointer itself here should not be changed; it is allocated and
/// managed for you by the framework」，所以换成我们自己分配的那张表是错的用法。
#[repr(C)]
#[allow(non_snake_case, reason = "字段名逐字对齐 NDK 头，便于对照 layout_check.c")]
pub struct ANativeActivityCallbacks {
    pub onStart: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub onResume: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub onSaveInstanceState:
        Option<unsafe extern "C" fn(*mut ANativeActivity, *mut usize) -> *mut c_void>,
    pub onPause: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub onStop: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub onDestroy: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub onWindowFocusChanged: Option<unsafe extern "C" fn(*mut ANativeActivity, c_int)>,
    pub onNativeWindowCreated: Option<unsafe extern "C" fn(*mut ANativeActivity, *mut c_void)>,
    pub onNativeWindowResized: Option<unsafe extern "C" fn(*mut ANativeActivity, *mut c_void)>,
    pub onNativeWindowRedrawNeeded: Option<unsafe extern "C" fn(*mut ANativeActivity, *mut c_void)>,
    pub onNativeWindowDestroyed: Option<unsafe extern "C" fn(*mut ANativeActivity, *mut c_void)>,
    pub onInputQueueCreated: Option<unsafe extern "C" fn(*mut ANativeActivity, *mut c_void)>,
    pub onInputQueueDestroyed: Option<unsafe extern "C" fn(*mut ANativeActivity, *mut c_void)>,
    pub onContentRectChanged: Option<unsafe extern "C" fn(*mut ANativeActivity, *const c_void)>,
    pub onConfigurationChanged: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
    pub onLowMemory: Option<unsafe extern "C" fn(*mut ANativeActivity)>,
}

/// NDK `ANativeActivity`。JavaVM*/JNIEnv*/jobject/AAssetManager* 一律按 `*mut c_void` 抄——
/// 本壳不碰 JNI，把跨语言句柄收窄成"不可解引用的裸指针"是刻意的：露一个 `JNIEnv` 出来
/// 就等于在壳里开出第二条对外契约，而防绕过那条立场挡的正是这类把手。
#[repr(C)]
#[allow(non_snake_case, reason = "同上：字段名对齐 NDK 头")]
pub struct ANativeActivity {
    pub callbacks: *mut ANativeActivityCallbacks,
    pub vm: *mut c_void,
    pub env: *mut c_void,
    pub clazz: *mut c_void,
    pub internalDataPath: *const c_char,
    pub externalDataPath: *const c_char,
    pub sdkVersion: i32,
    pub instance: *mut c_void,
    pub assetManager: *mut c_void,
    pub obbPath: *const c_char,
}

/// 平台侧的 `ANativeWindow`/`AInputQueue` 在本壳里始终是裸指针：只转交、不解引用（除下面
/// 取宽高那一处，而那两法也在同一个 extern 块里，边界不散）。
static WINDOW: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static INPUT_QUEUE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

#[cfg(target_os = "android")]
mod ndk {
    use std::os::raw::{c_char, c_int, c_void};

    #[allow(dead_code, reason = "声明按 NDK 头成组写，本帧只用到其中三条")]
    unsafe extern "C" {
        pub fn __android_log_print(prio: c_int, tag: *const c_char, fmt: *const c_char, ...) -> c_int;
        /// 这两个调用同时也是 `-landroid` 的真凭据：不引用它，链接器会把 libandroid 按
        /// `--as-needed` 丢掉，NEEDED 面就量不出壳真的挂上了平台库。
        pub fn ANativeWindow_getWidth(window: *mut c_void) -> c_int;
        pub fn ANativeWindow_getHeight(window: *mut c_void) -> c_int;
    }
}

#[cfg(target_os = "android")]
const LOG_INFO: c_int = 4;
#[cfg(target_os = "android")]
const LOG_ERROR: c_int = 6;
#[cfg(target_os = "android")]
const TAG: &std::ffi::CStr = c"midline";

/// 只收静态 CStr：走 `%s` 就得自己造 `CString`，那是每次回调一次堆分配，换个日志不值得。
#[cfg(target_os = "android")]
fn note(msg: &std::ffi::CStr) {
    unsafe { ndk::__android_log_print(LOG_INFO, TAG.as_ptr(), msg.as_ptr()) };
}

#[cfg(target_os = "android")]
fn note_size(msg: &std::ffi::CStr, w: c_int, h: c_int) {
    unsafe { ndk::__android_log_print(LOG_INFO, TAG.as_ptr(), msg.as_ptr(), w, h) };
}

#[cfg(target_os = "android")]
fn warn(msg: &std::ffi::CStr) {
    unsafe { ndk::__android_log_print(LOG_ERROR, TAG.as_ptr(), msg.as_ptr()) };
}

#[cfg(not(target_os = "android"))]
fn note(msg: &std::ffi::CStr) {
    eprintln!("[midline-shell] {}", msg.to_string_lossy());
}

#[cfg(not(target_os = "android"))]
fn note_size(msg: &std::ffi::CStr, w: c_int, h: c_int) {
    eprintln!("[midline-shell] {} {w}x{h}", msg.to_string_lossy());
}

#[cfg(not(target_os = "android"))]
fn warn(msg: &std::ffi::CStr) {
    eprintln!("[midline-shell:error] {}", msg.to_string_lossy());
}

unsafe extern "C" fn on_start(_activity: *mut ANativeActivity) {
    note(c"onStart");
}

unsafe extern "C" fn on_resume(_activity: *mut ANativeActivity) {
    note(c"onResume");
}

unsafe extern "C" fn on_pause(_activity: *mut ANativeActivity) {
    note(c"onPause");
}

unsafe extern "C" fn on_stop(_activity: *mut ANativeActivity) {
    note(c"onStop");
}

unsafe extern "C" fn on_destroy(_activity: *mut ANativeActivity) {
    WINDOW.store(std::ptr::null_mut(), Ordering::Release);
    INPUT_QUEUE.store(std::ptr::null_mut(), Ordering::Release);
    note(c"onDestroy");
}

unsafe extern "C" fn on_window_focus_changed(_activity: *mut ANativeActivity, has_focus: c_int) {
    note(if has_focus == 0 { c"onWindowFocusChanged: lost" } else { c"onWindowFocusChanged: gained" });
}

/// 框架给的实例状态由**框架** `free`，所以只能 `malloc` 出来还它。本壳此刻没有可持久化的
/// 实例状态，交回 `(NULL, 0)` 是如实答复，不是搪塞——`native_app_glue` 在无 bundle 时也交回空。
unsafe extern "C" fn on_save_instance_state(
    _activity: *mut ANativeActivity,
    out_size: *mut usize,
) -> *mut c_void {
    if !out_size.is_null() {
        unsafe { *out_size = 0 };
    }
    std::ptr::null_mut()
}

unsafe extern "C" fn on_native_window_created(
    _activity: *mut ANativeActivity,
    window: *mut c_void,
) {
    WINDOW.store(window, Ordering::Release);
    #[cfg(target_os = "android")]
    if !window.is_null() {
        let (w, h) =
            unsafe { (ndk::ANativeWindow_getWidth(window), ndk::ANativeWindow_getHeight(window)) };
        note_size(c"onNativeWindowCreated %dx%d", w, h);
    }
    #[cfg(not(target_os = "android"))]
    note(c"onNativeWindowCreated");
}

unsafe extern "C" fn on_native_window_resized(
    _activity: *mut ANativeActivity,
    window: *mut c_void,
) {
    WINDOW.store(window, Ordering::Release);
    note(c"onNativeWindowResized");
}

unsafe extern "C" fn on_native_window_redraw_needed(
    _activity: *mut ANativeActivity,
    _window: *mut c_void,
) {
    note(c"onNativeWindowRedrawNeeded");
}

/// 头里的硬约束：「You MUST ensure that you do not touch the window object after returning」——
/// 所以清空必须发生在返回之前，且顺序是 Release（后续读者不能看到已失效的窗口）。
unsafe extern "C" fn on_native_window_destroyed(
    _activity: *mut ANativeActivity,
    _window: *mut c_void,
) {
    WINDOW.store(std::ptr::null_mut(), Ordering::Release);
    note(c"onNativeWindowDestroyed");
}

unsafe extern "C" fn on_input_queue_created(
    _activity: *mut ANativeActivity,
    queue: *mut c_void,
) {
    INPUT_QUEUE.store(queue, Ordering::Release);
    note(c"onInputQueueCreated");
}

unsafe extern "C" fn on_input_queue_destroyed(
    _activity: *mut ANativeActivity,
    _queue: *mut c_void,
) {
    INPUT_QUEUE.store(std::ptr::null_mut(), Ordering::Release);
    note(c"onInputQueueDestroyed");
}

unsafe extern "C" fn on_content_rect_changed(
    _activity: *mut ANativeActivity,
    _rect: *const c_void,
) {
    note(c"onContentRectChanged");
}

unsafe extern "C" fn on_configuration_changed(_activity: *mut ANativeActivity) {
    note(c"onConfigurationChanged");
}

unsafe extern "C" fn on_low_memory(_activity: *mut ANativeActivity) {
    note(c"onLowMemory");
}

/// 平台加载器 `dlsym` 找的就是这个名字，所以它是本 crate **唯一**该出现在导出表里的符号。
/// 由 `scripts/shell-export-surface.sh` 按符号表量，不靠这里的自述。
#[unsafe(no_mangle)]
pub extern "C" fn ANativeActivity_onCreate(
    activity: *mut ANativeActivity,
    saved_state: *const c_void,
    saved_state_size: usize,
) {
    if activity.is_null() {
        return; // 平台契约保证非空；真进来是空，能做的只有不开这张壳
    }
    let callbacks = unsafe { (*activity).callbacks };
    if callbacks.is_null() {
        warn(c"ANativeActivity.callbacks is NULL — refusing to install lifecycle hooks");
        return;
    }
    let callbacks = unsafe { &mut *callbacks };
    callbacks.onStart = Some(on_start);
    callbacks.onResume = Some(on_resume);
    callbacks.onSaveInstanceState = Some(on_save_instance_state);
    callbacks.onPause = Some(on_pause);
    callbacks.onStop = Some(on_stop);
    callbacks.onDestroy = Some(on_destroy);
    callbacks.onWindowFocusChanged = Some(on_window_focus_changed);
    callbacks.onNativeWindowCreated = Some(on_native_window_created);
    callbacks.onNativeWindowResized = Some(on_native_window_resized);
    callbacks.onNativeWindowRedrawNeeded = Some(on_native_window_redraw_needed);
    callbacks.onNativeWindowDestroyed = Some(on_native_window_destroyed);
    callbacks.onInputQueueCreated = Some(on_input_queue_created);
    callbacks.onInputQueueDestroyed = Some(on_input_queue_destroyed);
    callbacks.onContentRectChanged = Some(on_content_rect_changed);
    callbacks.onConfigurationChanged = Some(on_configuration_changed);
    callbacks.onLowMemory = Some(on_low_memory);

    note(c"ANativeActivity_onCreate: lifecycle hooks installed");
    if !saved_state.is_null() && saved_state_size != 0 {
        // 本壳还没有可恢复的实例状态；这条留成日志而不是断言，是为了真机上第一次进程重建
        // 能被看见——看见了才知道该在这里接哪一层（那是 P8 落盘对撞要回答的问题）。
        note(c"saved state handed in but nothing restores it yet");
    }
}

// ── 布局尺（第一道）：钉住我这份声明的偏移与大小 ─────────────────────────────
// 数字表与 layout_check.c 共用，出处是 NDK r29 的 `android/native_activity.h`，
// 由 aarch64 的 clang 以 `-S -O2` 反读 `offsetof`/`sizeof` 常量得到（不是从文档抄的）。
const _: () = assert!(std::mem::size_of::<ANativeActivityCallbacks>() == 128);
const _: () = assert!(std::mem::size_of::<ANativeActivity>() == 80);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, callbacks) == 0);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, vm) == 8);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, env) == 16);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, clazz) == 24);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, internalDataPath) == 32);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, externalDataPath) == 40);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, sdkVersion) == 48);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, instance) == 56);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, assetManager) == 64);
const _: () = assert!(std::mem::offset_of!(ANativeActivity, obbPath) == 72);
const _: () = assert!(std::mem::offset_of!(ANativeActivityCallbacks, onStart) == 0);
const _: () = assert!(std::mem::offset_of!(ANativeActivityCallbacks, onSaveInstanceState) == 16);
const _: () = assert!(std::mem::offset_of!(ANativeActivityCallbacks, onNativeWindowCreated) == 56);
const _: () = assert!(std::mem::offset_of!(ANativeActivityCallbacks, onLowMemory) == 120);
