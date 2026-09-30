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

use crate::battle::{Battle, Difficulty, Outcome};
use crate::model::{CardInst, Faction};
use crate::save::{self, Progress};
use std::path::PathBuf;

/// §廿二:969 每日挑战＝本地种子，基于日期生成（`epoch秒/86400` ⇒ 日界是 UTC 零点，不是本地零点）。
/// 诚实缺口：每日挑战规则第 2 步还要「固定卡组+特殊规则」，文档**一字未定义**两者，
/// 所以当前 `daily` 只是"换日期种子的 play"，不是文档意义上的每日挑战（该缺口挂成显式债，见 model.rs 债表）。
pub fn daily_seed() -> u64 {  // §廿一:921 每日挑战规则第 1 步＝本地种子，基于日期生成
    let now = std::time::SystemTime::now()  // §廿一:925 改时间不可防：文档自己声明"不影响游戏平衡"⇒ 本实现不设闸，也不做服务器校时
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now / 86_400
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
}

/// `one_level` 唯一认识的存档形态：它不需要知道这是主线还是每日，只需要知道**要不要按关写快照**。
#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveUse {
    /// 主线：每次进关写「本关入口」快照（`裁定27`）。
    Progress,
    /// 每日：只由调用方写 `daily_done`，关卡与继承堆一个字节都不动——每日挑战跑的是无 Boss 无限爬梯，
    /// 把它写进主线进度会凭空造出一条没人打过的"主线进度"。
    DailyDoneOnly,
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
            // 每日槽原样保留盘上那份：它只由 `mark_daily_done` 写，而写是**整文件重写**——
            // 换成 `Progress::new()` 就等于把别人的 level 与继承堆一起抹回第1关（曾经真抹了）。
            // 也不给它下面那条覆盖警告：说"会覆盖你的进度"对它是假话。
            if kind == SaveUse::DailyDoneOnly {
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
pub fn mainline_run(seed: u64, faction: Faction, diff: Difficulty, path: PathBuf, resume: bool) {
    let mut st = RunState {
        inherit: Vec::new(),
        carry_karma: 0,
        has_next: true,
        save: Some(SaveSlot::open(path, faction, diff, resume, SaveUse::Progress)),
    };
    let mut level = st.save.as_ref().map_or(1, |s| s.progress.level);
    if let Some(s) = st.save.as_ref() {
        st.inherit = s.progress.inherit.clone();
        st.carry_karma = s.progress.carry_karma;
    }
    loop {
        let out = one_level(seed, faction, diff, level, &mut st, encounter_for);
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

/// 每日挑战：文档要求「完成后记录日期，防止重复完成」（§廿一:926），但**从未定义"完成"的边界**
/// （裁定29）。这里取最小可核读法——本局**第一次打赢一关**就算完成，只记 `daily_done`，不动主线进度。
/// 诚实缺口：每日挑战规则第 2 步还要「固定卡组+特殊规则」，文档一字未定义 ⇒ 现在的 daily 只是"换日期种子的 play"。
pub fn daily_run(path: PathBuf) {  // §廿一:916 每日挑战模式入口（复合行：本地种子已落，「固定规则」那半欠在每日挑战规则第 2 步的债上）
    let seed = daily_seed();
    println!("每日挑战 seed={seed}");
    let save = SaveSlot::open(path, Faction::Ember, Difficulty::Normal, false, SaveUse::DailyDoneOnly);
    if save.progress.daily_done == seed {  // §廿一:924 每日重置＝只比对**今天**的日种子：记过的是昨天，不挡今天重开
        println!("今天的每日挑战已经打过了（记录日种子={seed}）。想再来一局换个档就行：daily --save /tmp/another.kv");
        return;
    }
    ladder(seed, Faction::Ember, Difficulty::Normal, Some(save));
}

/// 无 Boss、无 60 关上限的爬梯（`play` 与 `daily` 共用；`save=None` 时就是原来的纯跳打）。
fn ladder(seed: u64, faction: Faction, diff: Difficulty, save: Option<SaveSlot>) {
    let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: true, save };
    let mut level = 1u32;
    loop {
        let out = one_level(seed, faction, diff, level, &mut st, |l| (enemy_faction_for(l), None));
        match out {
            Outcome::PlayerWin => {
                mark_daily_done(&mut st);
                level += 1;
            }
            _ => {
                println!("本局终结。seed={seed} 可复现整局。");
                return;
            }
        }
    }
}

/// 每日完成标记（§廿一:926）。只在 `DailyDoneOnly` 槽上生效；已记过就不再重写盘。
fn mark_daily_done(st: &mut RunState) {
    let Some(slot) = st.save.as_mut() else { return };
    if slot.kind != SaveUse::DailyDoneOnly || slot.progress.daily_done == daily_seed() {
        return;
    }
    slot.progress.daily_done = daily_seed();
    persist(slot, "每日完成记录");
}

/// `play`：无限爬梯、无 Boss、不落盘。`--save/--resume` 在这儿没有意义，由 `main.rs` 直接拒（裁定28）。
pub fn play_run(seed: u64, faction: Faction, diff: Difficulty) {
    ladder(seed, faction, diff, None);
}

/// `boss <id>` / `play --boss <id>`：单关跳打某章末 Boss。
/// 跳打是"试一把"，不是"走一遍主线"：把它的 level/牌写进 `progress.kv`，就会凭空多出一段没人打过的进度，
/// 而解锁本来就能由 level 推导（`boss.rs`），另存一份＝第二套真值。所以本函数**永远不落盘**。
pub fn boss_run(seed: u64, faction: Faction, diff: Difficulty, id: crate::boss::BossId) {
    use crate::boss::LEVELS_PER_CHAPTER;
    let level = id.chapter() * LEVELS_PER_CHAPTER;
    let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save: None };
    let out = one_level(seed, faction, diff, level, &mut st, |_| {
        (id.profile().faction, Some(id))
    });
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

/// 一关的外围＝**CLI 这块壳**：把会话层（`session::run_level`）的四个接缝落到 stdin/stdout/磁盘上，
/// 再提供"本关怎么构造"（阵营轮转与 Boss 遭遇）。阶段机本身不在这儿，所以摘壳时只换这个函数与 `CliHost`。
fn one_level<F: Fn(u32) -> (Faction, Option<crate::boss::BossId>)>(
    seed: u64,
    faction: Faction,
    diff: Difficulty,
    level: u32,
    st: &mut RunState,
    enc: F,
) -> Outcome {
    // 解构而非整体借用：会话层要 `inherit`／`carry_karma`，落盘要 `save`，三条各走各的字段。
    let RunState { inherit, carry_karma, has_next, save } = st;
    // 遭遇**只算一次**（改前也是"先定遭遇、再拿去两头用"）。存成值而不是把闭包 `enc` 留着再调一遍：
    // `enc` 是无状态查表，但"同一关两头各查一次"这种事，一旦哪天它有了副作用就是第二套真值。
    let (foe, boss) = enc(level);
    let cfg = crate::session::LevelCfg { head: level_head(level, boss), level, has_next: *has_next };
    let mut host = CliHost { seed, faction, diff, foe, boss, save };
    match crate::session::run_level(&mut host, &cfg, inherit, carry_karma) {
        crate::session::Terminus::Done(out) => out,
        crate::session::Terminus::Quit { msg } => {
            if let Some(m) = msg {
                println!("{m}");
            }
            std::process::exit(0);
        }
    }
}

/// `Host` 的 CLI 实现——全仓唯一还允许 `stdin().read_line()` 与 `process::exit()` 的地方。
/// 逐字节口径：`write` 原样打印（会话层已经带好换行，这里不补），`board`/`log_tail` 与改前的
/// `print!("\n{}", render(&b))`、`println!("{}", log_tail(&b, 12))` 一一对应。
/// flush 只在 `write` 后做一次：改前是"打完提示符必 flush"，改成"任何输出之后 flush"，字节不变、终端不再吞行。
struct CliHost<'a> {
    seed: u64,
    faction: Faction,
    diff: Difficulty,
    /// 本关遭遇，由 `one_level` 算好一次存这儿——头报与构造战斗用的是**同一个** `(阵营, Boss)`。
    foe: Faction,
    boss: Option<crate::boss::BossId>,
    save: &'a mut Option<SaveSlot>,
}

impl crate::session::Host for CliHost<'_> {
    fn write(&mut self, s: &str) {
        use std::io::Write;
        print!("{s}");
        std::io::stdout().flush().ok();
    }

    fn board(&mut self, b: &Battle) {
        print!("\n{}", crate::render::render(b));
    }

    fn log_tail(&mut self, b: &Battle) {
        println!("{}", crate::render::log_tail(b, 12));
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
            None => Battle::new_mainline(self.seed, self.faction, self.foe, self.diff, std::mem::take(inherit), level),
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

    /// 每日槽只许动 `daily_done`。让它顺手写进度就等于凭空造出一条没人打过的"主线进度"——
    /// 这个门禁是纯字段判断，一旦有人把 `daily` 改成共用 `Progress` 槽，全靠这条响。
    /// 注意它**只**管得住 `snapshot_entry` 这一侧；"记一次每日反而抹掉主线"是另一条路，
    /// 由 `daily_done_mark_preserves_an_existing_mainline_progress` 管（那条曾经真的漏了）。
    #[test]
    fn daily_slot_never_records_progress() {
        let path = snap_path("daily");
        let mut save = slot(path.clone(), SaveUse::DailyDoneOnly);
        snapshot_entry(&mut save, 5, Faction::Ember, Difficulty::Normal, &[], 9);
        assert!(!path.exists(), "每日槽不该建主线档");
        assert_eq!(save.unwrap().progress.level, 1, "内存档也不该被推进");
    }

    /// 每日记一次"今天打过"，走的是**整文件重写**——所以它不能顺带把主线档抹回第1关。
    /// 危险在于 `daily_run` 用 `resume=false` 开槽，而冷启动分支把内存档换成 `Progress::new()`，
    /// 于是 `mark_daily_done` 写回的是"第1关+空堆"。开槽和写入都走真实函数，不手工构造中间态。
    #[test]
    fn daily_done_mark_preserves_an_existing_mainline_progress() {
        let path = snap_path("daily-keeps-progress");
        let mut pre = slot(path.clone(), SaveUse::Progress);
        let pile = vec![
            CardInst::new(3, faction_cards(Faction::Ember)[1]),
            CardInst::new(9, faction_cards(Faction::Ember)[9]),
        ];
        snapshot_entry(&mut pre, 5, Faction::Ember, Difficulty::Normal, &pile, 3);

        let opened = SaveSlot::try_open(path.clone(), Faction::Ember, Difficulty::Normal, false, SaveUse::DailyDoneOnly)
            .expect("合法档应能开出每日槽");
        let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save: Some(opened) };
        mark_daily_done(&mut st);

        let got = Progress::load_from(&path).expect("写入后档仍应能被自己的校验器读回");
        assert_eq!(got.level, 5, "打一次每日不该把主线进度抹回第1关");
        assert_eq!(got.inherit.len(), 2, "继承堆不该被抹平");
        assert_eq!(got.inherit[0].id, 3, "id 是战斗输入，重写不得重编号");
        assert_eq!(got.carry_karma, 3, "结转业力不该被抹平");
        assert_eq!(got.daily_done, daily_seed(), "每日记录本身要写上");
        std::fs::remove_file(&path).ok();
    }

    /// 通关只翻标记，绝不写"第61关"：`is_mainline_end` 是 `>=`，越界 level 会让下一次续档赢一关就再报一次通关。
    #[test]
    fn mark_completed_flips_the_flag_without_faking_a_level_beyond_sixty() {
        let path = snap_path("completed");
        let mut save = slot(path.clone(), SaveUse::Progress);
        snapshot_entry(&mut save, 60, Faction::Ember, Difficulty::Normal, &[], 4);
        let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false, save };
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
}
