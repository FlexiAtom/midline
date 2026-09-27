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

pub fn interactive_run(seed: u64) {
    interactive_run_with(seed, Faction::Ember, Difficulty::Normal);
}

pub fn interactive_run_with(seed: u64, faction: Faction, diff: Difficulty) {
    let mut inherit: Vec<CardInst> = Vec::new();
    let mut level = 1u32;
    let mut carry_karma = 0i32;
    loop {
        println!("\n===== 第 {level} 关 · 准备阶段 =====");
        settle_phase(&mut inherit, &mut carry_karma, false);
        let mut b = Battle::new(seed, faction, enemy_faction_for(level), diff, std::mem::take(&mut inherit), level);
        loop {
            print!("\n{}", crate::render::render(&b));
            if let Some(out) = b.over {
                println!("{}", crate::render::log_tail(&b, 12));
                match out {
                    Outcome::PlayerWin => println!("胜：敌方烛尽（或蜡烛优势）。"),
                    Outcome::PlayerLose => println!("败：我方烛尽，人亡。阵亡自造牌永久消失。"),
                    Outcome::Draw => println!("平局（30回合蜡烛判定/双烛尽）。"),
                }
                collect_survivors(&mut b, &mut inherit);
                carry_karma = b.p_karma.max(0);
                match out {
                    Outcome::PlayerWin => {
                        println!("\n===== 第 {level} 关 · 结算阶段 =====（融合/升级/弃置，go 进入下一关）");
                        settle_phase(&mut inherit, &mut carry_karma, true);
                        level += 1;
                        break;
                    }
                    _ => {
                        println!("本局终结。seed={seed} 可复现整局。");
                        return;
                    }
                }
            }
            print!("\n> ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
                println!("输入结束，退出。");
                return;
            }
            execute_player_command(&mut b, &line);
        }
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
        "q" => {
            println!("弃局退出。");
            std::process::exit(0);
        }
        "h" | "help" => {
            println!("p <手牌idx> <P1-P4> 放置 | s <Pn> 场上献祭 | sh <idx> 手牌献祭 | di/ds 抽继承/开端 | e 结束回合 | l 日志 | q 退出");
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

/// 准备/结算阶段命令循环：查看/融合/升级（结算限定）/弃置/go。
fn settle_phase(inherit: &mut Vec<CardInst>, karma: &mut i32, post_battle: bool) {
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
            println!("（可保留业力 {karma}：fuse <主> <副>；up <idx> power|thr|skill；drop <idx>；go）");
        } else {
            println!("（准备阶段：可 fuse/drop 后 go；go 直接开战）");
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
            "up" if parts.len() == 3 => {
                if !post_battle {
                    println!("✖ 升级是通关奖励，仅结算阶段可用");
                    continue;
                }
                match upgrade_card(inherit, parse_idx(parts[1]), parts[2]) {
                    Ok(msg) => println!("✓ {msg}"),
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
    let m = &mut inherit[main];
    let n = s.skills.len();
    for sk in s.skills {
        m.skills.push(sk); // 同名技能叠加
    }
    Ok(format!("{} 吸收副牌「{}」的{n}个技能 → {}", m.def.name, s.def.name, short_card(m)))
}

fn upgrade_card(inherit: &mut Vec<CardInst>, idx: usize, kind: &str) -> Result<String, String> {
    if idx >= inherit.len() {
        return Err("下标无效".into());
    }
    let c = &mut inherit[idx];
    if c.upgrades >= 3 {
        return Err("该牌已达升级上限3次".into());
    }
    match kind {
        "power" => c.def.power += 1,
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
pub fn auto_battles(n: u32) {
    for i in 0..n {
        let seed = 1000 + i as u64;
        let mut inherit: Vec<CardInst> = Vec::new();
        let mut level = 1u32;
        let mut last = Outcome::Draw;
        for _ in 0..5 {
            let mut b = Battle::new(seed, Faction::Ember, enemy_faction_for(level), Difficulty::Normal, std::mem::take(&mut inherit), level);
            auto_play(&mut b);
            last = b.over.unwrap_or(Outcome::Draw);
            match last {
                Outcome::PlayerWin => {
                    collect_survivors(&mut b, &mut inherit);
                    // 自动融合最强两牌，验证融合路径
                    let (m, s) = if inherit.len() >= 2 { (0, 1usize) } else { break };
                    let mut k = b.p_karma.max(0);
                    let _ = fuse_cards(&mut inherit, m, s, &mut k);
                    level += 1;
                }
                _ => break,
            }
        }
        println!("auto#{i} seed={seed} → {last:?} 到达第{level}关");
    }
}

fn auto_play(b: &mut Battle) {
    let mut guard = 0;
    while b.over.is_none() && guard < 300 {
        guard += 1;
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
}

#[cfg(test)]
mod meta_tests {
    use super::*;
    use crate::model::{faction_cards, Skill};

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
    fn survivors_are_all_undefeated_cards() {
        let mut b = Battle::new(11, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        for i in 0..4u64 {
            let mut c = CardInst::new(50 + i, faction_cards(Faction::Ember)[1]);
            c.seq = 100 + i;
            b.p_front[i as usize] = Some(c);
        }
        let hand_before = b.hand.len();
        let pile_before = b.draw_pile.len();
        let survivors = b.battle_survivors();
        assert_eq!(survivors.len(), 4 + hand_before + pile_before, "剩余 = 场上+手牌+牌堆");
        assert!(b.hand.is_empty() && b.draw_pile.is_empty());
    }
}
