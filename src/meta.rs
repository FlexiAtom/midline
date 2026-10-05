// midline - 《中线》核心规则引擎 CLI
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
//! 元游戏：跨关继承堆、融合/升级/弃置（准备·结算阶段）、每日种子、自动对局冒烟。

use crate::battle::{Battle, DailyRule, Difficulty, Outcome, TURN_LIMIT};
use crate::model::{CardInst, Faction};
use crate::save::{self, Progress};
use std::path::PathBuf;

/// 同一台阶段机的两种**画帧方式**——`PlayHost` 唯一认识的壳差别。
/// 引擎、落盘、遭遇、head 全与它无关；它只决定 `board`/`log_tail` 这两处接缝从哪个上游取字串：
/// `Text`＝逐帧追加的 ASCII（`render`），`Tui`＝清屏重绘的终端帧（`tui::draw`）。
/// 之所以是一个枚举而非两个 host 结构：两者除这两处外**逐字节相同**，并行两套就是把"换壳只换画帧"这道缝的
/// 证据拆成两份各抄一遍——改一处忘改另一处即失真。加一档（2D/3D）在此加一臂即可，不必再复制整台 host。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Frame {
    Text,
    Tui,
}

/// 一帧盘面的可见字串——**纯函数、按 `Frame` 分派上游**。
/// Text 分支必须与改前 `print!("\n{}", render(b))` 的字节逐一对应（byte-baseline 靠这条不破）。
/// Tui 分支＝清屏转义符 + `tui::draw`，与改前 `TuiHost::board` 同一串。
fn board_str(frame: Frame, b: &Battle) -> String {
    match frame {
        Frame::Text => format!("\n{}", crate::render::render(b)),
        Frame::Tui => format!("\x1b[2J\x1b[H{}", crate::tui::draw(b)),
    }
}

/// 日志尾部的可见字串——同 `board_str`，两分支各取上游、都不补换行（换行由调用点的 `println!` 负责）。
/// Tui 分支取 `view().log_tail`（与改前 `TuiHost::log_tail` 同一口径），Text 分支取 `render::log_tail`。
fn tail_str(frame: Frame, b: &Battle) -> String {
    match frame {
        Frame::Text => crate::render::log_tail(b, 12),
        Frame::Tui => b.view().log_tail(12),
    }
}

/// 日期串 `"YYYYMMDD"` → 种子的**位置加权求和**（2026-10-05 裁 Q-新7）。
/// 设计 §3.4 那句是"简单求和"，但等权求和实测**撑不住每日 3 抽**：2026-01-01 起 731 天只产出
/// 20 个不同值（值域 396..415，最挤一档 72 天共用同一条抽取序列）⇒ 每日挑战一年只变 20 张脸。
/// 加权（`sum = sum*131 + byte`）仍是"逐字符相加"那一族的一次循环，实测 3653 天 → 3653 个不同种子。
/// 溢出**回绕**（`wrapping`）：种子只要稳定可复现，不需要数学上的单射证明。
fn date_seed(y: u32, m: u32, d: u32) -> u64 {
    let text = format!("{y:04}{m:02}{d:02}");
    let mut h: u64 = 0;
    for b in text.as_bytes() {
        h = h.wrapping_mul(131).wrapping_add(*b as u64);
    }
    h
}

/// 格里历年月日 ← 纪元日序号（UTC 口径，纯算术、零依赖；Hinnant 的 `days_from_civil` 逆运算）。
/// 它是**回落路径**：非 unix，或 `localtime_r` 调用失败时，日界退到 UTC 零点（见 `daily_seed` 的诚实边界）。
fn civil_from_unix_days(days: i64) -> (u32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 { month_prime + 3 } else { month_prime - 9 };
    ((if month <= 2 { year + 1 } else { year }) as u32, month as u32, day as u32)
}

/// 机器本地日历上的"今天"＝ `(年, 月, 日)`。
/// unix 走 libc 的 `localtime_r`：std 本来就静态链接 libc，这不新增依赖，只是本仓**第一次**直接调它。
/// 取不到（非 unix／返回空指针／结构体不认）一律回落 UTC——**宁可日界差几小时，也不让每日挑战开不了**。
fn local_today() -> (u32, u32, u32) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    #[cfg(unix)]
    {
        #[repr(C)]
        struct Tm {
            tm_sec: i32, tm_min: i32, tm_hour: i32, tm_mday: i32,
            tm_mon: i32, tm_year: i32, tm_wday: i32, tm_yday: i32, tm_isdst: i32,
            tm_gmtoff: i64, tm_zone: *const i8,
        }
        unsafe extern "C" {
            fn tzset();
            fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
        }
        // SAFETY: `localtime_r` 是 POSIX 的重入版本，只要求第二个实参指向一块可写的 `struct tm`；
        // 上面的 `#[repr(C)]` 镜像 glibc 的字段顺序，`zeroed` 出的整块内存都在本函数栈上。
        let filled = unsafe {
            let mut tm: Tm = std::mem::zeroed();
            tzset();
            if localtime_r(&now, &mut tm).is_null() { None } else { Some(tm) }
        };
        if let Some(tm) = filled {
            // tm_year 是"自 1900 年起"、tm_mon 从 0 起——这两条换算是 C 的口径，不是本程序的日期口径。
            return (tm.tm_year.max(0) as u32 + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32);
        }
    }
    civil_from_unix_days(now.div_euclid(86_400))
}

/// §廿二:969 每日挑战＝本地种子，基于日期生成（日界＝**机器本地零点**，2026-10-05 裁 Q-新8；
/// 非 unix 或取本地时区失败才回落 UTC 零点）。
/// 诚实缺口：每日挑战规则第 2 步还要「固定卡组+特殊规则」，卡组名单文档**一字未给**；4 条特殊规则已按日
/// 抽取、但只报幕不生效（生效在帧 D3）⇒ 现在的 `daily` 是"按日场次＋按日额度＋按日规则序列的单场生存战"，
/// 仍不是文档意义上的每日挑战（该缺口挂成显式债，见 model.rs 债表）。
pub fn daily_seed() -> u64 {  // §廿一:921 每日挑战规则第 1 步＝本地种子，基于日期生成
    let (y, m, d) = local_today(); // §廿一:925 改时间不可防：文档自己声明"不影响游戏平衡"⇒ 本实现不设闸，也不做服务器校时
    date_seed(y, m, d)             // 算法＝日期数字按位权 131 累乘（2026-10-05 裁 Q-新7），不是文档那句"简单求和"
}

pub fn enemy_faction_for(level: u32) -> Faction {
    match level % 3 {
        1 => Faction::Frost,
        2 => Faction::Shadow,
        _ => Faction::Ember,
    }
}

/// 主线一关的遭遇（§二十一）：章末＝该章 Boss（阵营取 Boss 自己的），其余＝三阵营轮转。
pub fn encounter_for(level: u32) -> (Faction, Option<crate::boss::BossId>) {
    match crate::boss::boss_for_level(level) {
        Some(id) => (id.profile().faction, Some(id)),
        None => (enemy_faction_for(level), None),
    }
}

/// 一局的可变成状态：跨关继承堆 + 结转业力 + 后面还有没有关 + 落盘槽（`None`＝纯跳打，不落盘）。
struct RunState {
    inherit: Vec<CardInst>,
    carry_karma: i32,
    has_next: bool,
    save: Option<SaveSlot>,
    /// 这一局盖的每日特殊规则。只有 `daily_run` 会给它非 `None` 值，其余三条路（主线／爬梯／跳打 Boss）恒 `None`
    /// ⇒ `new_battle` 里那次 `with_daily_rule` 是空操作，普通对局逐字节照旧。
    daily_rule: DailyRule,
}

/// `one_level` 唯一认识的存档形态：它不需要知道这是主线还是每日，只需要知道**要不要按关写快照**。
#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveUse {
    /// 主线：每次进关写「本关入口」快照（`裁定27`）。
    Progress,
    /// 每日：只由调用方写**每日那五个键**（日戳／额度／抽取序列三键），关卡与继承堆一个字节都不动——
    /// 每日跑的是单场生存战，把它写进主线进度会凭空造出一条没人打过的"主线进度"。
    Daily,
}

/// 落盘槽：路径 + 内存里的档。内存档在战斗期间可能落后于现实，所以写入一律以当场的 `inherit` 为准。
struct SaveSlot {
    path: PathBuf,
    progress: Progress,
    kind: SaveUse,
}

impl SaveSlot {
    /// 开局读盘。**坏档一律 `exit 2`**（裁定28）：读不懂就当空档会撞上 `Battle::new` 的空堆兜底分支，
    /// 玩家以为在续第N关、实际拿到第1关牌序——那是比丢进度更糟的失真。
    /// 全新档（文件不存在）⇒ 用默认值继续，第一次快照就会建文件。
    /// 拆成 `try_open` + 这层薄壳是为了**让闸本身可测**：`process::exit` 在测试里会带走整个测试进程，
    /// 上一批"坏档拒载"的全部断言都只能打在 `from_kv` 上，`open` 里那两条出口零覆盖。
    fn open(path: PathBuf, faction: Faction, diff: Difficulty, resume: bool, kind: SaveUse) -> SaveSlot {
        SaveSlot::try_open(path, faction, diff, resume, kind).unwrap_or_else(|msg| {
            println!("✖ {msg}");
            std::process::exit(2);
        })
    }

    fn try_open(path: PathBuf, faction: Faction, diff: Difficulty, resume: bool, kind: SaveUse) -> Result<SaveSlot, String> {
        let loaded = match save::load_if_any(&path) {
            Ok(p) => p,
            Err(e) => {
                return Err(format!(
                    "存档打不开：{e}\n处置：`midline progress --save {}` 先看，确认要弃档再 `progress recover`（它改名留证，不删）。",
                    path.display()
                ));
            }
        };
        let Some(loaded) = loaded else {
            return Ok(SaveSlot { path, progress: Progress::new(faction, diff), kind });
        };
        if !resume {
            // 每日槽原样保留盘上那份：它只由 `daily_finish` 写，而写是**整文件重写**——
            // 换成 `Progress::new()` 就等于把别人的 level 与继承堆一起抹回第1关（曾经真抹了）。
            // 也不给它下面那条覆盖警告：说"会覆盖你的进度"对它是假话。
            if kind == SaveUse::Daily {
                return Ok(SaveSlot { path, progress: loaded, kind });
            }
            // 冷启动只接历史（通关标记/每日记录），不接进度：接了就是玩家没要求的续关。
            // "不接进度"在内存里是无害的，在盘上是覆盖写——所以只要档上真有进度就必须先说出来，
            // 不能让人以为 `--resume` 只是礼貌用语。
            if loaded.level != 1 || !loaded.inherit.is_empty() {
                let swap = if loaded.faction != faction {
                    format!("档上是 {}，本次是 {}——换阵营请另开一个 --save 文件。", loaded.faction.name(), faction.name())
                } else {
                    String::new()
                };
                println!(
                    "⚠ 档上已有主线进度：第{}关·继承堆{}张·结转业力{}。未给 --resume ⇒ 本次从第1关重开，第一次快照就覆盖它（通关与每日记录保留）。{swap}",
                    loaded.level,
                    loaded.inherit.len(),
                    loaded.carry_karma,
                );
            }
            let fresh = Progress::new(faction, diff);
            return Ok(SaveSlot {
                path,
                progress: Progress { completed: loaded.completed, daily_done: loaded.daily_done, ..fresh },
                kind,
            });
        }
        if let Some(msg) = loaded.faction_conflict(faction) {
            return Err(msg);
        }
        if loaded.completed {
            println!("⚠ 档上已有主线通关标记 ⇒ 续档无进行中主线，本次从第1关重开（通关记录保留）。");
            return Ok(SaveSlot {
                path,
                progress: Progress { completed: true, daily_done: loaded.daily_done, ..Progress::new(faction, diff) },
                kind,
            });
        }
        if let Some(w) = loaded.load_warning() {
            println!("⚠ {w}");
        }
        // 难度以**本次命令行**为准（阵营不行：卡名只在原阵营表里存在，换了＝牌面错误，不是口味问题）。
        println!(
            "续档：第{}关 · {} · 继承堆{}张 · 结转业力{}（难度取本次 {}）",
            loaded.level,
            faction.name(),
            loaded.inherit.len(),
            loaded.carry_karma,
            diff.label()
        );
        Ok(SaveSlot { path, progress: Progress { diff, ..loaded }, kind })
    }
}

/// 主线 60 关＝5 章，章末 Boss。通关/终结即返回。落点由调用方定死（定不出位置时 `main.rs` 直接报错退出，
/// 不存在"带着默认路径悄悄不落盘"这一条路）。
pub fn mainline_run(seed: u64, faction: Faction, diff: Difficulty, path: PathBuf, resume: bool, frame: Frame) {
    let mut st = RunState {
        inherit: Vec::new(),
        carry_karma: 0,
        has_next: true,
        save: Some(SaveSlot::open(path, faction, diff, resume, SaveUse::Progress)),
        daily_rule: DailyRule::None,
    };
    let mut level = st.save.as_ref().map_or(1, |s| s.progress.level);
    if let Some(s) = st.save.as_ref() {
        st.inherit = s.progress.inherit.clone();
        st.carry_karma = s.progress.carry_karma;
    }
    loop {
        let (out, _) = one_level(seed, faction, diff, level, &mut st, encounter_for, frame);
        match out {
            Outcome::PlayerWin if crate::boss::is_mainline_end(level) => {
                println!("终影已灭 —— 主线通关（60/60）。seed={seed} 可复现整局。");
                mark_completed(&mut st);
                return;
            }
            Outcome::PlayerWin => {
                if level.is_multiple_of(crate::boss::LEVELS_PER_CHAPTER) {
                    let ch = crate::boss::chapter_of(level);
                    println!("第{ch}章通关（Boss「{}」已灭）→ 第{}章解锁。", crate::boss::BossId::all()[ch as usize - 1].name(), ch + 1);
                }
                level += 1;  // §十二:505 进入下一关准备
            }
            _ => {
                println!("本局终结于第{level}关。seed={seed} 可复现整局。");
                return;
            }
        }
    }
}

/// 通关历史落盘：只翻 `completed`，关卡与牌维持第60关入口那份快照（`is_mainline_end` 是 `>=`，
/// 把 level 写成 61 会让下一次续档"赢一关就再报一次通关"，那是伪造进度）。
fn mark_completed(st: &mut RunState) {
    let Some(slot) = st.save.as_mut() else { return };
    if slot.progress.completed {
        return;
    }
    slot.progress.completed = true;
    persist(slot, "通关标记");
}

/// 唯一的落盘口：失败就明说并摘掉存档槽（本次运行不再尝试写），**内存进度照常继续**。
/// 把写盘失败当成致命错误退出去，等于让"存不进去"毁掉一局已经打完的游戏。
fn persist(slot: &mut SaveSlot, what: &str) -> bool {
    match slot.progress.save_to(&slot.path) {
        Ok(()) => true,
        Err(e) => {
            println!("✖ {what}写入失败：{e}（本次运行不再落盘，内存进度继续）");
            false
        }
    }
}

/// 每日挑战的免费额度（修正2.MD 3.5「每日免费次数：3」）。
pub const DAILY_ATTEMPTS: u32 = 3;

/// 规则号 → 名字。**名单本身在 `battle::DailyRule`**（引擎要按它分支，名单再留一份 meta 侧的就成两处真值）；
/// 这里只留"越界号报『今日无规则』而不是 panic"这一条存档口径。
pub fn daily_rule_name(rule: u32) -> &'static str {
    DailyRule::from_index(rule).map_or("（今日无规则）", DailyRule::name)
}

/// pity 阈值——**AI 自决的实现常量**，设计只裁了"配保底、文案不明写"，一字未给数。
/// 取 2 ＝ 同一条规则连抽两次是允许的，第三次强推他条。不取"永不连重"：保底是**下限触发**的兜底，
/// 不是把 PRNG 的自由度全关掉（连重本身也是玩家会当成"今天就这么背"的合法局面）。
const DAILY_PITY_RUN: u32 = 2;

/// SplitMix64 走一步（种子驱动、纯算术、同种子必同序列——2026-10-04 裁「抽取应使用伪随机，避免真随机」）。
fn split_mix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 抽今天的下一条特殊规则，并把序列三键（state／last／run）往前推一格。
/// 到 pity 阈值时是**强推他条**而不是重抽：重抽需要一个上限，而文档那条"重抽上限 3 次"（修正2.MD 3.3 规则2）
/// 本来只在"抽到短兵相接且关卡是 Boss"时才走——每日永远是第 1 关，那个条件在这台机器上凑不出来。
fn daily_draw(p: &mut Progress) -> u32 {
    let pool = DailyRule::POOL.len() as u32;
    let mut rule = (split_mix64(&mut p.daily_draw_state) % pool as u64) as u32;
    if rule == p.daily_rule_last && p.daily_rule_run >= DAILY_PITY_RUN {
        rule = (rule + 1) % pool;
    }
    p.daily_rule_run = if rule == p.daily_rule_last { p.daily_rule_run.saturating_add(1) } else { 1 };
    p.daily_rule_last = rule;
    rule
}

/// 日界重置＋放行判据（纯函数：判据只在这一处，`daily_run` 与常驻测读同一条，不留第二份真值）。
/// 返回 `false`＝今天的额度已经用完。日戳不是今天 ⇒ 当日计数作废、抽取序列换成当日种子（额度重满）。
fn daily_begin(p: &mut Progress, seed: u64) -> bool {  // §廿一:924 每日重置＝只比对**今天**的日种子：记过的是昨天，不挡今天重开
    if p.daily_done != seed {
        p.daily_done = seed;
        p.daily_attempts = 0;
        p.daily_draw_state = seed; // 当日规则序列的起点
        p.daily_rule_last = 0;
        p.daily_rule_run = 0;
    }
    p.daily_attempts < DAILY_ATTEMPTS
}

/// 一场打完的记账（纯函数）。通关判据＝**活满回合上限**（2026-10-05 裁「一次挑战＝单场 30 回合生存战」）：
/// 通关扣一次额度，中途败不扣（修正2.MD 3.5「失败不扣挑战次数」）。抽取序列 state 不在这里回滚——
/// 回滚就等于允许同一局面无限重抽同一条规则（2026-10-05 裁 Q-新3 的耦合判据）。
fn daily_settle(p: &mut Progress, survived: i64) -> bool {
    if survived < TURN_LIMIT {
        return false;
    }
    // saturating：额度是 u32，手改成 42 亿的档不该让程序在这行 panic（那是"读得懂的档"，不是坏档）。
    p.daily_attempts = p.daily_attempts.saturating_add(1);
    true
}

/// 每日挑战那一场的遭遇：**恒第 1 关、恒无 Boss**。
/// 文档给【短兵相接】【粘稠暗影】各写了一句「若关卡为 Boss 关卡 → 不触发」（修正2.MD 3.3），闸落在引擎
/// 应用点；这条遭遇函数是那两句"凑不出输入"的正面说法——每日这条路压根没有 Boss，所以重抽分支不写。
/// 单独成函数就是为了这件事**可断言**（`daily_encounter_never_raises_a_boss`），而不是留在注释里信一句。
fn daily_encounter(_level: u32) -> (Faction, Option<crate::boss::BossId>) {
    (enemy_faction_for(1), None)
}

/// 每日挑战：一天 3 次（`DAILY_ATTEMPTS`）、一次**一场单场生存战**（修正2.MD 3.5/3.6 ＋ 2026-10-05 裁）。
/// 打完就结束——不再是"无尽爬梯到第一次败北"：额度与奖励都以"场"计，爬梯那场没有额度可对。
/// 每次开场先抽一条当日特殊规则（`daily_draw`），并把它经 `RunState::daily_rule` 交给这一局的战斗
/// （`with_daily_rule`）；落盘在场次收束那一刻，通关再多记一次额度。
/// 诚实缺口（两条，各挂各的）：① 固定卡组仍未落——修正2.MD 3.2 那条"占位算法"要的是**10 张卡的名字**，
/// 名单待人给（挂在每日挑战规则第 2 步那条债上）；② 【献祭余响】那一条现况已成立、三种读法产出不同东西 ⇒ 挂 Q-新9 待裁，
/// 抽中它这一场不改对局（`DailyRule::effect` 里照实写着"待裁"，不念引擎不兑现的口号）。
pub fn daily_run(path: PathBuf, frame: Frame) {  // §廿一:916 每日挑战模式入口（复合行：本地种子已落，「固定规则」那半欠在每日挑战规则第 2 步的债上）
    let seed = daily_seed();
    let mut save = SaveSlot::open(path, Faction::Ember, Difficulty::Normal, false, SaveUse::Daily);
    if !daily_begin(&mut save.progress, seed) {
        println!("每日挑战 seed={seed}：今天的 {} 次已经用完（日戳={seed}）。每日 0 点后重来；想现在就再来一局，换个 --save 文件就行。", DAILY_ATTEMPTS);
        return;
    }
    let nth = save.progress.daily_attempts + 1;
    let rule = daily_draw(&mut save.progress);
    let daily = DailyRule::from_index(rule).unwrap_or_default();
    println!(
        "每日挑战 seed={seed} · 今日第 {nth}/{} 次 · 本场规则：{}——{}",
        DAILY_ATTEMPTS,
        daily.name(),
        daily.effect()
    );
    let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save: Some(save), daily_rule: daily };
    let (_, survived) = one_level(seed, Faction::Ember, Difficulty::Normal, 1, &mut st, daily_encounter, frame);
    let cleared = daily_finish(&mut st, survived);
    let verdict = if cleared {
        format!("通关（活满 {TURN_LIMIT} 回合）⇒ 本次额度已用掉。")
    } else {
        format!("未活满 {TURN_LIMIT} 回合 ⇒ 不扣额度，今天还能再来。")
    };
    println!("本场存活 {survived} 回合：{verdict} seed={seed} 可复现整场。");
}

/// 无 Boss、无 60 关上限的纯跳打爬梯（`play` 专用；它**从不**落盘，`save` 恒为 `None`）。
/// 每日挑战不再走这里——一次挑战是一场单局生存战，额度、通关与奖励都以"场"为单位计（`daily_run`）。
fn ladder(seed: u64, faction: Faction, diff: Difficulty, frame: Frame) {
    let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: true, save: None, daily_rule: DailyRule::None };
    let mut level = 1u32;
    loop {
        let (out, _) = one_level(seed, faction, diff, level, &mut st, |l| (enemy_faction_for(l), None), frame);
        match out {
            Outcome::PlayerWin => {
                level += 1;
            }
            _ => {
                println!("本局终结。seed={seed} 可复现整局。");
                return;
            }
        }
    }
}

/// 每日收尾：记账（纯函数 `daily_settle`）＋**每次收束都过一次落盘闸**。
/// 为什么失败那次也要走这里：抽取序列必须跨进程前进（2026-10-05 裁 Q-新3 的耦合判据"失败不回滚 state"）。
/// 但"过一次闸"不等于"重写一遍文件"——字节相同 `save_to` 就原地返回，所以**败局只在抽取序列真的换了一步时**
/// 才落盘，主线那几键从此不再被每日的败局反复重写（落盘是整文件重写 tmp+rename）。
/// 两条各钉一处：额度照旧不动（`daily_quota_resets_on_the_day_boundary_and_frees_on_failure`），
/// 主线各键照不动（`daily_done_mark_preserves_an_existing_mainline_progress`，那条测记着这里
/// 曾经真的抹掉过别人的 level 与继承堆）。
/// 弃局走不到这里：`one_level` 的 Quit 臂直接结束进程 ⇒ 弃局既不占额度，也不消耗抽取序列。
/// 返回是否通关。
fn daily_finish(st: &mut RunState, survived: i64) -> bool {
    let Some(slot) = st.save.as_mut() else { return false };
    let cleared = daily_settle(&mut slot.progress, survived);
    persist(slot, if cleared { "每日额度" } else { "每日抽取序列" });
    cleared
}

/// `play`：无限爬梯、无 Boss、不落盘。`--save/--resume` 在这儿没有意义，由 `main.rs` 直接拒（裁定28）。
pub fn play_run(seed: u64, faction: Faction, diff: Difficulty, frame: Frame) {
    ladder(seed, faction, diff, frame);
}

/// `boss <id>` / `play --boss <id>`：单关跳打某章末 Boss。
/// 跳打是"试一把"，不是"走一遍主线"：把它的 level/牌写进 `progress.kv`，就会凭空多出一段没人打过的进度，
/// 而解锁本来就能由 level 推导（`boss.rs`），另存一份＝第二套真值。所以本函数**永远不落盘**。
pub fn boss_run(seed: u64, faction: Faction, diff: Difficulty, id: crate::boss::BossId, frame: Frame) {
    use crate::boss::LEVELS_PER_CHAPTER;
    let level = id.chapter() * LEVELS_PER_CHAPTER;
    let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save: None, daily_rule: DailyRule::None };
    let (out, _) = one_level(seed, faction, diff, level, &mut st, |_| {
        (id.profile().faction, Some(id))
    }, frame);
    match out {
        Outcome::PlayerWin if id == crate::boss::BossId::ZhongYing => {
            println!("终影已灭 —— 主线通关（60/60）。seed={seed} 可复现整局。")
        }
        Outcome::PlayerWin => println!("击败第{}章 Boss「{}」。seed={seed} 可复现整局。", id.chapter(), id.name()),
        _ => println!(
            "Boss「{}」未被击败：{}同样不解锁下一章（裁定22）。",
            id.name(),
            if out == Outcome::Draw { "平局" } else { "败北" }
        ),
    }
}

/// 关隘头报。普通关从第2章起标出敌方强化量——数值既然进了对局，就得在屏幕上看得见；
/// 章1 保持旧文案逐字不变。Boss 关报章号与名号，不报强化（豁免见 `one_level`）。
pub(crate) fn level_head(level: u32, boss: Option<crate::boss::BossId>) -> String {
    let strength = Battle::chapter_strength(level);
    match boss {
        Some(id) => format!(
            "主线 第{}章 第{level}关 · Boss「{}」· {}",
            crate::boss::chapter_of(level),
            id.name(),
            id.profile().title
        ),
        None if strength > 0 => format!("第 {level} 关（第{}章·敌方强化 +{strength}）", crate::boss::chapter_of(level)),
        None => format!("第 {level} 关"),
    }
}

/// 存档的唯一按关写入口＝**关隘入口快照**（裁定27）。
/// 为什么是这个时刻：`Battle::new` 会用 `std::mem::take` 把继承堆搬空，战斗进行中的 `st.inherit` 恒为空数组，
/// 那时候写盘＝把"10张遗产"写成"0张"。所以只在堆还完整、且准备阶段编辑已经做完的那一刻写，
/// 并且直接读传入的活堆——不留"改了内存忘了同步槽"这种缝。
/// 由此得到的语义是闭合的：败/平/弃局都不动盘 ⇒ 下一次续上的就是**这一关的入口**，
/// 阵亡的牌跟着重来一遍（它们没在"已通关的关"里死掉）。写失败的处置见 `persist`。
fn snapshot_entry(save: &mut Option<SaveSlot>, level: u32, faction: Faction, diff: Difficulty, inherit: &[CardInst], karma: i32) {
    let Some(slot) = save.as_mut() else { return };
    if slot.kind != SaveUse::Progress {
        return;
    }
    slot.progress.level = level;
    slot.progress.faction = faction;
    slot.progress.diff = diff;
    slot.progress.carry_karma = karma;
    slot.progress.inherit = inherit.to_vec();
    // 写失败就摘掉存档槽：不摘＝每一关都再喷一次同样的错误，把游戏界面变成磁盘报错回放。
    if persist(slot, "进度快照") {
        return;
    }
    *save = None;
}

/// 打赢一关之后的落盘＝**下一关的入口**快照。第60关不写"第61关入口"：那是越界值，
/// `is_mainline_end` 是 `>=`，写了就是伪造进度（下一次续档会立刻再报一次通关）。
/// 闸写在函数里而不是调用点，是为了让"记得判一下"这个会漏的步骤没有地方可漏。
fn persist_next_entry(
    save: &mut Option<SaveSlot>,
    level: u32,
    faction: Faction,
    diff: Difficulty,
    inherit: &[CardInst],
    karma: i32,
) {
    if crate::boss::is_mainline_end(level) {
        return;
    }
    snapshot_entry(save, level + 1, faction, diff, inherit, karma);
}

/// 一关的外围＝**这块壳**：把会话层（`session::run_level`）的四个接缝落到 stdin/stdout/磁盘上，
/// 再提供"本关怎么构造"（阵营轮转与 Boss 遭遇）。阶段机本身不在这儿，所以摘壳时只换这个函数与 `PlayHost`。
/// `frame` 决定画帧上游（Text/Tui），除此之外两条壳路径共用**同一个** host、同一条落盘与遭遇逻辑。
/// 交回来的是 `(胜负, 存活回合数)`：主线只认前者，每日两者都要——它的唯一结算标准是回合数（修正2.MD 3.6），
/// 而"胜负"分不出"第12回合烛尽"与"打满30回合但蜡烛短一格"。
fn one_level<F: Fn(u32) -> (Faction, Option<crate::boss::BossId>)>(
    seed: u64,
    faction: Faction,
    diff: Difficulty,
    level: u32,
    st: &mut RunState,
    enc: F,
    frame: Frame,
) -> (Outcome, i64) {
    // 解构而非整体借用：会话层要 `inherit`／`carry_karma`，落盘要 `save`，三条各走各的字段。
    let RunState { inherit, carry_karma, has_next, save, daily_rule } = st;
    // 遭遇**只算一次**（改前也是"先定遭遇、再拿去两头用"）。存成值而不是把闭包 `enc` 留着再调一遍：
    // `enc` 是无状态查表，但"同一关两头各查一次"这种事，一旦哪天它有了副作用就是第二套真值。
    let (foe, boss) = enc(level);
    let cfg = crate::session::LevelCfg { head: level_head(level, boss), level, has_next: *has_next };
    let mut host = PlayHost { seed, faction, diff, foe, boss, save, frame, daily_rule: *daily_rule };
    match crate::session::run_level(&mut host, &cfg, inherit, carry_karma) {
        crate::session::Terminus::Done { out, survived } => (out, survived),
        crate::session::Terminus::Quit { msg } => {
            if let Some(m) = msg {
                println!("{m}");
            }
            std::process::exit(0);
        }
    }
}

/// `Host` 的实现——全仓唯一还允许 `stdin().read_line()` 与 `process::exit()` 的地方。
/// `frame` 只改 `board`/`log_tail` 两处上游；其余接缝与改前的 `CliHost` 逐字节一致。
/// 逐字节口径：`write` 原样打印（会话层已经带好换行，这里不补），Text 分支的 `board`/`log_tail` 与改前的
/// `print!("\n{}", render(&b))`、`println!("{}", log_tail(&b, 12))` 一一对应。
/// flush 只在 `write` 后做一次：改前是"打完提示符必 flush"，改成"任何输出之后 flush"，字节不变、终端不再吞行。
struct PlayHost<'a> {
    seed: u64,
    faction: Faction,
    diff: Difficulty,
    /// 本关遭遇，由 `one_level` 算好一次存这儿——头报与构造战斗用的是**同一个** `(阵营, Boss)`。
    foe: Faction,
    boss: Option<crate::boss::BossId>,
    save: &'a mut Option<SaveSlot>,
    /// 画帧上游：Text＝`render`，Tui＝`tui::draw`。除 `board`/`log_tail` 外没有任何接缝读它。
    frame: Frame,
    /// 本关盖的每日特殊规则（`None`＝普通对局）。与 `foe`/`boss` 同类：由 `one_level` 算好一次存这儿，
    /// 只有 `new_battle` 读它，所以 trait 那五个接缝一个字都不改。
    daily_rule: DailyRule,
}

impl crate::session::Host for PlayHost<'_> {
    fn write(&mut self, s: &str) {
        use std::io::Write;
        print!("{s}");
        std::io::stdout().flush().ok();
    }

    fn board(&mut self, b: &Battle) {
        print!("{}", board_str(self.frame, b));
    }

    fn log_tail(&mut self, b: &Battle) {
        println!("{}", tail_str(self.frame, b));
    }

    fn read(&mut self) -> Option<String> {
        // 返回连行尾换行的原始行，与改前 `read_line(&mut line)` 喂给解析器的同一个字串。
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 { None } else { Some(line) }
    }

    fn snapshot_entry(&mut self, level: u32, inherit: &[CardInst], karma: i32) {
        snapshot_entry(&mut *self.save, level, self.faction, self.diff, inherit, karma);
    }

    fn snapshot_next(&mut self, level: u32, inherit: &[CardInst], karma: i32) {
        persist_next_entry(&mut *self.save, level, self.faction, self.diff, inherit, karma);
    }

    /// Boss 关豁免章强化：B1 的贪心全败读数要与改前逐帧可比（裁定24 只补"每章新阵营"的普通关缺口）；
    /// 普通关走 `new_mainline`＝构造即强化，省掉"记得再补一刀"这个会漏的步骤。
    fn new_battle(&mut self, inherit: &mut Vec<CardInst>, level: u32) -> Battle {
        match self.boss {
            Some(id) => Battle::new_boss(self.seed, self.faction, id, std::mem::take(inherit), level),
            None => Battle::new_mainline(self.seed, self.faction, self.foe, self.diff, std::mem::take(inherit), level)
                .with_daily_rule(self.daily_rule),
        }
    }
}

/// 自动对局冒烟：玩家侧也走贪心，验证规则闭环不 panic、能分胜负。
/// `diff` 是敌方 AI 档位；专家档额外让托管侧（有继承堆的一侧）在结算阶段由 AI 决策融合/升级。
pub fn auto_battles(n: u32, diff: Difficulty) {
    for i in 0..n {
        let seed = 1000 + i as u64;
        let mut inherit: Vec<CardInst> = Vec::new();
        let mut level = 1u32;
        let mut last = Outcome::Draw;
        for _ in 0..5 {
            // 与 one_level 同一套敌方构造（`new_mainline`＝构造即章强化）：本循环当前最多到第5关
            // （第1章，强化量 0），加上来是为了将来把轮数拉长读章节曲线时，托管冒烟与主线玩的不是两种游戏。
            let mut b =
                Battle::new_mainline(seed, Faction::Ember, enemy_faction_for(level), diff, std::mem::take(&mut inherit), level);
            auto_play(&mut b);
            last = b.over.unwrap_or(Outcome::Draw);
            match last {
                Outcome::PlayerWin => {
                    for l in crate::progress::collect_survivors(&mut b, &mut inherit) {
                        println!("{l}");
                    }
                    // 融合/升级：验证跨关持有与 meta 决策路径
                    let mut k = b.p_karma.max(0);
                    apply_meta_plan(&mut inherit, &mut k, diff, 1);
                    level += 1;
                }
                _ => break,
            }
        }
        println!("auto#{i} [{}] seed={seed} → {last:?} 到达第{level}关", diff.label());
    }
}

/// 战斗外（准备/结算阶段）的托管决策落地（§二十「专家含融合决策」；时机按 §九:317／§九:319 那张融合时机表，战斗内没有这条路）。
/// 只把 `ai::plan_meta` 的意图回灌既有规则函数执行，AI 不自开一套结算。
/// 非专家档保持既有冒烟行为：固定融合前两牌、不代管升级。
fn apply_meta_plan(inherit: &mut Vec<CardInst>, karma: &mut i32, diff: Difficulty, upgrades_left: u8) {
    if diff != Difficulty::Expert {
        if inherit.len() >= 2 {
            let _ = crate::progress::fuse_cards(inherit, 0, 1, karma);
        }
        return;
    }
    // plan 的下标基于同一快照；先执行不改长度的升级，再执行会摘牌的融合
    let plan = crate::ai::plan_meta(inherit, *karma, upgrades_left);
    let mut run = |m: &crate::ai::MetaMove| match m {
        crate::ai::MetaMove::Upgrade { idx, kind } => match crate::progress::upgrade_card(inherit, *idx, kind) {
            Ok(msg) => println!("AI·升级[{kind}] {msg}"),
            Err(e) => println!("✖ AI·升级 {e}"),
        },
        crate::ai::MetaMove::Fuse { main, sub } => match crate::progress::fuse_cards(inherit, *main, *sub, karma) {
            Ok(msg) => println!("AI·融合 {msg}"),
            Err(e) => println!("✖ AI·融合 {e}"),
        },
    };
    for m in plan.iter().filter(|m| matches!(m, crate::ai::MetaMove::Upgrade { .. })) {
        run(m);
    }
    for m in plan.iter().filter(|m| matches!(m, crate::ai::MetaMove::Fuse { .. })) {
        run(m);
    }
}

fn auto_play(b: &mut Battle) {
    let mut guard = 0;
    while b.over.is_none() && guard < 300 {
        guard += 1;
        auto_turn(b);
    }
}

/// 我方托管的一个回合（`auto` 与 Boss 冒烟共用同一驱动）。
pub(crate) fn auto_turn(b: &mut Battle) {
    if b.over.is_some() {
        return;
    }
    if b.p_karma == 0 {
        if let Some(i) = b.hand.iter().position(|c| c.is_starter()) {
            let _ = b.player_sacrifice_hand(i);
        }
    }
    for _ in 0..8 {
        let idx = (0..b.hand.len())
            .filter(|i| {
                let c = &b.hand[*i];
                (if c.is_starter() { 0 } else { c.def.cost }) <= b.p_karma
                    && !b.pf.sacrificed_names.contains(&c.def.name)
            })
            .max_by_key(|i| b.hand[*i].hp);
        if let Some(idx) = idx {
            let free_slot = (0..4).find(|c| b.p_front[*c].is_none());
            let slot = free_slot.unwrap_or_else(|| b.rng.below(4));
            if b.player_place(idx, slot).is_ok() {
                continue;
            }
        }
        // 献祭仅当"献祭后能放上更强的卡"时才做，避免烧掉唯一输出
        let mut sac: Option<usize> = None;
        if !b.pf.sacrifice_used {
            for col in 0..4 {
                let Some((gain, v_cost, v_hp)) = b.p_front[col].as_ref().map(|v| {
                    (if v.is_starter() { 2 } else { v.def.cost }, v.def.cost, v.hp)
                }) else {
                    continue;
                };
                if b.turn - b.p_front[col].as_ref().unwrap().placed_turn < 1 {
                    continue;
                }
                let better = b.hand.iter().any(|c| {
                    let cost = if c.is_starter() { 0 } else { c.def.cost };
                    cost <= b.p_karma + gain
                        && !b.pf.sacrificed_names.contains(&c.def.name)
                        && (c.hp > v_hp || cost > v_cost)
                });
                if better {
                    sac = Some(col);
                    break;
                }
            }
        }
        if let Some(col) = sac {
            if b.player_sacrifice_field(col).is_ok() {
                continue;
            }
        }
        break;
    }
    if b.pf.manual_draws > 0 && b.hand.len() < crate::battle::HAND_LIMIT && !b.draw_pile.is_empty() {
        let _ = b.action_draw(false);
    }
    b.end_player_turn();
}

#[cfg(test)]
mod meta_tests {
    use super::*;
    use crate::boss::BossId;
    use crate::model::faction_cards;

    /// 纪元日序号 → 格里历年月日的已知向量。回落路径（非 unix／`localtime_r` 失败）的全部真值来源，逐条钉死。
    /// 数是从 Python 的 `date` 独立算的，不是"跑一遍本实现再抄回来"。
    #[test]
    fn utc_civil_conversion_matches_the_stored_vectors() {
        for (days, want) in [
            (0, (1970, 1, 1)),
            (-1, (1969, 12, 31)),
            (11_016, (2000, 2, 29)),
            (19_782, (2024, 2, 29)),
            (20_454, (2026, 1, 1)),
            (20_730, (2026, 10, 4)),
            (20_731, (2026, 10, 5)),
            (20_818, (2026, 12, 31)),
            (20_819, (2027, 1, 1)),
            (84_005, (2199, 12, 31)),
        ] {
            assert_eq!(civil_from_unix_days(days), want, "纪元日 {days} 该换成 {want:?}");
        }
    }

    /// 位置加权求和：先钉一个**外部算出**的具体号（常数 131 与"逐字节"这条口径不许悄悄换），
    /// 再钉十年逐日一号一个——等权"简单求和"实测 731 天只有 20 个不同值，那样的日种子撑不住每日 3 抽。
    #[test]
    fn date_seed_is_pinned_and_one_to_one_with_the_day() {
        assert_eq!(date_seed(2026, 10, 5), 33_347_663_914_430_850, "20261005 的日种子号换了就是算法换了");
        assert_eq!(date_seed(2026, 10, 4), 33_347_663_914_430_849, "相邻两天该差一号，不是差一坨");
        let mut seen = std::collections::HashSet::new();
        for day in 20_454..20_454 + 3653 {
            let (y, m, d) = civil_from_unix_days(day);
            assert!(seen.insert(date_seed(y, m, d)), "纪元日 {day}（{y}-{m:02}-{d:02}）与前面某天撞了种子");
        }
        assert_eq!(seen.len(), 3653, "十年逐日不该有一个重号");
    }

    /// 本地那一格的**接线**检查：给的必须是一个真日期，且与 UTC 口径至多差一天。
    /// 它不证"时区算对了"——那要靠换 `TZ` 复跑（帧档记了实测：`TZ=Etc/GMT+11` 时号退到 20261004 那一档）。
    /// 这条只钉：`struct tm` 字段顺序读错、`tm_year+1900`／`tm_mon+1` 的 C 口径漏掉，当场红。
    #[test]
    fn local_today_is_a_real_date_within_a_day_of_the_utc_reading() {
        let (y, m, d) = local_today();
        assert!((2024..2200).contains(&y), "年 {y} 离谱：tm_year 的 +1900 大概漏了");
        assert!((1..=12).contains(&m), "月 {m} 越界：tm_mon 的 +1 大概漏了");
        assert!((1..=31).contains(&d), "日 {d} 越界：tm 字段大概读串了");
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let day = (now as i64).div_euclid(86_400);
        let nearby = [-1i64, 0, 1].map(|k| {
            let (uy, um, ud) = civil_from_unix_days(day + k);
            date_seed(uy, um, ud)
        });
        assert!(nearby.contains(&daily_seed()), "本地日种子与 UTC 口径差出三天以上：FFI 那条路读错了字段");
    }

    #[test]
    fn encounter_for_mounts_boss_only_on_chapter_ends() {
        // 普通关：三阵营轮转、无 Boss（与既有 enemy_faction_for 同一套真值，不开第二套）
        for lvl in [1u32, 11, 13, 23, 59] {
            assert_eq!(encounter_for(lvl), (enemy_faction_for(lvl), None), "第{lvl}关不该有 Boss");
        }
        // 章末：该章 Boss，阵营取 Boss 自己的（炎与冰主场是烬火，霜誓只作第二身份）
        assert_eq!(encounter_for(12), (Faction::Ember, Some(BossId::Luzhu)));
        assert_eq!(encounter_for(24), (Faction::Frost, Some(BossId::Xuejue)));
        assert_eq!(encounter_for(36), (Faction::Shadow, Some(BossId::Yingzhang)));
        assert_eq!(encounter_for(48), (Faction::Ember, Some(BossId::YanBing)));
        assert_eq!(encounter_for(60), (Faction::Shadow, Some(BossId::ZhongYing)));
    }

    #[test]
    fn survivors_are_all_undefeated_cards_no_starters() {
        let mut b = Battle::new(11, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        for i in 0..4u64 {
            let mut c = CardInst::new(50 + i, faction_cards(Faction::Ember)[1]);
            c.seq = 100 + i;
            b.p_front[i as usize] = Some(c);
        }
        let hand_before = b.hand.len();
        let pile_before = b.draw_pile.len();
        b.discard_pile.push(CardInst::new(70, faction_cards(Faction::Ember)[2]));
        let survivors = b.battle_survivors();
        assert_eq!(
            survivors.len(),
            4 + (hand_before - 1) + pile_before + 1,
            "剩余 = 场上+手牌+牌堆+弃牌堆，开端被过滤（-1），弃牌堆回归（+1）"
        );
        assert!(survivors.iter().all(|c| !c.is_starter()), "§五:185：开端不入继承堆");
        assert!(b.hand.is_empty() && b.draw_pile.is_empty() && b.discard_pile.is_empty());
    }

    #[test]
    fn level_head_shows_the_ramp_but_stays_quiet_on_chapter_one_and_boss() {
        assert_eq!(level_head(1, None), "第 1 关", "第1章头报必须与旧文案逐字相同");
        assert_eq!(level_head(12, None), "第 12 关", "第12关仍属第1章");
        assert_eq!(level_head(13, None), "第 13 关（第2章·敌方强化 +1）");
        assert!(level_head(37, None).contains("敌方强化 +3"), "第4章封顶值要可见");
        let boss = level_head(24, Some(crate::boss::BossId::Xuejue));
        assert!(boss.contains("主线 第2章 第24关") && boss.contains("Boss「"), "Boss 关头报：{boss}");
        assert!(!boss.contains("强化"), "Boss 关豁免章强化，头报不该谎称有");
    }

    /// 裁定24 的**读数**（不是断言平衡）：每章首关 × **三个敌方阵营** × 三个难度档，各打「未强化」与「已强化」两场贪心托管。
    /// 阵营这一轴是补的窟窿：章首关 `[1,13,25,37,49]` 全部 `% 3 == 1` ⇒ 只取 `encounter_for`
    /// 会五章打一味的霜阵营，而阵营差（霜 0.00% vs 烬 5.21%）比强化效应还大，读数只描述一种敌人。
    /// 断言只锁"能分出结果、不越回合闸、不 panic"；对照与实验同 seed，读数随
    /// `cargo test -- --nocapture` 打印并回填 pool/chapter-strengthening.md §2。
    #[test]
    fn chapter_ramp_matrix_reaches_a_decision() {
        let mut rows = Vec::new();
        for level in [1u32, 13, 25, 37, 49] {
            assert!(encounter_for(level).1.is_none(), "第{level}关不该是 Boss 章末");
            for foe in [Faction::Ember, Faction::Frost, Faction::Shadow] {
                for d in [Difficulty::Normal, Difficulty::Hard, Difficulty::Expert] {
                    for strengthen in [false, true] {
                        let mut b = Battle::new(4242, Faction::Ember, foe, d, Vec::new(), level);
                        if strengthen {
                            b.apply_chapter_strengthening(level);
                        }
                        auto_play(&mut b);
                        let out = b.over.expect("每章首关都必须分出结果（不得卡在 300 步护栏里）");
                        assert!(b.turn <= b.turn_limit, "第{level}关 [{}] 越回合闸：turn={}", d.label(), b.turn);
                        rows.push((level, foe, d.label(), strengthen, out, b.turn));
                    }
                }
            }
        }
        println!("裁定24 章强化读数（seed=4242，我方＝贪心托管、空继承堆单关；强化前=对照）：");
        for (level, foe, d, st, out, turn) in rows {
            let ch = crate::boss::chapter_of(level);
            let s = Battle::chapter_strength(level);
            let tag = if st { format!("强化+{s}") } else { "未强化  ".into() };
            println!("  第{level:>2}关（第{ch}章）敌{} {d:<3} {tag} → {out:?}，{turn} 回合", foe.name());
        }
    }

    use std::sync::atomic::{AtomicU32, Ordering};
    static SAVE_SEQ: AtomicU32 = AtomicU32::new(0);

    /// 存档写入落点：`/tmp` 下按 pid+计数器取唯一路径，绝不碰真实 `$HOME`。
    fn snap_path(tag: &str) -> PathBuf {
        let n = SAVE_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("midline-snap-{}-{n}-{tag}.kv", std::process::id()))
    }

    fn slot(path: PathBuf, kind: SaveUse) -> Option<SaveSlot> {
        Some(SaveSlot { path, progress: Progress::new(Faction::Ember, Difficulty::Normal), kind })
    }

    /// 快照写入的是**这一次当场传进来的东西**，不是内存里那份可能落后的档——五个字段少同步一个，
    /// 续档就拿回旧等级或旧牌堆，而这在 play 现场看不出来。
    #[test]
    fn snapshot_writes_every_argument_not_the_stale_memory_copy() {
        let path = snap_path("full");
        let mut save = slot(path.clone(), SaveUse::Progress);
        let pile = vec![
            CardInst::new(3, faction_cards(Faction::Ember)[1]),
            CardInst::new(9, faction_cards(Faction::Ember)[9]),
        ];
        snapshot_entry(&mut save, 7, Faction::Ember, Difficulty::Hard, &pile, 3);
        let got = Progress::load_from(&path).expect("快照应能被校验器读回");
        assert_eq!((got.level, got.carry_karma, got.diff), (7, 3, Difficulty::Hard));
        assert_eq!(got.inherit.len(), 2, "牌堆按当场传入的那份写，不按内存");
        assert_eq!(got.inherit[1].id, 9, "id 是战斗输入，快照不得重编号");
        assert!(save.is_some(), "写成功 ⇒ 存档槽留着");
        std::fs::remove_file(&path).ok();
    }

    /// 每日槽只许动**每日那五个键**。让它顺手写进度就等于凭空造出一条没人打过的"主线进度"——
    /// 这个门禁是纯字段判断，一旦有人把 `daily` 改成共用 `Progress` 槽，全靠这条响。
    /// 注意它**只**管得住 `snapshot_entry` 这一侧；"记一次每日反而抹掉主线"是另一条路，
    /// 由 `daily_done_mark_preserves_an_existing_mainline_progress` 管（那条曾经真的漏了）。
    #[test]
    fn daily_slot_never_records_progress() {
        let path = snap_path("daily");
        let mut save = slot(path.clone(), SaveUse::Daily);
        snapshot_entry(&mut save, 5, Faction::Ember, Difficulty::Normal, &[], 9);
        assert!(!path.exists(), "每日槽不该建主线档");
        assert_eq!(save.unwrap().progress.level, 1, "内存档也不该被推进");
    }

    /// 每日记一次通关，走的是**整文件重写**——所以它不能顺带把主线档抹回第1关。
    /// 危险在于 `daily_run` 用 `resume=false` 开槽，而冷启动分支把内存档换成 `Progress::new()`，
    /// 于是 `daily_finish` 写回的是"第1关+空堆"。开槽和写入都走真实函数，不手工构造中间态。
    #[test]
    fn daily_done_mark_preserves_an_existing_mainline_progress() {
        let path = snap_path("daily-keeps-progress");
        let mut pre = slot(path.clone(), SaveUse::Progress);
        let pile = vec![
            CardInst::new(3, faction_cards(Faction::Ember)[1]),
            CardInst::new(9, faction_cards(Faction::Ember)[9]),
        ];
        snapshot_entry(&mut pre, 5, Faction::Ember, Difficulty::Normal, &pile, 3);

        // 开槽与写入之间隔着的正是 `daily_run` 里那步开局判据：日戳由它换，额度由 `daily_finish` 记。
        // 两步都走真实函数，不手工构造中间态——手工设日戳就等于把"谁负责日戳"这条又写一遍。
        let mut opened = SaveSlot::try_open(path.clone(), Faction::Ember, Difficulty::Normal, false, SaveUse::Daily)
            .expect("合法档应能开出每日槽");
        assert!(daily_begin(&mut opened.progress, daily_seed()), "今天第一次进来必须放行");
        let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save: Some(opened), daily_rule: DailyRule::None };
        assert!(daily_finish(&mut st, TURN_LIMIT), "活满回合上限就是通关，这条出口不该是假阴性");

        let got = Progress::load_from(&path).expect("写入后档仍应能被自己的校验器读回");
        assert_eq!(got.level, 5, "打一次每日不该把主线进度抹回第1关");
        assert_eq!(got.inherit.len(), 2, "继承堆不该被抹平");
        assert_eq!(got.inherit[0].id, 3, "id 是战斗输入，重写不得重编号");
        assert_eq!(got.carry_karma, 3, "结转业力不该被抹平");
        assert_eq!(got.daily_done, daily_seed(), "每日记录本身要写上");
        assert_eq!(got.daily_attempts, 1, "通关扣一次额度，且只扣一次");
        // 中途败那一次：额度照旧不动，但**抽取序列必须落盘**。失败不回滚 state 是 Q-新3 定的耦合判据；
        // `save_to` 那道"字节相同就不重写"的闸在这里挡不住——序列确实前进了一步，落盘是真改动。
        let mut opened2 = SaveSlot::try_open(path.clone(), Faction::Ember, Difficulty::Normal, false, SaveUse::Daily)
            .expect("合法档应能开出每日槽");
        assert!(daily_begin(&mut opened2.progress, daily_seed()), "额度还剩就该继续放行");
        let drawn = daily_draw(&mut opened2.progress);
        let advanced_state = opened2.progress.daily_draw_state;
        let mut run2 = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save: Some(opened2), daily_rule: DailyRule::None };
        assert!(!daily_finish(&mut run2, TURN_LIMIT - 1), "差一个回合就是没活满");
        let after = Progress::load_from(&path).expect("失败那次落盘的档仍应能读回");
        assert_eq!(after.daily_attempts, 1, "失败不扣次数：额度还停在通关那一次");
        assert_eq!(after.daily_draw_state, advanced_state, "抽过一步的序列必须活着落盘，否则下一场重抽同一条");
        assert_eq!(after.daily_rule_last, drawn, "pity 的\"上一条\"要跟着走");
        assert_eq!(after.level, 5, "失败那次同样不许碰主线关卡");
        assert_eq!(after.inherit.len(), 2, "…继承堆也不许");
        assert_eq!(after.carry_karma, 3, "…结转业力也不许");
        std::fs::remove_file(&path).ok();
    }

    /// 抽取机件本身：同一天种子 ⇒ 同一条序列（可复现），跨进程接着走（Q-新3），pity 挡死三连同（Q-新4）。
    #[test]
    fn the_daily_draw_replays_the_same_sequence_and_pity_caps_the_run() {
        let seed = date_seed(2026, 10, 5);
        let mut a = Progress::new(Faction::Ember, Difficulty::Normal);
        let mut b = Progress::new(Faction::Ember, Difficulty::Normal);
        assert!(daily_begin(&mut a, seed) && daily_begin(&mut b, seed));
        let mut runs = [0u32; 4];
        let mut max_run = 0u32;
        let mut seen = std::collections::HashSet::new();
        for k in 0..400 {
            let drawn = daily_draw(&mut a);
            assert!(drawn < 4, "抽出的规则号 {drawn} 越界（规则池就 4 条）");
            assert_eq!(a.daily_rule_last, drawn, "\"上一条\"就该是刚抽的那条");
            if k == 2 {
                // 第三场之前先"重启"一次：把内存档写回去再读回来，序列必须接着走而不是从头再来
                let back = Progress::from_kv(&a.to_kv()).expect("自产档必须能读回");
                assert_eq!(back.daily_rule_last, a.daily_rule_last, "跨进程：上一条得记得");
                assert_eq!(back.daily_rule_run, a.daily_rule_run, "跨进程：连中次数得记得");
                assert_eq!(back.daily_draw_state, a.daily_draw_state, "跨进程：PRNG state 得记得");
            }
            if k < 4 {
                // 头几场拿两条独立实现的序列对撞：同种子必须同一条（SplitMix64 的每一步都是纯函数）
                assert_eq!(drawn, daily_draw(&mut b), "同一日种子的序列不该分叉");
            }
            max_run = max_run.max(a.daily_rule_run);
            runs[drawn as usize] += 1;
            seen.insert(drawn);
        }
        assert!(max_run <= 2, "pity 该把连中挡在 2，实测连中 {max_run} 次");
        assert_eq!(seen.len(), 4, "四条规则一条都没抽到，抽取面就有死角：{seen:?}");
        assert!(runs.iter().all(|n| *n > 0), "四条都该有出场：{runs:?}");
    }

    /// 旧日戳（帧 D1 之前那版程序记的 `epoch/86400` 日号）与新算法的种子必然不等 ⇒ 当日额度重满、序列换起点。
    /// 这不是 bug 而是**部署日的既有后果**，所以把它钉成一条测：谁改了种子算法而忘了这条，这里当场红。
    #[test]
    fn an_old_epoch_day_stamp_rolls_over_to_today_and_refills_the_quota() {
        let seed = daily_seed();
        let mut p = Progress::new(Faction::Ember, Difficulty::Normal);
        p.daily_done = 20_731; // D1 那版的口径：今天的 epoch 日号（旧算法留下的档面）
        p.daily_attempts = 3;
        p.daily_rule_last = 2;
        p.daily_rule_run = 2;
        assert!(daily_begin(&mut p, seed), "旧日戳不等于今天 ⇒ 放行");
        assert_eq!(p.daily_attempts, 0, "跨日：额度重满");
        assert_eq!(p.daily_done, seed, "日戳换成新算法的号");
        assert_eq!((p.daily_rule_last, p.daily_rule_run), (0, 0), "pity 计数随日作废");
        assert_eq!(p.daily_draw_state, seed, "抽取序列从当日种子重新起头");
        assert!(seed > 1_000_000, "新日种子是日期串的加权号，量级掉回日号就说明算法又换回去了：{seed}");
    }

    /// 每日额度的三条判据钉在一起：跨日重开、同日三次用满、失败不扣。判据是纯函数，
    /// 所以这里量的是**规则本身**，不是"壳有没有调它"（壳那条路由 byte-baseline 的每日场景量）。
    #[test]
    fn daily_quota_resets_on_the_day_boundary_and_frees_on_failure() {
        let seed = daily_seed();
        let mut p = Progress::new(Faction::Ember, Difficulty::Normal);
        // 昨天用满过：日戳不是今天 ⇒ 计数作废、抽取 state 换成当日种子、额度重满。
        p.daily_done = seed.saturating_sub(1);
        p.daily_attempts = DAILY_ATTEMPTS;
        p.daily_draw_state = 999;
        assert!(daily_begin(&mut p, seed), "跨日必须放行");
        assert_eq!(
            (p.daily_done, p.daily_attempts, p.daily_draw_state, p.daily_rule_last, p.daily_rule_run),
            (seed, 0, seed, 0, 0),
            "重置要五个键一起换，少一个就是第二套真值（pity 计数留在昨天＝今天第一场可能被误判成连中）"
        );
        // 今天打满三次 ⇒ 用完；用完之后同一天再进来必须被挡住。
        for _ in 0..DAILY_ATTEMPTS {
            assert!(daily_settle(&mut p, TURN_LIMIT));
        }
        assert_eq!(p.daily_attempts, DAILY_ATTEMPTS);
        assert!(!daily_begin(&mut p, seed), "额度用完当天不许再来");
        assert!(daily_begin(&mut p, seed + 1), "但到明天必须又是满格");
        // 失败不扣：29 回合与 0 回合都是"没活满"，且抽取 state 不回滚（回滚＝同一局面可无限重抽同一条）。
        let mut q = Progress::new(Faction::Ember, Difficulty::Normal);
        daily_begin(&mut q, seed);
        for t in [TURN_LIMIT - 1, 0] {
            assert!(!daily_settle(&mut q, t), "survived={t} 不该被读成通关");
            assert_eq!(q.daily_attempts, 0, "失败不扣次数");
            assert_eq!(q.daily_draw_state, seed, "失败也不回滚抽取 state");
        }
    }

    /// 【短兵相接】【粘稠暗影】各带一句「若关卡为 Boss 关卡 → 不触发」，后面还跟一句"重抽上限 3 次"。
    /// 那句在这个模式下**凑不出输入**：每日那一场恒第 1 关、恒无 Boss ⇒ 引擎只留应用点的闸，不写防御分支。
    /// 本条量的就是"凑不出输入"这件事本身（帧文档 §9：条款留在文档、不可达写成可断言的形）。
    #[test]
    fn daily_encounter_never_raises_a_boss() {
        for level in [1, 12, 13, 60, 999] {
            let (foe, boss) = daily_encounter(level);
            assert!(boss.is_none(), "第{level}关的输入也一样：每日这条路压根没有 Boss");
            assert_eq!(foe, enemy_faction_for(1), "遭遇只认第 1 关——传进来的关号不许把它领到别章的阵营轮上去");
        }
    }

    /// 规则号来自存档（人手可改、旧档缺键取默认 0），越界号不得 panic。
    /// 池顺序也是被钉的：`种子 % 池大小` 这张表一旦换序，历史上每一天抽到的规则都会换一条。
    #[test]
    fn the_rule_pool_order_is_the_truth_and_an_out_of_range_number_reads_as_no_rule() {
        assert_eq!(
            DailyRule::POOL.iter().map(|r| r.name()).collect::<Vec<_>>(),
            vec!["业火狂潮", "短兵相接", "献祭余响", "粘稠暗影"],
            "池顺序＝修正2.MD 3.3 那条规则池的书写顺序，换序等于改历"
        );
        assert_eq!(DailyRule::from_index(0), Some(DailyRule::FlameTide), "0 是池内第一条，不是「没规则」");
        assert_eq!(DailyRule::from_index(3), Some(DailyRule::ViscidShadow), "末位取得到");
        for i in 4..9 {
            assert_eq!(DailyRule::from_index(i), None, "{i} 越界该报 None 而不是 panic");
        }
        assert_eq!(DailyRule::default(), DailyRule::None, "抽取端那句 `unwrap_or_default()` 落在这里");
        assert_eq!(daily_rule_name(9), "（今日无规则）", "报幕口径：坏号照读，读成「今天没有规则」");
    }

    /// 通关只翻标记，绝不写"第61关"：`is_mainline_end` 是 `>=`，越界 level 会让下一次续档赢一关就再报一次通关。
    #[test]
    fn mark_completed_flips_the_flag_without_faking_a_level_beyond_sixty() {
        let path = snap_path("completed");
        let mut save = slot(path.clone(), SaveUse::Progress);
        snapshot_entry(&mut save, 60, Faction::Ember, Difficulty::Normal, &[], 4);
        let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save, daily_rule: DailyRule::None };
        mark_completed(&mut st);
        let got = Progress::load_from(&path).expect("通关标记写入后仍须可载");
        assert!(got.completed, "通关历史要落盘：它是不依赖 level 推不倒的那一个字段");
        assert_eq!(got.level, 60, "档留在第60关入口");
        assert_eq!(got.carry_karma, 4, "翻标记不许顺手抹牌");
        let before = std::fs::read_to_string(&path).unwrap();
        mark_completed(&mut st);
        assert_eq!(before, std::fs::read_to_string(&path).unwrap(), "已标过就不再重写盘");
        std::fs::remove_file(&path).ok();
    }

    /// 坏档闸的**两条真实入口**（读不懂、跨阵营）：断言的是出口本身。
    /// 上一批这两条零覆盖——`open` 里直接 `process::exit(2)`，测试进程会被一起带走，于是所有断言
    /// 只能打在 `from_kv` 上，"拒载之后到底走不走对局"这一步是空的。
    #[test]
    fn try_open_refuses_corrupt_and_cross_faction_saves() {
        let bad = snap_path("gate-bad");
        std::fs::write(&bad, "schema=1\nlevel=十七\n").unwrap();
        let e = SaveSlot::try_open(bad.clone(), Faction::Ember, Difficulty::Normal, true, SaveUse::Progress)
            .err()
            .expect("坏档不许被当成空档继续");
        assert!(e.contains("progress recover"), "拒绝里要带下一步处置：{e}");
        assert_eq!(std::fs::read(&bad).unwrap(), *"schema=1\nlevel=十七\n".as_bytes(), "拒载不许顺手修档");

        let frost = snap_path("gate-frost");
        let mut p = Progress::new(Faction::Frost, Difficulty::Normal);
        p.level = 7;
        p.inherit.push(CardInst::new(4, faction_cards(Faction::Frost)[2]));
        p.save_to(&frost).unwrap();
        let e = SaveSlot::try_open(frost.clone(), Faction::Ember, Difficulty::Normal, true, SaveUse::Progress)
            .err()
            .expect("烬火卡表认不出霜阵营的卡名，换阵营续档必须拒");
        assert!(e.contains("阵营不符"), "要指明是不符合阵营而不是泛泛报错：{e}");
        // 第1关空堆是豁免（新开一档换个阵营不算冲突），上面两条都得是"档上真有东西"才响。
        SaveSlot::try_open(frost.clone(), Faction::Ember, Difficulty::Normal, false, SaveUse::Progress)
            .expect("不续档时换阵营只是重开，警告归警告，不该 exit");
        std::fs::remove_file(&bad).ok();
        std::fs::remove_file(&frost).ok();
    }

    /// 存档侧**故意不校验**继承堆 id 唯一性——不是因为没想过，而是因为引擎自己就写得出发重号的档。
    /// 机理：`Battle::new` 每关从 `id = 1` 重新发号（`battle.rs:185`），而"继承堆不足3张 ⇒ 补齐"
    /// 那一支正好用这个计数器（`battle.rs:214-218`）⇒ 带着上关的 1、2 号牌进第2关，补进来的那张必然也叫 1。
    /// 所以"载入即拒重号"的结局是拒掉真档，比它要防的问题更糟。重号的真实代价在战斗内
    /// （`battle.rs:1363` 同列自排除按 id 比 ⇒ 我方一张牌会让同号敌牌免伤），那是发号方案的问题，
    /// 归 `pool/cross-side-id-collision.md`，不在这里治。
    #[test]
    fn engine_reissues_id_one_so_load_must_not_require_unique_ids() {
        let inherit = vec![
            CardInst::new(1, faction_cards(Faction::Ember)[1]),
            CardInst::new(2, faction_cards(Faction::Ember)[2]),
        ];
        let b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, inherit, 2);
        let all = b.hand.iter().chain(b.draw_pile.iter()).collect::<Vec<_>>();
        let ones = all.iter().filter(|c| c.id == 1).count();
        assert_eq!(ones, 2, "一张是带上关的 1 号牌，一张是补齐时重新发出的 1 号 ⇒ 重号由引擎自己产出");
        assert_eq!(all.iter().filter(|c| !c.is_starter()).count(), 3, "2 张继承 + 补齐 1 张（开端另算）");
    }

    /// 存不进去 ⇒ 摘槽并明说，**本局照打**。让写盘失败终止一局已经打完的游戏是反向的取舍。
    #[test]
    fn write_failure_detaches_the_slot_and_keeps_playing() {
        // 用一个"同名普通文件"堵死父目录：create_dir_all 必然失败，且不需要权限技巧。
        let blocker = snap_path("blocked");
        std::fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("nested").join("progress.kv");
        let mut save = slot(path.clone(), SaveUse::Progress);
        snapshot_entry(&mut save, 2, Faction::Ember, Difficulty::Normal, &[], 0);
        assert!(save.is_none(), "写失败后不再重复尝试");
        assert!(!path.exists());
        // 摘槽之后就是纯内存模式：再快照既不 panic 也不落盘。
        snapshot_entry(&mut save, 3, Faction::Ember, Difficulty::Normal, &[], 0);
        std::fs::remove_file(&blocker).ok();
    }

    /// 章末（第12关）要照常推进，第60关必须不写：`is_mainline_end` 是 `>=`，
    /// 写下"第61关入口"就是伪造进度——下一次续档会赢一关就再报一次通关。
    #[test]
    fn next_entry_advances_chapter_ends_and_stops_at_sixty() {
        for (level, want) in [(11u32, Some(12)), (12, Some(13)), (59, Some(60)), (60, None), (61, None)] {
            let path = snap_path(&format!("next-{level}"));
            let mut save = slot(path.clone(), SaveUse::Progress);
            persist_next_entry(&mut save, level, Faction::Ember, Difficulty::Normal, &[], 0);
            match want {
                Some(next) => assert_eq!(
                    Progress::load_from(&path).expect("非终关应落盘").level,
                    next,
                    "第{level}关赢完应记成第{next}关入口"
                ),
                None => assert!(!path.exists(), "第{level}关赢完不该建档（越界等级＝伪造进度）"),
            }
            std::fs::remove_file(&path).ok();
        }
    }

    /// L3-d 的分派核心：`board_str`/`tail_str` 两档各取**正确上游**，且 Text 档必须与改前逐字节一致
    /// （byte-baseline 就靠这条：`--tui` 不在基线场景里，Text 分支一旦漂一个字节整批 SAME 就破）。
    /// 断言用**全等**而非 contains——把"取 render / 取 tui::draw"钉成定义级，上游改了也不会误绿。
    #[test]
    fn frame_switch_selects_the_render_upstream_and_text_stays_byte_identical() {
        let b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        // Text 档＝改前的 `"\n" + render`，Tui 档＝清屏转义 + `tui::draw`。
        assert_eq!(board_str(Frame::Text, &b), format!("\n{}", crate::render::render(&b)));
        assert_eq!(board_str(Frame::Tui, &b), format!("\x1b[2J\x1b[H{}", crate::tui::draw(&b)));
        // 两档取的是**两个不同上游**：Tui 带清屏转义、Text 不带；内容源也不同（否则两档相等＝没分派）。
        assert!(board_str(Frame::Tui, &b).starts_with("\x1b[2J"), "Tui 帧要以清屏转义开头");
        assert!(!board_str(Frame::Text, &b).contains('\x1b'), "Text 帧不该含终端转义");
        assert_ne!(board_str(Frame::Text, &b), board_str(Frame::Tui, &b), "两档必须真的走两条上游");
        // 日志尾同理分派：Text 走 render::log_tail，Tui 走 view().log_tail，各自与改前同一口径。
        assert_eq!(tail_str(Frame::Text, &b), crate::render::log_tail(&b, 12));
        assert_eq!(tail_str(Frame::Tui, &b), b.view().log_tail(12));
    }
}
