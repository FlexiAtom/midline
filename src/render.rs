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
//!
//! 本模块是快照的**消费者**之一：所有数据来自 `view::Board`，不再直读 `Battle` 字段。
//! 壳层（TUI / 2D / 3D）接的是同一份 `Board`——所以这里出现的任何数字都必须能从快照里拿到，
//! 反过来也不许在这里偷偷回读引擎（那等于把刚拆掉的耦合再装回来）。

use crate::battle::Battle;
use crate::view::{Board, CardView, HolderView};

/// 格内一行（名字·当前值/生效阈值 焰累积）。
fn face(c: &CardView) -> String {
    format!("{:<12}", format!("{}·{}/{}焰{}", c.name, c.hp, c.threshold, c.flame))
}

fn blank() -> String {
    format!("{:<12}", "·")
}

/// 四个同列格：槽位为 None 画空。
fn row_cells(row: &[Option<CardView>; 4]) -> [String; 4] {
    std::array::from_fn(|i| row[i].as_ref().map(face).unwrap_or_else(blank))
}

pub fn candle_bar(hp: i32, cap: i32) -> String {  // §廿三:1013 持业者画成蜡烛，条长按 hp/cap；§十四:599 视觉与玩家持业者对称＝敌我两侧调同一个本函数（我方在 render_board、敌方在 holder_line）
    let hp = hp.max(0);
    let filled = (hp * 10 / cap.max(1)).min(10) as usize;
    format!("🕯️[{:<10}]{}{}/{cap}", "#".repeat(filled), if hp == 0 { "烛尽 " } else { "" }, hp)  // §十四:585 烛尽＝烛火熄灭，这一串是它在渲染层的表现处
}

/// 持业者称呼：普通对局沿用原文案；Boss 战带名（`name` 由引擎给出）。
fn holder_label(name: &str) -> String {
    if name == "敌方持业者" {
        "敌方持业者".to_string()
    } else {
        format!("敌方持业者「{name}」")
    }
}

fn holder_line(h: &HolderView) -> String {
    let karma = h.karma.map_or(String::new(), |k| format!(" 业力:{k}"));
    let zone = h.column_zone.map_or(String::new(), |(a, b)| format!("（第{a}-{b}列直击此柱）"));
    format!("{} {}{karma}{zone}\n", holder_label(h.name), candle_bar(h.hp, h.cap))
}

/// 一行简述（快照版）。它和 `model::short_card`（引擎版）是两份实现，
/// 等值由 view.rs 的 `hand_card_label_equals_the_engines_own_short_card` 钉住。
pub fn card_label(c: &CardView) -> String {
    let mut s = format!("{}{}({}费 值{} 阈{})[{}]", c.name, if c.crafted { "〈造〉" } else { "" }, c.cost, c.hp, c.threshold, c.flame);
    if !c.skills.is_empty() {
        s.push_str(&format!("{{{}}}", c.skills.join("+")));
    }
    s
}

/// 快照 → 文本。CLI 走 `render`，壳层若要"同一段文案"可直接走这个函数（正常情况它消费快照自己画）。
pub fn render_board(v: &Board) -> String {
    let mut s = String::new();
    for h in &v.enemy_holders {
        s.push_str(&holder_line(h));
    }
    if let Some(r) = &v.rule {
        s.push_str(&format!("特殊规则【{}】：{}\n", r.label, r.text));
    }
    let [b0, b1, b2, b3] = row_cells(&v.enemy_back);
    s.push_str(&format!("│ E1 {:<12}│ E2 {:<12}│ E3 {:<12}│ E4 {:<12}│ ← 敌方后排（不可攻击）\n", b0, b1, b2, b3));
    let [f0, f1, f2, f3] = row_cells(&v.enemy_front);
    s.push_str(&format!("│ E5 {:<12}│ E6 {:<12}│ E7 {:<12}│ E8 {:<12}│ ← 敌方前排（可攻击）\n", f0, f1, f2, f3));
    s.push_str("╞══════════════╪══════════════╪══════════════╪══════════════╡ ← 中线（绝对边界）\n");
    let [p0, p1, p2, p3] = row_cells(&v.player_front);
    s.push_str(&format!("│ P1 {:<12}│ P2 {:<12}│ P3 {:<12}│ P4 {:<12}│ ← 我方（全部可攻击）\n", p0, p1, p2, p3));
    s.push_str(&format!(
        "我方持业者 {} 业力:{}（回合 {}）\n",
        candle_bar(v.player.hp, v.player.cap),
        v.player.karma,
        v.player.turn
    ));
    s.push_str("手牌：");
    if v.hand.is_empty() {
        s.push_str("（空）");
    }
    for (i, c) in v.hand.iter().enumerate() {
        s.push_str(&format!("[{i}]{}  ", card_label(c)));
    }
    s.push_str(&format!("\n牌堆：继承堆剩{}张（自动抽）  开端堆{}张\n", v.draw_pile_len, v.starter_pile_len));
    s
}

pub fn render(b: &Battle) -> String {
    render_board(&b.view())
}

pub fn log_tail(b: &Battle, n: usize) -> String {
    b.view().log_tail(n)
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
