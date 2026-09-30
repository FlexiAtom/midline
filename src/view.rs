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
//! 呈现接口 L1：把引擎状态压成一份**纯数据快照**，壳层（TUI → 2D → 3D）只跟这份快照打交道。
//!
//! 边界线（本模块唯一的设计决定）：快照带**玩家可见子集**，不带引擎簿记。
//! 点名排除：`CardId`、`seq`、`placed_turn`、`triggered_turn`、`deaths`。
//! 那些字段一旦进了壳的视野，壳就会去猜哨兵值（`i64::MIN`）的含义，改引擎内部即破壳。
//! 反过来只带渲染当前要用的那几个字段也不够——2D 的悬停面板要 cost / 特性 / 技能 / 升级数，
//! 所以这里是"可见"而不是"已显示"。

use crate::battle::{Battle, CANDLE_HP, SideK};
use crate::model::CardInst;

/// 一张牌的可见面。`threshold` 的口径由构造点决定：在场格＝该格生效阈值，手牌＝基础阈值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardView {
    pub name: &'static str,
    pub cost: i32,
    pub hp: i32,
    pub threshold: i32,
    pub flame: i32,
    pub upgrades: u8,
    /// 融合产物（自造牌）：任何离场永久消失。
    pub crafted: bool,
    pub starter: bool,
    /// 特性名（§十八「特性」列的那个值，"无"＝没有特性）。
    pub trait_label: &'static str,
    /// 技能名列表，可为空（0~N）。
    pub skills: Vec<&'static str>,
}

/// 一个持业者（蜡烛）。`karma`/`column_zone` 为 `None` 表示"这一根不显示该项"——
/// 业力是敌方共用一池故只在第一根上给；受击列区只属于双持业者的第二根。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HolderView {
    pub name: &'static str,
    pub hp: i32,
    pub cap: i32,
    pub karma: Option<i32>,
    pub column_zone: Option<(usize, usize)>,
}

/// 我方持业者。称呼（"我方持业者"）留在渲染层，不进快照——那是文案不是事实。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerView {
    pub hp: i32,
    pub cap: i32,
    pub karma: i32,
    pub turn: i64,
}

/// Boss 特殊规则的可见文本（壳层直接显示，不需要认识 `BossRule` 枚举）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleView {
    pub label: &'static str,
    pub text: &'static str,
}

/// 一帧完整战况。12 个格位固定 4 列（P1-P4 / E1-E8），空格为 `None`。 §廿三:981 我方4×1＋敌方4×2 共用一张 4 列棋盘。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    /// 敌方持业者，1 或 2 根（双持只在炎与冰）。
    pub enemy_holders: Vec<HolderView>,
    pub rule: Option<RuleView>,
    pub enemy_back: [Option<CardView>; 4],
    pub enemy_front: [Option<CardView>; 4],
    pub player_front: [Option<CardView>; 4],
    pub player: PlayerView,
    pub hand: Vec<CardView>,
    /// 继承堆（自动抽）剩余张数。
    pub draw_pile_len: usize,
    /// 开端堆剩余张数。
    pub starter_pile_len: u32,
    /// 引擎日志全文（日志本来就是文本，没有"更原始"的形态可给）。
    pub log: Vec<String>,
}

impl Board {
    pub fn log_tail(&self, n: usize) -> String {
        let start = self.log.len().saturating_sub(n);
        self.log[start..].join("\n")
    }
}

// §七:270 业火条＝显示业火值/阈值的 UI 元素——快照层就是这里把 `flame` 与 `threshold` 两个数一起交出去。
fn card_view(c: &CardInst, threshold: i32) -> CardView {
    CardView {
        name: c.def.name,
        cost: c.def.cost,
        hp: c.hp,
        threshold,
        flame: c.flame,
        upgrades: c.upgrades,
        crafted: c.crafted,
        starter: c.is_starter(),
        trait_label: c.def.tr.label(),
        skills: c.skills.iter().map(|k| k.label()).collect(),
    }
}

impl Battle {
    /// 取本帧可见快照。渲染层改读它，壳层也读它——**同一个上游**，不允许出现第二个真相源。
    pub fn view(&self) -> Board {
        let mut enemy_back: [Option<CardView>; 4] = [None, None, None, None];
        let mut enemy_front: [Option<CardView>; 4] = [None, None, None, None];
        let mut player_front: [Option<CardView>; 4] = [None, None, None, None];
        for col in 0..4 {
            enemy_back[col] = self.e_back[col].as_ref().map(|c| card_view(c, self.effective_threshold(SideK::Enemy, col, c)));
            enemy_front[col] = self.e_front[col].as_ref().map(|c| card_view(c, self.effective_threshold(SideK::Enemy, col, c)));
            player_front[col] = self.p_front[col].as_ref().map(|c| card_view(c, self.effective_threshold(SideK::Player, col, c)));
        }

        let profile = self.boss_profile();
        let cap1 = profile.map_or(CANDLE_HP, |p| p.holder_hp);
        let mut enemy_holders = vec![HolderView { name: self.holder_names[0], hp: self.e_candle, cap: cap1, karma: Some(self.e_karma), column_zone: None }];
        if let Some(hp2) = self.e_candle2 {
            let cap2 = profile.map_or(CANDLE_HP, |p| p.holder_hp2.unwrap_or(CANDLE_HP));
            enemy_holders.push(HolderView { name: self.holder_names[1], hp: hp2, cap: cap2, karma: None, column_zone: Some((1, 2)) });
        }

        Board {
            rule: profile.map(|p| RuleView { label: p.rule.label(), text: p.rule_text }),
            enemy_holders,
            enemy_back,
            enemy_front,
            player_front,
            player: PlayerView { hp: self.p_candle, cap: CANDLE_HP, karma: self.p_karma, turn: self.turn },
            hand: self.hand.iter().map(|c| card_view(c, c.base_threshold())).collect(),
            draw_pile_len: self.draw_pile.len(),
            starter_pile_len: self.starter_pile,
            log: self.log.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::{Difficulty, Row};
    use crate::boss::BossId;
    use crate::model::{Faction, Skill, faction_cards, short_card};

    fn normal() -> Battle {
        Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1)
    }

    fn boss(id: BossId) -> Battle {
        Battle::new_boss(2026, Faction::Ember, id, Vec::new(), id.chapter() * 12)
    }

    /// 快照与引擎各有一份"一行简述"的实现（`short_card` 吃 `CardInst`，`card_label` 吃 `CardView`），
    /// 这就是两个真相源的形状——所以把它钉成等值，谁漂了测试立刻红。
    #[test]
    fn hand_card_label_equals_the_engines_own_short_card() {
        for b in [normal(), boss(BossId::Luzhu), boss(BossId::YanBing)] {
            let v = b.view();
            assert_eq!(v.hand.len(), b.hand.len());
            for (i, c) in b.hand.iter().enumerate() {
                assert_eq!(crate::render::card_label(&v.hand[i]), short_card(c), "手牌 {i} 两份简述漂了");
            }
        }
    }

    /// 在场格用的是**生效**阈值（含同列技能修正），手牌用的是基础阈值；两者不许混。
    #[test]
    fn on_board_threshold_is_the_effective_one() {
        let mut b = normal();
        b.e_karma = 50;
        let held = CardInst::new(900, faction_cards(Faction::Ember)[1]);
        let held_base = held.base_threshold();
        b.enemy_place_inst(held, 0, Row::Front).unwrap();
        assert_eq!(b.view().enemy_front[0].as_ref().unwrap().threshold, held_base, "同列无修正 ⇒ 生效＝基础");

        let mut helper = CardInst::new(901, faction_cards(Faction::Frost)[2]);
        helper.skills.push(Skill::AllyColThreshM1);
        let helper_base = helper.base_threshold();
        b.enemy_place_inst(helper, 0, Row::Back).unwrap();

        let v = b.view();
        assert_eq!(v.enemy_front[0].as_ref().unwrap().threshold, held_base - 1, "同列友方阈值-1 必须进快照");
        assert_eq!(v.enemy_back[0].as_ref().unwrap().threshold, helper_base, "自己的技能不算自己");
        assert_eq!(v.enemy_front[0].as_ref().unwrap().hp, faction_cards(Faction::Ember)[1].power, "快照里的 hp 是实例当前值");
    }

    #[test]
    fn dual_holder_carries_its_column_zone_and_no_karma() {
        let b = boss(BossId::YanBing);
        let v = b.view();
        assert_eq!(v.enemy_holders.len(), 2, "炎与冰是双持业者");
        assert_eq!(v.enemy_holders[0].karma, Some(b.e_karma), "第一根带业力");
        assert_eq!(v.enemy_holders[1].karma, None, "业力共用一池，第二根不重复给");
        assert_eq!(v.enemy_holders[1].hp, b.e_candle2.expect("炎与冰有第二柱"));
        assert_eq!(v.enemy_holders[1].column_zone, Some((1, 2)));
    }

    #[test]
    fn single_holder_battle_has_one_holder_and_a_rule_line() {
        let v = boss(BossId::Luzhu).view();
        assert_eq!(v.enemy_holders.len(), 1);
        assert_eq!(v.enemy_holders[0].name, "炉主");
        assert_eq!(v.rule.as_ref().unwrap().label, "炉温");
        let n = normal().view();
        assert_eq!(n.rule, None, "普通对局没有特殊规则行");
        assert_eq!(n.enemy_holders[0].name, "敌方持业者", "无名持业者是引擎给的称呼，不是渲染层造的");
    }

    /// 边界线：簿记字段不许进快照。判据取"非注释行里不出现这些标识符"——
    /// 注释里点名排除它们是文档义务，不是泄漏。
    #[test]
    fn the_snapshot_source_does_not_leak_engine_bookkeeping() {
        let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/view.rs")).unwrap();
        let banned = ["seq", "placed_turn", "triggered_turn", "deaths", "CardId"];
        let mut offenders: Vec<String> = Vec::new();
        for (i, l) in src.lines().enumerate() {
            let t = l.trim();
            // 本测试自己（含字面量表）从 22 行往后数不划算，直接按测试模块起点整段跳过。
            if t.starts_with("//") || t.contains("mod tests") || t.contains("let banned") {
                continue;
            }
            if banned.iter().any(|b| t.contains(b)) {
                offenders.push(format!("{}: {t}", i + 1));
            }
        }
        assert!(offenders.is_empty(), "view.rs 的非注释行里出现了引擎簿记字段：{offenders:?}");
    }
}
