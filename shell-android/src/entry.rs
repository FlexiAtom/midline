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
//!
//! 两道尺共用一张表，所以"我以为的布局"与"平台实际的布局"一旦分叉必有一边红。
//!
//! 本帧起它多一件事：**把一帧真的画到屏上**。呈现走的是自写 `ANativeWindow_Buffer` 那条
//! （价目表 ① 路，本档 §3.8.1 已证五区纯色逐字节到屏），盘面由 `frame.rs` 栅格、字形由 `text.rs`
//! 栅格，字体字节从 APK 资产通道读入（`AAssetManager`），所以壳仍然零 dex、零 Java、零第三方平台绑定。

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

/// 平台侧的 `AInputQueue` 在本壳里始终是裸指针：只转交、不解引用。`ANativeWindow` 从本帧起
/// 会被解引用（`ANativeWindow_lock` 写它给的缓冲），但**结构体本身仍是平台所有的**——我们只
/// 按它给的 `bits/stride/format` 往那块内存里写字节，不 own、不 free。
static WINDOW: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// 活队列集。**从前这里是一格** `AtomicPtr`：create 直接 `store` 覆盖、destroy 无条件清空。
/// 2026-10-13 那个假设被真机量掉了：`am start -n` 造出同进程第二个实例时 `created` 2 条、
/// `input attach` 2 条、live 通道 2 条**并存**；BACK 收掉顶层之后下面那条还挂在派发器里没人排空，
/// 系统自己给它判了刑——`Waited 10000ms for MotionEvent` ＋ `ANR in Window{f30f124 …}`。
/// 一格装不下两条并存，这就是全部改动的理由（不是"多几条更气派"）。
///
/// 槽位开 4：观测到的上限是 2，留一倍余量。**塞不下不假装塞得下**——`Insert::Full` 那条日志
/// 一响就是"又有一条队列没被记账"，正是 ANR 那一格的形状。
/// 改写权只在主线程（create/destroy/on_destroy 三条回调都跑在那条线程上），输入线程只读；
/// 这一条不是风格，是集内不丢更新的唯一前提。
const LIVE_QUEUE_SLOTS: usize = 4;
static LIVE_QUEUES: [AtomicPtr<c_void>; LIVE_QUEUE_SLOTS] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];

/// 每一条活队列的**属主**——那个 `ANativeActivity*`。它和 `LIVE_QUEUES` 同格对齐，是这条修正的
/// 第二半，不是装饰：`onDestroy` 也是**按实例**来的回调，而 2026-10-14 05:14:14 那次真机量到
/// "队列回调按格摘对了、紧接着 `onDestroy` 一次整集清空又把 A 抹了"（同毫秒两条回调，日志里
/// `input queue removed slot=1 held=1` 之后立刻两句 `input queue emptied mid-drain`）。
/// 头里对 `onDestroy` 只写了「See Java documentation for Activity.onDestroy()」，**没有**承诺
/// "每条队列都会先走一次自己的 destroy 回调"——所以这里不能靠次序假设，只能靠属主。
/// 这一排只有主线程读写（输入线程只读 `LIVE_QUEUES` 那一排），用原子只为让 `static` 合法。
static LIVE_OWNERS: [AtomicPtr<c_void>; LIVE_QUEUE_SLOTS] = [
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
    AtomicPtr::new(std::ptr::null_mut()),
];

/// 呈现互斥。**两条线程不许同时 `lock`/`unlockAndPost` 同一块窗口**：各自锁一块缓冲再各自 post，
/// 症状是"上一帧的尾巴出现在这一帧中间"，而那种画错一闪而过、截屏也抓不住。它顺带把
/// "什么时候允许碰窗口"收成一处：`WINDOW` 现读非空 ＋ 这把锁。
/// 锁中毒（上一次绘制 panic）之后只能继续——那块缓冲没有可恢复的状态，停手反而把屏冻住。
/// 不带 `#[cfg]`：`onNativeWindowDestroyed` 也要拿它（见那支的说明），而那条回调两侧都编。
static PAINT: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// NDK `ANativeWindow_Buffer`（`android/native_window.h`）：**字段序是 width, height, stride,
/// format, bits, reserved[6]**，整数在前、指针在后。抄写风险全在这条顺序上——把 `bits` 提前一位
/// 就变成 bits@8，行步长与格式各挪 8 字节，读到的 stride 其实是 reserved 的第一格。
#[repr(C)]
struct WindowBuffer {
    width: c_int,
    height: c_int,
    stride: c_int,
    format: c_int,
    bits: *mut c_void,
    reserved: [u32; 6],
}

/// NDK `ARect`（`android/rect.h`，`native_window.h`／`native_activity.h` 都把它带进来）：
/// **四个 int32、无填充**，且语义是「left 含、right 不含」——`right-left` 才是宽度。
/// 这一格是给 `onContentRectChanged` 的：系统把"你能画的矩形"通过这个结构交下来，
/// 而本壳从前把那个指针整个忽略（回调登记了，正文只打一句名字）。画面到不到边这件事
/// 只有一个证人，就是它——所以现在是**把四个数打出来**，不是拿它去算任何东西。
#[cfg(target_os = "android")]
#[repr(C)]
struct Area {
    left: c_int,
    top: c_int,
    right: c_int,
    bottom: c_int,
}

/// NDK 的像素格式真值在 `hardware_buffer.h`，**与 legacy HAL 的旧编号不一致**（旧表里 3=565，
/// 这里 3 是 RGB_888、4 才是 RGB_565）。上一帧按旧表把真机报的 `fmt=4` 当 4 字节写，
/// 每个"行"跨了两条真实行，缓冲整个写花（本档 §3.8.1 的失手记录）。
#[cfg(target_os = "android")]
const RGBA_8888: c_int = 1;
#[cfg(target_os = "android")]
const RGBX_8888: c_int = 2;

/// 一帧的字节宽：只认 4 字节/像素的两档。565 与 RGB_888 不在这一格——本壳用
/// `ANativeWindow_setBuffersGeometry` 买 RGBA_8888 且实测 `rc=0`（§3.8.1），买不到就如实
/// 报告"没画"，而不是拿另一套字节宽度去猜。
#[cfg(target_os = "android")]
const fn is_rgba32(format: c_int) -> bool {
    matches!(format, RGBA_8888 | RGBX_8888)
}

#[cfg(target_os = "android")]
mod ndk {
    use super::WindowBuffer;
    use std::os::raw::{c_char, c_int, c_long, c_void};

    #[allow(dead_code, reason = "声明按 NDK 头成组写，未用到的那几条是下一帧的接口面")]
    unsafe extern "C" {
        pub fn __android_log_print(prio: c_int, tag: *const c_char, fmt: *const c_char, ...) -> c_int;
        /// 这两个调用同时也是 `-landroid` 的真凭据：不引用它，链接器会把 libandroid 按
        /// `--as-needed` 丢掉，NEEDED 面就量不出壳真的挂上了平台库。
        pub fn ANativeWindow_getWidth(window: *mut c_void) -> c_int;
        pub fn ANativeWindow_getHeight(window: *mut c_void) -> c_int;
        pub fn ANativeWindow_lock(window: *mut c_void, out_buffer: *mut WindowBuffer, in_out_dirty: *const c_void) -> c_int;
        pub fn ANativeWindow_unlockAndPost(window: *mut c_void) -> c_int;
        /// `width/height` 传 0 ＝ "沿用窗口自己的尺寸，只换格式"。
        pub fn ANativeWindow_setBuffersGeometry(window: *mut c_void, width: c_int, height: c_int, format: c_int) -> c_int;
        /// 字体字节的来路：APK 里的 `assets/`。**AAsset 是不透明句柄**，所以这一组没有布局可抄，
        /// 只有两个数字要尺——`AASSET_MODE_BUFFER` 的枚举值与 `AAsset_getLength` 的**返回宽度**
        /// （见 layout_check.c：头里写的是 `off_t`，本机 NDK 现量 8 字节，所以这边必须是 `c_long`。
        /// 第一版抄成 `c_int`，那是"两侧各自自洽、一起错"的另一格——外部证人开口才拦住的）。
        pub fn AAssetManager_open(mgr: *mut c_void, filename: *const c_char, mode: c_int) -> *mut c_void;
        pub fn AAsset_read(asset: *mut c_void, buf: *mut c_void, count: usize) -> c_int;
        pub fn AAsset_getLength(asset: *mut c_void) -> c_long;
        pub fn AAsset_close(asset: *mut c_void) -> c_int;
        /// **输入运输**（第十八步·其四）。三个句柄 `AInputQueue`／`AInputEvent`／`ALooper` 在 NDK 头里
        /// 都是 `typedef struct X X;` 的**不透明**前置声明——所以这一族没有字段表可抄，也就没有
        /// `offset_of!` 那道尺；要抄的是**数字**（枚举值）与**返回宽度**（`int32_t` vs `size_t` 这类），
        /// 它们逐条在 `layout_check.c` 与真头对质。指针参数一律 `*mut c_void`：与 `ANativeWindow`
        /// 同一口径——壳只按平台给的调用读写它，不 own、不猜形状。
        pub fn AInputQueue_attachLooper(
            queue: *mut c_void,
            looper: *mut c_void,
            ident: c_int,
            callback: Option<unsafe extern "C" fn(fd: c_int, events: c_int, data: *mut c_void) -> c_int>,
            data: *mut c_void,
        );
        pub fn AInputQueue_detachLooper(queue: *mut c_void);
        pub fn AInputQueue_hasEvents(queue: *mut c_void) -> c_int;
        pub fn AInputQueue_getEvent(queue: *mut c_void, out_event: *mut *mut c_void) -> c_int;
        pub fn AInputQueue_preDispatchEvent(queue: *mut c_void, event: *mut c_void) -> c_int;
        pub fn AInputQueue_finishEvent(queue: *mut c_void, event: *mut c_void, handled: c_int);
        pub fn ALooper_prepare(opts: c_int) -> *mut c_void;
        pub fn ALooper_wake(looper: *mut c_void);
        pub fn ALooper_pollOnce(timeout_ms: c_int, out_fd: *mut c_int, out_events: *mut c_int, out_data: *mut *mut c_void) -> c_int;
        pub fn AInputEvent_getType(event: *mut c_void) -> c_int;
        pub fn AMotionEvent_getAction(motion_event: *mut c_void) -> c_int;
        pub fn AMotionEvent_getX(motion_event: *mut c_void, pointer_index: usize) -> f32;
        pub fn AMotionEvent_getY(motion_event: *mut c_void, pointer_index: usize) -> f32;
        pub fn AMotionEvent_getPointerCount(motion_event: *mut c_void) -> usize;
    }

    /// `AAssetManager_open` 的第三种读法：整份进内存。字体就一次读，SEQUENTIAL/RANDOM 都用不上。
    pub const ASSET_MODE_BUFFER: c_int = 3;
}

/// `ALOOPER_PREPARE_ALLOW_NON_CALLBACKS`：本壳**不注册 looper 回调**，而是拿 `ident` 轮询。
/// 走这一档的理由不是口味：回调是在**持有 looper 锁**时被调的，从回调里再碰输入队列或窗口
/// 就是自找死锁；轮询侧则可以自由地去锁 `SCENE` 与 `PAINT`。
#[cfg(target_os = "android")]
const LOOPER_ALLOW_NON_CALLBACKS: c_int = 1 << 0;
/// 本壳自己挑的那个 ident（头里没有预定义的 `LOOPER_ID_INPUT`，只有一个整数，0 是"没有 ident"的
/// 保留值，所以取 1）。它只用来让日志能说出"这次醒来是为输入醒的"，**排空**这件事不依赖它——
/// 头里明写「If you call ALooper_pollOnce() in a loop, you *must* treat all return values as if
/// they also indicate ALOOPER_POLL_WAKE」，照那句话做就不会漏事件。
#[cfg(target_os = "android")]
const ID_INPUT: c_int = 1;
/// 没人叫醒就多久回来看一眼。它同时是"销毁输入队列后最长多久线程会停手"的上界，所以不是随手取的：
/// 真机上手指一下约 60–120ms，50ms 既不糊（一帧的预算还没被它吃掉），也不至于让 detach 等上一秒。
#[cfg(target_os = "android")]
const POLL_TIMEOUT_MS: c_int = 50;
#[cfg(target_os = "android")]
const POLL_WAKE: c_int = -1;
#[cfg(target_os = "android")]
const POLL_CALLBACK: c_int = -2;
#[cfg(target_os = "android")]
const POLL_TIMEOUT: c_int = -3;
#[cfg(target_os = "android")]
const POLL_ERROR: c_int = -4;
/// `AINPUT_EVENT_TYPE_MOTION`：头里 1=KEY、**2=MOTION**（从前有些博客写 1，那是 focus/capture
/// 那几档被加进来之前的老编号）。这一格由 layout_check.c 拿真头钉。
#[cfg(target_os = "android")]
const EVENT_TYPE_MOTION: c_int = 2;
/// action 的低 8 位才是动作，高 8 位是 pointer index（`AMOTION_EVENT_ACTION_POINTER_INDEX_SHIFT`）。
/// 不-mask 的话双指的 `POINTER_UP(1)` 会被读成 `UP|1<<8`，那一支就永远落不进三档里。
#[cfg(target_os = "android")]
const ACTION_MASK: c_int = 0xff;
#[cfg(target_os = "android")]
const ACTION_DOWN: c_int = 0;
#[cfg(target_os = "android")]
const ACTION_UP: c_int = 1;
#[cfg(target_os = "android")]
#[allow(dead_code, reason = "这三档存在的意义是\"它们被如实跳过\"，日志里的 action 值要靠它辨认")]
const ACTION_MOVE: c_int = 2;
#[cfg(target_os = "android")]
const ACTION_CANCEL: c_int = 3;
#[cfg(target_os = "android")]
const ACTION_POINTER_DOWN: c_int = 5;
#[cfg(target_os = "android")]
const ACTION_POINTER_UP: c_int = 6;

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

unsafe extern "C" fn on_destroy(activity: *mut ANativeActivity) {
    WINDOW.store(std::ptr::null_mut(), Ordering::Release);
    // **只撤这个实例名下的那些格**，不许整集清空。头里对 `onDestroy` 只写了「See Java
    // documentation for Activity.onDestroy()」，没承诺"每条队列都先走过自己的 destroy 回调"，
    // 所以这里既不能假设队列还在、也不能假设它已经不在了——按属主摘是唯一站得住的动作。
    // 2026-10-14 05:14:14 那次反证就是这一格：整集清空把邻居实例活着的队列一起抹了，
    // 一毫秒后输入线程两句 `input queue emptied mid-drain`，再十秒系统报 `Waited 10000ms`。
    // `detach` 更不在这里做：每一条队列都在自己的回调里 detach 过了（那条硬约束）。
    let mut set = live_snapshot();
    let mut owners = live_owners();
    let released = queue_set_release_owner(&mut set, &mut owners, activity as *mut c_void);
    unsafe {
        publish_owners(&owners);
        publish_set(&set);
    }
    note(c"onDestroy");
    #[cfg(target_os = "android")]
    unsafe {
        ndk::__android_log_print(
            LOG_INFO,
            TAG.as_ptr(),
            c"onDestroy released=%d held=%d".as_ptr(),
            released as c_int,
            held_count(&set) as c_int,
        )
    };
    #[cfg(not(target_os = "android"))]
    let _ = released;
    #[cfg(target_os = "android")]
    unsafe { wake_input() };
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
    activity: *mut ANativeActivity,
    window: *mut c_void,
) {
    WINDOW.store(window, Ordering::Release);
    #[cfg(target_os = "android")]
    if !window.is_null() {
        let (w, h) =
            unsafe { (ndk::ANativeWindow_getWidth(window), ndk::ANativeWindow_getHeight(window)) };
        note_size(c"onNativeWindowCreated %dx%d", w, h);
        // 格式先买再画：真机默认给的是 16 位档（§3.8.1 现读 `fmt=4 stride=736`），而 text 面
        // 只写 4 字节/像素。买不到就如实说没画，不拿另一套字节宽度去猜。
        let gr = unsafe { ndk::ANativeWindow_setBuffersGeometry(window, 0, 0, RGBA_8888) };
        unsafe {
            ndk::__android_log_print(LOG_INFO, TAG.as_ptr(), c"setBuffersGeometry(RGBA8888) rc=%d".as_ptr(), gr)
        };
        unsafe {
            ensure_scene(activity);
            paint(window, c"frame1");
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = activity;
    #[cfg(not(target_os = "android"))]
    note(c"onNativeWindowCreated");
}

unsafe extern "C" fn on_native_window_resized(
    _activity: *mut ANativeActivity,
    window: *mut c_void,
) {
    WINDOW.store(window, Ordering::Release);
    note(c"onNativeWindowResized");
    #[cfg(target_os = "android")]
    unsafe { paint(window, c"frame-resized") };
}

unsafe extern "C" fn on_native_window_redraw_needed(
    _activity: *mut ANativeActivity,
    window: *mut c_void,
) {
    note(c"onNativeWindowRedrawNeeded");
    #[cfg(not(target_os = "android"))]
    let _ = window;
    #[cfg(target_os = "android")]
    unsafe { paint(window, c"frame-redraw") };
}

/// 头里的硬约束：「You MUST ensure that you do not touch the window object after returning」——
/// 所以清空必须发生在返回之前，且顺序是 Release（后续读者不能看到已失效的窗口）。
/// 清空前要拿到 `PAINT`：从本帧起画这条窗口的不止主线程（输入线程判完一次触摸也会画），而
/// 那条线程可能正锁着一块缓冲在写。锁在这里等最多一帧的时间，换到的是"锁在手 ⇒ 窗口还活着"。
unsafe extern "C" fn on_native_window_destroyed(
    _activity: *mut ANativeActivity,
    _window: *mut c_void,
) {
    let _paint = PAINT.lock().unwrap_or_else(|p| p.into_inner());
    WINDOW.store(std::ptr::null_mut(), Ordering::Release);
    note(c"onNativeWindowDestroyed");
}

unsafe extern "C" fn on_input_queue_created(
    activity: *mut ANativeActivity,
    queue: *mut c_void,
) {
    // 记账走纯谓词 `queue_set_insert`，**不再覆盖**：2026-10-13 那格 `store` 就是把旧那条抹掉，
    // 于是旧那条既没人 detach 也没人排空——系统在 dual 会话里替我们判了（`ANR in Window{f30f124 …}`）。
    if queue.is_null() {
        // 空指针不入集：集里"空"与"槽位 0"是同一格编码，放一条 null 进去等于多一条假的活队列。
        note(c"onInputQueueCreated queue=NULL — not recorded");
    } else {
        let mut set = live_snapshot();
        match queue_set_insert(&mut set, queue) {
            Insert::Added(slot) => {
                // 属主与队列同格：`onDestroy` 按实例来，它只认这张表。槽位由谓词给，所以
                // "记了队列没记属主"这一格不可能靠忘记写发生——两处写用的是同一个 `slot`。
                LIVE_OWNERS[slot].store(activity as *mut c_void, Ordering::Release);
                unsafe { publish_set(&set) };
                note(c"onInputQueueCreated");
                #[cfg(target_os = "android")]
                unsafe {
                    ndk::__android_log_print(
                        LOG_INFO,
                        TAG.as_ptr(),
                        c"input queue added slot=%d held=%d queue=%p".as_ptr(),
                        slot as c_int,
                        held_count(&set) as c_int,
                        queue,
                    )
                };
            }
            Insert::Already(slot) => {
                note(c"onInputQueueCreated already-held — not recorded twice");
                #[cfg(target_os = "android")]
                unsafe {
                    ndk::__android_log_print(
                        LOG_INFO,
                        TAG.as_ptr(),
                        c"input queue already at slot=%d queue=%p".as_ptr(),
                        slot as c_int,
                        queue,
                    )
                };
                #[cfg(not(target_os = "android"))]
                let _ = slot;
            }
            Insert::Full => {
                // 这一格响起来就是"有条队列不会被排空"，与 ANR 同形，所以说重一点不夸张。
                warn(c"onInputQueueCreated set FULL — this queue will NOT be drained");
            }
        }
    }
    // 从前这条到这儿就完了：指针存进静态、没人读。现在它记的是**意图**——attach 这个动作
    // 发生在拥有 looper 的那条线程上（回调里直接 attach 会绕开那条线程，头里的注释就是那么用的，
    // 但本壳的排空循环在那边，attach 不归这里做）。
    #[cfg(target_os = "android")]
    unsafe {
        start_input_thread();
        wake_input();
    }
}

/// 头里的硬约束对队列同样成立：**返回之后不许再碰它**，而它返回之后随时可能被释放。所以
/// `detachLooper` 必须在这条回调里做完，不能留给线程下一次轮询（那时它可能已经是悬垂指针）。
/// 这格是抄写风险最高的一格——"detach 可以在别的线程调"这件事头里没写，靠的是 NDK `Looper`
/// 内部带锁这个实现事实，所以它**不进断言、只进现读**：真机日志里若 `detachLooper done` 之后
/// 还能看到 `hasEvents`，就说明这条假设在设备上不对，得换成"线程自己停手"那一档。
unsafe extern "C" fn on_input_queue_destroyed(
    _activity: *mut ANativeActivity,
    queue: *mut c_void,
) {
    // 顺序是**先 detach、后摘除**：线程每轮排空都拿整集现读对质，摘除那一刻它就停手；
    // 而 detach 必须在还认得这条队列的时候做完。反过来会留一格"已 detach 前就被摘掉、这一轮不排"。
    #[cfg(target_os = "android")]
    if !queue.is_null() {
        unsafe { ndk::AInputQueue_detachLooper(queue) };
        unsafe {
            ndk::__android_log_print(LOG_INFO, TAG.as_ptr(), c"input detachLooper done".as_ptr())
        };
    }
    note(c"onInputQueueDestroyed");
    // **只摘"就是它"那一格**。从前这里是一次无条件清空，而 2026-10-13 那次真机把它量出来了：
    // 同进程第二条队列还活得好好的，一条无关的销毁把它的账也抹了，于是它既不排空也没人 detach，
    // 系统自己给它判了刑（`Waited 10000ms for MotionEvent` ＋ `ANR in Window{f30f124 …}`）。
    let mut set = live_snapshot();
    match queue_set_remove(&mut set, queue) {
        Remove::Removed(slot) => {
            // 属主那一格跟着一起清：留着一个已摘槽位的旧属主，下一次 `on_destroy` 就会按它去
            // 匹配一条**别人刚复用的槽位**。同一个 `slot` 写两处，与创建侧对称。
            LIVE_OWNERS[slot].store(std::ptr::null_mut(), Ordering::Release);
            unsafe { publish_set(&set) };
            #[cfg(target_os = "android")]
            unsafe {
                ndk::__android_log_print(
                    LOG_INFO,
                    TAG.as_ptr(),
                    c"input queue removed slot=%d held=%d queue=%p".as_ptr(),
                    slot as c_int,
                    held_count(&set) as c_int,
                    queue,
                )
            };
        }
        Remove::NotHeld => {
            // 集里根本没这条（`queue=NULL` 也在这一格）。**不整集重发**：没摘掉任何东西却发一次，
            // 等于把别人的账照当前快照抄一遍——那次抄写没有对应的 detach，除了掩盖错乱没有别的用处。
            warn(c"onInputQueueDestroyed queue not held — set unchanged");
        }
    }
    #[cfg(target_os = "android")]
    unsafe { wake_input() };
}

unsafe extern "C" fn on_content_rect_changed(
    _activity: *mut ANativeActivity,
    rect: *const c_void,
) {
    // 从前这条只登记、不读——回调进来打一句 `onContentRectChanged` 就完了，那个 `ARect*` 里
    // 四个数没人看过。现在把它们打出来：这是系统对"你能画的矩形"唯一的现读证人，而本帧问的
    // 恰好就是这句话（挖孔区那条 52 到不到边）。**只印不裁**：壳里没有任何一处版面拿它当输入，
    // 拿它当输入的下一步是 hit-test（#132），那时再决定它归谁持。
    #[cfg(target_os = "android")]
    if rect.is_null() {
        warn(c"onContentRectChanged: rect pointer is NULL");
    } else {
        let r = unsafe { &*(rect as *const Area) };
        unsafe {
            ndk::__android_log_print(
                LOG_INFO,
                TAG.as_ptr(),
                c"onContentRectChanged l=%d t=%d r=%d b=%d (%dx%d)".as_ptr(),
                r.left,
                r.top,
                r.right,
                r.bottom,
                r.right - r.left,
                r.bottom - r.top,
            )
        };
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = rect;
        note(c"onContentRectChanged");
    }
}

unsafe extern "C" fn on_configuration_changed(_activity: *mut ANativeActivity) {
    note(c"onConfigurationChanged");
}

unsafe extern "C" fn on_low_memory(_activity: *mut ANativeActivity) {
    note(c"onLowMemory");
}

// ── 呈现面（本帧第一件）─────────────────────────────────────────────────────
//
// 资产里那份 UI 子集的路径名与 `scripts/android-font.sh` 的输出名一对一；改名要两边一起改，
// 读不到时这里会打日志，不会静默变成"白屏"。
#[cfg(target_os = "android")]
const FONT_ASSET: &std::ffi::CStr = c"fonts/ui.otf";

// 盘面、字号、色值、圆角、定位点边长都**不在这里**——它们在 `frame.rs`／`text.rs`／`md3.rs`，
// host 侧对撞工具 `#[path]` 引的是同一份源码（复制一份数字的对撞不算对撞）。
// 本文件只留平台侧的那一条路径名，和"何时锁、何时放"。
#[cfg(target_os = "android")]
struct Scene {
    face: crate::text::Face,
    atlas: crate::text::Atlas,
    /// 活的那一桌牌。从前这里每次绘制现打一桌（`frame::demo_frame()`），画完就丢，
    /// 所以屏上的东西**不可被操作**；本帧把它持住，于是 `input.rs` 改完状态后画的就是这一桌。
    live: crate::input::Live,
}

/// 单线程的呈现面也要 `Sync`，因为回调表是框架给的、静态比栈寿命长。锁不竞争，只为类型成立。
#[cfg(target_os = "android")]
static SCENE: std::sync::Mutex<Option<Scene>> = std::sync::Mutex::new(None);

/// 读一次资产。`AAsset_read` 返回负数是读失败——这是系统边界（APK 里可能真没这个文件），
/// 所以它必须被区分出来，而不是当成 0 字节继续走。
#[cfg(target_os = "android")]
unsafe fn read_asset(mgr: *mut c_void) -> Option<Vec<u8>> {
    if mgr.is_null() {
        warn(c"assetManager is NULL — cannot load font");
        return None;
    }
    let asset = unsafe { ndk::AAssetManager_open(mgr, FONT_ASSET.as_ptr(), ndk::ASSET_MODE_BUFFER) };
    if asset.is_null() {
        unsafe {
            ndk::__android_log_print(
                LOG_ERROR, TAG.as_ptr(), c"asset %s missing".as_ptr(), FONT_ASSET.as_ptr(),
            )
        };
        return None;
    }
    let len = unsafe { ndk::AAsset_getLength(asset) };
    if len <= 0 {
        unsafe { ndk::AAsset_close(asset) };
        unsafe { ndk::__android_log_print(LOG_ERROR, TAG.as_ptr(), c"asset length=%ld".as_ptr(), len) };
        return None;
    }
    let n = len as usize;
    let mut buf = vec![0u8; n];
    let got = unsafe { ndk::AAsset_read(asset, buf.as_mut_ptr() as *mut c_void, n) };
    unsafe { ndk::AAsset_close(asset) };
    // 两边宽度不同（`getLength` 是 `off_t`，`read` 是 `int`），所以比的是**换算到 i64 之后**的值。
    if (got as i64) != (len as i64) {
        unsafe {
            ndk::__android_log_print(
                LOG_ERROR, TAG.as_ptr(), c"asset read %lld of %lld".as_ptr(),
                got as i64, len as i64,
            )
        };
        return None;
    }
    Some(buf)
}

/// 一帧真盘面的两档耗时：**懒**栅格一遍（只栅这一帧落得到的字），重铺底色，再画**同一帧**
/// 一遍（这次图集全热）。两档打在同一行日志里，就是 §5 L7 尾巴上"图集与缓存"那件工程件的
/// 读数——host 与真机跑的是同一份 `frame.rs`，差的就是机器。
///
/// 中间那次 `fill_bg` 不是洁癖：态层（`Block{opacity}`）是就地混色、没有合成器兜着，
/// 直接在第一遍的结果上再画一遍会让那些带**越画越深**，"热档"读到的就不是同一帧了。
/// 副作用是上屏的那一帧来自热档——冷/热同函数、同参数、同图集终态，两侧对撞读的还是同一张。
#[cfg(target_os = "android")]
fn paint_board(dst: &mut [u8], w: usize, h: usize, stride: usize, why: &std::ffi::CStr, bg_ms: f64) -> bool {
    let mut guard = SCENE.lock().unwrap_or_else(|p| p.into_inner());
    let Some(scene) = guard.as_mut() else { return false };
    let Scene { face, atlas, live } = scene;
    let frame = live.frame();
    // 尺与帧**同批**记下：命中的那把尺就是画这一帧用的那把。拿窗口现读尺寸在点的时候重算，
    // 在 resized 与触摸交错的时刻会得到一把"没画过"的尺——屏上那一格的位置是上一次绘制定下的。
    live.set_geom(crate::input::Geom::of(w, h, &frame));

    let t = std::time::Instant::now();
    let cold = crate::frame::paint(&frame, face, atlas, dst, w, h, stride);
    let cold_ms = t.elapsed().as_secs_f64() * 1000.0;

    crate::text::fill_bg(dst, w, h, stride);
    let t = std::time::Instant::now();
    crate::frame::paint(&frame, face, atlas, dst, w, h, stride);
    let warm_ms = t.elapsed().as_secs_f64() * 1000.0;

    // 推进宽度这一格是给 #133 的：引擎那张表写的是 CJK=1.0／ASCII=0.55（`typo::ADVANCE_*`），
    // 真字体里"advance ÷ em"到底是多少，只有引了这条链才量得出来（量法住 `frame::advance_probe`）。
    let (adv_cjk, adv_latin) = crate::frame::advance_probe(face);
    unsafe {
        ndk::__android_log_print(
            LOG_INFO, TAG.as_ptr(),
            c"%s bg=%.2f cold=%.2f warm=%.2f k=%.4f off=%dx%d prims=%u+%u blank=%u empty=%u digest=%016llx rasters=%u atlas=%dB adv=%.4f/%.4f".as_ptr(),
            why.as_ptr(),
            bg_ms,
            cold_ms,
            warm_ms,
            cold.scale as f64,
            cold.off_x,
            cold.off_y,
            cold.blocks,
            cold.labels,
            cold.blank_labels,
            cold.empty_blocks,
            crate::frame::frame_digest(&frame),
            cold.rasters,
            atlas.bytes() as u32,
            adv_cjk,
            adv_latin,
        )
    };
    // 一格空面不叫"画过了"：一帧里既没方块也没字，多半是指令表那侧空跑，屏上是纯底色。
    if cold.blocks == 0 && cold.labels == 0 {
        warn(c"frame carried no prims — bg only");
        return false;
    }
    true
}
/// 一次装好一份 scene（资产字节 → `Face`）。失败就是**没字**，不是"字很小"——所以每一格失败
/// 都单独留一行日志：真机看见空白屏时，要能当场分清是资产没进包、字节读歪，还是画了但没到屏。
#[cfg(target_os = "android")]
unsafe fn ensure_scene(activity: *mut ANativeActivity) {
    if SCENE.lock().map(|g| g.is_some()).unwrap_or(true) {
        return;
    }
    let mgr = if activity.is_null() { std::ptr::null_mut() } else { unsafe { (*activity).assetManager } };
    let t0 = std::time::Instant::now();
    let Some(bytes) = (unsafe { read_asset(mgr) }) else { return };
    let n = bytes.len();
    let read_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let t1 = std::time::Instant::now();
    let face = match crate::text::Face::try_from_vec(bytes) {
        Ok(f) => f,
        Err(crate::text::FaceError::BadBytes(_)) => {
            warn(c"font bytes unreadable (BadBytes)");
            return;
        }
        Err(crate::text::FaceError::NoUpem) => {
            warn(c"font has no unitsPerEm");
            return;
        }
    };
    let parse_ms = t1.elapsed().as_secs_f64() * 1000.0;
    unsafe {
        ndk::__android_log_print(
            LOG_INFO, TAG.as_ptr(),
            c"font %dB read=%.2f parse=%.2f ms glyphs=%u upem=%.0f asc=%.0f desc=%.0f ratio=%.4f".as_ptr(),
            n as c_int,
            read_ms,
            parse_ms,
            face.glyph_count() as u32,
            face.upem() as f64,
            face.ascent_unscaled() as f64,
            face.descent_unscaled() as f64,
            face.ratio() as f64,
        )
    };
    let mut guard = SCENE.lock().unwrap_or_else(|p| p.into_inner());
    *guard = Some(Scene {
        face,
        atlas: crate::text::Atlas::new(),
        live: crate::input::Live::new(),
    });
}

/// 锁→画→放。**这一格第一次把引擎算出来的整张盘面铺进 `ANativeWindow_Buffer`**，所以每一步的
/// 现读尺寸都进日志：真机给的是 `stride > width`（§3.8.1 现读 736 对 720）还是别的，当场可见。
#[cfg(target_os = "android")]
unsafe fn paint(window: *mut c_void, why: &'static std::ffi::CStr) {
    if window.is_null() {
        return;
    }
    // **先拿锁，再看窗口**。反过来（先看窗口再拿锁）时，输入线程可能拿到一个主线程刚拆掉的
    // 指针——`onNativeWindowDestroyed` 现在要拿到这把锁之后才清空 `WINDOW`，所以"锁在手且
    // 窗口仍是当前那块"这句才有证人。锁等着锁着最长一次是一帧的绘制时间（现读 §2 那行 warm=）。
    let _paint = PAINT.lock().unwrap_or_else(|p| p.into_inner());
    if WINDOW.load(Ordering::Acquire) != window {
        warn(c"paint skipped: window is no longer the current one");
        return;
    }
    let mut buf = WindowBuffer {
        width: 0,
        height: 0,
        stride: 0,
        format: 0,
        bits: std::ptr::null_mut(),
        reserved: [0; 6],
    };
    let rc = unsafe { ndk::ANativeWindow_lock(window, &mut buf, std::ptr::null()) };
    if rc != 0 || buf.bits.is_null() {
        unsafe { ndk::__android_log_print(LOG_ERROR, TAG.as_ptr(), c"lock failed rc=%d".as_ptr(), rc) };
        return;
    }
    let (w, h) = (buf.width.max(0) as usize, buf.height.max(0) as usize);
    let stride = buf.stride.max(0) as usize;
    let format = buf.format;
    let base = buf.bits as *mut u8;
    unsafe {
        ndk::__android_log_print(
            LOG_INFO, TAG.as_ptr(),
            c"%s buffer %dx%d stride=%d fmt=%d".as_ptr(),
            why.as_ptr(),
            buf.width,
            buf.height,
            buf.stride,
            format,
        )
    };
    // 只认 4 字节/像素的两档，且行步长得容得下宽度。不合规就报告"没画"然后放行缓冲——
    // 留着一块写歪的内存上屏，比白屏更难查。
    if !is_rgba32(format) || stride < w {
        unsafe {
            ndk::__android_log_print(
                LOG_ERROR, TAG.as_ptr(),
                c"unsupported fmt=%d stride=%d width=%d — painted nothing".as_ptr(),
                format,
                buf.stride,
                buf.width,
            )
        };
        unsafe { ndk::ANativeWindow_unlockAndPost(window) };
        return;
    }
    let dst = unsafe { std::slice::from_raw_parts_mut(base, stride * h * 4) };
    let painted = if SCENE.lock().map(|g| g.is_some()).unwrap_or(false) {
        let t = std::time::Instant::now();
        crate::text::fill_bg(dst, w, h, stride);
        let bg_ms = t.elapsed().as_secs_f64() * 1000.0;
        paint_board(dst, w, h, stride, why, bg_ms)
    } else {
        note(c"no scene: bg only");
        crate::text::fill_bg(dst, w, h, stride);
        false
    };
    let pr = unsafe { ndk::ANativeWindow_unlockAndPost(window) };
    unsafe {
        ndk::__android_log_print(LOG_INFO, TAG.as_ptr(), c"unlockAndPost rc=%d painted=%d".as_ptr(), pr, painted as c_int)
    };
}

// ── 输入运输面（本帧第二件）─────────────────────────────────────────────────

// 归属判断收在这一处，**不带 `#[cfg]`**：只比指针、不解引用、不碰原子，所以 host 侧直接喂
// 假指针就能把三种次序各测一遍。三条 `pub` 是它作为被测面的接口面，不是给外面用的。

/// 加入的结果，三种要分开说：`Added` 正常；`Already` 是同一条指针被 create 了两回（集里不许
/// 出现两条一样的，第二回不记账）；`Full` 是槽位不够——那意味着**有一条队列没被记账**，
/// 它不会被 attach、不会被排空，正是 2026-10-13 那次 ANR 的形状，日志必须响。
#[derive(Debug, PartialEq, Eq)]
pub enum Insert {
    Added(usize),
    Already(usize),
    Full,
}

/// 摘除的结果。`NotHeld` 是"销毁回调报的那条不在我们手里"——可能是记账已经错乱，也可能平台
/// 报了一条我们从没被 create 过的队列。两种都不是"什么都不做"能盖住的，所以不许静默。
#[derive(Debug, PartialEq, Eq)]
pub enum Remove {
    Removed(usize),
    NotHeld,
}

/// 排空那一轮的闸门，三档。**从前这一格只有一句话**（`input queue vanished mid-drain — stop`），
/// 而"vanished"说的比量到的多：真机从没量到"队列凭空消失"，量到的是① finish 之后空集、
/// ② 换代（还有一条别的在）。这两件事该打两句日志，处置也不同。
#[derive(Debug, PartialEq, Eq)]
pub enum DrainGate {
    /// 我要排的这条还在集里，继续排。
    KeepGoing,
    /// 集里一条都没了：正常销毁之后的那一格。
    Emptied,
    /// 这条已被摘掉，但集里还剩 `usize` 条别的：换代那一格。
    Replaced(usize),
}

pub fn queue_set_insert(set: &mut [*mut c_void], queue: *mut c_void) -> Insert {
    if let Some(i) = set.iter().position(|q| *q == queue) {
        return Insert::Already(i);
    }
    match set.iter().position(|q| q.is_null()) {
        Some(i) => {
            set[i] = queue;
            Insert::Added(i)
        }
        None => Insert::Full,
    }
}

pub fn queue_set_remove(set: &mut [*mut c_void], gone: *mut c_void) -> Remove {
    match set.iter().position(|q| !q.is_null() && *q == gone) {
        Some(i) => {
            set[i] = std::ptr::null_mut();
            Remove::Removed(i)
        }
        None => Remove::NotHeld,
    }
}

pub fn drain_gate(set: &[*mut c_void], was: *mut c_void) -> DrainGate {
    if set.contains(&was) {
        return DrainGate::KeepGoing;
    }
    match set.iter().filter(|q| !q.is_null()).count() {
        0 => DrainGate::Emptied,
        others => DrainGate::Replaced(others),
    }
}

/// 按**属主**摘除：清掉 `owner` 名下的每一格，返回摘掉的条数。这条存在的全部理由是
/// `onDestroy` 也按实例来：它有权撤的只有这个实例名下的账。2026-10-14 05:14:14 那次真机就是
/// 反证——队列回调按格摘对了（`removed slot=1 held=1`），紧接着 `onDestroy` 一次整集清空又把
/// 邻居那条抹了，症状与 10-13 那一格一模一样（`Waited 10000ms for MotionEvent`）。
/// 返回 0 是**合法结局**：那条队列的 destroy 回调先走完了，这里本来就没剩下什么。那一格由
/// 日志如实说"0 条"，不许由"顺手清一次整集"代替。
pub fn queue_set_release_owner(
    set: &mut [*mut c_void],
    owners: &mut [*mut c_void],
    owner: *mut c_void,
) -> usize {
    let mut released = 0;
    if !owner.is_null() {
        for i in 0..set.len() {
            if !set[i].is_null() && owners[i] == owner {
                set[i] = std::ptr::null_mut();
                owners[i] = std::ptr::null_mut();
                released += 1;
            }
        }
    }
    released
}

/// 空集：给调用方起局部数组用的字面量（`[*mut c_void; LIVE_QUEUE_SLOTS]` 没有 const-new 可用，
/// 裸指针是 Copy，重复表达式在这里合法）。
fn empty_queue_set() -> [*mut c_void; LIVE_QUEUE_SLOTS] {
    [std::ptr::null_mut(); LIVE_QUEUE_SLOTS]
}

/// 现读整集。不带 `#[cfg]`：它只读那四个原子量、不碰 NDK，而三条回调两侧都编——
/// 加了 cfg 就等于让 host 侧的回调走一条设备上不存在的路径。
fn live_snapshot() -> [*mut c_void; LIVE_QUEUE_SLOTS] {
    let mut out = empty_queue_set();
    for (src, dst) in LIVE_QUEUES.iter().zip(out.iter_mut()) {
        *dst = src.load(Ordering::Acquire);
    }
    out
}

/// 把改好的集发回去。调用点全在主线程（见 `LIVE_QUEUES` 那条注释），所以这里没有 CAS：
/// 四个 store 之间读者可能看到"少一条"的中间态，那种中间态的后果是那一条这一轮不排、
/// 下一轮排到——**不是丢事件**；反过来若允许两条线程各自整集覆盖，丢的就是真更新。
unsafe fn publish_set(set: &[*mut c_void]) {
    for (dst, src) in LIVE_QUEUES.iter().zip(set) {
        dst.store(*src, Ordering::Release);
    }
}

fn held_count(set: &[*mut c_void]) -> usize {
    set.iter().filter(|q| !q.is_null()).count()
}

/// 属主那一排的现读与发回，与上面两条同形。**只有主线程调**（三条回调都在那条线程上），
/// 输入线程一次都不读它——所以这两条不必与 `live_snapshot`/`publish_set` 原子配对，
/// 中间态最多是"主线程自己看到旧属主"，而那条线程下一步就是写它。
fn live_owners() -> [*mut c_void; LIVE_QUEUE_SLOTS] {
    let mut out = empty_queue_set();
    for (src, dst) in LIVE_OWNERS.iter().zip(out.iter_mut()) {
        *dst = src.load(Ordering::Acquire);
    }
    out
}

unsafe fn publish_owners(owners: &[*mut c_void]) {
    for (dst, src) in LIVE_OWNERS.iter().zip(owners) {
        dst.store(*src, Ordering::Release);
    }
}

/// 输入线程自己的 looper，发布出来只为一个用途：**主线程能叫醒它**。读不到（线程还没起来）
/// 就什么都不做——线程起来后的第一件事是现读 `LIVE_QUEUES` 整集，所以"队列先创建、线程后起来"
/// 那一格不会丢事件，只会晚一轮 poll（≤ `POLL_TIMEOUT_MS`）。
#[cfg(target_os = "android")]
static LOOPER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// 线程只起一次：一次进程里 `onInputQueueCreated` 可以来好几回（活动重建、配置变更），而
/// looper 是**线程局部**的——多起一条线程就多一个 looper，队列会被 attach 到不知道哪一条上，
/// 表现是"有时点得动有时点不动"。
#[cfg(target_os = "android")]
static INPUT_STARTED: std::sync::Once = std::sync::Once::new();

/// 四个计数只为日志那一行存在：**判定不看它们**。留这一格是因为"运输断了"与"运输到了但
/// 引擎拒了"在屏上长得一模一样（都是没反应），只有这几个数分得开。
#[cfg(target_os = "android")]
static COUNT: Counters = Counters {
    events: std::sync::atomic::AtomicU32::new(0),
    judged: std::sync::atomic::AtomicU32::new(0),
    skipped: std::sync::atomic::AtomicU32::new(0),
    foreign: std::sync::atomic::AtomicU32::new(0),
    pre_dispatched: std::sync::atomic::AtomicU32::new(0),
};

#[cfg(target_os = "android")]
struct Counters {
    events: std::sync::atomic::AtomicU32,
    /// MOTION 且落在三档（DOWN/UP/CANCEL）里、真进了 `Live::on_touch` 的那些。
    judged: std::sync::atomic::AtomicU32,
    /// MOTION 但本帧不建模（MOVE/POINTER_*/SCROLL/HOVER_*…）——**仍然要 finishEvent**，
    /// 不然平台等不到回话就报 "input dispatching timed out"（那是 ANR 那一族的文案）。
    skipped: std::sync::atomic::AtomicU32,
    /// 非 MOTION（KEY/FOCUS/CAPTURE/DRAG/TOUCH_MODE）：本壳不接键盘，原样交回平台。
    foreign: std::sync::atomic::AtomicU32,
    /// `preDispatchEvent` 返回非 0 的次数：那条要求"放弃这条、让它重新回到队列"，不许 finish。
    /// 一个没有输入法的原生窗口本该恒 0，真跳起来了就是我对那半句读错了。
    pre_dispatched: std::sync::atomic::AtomicU32,
}

#[cfg(target_os = "android")]
fn bump(which: &std::sync::atomic::AtomicU32) {
    which.fetch_add(1, Ordering::Relaxed);
}

/// 队列变了就叫线程回来看一眼。`ALooper_wake` 是线程安全的（头里明写它可以被别的线程调），
/// 这也是本壳不用自己那条管道（eventfd/condvar）的原因：平台已经给了一条。
#[cfg(target_os = "android")]
unsafe fn wake_input() {
    let looper = LOOPER.load(Ordering::Acquire);
    if !looper.is_null() {
        unsafe { ndk::ALooper_wake(looper) };
    }
}

#[cfg(target_os = "android")]
unsafe fn start_input_thread() {
    INPUT_STARTED.call_once(|| {
        // 起线程失败是系统边界（线程数/内存），如实报告、不重试：重试只会再失败一次。
        match std::thread::Builder::new().name("midline-input".to_string()).spawn(|| unsafe { input_loop() }) {
            Ok(_) => note(c"input thread started"),
            Err(_) => warn(c"input thread spawn FAILED — touches will not arrive"),
        }
    });
}

/// 线程正文。**attach/detach/排空都发生在这一条线程上**（只有它有自己的 looper），
/// 而窗口的绘制仍可以在这里发生——那件事由 `PAINT` 锁与主线程分先后。
#[cfg(target_os = "android")]
unsafe fn input_loop() {
    let looper = unsafe { ndk::ALooper_prepare(LOOPER_ALLOW_NON_CALLBACKS) };
    if looper.is_null() {
        warn(c"ALooper_prepare returned NULL — no input this process");
        return;
    }
    LOOPER.store(looper, Ordering::Release);
    // 本线程的账，**与 `LIVE_QUEUES` 按槽位对齐**：`attached[i]` 说的是"槽位 i 那条我挂过没有"。
    // 不能只复制集——"集里现在是谁"与"我对谁发过 attach"是两件事，同一格换了队列时只有这本账
    // 知道该不该再挂一次；也不能只记指针不看槽位——销毁那条已经在回调里 detach 过了，这里唯一
    // 能做的动作是**忘记**，再发一次 detach 就是碰已释放的对象。
    let mut attached: [*mut c_void; LIVE_QUEUE_SLOTS] = empty_queue_set();
    let (mut fd, mut events) = (0 as c_int, 0 as c_int);
    let mut data: *mut c_void = std::ptr::null_mut();
    loop {
        let want = live_snapshot();
        for (i, q) in want.iter().enumerate() {
            if attached[i] == *q {
                continue;
            }
            attached[i] = std::ptr::null_mut();
            if !q.is_null() {
                // callback 传 `None` ⇒ 这一路是 ident 轮询，不是回调（见 `LOOPER_ALLOW_NON_CALLBACKS`）。
                unsafe { ndk::AInputQueue_attachLooper(*q, looper, ID_INPUT, None, std::ptr::null_mut()) };
                unsafe {
                    ndk::__android_log_print(LOG_INFO, TAG.as_ptr(), c"input attach queue=%p ident=%d".as_ptr(), *q, ID_INPUT)
                };
                attached[i] = *q;
            }
        }
        let r = unsafe { ndk::ALooper_pollOnce(POLL_TIMEOUT_MS, &mut fd, &mut events, &mut data) };
        if r == POLL_ERROR {
            warn(c"ALooper_pollOnce ERROR — looper dead");
        }
        // 不看 `r` 的档位决定排不排空：头里那句「you *must* treat all return values as if they also
        // indicate ALOOPER_POLL_WAKE」就是这条。按 ident 分支只用于**日志**说清这次为什么醒。
        if r >= 0 && r != ID_INPUT {
            unsafe {
                ndk::__android_log_print(LOG_INFO, TAG.as_ptr(), c"unexpected looper ident=%d fd=%d events=%d".as_ptr(), r, fd, events)
            };
        }
        // 每一轮把**本线程挂着的每一条**都排一次。只排最后创建那条正是 2026-10-13 那次的坏：
        // 底层那条有事件、没人读，平台等不到 finishEvent 就报了超时。
        for q in attached {
            if !q.is_null() {
                unsafe { drain(q) };
            }
        }
    }
}

/// 把当前这一批事件读干净。**每条从 `getEvent` 出来的都必须走到 `finishEvent`**（预派发那次除外），
/// 头里那句 "This must be called after receiving an event" 不是风格：漏了它平台会一直等回话，到超时
/// 那一刻 logcat 打出 "Input dispatching timed out"——超时那一档**本机现读 10000 ms**（不是常引的 5 s，
/// 负样本那次系统打的字面就是 Waited 10000ms）。这是本壳最容易犯也最难自己发现的一种坏。
#[cfg(target_os = "android")]
unsafe fn drain(queue: *mut c_void) {
    loop {
        // 每轮现读**整集**对质：这条被摘掉时立刻停手，别去碰一条正在被拆的队列。
        // 从前这里只有一句话（`input queue vanished mid-drain`），而 "vanished" 说的比量到的多：
        // 真机从没量到"队列凭空消失"，量到的是**清空**与**换代**两格，处置和证据都不一样。
        match drain_gate(&live_snapshot(), queue) {
            DrainGate::KeepGoing => {}
            DrainGate::Emptied => {
                note(c"input queue emptied mid-drain — stop");
                return;
            }
            DrainGate::Replaced(others) => {
                // 换代：我排的这条已经不在账上，但集里还有别的活着。带两个指针是为了在日志里
                // 能认出"是哪一条接替了它"，否则这句话只是"我停了"，落不到归属上。
                unsafe {
                    ndk::__android_log_print(
                        LOG_INFO,
                        TAG.as_ptr(),
                        c"input queue replaced mid-drain — stop this=%p others=%d".as_ptr(),
                        queue,
                        others as c_int,
                    )
                };
                return;
            }
        }
        let has = unsafe { ndk::AInputQueue_hasEvents(queue) };
        if has < 0 {
            unsafe { ndk::__android_log_print(LOG_ERROR, TAG.as_ptr(), c"hasEvents=%d".as_ptr(), has) };
            return;
        }
        if has == 0 {
            return;
        }
        let mut event: *mut c_void = std::ptr::null_mut();
        let gr = unsafe { ndk::AInputQueue_getEvent(queue, &mut event) };
        if gr < 0 || event.is_null() {
            unsafe { ndk::__android_log_print(LOG_ERROR, TAG.as_ptr(), c"getEvent=%d".as_ptr(), gr) };
            return;
        }
        bump(&COUNT.events);
        if unsafe { ndk::AInputQueue_preDispatchEvent(queue, event) } != 0 {
            // 非 0 ⇒ 这条被交出去（输入法那类）预派发了，头里要求**放弃当前处理**且不 finish，
            // 让它下一轮重新出现在队列里。所以这里直接回轮询，不是 break 掉排空就算完。
            bump(&COUNT.pre_dispatched);
            return;
        }
        let handled = unsafe { dispatch(event) };
        unsafe { ndk::AInputQueue_finishEvent(queue, event, handled as c_int) };
    }
}

/// 一条事件 → 要么进 `Live::on_touch`，要么如实归到某一档计数。返回 `handled`。
/// 交回 0 的那些（非 MOTION、本帧不建模的 action）不是"丢了"——是"本壳不接，平台可继续派"。
#[cfg(target_os = "android")]
unsafe fn dispatch(event: *mut c_void) -> bool {
    let kind = unsafe { ndk::AInputEvent_getType(event) };
    if kind != EVENT_TYPE_MOTION {
        bump(&COUNT.foreign);
        return false;
    }
    let raw = unsafe { ndk::AMotionEvent_getAction(event) };
    let action = raw & ACTION_MASK;
    let multi = unsafe { ndk::AMotionEvent_getPointerCount(event) };
    let act = match action {
        ACTION_DOWN => crate::input::Action::Down,
        ACTION_UP => crate::input::Action::Up,
        ACTION_CANCEL => crate::input::Action::Cancel,
        // MOVE/POINTER_DOWN/POINTER_UP/SCROLL/HOVER_*…：本帧不立多指与拖拽那条手势（档面 §1 裁掉的那格）。
        // 计数、不判——它们的正确结局是"什么也不做"，但仍要走 finishEvent。
        _ => {
            bump(&COUNT.skipped);
            unsafe {
                ndk::__android_log_print(
                    LOG_INFO, TAG.as_ptr(),
                    c"touch action=%d (masked=%d) pointers=%u — not modeled".as_ptr(),
                    raw, action, multi as u32,
                )
            };
            return true;
        }
    };
    // 只读第一指。`getX/getY` 的坐标系是**接收这个事件的那个窗口**，而我们的缓冲就是这个窗口的
    // 像素，所以这里不做任何换算——换算住在 `input::Geom`（正向、与绘制同一支 `at`）。
    // `float` 取整用 `round`：手指落在两像素缝上时，"看见的那格"由绘制侧的取整决定，这里要跟它同档。
    let x = unsafe { ndk::AMotionEvent_getX(event, 0) }.round() as c_int;
    let y = unsafe { ndk::AMotionEvent_getY(event, 0) }.round() as c_int;
    if multi > 1 {
        // 第二指及以上：如实说出来（本帧不判它），症状"多指时单指判定漂"就靠这一行辨认。
        unsafe {
            ndk::__android_log_print(LOG_INFO, TAG.as_ptr(), c"touch has %u pointers — first only".as_ptr(), multi as u32)
        };
    }
    bump(&COUNT.judged);
    touch(act, x, y)
}

/// 判定 + 重画。**判定住在 `input.rs`**（host 侧对撞引的是同一份源码），这里只做三件事：
/// 拿到那一行的结论、把它打进 logcat、叫一次绘制。
#[cfg(target_os = "android")]
fn touch(act: crate::input::Action, x: c_int, y: c_int) -> bool {
    let line = {
        let mut guard = SCENE.lock().unwrap_or_else(|p| p.into_inner());
        let Some(scene) = guard.as_mut() else {
            warn(c"touch before scene — nothing to hit");
            return false;
        };
        scene.live.on_touch(act, x, y).log_line()
    };
    // `%s` ＋ `CString`：日志正文里有中文、有引号、也可能有 `%`，把它当格式串是把内容交给格式串
    // 那一族漏洞——这里内容只可能是 `%s` 的参数。
    match std::ffi::CString::new(line) {
        Ok(s) => {
            unsafe { ndk::__android_log_print(LOG_INFO, TAG.as_ptr(), c"input %s".as_ptr(), s.as_ptr()) };
        }
        Err(_) => warn(c"input log line is not valid UTF-8/C"),
    }
    let window = WINDOW.load(Ordering::Acquire);
    if window.is_null() {
        warn(c"touch judged but no window to paint");
        return true;
    }
    unsafe { paint(window, c"frame-touch") };
    true
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
// `ANativeWindow_Buffer` 在 64 位上的现量：四个整数先排满 16 字节，`bits` 正好落在 16 不用填隙，
// `reserved[6]` 从 24 排到 48。**字段序**由 layout_check.c 拿 NDK 真头对质；这几行只保证我自己的
// 声明自洽——上一帧把 `bits` 误抄在 stride 前面（⇒ bits@8），两侧都能编译，读到的 stride 却是
// reserved[0]，颜色整片写歪（§3.8.1 的失手记录）。
const _: () = assert!(std::mem::size_of::<WindowBuffer>() == 48);
const _: () = assert!(std::mem::offset_of!(WindowBuffer, stride) == 8);
const _: () = assert!(std::mem::offset_of!(WindowBuffer, format) == 12);
const _: () = assert!(std::mem::offset_of!(WindowBuffer, bits) == 16);
// `ARect` 那四个 int32：漏一个填充就把 (left,top,right,bottom) 读成 (left,top,top,right)。
// 与 `WindowBuffer` 同一口径——这几行只证明我自己的声明自洽，真头那一侧由 layout_check.c 对质。
#[cfg(target_os = "android")]
const _: () = assert!(std::mem::size_of::<Area>() == 16);
#[cfg(target_os = "android")]
const _: () = assert!(std::mem::offset_of!(Area, top) == 4);
#[cfg(target_os = "android")]
const _: () = assert!(std::mem::offset_of!(Area, right) == 8);
#[cfg(target_os = "android")]
const _: () = assert!(std::mem::offset_of!(Area, bottom) == 12);

#[cfg(test)]
mod tests {
    use super::*;

    /// 三条次序测都只喂**假指针**：谓词不比大小、不解引用，所以 `0x1` 这种地址在 host 侧
    /// 完全合法（它从没被 deref 过）。真机上那些指针是平台给的 `AInputQueue*`，归属判断
    /// 只看"是不是同一条"，与指向什么无关——这正是把判断抽成纯谓词换来的东西。
    /// 走这支 `const fn` 而不是直接写字面 cast：clippy 的 manual_dangling_ptr 会建议换成
    /// `std::ptr::dangling_mut`，而那个每次回**同一个值**，装不下"两条队列并存"要的几个互不相同的哨兵。
    const fn sentinel(n: usize) -> *mut c_void {
        n as *mut c_void
    }
    const A: *mut c_void = sentinel(1);
    const B: *mut c_void = sentinel(2);
    const C: *mut c_void = sentinel(3);
    /// 一条我们从没被 create 过的队列。真机上它不是幻想：`onDestroy` 之后平台仍可再来一条
    /// destroy（那格的形状就是"账上没有"），此时不许静默。
    const NEVER: *mut c_void = sentinel(9);

    fn set_with(qs: [*mut c_void; LIVE_QUEUE_SLOTS]) -> [*mut c_void; LIVE_QUEUE_SLOTS] {
        qs
    }

    /// 换代那一格：**销毁先来、创建后来**时，新那条必须留在账上，旧那条的销毁不许连它一起抹。
    /// 从前这一格是整集无条件清空，于是同进程第二个实例的队列没人排空——系统自己判了刑
    /// （`Waited 10000ms for MotionEvent` ＋ `ANR in Window{f30f124 …}`，2026-10-13 现读）。
    #[test]
    fn destroying_the_old_queue_does_not_unaccount_the_new_one() {
        let mut set = empty_queue_set();
        assert_eq!(queue_set_insert(&mut set, A), Insert::Added(0), "A 落槽位 0");
        assert_eq!(queue_set_insert(&mut set, B), Insert::Added(1), "B 落槽位 1（同进程第二个实例）");
        assert_eq!(queue_set_remove(&mut set, A), Remove::Removed(0));
        assert_eq!(set, set_with([std::ptr::null_mut(), B, std::ptr::null_mut(), std::ptr::null_mut()]),
            "只许清 A 那一格：B 还活着");
        assert_eq!(held_count(&set), 1);
        // B 仍在账上 ⇒ 排它那一轮闸门放行，不该说"没了"。
        assert_eq!(drain_gate(&set, B), DrainGate::KeepGoing);
    }

    /// 创建先来、销毁后来（同一批队列，只是次序换成"两条都 create 完才拆"）。测的是**槽位**
    /// 而不是插入顺序：拆中间那条，两边都得原地不动。
    #[test]
    fn removal_clears_exactly_the_slot_that_matches() {
        let mut set = empty_queue_set();
        assert_eq!(queue_set_insert(&mut set, A), Insert::Added(0));
        assert_eq!(queue_set_insert(&mut set, B), Insert::Added(1));
        assert_eq!(queue_set_insert(&mut set, C), Insert::Added(2));
        assert_eq!(queue_set_remove(&mut set, B), Remove::Removed(1));
        assert_eq!(set, set_with([A, std::ptr::null_mut(), C, std::ptr::null_mut()]), "拆中间，两边不许动");
        // 空槽优先复用：下一次 create 落在 1，不是往后追加。
        assert_eq!(queue_set_insert(&mut set, NEVER), Insert::Added(1));
        assert_eq!(held_count(&set), 3);
    }

    /// 同一条队列被 create 两回（活动重建时平台真会这么做）：不许记成两条，否则 `held` 那个数
    /// 从此骗人，而排空会把同一条排两遍。
    #[test]
    fn the_same_queue_is_never_counted_twice() {
        let mut set = empty_queue_set();
        assert_eq!(queue_set_insert(&mut set, A), Insert::Added(0));
        assert_eq!(queue_set_insert(&mut set, A), Insert::Already(0));
        assert_eq!(held_count(&set), 1, "重复 create 不增账");
        assert_eq!(queue_set_remove(&mut set, A), Remove::Removed(0));
        assert_eq!(queue_set_remove(&mut set, A), Remove::NotHeld, "拆第二回要说『没这条』，不许再报 Removed");
    }

    /// 账上没有的指针（含 `queue=NULL` 那一格）：`NotHeld`。这一格从前被无条件清空盖掉——
    /// 它看起来"也能收敛到空集"，代价是把别人活着的账一起清了。
    #[test]
    fn a_queue_we_never_recorded_is_reported_not_silently_dropped() {
        let mut set = empty_queue_set();
        assert_eq!(queue_set_insert(&mut set, A), Insert::Added(0));
        assert_eq!(queue_set_remove(&mut set, NEVER), Remove::NotHeld);
        assert_eq!(set, set_with([A, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()]),
            "NotHeld 这一支一个字都不许改");
        assert_eq!(drain_gate(&set, A), DrainGate::KeepGoing);
    }

    /// `drain` 从前只有一句话（`vanished`），而真机从没量到"凭空消失"。这里把两格钉开：
    /// **换代**（我还不在账上，但还有别的活着）与**清空**（整集没了）。
    #[test]
    fn replacement_and_empty_are_two_different_words() {
        let mut set = empty_queue_set();
        assert_eq!(queue_set_insert(&mut set, A), Insert::Added(0));
        assert_eq!(queue_set_insert(&mut set, B), Insert::Added(1));
        assert_eq!(queue_set_remove(&mut set, A), Remove::Removed(0));
        assert_eq!(drain_gate(&set, A), DrainGate::Replaced(1), "A 已摘、B 还活着 ⇒ 换代不是消失");
        assert_eq!(queue_set_remove(&mut set, B), Remove::Removed(1));
        assert_eq!(drain_gate(&set, B), DrainGate::Emptied, "整集空了才是那一格正常收尾");
        assert_eq!(drain_gate(&set, NEVER), DrainGate::Emptied);
    }

    /// 槽位塞不下时的形状：`Full` 必须**一个字都不改**，且日志那一格由调用方负责响。
    /// 这一格响起来与 ANR 同形（有条队列不会被排空），所以它是这条测存在的全部理由。
    #[test]
    fn a_full_set_refuses_without_overwriting_what_it_holds() {
        let mut set = empty_queue_set();
        for i in 0..LIVE_QUEUE_SLOTS {
            assert_eq!(
                queue_set_insert(&mut set, (0x10 + i) as *mut c_void),
                Insert::Added(i),
                "前 {i} 条各占一格"
            );
        }
        let before = set;
        assert_eq!(queue_set_insert(&mut set, NEVER), Insert::Full);
        assert_eq!(set, before, "Full 不许挤掉任何一条已有账目");
        assert_eq!(held_count(&set), LIVE_QUEUE_SLOTS);
    }

    // ── 属主那一半 ──────────────────────────────────────────────────────────
    // 这三条不是把上面几条换个说法：它们钉的是**另一个回调**。2026-10-14 05:14:14 那次真机
    // 就是这一格漏的——队列回调按格摘对了（`removed slot=1 held=1`），紧接着同毫秒的
    // `onDestroy` 一次整集清空又把邻居抹了，日志随后两句 `input queue emptied mid-drain`、
    // 十秒后系统报 `Waited 10000ms for MotionEvent`。头里没承诺两条回调的次序，所以两格都得测。

    /// 邻居的 `onDestroy` 不许把我这条抹掉（队列回调**还没来**那一档）。
    #[test]
    fn another_instances_destroy_leaves_my_queue_accounted() {
        let (x, y) = (0xAusize, 0xBusize);
        let mut set = empty_queue_set();
        let mut own = empty_queue_set();
        assert_eq!(queue_set_insert(&mut set, A), Insert::Added(0));
        own[0] = x as *mut c_void;
        assert_eq!(queue_set_insert(&mut set, B), Insert::Added(1));
        own[1] = y as *mut c_void;
        assert_eq!(queue_set_release_owner(&mut set, &mut own, y as *mut c_void), 1, "Y 名下就一条");
        assert_eq!(set, set_with([A, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()]),
            "B 归 B，A 不许跟着走");
        assert_eq!(own[0], x as *mut c_void, "留着已摘槽位的旧属主才会错配，A 的属主必须原样");
        assert_eq!(drain_gate(&set, A), DrainGate::KeepGoing, "A 还活着，那一轮闸门该放行");
    }

    /// 队列回调**先来**、`onDestroy` 后来：这一档 `release_owner` 该数出 **0 条**并且一个字都不改。
    /// "0 条"是合法结局，不是需要补救的异常——把它写成"再清一次整集"就是上面那个反证。
    #[test]
    fn destroy_after_the_queue_already_left_releases_nothing() {
        let x = 0xAusize;
        let mut set = empty_queue_set();
        let mut own = empty_queue_set();
        assert_eq!(queue_set_insert(&mut set, A), Insert::Added(0));
        own[0] = x as *mut c_void;
        assert_eq!(queue_set_insert(&mut set, B), Insert::Added(1));
        own[1] = x as *mut c_void;
        // 同一条队列自己的 destroy 回调先走完（含把属主那一格一起清掉）
        assert_eq!(queue_set_remove(&mut set, B), Remove::Removed(1));
        own[1] = std::ptr::null_mut();
        let before = set;
        assert_eq!(set, set_with([A, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()]),
            "队列回调之后：集里只剩 A 这一条");
        assert_eq!(queue_set_release_owner(&mut set, &mut own, x as *mut c_void), 1,
            "同一个实例的两条队列，回调先拆掉一条 ⇒ 这里只该数到剩下那条");
        assert_eq!(set, empty_queue_set(), "X 名下确实什么都不剩了");
        assert_eq!(own, empty_queue_set(), "属主那一排跟着一起清，不许留旧账");
        assert_eq!(drain_gate(&set, before[0]), DrainGate::Emptied);
    }

    /// 空属主不匹配任何格。槽位空与属主空是同一个编码（`null`），放它进去扫一遍就会把
    /// "所有没记属主的活队列"一次撤光——那是同一把刀的另一种握法。
    #[test]
    fn a_null_owner_releases_nothing_at_all() {
        let mut set = empty_queue_set();
        let mut own = empty_queue_set();
        assert_eq!(queue_set_insert(&mut set, A), Insert::Added(0));
        assert_eq!(queue_set_insert(&mut set, B), Insert::Added(1));
        let before = set;
        assert_eq!(queue_set_release_owner(&mut set, &mut own, std::ptr::null_mut()), 0);
        assert_eq!(set, before, "属主为空时一个字都不许改");
        assert_eq!(held_count(&set), 2);
    }
}
