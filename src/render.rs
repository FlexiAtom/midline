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
//! ASCII 棋盘渲染（§二 布局基线）。

use crate::battle::{CANDLE_HP, SideK};
use crate::battle::Battle;
use crate::model::short_card;

fn cell(b: &Battle, side_front: bool, col: usize) -> String {
    let c = if side_front { b.p_front[col].as_ref() } else { b.e_front[col].as_ref() };
    match c {
        Some(x) => {
            let side = if side_front { SideK::Player } else { SideK::Enemy };
            format!(
                "{:<12}",
                format!("{}·{}/{}焰{}", x.def.name, x.hp, b.effective_threshold(side, col, x), x.flame)
            )
        }
        None => format!("{:<12}", "·"),
    }
}

fn ecell(b: &Battle, back: bool, col: usize) -> String {
    let c = if back { b.e_back[col].as_ref() } else { b.e_front[col].as_ref() };
    match c {
        Some(x) => format!(
            "{:<12}",
            format!("{}·{}/{}焰{}", x.def.name, x.hp, b.effective_threshold(SideK::Enemy, col, x), x.flame)
        ),
        None => format!("{:<12}", "·"),
    }
}

pub fn candle_bar(hp: i32, cap: i32) -> String {
    let hp = hp.max(0);
    let filled = (hp * 10 / cap.max(1)).min(10) as usize;
    format!("🕯️[{:<10}]{}{}/{cap}", "#".repeat(filled), if hp == 0 { "烛尽 " } else { "" }, hp)
}

/// 持业者称呼：普通对局沿用原文案；Boss 战带名（`holder_names` 由 profile 提供）。
fn holder_label(name: &str) -> String {
    if name == "敌方持业者" {
        "敌方持业者".to_string()
    } else {
        format!("敌方持业者「{name}」")
    }
}

/// 持业者行（业力是敌方共用一池，只在第一根那行列出）。双烛时第二行列出另一根与其受击列区。
fn holder_lines(b: &Battle) -> Vec<String> {
    let p = b.boss_profile();
    let cap1 = p.map_or(CANDLE_HP, |x| x.holder_hp);
    let mut v = vec![format!(
        "{} {} 业力:{}\n",
        holder_label(b.holder_names[0]),
        candle_bar(b.e_candle, cap1),
        b.e_karma
    )];
    if let Some(c2) = b.e_candle2 {
        v.push(format!(
            "{} {}（第1-2列直击此柱）\n",
            holder_label(b.holder_names[1]),
            candle_bar(c2, p.map_or(CANDLE_HP, |x| x.holder_hp2.unwrap_or(CANDLE_HP)))
        ));
    }
    v
}

pub fn render(b: &Battle) -> String {
    let mut s = String::new();
    for line in holder_lines(b) {
        s.push_str(&line);
    }
    if let Some(p) = b.boss_profile() {
        s.push_str(&format!("特殊规则【{}】：{}\n", p.rule.label(), p.rule_text));
    }
    s.push_str(&format!("│ E1 {:<12}│ E2 {:<12}│ E3 {:<12}│ E4 {:<12}│ ← 敌方后排（不可攻击）\n",
        ecell(b, true, 0), ecell(b, true, 1), ecell(b, true, 2), ecell(b, true, 3)));
    s.push_str(&format!("│ E5 {:<12}│ E6 {:<12}│ E7 {:<12}│ E8 {:<12}│ ← 敌方前排（可攻击）\n",
        ecell(b, false, 0), ecell(b, false, 1), ecell(b, false, 2), ecell(b, false, 3)));
    s.push_str("╞══════════════╪══════════════╪══════════════╪══════════════╡ ← 中线（绝对边界）\n");
    s.push_str(&format!("│ P1 {:<12}│ P2 {:<12}│ P3 {:<12}│ P4 {:<12}│ ← 我方（全部可攻击）\n",
        cell(b, true, 0), cell(b, true, 1), cell(b, true, 2), cell(b, true, 3)));
    s.push_str(&format!(
        "我方持业者 {} 业力:{}（回合 {}）\n",
        candle_bar(b.p_candle, CANDLE_HP),
        b.p_karma,
        b.turn
    ));
    s.push_str("手牌：");
    if b.hand.is_empty() {
        s.push_str("（空）");
    }
    for (i, c) in b.hand.iter().enumerate() {
        s.push_str(&format!("[{i}]{}  ", short_card(c)));
    }
    s.push_str(&format!("\n牌堆：继承堆剩{}张（自动抽）  开端堆{}张\n", b.draw_pile.len(), b.starter_pile));
    s
}

pub fn log_tail(b: &Battle, n: usize) -> String {
    let start = b.log.len().saturating_sub(n);
    b.log[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Difficulty;
    use crate::boss::BossId;
    use crate::model::Faction;

    fn normal() -> Battle {
        Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1)
    }

    fn boss(id: BossId) -> Battle {
        Battle::new_boss(2026, Faction::Ember, id, Vec::new(), id.chapter() * 12)
    }

    #[test]
    fn candle_bar_scales_to_the_holder_cap() {
        assert_eq!(candle_bar(20, 20), "🕯️[##########]20/20");
        assert_eq!(candle_bar(30, 30), "🕯️[##########]30/30", "终影30烛不该被画成溢出");
        assert_eq!(candle_bar(15, 30), "🕯️[#####     ]15/30");
        assert_eq!(candle_bar(0, 20), "🕯️[          ]烛尽 0/20");
    }

    #[test]
    fn ordinary_battle_keeps_the_original_holder_line() {
        let s = render(&normal());
        assert!(s.starts_with("敌方持业者 🕯️[##########]20/20 业力:"), "{s}");
        assert!(!s.contains("特殊规则"), "普通对局不出现 Boss 行");
        assert!(s.contains("我方持业者 🕯️[##########]20/20"));
    }

    #[test]
    fn boss_holder_line_shows_name_rule_and_cap() {
        let s = render(&boss(BossId::Luzhu));
        assert!(s.contains("敌方持业者「炉主」"), "{s}");
        assert!(s.contains("特殊规则【炉温】"));
        let s = render(&boss(BossId::ZhongYing));
        assert!(s.contains("敌方持业者「终影」"));
        assert!(s.contains("30/30"), "终影蜡烛30");
        assert!(s.contains("特殊规则【吞名】"));
    }

    #[test]
    fn twin_boss_renders_two_candles_with_column_split() {
        let s = render(&boss(BossId::YanBing));
        let lines: Vec<&str> = s.lines().collect();
        assert!(lines[0].contains("「炎」") && lines[0].contains("业力:"), "{:?}", lines[0]);
        assert!(lines[1].contains("「冰」") && lines[1].contains("20/20（第1-2列直击此柱）"), "{:?}", lines[1]);
        assert!(lines[2].contains("特殊规则【炎冰同源】"));
    }
}
