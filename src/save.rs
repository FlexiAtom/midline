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
//! 存档：主线进度的**关隘入口快照** `progress.kv`（一行一键，卡一行一张）。
//!
//! 文档对"存档"零规定（全文 grep「存档」＝ 0 命中）⇒ 格式、路径、原子性、坏档处置全是本包自设口径，
//! 逐条登记在 裁定27（败/平＝回到本关入口）与 裁定28（格式与校验）。这里只留能溯源的那几条硬约束：
//! §十:382 继承堆不含开端、§十:376 继承堆上限10、§十一:405 升级跨关保留、§廿二:966 第2关起从继承堆抽。
//! 校验器的职责不是挑毛病，是**拦住会撞进 `Battle::new` 静默回落分支的档**——那种档让玩家以为在续第N关，
//! 实际拿到的是第1关的牌序，比丢进度更糟。

use crate::battle::Difficulty;
use crate::boss::MAINLINE_LEVELS;
use crate::model::{CardInst, Faction, Skill, card_by_name};
use std::path::{Path, PathBuf};

/// 档格式版本。改格式必须 +1 且不自动迁移：自动迁移＝替人改战斗输入，旧档一律由人显式 `progress recover` 处置。
pub const SCHEMA: u32 = 1;
/// 默认档名（放在 `pick_path` 解析出的目录里）。
pub const FILE: &str = "progress.kv";

/// 空白与空串一律当"没给"（`--save " "` 不该产出一个名叫空格的档）。
fn tidy(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// 落盘位置的纯函数决策。env 读取留在调用方，测试才能在不碰真实 `$HOME` 的前提下验完四条优先级。
/// `--save` > `$MIDLINE_SAVE` > `$XDG_DATA_HOME/midline/` > `$HOME/.local/share/midline/`；全空 ⇒ `None`（不落盘，由调用方明说）。
pub fn pick_path(explicit: Option<&str>, save_env: Option<&str>, xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = tidy(explicit) {
        return Some(expand_home(p, home));
    }
    if let Some(p) = tidy(save_env) {
        return Some(expand_home(p, home));
    }
    if let Some(d) = tidy(xdg) {
        return Some(expand_home(d, home).join("midline").join(FILE));
    }
    tidy(home).map(|h| PathBuf::from(h.trim_end_matches('/')).join(".local/share/midline").join(FILE))
}

/// `--save ~/x.kv` 若不展开，会在当前目录建出一个字面量 `~` 目录——那是最不像副作用的副作用。
fn expand_home(s: &str, home: Option<&str>) -> PathBuf {
    let h = home.map(str::trim).filter(|h| !h.is_empty());
    if s == "~" {
        return h.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(s));
    }
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(h) = h
    {
        return PathBuf::from(h).join(rest);
    }
    PathBuf::from(s)
}

/// 一次主线运行的可持久进度。字段全部是**关隘入口**那一刻的真值；战斗内的瞬态（hp/flame/seq/…）一个不存。
/// 不建金币/成就/收集字段：文档只点名它们、从未给获取条件（§廿二 的激励系统那行已在排除表里判为规则层范围外），
/// 建了就是没有真值源的第二套壳。
#[derive(Clone, Debug)]
pub struct Progress {
    pub faction: Faction,
    pub diff: Difficulty,
    pub level: u32,
    pub carry_karma: i32,
    /// 主线 60 关打过没有。不是 `level` 的函数（level 会退回 1 重打），是历史凭据，一旦为真不再抹掉。
    pub completed: bool,
    /// 每日挑战已完成的日种子（0＝从没记过）。§廿一:926「完成后记录日期，防止重复完成」。
    pub daily_done: u64,
    pub inherit: Vec<CardInst>,
}

/// 键的写法：faction 用 `--faction` 的 1|2|3，difficulty 用 `--difficulty` 的英文小写⇒ 档面与命令行口径一致。
fn faction_token(f: Faction) -> &'static str {
    match f {
        Faction::Ember => "1",
        Faction::Frost => "2",
        Faction::Shadow => "3",
    }
}

fn faction_of(t: &str) -> Option<Faction> {
    match t {
        "1" => Some(Faction::Ember),
        "2" => Some(Faction::Frost),
        "3" => Some(Faction::Shadow),
        _ => None,
    }
}

fn diff_token(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Easy => "easy",
        Difficulty::Normal => "normal",
        Difficulty::Hard => "hard",
        Difficulty::Expert => "expert",
    }
}

/// 技能写变体名，不写 `Skill::list()` 下标：下标会把格式焊死在枚举顺序上，日后调整顺序＝旧档全体**静默**换技能；
/// 名字对不上则解析失败、判成损坏，出错方向是"响"不是"歪"。
fn skill_token(s: Skill) -> &'static str {
    match s {
        Skill::AtkSelfFlame1 => "AtkSelfFlame1",
        Skill::AtkSameColFlame1 => "AtkSameColFlame1",
        Skill::PlaySameColFlame1 => "PlaySameColFlame1",
        Skill::DeathSameColFlame1 => "DeathSameColFlame1",
        Skill::AllyColAtk1 => "AllyColAtk1",
        Skill::EnemyColAtkM1 => "EnemyColAtkM1",
        Skill::AllyColDmgTakenM1 => "AllyColDmgTakenM1",
        Skill::EnemyColDmgTakenP1 => "EnemyColDmgTakenP1",
        Skill::AtkAdjColFlame1 => "AtkAdjColFlame1",
        Skill::PlayAdjColFlame1 => "PlayAdjColFlame1",
        Skill::AllyColThreshM1 => "AllyColThreshM1",
        Skill::EnemyColThreshP1 => "EnemyColThreshP1",
    }
}

fn skill_of(t: &str) -> Option<Skill> {
    Some(match t {
        "AtkSelfFlame1" => Skill::AtkSelfFlame1,
        "AtkSameColFlame1" => Skill::AtkSameColFlame1,
        "PlaySameColFlame1" => Skill::PlaySameColFlame1,
        "DeathSameColFlame1" => Skill::DeathSameColFlame1,
        "AllyColAtk1" => Skill::AllyColAtk1,
        "EnemyColAtkM1" => Skill::EnemyColAtkM1,
        "AllyColDmgTakenM1" => Skill::AllyColDmgTakenM1,
        "EnemyColDmgTakenP1" => Skill::EnemyColDmgTakenP1,
        "AtkAdjColFlame1" => Skill::AtkAdjColFlame1,
        "PlayAdjColFlame1" => Skill::PlayAdjColFlame1,
        "AllyColThreshM1" => Skill::AllyColThreshM1,
        "EnemyColThreshP1" => Skill::EnemyColThreshP1,
        _ => return None,
    })
}

fn required<T: Copy>(slot: &Option<T>, name: &str) -> Result<T, String> {
    match slot {
        Some(v) => Ok(*v),
        None => Err(format!("缺少键 {name}")),
    }
}

fn set_once<T>(slot: &mut Option<T>, v: T, name: &str) -> Result<(), String> {
    if slot.replace(v).is_some() {
        return Err(format!("键 {name} 重复"));
    }
    Ok(())
}

fn num<T: std::str::FromStr>(v: &str, name: &str, no: usize) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    v.trim()
        .parse::<T>()
        .map_err(|e| format!("第{no}行 {name}＝「{v}」不是数：{e}"))
}

fn boolean(v: &str, name: &str, no: usize) -> Result<bool, String> {
    match v.trim() {
        "0" => Ok(false),
        "1" => Ok(true),
        other => Err(format!("第{no}行 {name}＝「{other}」只能是 0|1")),
    }
}

impl Progress {
    pub fn new(faction: Faction, diff: Difficulty) -> Self {
        Progress {
            faction,
            diff,
            level: 1,
            carry_karma: 0,
            completed: false,
            daily_done: 0,
            inherit: Vec::new(),
        }
    }

    /// 序列化。只写承重字段：`def.cost`/`def.tr`/`def.faction` 由 `card_by_name` 从卡表重建（全仓只有
    /// power/threshold 两个字段会被运行时改写），hp 之类瞬态写进去只会说谎——`Battle::new` 无条件重置它们。
    pub fn to_kv(&self) -> String {
        let mut s = format!(
            "schema={SCHEMA}\nfaction={}\ndifficulty={}\nlevel={}\ncarry_karma={}\ncompleted={}\ndaily_done={}\n",
            faction_token(self.faction),
            diff_token(self.diff),
            self.level,
            self.carry_karma,
            self.completed as u8,
            self.daily_done
        );
        for c in &self.inherit {
            let sk: Vec<&str> = c.skills.iter().map(|k| skill_token(*k)).collect();
            s.push_str(&format!(
                "card={}:{}:{}:{}:{}:{}:{}:{}\n",
                c.def.name,
                c.id,
                c.def.power,
                c.def.threshold,
                c.deaths,
                c.upgrades,
                c.crafted as u8,
                sk.join("+")
            ));
        }
        s
    }

    /// 解析 + 校验。策略＝严格拒绝（裁定28）：任何解析失败、未知 schema、越界 level 一律 `Err`，
    /// 调用方 `exit 2` 且不写不删——"读不懂就当空档"会静默触发 `Battle::new` 的基础牌堆回落分支。
    /// 未知**键**忽略（前向兼容：新字段不该让旧程序把整份档判成损坏）。
    pub fn from_kv(text: &str) -> Result<Progress, String> {
        let (mut schema, mut faction, mut diff, mut level, mut karma, mut completed, mut daily) =
            (None, None, None, None, None, None, None);
        let mut cards: Vec<(usize, String)> = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let no = i + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(format!("第{no}行不是 key=value：「{raw}」"));
            };
            match k {
                "schema" => set_once(&mut schema, num::<u32>(v, k, no)?, k)?,
                "faction" => {
                    let f = faction_of(v.trim()).ok_or_else(|| format!("第{no}行 faction＝「{v}」未知（1|2|3）"))?;
                    set_once(&mut faction, f, k)?
                }
                "difficulty" => {
                    let d = Difficulty::parse(v.trim()).ok_or_else(|| format!("第{no}行 difficulty＝「{v}」未知"))?;
                    set_once(&mut diff, d, k)?
                }
                "level" => set_once(&mut level, num::<u32>(v, k, no)?, k)?,
                "carry_karma" => set_once(&mut karma, num::<i32>(v, k, no)?, k)?,
                "completed" => set_once(&mut completed, boolean(v, k, no)?, k)?,
                "daily_done" => set_once(&mut daily, num::<u64>(v, k, no)?, k)?,
                "card" => cards.push((no, v.trim().to_string())),
                _ => {}
            }
        }
        if required(&schema, "schema")? != SCHEMA {
            return Err(format!(
                "存档版本 schema={} 与本程序（SCHEMA={SCHEMA}）不符 ⇒ 不自动迁移；请人处置（progress recover 会留证）",
                required(&schema, "schema")?
            ));
        }
        let faction = required(&faction, "faction")?;
        let diff = required(&diff, "difficulty")?;
        let level = required(&level, "level")?;
        // `is_mainline_end` 是 `>=`，所以手改出 61 的档赢一关就再报一次"主线通关"；界必须在这里钉死。
        if level == 0 || level > MAINLINE_LEVELS {
            return Err(format!("level＝{level} 越界（合法 1..={MAINLINE_LEVELS}）"));
        }
        let mut p = Progress {
            faction,
            diff,
            level,
            carry_karma: required(&karma, "carry_karma")?,
            completed: required(&completed, "completed")?,
            daily_done: required(&daily, "daily_done")?,
            inherit: Vec::new(),
        };
        if p.carry_karma < 0 {
            return Err(format!("carry_karma＝{} 为负（战斗结束即取 max(0)，负值＝手改）", p.carry_karma));
        }
        for (no, body) in cards {
            p.inherit.push(parse_card(faction, &body, no)?);
        }
        // 载入侧的两条文档硬约束（现实现只在 `collect_survivors` 处强制上限，载入侧此前零检查点）。
        if p.inherit.len() > 10 {
            return Err(format!("继承堆 {} 张 ＞ 上限10（§十:376）", p.inherit.len()));
        }
        if let Some(c) = p.inherit.iter().find(|c| c.is_starter()) {
            return Err(format!("继承堆含开端「{}」（§十:382 明写不包含）", c.def.name));
        }
        // level==1 且堆非空 ⇒ `Battle::new` 的 `level <= 1` 分支把整包牌静默丢掉（玩家看到的是"牌没了"）。
        if level == 1 && !p.inherit.is_empty() {
            return Err(format!(
                "第1关却带 {} 张继承堆：这组合会撞上 `Battle::new` 的丢弃分支，拒载（裁定27）",
                p.inherit.len()
            ));
        }
        Ok(p)
    }

    /// 合法但不好看的那一类：解析成功、可续，但会走 `Battle::new` 的空堆兜底（与 §廿二:966 相抵）。
    /// 不拒载是因为空堆本身是可达状态（整章阵亡后只剩极少牌…实测上限10与FIFO弃置都能留空）；
    /// 但必须打印，否则"续第5关"这句话在玩家耳朵里等于"第5关的牌序"，而那是假的。
    pub fn load_warning(&self) -> Option<String> {
        if self.level >= 2 && self.inherit.is_empty() {
            return Some(format!(
                "第{}关的继承堆是空的 ⇒ 本关实际从基础牌堆开局（§廿二:966 说第2关起从继承堆抽；这是实现的既有兜底，不是存档丢牌）",
                self.level
            ));
        }
        None
    }

    /// 原子写：先落 `同名.tmp` 再 `rename` 覆盖。半截档是最坏的档——它可能解析成功但少几张牌，
    /// 那正是"静默改变战斗输入"的形态，比直接报错难查得多。
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let text = self.to_kv();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败 {}：{e}", dir.display()))?;
        }
        let tmp = side_car(path, ".tmp");
        std::fs::write(&tmp, &text).map_err(|e| format!("写临时档失败 {}：{e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            std::fs::remove_file(&tmp).ok();
            format!("替换失败 {} → {}：{e}", tmp.display(), path.display())
        })
    }

    pub fn load_from(path: &Path) -> Result<Progress, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("读档失败 {}：{e}", path.display()))?;
        Progress::from_kv(&text)
    }

    /// 续档时的阵营闸：档内卡名全部来自上一个阵营的卡表（实测三阵营非开端卡名互不重叠），
    /// 换阵营续档＝12/12 名字解析失败。所以这里不是偏好问题，是"照续必然产出错误牌面"，必须拒。
    pub fn faction_conflict(&self, want: Faction) -> Option<String> {
        if self.faction == want || (self.level == 1 && self.inherit.is_empty()) {
            return None;
        }
        Some(format!(
            "阵营不符：档内是{}、本次 --faction 是{}。继承堆里的卡名只在原阵营卡表里存在 ⇒ 换阵营请开新档（或用 progress recover 清档）",
            self.faction.name(),
            want.name()
        ))
    }
}

fn parse_card(faction: Faction, body: &str, no: usize) -> Result<CardInst, String> {
    let fields: Vec<&str> = body.split(':').collect();
    let [name, id, power, threshold, deaths, upgrades, crafted, skills] = fields.as_slice() else {
        return Err(format!("第{no}行 card 需要 8 段（名:id:值:阈:死:升:造:技能），实测 {} 段：「{body}」", fields.len()));
    };
    if name.contains(':') {
        return Err(format!("第{no}行卡名含冒号，格式无法表达：「{name}」"));
    }
    let def = card_by_name(faction, name).ok_or_else(|| format!("第{no}行卡名「{name}」不在{}卡表里", faction.name()))?;
    // 逐字段范围闸：越界一律判损坏，不做"夹一下继续"。静默夹数＝替人改牌面。
    let power = num::<i32>(power, "power", no)?;
    let threshold = num::<i32>(threshold, "threshold", no)?;
    if power < 1 || threshold < 1 {
        return Err(format!("第{no}行「{name}」数值/阈值必须 ≥1（实测 {power}/{threshold}）"));
    }
    let upgrades = num::<u8>(upgrades, "upgrades", no)?;
    if upgrades > 3 {
        return Err(format!("第{no}行 upgrades＝{upgrades} 越界（§十一:404 每张最多3次）"));
    }
    let mut def = def;
    def.power = power;
    def.threshold = threshold;
    let mut c = CardInst::new(num::<u64>(id, "id", no)?, def);
    c.deaths = num::<u32>(deaths, "deaths", no)?;
    c.upgrades = upgrades;
    c.crafted = boolean(crafted, "crafted", no)?;
    if !skills.is_empty() {
        for t in skills.split('+') {
            c.skills.push(skill_of(t).ok_or_else(|| format!("第{no}行技能「{t}」未知"))?);
        }
    }
    Ok(c)
}

/// 同目录旁的派生名（`.tmp` 写入、`.bad-N` 留证）。`rename` 只在同一文件系统内原子，所以派生名一律同目录。
fn side_car(path: &Path, suffix: &str) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{name}{suffix}"))
}

/// `progress recover`：把当前档整体改名留证，原位清空 ⇒ 下一次运行从空档开始。
/// 只 `rename` 不删除：坏档是人对照裁定时唯一的物证，删了就变成"AI 说它坏了所以它不存在"。
pub fn backup_and_clear(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("没有存档可处置：{}", path.display()));
    }
    let mut n = 1u32;
    loop {
        let cand = side_car(path, &format!(".bad-{n}"));
        if !cand.exists() {
            std::fs::rename(path, &cand).map_err(|e| format!("改名失败 {} → {}：{e}", path.display(), cand.display()))?;
            return Ok(cand);
        }
        n += 1;
    }
}

/// 已存在且可解析的档；不存在 ⇒ `Ok(None)`。**解析失败 ⇒ `Err`**（本包的默认态度：坏档不猜）。
pub fn load_if_any(path: &Path) -> Result<Option<Progress>, String> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(Progress::load_from(path)?))
}

/// 当前档的真实落盘位置（生产路径的唯一读取口，便于 `progress` 子命令如实报路径）。
pub fn default_path() -> Option<PathBuf> {
    pick_path(
        None,
        std::env::var("MIDLINE_SAVE").ok().as_deref(),
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// `--save <路径>` 的落盘位置：显式路径 > 环境 > 默认；一条都没有 ⇒ `None`（调用方必须明说不落盘，不静默）。
pub fn path_for(explicit: Option<&str>) -> Option<PathBuf> {
    pick_path(
        explicit,
        std::env::var("MIDLINE_SAVE").ok().as_deref(),
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// `progress` 子命令的全部输出。只读，绝不写、绝不改、绝不"顺手修复"。
pub fn describe(path: &Path) -> String {
    let mut out = format!("存档路径：{}\n", path.display());
    if !path.exists() {
        out.push_str("（没有这份存档）\n");
        return out;
    }
    match Progress::load_from(path) {
        Err(e) => {
            out.push_str(&format!("✖ 解析失败：{e}\n"));
            out.push_str("处置：`progress recover` 改名留证后从空档开始（它不删文件）。\n");
            out
        }
        Ok(p) => {
            let ch = crate::boss::chapter_of(p.level);
            let (foe, boss) = crate::meta::encounter_for(p.level);
            out.push_str(&format!(
                "schema={SCHEMA} · 我方{} · 难度{} · 进度第{}/{}关（第{ch}章·遭遇{}）\n",
                p.faction.name(),
                p.diff.label(),
                p.level,
                MAINLINE_LEVELS,
                boss.map_or(foe.name().to_string(), |b| format!("Boss「{}」", b.name()))
            ));
            out.push_str(&format!(
                "结转业力={} · 主线通关标记={} · 每日已完成日种子={}（今日种子 {}）\n",
                p.carry_karma,
                if p.completed { "是" } else { "否" },
                p.daily_done,
                crate::meta::daily_seed()
            ));
            if let Some(w) = p.load_warning() {
                out.push_str(&format!("⚠ {w}\n"));
            }
            out.push_str(&format!("继承堆（{}张，堆顶在前＝开局先抽）：\n", p.inherit.len()));
            if p.inherit.is_empty() {
                out.push_str("  （空）\n");
            }
            for (i, c) in p.inherit.iter().enumerate() {
                out.push_str(&format!("  [{i}] id{} {}\n", c.id, crate::model::short_card(c)));
            }
            out
        }
    }
}

#[cfg(test)]
mod save_tests {
    use super::*;
    use crate::model::{faction_cards, Skill};
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// 测试专用落盘位：一律 `/tmp` 下按 pid+计数器取唯一目录，**绝不碰真实 `$HOME`**（本模块的默认路径逻辑
    /// 由 `pick_path` 的纯函数版本验，测试不读环境变量）。
    fn temp_path(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("midline-save-{}-{n}-{tag}", std::process::id()))
    }

    fn ember_card(i: usize) -> CardInst {
        CardInst::new(i as u64 + 1, faction_cards(Faction::Ember)[i])
    }

    fn sample() -> Progress {
        let mut a = ember_card(1); // 火苗
        a.def.power = 5; // 升级过的数值（md:405 跨关保留）
        a.deaths = 2;
        a.upgrades = 3;
        a.skills = vec![Skill::AtkSelfFlame1, Skill::AtkSelfFlame1]; // 同名叠加两层，不许去重
        let mut b = ember_card(3); // 焚稿人（带阈值特性）
        b.def.threshold = 1;
        b.crafted = true;
        let mut c = ember_card(9); // 燎原
        c.id = 9_000; // id 原样持久化：它是战斗输入（跨侧自排除 + 收尸排序），重编号会改牌面
        Progress {
            faction: Faction::Ember,
            diff: Difficulty::Hard,
            level: 17,
            carry_karma: 6,
            completed: false,
            daily_done: 20_454,
            inherit: vec![a, b, c],
        }
    }

    #[test]
    fn roundtrip_keeps_every_load_bearing_field_and_resets_transients() {
        let p = sample();
        let back = Progress::from_kv(&p.to_kv()).expect("自产档必须能读回");
        assert_eq!(back.level, 17);
        assert_eq!(back.faction, Faction::Ember);
        assert_eq!(back.diff, Difficulty::Hard);
        assert_eq!(back.carry_karma, 6);
        assert_eq!(back.daily_done, 20_454);
        assert!(!back.completed);
        assert_eq!(back.inherit.len(), 3);
        assert_eq!(back.inherit[0].id, 2, "id 逐位原样");
        assert_eq!(back.inherit[2].id, 9_000, "大 id 不得被重新编号");
        assert_eq!(back.inherit[0].def.power, 5, "power 覆写必须落盘：只存卡名会退回卡表基准");
        assert_eq!(back.inherit[0].deaths, 2, "死亡返还是按实例计数的（§廿三:984）");
        assert_eq!(back.inherit[0].upgrades, 3);
        assert_eq!(back.inherit[0].skills, vec![Skill::AtkSelfFlame1, Skill::AtkSelfFlame1], "同名技能两层不得折叠成集合");
        assert!(back.inherit[1].crafted, "自造牌标记＝它下关阵亡要永久消失");
        assert_eq!(back.inherit[1].def.threshold, 1);
        assert_eq!(back.inherit[1].def.cost, 3, "cost 由卡表重建，不入档");
        // 瞬态一律不得被带进来：写它们既被 `Battle::new` 忽略、又会让 `progress` 说谎
        assert_eq!(back.inherit[0].hp, 5, "hp 由 def.power 重建（每关重置满格 md:373）");
        assert_eq!(back.inherit[0].flame, 0);
        assert_eq!(back.inherit[0].seq, 0);
        assert_eq!(back.inherit[0].placed_turn, i64::MIN);
        assert_eq!(back.inherit[1].triggered_turn, i64::MIN);
        assert_eq!(back.inherit[2].def.name, "燎原", "顺序是战斗输入（堆顶先抽），载入不得重排");
    }

    #[test]
    fn empty_pile_writes_a_readable_file() {
        let p = Progress::new(Faction::Shadow, Difficulty::Easy);
        let back = Progress::from_kv(&p.to_kv()).expect("空档也是合法档");
        assert_eq!(back.level, 1);
        assert!(back.inherit.is_empty());
        assert_eq!(back.faction, Faction::Shadow);
        assert!(!p.to_kv().contains("card="));
    }

    #[test]
    fn unknown_keys_are_ignored_for_forward_compatibility() {
        let text = sample().to_kv() + "coins=99\nfuture_thing=abc\n";
        let p = Progress::from_kv(&text).expect("未知键必须忽略（文档点名的金币/成就没有获取条件⇒不建字段，但旧程序要能读新档）");
        assert_eq!(p.level, 17);
    }

    /// 每一行破坏都必须是**响**的：本表列的都是会静默改成战斗输入、或静默丢牌、或谎报进度的形态。
    #[test]
    fn each_corruption_is_refused_not_guessed() {
        let good = sample().to_kv();
        let cases: Vec<(&str, String)> = vec![
            ("版本不符", good.replace("schema=1", "schema=2")),
            ("缺键", good.replace("level=17\n", "")),
            ("重复键", format!("{good}level=3\n")),
            ("非 kv 行", format!("{good}手改的一行说明\n")),
            ("level=0", good.replace("level=17", "level=0")),
            ("level=61 越界", good.replace("level=17", "level=61")),
            ("level 非数", good.replace("level=17", "level=十七")),
            ("负业力", good.replace("carry_karma=6", "carry_karma=-1")),
            ("completed 非布尔", good.replace("completed=0", "completed=2")),
            ("daily_done 负", good.replace("daily_done=20454", "daily_done=-1")),
            ("未知卡名", good.replace("card=火苗", "card=不存在的卡")),
            ("跨阵营卡名", good.replace("card=火苗", "card=初霜")),
            ("开端入堆 §十:382", good.replace("card=火苗", "card=开端")),
            ("段数不足", good.replace("card=火苗:2:5:4:2:3:0", "card=火苗:2:5:4")),
            ("数值越界", good.replace("card=火苗:2:5:4", "card=火苗:2:0:4")),
            ("阈值越界", good.replace("card=焚稿人:4:4:1", "card=焚稿人:4:4:0")),
            ("升级数越界 §十一:404", good.replace(":3:0:AtkSelfFlame1", ":4:0:AtkSelfFlame1")),
            ("未知技能名", good.replace("AtkSelfFlame1+AtkSelfFlame1", "AtkSelfFlame1+NoSuchSkill")),
            ("crafted 非布尔", good.replace("2:3:0:AtkSelfFlame1", "2:3:yes:AtkSelfFlame1")),
            (
                "上限10 §十:376",
                format!("{}{good}", "card=余温:101:2:4:0:0:0:\n".repeat(11)),
            ),
        ];
        for (tag, text) in cases {
            assert!(Progress::from_kv(&text).is_err(), "破坏样本「{tag}」被当成合法档读过了");
        }
    }

    #[test]
    fn level_one_with_a_pile_is_refused_but_empty_high_level_warns() {
        // level==1 且堆非空 ⇒ `Battle::new` 整包丢弃继承堆（静默丢牌）⇒ 拒载。
        let mut p = sample();
        p.level = 1;
        assert!(Progress::from_kv(&p.to_kv()).is_err(), "第1关带牌必须拒");
        // level≥2 且空堆 ⇒ 走既有兜底（基础牌堆），合法但必须能打印警告。
        let mut q = sample();
        q.level = 5;
        q.inherit.clear();
        let back = Progress::from_kv(&q.to_kv()).expect("空堆的第5关是可达状态，不是损坏");
        assert!(back.load_warning().expect("必须给出 §廿二:966 口径的警告").contains("基础牌堆"));
        assert!(p.load_warning().is_none(), "第1关带牌不该走到警告这条路");
    }

    #[test]
    fn faction_gate_refuses_cross_faction_resume_but_spares_fresh_file() {
        let frost = Progress {
            faction: Faction::Frost,
            inherit: vec![CardInst::new(3, faction_cards(Faction::Frost)[1])],
            level: 4,
            ..sample()
        };
        assert!(frost.faction_conflict(Faction::Ember).is_some(), "档内霜卡名在烬火表里一个都查不到 ⇒ 必须拒");
        assert!(frost.faction_conflict(Faction::Frost).is_none());
        // 空档（第1关、无牌）的阵营是无主默认值：拿它拒人＝凭空造出一条"你换过阵营"的假史
        let fresh = Progress::new(Faction::Frost, Difficulty::Normal);
        assert!(fresh.faction_conflict(Faction::Ember).is_none(), "全新档不该拦第一次开局");
    }

    #[test]
    fn path_priority_is_env_free_and_explicit_wins() {
        let h = "/home/tester";
        assert_eq!(
            pick_path(Some("/tmp/a.kv"), Some("/tmp/b.kv"), Some("/xdg"), Some(h)).unwrap(),
            PathBuf::from("/tmp/a.kv")
        );
        assert_eq!(pick_path(None, Some("/tmp/b.kv"), Some("/xdg"), Some(h)).unwrap(), PathBuf::from("/tmp/b.kv"));
        assert_eq!(
            pick_path(None, None, Some("/xdg"), Some(h)).unwrap(),
            PathBuf::from("/xdg/midline/progress.kv")
        );
        assert_eq!(
            pick_path(None, None, None, Some(h)).unwrap(),
            PathBuf::from("/home/tester/.local/share/midline/progress.kv")
        );
        assert_eq!(pick_path(None, None, None, None), None, "一个都定不出来 ⇒ None，由调用方明说不落盘");
        // 空白值视为没给（`--save " "` 不该产出一个名叫空格的档）
        assert_eq!(pick_path(Some("  "), None, None, Some(h)).unwrap(), PathBuf::from(h).join(".local/share/midline/progress.kv"));
        assert_eq!(pick_path(Some("~/x.kv"), None, None, Some(h)).unwrap(), PathBuf::from("/home/tester/x.kv"), "`~` 不展开会在当前目录建字面量 ~ 目录");
        assert_eq!(pick_path(Some("~/x.kv"), None, None, None).unwrap(), PathBuf::from("~/x.kv"), "没有 HOME 时原样给出，不假装展开");
    }

    #[test]
    fn save_is_atomic_and_creates_missing_dirs() {
        let path = temp_path("atomic").join("nested").join("dir").join(FILE);
        sample().save_to(&path).expect("父目录不存在也要写成");
        assert_eq!(Progress::load_from(&path).unwrap().level, 17);
        assert!(!side_car(&path, ".tmp").exists(), "写完不留 .tmp 残骸");
        // 覆盖写：旧内容不得残留（半截档比报错更坏）
        let mut p = sample();
        p.level = 41;
        p.inherit.clear();
        p.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("level=41") && !text.contains("card="), "覆盖后＝新快照，不做增量合并");
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap().parent().unwrap()).ok();
    }

    #[test]
    fn recover_renames_and_never_deletes() {
        let dir = temp_path("recover");
        let path = dir.join(FILE);
        sample().save_to(&path).unwrap();
        let bad = backup_and_clear(&path).unwrap();
        assert!(bad.to_string_lossy().ends_with(".bad-1"), "第一份留证名：{}", bad.display());
        assert!(!path.exists());
        assert!(std::fs::read_to_string(&bad).unwrap().contains("level=17"), "内容整体搬家，一个字节不丢");
        // 再攒两份：计数器递增、绝不覆盖已有留证
        sample().save_to(&path).unwrap();
        assert!(backup_and_clear(&path).unwrap().to_string_lossy().ends_with(".bad-2"));
        assert!(backup_and_clear(&PathBuf::from("/definitely/not/here/x.kv")).is_err(), "没有档可处置要报错，不静默成功");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 格式的承重不变量：卡名里不能有分隔符。今天是 37 个名字都干净，明天加一张「A:B」就会解析错位。
    #[test]
    fn card_names_survive_the_field_separator() {
        for f in [Faction::Ember, Faction::Frost, Faction::Shadow] {
            for d in faction_cards(f) {
                assert!(!d.name.contains(':') && !d.name.contains('=') && !d.name.contains('\n'), "卡名「{}」会打破 kv 格式", d.name);
            }
        }
    }

    #[test]
    fn describe_reports_missing_and_present_without_touching_disk() {
        let dir = temp_path("describe");
        let missing = dir.join(FILE);
        let text = describe(&missing);
        assert!(text.contains("没有这份存档"), "缺档要如实报缺，不写成空档：\n{text}");
        sample().save_to(&missing).unwrap();
        let text = describe(&missing);
        assert!(text.contains("第17/60关") && text.contains("继承堆（3张"), "进度与牌数要看得见：\n{text}");
        assert!(text.contains("id2"), "id 是战斗输入，报进度就该报出来");
        std::fs::write(&missing, "schema=1\nlevel=oops\n").unwrap();
        let text = describe(&missing);
        assert!(text.contains("解析失败") && text.contains("progress recover"), "坏档给出下一步，不止报错：\n{text}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
