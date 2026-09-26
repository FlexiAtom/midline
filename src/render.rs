//! ASCII 棋盘渲染（§二 布局基线）。

use crate::battle::Battle;
use crate::model::short_card;

fn cell(b: &Battle, side_front: bool, col: usize) -> String {
    let c = if side_front { b.p_front[col].as_ref() } else { b.e_front[col].as_ref() };
    match c {
        Some(x) => format!("{:<12}", format!("{}·{}/{}焰{}", x.def.name, x.hp, x.base_threshold(), x.flame)),
        None => format!("{:<12}", "·"),
    }
}

fn ecell(b: &Battle, back: bool, col: usize) -> String {
    let c = if back { b.e_back[col].as_ref() } else { b.e_front[col].as_ref() };
    match c {
        Some(x) => format!("{:<12}", format!("{}·{}/{}焰{}", x.def.name, x.hp, x.base_threshold(), x.flame)),
        None => format!("{:<12}", "·"),
    }
}

pub fn candle_bar(hp: i32) -> String {
    let hp = hp.max(0);
    let filled = (hp * 10 / 20).min(10) as usize;
    format!("🕯️[{:<10}]{}", "#".repeat(filled), if hp == 0 { "烛尽" } else { "" })
}

pub fn render(b: &Battle) -> String {
    let mut s = String::new();
    s.push_str(&format!("敌方持业者 {} 业力:{}\n", candle_bar(b.e_candle), b.e_karma));
    s.push_str(&format!("│ E1 {:<12}│ E2 {:<12}│ E3 {:<12}│ E4 {:<12}│ ← 敌方后排（不可攻击）\n",
        ecell(b, true, 0), ecell(b, true, 1), ecell(b, true, 2), ecell(b, true, 3)));
    s.push_str(&format!("│ E5 {:<12}│ E6 {:<12}│ E7 {:<12}│ E8 {:<12}│ ← 敌方前排（可攻击）\n",
        ecell(b, false, 0), ecell(b, false, 1), ecell(b, false, 2), ecell(b, false, 3)));
    s.push_str("╞══════════════╪══════════════╪══════════════╪══════════════╡ ← 中线（绝对边界）\n");
    s.push_str(&format!("│ P1 {:<12}│ P2 {:<12}│ P3 {:<12}│ P4 {:<12}│ ← 我方（全部可攻击）\n",
        cell(b, true, 0), cell(b, true, 1), cell(b, true, 2), cell(b, true, 3)));
    s.push_str(&format!("我方持业者 {} 业力:{}（回合 {}）\n", candle_bar(b.p_candle), b.p_karma, b.turn));
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
