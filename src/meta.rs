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
use crate::model::{CardInst, Faction, Skill, short_card};

pub fn daily_seed() -> u64 {
    let now = std::time::SystemTime::now()
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

/// 一局的可变成状态：跨关继承堆 + 结转业力 + 后面还有没有关。
/// `has_next=false` ＝单关跳打（`boss <id>`）：胜了也没有下一关可走。
struct RunState {
    inherit: Vec<CardInst>,
    carry_karma: i32,
    has_next: bool,
}

/// 主线 60 关＝5 章，章末 Boss。通关/终结即返回；**存档不在本包**（progress.kv 归 meta-main）。
pub fn mainline_run(seed: u64, faction: Faction, diff: Difficulty) {
    let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: true };
    let mut level = 1u32;
    loop {
        let out = one_level(seed, faction, diff, level, &mut st, encounter_for);
        match out {
            Outcome::PlayerWin if crate::boss::is_mainline_end(level) => {
                println!("终影已灭 —— 主线通关（60/60）。seed={seed} 可复现整局。");
                return;
            }
            Outcome::PlayerWin => {
                if level.is_multiple_of(crate::boss::LEVELS_PER_CHAPTER) {
                    let ch = crate::boss::chapter_of(level);
                    println!("第{ch}章通关（Boss「{}」已灭）→ 第{}章解锁。", crate::boss::BossId::all()[ch as usize - 1].name(), ch + 1);
                }
                level += 1;
            }
            _ => {
                println!("本局终结于第{level}关。seed={seed} 可复现整局。");
                return;
            }
        }
    }
}

pub fn interactive_run(seed: u64) {
    interactive_run_with(seed, Faction::Ember, Difficulty::Normal);
}

pub fn interactive_run_with(seed: u64, faction: Faction, diff: Difficulty) {
    let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: true };
    let mut level = 1u32;
    loop {
        let out = one_level(seed, faction, diff, level, &mut st, |l| {
            (enemy_faction_for(l), None)
        });
        match out {
            Outcome::PlayerWin => level += 1,
            _ => {
                println!("本局终结。seed={seed} 可复现整局。");
                return;
            }
        }
    }
}

/// `boss <id>` / `play --boss <id>`：单关跳打某章末 Boss。
/// 主线 60 关的挂载与存档（`mainline_level`/`boss_down`/`progress.kv`）属 meta-main 包，此处不建第二套真值。
pub fn boss_run(seed: u64, faction: Faction, diff: Difficulty, id: crate::boss::BossId) {
    use crate::boss::LEVELS_PER_CHAPTER;
    let level = id.chapter() * LEVELS_PER_CHAPTER;
    let mut st = RunState { inherit: Vec::new(), carry_karma: 0, has_next: false };
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

/// 一关的外围：头报 → 准备阶段 → 战斗 → 收尸 →（胜且还有下一关时）结算阶段。返回战斗结果。
/// `enc` 是本关遭遇（普通关＝阵营轮转，章末关＝Boss）；主线与跳打 Boss 共用这一份循环，不开第二套。
fn one_level<F: Fn(u32) -> (Faction, Option<crate::boss::BossId>)>(
    seed: u64,
    faction: Faction,
    diff: Difficulty,
    level: u32,
    st: &mut RunState,
    enc: F,
) -> Outcome {
    let has_next = st.has_next;
    let inherit = &mut st.inherit;
    let carry_karma = &mut st.carry_karma;
    let (foe, boss) = enc(level);
    let head = level_head(level, boss);
    println!("\n===== {head} · 准备阶段 =====");
    settle_phase(inherit, carry_karma, false);
    let mut b = match boss {
        Some(id) => Battle::new_boss(seed, faction, id, std::mem::take(inherit), level),
        None => Battle::new(seed, faction, foe, diff, std::mem::take(inherit), level),
    };
    // Boss 关豁免章强化：B1 的贪心全败读数要与改前逐帧可比（裁定24 只补"每章新阵营"的普通关缺口）。
    if boss.is_none() {
        b.apply_chapter_strengthening(level);
    }
    loop {
        print!("\n{}", crate::render::render(&b));
        if let Some(out) = b.over {
            println!("{}", crate::render::log_tail(&b, 12));
            match out {
                Outcome::PlayerWin => println!("胜：敌方烛尽（或蜡烛优势）。"),
                Outcome::PlayerLose => println!(
                    "败：我方烛尽，人亡。{}",
                    if has_next { "阵亡自造牌永久消失。" } else { "本局到此为止。" }
                ),
                Outcome::Draw => println!("平局（30回合蜡烛判定/双烛尽）。"),
            }
            if has_next {
                collect_survivors(&mut b, inherit);
                *carry_karma = b.p_karma.max(0);
                if out == Outcome::PlayerWin {
                    println!("\n===== {head} · 结算阶段 =====（融合/升级/弃置，go 进入下一关）");
                    settle_phase(inherit, carry_karma, true);
                }
            }
            return out;
        }
        print!("\n> ");
        use std::io::Write;
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            println!("输入结束，退出。");
            std::process::exit(0);
        }
        execute_player_command(&mut b, &line);
    }
}

fn execute_player_command(b: &mut Battle, line: &str) {
    let parts: Vec<&str> = line.trim().split_whitespace().collect();
    if parts.is_empty() {
        return;
    }
    let r = match parts[0] {
        "p" if parts.len() == 3 => b.player_place(parts[1].parse().unwrap_or(99), parse_slot(parts[2])),
        "s" if parts.len() == 2 => b.player_sacrifice_field(parse_slot(parts[1])),
        "sh" if parts.len() == 2 => b.player_sacrifice_hand(parts[1].parse().unwrap_or(99)),
        "di" => b.action_draw(false),
        "ds" => b.action_draw(true),
        "e" => {
            b.end_player_turn();
            Ok(())
        }
        "l" => {
            println!("{}", crate::render::log_tail(b, 40));
            Ok(())
        }
        "b" => {
            println!("{}", crate::boss::dossier(b));
            Ok(())
        }
        "q" => {
            println!("弃局退出。");
            std::process::exit(0);
        }
        "h" | "help" => {
            println!("p <手牌idx> <P1-P4> 放置 | s <Pn> 场上献祭 | sh <idx> 手牌献祭 | di/ds 抽继承/开端 | e 结束回合 | l 日志 | b Boss档案 | q 退出");
            Ok(())
        }
        _ => Err(format!("未知命令：{}（h 看帮助）", parts[0])),
    };
    if let Err(e) = r {
        println!("✖ {e}");
    }
}

fn parse_slot(s: &str) -> usize {
    s.chars()
        .next_back()
        .and_then(|c| c.to_digit(10))
        .map(|d| (d.saturating_sub(1)) as usize)
        .unwrap_or(9)
}

/// 幸存者回继承堆（手牌+堆底+场上），上限10，超出弃最早。
fn collect_survivors(b: &mut Battle, inherit: &mut Vec<CardInst>) {
    let mut survivors = b.battle_survivors();
    survivors.sort_by_key(|c| c.id);
    inherit.append(&mut survivors);
    while inherit.len() > 10 {
        let c = inherit.remove(0);
        println!("继承堆超10张 → 弃置（永久消失）：{}", short_card(&c));
    }
}

/// 准备/结算阶段命令循环：查看/融合/升级（结算限定，每通关限1张）/弃置/排序/go。
fn settle_phase(inherit: &mut Vec<CardInst>, karma: &mut i32, post_battle: bool) {
    let mut up_used = false;
    loop {
        println!("继承堆（{}张）：", inherit.len());
        for (i, c) in inherit.iter().enumerate() {
            let tr = c.def.tr.label();
            println!(
                "  [{i}] {} | 特性:{tr} | 死过{}次{}",
                short_card(c),
                c.deaths,
                if c.is_starter() { "〈开端·不可融合〉" } else { "" }
            );
        }
        if post_battle {
            println!(
                "（可保留业力 {karma}：fuse <主> <副>；up <idx> power|thr|skill（本结算限1张{}）；drop <idx>；move <从> <到>；go）",
                if up_used { "已用完" } else { "余1" }
            );
        } else {
            println!("（准备阶段：可 fuse/drop/move 后 go；go 直接开战）");
        }
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        match parts.first().copied().unwrap_or("") {
            "go" => return,
            "q" | "quit" | "exit" => std::process::exit(0),
            "fuse" if parts.len() == 3 => match fuse_cards(inherit, parse_idx(parts[1]), parse_idx(parts[2]), karma) {
                Ok(msg) => println!("✓ {msg}"),
                Err(e) => println!("✖ {e}"),
            },
            "drop" if parts.len() == 2 => {
                let i = parse_idx(parts[1]);
                if i < inherit.len() {
                    let c = inherit.remove(i);
                    println!("弃置 {}（进弃牌堆，本关不再使用；自造弃牌永久消失）", short_card(&c));
                } else {
                    println!("✖ 下标无效");
                }
            }
            "move" if parts.len() == 3 => {
                let (from, to) = (parse_idx(parts[1]), parse_idx(parts[2]));
                if from < inherit.len() && to < inherit.len() {
                    let c = inherit.remove(from);
                    inherit.insert(to, c);
                    println!("✓ 排序：第{from}位 → 第{to}位（抽牌从堆顶起）");
                } else {
                    println!("✖ 下标无效");
                }
            }
            "up" if parts.len() == 3 => {
                if !post_battle {
                    println!("✖ 升级是通关奖励，仅结算阶段可用");
                    continue;
                }
                if up_used {
                    println!("✖ 每次通关只能升级1张（§十一:402 / §廿二:965）");
                    continue;
                }
                match upgrade_card(inherit, parse_idx(parts[1]), parts[2]) {
                    Ok(msg) => {
                        up_used = true;
                        println!("✓ {msg}");
                    }
                    Err(e) => println!("✖ {e}"),
                }
            }
            "" => {}
            other => println!("✖ 未知命令：{other}"),
        }
    }
}

fn parse_idx(s: &str) -> usize {
    s.parse().unwrap_or(99)
}

pub fn fuse_cards(inherit: &mut Vec<CardInst>, main: usize, sub: usize, karma: &mut i32) -> Result<String, String> {
    if main >= inherit.len() || sub >= inherit.len() || main == sub {
        return Err("下标无效".into());
    }
    if inherit[main].is_starter() || inherit[sub].is_starter() {
        return Err("开端不可融合（无论在手牌还是继承堆）".into());
    }
    let price = (inherit[sub].def.cost - 1).max(0);
    if *karma < price {
        return Err(format!("业力不足：融合需{price}，当前{karma}"));
    }
    *karma -= price;
    let s = inherit.remove(sub);
    // 先按原下标校正主牌位置：sub 被摘除后，其后的下标整体前移一位
    let m = &mut inherit[if main > sub { main - 1 } else { main }];
    let n = s.skills.len();
    for sk in s.skills {
        m.skills.push(sk); // 同名技能叠加
    }
    m.crafted = true; // 自造牌：任何离场永久消失（§十372）
    Ok(format!("{} 吸收副牌「{}」的{n}个技能 → {}", m.def.name, s.def.name, short_card(m)))
}

pub fn upgrade_card(inherit: &mut Vec<CardInst>, idx: usize, kind: &str) -> Result<String, String> {
    if idx >= inherit.len() {
        return Err("下标无效".into());
    }
    let c = &mut inherit[idx];
    if c.upgrades >= 3 {
        return Err("该牌已达升级上限3次".into());
    }
    match kind {
        "power" => {
            c.def.power += 1;
            c.hp = c.def.power; // 升级即时可见（每关本就重置满格）
        }
        "thr" => c.def.threshold = (c.def.threshold - 1).max(1),
        "skill" => {
            let pool = Skill::list();
            c.skills.push(pool[c.upgrades as usize % pool.len()]);
        }
        _ => return Err("power|thr|skill".into()),
    }
    c.upgrades += 1;
    Ok(format!("升级后：{}", short_card(c)))
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
            let mut b = Battle::new(seed, Faction::Ember, enemy_faction_for(level), diff, std::mem::take(&mut inherit), level);
            // 与 one_level 同一套敌方构造：本循环当前最多到第5关（第1章，强化量 0），加上来是为了
            // 将来把轮数拉长读章节曲线时，托管冒烟与主线玩的不是两种游戏。
            b.apply_chapter_strengthening(level);
            auto_play(&mut b);
            last = b.over.unwrap_or(Outcome::Draw);
            match last {
                Outcome::PlayerWin => {
                    collect_survivors(&mut b, &mut inherit);
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

/// 战斗外（准备/结算阶段）的托管决策落地（§二十「专家含融合决策」；时机按 §九 融合时机表）。
/// 只把 `ai::plan_meta` 的意图回灌既有规则函数执行，AI 不自开一套结算。
/// 非专家档保持既有冒烟行为：固定融合前两牌、不代管升级。
fn apply_meta_plan(inherit: &mut Vec<CardInst>, karma: &mut i32, diff: Difficulty, upgrades_left: u8) {
    if diff != Difficulty::Expert {
        if inherit.len() >= 2 {
            let _ = fuse_cards(inherit, 0, 1, karma);
        }
        return;
    }
    // plan 的下标基于同一快照；先执行不改长度的升级，再执行会摘牌的融合
    let plan = crate::ai::plan_meta(inherit, *karma, upgrades_left);
    let mut run = |m: &crate::ai::MetaMove| match m {
        crate::ai::MetaMove::Upgrade { idx, kind } => match upgrade_card(inherit, *idx, kind) {
            Ok(msg) => println!("AI·升级[{kind}] {msg}"),
            Err(e) => println!("✖ AI·升级 {e}"),
        },
        crate::ai::MetaMove::Fuse { main, sub } => match fuse_cards(inherit, *main, *sub, karma) {
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
    use crate::model::{faction_cards, Skill};

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
    fn fuse_moves_skills_keeps_main_and_costs_sub_minus_one() {
        let mut inherit: Vec<CardInst> = vec![
            CardInst::new(1, faction_cards(Faction::Ember)[3]), // 主牌 焚稿人 3费
            CardInst::new(2, faction_cards(Faction::Ember)[8]), // 副牌 雷烬 4费
        ];
        inherit[1].skills.push(Skill::AtkSelfFlame1);
        let mut karma = 5;
        let msg = fuse_cards(&mut inherit, 0, 1, &mut karma).expect("融合应成功");
        assert!(msg.contains("吸收"));
        assert_eq!(karma, 5 - (4 - 1), "融合消耗=副牌费用-1");
        assert_eq!(inherit.len(), 1, "副牌消失、总数不变");
        assert_eq!(inherit[0].def.cost, 3, "费用取主牌");
        assert_eq!(inherit[0].skills, vec![Skill::AtkSelfFlame1]);
    }

    #[test]
    fn starter_cannot_fuse() {
        let mut inherit = vec![
            CardInst::new(1, crate::model::STARTER),
            CardInst::new(2, faction_cards(Faction::Ember)[1]),
        ];
        let mut k = 9;
        assert!(fuse_cards(&mut inherit, 0, 1, &mut k).is_err());
        assert!(fuse_cards(&mut inherit, 1, 0, &mut k).is_err());
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
        assert!(survivors.iter().all(|c| !c.is_starter()), "§五185：开端不入继承堆");
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

    /// 裁定24 的**读数**（不是断言平衡）：每章首关 × 三个难度档，各打「未强化」与「已强化」两场贪心托管。
    /// 断言只锁"能分出结果、不越回合闸、不 panic"；对照与实验同 seed，读数随
    /// `cargo test -- --nocapture` 打印并回填 pool/chapter-strengthening.md §2。
    #[test]
    fn chapter_ramp_matrix_reaches_a_decision() {
        let mut rows = Vec::new();
        for level in [1u32, 13, 25, 37, 49] {
            for d in [Difficulty::Normal, Difficulty::Hard, Difficulty::Expert] {
                let (foe, boss) = encounter_for(level);
                assert!(boss.is_none(), "第{level}关不该是 Boss 章末");
                for strengthen in [false, true] {
                    let mut b = Battle::new(4242, Faction::Ember, foe, d, Vec::new(), level);
                    if strengthen {
                        b.apply_chapter_strengthening(level);
                    }
                    auto_play(&mut b);
                    let out = b.over.expect("每章首关都必须分出结果（不得卡在 300 步护栏里）");
                    assert!(b.turn <= b.turn_limit, "第{level}关 [{}] 越回合闸：turn={}", d.label(), b.turn);
                    rows.push((level, d.label(), strengthen, out, b.turn));
                }
            }
        }
        println!("裁定24 章强化读数（seed=4242，我方＝贪心托管、空继承堆单关；强化前=对照）：");
        for (level, d, st, out, turn) in rows {
            let ch = crate::boss::chapter_of(level);
            let s = Battle::chapter_strength(level);
            let tag = if st { format!("强化+{s}") } else { "未强化  ".into() };
            println!("  第{level:>2}关（第{ch}章）{d:<3} {tag} → {out:?}，{turn} 回合");
        }
    }
}
