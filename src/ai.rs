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
//! 敌方 AI（§二十）：简单=随机放置不评估；普通=贪心评分；困难=2步预判+计算献祭价值；
//! 专家=3步预判+战斗外融合决策（融合时机＝准备/结算阶段，战斗阶段不可融合）。

use crate::battle::{Battle, Difficulty, Outcome, Row, SideK};
use crate::model::{CardInst, Skill, TraitKind};

/// 「步」= 敌方 1 个原子动作 ply（放置/献祭）；深度用尽或选中 Pass 才推进到回合末评估。
/// 文档只写「2步/3步」未定义单位（另一读法＝§十二的四阶段），见 model.rs 裁定14。
fn search_depth(d: Difficulty) -> usize {
    match d {
        Difficulty::Easy | Difficulty::Normal => 0,
        Difficulty::Hard => 2,
        Difficulty::Expert => 3,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SacReason {
    StarterAtZero,          // 条件1 §二十:893
    UnlockCostlyCard,       // 条件2 §二十:894
    DeathRattleImminent,    // 条件3 §二十:895
    ColumnSuppressed,       // 条件4 §二十:896
    FieldSumWeakerThanHand, // 条件5 §二十:897
}

impl SacReason {
    pub const ALL: [SacReason; 5] = [
        SacReason::StarterAtZero,
        SacReason::UnlockCostlyCard,
        SacReason::DeathRattleImminent,
        SacReason::ColumnSuppressed,
        SacReason::FieldSumWeakerThanHand,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SacReason::StarterAtZero => "开局业力0且有开端",
            SacReason::UnlockCostlyCard => "解锁高费卡",
            SacReason::DeathRattleImminent => "亡语卡即将死亡",
            SacReason::ColumnSuppressed => "该列被压制·腾空间",
            SacReason::FieldSumWeakerThanHand => "场上总和劣于手牌",
        }
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

pub type SacFlags = u8;

fn add_flag(f: &mut SacFlags, r: SacReason) {
    *f |= r.bit();
}

pub fn sac_reasons(f: SacFlags) -> Vec<SacReason> {
    SacReason::ALL.into_iter().filter(|r| f & r.bit() != 0).collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiAction {
    Place { hand_idx: usize, col: usize, row: Row },
    SacField { row: Row, col: usize, why: SacFlags },
    SacHand { idx: usize, why: SacFlags },
    Pass,
}

/// §二十 决策树的完整过程日志（常开，无开关）：逐条列出每个候选动作的三类评分。
/// 推进得分（§廿:872）：文档口径是「挤压敌方次数」，只有我方推进会挤压（battle.rs 挤压分支），
/// 敌 AI 侧的对应物是「放后排→回合末 enemy_advance 抬到前排拦截」→ 由叶子推进到回合末后体现（裁定17）。
fn trace_turn(b: &mut Battle) {
    let field = (0..4)
        .filter(|c| slot_any(b, Row::Front, *c).is_some() || slot_any(b, Row::Back, *c).is_some())
        .count();
    let head = format!("AI·[{}] 回合{} 业力{} 手{}张 场{field}张", b.difficulty.label(), b.turn, b.e_karma, b.enemy_hand.len());
    b.log.push(head);
    if b.difficulty == Difficulty::Easy {
        b.log.push("  AI 简单档：随机放置，不评估（§廿「简单」）→ 三类评分不适用".to_string());
        return;
    }
    if b.e_karma == 0 && b.enemy_hand.iter().any(|c| c.is_starter()) {
        b.log.push(format!("  献祭条件1（§廿:893）命中：业力0且手有开端 → 献祭开端 {}", sac_labels(1 << SacReason::StarterAtZero as u8)));
    }
    b.log.push(format!("  放置评分（§廿:870 数值×1.5+特性/技能×2+本列威胁×2）：{}", if best_place(b).is_some() { "候选↓" } else { "无可负担可放牌" }));
    for idx in 0..b.enemy_hand.len() {
        let c = &b.enemy_hand[idx];
        let cost = hand_cost(c);
        if !affordable(b, idx) {
            b.log.push(format!("    手{idx} {} 费{cost} ✗（业力{}不足{}）", c.def.name, b.e_karma,
                if b.ef.sacrificed_names.contains(&c.def.name) { "·同名已祭禁回置" } else { "" }));
            continue;
        }
        b.log.push(format!("    手{idx} {} 费{cost} 基础分={:.2}", c.def.name, card_field_value(c)));
    }
    for (row, rn) in [(Row::Back, "后"), (Row::Front, "前")] {
        for col in 0..4 {
            if !enemy_empty_slots(b).contains(&(row, col)) {
                continue;
            }
            let adv = if row == Row::Back && slot_any(b, Row::Front, col).is_none() { 1.0 } else { 0.0 };
            let contrib = col_threat(b, col) * 2.0;
            let tail = if adv > 0.0 { "（回合末抬前排拦截）" } else { "" };
            b.log.push(format!("    落格 {rn}{}：列威胁={:.2} 贡献={contrib:.2} 推进={adv:.1}{tail}", col + 1, col_threat(b, col)));
        }
    }
    b.log.push("  献祭评分（§廿:871 获得业力×1.5+亡语×2−失去卡牌价值）：".to_string());
    let mut any = false;
    for row in [Row::Front, Row::Back] {
        for col in 0..4 {
            let Some(c) = enemy_slot(b, row, col) else { continue };
            let (sc, fl) = sacrifice_value(b, row, col);
            any = true;
            let name = c.def.name;
            let threat = b.threat_to(SideK::Enemy, row, col);
            b.log.push(format!(
                "    {}{}列 {} 分={sc:.2}（受威胁{threat}/生命{}）{}",
                if row == Row::Front { "前" } else { "后" },
                col + 1,
                name,
                c.hp,
                sac_labels(fl)
            ));
            if !b.enemy_sac_field_allowed(row, col) {
                b.log.push(format!("      ✗ 不可祭：{}", if b.ef.sacrifice_used { "本回合已用献祭额度" } else { "入场不足1回合" }));
            }
        }
    }
    if !any {
        b.log.push("    场上无卡".to_string());
    }
    for idx in 0..b.enemy_hand.len() {
        if !b.enemy_sac_hand_allowed(idx) {
            continue;
        }
        let c = &b.enemy_hand[idx];
        let gain = if c.is_starter() { 2 } else { c.def.cost };
        b.log.push(format!("    手牌献祭候选 手{idx} {} 得业力{gain}", c.def.name));
    }
}

fn sac_labels(fl: SacFlags) -> String {
    let v = sac_reasons(fl);
    if v.is_empty() {
        String::new()
    } else {
        format!("[{}]", v.iter().map(|r| r.label()).collect::<Vec<_>>().join("|"))
    }
}

fn slot_any(b: &Battle, row: Row, col: usize) -> Option<&CardInst> {
    enemy_slot(b, row, col).filter(|c| c.hp > 0)
}

/// 随机放置，不评估（§二十「简单」）。
fn run_easy(b: &mut Battle) {
    if b.enemy_hand.is_empty() {
        return;
    }
    let idx = b.rng.below(b.enemy_hand.len());
    let c = &b.enemy_hand[idx];
    let affordable = c.is_starter() || c.def.cost <= b.e_karma;
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

/// 手牌费用（开端免费，与 battle.rs 放置同口径）。
fn hand_cost(c: &CardInst) -> i32 {
    if c.is_starter() { 0 } else { c.def.cost }
}

fn enemy_slot<'a>(b: &'a Battle, row: Row, col: usize) -> Option<&'a CardInst> {
    match row {
        Row::Front => b.e_front[col].as_ref(),
        Row::Back => b.e_back[col].as_ref(),
    }
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
    }
    if back.is_empty() { front } else { back }
}

/// 列威胁度：该列我方前排数值和（近似承伤压力）。文档未定义该量算法（裁定14）。
fn col_threat(b: &Battle, col: usize) -> f64 {
    b.p_front[col].as_ref().filter(|c| c.hp > 0).map(|c| c.hp as f64).unwrap_or(0.0)
}

/// 卡牌在场价值：数值×1.5 +（特性价值+Σ技能价值）×2。
pub fn card_field_value(c: &CardInst) -> f64 {
    let sv: f64 = c.skills.iter().map(|s| skill_value(*s)).sum();
    c.hp as f64 * 1.5 + (trait_value(c.def.tr) + sv) * 2.0
}

/// 放置得分（§廿:870）＝卡牌价值 + 本列威胁度×2（文档字面：本列，越大越优 → 裁定14）。§十二:457 放置阶段＝评估放置得分后决定是否放置。
fn placement_score_at(b: &Battle, idx: usize, col: usize) -> f64 {
    card_field_value(&b.enemy_hand[idx]) + col_threat(b, col) * 2.0
}

fn affordable(b: &Battle, idx: usize) -> bool {
    let c = &b.enemy_hand[idx];
    hand_cost(c) <= b.e_karma && !b.ef.sacrificed_names.contains(&c.def.name)
}

/// 「有无可负担的牌」——决策树条件2/献祭门控用（卡牌价值最大者）。
fn best_place(b: &Battle) -> Option<usize> {
    (0..b.enemy_hand.len())
        .filter(|&i| affordable(b, i))
        .max_by(|&i1, &i2| card_field_value(&b.enemy_hand[i1]).total_cmp(&card_field_value(&b.enemy_hand[i2])))
}

/// 场上卡牌数值总和（条件5 左式）。
fn field_sum(b: &Battle) -> f64 {
    let mut s = 0.0;
    for col in 0..4 {
        for row in [Row::Front, Row::Back] {
            if let Some(c) = enemy_slot(b, row, col) {
                s += c.hp as f64;
            }
        }
    }
    s
}

/// 条件5 右式：手牌「祭完之后可能放上」的最高价值。
/// 不看当前业力（献祭本身就产出业力），只看同名已禁回置的牌。
fn best_hand_value(b: &Battle) -> f64 {
    b.enemy_hand
        .iter()
        .filter(|c| !c.is_starter() && !b.ef.sacrificed_names.contains(&c.def.name))
        .map(card_field_value)
        .fold(0.0f64, f64::max)
}

/// 亡语价值：特性亡语 1.5（§十九:840）+ 每个 4 号技能 1.0（线性折算，文档未给技能亡语值）。
fn deathrattle_value(c: &CardInst) -> f64 {
    let tr = if c.def.tr == TraitKind::DeathRattleSameColFlame3 { 1.5 } else { 0.0 };
    tr + c.skills.iter().filter(|s| **s == Skill::DeathSameColFlame1).count() as f64
}

/// 该列被压制程度：我方（玩家）该列前排数值和 − 敌方该列前排数值和。
fn col_pressure(b: &Battle, col: usize) -> f64 {
    let mine = enemy_slot(b, Row::Front, col).map(|c| c.hp as f64).unwrap_or(0.0);
    col_threat(b, col) - mine
}

/// 献祭该格后，该列是否还能落子（放置规则：后排有空位时只能放后排）。
fn placeable_after_sac(b: &Battle, row: Row) -> bool {
    match row {
        Row::Back => true,
        Row::Front => (0..4).all(|c| b.e_back[c].is_some()),
    }
}

/// §二十:871 献祭得分＝获得业力×1.5 + 亡语价值×2 − 失去卡牌价值，并标注命中的献祭条件。
/// 文档把亡语同时计入「失去卡牌价值」的特性与独立的亡语×2（重复计价）→ 裁定15 维持字面。
pub fn sacrifice_value(b: &Battle, row: Row, col: usize) -> (f64, SacFlags) {
    let Some(c) = enemy_slot(b, row, col) else { return (0.0, 0) };
    let gain = if c.is_starter() { 2 } else { c.def.cost };
    let lost = card_field_value(c);
    let sc = gain as f64 * 1.5 + deathrattle_value(c) * 2.0 - lost;

    let mut flags: SacFlags = 0;
    let unlocks = b.enemy_hand.iter().any(|h| {
        !h.is_starter()
            && !b.ef.sacrificed_names.contains(&h.def.name)
            && hand_cost(h) > b.e_karma
            && hand_cost(h) <= b.e_karma + gain
            && card_field_value(h) > lost
    });
    if unlocks {
        add_flag(&mut flags, SacReason::UnlockCostlyCard);
    }
    // 条件3 行感知：只有前排能被直接攻击，后排须先推进才挨打（threat_to 对后排恒 0）
    if deathrattle_value(c) > 0.0 && b.threat_to(SideK::Enemy, row, col) >= c.hp {
        add_flag(&mut flags, SacReason::DeathRattleImminent);
    }
    if col_pressure(b, col) > 0.0 && placeable_after_sac(b, row) && unlocks {
        add_flag(&mut flags, SacReason::ColumnSuppressed);
    }
    if field_sum(b) < best_hand_value(b) && lost <= field_sum(b) * 0.5 + 1e-9 {
        add_flag(&mut flags, SacReason::FieldSumWeakerThanHand);
    }
    (sc, flags)
}

/// 贪心（普通档）候选分：直接取 §廿 的动作评分，不做前瞻。
/// 献祭沿用决策树门控「放不出才祭」（条件2/3），场上祭还需 分>0。
fn greedy_key(b: &Battle, a: AiAction) -> Option<f64> {
    match a {
        AiAction::Place { hand_idx, col, .. } => Some(placement_score_at(b, hand_idx, col)),
        AiAction::SacField { row, col, .. } => {
            if best_place(b).is_some() {
                return None;
            }
            let (sc, _) = sacrifice_value(b, row, col);
            if sc > 0.0 { Some(sc) } else { None }
        }
        AiAction::SacHand { idx, why } => {
            if why == 0 || best_place(b).is_some() {
                return None;
            }
            let c = &b.enemy_hand[idx];
            Some(if c.is_starter() { 2 } else { c.def.cost } as f64 * 1.5)
        }
        AiAction::Pass => Some(f64::MIN),
    }
}

/// 决策树第1步／献祭条件1：业力0 且手牌有开端 → 献祭开端（开端手牌献祭不占额度）。
fn starter_sac_at_zero(b: &mut Battle) {
    if b.e_karma == 0
        && let Some(i) = b.enemy_hand.iter().position(|c| c.is_starter())
    {
        let _ = b.enemy_sacrifice_hand(i);
    }
}

/// 动作枚举的确定次序：放置（手牌序×空位序）→ 场上献祭（前排0-3、后排0-3）→ 手牌献祭 → Pass。
/// Pass 末位 + 平局取先枚举者 ⇒ 搜索不会退化成「什么都不做」。
fn legal_actions(b: &Battle) -> Vec<AiAction> {
    let mut v = Vec::new();
    let slots = enemy_empty_slots(b);
    for idx in 0..b.enemy_hand.len() {
        if !affordable(b, idx) {
            continue;
        }
        for &(row, col) in &slots {
            v.push(AiAction::Place { hand_idx: idx, col, row });
        }
    }
    for row in [Row::Front, Row::Back] {
        for col in 0..4 {
            if !b.enemy_sac_field_allowed(row, col) {
                continue;
            }
            let (sc, why) = sacrifice_value(b, row, col);
            if sc <= 0.0 && why == 0 {
                continue;
            }
            v.push(AiAction::SacField { row, col, why });
        }
    }
    for idx in 0..b.enemy_hand.len() {
        if !b.enemy_sac_hand_allowed(idx) {
            continue;
        }
        let c = &b.enemy_hand[idx];
        let gain = if c.is_starter() { 2 } else { c.def.cost };
        let mut why: SacFlags = 0;
        if c.is_starter() && b.e_karma == 0 {
            add_flag(&mut why, SacReason::StarterAtZero);
        }
        if b.enemy_hand.iter().any(|h| {
            !std::ptr::eq(h, c) && hand_cost(h) > b.e_karma && hand_cost(h) <= b.e_karma + gain
        }) {
            add_flag(&mut why, SacReason::UnlockCostlyCard);
        }
        v.push(AiAction::SacHand { idx, why });
    }
    v.push(AiAction::Pass);
    v
}

fn apply_action(b: &mut Battle, a: AiAction) -> bool {
    match a {
        AiAction::Place { hand_idx, col, row } => b.enemy_place(hand_idx, col, row).is_ok(),
        AiAction::SacField { row, col, .. } => b.enemy_sacrifice_field(row, col).is_ok(),
        AiAction::SacHand { idx, .. } => b.enemy_sacrifice_hand(idx).is_ok(),
        AiAction::Pass => true,
    }
}

/// 叶子评估（敌方视角）。权重属工程自定：§二十 只给动作评分式，未给局面评估函数。
fn evaluate(b: &Battle) -> f64 {
    match b.over {
        Some(Outcome::PlayerLose) => return 10_000.0,
        Some(Outcome::PlayerWin) => return -10_000.0,
        Some(Outcome::Draw) => return 0.0,
        None => {}
    }
    let v = 3.0 * (b.p_candle - b.e_candle) as f64;
    let mut mine = 0.0;
    let mut unblocked = 0.0;
    for col in 0..4 {
        for row in [Row::Front, Row::Back] {
            if let Some(c) = enemy_slot(b, row, col).filter(|c| c.hp > 0) {
                mine += card_field_value(c);
            }
        }
        if enemy_slot(b, Row::Front, col).filter(|c| c.hp > 0).is_none() {
            unblocked += col_threat(b, col) * 2.0; // 该列无敌前排拦截 → 我方卡牌可直冲中线
        }
    }
    let theirs: f64 = (0..4)
        .filter_map(|c| b.p_front[c].as_ref())
        .filter(|c| c.hp > 0)
        .map(card_field_value)
        .sum();
    v + 1.5 * mine - 1.5 * theirs - 4.0 * unblocked
}

/// 叶子＝推进到敌方回合末再评估；`left`＝还可展开的敌方动作 ply 数。
/// 中间 ply 不把 Pass 当作分支（不消耗步数却改变树形），Pass 只在根作为兜底。
fn value(b: &Battle, left: usize, nodes: &mut u64) -> f64 {
    if left > 0 {
        let mut best: Option<f64> = None;
        for a in legal_actions(b) {
            if a == AiAction::Pass {
                continue;
            }
            let mut s = b.clone_for_search();
            if !apply_action(&mut s, a) {
                continue;
            }
            *nodes += 1;
            let v = value(&s, left - 1, nodes);
            if best.is_none_or(|bv| v > bv) {
                best = Some(v);
            }
        }
        if let Some(v) = best {
            return v;
        }
    }
    let mut s = b.clone_for_search();
    if s.over.is_none() {
        s.enemy_resolve_turn_end();
    }
    evaluate(&s)
}

/// 根节点：枚举并试算每个候选（在克隆上执行，随后丢弃＝试算后回滚）。
/// `depth==0` 用 §廿 动作评分；`depth>0` 用前瞻叶子值。返回全部候选供日志打印完整过程。
fn candidates(b: &Battle, depth: usize, left: usize, nodes: &mut u64) -> Vec<(f64, String, AiAction)> {
    let mut out = Vec::new();
    for a in legal_actions(b) {
        let desc = action_desc(b, a);
        // §廿 决策树门控（放不出才祭）只约束普通档；困难/专家的献祭条件4/5 正是「放得出也要腾空间」
        let gk = greedy_key(b, a);
        let mut s = b.clone_for_search();
        if !apply_action(&mut s, a) {
            continue;
        }
        *nodes += 1;
        let key = match (depth, gk) {
            (0, Some(k)) => k,
            (0, None) => continue,
            _ => value(&s, left - 1, nodes),
        };
        out.push((key, desc, a));
    }
    out
}

/// 一档到底：普通＝0步贪心；困难/专家＝逐 ply 重规划（落地 1 个动作后以 left-1 再搜）。
/// 平局取先枚举者，Pass 末位 ⇒ 搜索不会退化成「什么都不做」。
pub fn run(b: &mut Battle) {
    trace_turn(b);
    if b.difficulty == Difficulty::Easy {
        run_easy(b);
        return;
    }
    let depth = search_depth(b.difficulty);
    let unit = if depth == 0 { "分" } else { "叶值" };
    let mut left = depth;
    let mut nodes = 0u64;
    starter_sac_at_zero(b);
    loop {
        if b.over.is_some() || (depth > 0 && left == 0) {
            break;
        }
        let scored = candidates(b, depth, left, &mut nodes);
        let mut lines = Vec::with_capacity(scored.len());
        let mut best: Option<(f64, String, AiAction)> = None;
        for (k, desc, a) in &scored {
            lines.push(format!("  候选：{desc} {unit}={k:.2}"));
            if best.as_ref().is_none_or(|(bk, _, _)| *k > *bk) {
                best = Some((*k, desc.clone(), *a));
            }
        }
        b.log.append(&mut lines);
        let Some((k, desc, a)) = best else { break };
        if a == AiAction::Pass {
            let why = if k == f64::MIN { "无可用候选".to_string() } else { format!("{unit}={k:.2}") };
            b.log.push(format!("  采纳：不动作（{why}）→ 敌方本回合行动结束"));
            break;
        }
        if !apply_action(b, a) {
            break;
        }
        let tail = if depth > 0 { format!("，余{}步重规划", left - 1) } else { String::new() };
        b.log.push(format!("  采纳：{desc}（{unit}={k:.2}{tail}）"));
        if depth > 0 {
            left -= 1;
        }
    }
    if depth > 0 {
        b.log.push(format!("AI·[{}] 本回合搜索节点={nodes}", b.difficulty.label()));
    }
}

fn action_desc(b: &Battle, a: AiAction) -> String {
    let one = |c: usize| c + 1; // 日志按玩家口径 1..4（与渲染 P1-P4 一致）
    match a {
        AiAction::Place { hand_idx, col, row } => format!(
            "放置 手{hand_idx} {}→{}{}",
            b.enemy_hand[hand_idx].def.name,
            if row == Row::Front { "前" } else { "后" },
            one(col)
        ),
        AiAction::SacField { row, col, why } => format!(
            "献祭场上 {}{}列{}",
            if row == Row::Front { "前" } else { "后" },
            one(col),
            sac_labels(why)
        ),
        AiAction::SacHand { idx, why } => format!("献祭手牌{idx}{}{}", b.enemy_hand[idx].def.name, sac_labels(why)),
        AiAction::Pass => "不动作".to_string(),
    }
}

/// 困难/专家：逐 ply 重规划（落地 1 个动作后以 depth-1 再搜），Pass 即结束本回合行动。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetaMove {
    Fuse { main: usize, sub: usize },
    Upgrade { idx: usize, kind: &'static str },
}

/// 专家档战斗外决策（§二十「含融合决策」；融合时机＝准备/结算阶段，人裁定 2026-09-28）。
/// 只产出意图，由既有规则函数执行——AI 不自开一套结算，规则合法性单一来源。
pub fn plan_meta(inherit: &[CardInst], karma: i32, upgrades_left: u8) -> Vec<MetaMove> {
    let mut moves = Vec::new();
    let eligible = |i: usize| inherit.get(i).filter(|c| !c.is_starter()).is_some();

    let mut best: Option<(usize, usize, i32)> = None; // (主, 副, 吸收技能数)
    for main in 0..inherit.len() {
        if !eligible(main) {
            continue;
        }
        for sub in 0..inherit.len() {
            if sub == main || !eligible(sub) {
                continue;
            }
            let price = (inherit[sub].def.cost - 1).max(0);
            if price > karma {
                continue;
            }
            let gain = inherit[sub].skills.len() as i32;
            if gain == 0 {
                continue;
            }
            if best.is_none_or(|(_, _, bg)| gain > bg) {
                best = Some((main, sub, gain));
            }
        }
    }
    if let Some((main, sub, _)) = best {
        moves.push(MetaMove::Fuse { main, sub });
    }
    if upgrades_left > 0 {
        let pick = (0..inherit.len())
            .filter(|&i| inherit[i].upgrades < 3)
            .max_by_key(|&i| (inherit[i].hp, std::cmp::Reverse(i)));
        if let Some(i) = pick {
            moves.push(MetaMove::Upgrade { idx: i, kind: "power" });
        }
    }
    moves
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

#[cfg(test)]
mod ai_tests {
    use super::*;
    use crate::meta::enemy_faction_for;
    use crate::model::{CardDef, Faction, STARTER, faction_cards};

    /// 干净战场：清空双方，便于逐条验证评分与决策。
    fn bare(d: Difficulty) -> Battle {
        let mut b = Battle::new(7, Faction::Ember, Faction::Frost, d, Vec::new(), 1);
        b.enemy_hand.clear();
        b.hand.clear();
        b.draw_pile.clear();
        b.discard_pile.clear();
        for i in 0..4 {
            b.e_front[i] = None;
            b.e_back[i] = None;
            b.p_front[i] = None;
        }
        b.ef = Default::default();
        b.log.clear();
        b.turn = 5;
        b.e_karma = 3;
        b
    }

    fn card(id: u64, idx: usize) -> CardInst {
        let mut c = CardInst::new(id, faction_cards(Faction::Ember)[idx]);
        c.skills.clear(); // 让价值可手算
        c
    }

    fn put(b: &mut Battle, row: Row, col: usize, mut c: CardInst) {
        c.placed_turn = b.turn - 1; // 满足「在场≥1回合」
        c.seq = b.seq;
        match row {
            Row::Front => b.e_front[col] = Some(c),
            Row::Back => b.e_back[col] = Some(c),
        }
    }

    fn hits(b: &Battle, row: Row, col: usize) -> Vec<SacReason> {
        sac_reasons(sacrifice_value(b, row, col).1)
    }

    #[test]
    fn ai_turns_never_consume_the_main_rng() {
        for d in [Difficulty::Normal, Difficulty::Hard, Difficulty::Expert] {
            let mut b = bare(d);
            put(&mut b, Row::Front, 1, card(30, 1));
            b.enemy_hand.push(card(31, 8));
            b.enemy_hand.push(card(32, 2));
            let before = b.rng.state();
            run(&mut b);
            assert_eq!(before, b.rng.state(), "{d:?}：搜索/贪心不得推进主 rng（否则种子复现失效）");
        }
    }

    #[test]
    fn ai_cannot_see_hidden_player_cards() {
        let scenario = |hide_in_draw: bool| {
            let mut b = bare(Difficulty::Hard);
            b.enemy_hand.push(card(10, 2));
            put(&mut b, Row::Back, 0, card(11, 1));
            if hide_in_draw {
                b.draw_pile.push(card(12, 8));
            } else {
                b.hand.push(card(12, 8));
            }
            run(&mut b);
            b.log
        };
        assert_eq!(scenario(false), scenario(true), "AI 决策不得读玩家手牌/牌堆（信息公平）");
    }

    #[test]
    fn placement_prefers_the_contested_column() {
        let mut b = bare(Difficulty::Normal);
        b.enemy_hand.push(card(20, 1)); // 火苗 1费
        let mut wall = card(21, 9);
        wall.hp = 6;
        b.p_front[3] = Some(wall); // 第4列受压
        run(&mut b);
        let line = b.log.iter().find(|l| l.contains("采纳：放置")).expect("应有放置采纳");
        assert!(line.contains("后4"), "§廿:870 本列威胁度×2 应偏好受压列：{line}");
    }

    #[test]
    fn greedy_sacrifices_only_when_nothing_can_be_placed() {
        let mut b = bare(Difficulty::Normal);
        b.enemy_hand.push(card(30, 1)); // 放得起
        put(&mut b, Row::Front, 0, card(31, 8)); // 祭它得分很高
        run(&mut b);
        assert!(!b.ef.sacrifice_used, "§廿 决策树：能放就不祭");
        assert!(b.e_front[0].is_some());
    }

    #[test]
    fn hard_enumers_suppressed_column_sacrifice_that_normal_gates_out() {
        let play = |d: Difficulty| {
            let mut b = bare(d);
            b.e_karma = 1;
            for col in 0..4 {
                put(&mut b, Row::Back, col, card(200 + col as u64, 1)); // 后排满 → 只能放前排
            }
            put(&mut b, Row::Front, 0, card(210, 1)); // 1费 值3.0，被同列 6 值压制
            let mut wall = card(211, 9);
            wall.hp = 6;
            b.p_front[0] = Some(wall);
            b.enemy_hand.push(card(212, 1)); // 1费：放得起 → 普通档据此禁止献祭
            b.enemy_hand.push(card(213, 2)); // 2费 值5.5 > 失去 3.0 → 祭完可放上
            run(&mut b);
            b
        };
        let hard = play(Difficulty::Hard);
        let normal = play(Difficulty::Normal);
        let listed = |b: &Battle| b.log.iter().any(|l| l.contains("候选：献祭场上 前1列") && l.contains("该列被压制"));
        assert!(listed(&hard), "§廿:896 条件4 必须进入困难档决策集合：\n{}", hard.log.join("\n"));
        assert!(!listed(&normal), "普通档遵循 §廿 决策树门控（放得出就不祭）：\n{}", normal.log.join("\n"));
    }

    #[test]
    fn sac_condition2_unlocks_a_costlier_hand_card() {
        let mut b = bare(Difficulty::Normal);
        b.e_karma = 2;
        put(&mut b, Row::Front, 0, card(50, 1)); // 1费 → 祭得1业力
        b.enemy_hand.push(card(51, 3)); // 焚稿人 3费，正好差1
        assert!(
            hits(&b, Row::Front, 0).contains(&SacReason::UnlockCostlyCard),
            "§廿:894 条件2 未命中：{}",
            hits(&b, Row::Front, 0).iter().map(|r| r.label()).collect::<Vec<_>>().join(",")
        );
    }

    #[test]
    fn sac_condition3_is_row_aware() {
        let dr = || CardDef {
            name: "亡语测试",
            faction: Faction::Ember,
            cost: 3,
            power: 2,
            threshold: 6,
            tr: TraitKind::DeathRattleSameColFlame3,
        };
        let mut b = bare(Difficulty::Normal);
        let mut back = CardInst::new(60, dr());
        back.skills.clear();
        put(&mut b, Row::Back, 0, back);
        let mut blocker = card(61, 9);
        blocker.hp = 5;
        b.e_front[0] = Some(blocker); // 前排有自己人 → 后排挨不到打
        let mut atk = card(62, 9);
        atk.hp = 9;
        b.p_front[0] = Some(atk);
        assert!(!hits(&b, Row::Back, 0).contains(&SacReason::DeathRattleImminent), "后排不可被直接攻击，不该判即将死亡");
        let mut front = CardInst::new(63, dr());
        front.skills.clear();
        let mut b2 = b.clone();
        b2.e_front[0] = Some(front);
        assert!(hits(&b2, Row::Front, 0).contains(&SacReason::DeathRattleImminent), "§廿:895 同列致死威胁应命中条件3");
    }

    #[test]
    fn sac_condition5_burns_only_the_weakest() {
        let mut b = bare(Difficulty::Normal);
        b.e_karma = 0;
        put(&mut b, Row::Front, 0, card(70, 1)); // 值3.0（最弱）
        let mut strong = card(71, 9);
        strong.hp = 6;
        put(&mut b, Row::Front, 1, strong); // 值≥9.0
        let mut big = card(72, 8);
        big.hp = 8; // 手牌价值 14 > 场上总和 8
        b.enemy_hand.push(big);
        assert!(hits(&b, Row::Front, 0).contains(&SacReason::FieldSumWeakerThanHand), "§廿:897 条件5 应命中最弱卡");
        assert!(!hits(&b, Row::Front, 1).contains(&SacReason::FieldSumWeakerThanHand), "不得误烧唯一输出");
    }

    #[test]
    fn pass_is_enumerated_last_and_only_wins_by_default() {
        let mut b = bare(Difficulty::Normal);
        assert_eq!(legal_actions(&b).pop(), Some(AiAction::Pass), "Pass 必须末位（平局取先枚举者）");
        b.enemy_hand.push(card(80, 1));
        assert!(matches!(legal_actions(&b).first(), Some(AiAction::Place { .. })));
        run(&mut b);
        assert!(b.e_back[0].is_some() || b.e_back[1].is_some(), "有牌可放时不得选择不动作");
    }

    #[test]
    fn empty_side_adopts_no_action() {
        let mut b = bare(Difficulty::Hard);
        run(&mut b);
        assert!(b.log.iter().any(|l| l.contains("采纳：不动作")), "无候选时应明确记录不动作：\n{}", b.log.join("\n"));
    }

    #[test]
    fn trace_shows_all_three_doc_scores() {
        for d in [Difficulty::Normal, Difficulty::Hard, Difficulty::Expert] {
            let mut b = bare(d);
            put(&mut b, Row::Back, 0, card(90, 1));
            b.enemy_hand.push(card(91, 2));
            run(&mut b);
            let all = b.log.join("\n");
            assert!(all.contains("放置评分"), "{d:?} 缺放置评分");
            assert!(all.contains("献祭评分"), "{d:?} 缺献祭评分");
            assert!(all.contains("推进="), "{d:?} 缺推进得分");
            assert!(all.contains("采纳：") || all.contains("不动作"), "{d:?} 缺采纳记录");
        }
    }

    #[test]
    fn back_row_placement_is_credited_with_advancing() {
        let mut b = bare(Difficulty::Normal);
        b.enemy_hand.push(card(100, 1));
        run(&mut b);
        assert!(
            b.log.iter().any(|l| l.contains("落格 后1") && l.contains("推进=1.0")),
            "后排落子应标注会在回合末抬到前排：\n{}",
            b.log.join("\n")
        );
    }

    #[test]
    fn deeper_difficulty_expands_more_nodes() {
        let nodes = |d: Difficulty| {
            let mut b = bare(d);
            put(&mut b, Row::Front, 0, card(110, 1));
            b.enemy_hand.push(card(111, 2));
            b.enemy_hand.push(card(112, 1));
            run(&mut b);
            b.log
                .iter()
                .find_map(|l| l.split('=').next_back().and_then(|s| s.parse::<u64>().ok()))
                .unwrap_or(0)
        };
        let (n, h, e) = (nodes(Difficulty::Normal), nodes(Difficulty::Hard), nodes(Difficulty::Expert));
        assert_eq!(n, 0, "普通档不做前瞻搜索");
        assert!(h > 0 && e > h, "专家档展开节点应多于困难档：normal={n} hard={h} expert={e}");
    }

    #[test]
    fn same_seed_reproduces_identical_ai_log() {
        let play = |d: Difficulty| {
            let mut b = Battle::new(21, Faction::Ember, enemy_faction_for(2), d, Vec::new(), 2);
            for _ in 0..10 {
                if b.over.is_some() {
                    break;
                }
                b.end_player_turn();
            }
            b.log
        };
        assert_eq!(play(Difficulty::Hard), play(Difficulty::Hard));
        assert_eq!(play(Difficulty::Expert), play(Difficulty::Expert));
    }

    #[test]
    fn every_difficulty_finishes_battles_without_panic() {
        for d in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard, Difficulty::Expert] {
            for seed in 0..6u64 {
                let mut b = Battle::new(seed, Faction::Ember, enemy_faction_for(1 + seed as u32 % 3), d, Vec::new(), 1);
                for _ in 0..40 {
                    if b.over.is_some() {
                        break;
                    }
                    b.end_player_turn();
                }
                assert!(b.over.is_some(), "{d:?} seed={seed} 未在40回合内终结");
                assert!(b.turn <= 31, "{d:?} seed={seed} 超过回合上限仍在推进（turn={})", b.turn);
            }
        }
    }

    #[test]
    fn survivor_view_keeps_field_hand_and_piles() {
        let mut b = bare(Difficulty::Expert);
        b.p_front[2] = Some(card(130, 3));
        b.p_front[1] = Some(card(131, 1));
        b.hand.push(card(132, 2));
        b.draw_pile.push(card(133, 8));
        let mut starter = CardInst::new(134, STARTER);
        starter.skills.clear();
        b.p_front[0] = Some(starter);
        let mut v = b.battle_survivors();
        v.sort_by_key(|c| c.id);
        assert_eq!(v.iter().map(|c| c.id).collect::<Vec<_>>(), vec![130, 131, 132, 133], "场上+手牌+牌堆回归，开端不入堆");
    }
}
