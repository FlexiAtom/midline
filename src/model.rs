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
//! 核心类型：卡牌三段模型（定义 / 实例 / 随机技能）、特性与技能枚举、三阵营卡牌表。
//!
//! ## 规则歧义解释清单（文档未穷尽处，本实现的裁定；对局验证后可再议）
//! 1. 开端"死亡获2业力"是特性定额：不随死亡返还递减（递减只作用于"返还=费用"路径）；
//!    主动献祭同样拿 2（全额=定额）。
//! 2. "可攻击相邻列"：同列中线对面无卡时，改打相邻列敌方前排卡（先左后右），
//!    仍无卡才攻击中线（持业者）。
//! 3. 阈值技"对同列+N累积"：作用于攻击者所在列的所有卡（敌我皆算，不含自身）。
//!    "同列友方"仅己方。"相邻列"＝攻击者列的左右两列全部卡（敌我）。
//! 4. "对整列造成3伤"＝对同列所有卡各造成3点伤害（敌我皆算，不含自身；不触及持业者）。
//! 5. "本回合友方累积+2"＝触发回合内的**增益窗口**：该侧每一次"业火获得事件"额外 +2
//!    （攻击后自增、阈值技分发、受击转焰、超额分配皆算），不是触发瞬间的一次性发放。
//! 6. 超额伤害分配＝按我方攻击顺序轮转：每名存活攻击者至多分配其当前数值点伤害，
//!    目标为其同列敌前排（越线序优先），无卡则击敌方持业者，直至超额耗尽。
//! 7. 离场去向（文档未写死去处，由 §三"火苗A第二次死亡→50%"与 §十"自造牌阵亡永久消失"反推闭合）：
//!    基础牌任何离场（自然死亡/越线/献祭/溢出弃置）→ 弃牌堆，本关不再使用，下关并入继承堆，
//!    实例死亡计数随卡跨关保留（这是递减梯队唯一可达路径）；献祭不使死亡计数+1（返还互斥递减）；
//!    开端一切离场不入继承堆（每关固定发放）；自造牌（融合产物 crafted）任何离场永久消失。
//! 8. 业火触发检查点：业火值每次增加后立即检查（受"每回合每卡最多1次"约束）。
//! 9. 跨关"上一关剩余"＝场上未阵亡 + 手牌 + 牌堆未抽 + 弃牌堆（本关已离场的基础牌）；
//!    开局继承堆不足3张时按文档从基础牌堆补齐（同名卡因此可多实例并存，佐证裁定7的实例独立计数）。
//! 10. 手牌献祭的次数豁免**只适用开端**（§四第4条写在"开局献祭开端"块下，人裁定 2026-09-28）：
//!     普通手牌献祭仍占每回合1次额度；"手牌献祭不受在场限制"对一切手牌卡适用。
//! 11. 继承堆顺序有意义（§十"可调整继承堆顺序"）：我方抽牌一律取堆顶；准备阶段 `move` 调序。
//! 12. 技能叠加（§九"融合后技能可叠加"）：被动光环类技能按**出现次数**计层（同名两个＝+2），
//!     特性不叠加（一张卡只有一个特性，计 1 层）。
//! 13. "同列敌方受伤+1"（技能8）**不排除持有者自身**——持有者自己攻击该列敌人时伤害同样 +1；
//!     而"同列友方攻击+1 / 友方阈值-1"类友方增益排除持有者自身（自己不给自己的加成，避免自增强循环）。
//!     后果（待人向设计确认）：我方只有前排4格，"同列友方"类技能（5/7/11）对我方结构性空转，
//!     敌方因前后排同列才可生效。
//! 14. §二十 AI 搜索的**「步」＝敌方 1 个原子动作 ply**（放置 / 场上献祭 / 手牌献祭 / 不动作），
//!     不是 §十二 的一整回合四阶段（另一读法）；难度档 → 深度：简单·普通＝0（贪心），困难＝2，专家＝3，
//!     每落地 1 个 ply 后以 depth-1 **重规划**。同条一并自定的口径（文档未给算法，待人向设计确认）：
//!     「列威胁度」＝该列玩家前排存活卡的生命值；放置得分取文档字面 `卡牌价值 + 本列威胁度×2`
//!     （＝优先去压力大的列守），取代早期按 0.25 折减的猜测式实现。
//! 15. 献祭得分（§廿:871）＝获得业力×1.5 + 亡语价值×2 − 失去卡牌价值：文档把亡语**同时**算进
//!     "失去卡牌价值"里的特性与独立的"亡语×2"，属重复计价；本实现维持字面不修正（要改须由设计定删哪一项）。
//! 16. AI 献祭的**区域＝手牌 + 场上**：5 条献祭条件按语义分派到场两侧（文档未写"只其其一"）——
//!     手牌侧只可能命中条件 1（开局业力0有开端）、2（解锁高费卡）；场上侧命中 2/3/4/5
//!     （3 需行感知：只有前排能被直接攻击；4 需"祭后该列还能落子"；5 用 `场上数值总和 < 手牌可放置最高价值`）。
//!     场上献祭一律要求"得分>0 或命中任一条件"才入枚举；其合法性与额度（每回合1次、在场≥1回合、同名禁回置）
//!     与玩家侧走同一套规则函数，AI 不自开例外。
//!     难度差异：简单/普通另沿用决策树门控「有可负担的牌就不祭」（放不出才祭），困难/专家在合法范围内全枚举、由搜索定夺。
//! 17. 推进得分（§廿:872 `挤压敌方次数×3 − 自身被挤风险×2`）对敌方 AI 的对应物＝**后排落子在回合末经
//!     `enemy_advance` 抬到前排拦截**：故搜索叶子统一推进到回合末后再评估，不另计奖励项；
//!     "挤压敌方"以烛差体现（`（玩家烛−敌方烛）×3`，烧对方烛即挤压），"自身被挤风险"以
//!     无前排拦截的列的威胁度计入负项。
//! 18. §二十"专家档含融合决策"与§十一"融合只发生在准备/结算阶段"的接合：专家 AI 的融合决策**落在战斗外**
//!     （准备·结算阶段），战斗阶段任何一侧都不可融合（人裁定 2026-09-28）。实现上 `ai::plan_meta` 只产出**意图**
//!     （`MetaMove`），回灌 `meta::fuse_cards` / `meta::upgrade_card` 执行，AI 不自第二套结算规则。
//! 19. §二十一"炎与冰 双Boss"文档没有任何棋盘与判定定义（缺口 s21-未列），本实现裁定：**共用同一张 4×2 棋盘**
//!     （不为双 Boss 扩格），中线按列分两段——第1-2列归 2 号「冰」、第3-4列归 1 号「炎」；直击该段即削对应
//!     持业者，同时另一根同步承受 ⌊削减/2⌋（炎冰同源）；**两根皆尽才算玩家胜**；30 回合判定取两根中的
//!     **较长值**（不取和——取和会让"熬到回合终"形同必败）；单根尽时不结束，日志显式标注剩哪一根。
//! 20. Boss 经济（§三"业力不自动恢复"与"Boss 脚本必须有预算"的接合）：脚本卡**直放**（不经敌方手牌/双牌堆、
//!     不吃 rng、技能由脚本显式声明），预算只给一次 `start_karma`（＝intro 花费＋周期最大下凹，实测反推），
//!     **没有**"每回合发钱"——5 份脚本按 FIFO 传送带写成周期自我闭合，30 回合零跳过（见
//!     `boss::boss_tests::boss_ledger_is_self_funding_over_30_turns`）。
//!     代价须如实写清：§三:137 有一行「初始业力 | 0」，`start_karma != 0` **就是**与它相抵的自造豁免，
//!     不是"没违反 §三"。理由：在场≥1回合 + 每回合1次献祭 两条闸使 0 起手无法在第 2 周期前供养任何脚本，
//!     要么给起手预算，要么整层脚本不成立。待人向设计裁决的三个方向：① 承认 Boss 例外并给数；
//!     ② 改 §三 使"初始业力"按侧区分；③ 把脚本降级为"从 0 起手也只放得起 1-2 费"的弱脚本（等于放弃 §廿一 的战术设计）。
//!     另一处后果：脚本直放每次都是新实例 ⇒ `deaths` 恒 0 ⇒ 影长「暗渡」的越线死亡**恒按 100% 返还**，
//!     玩家侧的 100/50/25/10 递减梯队对 Boss 不可达。
//! 21. §二十 难度表把"Boss"与 简单/普通/困难/专家 并列，同一行又写"Boss＝特殊规则"——本实现读作
//!     **遭遇轴**而非难度档：`Battle::boss: Option<BossId>` 是真值，Boss 战的敌方回合由脚本接管（贪心/搜索 AI
//!     不参与），难度档照常可选（决定我方托管与评分口径）。因此不存在"难度＝Boss 但无 Boss"的退化态，
//!     设计件要求的"`Difficulty::Boss` 且 `boss==None` → 退化 `run_normal`"无需实现。
//!     如实补一句边界：Boss 战里 `--difficulty` **实际不改变任何行为**——敌方是脚本，我方交互局是人类，
//!     `auto --boss` 的托管走贪心 `auto_turn` 不读难度档。USAGE 已按此写明，不再暗示它能调 Boss 强度。
//! 22. Boss 战**平局＝未通关**：不解锁下一章、不记 `boss_down`（否则可用平局白嫖章节奖励）。本实现自造条款，
//!     文档 §22"30回合未分胜负→蜡烛长者胜"未区分 Boss 战。
//! 23. **五条 Boss 特殊规则与其全部数值，都是实现自造，文档没有真值可核对**：§廿一 Boss 表只有
//!     「章节 / Boss / 特征」三列（md:1033-1037），"特殊规则"在 md:907、md:1002 各出现一次且一字未定义。
//!     本实现按 特征 名字反推规则，逐条**故意覆盖**下列文档硬规则（待人向设计逐条确认或换掉）：
//!     - 炉温：敌方回合开始全体在场卡业火+1 —— 与 md:650「业火只由攻击造成的伤害触发」相反（非伤害来源发焰）。
//!     - 霜封：我方卡对敌前排伤害-1（开端不削）。口径：减免只作用于**攻击落子那一发**；§十五 超额分配
//!       是同一发已减免伤害的再路由，故不二次减免（另一读法＝每一发命中都减，未采）。
//!     - 暗渡：脚本主动把后排压上已占用前排 —— 覆盖的是 md:847「AI 不会主动将 E5-E8 挤越线（避免自杀）」
//!       这条 **AI 自律**，不覆盖 §十七 挤压机制本身（机制在 md:845「敌方挤压：后排挤前排，越线死亡」里本来就有）。
//!     - 炎冰同源：见裁定19（双烛/列分区/⌊伤害/2⌋ 反弹/皆尽判胜/回合终取较长值，全为自造）。
//!     - 吞名：我方主动献祭改按**当前**死亡返还档位计 —— 与 md:148、速查 md:1088「献祭获得全额费用、
//!       不触发死亡返还递减」相反。不推进档位：推进会把单场惩罚顺着裁定7 的跨关台账变成永久惩罚。
//!     - 终影 持业者 30：与 md:683「敌方持业者初始长度20」相反；后果是 30 回合判对我方结构性不利（须净多打 >10）。
//!     平衡现状（实测，非结论）：贪心托管对 5 份脚本 `auto 5 --boss all` 全败（4-11 回合），
//!     按台账「手感调试先挂起」只登记不擅改。
//! 24. §廿一 主线"每章新阵营"＝三阵营循环 + 强化（人裁定 2026-09-28）。**只落了循环**：`level` 目前仅用于
//!     rng 种子与头报，没有任何按章递增的敌方数值。缺的是数值口径而非挂载点 ⇒ 待人向设计给数（见 pending）。

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Faction {
    Ember,  // 烬火教团
    Frost,  // 霜誓守卫
    Shadow, // 幽影议会
}

impl Faction {
    pub fn name(self) -> &'static str {
        match self {
            Faction::Ember => "烬火教团",
            Faction::Frost => "霜誓守卫",
            Faction::Shadow => "幽影议会",
        }
    }
}

/// 固定特性（不参与融合）。开端为聚合特性（含免费/定额2业力/在场每回合+1业力·每关上限2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraitKind {
    None,
    Starter,
    AttackAdjacent,
    SelfDmgOnAttack,
    AllyColDamageTakenMinus1,
    EnemyColAttackMinus1,
    ThresholdSameColFlame2,
    ThresholdAllyColFlame2,
    DeathRattleSameColFlame3,
    BattleCrySameColFlame2,
    ThresholdAdjColFlame3,
    SelfFlameOnAttack1,
    ThresholdFullColDamage3,
    ThresholdAllyTurnFlame2,
}

/// 随机附加技能（12 种，可融合、可叠加）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skill {
    AtkSelfFlame1,       // 1 攻击后自身+1累积
    AtkSameColFlame1,    // 2 攻击后对同列+1累积
    PlaySameColFlame1,   // 3 放置时对同列+1累积
    DeathSameColFlame1,  // 4 死亡时对同列+1累积
    AllyColAtk1,         // 5 同列友方攻击+1
    EnemyColAtkM1,       // 6 同列敌方攻击-1
    AllyColDmgTakenM1,   // 7 同列友方受伤-1
    EnemyColDmgTakenP1,  // 8 同列敌方受伤+1
    AtkAdjColFlame1,     // 9 攻击后相邻列+1累积
    PlayAdjColFlame1,    // 10 放置时相邻列+1累积
    AllyColThreshM1,     // 11 同列友方阈值-1
    EnemyColThreshP1,    // 12 同列敌方阈值+1
}

impl Skill {
    pub fn list() -> [Skill; 12] {
        [
            Skill::AtkSelfFlame1,
            Skill::AtkSameColFlame1,
            Skill::PlaySameColFlame1,
            Skill::DeathSameColFlame1,
            Skill::AllyColAtk1,
            Skill::EnemyColAtkM1,
            Skill::AllyColDmgTakenM1,
            Skill::EnemyColDmgTakenP1,
            Skill::AtkAdjColFlame1,
            Skill::PlayAdjColFlame1,
            Skill::AllyColThreshM1,
            Skill::EnemyColThreshP1,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Skill::AtkSelfFlame1 => "攻+己焰1",
            Skill::AtkSameColFlame1 => "攻+同焰1",
            Skill::PlaySameColFlame1 => "放+同焰1",
            Skill::DeathSameColFlame1 => "亡+同焰1",
            Skill::AllyColAtk1 => "同友攻+1",
            Skill::EnemyColAtkM1 => "同敌攻-1",
            Skill::AllyColDmgTakenM1 => "同友伤-1",
            Skill::EnemyColDmgTakenP1 => "同敌伤+1",
            Skill::AtkAdjColFlame1 => "攻+邻焰1",
            Skill::PlayAdjColFlame1 => "放+邻焰1",
            Skill::AllyColThreshM1 => "同友阈-1",
            Skill::EnemyColThreshP1 => "同敌阈+1",
        }
    }
}

impl TraitKind {
    pub fn label(self) -> &'static str {
        match self {
            TraitKind::None => "无",
            TraitKind::Starter => "开端",
            TraitKind::AttackAdjacent => "可攻击相邻列",
            TraitKind::SelfDmgOnAttack => "攻击后自身-1",
            TraitKind::AllyColDamageTakenMinus1 => "同列友方受伤-1",
            TraitKind::EnemyColAttackMinus1 => "同列敌方攻击-1",
            TraitKind::ThresholdSameColFlame2 => "阈值:同列+2焰",
            TraitKind::ThresholdAllyColFlame2 => "阈值:同友+2焰",
            TraitKind::DeathRattleSameColFlame3 => "亡语:同列+3焰",
            TraitKind::BattleCrySameColFlame2 => "战吼:同列+2焰",
            TraitKind::ThresholdAdjColFlame3 => "阈值:邻列+3焰",
            TraitKind::SelfFlameOnAttack1 => "攻后自身+1焰",
            TraitKind::ThresholdFullColDamage3 => "阈值:整列3伤",
            TraitKind::ThresholdAllyTurnFlame2 => "阈值:本回合友+2焰",
        }
    }

    /// 评分用特性价值（§19 量化表）。
    pub fn ai_value(self) -> f64 {
        match self {
            TraitKind::None | TraitKind::Starter => 0.0,
            TraitKind::AttackAdjacent => 0.5,
            TraitKind::EnemyColAttackMinus1 => 1.0,
            TraitKind::AllyColDamageTakenMinus1 => 1.5,
            TraitKind::ThresholdSameColFlame2 | TraitKind::ThresholdAllyColFlame2 => 1.0,
            TraitKind::DeathRattleSameColFlame3 => 1.5,
            TraitKind::BattleCrySameColFlame2 => 0.5,
            TraitKind::SelfDmgOnAttack => -0.5,
            _ => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CardDef {
    pub name: &'static str,
    #[allow(dead_code)] // 数据完整性：卡表按阵营归档，读取方在渲染层（后置）
    pub faction: Faction,
    pub cost: i32,
    pub power: i32,
    pub threshold: i32,
    pub tr: TraitKind,
}

pub const STARTER: CardDef = CardDef {
    name: "开端",
    faction: Faction::Ember,
    cost: 0,
    power: 1,
    threshold: 4,
    tr: TraitKind::Starter,
};

/// 卡牌表（§18）。索引 0 为开端（各阵营共用同一开端定义）。
pub fn faction_cards(f: Faction) -> &'static [CardDef] {
    use Faction::*;
    match f {
        Ember => &[
            STARTER,
            CardDef { name: "火苗", faction: Ember, cost: 1, power: 2, threshold: 4, tr: TraitKind::None },
            CardDef { name: "引燃者", faction: Ember, cost: 2, power: 3, threshold: 5, tr: TraitKind::AttackAdjacent },
            CardDef { name: "焚稿人", faction: Ember, cost: 3, power: 4, threshold: 6, tr: TraitKind::ThresholdSameColFlame2 },
            CardDef { name: "余温", faction: Ember, cost: 2, power: 2, threshold: 4, tr: TraitKind::SelfDmgOnAttack },
            CardDef { name: "续炭", faction: Ember, cost: 2, power: 1, threshold: 3, tr: TraitKind::ThresholdAllyColFlame2 },
            CardDef { name: "炉壁", faction: Ember, cost: 3, power: 3, threshold: 8, tr: TraitKind::AllyColDamageTakenMinus1 },
            CardDef { name: "火星", faction: Ember, cost: 2, power: 3, threshold: 3, tr: TraitKind::BattleCrySameColFlame2 },
            CardDef { name: "雷烬", faction: Ember, cost: 4, power: 4, threshold: 7, tr: TraitKind::ThresholdAdjColFlame3 },
            CardDef { name: "燎原", faction: Ember, cost: 3, power: 3, threshold: 5, tr: TraitKind::SelfFlameOnAttack1 },
            CardDef { name: "山火", faction: Ember, cost: 5, power: 6, threshold: 10, tr: TraitKind::ThresholdFullColDamage3 },
            CardDef { name: "守夜人", faction: Ember, cost: 3, power: 2, threshold: 6, tr: TraitKind::ThresholdAllyTurnFlame2 },
            CardDef { name: "回燃", faction: Ember, cost: 4, power: 3, threshold: 7, tr: TraitKind::DeathRattleSameColFlame3 },
        ],
        Frost => &[
            STARTER,
            CardDef { name: "初霜", faction: Frost, cost: 1, power: 2, threshold: 4, tr: TraitKind::None },
            CardDef { name: "望哨", faction: Frost, cost: 2, power: 3, threshold: 5, tr: TraitKind::AttackAdjacent },
            CardDef { name: "霜序", faction: Frost, cost: 3, power: 4, threshold: 6, tr: TraitKind::ThresholdSameColFlame2 },
            CardDef { name: "寒哨", faction: Frost, cost: 2, power: 3, threshold: 4, tr: TraitKind::EnemyColAttackMinus1 },
            CardDef { name: "暖誓", faction: Frost, cost: 2, power: 1, threshold: 3, tr: TraitKind::ThresholdAllyColFlame2 },
            CardDef { name: "壁", faction: Frost, cost: 3, power: 5, threshold: 8, tr: TraitKind::AllyColDamageTakenMinus1 },
            CardDef { name: "冰刺", faction: Frost, cost: 2, power: 3, threshold: 3, tr: TraitKind::BattleCrySameColFlame2 },
            CardDef { name: "极光", faction: Frost, cost: 4, power: 4, threshold: 7, tr: TraitKind::ThresholdAdjColFlame3 },
            CardDef { name: "雪线", faction: Frost, cost: 3, power: 3, threshold: 5, tr: TraitKind::SelfFlameOnAttack1 },
            CardDef { name: "冰川", faction: Frost, cost: 5, power: 8, threshold: 10, tr: TraitKind::ThresholdFullColDamage3 },
            CardDef { name: "冻时", faction: Frost, cost: 3, power: 2, threshold: 6, tr: TraitKind::ThresholdAllyTurnFlame2 },
            CardDef { name: "霜葬", faction: Frost, cost: 4, power: 4, threshold: 7, tr: TraitKind::DeathRattleSameColFlame3 },
        ],
        Shadow => &[
            STARTER,
            CardDef { name: "影仆", faction: Shadow, cost: 1, power: 2, threshold: 3, tr: TraitKind::None },
            CardDef { name: "暗哨", faction: Shadow, cost: 2, power: 2, threshold: 5, tr: TraitKind::AttackAdjacent },
            CardDef { name: "蚀", faction: Shadow, cost: 3, power: 3, threshold: 6, tr: TraitKind::ThresholdSameColFlame2 },
            CardDef { name: "低语", faction: Shadow, cost: 2, power: 2, threshold: 4, tr: TraitKind::EnemyColAttackMinus1 },
            CardDef { name: "余光", faction: Shadow, cost: 2, power: 1, threshold: 3, tr: TraitKind::ThresholdAllyColFlame2 },
            CardDef { name: "渊壁", faction: Shadow, cost: 3, power: 4, threshold: 8, tr: TraitKind::AllyColDamageTakenMinus1 },
            CardDef { name: "无痕", faction: Shadow, cost: 2, power: 3, threshold: 3, tr: TraitKind::BattleCrySameColFlame2 },
            CardDef { name: "幽雷", faction: Shadow, cost: 4, power: 3, threshold: 7, tr: TraitKind::ThresholdAdjColFlame3 },
            CardDef { name: "夜行", faction: Shadow, cost: 3, power: 3, threshold: 5, tr: TraitKind::SelfFlameOnAttack1 },
            CardDef { name: "深壑", faction: Shadow, cost: 5, power: 6, threshold: 10, tr: TraitKind::ThresholdFullColDamage3 },
            CardDef { name: "止时", faction: Shadow, cost: 3, power: 2, threshold: 6, tr: TraitKind::ThresholdAllyTurnFlame2 },
            CardDef { name: "引渡", faction: Shadow, cost: 4, power: 3, threshold: 7, tr: TraitKind::DeathRattleSameColFlame3 },
        ],
    }
}

/// 按名查卡（Boss 脚本解析卡名的唯一入口；开端用 `STARTER` 名查任意阵营表均可命中）。
pub fn card_by_name(f: Faction, name: &str) -> Option<CardDef> {
    faction_cards(f).iter().find(|c| c.name == name).copied()
}

pub type CardId = u64;

/// 场上/手中/继承堆里的卡牌实例。业火、死亡计数、升级数都挂在实例上。
#[derive(Clone, Debug)]
pub struct CardInst {
    pub id: CardId,
    pub def: CardDef,
    pub skills: Vec<Skill>,
    /// 当前数值（= 血量）。每关开始重置满格。
    pub hp: i32,
    pub flame: i32,
    /// 自然死亡次数（用于返还递减：100/50/25/10%）。献祭不走此计数。
    pub deaths: u32,
    pub upgrades: u8,
    /// 入场序（攻击顺序）；不在场时为 0。
    pub seq: u64,
    /// 入场回合（献祭在场时长判定）；不在场时为 i64::MIN。
    pub placed_turn: i64,
    /// 本回合是否已触发过特性（每卡每回合上限 1 次）。
    pub triggered_turn: i64,
    /// 融合产物标记（自造牌）：任何离场永久消失（§十"自造牌阵亡后永久消失"）。
    pub crafted: bool,
}

impl CardInst {
    pub fn new(id: CardId, def: CardDef) -> Self {
        CardInst {
            id,
            def,
            skills: Vec::new(),
            hp: def.power,
            flame: 0,
            deaths: 0,
            upgrades: 0,
            seq: 0,
            placed_turn: i64::MIN,
            triggered_turn: i64::MIN,
            crafted: false,
        }
    }

    pub fn is_starter(&self) -> bool {
        self.def.tr == TraitKind::Starter
    }

    /// 阈值存于实例内嵌定义（升级直接改定义值），此处不再叠加计数。
    pub fn base_threshold(&self) -> i32 {
        self.def.threshold.max(1)
    }
}

/// 卡牌一行简述（渲染用）。
pub fn short_card(c: &CardInst) -> String {
    let mut s = format!("{}{}({}费 值{} 阈{})[{}]", c.def.name, if c.crafted { "〈造〉" } else { "" }, c.def.cost, c.hp, c.base_threshold(), c.flame);
    if !c.skills.is_empty() {
        s.push_str(&format!("{{{}}}", c.skills.iter().map(|k| k.label()).collect::<Vec<_>>().join("+")));
    }
    s
}

impl fmt::Display for CardInst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", short_card(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_tables_have_13_each() {
        for faction in [Faction::Ember, Faction::Frost, Faction::Shadow] {
            assert_eq!(faction_cards(faction).len(), 13, "{faction:?}");
        }
    }

    #[test]
    fn skill_pool_has_12() {
        assert_eq!(Skill::list().len(), 12);
    }
}
