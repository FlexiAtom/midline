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
//! 敌方 AI：简单=随机放置不评估；普通=贪心评分（§二十 决策树+评分函数）。

use crate::battle::{Battle, Difficulty, Row};
use crate::model::{Skill, TraitKind};

pub fn run(b: &mut Battle) {
    match b.difficulty {
        Difficulty::Easy => run_easy(b),
        Difficulty::Normal => run_normal(b),
    }
}

fn run_easy(b: &mut Battle) {
    // 随机放置，不评估：有几率放一张可负担的牌到随机空格
    if b.enemy_hand.is_empty() {
        return;
    }
    let idx = b.rng.below(b.enemy_hand.len());
    let affordable = {
        let c = &b.enemy_hand[idx];
        c.is_starter() || c.def.cost <= b.e_karma
    };
    if !affordable || !b.rng.chance(2, 3) {
        return;
    }
    let empties: Vec<(Row, usize)> = enemy_empty_slots(b);
    if empties.is_empty() {
        return;
    }
    let (row, col) = empties[b.rng.below(empties.len())];
    let _ = b.enemy_place(idx, col, row);
}

fn enemy_empty_slots(b: &Battle) -> Vec<(Row, usize)> {
    let mut back: Vec<(Row, usize)> = Vec::new();
    let mut front: Vec<(Row, usize)> = Vec::new();
    for col in 0..4 {
        if b.e_back[col].is_none() {
            back.push((Row::Back, col));
        }
    }
    if back.is_empty() {
        for col in 0..4 {
            if b.e_front[col].is_none() {
                front.push((Row::Front, col));
            }
        }
    } else {
        // 后排有空位时前排也可用（文档：后排满后方可放前排）——严格遵守：只用后排
    }
    if back.is_empty() {
        front
    } else {
        back
    }
}

/// 列威胁度：该列我方前排数值和（近似承伤压力）。
fn col_threat(b: &Battle, col: usize) -> f64 {
    b.p_front[col].as_ref().filter(|c| c.hp > 0).map(|c| c.hp as f64).unwrap_or(0.0)
}

fn run_normal(b: &mut Battle) {
    // 1. 业力=0 且手牌有开端 → 献祭开端
    if b.e_karma == 0 {
        if let Some(i) = b.enemy_hand.iter().position(|c| c.is_starter()) {
            let _ = b.enemy_sacrifice_hand(i);
        }
    }
    // 2/3. 反复：能放则放最优；业力不足则评估献祭
    loop {
        if let Some(idx) = best_place(b) {
            if let Some((row, col)) = best_slot(b, &b.enemy_hand[idx]) {
                let _ = b.enemy_place(idx, col, row);
                continue;
            }
        }
        if try_sacrifice(b) {
            continue;
        }
        break;
    }
}

fn placement_score(b: &Battle, idx: usize) -> f64 {
    let c = &b.enemy_hand[idx];
    let power = c.hp as f64;
    let tv = trait_value(c.def.tr);
    let sv: f64 = c.skills.iter().map(|s| skill_value(*s)).sum();
    let threat = (0..4).map(|col| col_threat(b, col)).fold(0.0f64, f64::max);
    power * 1.5 + (tv + sv) * 2.0 + threat * 2.0 * 0.25 // 列威胁取当量折减，避免主导
}

fn best_place(b: &Battle) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for (i, c) in b.enemy_hand.iter().enumerate() {
        let cost = if c.is_starter() { 0 } else { c.def.cost };
        if cost > b.e_karma {
            continue;
        }
        if b.ef.sacrificed_names.contains(&c.def.name) {
            continue;
        }
        let sc = placement_score(b, i);
        if best.is_none_or(|(bs, _)| sc > bs) {
            best = Some((sc, i));
        }
    }
    best.map(|(_, i)| i)
}

fn best_slot(b: &Battle, _card: &crate::model::CardInst) -> Option<(Row, usize)> {
    let slots = enemy_empty_slots(b);
    slots
        .into_iter()
        .min_by(|(_, c1), (_, c2)| col_threat(b, *c1).partial_cmp(&col_threat(b, *c2)).unwrap())
}

fn try_sacrifice(b: &mut Battle) -> bool {
    if b.ef.sacrifice_used {
        return false;
    }
    // 献祭得分 = 获得业力×1.5 + 亡语价值×2 - 失去卡牌价值；只为解锁高费放置而献祭
    let candidates: Vec<(f64, Row, usize)> = {
        let mut v = Vec::new();
        for (row, slots) in [(Row::Front, &b.e_front as &[Option<crate::model::CardInst>; 4]), (Row::Back, &b.e_back)] {
            for col in 0..4 {
                if let Some(c) = slots[col].as_ref() {
                    if b.turn - c.placed_turn < 1 {
                        continue;
                    }
                    let gain = if c.is_starter() { 2 } else { c.def.cost };
                    let dying = c.hp <= 2;
                    let dr = if c.def.tr == TraitKind::DeathRattleSameColFlame3 { 1.5 } else { 0.0 };
                    let lost = c.hp as f64 * 1.5 + trait_value(c.def.tr) * 2.0;
                    let sc = gain as f64 * 1.5 + dr * 2.0 - lost + if dying { 2.0 } else { 0.0 };
                    v.push((sc, row, col));
                }
            }
        }
        v
    };
    if let Some((sc, row, col)) = candidates.into_iter().max_by(|a, b2| a.0.partial_cmp(&b2.0).unwrap()) {
        if sc > 0.0 && best_place(b).is_none() {
            return b.enemy_sacrifice_field(row, col).is_ok();
        }
    }
    false
}

fn trait_value(tr: TraitKind) -> f64 {
    tr.ai_value()
}

fn skill_value(s: Skill) -> f64 {
    match s {
        Skill::AtkSelfFlame1 | Skill::AtkSameColFlame1 => 0.5,
        Skill::PlaySameColFlame1 | Skill::PlayAdjColFlame1 | Skill::DeathSameColFlame1 => 0.4,
        Skill::AtkAdjColFlame1 => 0.5,
        Skill::AllyColAtk1 => 0.7,
        Skill::EnemyColAtkM1 => 0.8,
        Skill::AllyColDmgTakenM1 => 1.0,
        Skill::EnemyColDmgTakenP1 => 0.8,
        Skill::AllyColThreshM1 => 0.6,
        Skill::EnemyColThreshP1 => 0.6,
    }
}
