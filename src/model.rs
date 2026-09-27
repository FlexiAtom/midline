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
//! 5. "本回合友方累积+2"＝触发瞬间给全部在场友方 +2 业火（本回合内的+2语义按瞬时结算，
//!    骨架阶段不做回合内伤害放大器）。
//! 6. 超额伤害分配＝按我方攻击顺序轮转：每名存活攻击者至多分配其当前数值点伤害，
//!    目标为其同列敌前排（越线序优先），无卡则击敌方持业者，直至超额耗尽。
//! 7. 献祭/死亡后卡牌实例彻底离场；死亡返还计数挂在实例上，随继承堆跨关保留。
//! 8. 业火触发检查点：业火值每次增加后立即检查（受"每回合每卡最多1次"约束）。
//! 9. 跨关"上一关剩余"＝场上未阵亡 + 手牌 + 牌堆未抽部分（三者皆算剩余）；
//!    开局继承堆不足3张时按文档从基础牌堆补齐（同名卡因此可多实例并存，佐证裁定7的实例独立计数）。

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
    let mut s = format!("{}({}费 值{} 阈{})[{}]", c.def.name, c.def.cost, c.hp, c.base_threshold(), c.flame);
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
