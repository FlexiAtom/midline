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
/// 诚实缺口：每日挑战规则第 2 步还要「固定卡组+特殊规则」，卡组名单文档**一字未给**；4 条特殊规则已在
/// 帧 D3／D3·补 接进引擎、对局内生效（生效面见 `DailyRule`）⇒ 现在的 `daily` 是"按日场次＋按日额度＋按日规则序列的单场生存战"，
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
    /// 每日：只由调用方写**每日那五个键＋代币余额**（日戳／额度／抽取序列三键 ＋ `tokens`），关卡与继承堆一个字节都不动——
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

/// 存活一回合给多少代币（修正2.MD 3.6「基础奖励：存活回合数 × 3」）。
const TOKENS_PER_SURVIVED_TURN: u32 = 3;
/// 通关那场的额外奖励（修正2.MD 3.6「通关奖励：额外 +15 代币」）。
/// 上限因此是 `30×3+15=105`：回合上限是 `TURN_LIMIT`，乘出来就到不了更高的数。
const TOKENS_CLEAR_BONUS: u32 = 15;

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
/// **重置只动"今天"的那五个键**：代币余额不在里面——它是历史攒下的钱，跟着日界清零不是"每日重置"，是丢档。
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

/// 一场该发多少代币（修正2.MD 3.6 的三条数在这一处收成一个式子）。
/// 基础＝**存活回合数 × 3**；通关再 +15；失败只有基础那条（3.6 第 3 条「失败奖励：存活回合数 × 3」）。
/// 所以它**不读胜负**：3.6 第 4 条把话说死了——「结算标准：唯一标准＝存活回合数」。
/// 胜/败/平三种出口、蜡烛余量、击杀数，在这里都不该有价钱：给了就是第二套真值，且文档一字未给。
/// `survived` 是 `i64`（从战斗里数出来的），负值只可能来自手改或未来的调试路径 ⇒ 先夹到 0，
/// 免得一笔负奖励把攒下的余额掏空（那是"读得懂但会算出负钱"的那类静默）。
fn daily_tokens(survived: i64, cleared: bool) -> u32 {
    let turns = u32::try_from(survived.max(0)).unwrap_or(u32::MAX);
    turns.saturating_mul(TOKENS_PER_SURVIVED_TURN).saturating_add(if cleared { TOKENS_CLEAR_BONUS } else { 0 })
}

/// 一场打完的记账（纯函数）。通关判据＝**活满回合上限**（2026-10-05 裁「一次挑战＝单场 30 回合生存战」）：
/// 通关扣一次额度，中途败不扣（修正2.MD 3.5「失败不扣挑战次数」）。抽取序列 state 不在这里回滚——
/// 回滚就等于允许同一局面无限重抽同一条规则（2026-10-05 裁 Q-新3 的耦合判据）。
/// 代币也在这一处入账，而且**挂在 `cleared` 之外**：3.6 第 3 条明写败局照发 `存活×3`，
/// 把它写成"通关才发"就是少给一半钱，而那条分支看起来还挺像"奖励＝奖励给赢的人"。
/// 返回 `(是否通关, 本次入账)`——入账报的是**这里实际记下的那个数**，调用方不许另算一遍。
fn daily_settle(p: &mut Progress, survived: i64) -> (bool, u32) {
    let cleared = survived >= TURN_LIMIT;
    if cleared {
        // saturating：额度是 u32，手改成 42 亿的档不该让程序在这行 panic（那是"读得懂的档"，不是坏档）。
        p.daily_attempts = p.daily_attempts.saturating_add(1);
    }
    let earned = daily_tokens(survived, cleared);
    p.tokens = p.tokens.saturating_add(earned);
    (cleared, earned)
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
/// 诚实缺口（只剩一条）：① 固定卡组仍未落——修正2.MD 3.2 那条"占位算法"要的是**10 张卡的名字**，
/// 名单待人给（挂在每日挑战规则第 2 步那条债上）。② 曾挂在这里的【献祭余响】"抽中它这一场不改对局"已于帧 D3·补
/// 按 `daily-echo-karma-ruling` 裁定一落地：场上献祭时该牌的亡语当场再结算一次（手牌献祭拿不到"哪一列"⇒ 不适用，
/// 那条语义未定登记在 `model.rs` 每日挑战那条债的去处里）。
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
    let (cleared, earned) = daily_finish(&mut st, survived);
    let balance = st.save.as_ref().map_or(0, |s| s.progress.tokens);
    let verdict = if cleared {
        format!("通关（活满 {TURN_LIMIT} 回合）⇒ 本次额度已用掉")
    } else {
        format!("未活满 {TURN_LIMIT} 回合 ⇒ 不扣额度，今天还能再来")
    };
    // 奖励行跟着存活回合数走，因为那就是 3.6 的唯一结算标准；败局也有 `+3×N`，别把它读成"没通关就一分钱没有"。
    println!("本场存活 {survived} 回合：{verdict} · +{earned} 代币（余额 {balance}）· seed={seed} 可复现整场。");
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
/// 返回 `(是否通关, 本场入账代币)`。入账那个数是**内存槽**的增量：写失败时 `persist` 自己会打一行 ✖ 并说明
/// "本次运行不再落盘，内存进度继续"——代币与额度、抽取序列走的是同一条口径，不单独给钱再加一道"报喜先验盘"。
fn daily_finish(st: &mut RunState, survived: i64) -> (bool, u32) {
    let Some(slot) = st.save.as_mut() else { return (false, 0) };
    let (cleared, earned) = daily_settle(&mut slot.progress, survived);
    persist(slot, if cleared { "每日额度" } else { "每日抽取序列" });
    (cleared, earned)
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

    /// 没有存档槽 ⇒ **0**，不是"免票"：爬梯（`ladder`）从不落盘，它也就没有可扣的钱，
    /// 读成一个哨兵值（`u32::MAX` 之类）等于在该模式里悄悄把价目取消。
    fn tokens(&self) -> u32 {
        self.save.as_ref().map_or(0, |s| s.progress.tokens)
    }

    fn set_tokens(&mut self, tokens: u32) {
        // 只改内存槽、**不在这里写盘**：写盘的时机仍归「关卡入口快照」那一条路（裁定27），
        // 于是升级和它的价钱要么一起进档、要么一起弃局作废。
        if let Some(slot) = self.save.as_mut() {
            slot.progress.tokens = tokens;
        }
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

/// `auto` 冒烟的默认起点种子：从 1000 起、每轮 +1。**只在调用方没给 `--seed` 时用它**，
/// 为的是保住既有冒烟读数（`auto 3` 与 HEAD 逐字节相同是这一帧的反证项之一）。
const AUTO_SEED_BASE: u64 = 1000;

/// 自动对局冒烟：玩家侧也走贪心，验证规则闭环不 panic、能分胜负。
/// `diff` 是敌方 AI 档位；专家档额外让托管侧（有继承堆的一侧）在结算阶段由 AI 决策融合/升级。
/// `seed`/`faction` 是**调用方给的**（`--seed`/`--faction`），没给就用冒烟夹具自己的默认：
/// 种子从 1000 起、每轮 +1，阵营烬火。那两个默认**不属于** `play` 那一族的"不给＝种子7·烬火"——
/// 从前这里根本不看调用方给的值，于是 `auto --seed 7` 的头报印着 `seed=1000`：一行自称"可复现"的输出，
/// 复现的却是别人那一局。每轮 `base + i` 与 `auto --boss` 的 `base + round` 是同一条规则。
pub fn auto_battles(seed: Option<u64>, faction: Option<Faction>, n: u32, diff: Difficulty) {
    let base = seed.unwrap_or(AUTO_SEED_BASE);
    let mine = faction.unwrap_or(Faction::Ember);
    for i in 0..n {
        let seed = base.wrapping_add(i as u64);
        let mut inherit: Vec<CardInst> = Vec::new();
        let mut level = 1u32;
        let mut last = Outcome::Draw;
        // 冒烟夹具的钱包：`--auto` 没有存档槽（它不落盘），而升级现在是**要花钱的**，所以这里显式给它
        // 一笔只存在于本次调用的预算——数＝单场最多 5 次通关 × 每次的价。它是夹具，不是发放面的产出：
        // 真游戏里那笔钱只能由 `daily_settle` 挣来，这条路上一个代币都不会落进任何档面。
        let mut purse = crate::progress::UPGRADE_COST * 5;
        for _ in 0..5 {
            // 与 one_level 同一套敌方构造（`new_mainline`＝构造即章强化）：本循环当前最多到第5关
            // （第1章，强化量 0），加上来是为了将来把轮数拉长读章节曲线时，托管冒烟与主线玩的不是两种游戏。
            let mut b =
                Battle::new_mainline(seed, mine, enemy_faction_for(level), diff, std::mem::take(&mut inherit), level);
            auto_play(&mut b);
            last = b.over.unwrap_or(Outcome::Draw);
            match last {
                Outcome::PlayerWin => {
                    for l in crate::progress::collect_survivors(&mut b, &mut inherit) {
                        println!("{l}");
                    }
                    // 融合/升级：验证跨关持有与 meta 决策路径
                    let mut k = b.p_karma.max(0);
                    apply_meta_plan(&mut inherit, &mut k, &mut purse, diff, 1);
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
/// `tokens` 是这一局用来付升级费的钱包——托管**代扣**，与玩家走的是 `upgrade_card` 里同一道价闸；
/// 冒烟没有存档槽，那一笔预算由调用点显式给（夹具的钱，不是发放面的产出）。
fn apply_meta_plan(inherit: &mut Vec<CardInst>, karma: &mut i32, tokens: &mut u32, diff: Difficulty, upgrades_left: u8) {
    if diff != Difficulty::Expert {
        if inherit.len() >= 2 {
            let _ = crate::progress::fuse_cards(inherit, 0, 1, karma);
        }
        return;
    }
    // plan 的下标基于同一快照；先执行不改长度的升级，再执行会摘牌的融合
    let plan = crate::ai::plan_meta(inherit, *karma, upgrades_left);
    let mut run = |m: &crate::ai::MetaMove| match m {
        crate::ai::MetaMove::Upgrade { idx, kind } => match crate::progress::upgrade_card(inherit, *idx, kind, tokens) {
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

/// 帧记录器一次最多走几关。它 bounding 的是**时长**而不是胜负：托管在第1章普通关能连赢，
/// 不设顶就会一路打到败局才停，而每一关都是几十帧。5 ＝ `auto_battles` 那个冒烟环同一档（它 `for _ in 0..5`），
/// 两处用同一个数是为了让"托管能走多远"在两条路上是同一个答案，不是一处 5 一处 3。
const FRAMES_LEVEL_CAP: u32 = 5;

/// 2D 离线帧记录器（第十六步·其九，走「零依赖离线帧」那一支）：托管打一关，**每次写入一张 SVG**。
///
/// 文件名序号＝**写入次序**而不是回合数：战斗是在某一回合的结算里结束的，那一回合连着出两帧（战前、战后），
/// 拿回合数命名就让终局帧**静默盖掉**战前帧——帧少一张，而报数按写入次数算，于是"10 张"只有 9 个文件。
///
/// 它落在 `meta` 而不是新开一个壳，是为了复用同一套托管驱动（`auto_turn`）——换的只是"这一帧变成什么"：
/// Text/Tui 变成 stdout 上的一段字串，本条变成**落盘的字节**。这两种 IO 语义不该塞进同一个 `Frame` 枚举，
/// 所以这里既不加档也不复制 host：`Frame` 那条 match 一字不动，文本路逐字节如旧。
/// 唯一的写是 `out` 目录里的 `frame-NNN.svg` 与 `index.html`——不落存档、不碰 `--save` 那条面、不删任何既有文件。
///
/// **打赢一关就往下走一格**（其七 D4）：本关胜 ⇒ 收尸进继承堆 ⇒ 走一次**真结算命令通路**（`command::execute_settle`，
/// 与玩家敲 `fuse`／`up` 是同一套词表与同一道闸）⇒ 用改过的继承堆开下一关。这一格之前没有：记录器永远
/// 用 `Vec::new()` 开局、只打第1关，于是融合产物与升级态在**任何**交付帧里都不存在——不是没画，是走不到。
/// Boss 那一支不进环：`boss_run` 本身是单关跳打（`has_next=false`，§十二:499 那格的败/平/弃都不动盘同理），
/// 拿它连关会凭空多出"打过 Boss 还接着爬"的第二套关卡语义。
pub fn frames_dump(seed: u64, faction: Faction, diff: Difficulty, boss: Option<crate::boss::BossId>, out: PathBuf) {
    use crate::command::{SettleStep, execute_settle, parse_settle};
    if let Err(e) = std::fs::create_dir_all(&out) {
        println!("✖ 帧目录建不出来：{e}");
        std::process::exit(2);
    }

    let mut names: Vec<String> = Vec::new();
    let mut guard = 0;
    let mut inherit: Vec<CardInst> = Vec::new();
    // 夹具钱包，口径与 `auto_battles` 里那一笔相同：记录器没有存档槽（它不落盘），而升级现在是**要花钱的**。
    // 读成一个哨兵值就是在这条路上把价目取消，所以给一笔显式的、只活在本次调用的预算，并让 `up` 照付。
    let mut purse = crate::progress::UPGRADE_COST * 5;
    let mut level = 1u32;
    let mut scene;
    let mut outcome;

    loop {
        scene = match boss {
            Some(id) => format!("Boss {} 第{}关", id.name(), id.chapter() * 12),
            None => format!("普通爬关 第{level}关"),
        };
        let mut b = match boss {
            Some(id) => Battle::new_boss(seed, faction, id, std::mem::take(&mut inherit), id.chapter() * 12),
            None => Battle::new_mainline(seed, faction, enemy_faction_for(level), diff, std::mem::take(&mut inherit), level),
        };
        loop {
            guard += 1;
            let v = b.view();
            let frame = crate::canvas::svg_frame(&v, seed, &scene);
            // 文件名用**写入序号**，不用回合数：战斗是在某一回合的结算里结束的，那一回合会连着出两帧
            // （战前、战后），拿回合数命名就让第二帧盖掉第一帧——帧少一张，而报数与 index 都按写入次数算，
            // 于是"10 张"只有 9 个文件。回合数仍在帧内标题里，序号只保证一帧一格。
            let name = format!("frame-{:03}.svg", guard);
            if let Err(e) = std::fs::write(out.join(&name), frame) {
                println!("✖ 帧文件写不下去：{e}");
                std::process::exit(2);
            }
            names.push(name);
            if b.over.is_some() || guard >= 300 {
                break;
            }
            auto_turn(&mut b);
        }
        outcome = b.over.map_or("未分胜负（到达回合上限）".to_string(), |o| format!("{o:?}"));

        // 三条不进环的出口：败/平（收尸只在胜局发生，§十二:503）、Boss 单关、帧数顶到 300。
        // 关卡顶那条排在收尸**之前**判，免得"第6关的堆已经收好了却没开局"这种半截状态留在报告里。
        if b.over != Some(Outcome::PlayerWin) || boss.is_some() || guard >= 300 || level >= FRAMES_LEVEL_CAP {
            break;
        }
        for l in crate::progress::collect_survivors(&mut b, &mut inherit) {
            println!("帧·第{level}关 {l}");
        }
        let mut karma = b.p_karma.max(0);
        let mut up_used = false;
        // 下单的是**引擎自己在专家档用的那个战斗外规划器**（`ai::plan_meta`）：它拿当前业力筛掉付不起的融合、
        // 拿 3 次上限筛掉不能再升的牌，只交出 (main, sub)／idx 这几个数。这里把它们拼成玩家会敲的那几行字，
        // 再交给 `execute_settle` 走同一套词表、同一道闸——证人由此拿到的自造牌是**对局可达**的产物，
        // 不是夹具直接 `c.crafted = true` 那种手改字段（那样帧上那块金边就没有"打得出来"的含义）。
        // 顺序照 `apply_meta_plan`：先排不摘牌的升级，再排会摘牌的融合，下标才对得上同一份快照。
        use crate::ai::MetaMove;
        let word = |m: &MetaMove| match m {
            MetaMove::Upgrade { idx, kind } => format!("up {idx} {kind}"),
            MetaMove::Fuse { main, sub } => format!("fuse {main} {sub}"),
        };
        let plan = crate::ai::plan_meta(&inherit, karma, 1);
        let mut lines: Vec<String> = plan.iter().filter(|m| matches!(m, MetaMove::Upgrade { .. })).map(&word).collect();
        lines.extend(plan.iter().filter(|m| matches!(m, MetaMove::Fuse { .. })).map(&word));
        lines.push("go".to_string());
        // 拒了也照打输出——"这一环今天走不走得通"是要人眼看的读数，不是可以静默吞掉的分支。
        println!("帧·第{level}关 结算开环：业力{karma} 代币{purse} 继承堆{}张", inherit.len());
        for (i, c) in inherit.iter().enumerate() {
            println!("帧·第{level}关 堆[{i}] {}", crate::model::short_card(c));
        }
        for line in &lines {
            match execute_settle(
                &mut inherit,
                &mut karma,
                &mut up_used,
                &mut purse,
                true,
                parse_settle(line),
            ) {
                SettleStep::Stay(replies) => {
                    for l in replies {
                        println!("帧·第{level}关 结算 {line} → {l}");
                    }
                }
                SettleStep::Go => break,
                SettleStep::Quit => return,
            }
        }
        level += 1;
    }

    let mut index = format!("<!DOCTYPE html>\n<html lang=\"zh\"><head><meta charset=\"utf-8\"><title>中线 2D 帧 · seed {seed}</title></head>\n<body style=\"background:#10131a;color:#e6e9f2;font-family:sans-serif\">\n<h2>《中线》2D 帧 · {scene} · seed {seed} · 共 {} 张（打到第{level}关）</h2>\n", names.len());
    for n in &names {
        index.push_str(&format!("<p>{n}</p><img src=\"{n}\" width=\"1240\" alt=\"{n}\">\n"));
    }
    index.push_str("</body></html>\n");
    if let Err(e) = std::fs::write(out.join("index.html"), index) {
        println!("✖ index.html 写不下去：{e}");
        std::process::exit(2);
    }

    println!(
        "✓ 帧目录 {}：{} 张 SVG ＋ index.html（末景={scene} seed={seed} 结果={outcome}）",
        out.display(),
        names.len()
    );
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
                if !b.p_front[col].as_ref().unwrap().aged_one_turn(b.turn) {
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

    /// 壳这一侧的钱包接缝：交回只动内存槽、**不自己写盘**——落盘时机仍归「关卡入口快照」独家决定。
    /// 若有人在这条路上顺手补一次 `persist`，弃局（`q`）就会留下一份"钱已扣、牌没升"的档面，
    /// 而会话层那两条测（`session.rs`）看不见磁盘，拦不住它。
    #[test]
    fn handing_the_wallet_back_moves_the_slot_but_not_the_disk() {
        use crate::session::Host;
        let path = snap_path("purse");
        let mut save = slot(path.clone(), SaveUse::Progress);
        let mut host = PlayHost {
            seed: 7, faction: Faction::Ember, diff: Difficulty::Normal,
            foe: Faction::Frost, boss: None, save: &mut save, frame: Frame::Text, daily_rule: DailyRule::None,
        };
        host.set_tokens(15);
        assert_eq!(host.tokens(), 15, "阶段出口交回的余额进内存槽");
        assert!(!path.exists(), "交回钱包这一步不许写盘");
        host.snapshot_next(1, &[], 0);
        assert_eq!(Progress::load_from(&path).unwrap().tokens, 15, "下一次入口快照把新余额一起带进档面");
        std::fs::remove_file(&path).ok();
    }

    /// 无存档槽 ⇒ 余额读作 **0**，不是免票。爬梯（`ladder`）与冒烟都不落盘，所以它们在规则上就是
    /// "没攒过钱"：把 `map_or(0, ..)` 换成哨兵值（`u32::MAX`）或干脆跳过检查，这条当场红。
    #[test]
    fn a_battle_without_a_save_slot_has_nothing_to_pay_with() {
        use crate::session::Host;
        let mut no_slot: Option<SaveSlot> = None;
        let mut host = PlayHost {
            seed: 7, faction: Faction::Ember, diff: Difficulty::Normal,
            foe: Faction::Frost, boss: None, save: &mut no_slot, frame: Frame::Text, daily_rule: DailyRule::None,
        };
        assert_eq!(host.tokens(), 0, "没有档面就没有余额");
        host.set_tokens(15);
        assert_eq!(host.tokens(), 0, "无槽时交回来的钱不落任何地方，也不凭空多出一槽");
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
        assert!(daily_finish(&mut st, TURN_LIMIT).0, "活满回合上限就是通关，这条出口不该是假阴性");

        let got = Progress::load_from(&path).expect("写入后档仍应能被自己的校验器读回");
        assert_eq!(got.level, 5, "打一次每日不该把主线进度抹回第1关");
        assert_eq!(got.inherit.len(), 2, "继承堆不该被抹平");
        assert_eq!(got.inherit[0].id, 3, "id 是战斗输入，重写不得重编号");
        assert_eq!(got.carry_karma, 3, "结转业力不该被抹平");
        assert_eq!(got.daily_done, daily_seed(), "每日记录本身要写上");
        assert_eq!(got.daily_attempts, 1, "通关扣一次额度，且只扣一次");
        assert_eq!(got.tokens, 30 * 3 + 15, "通关那场走**整文件重写**，代币必须跟额度一起落到盘上");
        // 中途败那一次：额度照旧不动，但**抽取序列必须落盘**。失败不回滚 state 是 Q-新3 定的耦合判据；
        // `save_to` 那道"字节相同就不重写"的闸在这里挡不住——序列确实前进了一步，落盘是真改动。
        let mut opened2 = SaveSlot::try_open(path.clone(), Faction::Ember, Difficulty::Normal, false, SaveUse::Daily)
            .expect("合法档应能开出每日槽");
        assert!(daily_begin(&mut opened2.progress, daily_seed()), "额度还剩就该继续放行");
        let drawn = daily_draw(&mut opened2.progress);
        let advanced_state = opened2.progress.daily_draw_state;
        let mut run2 = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save: Some(opened2), daily_rule: DailyRule::None };
        assert!(!daily_finish(&mut run2, TURN_LIMIT - 1).0, "差一个回合就是没活满");
        let after = Progress::load_from(&path).expect("失败那次落盘的档仍应能读回");
        assert_eq!(after.daily_attempts, 1, "失败不扣次数：额度还停在通关那一次");
        assert_eq!(after.tokens, (30 * 3 + 15) + (TURN_LIMIT as u32 - 1) * 3, "败局照发 存活×3，且是**接着上一场的余额攒**，不是覆盖");
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
            assert!(daily_settle(&mut p, TURN_LIMIT).0);
        }
        assert_eq!(p.daily_attempts, DAILY_ATTEMPTS);
        assert!(!daily_begin(&mut p, seed), "额度用完当天不许再来");
        assert!(daily_begin(&mut p, seed + 1), "但到明天必须又是满格");
        assert_eq!(p.tokens, DAILY_ATTEMPTS * (30 * 3 + 15), "跨日：额度重满，代币**照旧攒着**——钱包不是日历");
        // 失败不扣：29 回合与 0 回合都是"没活满"，且抽取 state 不回滚（回滚＝同一局面可无限重抽同一条）。
        let mut q = Progress::new(Faction::Ember, Difficulty::Normal);
        daily_begin(&mut q, seed);
        for t in [TURN_LIMIT - 1, 0] {
            assert!(!daily_settle(&mut q, t).0, "survived={t} 不该被读成通关");
            assert_eq!(q.daily_attempts, 0, "失败不扣次数");
            assert_eq!(q.daily_draw_state, seed, "失败也不回滚抽取 state");
        }
    }

    /// 3.6 那三条数的**表**：回合数是唯一乘数，+15 只跟着"活满"那一支。
    /// 胜/败/平在这里没有区别——文档第 4 条把唯一结算标准写死成存活回合数，给胜负定价就是第二套真值。
    #[test]
    fn the_award_is_three_per_turn_plus_fifteen_only_on_a_clear() {
        for (survived, cleared, want) in [
            (0, false, 0),
            (1, false, 3),
            (12, false, 36),
            (TURN_LIMIT - 1, false, 87),
            (TURN_LIMIT, true, 105),
        ] {
            assert_eq!(daily_tokens(survived, cleared), want, "survived={survived} cleared={cleared}");
        }
        assert_eq!(daily_tokens(TURN_LIMIT, true), 3 * 30 + 15, "上限 105：回合上限 30 就是这场奖励的天花板");
        assert_eq!(daily_tokens(-1, false), 0, "负回合数不许把余额掏空（夹到 0 是发放侧的闸，不是解析侧的）");
    }

    /// 每日挑战 D1 留下那条"仍待测"的端到端：**第 N 回合中途败 ⇒ 存活 N ⇒ 发 3N ⇒ 落到磁盘上**。
    /// 三段都不手工设数：回合数由会话层从 `b.turn` 取（那条接缝另有
    /// `session::the_terminus_hands_back_the_turn_the_battle_stopped_on` 钉住），乘与加在 `daily_settle`，
    /// 写盘走 `daily_finish` 的真实整文件重写 ⇒ 断言读的是**磁盘上的数**，不是内存里的。
    #[test]
    fn a_mid_battle_loss_pays_three_times_the_turn_it_stopped_on() {
        let path = snap_path("tokens-12-turn-loss");
        let mut opened = SaveSlot::try_open(path.clone(), Faction::Ember, Difficulty::Normal, false, SaveUse::Daily)
            .expect("没有档也该开得出新每日槽");
        assert!(daily_begin(&mut opened.progress, daily_seed()), "第一次进来必须放行");
        let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save: Some(opened), daily_rule: DailyRule::None };
        let (cleared, earned) = daily_finish(&mut st, 12);
        assert!(!cleared, "12 回合不是通关");
        assert_eq!(earned, 36, "存活 12 回合 ⇒ 3×12，不多不少");
        let got = Progress::load_from(&path).expect("发过钱的那次必须落盘");
        assert_eq!(got.tokens, 36, "报出来的数要活着到磁盘上，否则结语就是替内存吹牛");
        assert_eq!(got.daily_attempts, 0, "发钱不扣额度：这两件事在 3.5/3.6 里本来就是两条独立的判据");
        std::fs::remove_file(&path).ok();
    }

    /// 主线一关都不发钱：3.6 整节挂在**每日挑战**章下，接到主线就是给"通关一次"凭空造出第二份收益。
    /// 码面上代币的唯一写点是 `daily_settle`，而主线走的是**逐字段**的入口快照——所以这条量的不是"有没有写"，
    /// 是"按关写入口既不顺手发钱、也不把已有的余额抹回 0"（后者是真会发生的形态：`snapshot_entry` 若整份换成
    /// 新建的 `Progress`，钱就跟着 hp/瞬态一起没了）。
    #[test]
    fn mainline_snapshots_neither_pay_nor_erase_the_balance() {
        let path = snap_path("tokens-mainline-idle");
        let mut save = slot(path.clone(), SaveUse::Progress);
        save.as_mut().unwrap().progress.tokens = 40;
        let pile = vec![CardInst::new(3, faction_cards(Faction::Ember)[1])];
        for level in 2..=4 {
            snapshot_entry(&mut save, level, Faction::Ember, Difficulty::Normal, &pile, 0);
        }
        let got = Progress::load_from(&path).expect("主线快照应落盘");
        assert_eq!(got.tokens, 40, "主线不发钱，也不许把攒下的钱抹平");
        assert_eq!(got.level, 4, "关卡照旧是按关写的（这条测不是靠'什么都不变'来判绿的）");
        std::fs::remove_file(&path).ok();
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

    /// 帧记录器**一帧一个文件**，且报出的张数等于目录里真有的文件数。
    /// 这条装的是终局那一格：战斗是在某一回合的结算里结束的，那一回合连着出两帧（战前、战后），
    /// 一旦拿回合数当文件名，战后那帧就静默盖掉战前那帧——盘上少一张，而计数照旧往上走。
    #[test]
    fn every_frame_lands_in_its_own_file_and_the_index_agrees_with_the_disk() {
        let n = SAVE_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("midline-frames-{}-{n}", std::process::id()));
        frames_dump(2026, Faction::Ember, Difficulty::Normal, Some(BossId::YanBing), dir.clone());

        let mut svgs: Vec<String> = std::fs::read_dir(&dir)
            .expect("帧目录应已建出")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".svg"))
            .collect();
        svgs.sort();
        let index = std::fs::read_to_string(dir.join("index.html")).expect("index.html 应与帧同批写出");
        let claimed: usize = index
            .split("共 ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .and_then(|num| num.parse().ok())
            .expect("index.html 里那句「共 N 张」要能读回来（读不回＝这条没在量任何东西）");

        assert!(svgs.len() >= 3, "至少要有几帧可数，否则这条在空目录上永真：实得 {}", svgs.len());
        assert_eq!(svgs.len(), claimed, "index 报的张数要等于盘上的 svg 数");
        let unique: std::collections::HashSet<&String> = svgs.iter().collect();
        assert_eq!(unique.len(), svgs.len(), "帧文件名不得重复（重复＝后一帧覆盖了前一帧）");
        // 序号必须连续从 001 起：跳号说明有帧没写出去，重复说明有帧被盖。
        for (i, name) in svgs.iter().enumerate() {
            assert_eq!(name, &format!("frame-{:03}.svg", i + 1), "帧序号要连续，第 {} 格实得 {name}", i + 1);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 其七 D4 的证人：交付帧里那块金边到底是**谁画上去的**。两局各写一遍盘，再从 svg 字节里读回两枚 token。
    /// ① 打赢那局：自造牌的金边（`lane::CRAFT_EDGE`）与升过级那一圈的加强线宽（`layout::STROKE_EMPHASIS`）都要读到，
    ///    而**第 1 帧两处皆干净**——融合与升级都发生在第 1 关打完的那次结算里，那之前没有任何一帧该带金。
    /// ② 打不赢那局（正常难度托管必败，本关现量）：整批帧里两处 0 命中。少了这一半，①就退化成"帧够多总会亮"，
    ///    那块金也就说不出"对局可达"四个字——这正是档 §2 那行拒绝 `c.crafted = true` 的判据。
    /// 它钉的是**落盘的交付物**而不是某个函数的返回值：呈现层与记录器之间哪天断了手（比如手牌行不再带边框通道），这条当场红。
    #[test]
    fn the_crafted_gold_in_the_delivered_frames_comes_from_the_settlement_ring() {
        use crate::md3::{lane, layout};
        let n = SAVE_SEQ.fetch_add(1, Ordering::Relaxed);
        let win = std::env::temp_dir().join(format!("midline-witness-win-{}-{n}", std::process::id()));
        let lose = std::env::temp_dir().join(format!("midline-witness-lose-{}-{n}", std::process::id()));
        frames_dump(2, Faction::Ember, Difficulty::Easy, None, win.clone());
        frames_dump(2, Faction::Ember, Difficulty::Normal, None, lose.clone());

        let gold = format!(r#"stroke="{}""#, lane::CRAFT_EDGE);
        let emph = format!(r#"stroke-width="{}""#, layout::STROKE_EMPHASIS);
        let scan = |dir: &std::path::Path| -> Vec<(String, usize, usize)> {
            let mut files: Vec<String> = std::fs::read_dir(dir)
                .expect("帧目录应已建出")
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".svg"))
                .collect();
            files.sort();
            files
                .into_iter()
                .map(|name| {
                    let text = std::fs::read_to_string(dir.join(&name)).expect("帧文件应读得回来");
                    (name, text.matches(&gold).count(), text.matches(&emph).count())
                })
                .collect()
        };
        let won = scan(&win);
        let lost = scan(&lose);
        std::fs::remove_dir_all(&win).ok();
        std::fs::remove_dir_all(&lose).ok();

        assert!(won.len() >= 10, "打赢那局至少要出十几帧，否则下面几句在稀疏样本上永真：实得 {}", won.len());
        assert_eq!(won[0].1, 0, "第 1 帧不许有金边：{} 里读到了", won[0].0);
        assert_eq!(won[0].2, 0, "第 1 帧不许有加强线宽：升级也在第一次结算里才发生");
        assert!(won.iter().any(|(_, g, _)| *g > 0), "打完几关的帧里读不到一块金边：证人通路没把自造牌送到画面上");
        assert!(won.iter().skip(1).any(|(_, _, e)| *e > 0), "读不到加强线宽：升级在帧上没留下非文字的读法");

        assert!(lost.len() >= 3, "打不赢那局也要有几帧可数，否则那句 0 命中是空目录给的：实得 {}", lost.len());
        assert!(lost.iter().all(|(_, g, e)| *g == 0 && *e == 0), "一局没打赢却帧上亮了金边或加强线宽：那块金就不是结算通路的产物");
    }

    /// 其十 的交付帧证人：**手牌行**右上角那串旗子，在落盘的帧文件里读不读得回来。
    ///
    /// 与上面那条金边证人同一口径、同一台戏（seed 2 两局）：单元测拿合成牌能把 `hand_row` 点亮，
    /// 但通道开了而牌不在那一格里，交付件就一个字都没变——本帧补的正是"手里那张牌说不清自己是什么"。
    ///
    /// 认法只按两个属性同时命中：`font-size=LABEL_MEDIUM` × `text-anchor=end`。在场格那串是
    /// `BODY_LARGE`(20)、`焰{}` 那串是 FLAME 色、两枚 `·` 是 22/start，都不会被误认成手牌旗子。
    ///
    /// 两臂的分工：① 打赢那局里 开端／造／升级+N 三族词都要读到，而**第 1 帧只有开端**（融合与升级
    /// 发生在第一次结算里）；② 打不赢那局里第 1 帧之外**一个旗子都没有**——少了这一半，①就退化成
    /// "帧够多总会亮"，那些词也就说不出"来自结算"四个字。
    #[test]
    fn the_hand_row_flags_reach_the_delivered_frames() {
        use crate::md3::{lane, typo};
        let n = SAVE_SEQ.fetch_add(1, Ordering::Relaxed);
        let win = std::env::temp_dir().join(format!("midline-handflags-win-{}-{n}", std::process::id()));
        let lose = std::env::temp_dir().join(format!("midline-handflags-lose-{}-{n}", std::process::id()));
        frames_dump(2, Faction::Ember, Difficulty::Easy, None, win.clone());
        frames_dump(2, Faction::Ember, Difficulty::Normal, None, lose.clone());

        let size = format!(r##"font-size="{}""##, typo::LABEL_MEDIUM);
        let mark = format!(r##"fill="{}""##, lane::MARK_TEXT);
        let scan = |dir: &std::path::Path| -> Vec<Vec<String>> {
            let mut files: Vec<String> = std::fs::read_dir(dir)
                .expect("帧目录应已建出")
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".svg"))
                .collect();
            files.sort();
            files
                .into_iter()
                .map(|name| {
                    let text = std::fs::read_to_string(dir.join(&name)).expect("帧文件应读得回来");
                    text.split("<text ")
                        .skip(1)
                        .filter_map(|chunk| {
                            let (head, rest) = chunk.split_once('>')?;
                            (head.contains(&size) && head.contains(r##"text-anchor="end""##)).then(|| {
                                assert!(head.contains(&mark), "手牌旗子换了墨色：{head}");
                                rest.split('<').next().unwrap_or("").to_string()
                            })
                        })
                        .collect()
                })
                .collect()
        };
        let won = scan(&win);
        let lost = scan(&lose);
        std::fs::remove_dir_all(&win).ok();
        std::fs::remove_dir_all(&lose).ok();

        assert!(won.len() >= 10, "打赢那局至少要出十几帧，否则下面几句在稀疏样本上永真：实得 {}", won.len());
        let all = |run: &Vec<Vec<String>>| -> Vec<String> { run.iter().flatten().cloned().collect() };
        assert_eq!(won[0], vec!["开端".to_string()], "第 1 帧手里只该有开端：造与升级都在第一次结算里才发生");
        let words = all(&won);
        assert!(words.iter().any(|w| w == "造"), "打赢那局的帧里读不到「造」：自造牌从没进过手牌行的旗子串");
        assert!(words.iter().any(|w| w.starts_with("升级+")), "打赢那局的帧里读不到「升级+N」：升过级的手牌在帧上仍只剩一圈线");
        assert_eq!(
            won[won.len() / 2..].iter().filter(|f| f.is_empty()).count(),
            0,
            "后半程有帧没带旗子：那张造过／升过的牌一旦进了手，其后每一帧都该在（放牌要占业力）"
        );

        assert!(lost.len() >= 3, "打不赢那局也要有几帧可数，否则那句 0 命中是空目录给的：实得 {}", lost.len());
        assert_eq!(lost[0], vec!["开端".to_string()], "打不赢那局的第 1 帧也该有开端（起手手里那张不是结算给的）");
        assert!(lost.iter().skip(1).all(|f| f.is_empty()), "一局没打赢却读到了旗子：那些词就不是结算通路的产物");
    }
}
