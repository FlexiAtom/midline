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
//! 关与关之间的牌堆操作（融合 / 升级 / 收尸）——**纯函数层，一个字都不打印**。
//!
//! 为什么单开一个文件：这三条规则原先住在 `meta.rs`，而 `meta.rs` 同时是 CLI 驱动器（stdin、存档落点、
//! 托管模拟）。命令层 `command.rs` 要执行 `fuse/up` 就得回调 `meta`，于是 `meta → session → command → meta`
//! 成环——把壳摘出来时会连 CLI 一起拖走。摘出这三条只依赖 `model` 的操作后，依赖是单向的：
//! `command → progress → model`，`meta` 与将来的壳都在最上层。
//!
//! 返回值即文案：`Ok(String)` 是「✓ 」后面的那句，`Err(String)` 是「✖ 」后面的那句，
//! 前缀由调用方加（与改前的 `meta.rs` 同一分工）。

use crate::battle::Battle;
use crate::model::{CardInst, Skill, short_card};

/// §廿二:946 融合后新牌费用＝主牌费用（副牌只贡献技能，不贡献费用）。
/// §廿二:950 副牌直接消失——**不进弃牌堆**，故也不会有死亡返还、不会再被抽到。
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

/// §十一:402 升级＝每通关一次的奖励；§廿二:965 上限 3 次。
pub fn upgrade_card(inherit: &mut [CardInst], idx: usize, kind: &str) -> Result<String, String> {
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

/// §廿二:947 继承堆上限 10 张，超出弃最早入堆的牌（永久消失）。
/// 幸存者回继承堆（手牌+堆底+场上），上限10，超出弃最早。
/// 返回那些"被上限挤出去"的牌该显示的文案——本层不打印，打印归调用方。
pub fn collect_survivors(b: &mut Battle, inherit: &mut Vec<CardInst>) -> Vec<String> {  // §十二:503 存活牌收进继承堆＝带入下一关
    let mut out = Vec::new();
    let mut survivors = b.battle_survivors();
    survivors.sort_by_key(|c| c.id);
    inherit.append(&mut survivors);
    while inherit.len() > 10 {
        let c = inherit.remove(0);
        out.push(format!("继承堆超10张 → 弃置（永久消失）：{}", short_card(&c)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Difficulty;
    use crate::model::{Faction, Skill, faction_cards};

    fn ember(i: usize) -> CardInst {
        CardInst::new(i as u64 + 1, faction_cards(Faction::Ember)[i])
    }

    fn pile(n: usize) -> Vec<CardInst> {
        (0..n).map(ember).collect()
    }

    #[test]
    fn fuse_moves_skills_keeps_main_and_costs_sub_minus_one() {
        // 主牌 焚稿人 3费、副牌 雷烬 4费（本仓既有的取牌口，与 meta 时代的同一对）
        let mut inherit: Vec<CardInst> = vec![ember(3), ember(8)];
        inherit[1].skills.push(Skill::AtkSelfFlame1);
        let mut karma = 5;
        let msg = fuse_cards(&mut inherit, 0, 1, &mut karma).expect("融合应成功");
        assert!(msg.contains("吸收副牌「雷烬」的1个技能"), "{msg}");
        assert!(msg.ends_with(short_card(&inherit[0]).as_str()), "{msg}");
        assert_eq!(karma, 5 - (4 - 1), "融合消耗=副牌费用-1（§廿二:946）");
        assert_eq!(inherit.len(), 1, "副牌直接消失、不进弃牌堆（§廿二:950）");
        assert_eq!(inherit[0].def.cost, 3, "费用取主牌");
        assert_eq!(inherit[0].skills, vec![Skill::AtkSelfFlame1]);
        assert!(inherit[0].crafted, "融合产物转自造（§十372）");
    }

    #[test]
    fn starter_cannot_fuse_and_refusal_touches_nothing() {
        let mut inherit = vec![CardInst::new(1, crate::model::STARTER), ember(1)];
        let mut k = 9;
        for (m, s) in [(0, 1), (1, 0), (1, 9), (1, 1)] {
            assert!(fuse_cards(&mut inherit, m, s, &mut k).is_err(), "({m},{s}) 该被拒");
        }
        assert_eq!(inherit.len(), 2, "被拒的融合不动堆");
        assert_eq!(k, 9, "被拒的融合不扣业力");
    }

    #[test]
    fn upgrade_three_kind_and_cap() {
        let mut inherit = pile(1);
        assert!(upgrade_card(&mut inherit, 0, "power").unwrap().starts_with("升级后："));
        assert!(upgrade_card(&mut inherit, 0, "thr").is_ok());
        assert!(upgrade_card(&mut inherit, 0, "skill").is_ok());
        assert_eq!(inherit[0].upgrades, 3);
        assert_eq!(upgrade_card(&mut inherit, 0, "power").err().unwrap(), "该牌已达升级上限3次");
        assert_eq!(upgrade_card(&mut inherit, 4, "thr").err().unwrap(), "下标无效");
        // 坏 kind 用的是**没升过级**的新牌：上限检查在 kind 检查之前，拿上面那张已升满的牌只会收到上限文案。
        assert_eq!(upgrade_card(&mut pile(1), 0, "pw").err().unwrap(), "power|thr|skill");
    }

    #[test]
    fn survivors_are_capped_at_ten_and_the_squeezed_out_ones_are_reported() {
        // §廿二:947：超上限弃**最早入堆**的那些，且每条都要有可见文案（永久消失，静默＝丢档无凭据）。
        let mut b = Battle::new(11, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        b.draw_pile.clear();
        b.discard_pile.clear();
        b.p_front = [None, None, None, None];
        b.hand = vec![ember(2), ember(4)];
        let mut inherit = pile(10);
        let lines = collect_survivors(&mut b, &mut inherit);
        assert_eq!(inherit.len(), 10, "上限恒 10");
        assert_eq!(lines.len(), 2, "两张幸存者挤掉两张");
        for l in &lines {
            assert!(l.starts_with("继承堆超10张 → 弃置（永久消失）："), "{l}");
        }
        assert_eq!(lines[0], format!("继承堆超10张 → 弃置（永久消失）：{}", short_card(&ember(0))), "弃的是最早入堆那张");
        assert_eq!(inherit[0].def.name, ember(2).def.name, "堆首前移两位");
    }
}

