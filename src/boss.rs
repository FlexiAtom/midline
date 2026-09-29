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
//! Boss 遭遇层（§二十「Boss 脚本Combo + 特殊规则」+ §二十一 五章 Boss 表）。
//! 脚本 = 「回合 → 动作表」：intro 一次性 + cycle 回绕；每个动作仍走 battle.rs 的全部合法性检查，
//! 脚本只是"更守规矩的玩家"，不另开规则。特殊规则 5 条（炉温/霜封/暗渡/炎冰同源/吞名）以
//! `BossRule` 派生，battle.rs 通过本模块的 hook 函数调用，普通对局（boss=None）永不进入。

use crate::battle::{Battle, Outcome, Row};
use crate::model::{CardInst, Faction, Skill, card_by_name};

/// §廿一 Boss 表 5 行——逐行锚点挂在下面各变体上（列头行不算规则行，由 §廿一 推导器按结构剔掉）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BossId {
    Luzhu,    // §廿一:932 第一章 炉主 · 烬火教团首领
    Xuejue,   // §廿一:933 第二章 雪爵 · 霜誓守卫领袖
    Yingzhang, // §廿一:934 第三章 影长 · 幽影议会首脑
    YanBing,  // §廿一:935 第四章 炎与冰 · 双Boss
    ZhongYing, // §廿一:936 第五章 终影 · 最终Boss
}

/// 每 Boss 恰好一条特殊规则（测试按此枚举分派）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BossRule {
    None,
    FurnaceHeat, // 炉温：敌方回合开始，敌方全部在场卡业火+1（立即检查阈值）
    FrostBrand,  // 霜封：我方卡牌对敌方前排卡伤害-1（最低0），开端不受削减
    ShadowPush,  // 暗渡：脚本主动把后排压上已占用的前排 → 前排越线死亡（覆盖 §十七:757「AI不会主动将E5-E8挤越线」这条 AI 自律，不覆盖挤压机制本身）
    TwinKindle,  // 炎冰同源：双持业者 + 按列分区 + 同源反噬 ⌊伤害/2⌋
    DevourName,  // 吞名：我方主动献祭改按死亡返还递减（开端仍定额2）
}

impl BossRule {
    pub fn label(self) -> &'static str {
        match self {
            BossRule::None => "无",
            BossRule::FurnaceHeat => "炉温",
            BossRule::FrostBrand => "霜封",
            BossRule::ShadowPush => "暗渡",
            BossRule::TwinKindle => "炎冰同源",
            BossRule::DevourName => "吞名",
        }
    }
}

/// 列分区（双 Boss 棋盘用）：Left = 第1-2列，Right = 第3-4列。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    All,
    Left,
    Right,
}

impl Zone {
    fn has(self, col: usize) -> bool {
        match self {
            Zone::All => true,
            Zone::Left => col < 2,
            Zone::Right => col >= 2,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Zone::All => "全场",
            Zone::Left => "左侧",
            Zone::Right => "右侧",
        }
    }
}

/// 目标格：`Back/Front` 为脚本固定格（combo 用），`Any*In` 为确定性择优（分区内列号最小者）。
#[derive(Clone, Copy, Debug)]
pub enum Slot {
    Back(usize),
    Front(usize),
    AnyBackIn(Zone),
}

/// 脚本卡（按名从 model 卡表解析；技能由脚本显式声明，不随机、不消耗 Rng）。
#[derive(Clone, Copy, Debug)]
pub struct BossCard {
    pub faction: Faction,
    pub name: &'static str,
    pub skills: &'static [Skill],
}

const fn card(f: Faction, name: &'static str) -> BossCard {
    BossCard { faction: f, name, skills: &[] }
}

const fn card_sk(f: Faction, name: &'static str, skills: &'static [Skill]) -> BossCard {
    BossCard { faction: f, name, skills }
}

/// 脚本动作。`Sac` 显式指名（祭哪张是数据真值，不是运行时择优）。
#[derive(Clone, Copy, Debug)]
pub enum BossAction {
    Place { card: BossCard, slot: Slot },
    /// 献祭指定名的卡（场上，先前排后后排，限该分区）。
    Sac { card: BossCard },
}

#[derive(Clone, Copy, Debug)]
pub struct BossTurn {
    pub actions: &'static [BossAction],
}

const fn turn(actions: &'static [BossAction]) -> BossTurn {
    BossTurn { actions }
}

pub struct BossProfile {
    pub chapter: u32,
    pub name: &'static str,
    pub title: &'static str,
    pub faction: Faction,
    pub second_faction: Option<Faction>,
    pub rule: BossRule,
    pub rule_text: &'static str,
    pub intro: &'static [BossTurn],
    pub cycle: &'static [BossTurn],
    /// 敌方起手业力（一次性预算 = intro 花费 + 周期最大下凹；自造，见 model.rs 裁定20）。
    pub start_karma: i32,
    pub holder_hp: i32,
    /// Some ⇒ 双持业者（仅炎与冰）；`e_candle` 恒为 1 号。
    pub holder_hp2: Option<i32>,
    pub holder_names: [&'static str; 2],
}

use Faction::*;

/// 脚本账本模型（Boss 经济的落地方式；design-boss §10-C1 提的"首领威压每回合发钱"实测不需要，已剪掉）：
/// 每个 cycle 回合 = **先祭后放**（引擎 §四「每回合最多献祭1次」对脚本同样生效，不例外），
/// 且祭掉的正是 `m` 回合前放进去的那张（FIFO 传送带，`m` = 该 Boss 场内常驻脚本卡数）。于是：
/// - I1 棋盘不溢出：场内脚本卡数恒为 `m`（另加开端与至多一张常驻燃料卡）≤ 8 格；
/// - I2 业力自我闭合：一个周期内 Σ献祭返还 = Σ放置费用 ⇒ 每回合发钱恒 0，
///   §三「业力不自动恢复」不被违反（Boss 烧自己的牌供能，与玩家同一条经济规则）；
///   影长是退化形：它不献祭，靠暗渡把前排挤越线拿死亡返还，故每回合只放一张（周期净额同样为 0）；
/// - I3 可复现：脚本不走敌方手牌/双牌堆、不吃 rng，祭/放的名字全是数据真值。
///
/// `intro` 的职责是**种下 cycle 前 m 个回合要祭的那些卡**——漏种就会在第 2 周期起 `无可献祭`。
/// 反推 start_karma：intro 总花费 + 周期内最大下凹（下凹按回合中段的瞬时点算，不是按回合净额）。
/// 下面 5 份数值全部由 `boss_ledger_is_self_funding_over_30_turns`（被动沙包 30 回合零跳过）实测反推。
const LUZHU: BossProfile = BossProfile {
    chapter: 1,
    name: "炉主",
    title: "烬火教团首领",
    faction: Ember,
    second_faction: None,
    rule: BossRule::FurnaceHeat,
    rule_text: "敌方回合开始时，敌方全部在场卡（E1-E8）业火+1（立即检查阈值）",
    // 传送带 m=4，放序 引燃者→炉壁→守夜人→燎原→山火→火星，祭序 = 4 回合前的放序。
    intro: &[
        turn(&[
            BossAction::Place { card: card(Ember, "开端"), slot: Slot::Back(0) },
            BossAction::Place { card: card(Ember, "守夜人"), slot: Slot::Back(1) },
        ]),
        turn(&[
            BossAction::Place { card: card(Ember, "燎原"), slot: Slot::Back(2) },
            BossAction::Place { card: card(Ember, "山火"), slot: Slot::Back(3) },
        ]),
        turn(&[
            BossAction::Place { card: card(Ember, "火星"), slot: Slot::AnyBackIn(Zone::All) },
            // 常驻火种：不入传送带；火苗无阈值特性 ⇒ 炉温只让它业火越叠越厚，不会自毁。
            BossAction::Place { card: card(Ember, "火苗"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
    ],
    cycle: &[
        turn(&[
            BossAction::Sac { card: card(Ember, "守夜人") },
            BossAction::Place { card: card(Ember, "引燃者"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Ember, "燎原") },
            BossAction::Place { card: card(Ember, "炉壁"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Ember, "山火") },
            BossAction::Place { card: card(Ember, "守夜人"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Ember, "火星") },
            BossAction::Place { card: card(Ember, "燎原"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Ember, "引燃者") },
            BossAction::Place { card: card(Ember, "山火"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Ember, "炉壁") },
            BossAction::Place { card: card(Ember, "火星"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
    ],
    // intro 花 3+3+5+2+1=14；周期下凹最低点在山火那一手（回合内瞬时差 1）⇒ 留 2 余量。
    start_karma: 16,
    holder_hp: 20,
    holder_hp2: None,
    holder_names: ["炉主", ""],
};

const XUEJUE: BossProfile = BossProfile {
    chapter: 2,
    name: "雪爵",
    title: "霜誓守卫领袖",
    faction: Frost,
    second_faction: None,
    rule: BossRule::FrostBrand,
    rule_text: "我方卡牌对敌方前排卡的伤害-1（最低0）；开端不受霜封削减（无热可封）",
    // 传送带 m=4，放序 极光→壁→雪线→冰川→望哨→冰刺。
    intro: &[
        turn(&[
            BossAction::Place { card: card(Frost, "开端"), slot: Slot::Back(0) },
            BossAction::Place { card: card(Frost, "望哨"), slot: Slot::Back(1) },
        ]),
        turn(&[
            BossAction::Place { card: card(Frost, "冰川"), slot: Slot::Back(2) },
            BossAction::Place { card: card(Frost, "雪线"), slot: Slot::Back(3) },
        ]),
        turn(&[
            BossAction::Place { card: card(Frost, "冰刺"), slot: Slot::AnyBackIn(Zone::All) },
            // 常驻寒哨：削我方该列攻击，是雪爵"守"的身份牌，不被传送带回收。
            BossAction::Place { card: card(Frost, "寒哨"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
    ],
    cycle: &[
        turn(&[
            BossAction::Sac { card: card(Frost, "望哨") },
            BossAction::Place { card: card(Frost, "极光"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Frost, "冰川") },
            BossAction::Place { card: card(Frost, "壁"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Frost, "雪线") },
            BossAction::Place { card: card(Frost, "望哨"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Frost, "冰刺") },
            BossAction::Place { card: card(Frost, "冰川"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Frost, "极光") },
            BossAction::Place { card: card(Frost, "雪线"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Frost, "壁") },
            BossAction::Place { card: card(Frost, "冰刺"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
    ],
    // intro 花 2+5+3+2+2=14；回合内瞬时最低差 2（冰川 5 费接在 2 费返还之后）⇒ 留 3 余量。
    start_karma: 17,
    holder_hp: 20,
    holder_hp2: None,
    holder_names: ["雪爵", ""],
};

const YINGZHANG: BossProfile = BossProfile {
    chapter: 3,
    name: "影长",
    title: "幽影议会首脑",
    faction: Shadow,
    second_faction: None,
    rule: BossRule::ShadowPush,
    rule_text: "敌方后排卡可直接压上已占用的前排（寻常 AI 不会主动挤越线自杀，本 Boss 会）→ 前排越线死亡，按死亡返还递减给敌方业力",
    // 暗渡没有"献祭"这一手：业力全靠把前排挤越线。故传送带退化为**每回合只放一张**，
    // 场内卡数恒定（挤死一张才补上一张）；费用按"贵→影仆→贵→影仆"配对，一周期净 0。
    intro: &[
        turn(&[
            BossAction::Place { card: card(Shadow, "开端"), slot: Slot::Back(0) },
            BossAction::Place { card: card(Shadow, "影仆"), slot: Slot::Back(1) },
        ]),
        turn(&[
            BossAction::Place { card: card(Shadow, "无痕"), slot: Slot::Back(2) },
            BossAction::Place { card: card(Shadow, "低语"), slot: Slot::Back(3) },
        ]),
        turn(&[
            // 第一手就当众演一遍暗渡：连自己的开端也挤越线（返还 2，脚本不认"自己人"）。
            BossAction::Place { card: card(Shadow, "蚀"), slot: Slot::Front(0) },
        ]),
    ],
    cycle: &[
        turn(&[
            BossAction::Place { card: card(Shadow, "幽雷"), slot: Slot::Front(1) },
        ]),
        turn(&[
            BossAction::Place { card: card(Shadow, "影仆"), slot: Slot::Front(1) },
        ]),
        turn(&[
            BossAction::Place { card: card_sk(Shadow, "引渡", &[Skill::DeathSameColFlame1]), slot: Slot::Front(2) },
        ]),
        turn(&[
            BossAction::Place { card: card(Shadow, "影仆"), slot: Slot::Front(2) },
        ]),
        turn(&[
            BossAction::Place { card: card(Shadow, "夜行"), slot: Slot::Front(3) },
        ]),
        turn(&[
            BossAction::Place { card: card(Shadow, "影仆"), slot: Slot::Front(3) },
        ]),
    ],
    // intro 花 1+2+2+3=8 且挤开端只回 2；C1 一手要 4 费而前排只回 1 ⇒ 最低点 K-9。
    start_karma: 10,
    holder_hp: 20,
    holder_hp2: None,
    holder_names: ["影长", ""],
};

const YANBING: BossProfile = BossProfile {
    chapter: 4,
    name: "炎与冰",
    title: "双Boss",
    faction: Ember,
    second_faction: Some(Frost),
    rule: BossRule::TwinKindle,
    rule_text: "两根持业者（炎/冰各20）；我方按列分区直击（第1-2列→冰，第3-4列→炎）；任一根被削时另一根同步承受⌊伤害/2⌋；两根皆烛尽才算胜；30回合判定取两根中较长值",
    // 炎冰各烧各的薪：奇数回合动右带（烬火），偶数回合动左带（霜誓），每回合仍是引擎允许的
    // **一次**献祭 + 一次放置（§四 每回合最多献祭1次，脚本不例外）。带长 m=4 ⇒ 场内恒 2炎+2冰。
    intro: &[
        turn(&[
            BossAction::Place { card: card(Ember, "开端"), slot: Slot::Back(0) },
            BossAction::Place { card: card(Ember, "引燃者"), slot: Slot::AnyBackIn(Zone::Right) },
        ]),
        turn(&[
            BossAction::Place { card: card(Ember, "炉壁"), slot: Slot::AnyBackIn(Zone::Right) },
            BossAction::Place { card: card(Frost, "望哨"), slot: Slot::AnyBackIn(Zone::Left) },
        ]),
        turn(&[
            BossAction::Place { card: card(Frost, "壁"), slot: Slot::AnyBackIn(Zone::Left) },
        ]),
    ],
    cycle: &[
        turn(&[
            BossAction::Sac { card: card(Ember, "引燃者") },
            BossAction::Place { card: card(Ember, "火苗"), slot: Slot::AnyBackIn(Zone::Right) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Frost, "望哨") },
            BossAction::Place { card: card(Frost, "初霜"), slot: Slot::AnyBackIn(Zone::Left) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Ember, "炉壁") },
            BossAction::Place { card: card(Ember, "引燃者"), slot: Slot::AnyBackIn(Zone::Right) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Frost, "壁") },
            BossAction::Place { card: card(Frost, "望哨"), slot: Slot::AnyBackIn(Zone::Left) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Ember, "火苗") },
            BossAction::Place { card: card(Ember, "炉壁"), slot: Slot::AnyBackIn(Zone::Right) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Frost, "初霜") },
            BossAction::Place { card: card(Frost, "壁"), slot: Slot::AnyBackIn(Zone::Left) },
        ]),
    ],
    // intro 花 2+3+2+3=10；C5/C6 用 1 费祭品换 3 费炉料，周期净 0 ⇒ 只需 2 点余量。
    start_karma: 12,
    holder_hp: 20,
    holder_hp2: Some(20),
    holder_names: ["炎", "冰"],
};

const ZHONGYING: BossProfile = BossProfile {
    chapter: 5,
    name: "终影",
    title: "最终Boss",
    faction: Shadow,
    second_faction: None,
    rule: BossRule::DevourName,
    rule_text: "我方主动献祭不再获得全额费用，改按死亡返还递减（100/50/25/10，deaths+1）；开端献祭仍为定额2；本战持业者蜡烛30",
    // 传送带 m=4，放序 幽雷→引渡→深壑→止时→夜行→无痕；最终 Boss 的"多"落在技能与时长（30烛），不落在新机制。
    intro: &[
        turn(&[
            BossAction::Place { card: card(Shadow, "开端"), slot: Slot::Back(0) },
            BossAction::Place { card: card(Shadow, "深壑"), slot: Slot::Back(1) },
            BossAction::Place { card: card_sk(Shadow, "止时", &[Skill::PlaySameColFlame1]), slot: Slot::Back(2) },
        ]),
        turn(&[
            BossAction::Place { card: card(Shadow, "夜行"), slot: Slot::Back(3) },
            BossAction::Place { card: card(Shadow, "无痕"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            // 常驻影子：终影的真名藏在一张不起眼的影仆里（吞名的门面），不入传送带。
            BossAction::Place { card: card(Shadow, "影仆"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
    ],
    cycle: &[
        turn(&[
            BossAction::Sac { card: card(Shadow, "深壑") },
            BossAction::Place { card: card(Shadow, "幽雷"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Shadow, "止时") },
            BossAction::Place { card: card_sk(Shadow, "引渡", &[Skill::DeathSameColFlame1]), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Shadow, "夜行") },
            BossAction::Place { card: card(Shadow, "深壑"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Shadow, "无痕") },
            BossAction::Place { card: card(Shadow, "止时"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Shadow, "幽雷") },
            BossAction::Place { card: card(Shadow, "夜行"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
        turn(&[
            BossAction::Sac { card: card(Shadow, "引渡") },
            BossAction::Place { card: card(Shadow, "无痕"), slot: Slot::AnyBackIn(Zone::All) },
        ]),
    ],
    // intro 花 5+3+3+2+1=14；周期内瞬时最低差 3（深壑 5 费接在止时 3 费返还之后）⇒ 留 4。
    start_karma: 18,
    holder_hp: 30,
    holder_hp2: None,
    holder_names: ["终影", ""],
};

impl BossId {
    pub fn all() -> [BossId; 5] {
        [BossId::Luzhu, BossId::Xuejue, BossId::Yingzhang, BossId::YanBing, BossId::ZhongYing]
    }

    pub fn profile(self) -> &'static BossProfile {
        match self {
            BossId::Luzhu => &LUZHU,
            BossId::Xuejue => &XUEJUE,
            BossId::Yingzhang => &YINGZHANG,
            BossId::YanBing => &YANBING,
            BossId::ZhongYing => &ZHONGYING,
        }
    }

    /// CLI 双写兼容：`boss luzhu` / `boss 炉主` / `boss 4`。
    pub fn parse(s: &str) -> Option<BossId> {
        let by_name = [("luzhu", "炉主"), ("xuejue", "雪爵"), ("yingzhang", "影长"), ("yanbing", "炎与冰"), ("zhongying", "终影")];
        if let Some(i) = s.parse::<u32>().ok().filter(|n| (1..=5).contains(n)) {
            return BossId::all().get((i - 1) as usize).copied();
        }
        by_name
            .iter()
            .position(|(ascii, cn)| *ascii == s || *cn == s)
            .map(|i| BossId::all()[i])
    }

    pub fn chapter(self) -> u32 {
        self.profile().chapter
    }

    pub fn name(self) -> &'static str {
        self.profile().name
    }
}

// ---------- 主线章节↔关卡映射（章末为 Boss） ----------

pub const MAINLINE_LEVELS: u32 = 60; // §廿一:914 主线体量「60关」（复合行另两clause：5章见下一行，每章新阵营见 battle.rs:321）
pub const LEVELS_PER_CHAPTER: u32 = 12; // §廿一:914 「5章」＝60/12，章末为 Boss（`boss_for_level`）

pub fn chapter_of(level: u32) -> u32 {
    ((level.saturating_sub(1)) / LEVELS_PER_CHAPTER + 1).clamp(1, 5)
}

/// 仅章末（level % 12 == 0）为 Boss 关。
pub fn boss_for_level(level: u32) -> Option<BossId> {
    if level == 0 || !level.is_multiple_of(LEVELS_PER_CHAPTER) {
        return None;
    }
    BossId::all().get((level / LEVELS_PER_CHAPTER - 1) as usize).copied()
}

pub fn is_mainline_end(level: u32) -> bool {
    level >= MAINLINE_LEVELS
}

// ---------- 脚本执行 ----------

/// 当前回合的脚本段：intro 用尽后 cycle 取模回绕。第二项是日志用的段标签。
pub fn turn_actions(p: &'static BossProfile, turn: i64) -> (&'static BossTurn, String) {
    let t = (turn - 1).max(0) as usize;
    if t < p.intro.len() {
        (&p.intro[t], format!("intro{}", t + 1))
    } else {
        let j = (t - p.intro.len()) % p.cycle.len();
        (&p.cycle[j], format!("cycle{}/{}", j + 1, p.cycle.len()))
    }
}

/// 脚本放置/献祭的跳过原因（测试按「日志不含 跳过」机检账本）。
type Skip = String;

fn slot_label(row: Row, col: usize) -> String {
    match row {
        Row::Front => format!("E{}", col + 1),
        Row::Back => format!("E后{}", col + 1),
    }
}

fn enemy_occupied(b: &Battle, row: Row, col: usize) -> bool {
    match row {
        Row::Front => b.e_front[col].is_some(),
        Row::Back => b.e_back[col].is_some(),
    }
}

/// 解析目标格 → (row, col)。前排挤压一律走 `Slot::Front`（仅暗渡放行已占用格）。
fn resolve_slot(b: &Battle, slot: Slot) -> Result<(Row, usize), Skip> {
    match slot {
        Slot::Back(col) => {
            if enemy_occupied(b, Row::Back, col) {
                return Err(format!("格位占用 {}", slot_label(Row::Back, col)));
            }
            Ok((Row::Back, col))
        }
        Slot::Front(col) => {
            if enemy_occupied(b, Row::Front, col) && b.boss_rule() != BossRule::ShadowPush {
                return Err(format!("格位占用 {}", slot_label(Row::Front, col)));
            }
            Ok((Row::Front, col))
        }
        Slot::AnyBackIn(z) => (0..4)
            .find(|c| z.has(*c) && b.e_back[*c].is_none())
            .map(|c| (Row::Back, c))
            .ok_or_else(|| format!("无空位 后排{}", z.label())),
    }
}

/// 献祭目标：该名的非开端卡，先找前排再找后排（列号最小者）。
fn sac_target(b: &Battle, c: &BossCard) -> Option<(Row, usize)> {
    for row in [Row::Front, Row::Back] {
        for col in 0..4 {
            if let Some(x) = b.enemy_slot(row, col)
                && x.def.name == c.name
                && x.def.faction == c.faction
                && !x.is_starter()
            {
                return Some((row, col));
            }
        }
    }
    None
}

/// 脚本卡实例：按名解析 + 脚本声明的技能（不随机附加、不消耗 `b.rng`）。
pub(crate) fn make_script_card(b: &mut Battle, c: BossCard) -> Option<CardInst> {
    let def = card_by_name(c.faction, c.name)?;
    let mut inst = CardInst::new(b.next_id, def);
    b.next_id += 1;
    inst.skills = c.skills.to_vec();
    Some(inst)
}

/// 敌方脚本回合（`battle::enemy_turn` 在 `boss.is_some()` 时调用）。
pub fn run(b: &mut Battle) {
    let Some(id) = b.boss else {
        return;
    };
    let p = id.profile();
    let turn = b.turn;
    let (t, seg) = turn_actions(p, turn);
    for act in t.actions {
        let skip = match *act {
            BossAction::Sac { card } => run_sac(b, &card, turn, &seg),
            BossAction::Place { card, slot } => run_place(b, &card, slot, turn, &seg),
        };
        if let Some(s) = skip {
            b.log.push(format!("【脚本T{turn}·{seg}】跳过·{s}"));
        }
    }
}

/// 标记行先推、失败则**撤回**：账本机检判据是「存在一条不含『跳过』的【脚本T】行」，
/// 若失败时留着标记行，它自身就能满足判言 ⇒ 假绿通道。撤回后失败只剩 `跳过·原因`。
/// （上面的合法性检查与 `enemy_place_inst` 同源，正常不可失败；这条是防两处的闸日后错位。）
fn run_sac(b: &mut Battle, c: &BossCard, turn: i64, seg: &str) -> Option<Skip> {
    let Some((row, col)) = sac_target(b, c) else {
        return Some(format!("无可献祭 {}", c.name));
    };
    let mark = b.log.len();
    b.log.push(format!("【脚本T{turn}·{seg}】献祭 {} → {}", c.name, slot_label(row, col)));
    match b.enemy_sacrifice_field(row, col) {
        Ok(()) => None,
        Err(e) => {
            b.log.remove(mark);
            Some(format!("无可献祭 {}（{e}）", c.name))
        }
    }
}

fn run_place(b: &mut Battle, c: &BossCard, slot: Slot, turn: i64, seg: &str) -> Option<Skip> {
    let Some(def) = card_by_name(c.faction, c.name) else {
        return Some(format!("无名卡 {}", c.name));
    };
    let cost = if def.tr == crate::model::TraitKind::Starter { 0 } else { def.cost };
    if b.ef.sacrificed_names.contains(&def.name) {
        return Some(format!("同名录 {}", def.name));
    }
    if b.e_karma < cost {
        return Some(format!("业力不足 {}（需{cost} 有{}）", def.name, b.e_karma));
    }
    let (row, col) = match resolve_slot(b, slot) {
        Ok(x) => x,
        Err(s) => return Some(s),
    };
    let Some(inst) = make_script_card(b, *c) else {
        return Some(format!("无名卡 {}", c.name));
    };
    let mark = b.log.len();
    b.log.push(format!("【脚本T{turn}·{seg}】放置 {} → {}", def.name, slot_label(row, col)));
    match b.enemy_place_inst(inst, col, row) {
        Ok(()) => None,
        Err(e) => {
            b.log.remove(mark);
            Some(format!("放置失败 {}（{e}）", def.name))
        }
    }
}



// ---------- 特殊规则 hook（battle.rs 调用） ----------

/// 敌方回合开始：炉温（全体在场卡业火+1，立即检查阈值）。业力**不发**——脚本自养。
pub(crate) fn on_enemy_turn_start(b: &mut Battle) {
    let Some(p) = b.boss_profile() else { return };
    if p.rule == BossRule::FurnaceHeat {
        let mut n = 0;
        for row in [Row::Front, Row::Back] {
            for col in 0..4 {
                if let Some(c) = b.enemy_slot_mut(row, col) && c.hp > 0 {
                    c.flame += 1;
                    n += 1;
                }
            }
        }
        b.log.push(format!("【炉温】敌方在场 {n} 张卡业火+1（立即检查阈值）"));
        b.check_all_triggers();
    }
}

/// 霜封：我方卡对敌方前排卡的伤害-1（最低0）；开端不受削减（否则 1 点伤害归零 → 不可解）。
pub(crate) fn card_damage_adjust(b: &Battle, side: crate::battle::SideK, atk_is_starter: bool, dmg: i32) -> i32 {
    if side != crate::battle::SideK::Player || b.boss_rule() != BossRule::FrostBrand || atk_is_starter {
        return dmg;
    }
    (dmg - 1).max(0)
}

/// 炎冰同源：按列分区（第3-4列→1号「炎」，第1-2列→2号「冰」；无列信息归 1 号）。
pub(crate) fn holder_index(col: Option<usize>) -> usize {
    match col {
        Some(c) if c < 2 => 1,
        _ => 0,
    }
}

/// 双 Boss 是否两根皆尽（皆尽才算玩家胜）。
pub(crate) fn both_holders_out(b: &Battle) -> bool {
    match b.e_candle2 {
        None => b.e_candle <= 0,
        Some(c2) => b.e_candle <= 0 && c2 <= 0,
    }
}

/// 战斗内 `b` 命令的 Boss 档案：身份 / 规则 / 烛血 / 脚本形状。
/// **不列未来回合的动作**——§二十 只说脚本 Combo 是 Boss 的战术，没说要向玩家公开底牌。
pub fn dossier(b: &Battle) -> String {
    let Some(p) = b.boss_profile() else {
        return "本局无 Boss（普通对局）。想跳打：cargo run -- boss <1-5|luzhu|炉主>".to_string();
    };
    let second = p.second_faction.map_or(String::new(), |f| format!(" + {}", f.name()));
    let candles = match (p.holder_names[1], b.e_candle2) {
        ("", None) => format!("{} 🕯️{}/{}", p.holder_names[0], b.e_candle.max(0), p.holder_hp),
        (n2, Some(c2)) => format!(
            "{} 🕯️{}/{} ｜ {} 🕯️{}/{}",
            p.holder_names[0],
            b.e_candle.max(0),
            p.holder_hp,
            n2,
            c2.max(0),
            p.holder_hp2.unwrap_or(p.holder_hp)
        ),
        _ => unreachable!("双持业者必有第二根的名字与血量"),
    };
    format!(
        "Boss：{}·{}（第{}章 · 主线第{}关）\n阵营：{}{}\n特殊规则【{}】：{}\n持业者：{}\n脚本：intro {} 回合 + cycle {} 回合回绕；本回合位＝{}\n（脚本内容不显示：那是 Boss 的战术底牌）",
        p.name,
        p.title,
        p.chapter,
        p.chapter * LEVELS_PER_CHAPTER,
        p.faction.name(),
        second,
        p.rule.label(),
        p.rule_text,
        candles,
        p.intro.len(),
        p.cycle.len(),
        turn_actions(p, b.turn).1
    )
}

/// 单场 Boss 冒烟：我方也托管（贪心），打到分出结果；40 回合护栏防脚本死循环。
/// `auto N --boss all|<id>` 逐 Boss 调它（每轮换种子），故此处不再另设 smoke_all。
pub fn smoke_one(seed: u64, faction: Faction, id: BossId) -> (Outcome, i64) {
    let mut b = Battle::new_boss(seed, faction, id, Vec::new(), id.chapter() * LEVELS_PER_CHAPTER);
    let mut guard = 0;
    while b.over.is_none() && guard < 40 {
        crate::meta::auto_turn(&mut b);
        guard += 1;
    }
    (b.over.unwrap_or(Outcome::Draw), b.turn)
}

#[cfg(test)]
mod boss_tests {
    use super::*;
    use crate::battle::{Difficulty, TURN_LIMIT};
    use crate::model::TraitKind;

    #[test]
    fn boss_table_has_five_rows_in_chapter_order() {
        let ids = BossId::all();
        assert_eq!(ids.len(), 5);
        let names: Vec<&str> = ids.iter().map(|i| i.name()).collect();
        assert_eq!(names, vec!["炉主", "雪爵", "影长", "炎与冰", "终影"]);
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(id.profile().chapter, (i + 1) as u32);
        }
        assert_eq!(YANBING.second_faction, Some(Frost));
        assert_eq!(LUZHU.title, "烬火教团首领");
        assert_eq!(XUEJUE.title, "霜誓守卫领袖");
        assert_eq!(YINGZHANG.title, "幽影议会首脑");
        assert_eq!(YANBING.title, "双Boss");
        assert_eq!(ZHONGYING.title, "最终Boss");
    }

    #[test]
    fn boss_for_level_maps_chapter_ends_only() {
        assert_eq!(boss_for_level(12), Some(BossId::Luzhu));
        assert_eq!(boss_for_level(24), Some(BossId::Xuejue));
        assert_eq!(boss_for_level(36), Some(BossId::Yingzhang));
        assert_eq!(boss_for_level(48), Some(BossId::YanBing));
        assert_eq!(boss_for_level(60), Some(BossId::ZhongYing));
        assert_eq!(boss_for_level(0), None);
        assert_eq!(boss_for_level(11), None);
        assert_eq!(boss_for_level(13), None);
        assert_eq!(chapter_of(1), 1);
        assert_eq!(chapter_of(60), 5);
        assert!(is_mainline_end(60));
        assert!(!is_mainline_end(59));
    }

    #[test]
    fn boss_script_card_names_all_resolve() {
        let pool = Skill::list();
        for id in BossId::all() {
            let p = id.profile();
            for t in p.intro.iter().chain(p.cycle) {
                for act in t.actions {
                    if let BossAction::Place { card, .. } = *act {
                        let def = card_by_name(card.faction, card.name)
                            .unwrap_or_else(|| panic!("{} 脚本卡名解析失败：{}", p.name, card.name));
                        assert_eq!(def.name, card.name);
                        for s in card.skills {
                            assert!(pool.contains(s), "脚本技能非法：{card:?} {s:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn boss_turn_actions_cycles_after_intro() {
        let p = BossId::Luzhu.profile(); // intro=3 cycle=6
        assert!(matches!(turn_actions(p, 1).0.actions[0], BossAction::Place { card, .. } if card.name == "开端"));
        assert!(std::ptr::eq(turn_actions(p, 3).0, &p.intro[2]));
        assert!(std::ptr::eq(turn_actions(p, 4).0, &p.cycle[0]));
        assert!(std::ptr::eq(turn_actions(p, 9).0, &p.cycle[5]));
        assert!(std::ptr::eq(turn_actions(p, 10).0, &p.cycle[0]));
        assert_eq!(turn_actions(p, 4).1, "cycle1/6");
    }

    /// 账本机检（沙包侧钉死蜡烛，只问"脚本能否自养 12 回合不跳过"）。
    fn sandbox(id: BossId, turns: i64) -> Battle {
        let mut b = Battle::new_boss(2026, Faction::Ember, id, Vec::new(), id.chapter() * LEVELS_PER_CHAPTER);
        b.p_candle = 100_000; // 被动沙包：不让脚本因提前获胜而中断
        for _ in 0..turns {
            if b.over.is_some() {
                break;
            }
            b.end_player_turn();
        }
        b
    }

    #[test]
    fn boss_scripts_run_12_turns_without_skip() {
        for id in BossId::all() {
            let b = sandbox(id, 12);
            let skipped: Vec<&String> = b.log.iter().filter(|l| l.contains("跳过")).collect();
            assert!(skipped.is_empty(), "{} 脚本账本跳过程：{:#?}", id.name(), skipped);
            for t in 1..=12 {
                assert!(
                    b.log.iter().any(|l| l.starts_with(&format!("【脚本T{t}·"))),
                    "{} 第{t}回合无脚本执行行",
                    id.name()
                );
            }
            assert!(b.e_karma >= 0, "{} 业力为负", id.name());
        }
    }

    /// 账本自养证明（裁定20 的实测口径）：烧自己的牌就够，不需要"每回合发钱"。
    #[test]
    fn boss_ledger_is_self_funding_over_30_turns() {
        for id in BossId::all() {
            let b = sandbox(id, TURN_LIMIT);
            let skipped: Vec<&String> = b.log.iter().filter(|l| l.contains("跳过")).collect();
            assert!(skipped.is_empty(), "{} 30 回合跳过程：{:#?}", id.name(), skipped);
            for t in 1..=TURN_LIMIT {
                assert!(
                    b.log
                        .iter()
                        .any(|l| l.starts_with(&format!("【脚本T{t}·")) && !l.contains("跳过")),
                    "{} 第{t}回合无成功脚本行",
                    id.name()
                );
            }
            assert!(b.over.is_some(), "{} 未分出结果（脚本死循环）", id.name());
            assert!(b.e_karma >= 0, "{} 业力为负", id.name());
        }
    }

    #[test]
    fn boss_battle_terminates_within_30_turns() {
        for id in BossId::all() {
            let b = sandbox(id, TURN_LIMIT + 1);
            assert!(b.over.is_some(), "{} 未在 30 回合内结束", id.name());
            assert!(b.turn <= TURN_LIMIT, "{} 越过回合上限仍继续：turn={}", id.name(), b.turn);
        }
    }

    #[test]
    fn boss_normal_battle_has_no_boss_state() {
        let b = Battle::new(7, Ember, Frost, Difficulty::Normal, Vec::new(), 1);
        assert!(b.boss.is_none());
        assert_eq!(b.boss_rule(), BossRule::None);
        assert!(b.e_candle2.is_none());
        assert_eq!(b.turn_limit, TURN_LIMIT);
        assert_eq!(b.e_candle, crate::battle::CANDLE_HP);
        assert_eq!(b.holder_names, ["敌方持业者", ""]);
    }

    #[test]
    fn dossier_describes_boss_without_leaking_script() {
        let b = Battle::new_boss(
            2026,
            Faction::Ember,
            BossId::Luzhu,
            Vec::new(),
            BossId::Luzhu.chapter() * LEVELS_PER_CHAPTER,
        );
        let d = dossier(&b);
        assert!(d.contains("Boss：炉主·烬火教团首领（第1章"), "{d}");
        assert!(d.contains("特殊规则【炉温】"));
        assert!(d.contains("炉主 🕯️20/20"));
        assert!(d.contains("intro 3 回合 + cycle 6 回合回绕"), "{d}");
        // 档案只给形状，不给内容：脚本卡名与执行行都不许出现
        assert!(!d.contains("开端") && !d.contains("守夜人"), "泄漏脚本卡名：{d}");
        assert!(!d.contains("【脚本T"), "泄漏脚本执行行：{d}");

        let plain = Battle::new(7, Faction::Ember, Faction::Frost, crate::battle::Difficulty::Normal, Vec::new(), 1);
        assert!(dossier(&plain).contains("本局无 Boss"));
    }

    #[test]
    fn boss_id_parse_accepts_ascii_name_and_number() {
        assert_eq!(BossId::parse("luzhu"), Some(BossId::Luzhu));
        assert_eq!(BossId::parse("炉主"), Some(BossId::Luzhu));
        assert_eq!(BossId::parse("5"), Some(BossId::ZhongYing));
        assert_eq!(BossId::parse("炎与冰"), Some(BossId::YanBing));
        assert_eq!(BossId::parse("nope"), None);
        assert_eq!(BossId::parse("0"), None);
    }

    #[test]
    fn boss_script_place_marker_and_engine_line_are_paired() {
        let mut b = Battle::new_boss(9, Ember, BossId::Luzhu, Vec::new(), 12);
        b.enemy_turn();
        let i = b.log.iter().position(|l| l.starts_with("【脚本T1·intro1】放置 开端")).unwrap();
        assert_eq!(b.log[i + 1], "敌方放置 开端 → E后1", "标记行与 place_side 既有行成对");
    }

    /// 成对性推广到 5 份脚本全程：每条「放置」标记行的**下一行**必须是对应的引擎落子行。
    /// 这条判言封掉了「标记先推、动作后失败」的假绿通道——旧写法下失败会留下孤立标记行，
    /// 账本机检（存在一条不含『跳过』的【脚本T】行）会被它单独满足。
    #[test]
    fn every_script_place_marker_is_followed_by_its_engine_line() {
        for id in BossId::all() {
            let mut b = Battle::new_boss(2026, Ember, id, Vec::new(), id.chapter() * LEVELS_PER_CHAPTER);
            let mut guard = 0;
            while b.over.is_none() && guard < 40 {
                crate::meta::auto_turn(&mut b);
                guard += 1;
            }
            for (i, l) in b.log.iter().enumerate() {
                let Some((_seg, body)) = l.strip_prefix("【脚本T").and_then(|r| r.split_once('】')) else { continue };
                let Some(body) = body.strip_prefix("放置 ") else { continue };
                let name = body.split(' ').next().unwrap_or("");
                let next = b.log.get(i + 1).map(|s| s.as_str()).unwrap_or("<日志结束>");
                assert!(
                    next.starts_with("敌方放置 ") && next.contains(name),
                    "{:?} 第{i}条标记 {:?} 后面不是引擎落子行，而是 {:?}",
                    id.name(),
                    l,
                    next
                );
            }
        }
    }

    #[test]
    fn script_card_injection_uses_declared_skills_only() {
        let mut b = Battle::new_boss(3, Ember, BossId::Luzhu, Vec::new(), 12);
        let c = make_script_card(&mut b, card_sk(Ember, "焚稿人", &[Skill::AtkSameColFlame1])).unwrap();
        assert_eq!(c.skills, vec![Skill::AtkSameColFlame1]);
        assert_eq!(c.def.tr, TraitKind::ThresholdSameColFlame2);
        let st = make_script_card(&mut b, card(Ember, "开端")).unwrap();
        assert!(st.is_starter());
        assert!(make_script_card(&mut b, card(Ember, "不存在的卡")).is_none());
    }
}

