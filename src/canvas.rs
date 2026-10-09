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
//! 2D 帧编码器：把一份 `Board` 快照画成一张**看得见的** SVG（纯色矩形＋文字，本仓第一件文件形状的呈现物）。
//!
//! 形状声明：它只画**静态一帧**——没有交互、没有动画、没有音效。存在的理由不是"做个好看界面"，
//! 而是给设计交一件**共有的实物**：同一句文字描述有两个读者就有两份想象，一张图只有一份。
//!
//! 上游只有一条：所有数都从 `view::Board` 拿，一个引擎字段都不直读——口径与 `render`／`tui` 相同，不开第二真相源。
//! 列数不写死在版式里，而是从快照的数组长度取（`enemy_back.len()`）：那一维本来就是盘面形状，
//! 在编码器里再钉一次，就成了"引擎改了棋盘列数、图还在画四列"这种最难看的失真。
//!
//! 规则锚：本模块**不写**文档锚点，理由与 `tui` 那条相同——排布不新增任何规则真值，写锚会去动反向覆盖尺的生产面读数。
//! 版式里几句像规则的措辞（后排不可攻击、中线不可越过）**不是**这里定的，是照抄 `render` 既有文案。

use crate::view::{Board, CardView};

const MARGIN: i32 = 48;
const CELL_H: i32 = 104;
const HAND_H: i32 = 92;
const COL_GAP: i32 = 8;

const BG: &str = "#10131a";
const PANEL: &str = "#1a1f2a";
const INK: &str = "#e6e9f2";
const MUTED: &str = "#8b94a7";
const FAINT: &str = "#4a5266";
const EMPTY_FILL: &str = "#20252f";
const ENEMY_BACK: &str = "#2a2f3d";
const ENEMY_FRONT: &str = "#5c2730";
const PLAYER_CELL: &str = "#1f3f66";
const EDGE_ENEMY: &str = "#c0536a";
const EDGE_PLAYER: &str = "#4a90d9";
const EDGE_CRAFT: &str = "#a06cd5";
const MIDLINE: &str = "#c9a227";
const FLAME: &str = "#e8853c";

/// XML 文本转义。牌名／技能名是 `&'static str`，但快照是数据不是字面量——
/// 转义这一层不许省：哪天一张卡名里出现 `&`，整张图就从"画错了"降级成"后面全不渲染"。
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn text(x: i32, y: i32, size: i32, fill: &str, anchor_end: bool, s: &str) -> String {
    let a = if anchor_end { "end" } else { "start" };
    format!(r##"<text x="{x}" y="{y}" font-size="{size}" fill="{fill}" text-anchor="{a}">{}</text>"##, esc(s))
}

fn rect(x: i32, y: i32, w: i32, h: i32, fill: &str, stroke: &str, dashed: bool) -> String {
    let dash = if dashed { r##" stroke-dasharray="6 5""## } else { "" };
    format!(r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="6" fill="{fill}" stroke="{stroke}" stroke-width="2"{dash}/>"##)
}

/// 一条水平条：底槽满宽，填充按 value/cap 截断（负数与超上限都夹住，不画成溢出）。
fn bar(x: i32, y: i32, w: i32, h: i32, value: i32, cap: i32, fill: &str) -> String {
    let filled = if cap <= 0 { 0 } else { value.max(0).min(cap) * w / cap };
    format!(
        r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="4" fill="{PANEL}" stroke="{FAINT}" stroke-width="2"/><rect x="{x}" y="{y}" width="{filled}" height="{h}" rx="4" fill="{fill}"/>"##
    )
}

/// 持业者读数：负值夹到 0、见底要标"烛尽"。这不是美化——文本路 `candle_bar` 就是这么印的，
/// 同一状态在两个壳里读出 `-1` 与 `0 烛尽` 两个数，等于悄悄开了第二套呈现口径。
fn candle_readout(hp: i32, cap: i32) -> String {
    let shown = hp.max(0);
    format!("{shown}/{cap}{}", if hp <= 0 { " · 烛尽" } else { "" })
}

/// 一张在场牌的格子：名字、值/阈值、费/焰、标记（造·开端·升级）、技能串。
fn card_cell(x: i32, y: i32, w: i32, c: &CardView, fill: &str, edge: &str) -> String {
    let mut s = rect(x, y, w, CELL_H, fill, if c.crafted { EDGE_CRAFT } else { edge }, false);
    s.push_str(&text(x + 14, y + 30, 22, INK, false, c.name));
    s.push_str(&text(x + 14, y + 66, 28, INK, false, &format!("{} / {}", c.hp, c.threshold)));
    s.push_str(&text(x + 14, y + 92, 16, MUTED, false, &format!("费{} 焰{}", c.cost, c.flame)));
    let mut marks: Vec<String> = Vec::new();
    if c.crafted {
        marks.push("造".to_string());
    }
    if c.starter {
        marks.push("开端".to_string());
    }
    if c.upgrades > 0 {
        marks.push(format!("升级+{}", c.upgrades));
    }
    if c.trait_label != "无" {
        marks.push(c.trait_label.to_string());
    }
    // 开端牌的 `starter` 与 `trait_label` 会撞成同一个词（快照两个字段、中文一个说法），印两遍是噪声不是信息。
    marks.dedup();
    // 业火足额标记：一个实心小块，不配文字——它是给眼睛看的状态，不是第三条数值。
    let lit = c.flame >= c.threshold;
    // 标记占右上角那一格，属性串是右对齐的：不腾出这块宽度，长属性（`战吼:同列+2焰`）会被方块压住尾字。
    if !marks.is_empty() {
        let marks_x = x + w - if lit { 34 } else { 14 };
        s.push_str(&text(marks_x, y + 30, 16, EDGE_CRAFT, true, &marks.join("·")));
    }
    if !c.skills.is_empty() {
        s.push_str(&text(x + w - 14, y + 92, 14, MUTED, true, &c.skills.join("+")));
    }
    if lit {
        s.push_str(&format!(r##"<rect x="{}" y="{}" width="12" height="12" fill="{FLAME}"/>"##, x + w - 26, y + 10));
    }
    s
}

fn empty_cell(x: i32, y: i32, w: i32) -> String {
    let mut s = rect(x, y, w, CELL_H, EMPTY_FILL, FAINT, true);
    s.push_str(&text(x + w / 2, y + CELL_H / 2 + 8, 26, FAINT, false, "·"));
    s
}

/// 一横排四格（空格照画，位置本身就是信息）。
fn card_row(row: &[Option<CardView>; 4], x0: i32, y: i32, w: i32, fill: &str, edge: &str) -> String {
    let mut s = String::new();
    for (i, c) in row.iter().enumerate() {
        let x = x0 + (w + COL_GAP) * i as i32;
        s.push_str(&match c {
            Some(card) => card_cell(x, y, w, card, fill, edge),
            None => empty_cell(x, y, w),
        });
    }
    s
}

/// 单个字符占多宽（以字号为单位）：CJK 记 1，ASCII 记 0.55。
/// 版式要的是"这行字占多宽"，而快照里的文案全是人读的中文——按 `chars()` 计数会把一行 ASCII
/// 当成一行中文那么宽，那正是文本渲染层在定宽格子上踩过的坑。
fn advance(c: char) -> f64 {
    if c.is_ascii() { 0.55 } else { 1.0 }
}

fn display_width(s: &str) -> f64 {
    s.chars().map(advance).sum()
}

/// 按显示宽度折行。Boss 规则行走这条：那句整文是规则本体，**截断等于把规则改短**，宁可占两行。
fn wrap(s: &str, budget: f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        if w + advance(c) > budget && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            w = 0.0;
        }
        line.push(c);
        w += advance(c);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// 按显示宽度截断、补省略号。日志行走这条：它是滚过去的过程记录，没人需要在这一帧看完一整条。
fn fit(s: &str, budget: f64) -> String {
    if display_width(s) <= budget {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        if w + advance(c) > budget - 1.0 {
            break;
        }
        out.push(c);
        w += advance(c);
    }
    out.push('…');
    out
}

/// 一帧盘面 → 一份完整 SVG 文档。**纯函数、零 IO**。
/// `seed` 与 `scene` 只进标题行：帧文件要能自证是哪一桌打出来的，否则一堆图摆在一起无人能对上号。
/// 画布高度**不写死**——它由"这一帧实际排到哪儿"推出来：Boss 规则句比盘面任何一行都长，
/// 钉死高度就等于要么留一截空白、要么把那句挤成看不见的半句。
pub(crate) fn svg_frame(v: &Board, seed: u64, scene: &str) -> String {
    const WIDTH: i32 = 1240;
    let cols = v.enemy_back.len().max(1) as i32;
    let board_w = WIDTH - 2 * MARGIN;
    let cell_w = (board_w - COL_GAP * (cols - 1)) / cols;

    let mut s = String::new();
    let mut y = 40;
    s.push_str(&text(MARGIN, y, 24, INK, false, &format!("《中线》2D 帧 · {scene} · seed {seed}")));
    s.push_str(&text(WIDTH - MARGIN, y, 24, MUTED, true, &format!("第 {} 回合", v.player.turn)));
    y += 28;

    if let Some(r) = &v.rule {
        for line in &wrap(&format!("特殊规则【{}】{}", r.label, r.text), board_w as f64 / 18.0 - 1.0) {
            s.push_str(&text(MARGIN, y, 18, MIDLINE, false, line));
            y += 22;
        }
    }
    y += 24;

    // 敌方持业者 1~2 根并排：业力只在第一根（共用一池），第二根带受击列区。
    for (i, h) in v.enemy_holders.iter().enumerate() {
        let x = MARGIN + (i as i32) * 400;
        let mut label = format!("{} {}", h.name, candle_readout(h.hp, h.cap));
        if let Some(k) = h.karma {
            label.push_str(&format!(" · 业力 {}", k));
        }
        if let Some((a, b)) = h.column_zone {
            label.push_str(&format!(" · 第{}-{}列直击此柱", a, b));
        }
        s.push_str(&text(x, y, 18, EDGE_ENEMY, false, &label));
        s.push_str(&bar(x, y + 8, 360, 20, h.hp, h.cap, EDGE_ENEMY));
    }
    y += 28 + 16;

    s.push_str(&text(MARGIN, y, 16, FAINT, false, "敌方后排 · 不可攻击"));
    y += 16;
    s.push_str(&card_row(&v.enemy_back, MARGIN, y, cell_w, ENEMY_BACK, FAINT));
    y += CELL_H + 10;
    s.push_str(&text(MARGIN, y, 16, EDGE_ENEMY, false, "敌方前排 · 可攻击"));
    y += 16;
    s.push_str(&card_row(&v.enemy_front, MARGIN, y, cell_w, ENEMY_FRONT, EDGE_ENEMY));
    y += CELL_H + 12;

    s.push_str(&format!(r##"<rect x="{MARGIN}" y="{y}" width="{board_w}" height="24" fill="{MIDLINE}" opacity="0.85"/>"##));
    s.push_str(&text(WIDTH / 2, y + 17, 15, BG, false, "中线 · 绝对边界（任何卡牌不能越过）"));
    y += 24 + 26;

    s.push_str(&text(MARGIN, y, 16, EDGE_PLAYER, false, "我方 P1-P4 · 全部可攻击"));
    y += 16;
    s.push_str(&card_row(&v.player_front, MARGIN, y, cell_w, PLAYER_CELL, EDGE_PLAYER));
    y += CELL_H + 22;

    // 我方持业者：与敌方对称，同一支 bar、同一套读数口径。
    let mut me = format!("我方持业者 {}", candle_readout(v.player.hp, v.player.cap));
    me.push_str(&format!(" · 业力 {}", v.player.karma));
    s.push_str(&text(MARGIN, y, 18, EDGE_PLAYER, false, &me));
    s.push_str(&bar(MARGIN, y + 8, 360, 20, v.player.hp, v.player.cap, EDGE_PLAYER));
    y += 28 + 26;

    // 手牌：一格一张，序号就是落点命令里的那个 idx。
    s.push_str(&text(MARGIN, y, 16, MUTED, false, &format!("手牌 {} 张（序号＝落点命令里的 idx）", v.hand.len())));
    y += 16;
    if v.hand.is_empty() {
        s.push_str(&text(MARGIN + 4, y + 24, 16, FAINT, false, "（空）"));
    } else {
        let n = v.hand.len().max(1) as i32;
        let hw = (board_w - COL_GAP * (n - 1)) / n;
        for (i, c) in v.hand.iter().enumerate() {
            let x = MARGIN + (hw + COL_GAP) * i as i32;
            let mut cell = rect(x, y, hw, HAND_H, PLAYER_CELL, EDGE_PLAYER, false);
            cell.push_str(&text(x + 10, y + 24, 16, FAINT, false, &format!("[{}]", i)));
            cell.push_str(&text(x + 10, y + 48, 18, INK, false, &fit(c.name, (hw - 20) as f64 / 18.0)));
            cell.push_str(&text(x + 10, y + 72, 15, MUTED, false, &format!("费{} 值{} 阈{}", c.cost, c.hp, c.threshold)));
            if c.flame > 0 {
                cell.push_str(&text(x + hw - 10, y + 72, 15, FLAME, true, &format!("焰{}", c.flame)));
            }
            s.push_str(&cell);
        }
    }
    y += HAND_H + 30;

    s.push_str(&text(MARGIN, y, 16, MUTED, false, &format!("继承堆 {} 张（自动抽） · 开端堆 {} 张", v.draw_pile_len, v.starter_pile_len)));
    y += 14;

    // 日志尾：引擎日志本来就是文本，这里只做转义、截行，不重排。
    let log_y = y;
    s.push_str(&format!(r##"<rect x="{MARGIN}" y="{log_y}" width="{board_w}" height="132" rx="6" fill="{PANEL}" stroke="{FAINT}" stroke-width="1"/>"##));
    let start = v.log.len().saturating_sub(6);
    for (i, l) in v.log[start..].iter().enumerate() {
        s.push_str(&text(MARGIN + 12, log_y + 22 + (i as i32) * 18, 15, INK, false, &fit(l.trim_start(), board_w as f64 / 15.0 - 2.0)));
    }
    let height = log_y + 132 + 20;

    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    out.push_str(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" viewBox="0 0 {WIDTH} {height}" font-family="Source Han Sans CN, Noto Sans CJK SC, sans-serif">"##
    ));
    out.push_str(&format!(r##"<rect width="{WIDTH}" height="{height}" fill="{BG}"/>"##));
    out.push_str(&s);
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::{Battle, Difficulty, Row};
    use crate::boss::BossId;
    use crate::model::{faction_cards, CardInst, Faction};

    fn normal() -> Battle {
        Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1)
    }

    /// 编码器只许吃快照：格子里出现的每个数，都必须能在**同一份 `Board`** 里逐字找到。
    /// 断言一律拿快照的实际字段去撞（不硬编码名字/数字），所以引擎文案改了不会误红，
    /// 而"编码器自己造了一套数"这种形状当场红。
    #[test]
    fn every_number_on_the_frame_comes_from_the_snapshot() {
        let mut b = normal();
        b.e_karma = 50;
        let held = CardInst::new(900, faction_cards(Faction::Ember)[1]);
        let held_name = held.def.name;
        b.enemy_place_inst(held, 0, Row::Front).unwrap();
        let v = b.view();
        let frame = svg_frame(&v, 7, "test");

        assert!(frame.contains(held_name), "在场牌名要出现：{held_name}");
        assert!(frame.contains(&format!("{} / {}", v.enemy_front[0].as_ref().unwrap().hp, v.enemy_front[0].as_ref().unwrap().threshold)), "值/阈值那一对要用快照的实际值");
        assert!(frame.contains(&format!("第 {} 回合", v.player.turn)));
        assert!(frame.contains(&format!("业力 {}", v.player.karma)));
        assert!(frame.contains(&format!("继承堆 {} 张", v.draw_pile_len)));
        for c in &v.hand {
            assert!(frame.contains(c.name), "每张手牌都要画出来（漏一张＝手牌行只画了标题）：{}/{}", c.name, v.hand.len());
        }
    }

    /// 纯函数：同一份快照连编两次必须逐字节相同。帧文件一旦可比 md5，"同 seed 同帧"这条口径才立得住。
    #[test]
    fn the_encoder_is_pure() {
        let b = normal();
        let v = b.view();
        assert_eq!(svg_frame(&v, 7, "same"), svg_frame(&v, 7, "same"), "同快照同标题 ⇒ 同字节");
        assert_ne!(svg_frame(&v, 7, "a"), svg_frame(&v, 8, "a"), "换 seed 要换字节（标题带 seed）");
    }

    /// 双柱 Boss 两根都要画，且第二根不许带业力（快照给 None，编码器不许自己补一个数）。
    #[test]
    fn dual_holder_draws_both_bars_and_inherits_the_karma_absence() {
        let b = Battle::new_boss(2026, Faction::Ember, BossId::YanBing, Vec::new(), 48);
        let v = b.view();
        let frame = svg_frame(&v, 2026, "boss");
        assert_eq!(v.enemy_holders.len(), 2);
        assert!(frame.contains(v.enemy_holders[0].name), "第一柱名要出现");
        assert!(frame.contains(v.enemy_holders[1].name), "第二柱名也要出现（画漏一根就成了一柱图）");
        assert!(frame.contains("直击此柱"), "第二根的受击列区要带上");
    }

    /// 转义不是装饰：快照里出现 XML 元字符时，图必须还是合法文档。
    /// `Board` 的字段是 `pub(crate)`，所以这里能直接造一张带 `&`/`<` 的假牌——真实卡表里没有这种名字，
    /// 但编码器不该依赖"卡表永远干净"这个它自己看不见的性质。
    #[test]
    fn snapshot_text_is_escaped_not_interpolated() {
        let mut v = normal().view();
        let mut c = CardView {
            name: "甲&乙<丙>",
            cost: 1,
            hp: 2,
            threshold: 3,
            flame: 0,
            upgrades: 0,
            crafted: false,
            starter: false,
            trait_label: "无",
            skills: vec![],
        };
        v.hand.push(c.clone());
        c.name = "丁\"戊";
        v.player_front[0] = Some(c);
        let frame = svg_frame(&v, 1, "escape");
        assert!(frame.contains("甲&amp;乙&lt;丙&gt;"), "手牌里的元字符要转义后出现");
        assert!(frame.contains("丁&quot;戊"), "在场格同样转义");
        assert!(!frame.contains("甲&乙<"), "原始元字符不许进文档（那会撕掉后面的节点）");
    }

    /// 一帧里格子的总数＝盘面 12 格，空格也要画格子（虚线框）。
    /// 列数从快照长度取，所以这条钉的是"取长度这条路真的走通了"：少画一格不会报错，只会静默变成一张缺一列的图。
    #[test]
    fn the_board_draws_one_cell_per_slot_not_per_hardcoded_count() {
        let v = normal().view();
        let frame = svg_frame(&v, 7, "grid");
        let slots = v.enemy_back.len() + v.enemy_front.len() + v.player_front.len();
        assert_eq!(slots, 12, "盘面固定 4 列 × 3 排");
        let empties = v.enemy_back.iter().chain(v.enemy_front.iter()).chain(v.player_front.iter()).filter(|c| c.is_none()).count();
        assert_eq!(frame.matches("stroke-dasharray").count(), empties, "虚线框数＝空格数（少一个＝有一格没画）");
        assert_eq!(frame.matches(r##"height="104""##).count(), slots, "格子总数＝槽位数（空格也要画框）");
    }

    /// 业火足额那个实心块：**恰好**在 flame ≥ threshold 时出现，一格一块。
    /// 这条钉的是"图上那个记号真的对应引擎那两个数"，不是"记号好不好看"。
    #[test]
    fn the_flame_marker_appears_exactly_when_the_cell_is_at_threshold() {
        let mut v = normal().view();
        assert_eq!(frame_markers(&svg_frame(&v, 7, "m")), 0, "空盘没有可标的格");

        let mut lit = CardView { name: "足", cost: 1, hp: 5, threshold: 3, flame: 3, upgrades: 0, crafted: false, starter: false, trait_label: "无", skills: vec![] };
        v.player_front[0] = Some(lit.clone());
        assert_eq!(frame_markers(&svg_frame(&v, 7, "m")), 1, "焰＝阈 算足额");

        lit.flame = 2;
        v.player_front[1] = Some(lit);
        assert_eq!(frame_markers(&svg_frame(&v, 7, "m")), 1, "焰<阈 不许亮");

        let other = CardView { name: "差", cost: 1, hp: 5, threshold: 9, flame: 0, upgrades: 0, crafted: false, starter: false, trait_label: "无", skills: vec![] };
        v.enemy_front[0] = Some(other);
        assert_eq!(frame_markers(&svg_frame(&v, 7, "m")), 1, "敌方格也走同一支笔画，不许多出一块");
    }

    fn frame_markers(frame: &str) -> usize {
        frame.matches(r##"width="12" height="12""##).count()
    }

    /// 烛的读数与文本路同口径：负值夹到 0、见底标"烛尽"。
    /// 引擎在超额削烛那一刻确实把 `-1` 交出来（帧 r7 末格实测），原样印上去就是拿一个眼睛读不懂的数当呈现。
    #[test]
    fn the_candle_readout_clamps_below_zero_and_marks_the_extinguished_wick() {
        assert_eq!(candle_readout(14, 20), "14/20", "常态读数不加任何装饰");
        assert_eq!(candle_readout(0, 20), "0/20 · 烛尽", "见底要标出来");
        assert_eq!(candle_readout(-1, 20), "0/20 · 烛尽", "负值夹到 0，不把超额伤害原样画出去");
    }

    /// 业火方块占右上角，属性串是右对齐的 ⇒ 点亮时锚点必须让到方块左侧，否则长属性（`战吼:同列+2焰`）
    /// 的尾字会被方块压住——这一条就是那次压字事故的牙：锚点只要往回挪到常态那个数，断言当场红。
    #[test]
    fn the_flame_badge_never_sits_on_top_of_the_trait_text() {
        let lit = CardView { name: "火星", cost: 2, hp: 1, threshold: 3, flame: 3, upgrades: 0, crafted: false, starter: false, trait_label: "战吼:同列+2焰", skills: vec![] };
        let dim = CardView { flame: 0, ..lit.clone() };
        let cell_w = 300;

        let s = card_cell(0, 0, cell_w, &lit, PLAYER_CELL, EDGE_PLAYER);
        let badge_x = attr_before(&s, r##"width="12" height="12""##).expect("足额格要画方块");
        let marks_x = attr_before(&s, r##"y="30" font-size="16""##).expect("有属性串就要画出来");
        assert!(marks_x <= badge_x, "属性串右界 {marks_x} 不得越过方块左界 {badge_x}（越过去＝方块压在字上）");

        // 不点亮时不该白让这块宽度：字要贴着常态的右边距。
        let t = card_cell(0, 0, cell_w, &dim, PLAYER_CELL, EDGE_PLAYER);
        assert_eq!(attr_before(&t, r##"y="30" font-size="16""##).expect("属性串要画出来"), cell_w - 14, "没点亮就不该让位");
    }

    /// 从 `needle` 处往回找最近的那个 `x="N"`，取回 N。只用来读几何断言需要的坐标，不是 SVG 解析器。
    fn attr_before(s: &str, needle: &str) -> Option<i32> {
        let at = s.find(needle)?;
        let head = &s[..at];
        let tail = head.rsplit(r##"x=""##).next()?;
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    }

    /// 开端牌不许把"开端"印两遍：快照里 `starter` 与 `trait_label` 是两个字段、中文是同一个说法，
    /// 各自往右上角推一次就成 `开端 · 开端`（首版实测就是这样，交付图上一眼可见）。
    #[test]
    fn a_starter_card_says_it_once_not_twice() {
        let starter = CardView { name: "余烬", cost: 0, hp: 1, threshold: 4, flame: 0, upgrades: 0, crafted: false, starter: true, trait_label: "开端", skills: vec![] };
        let s = card_cell(0, 0, 300, &starter, PLAYER_CELL, EDGE_PLAYER);
        assert_eq!(s.matches("开端").count(), 1, "同一个词印两遍＝噪声：{s}");

        // 反面对照：不同来源的两个词该都留下，不许把 dedup 写成"只留第一个标记"。
        let both = CardView { name: "余烬", cost: 0, hp: 1, threshold: 4, flame: 0, upgrades: 1, crafted: true, starter: false, trait_label: "战吼:同列+2焰", skills: vec![] };
        let t = card_cell(0, 0, 300, &both, PLAYER_CELL, EDGE_PLAYER);
        assert!(t.contains("造·升级+1·战吼:同列+2焰"), "三个不同来源的标记要连起来全在：{t}");
    }

    /// 折行按**显示宽度**，不按字符数：同样一行预算，全 ASCII 装得下更多字符；
    /// 中文要在画布边缘之前断开——断不开就是这份交付里最容易看见的失真（规则句被裁掉半句）。
    #[test]
    fn text_wraps_by_display_width_not_by_char_count() {
        assert_eq!(wrap("中文中文中文", 3.0).len(), 2, "每行 3 个汉字 ⇒ 六个字两行");
        assert_eq!(wrap("abcdefghij", 3.0).len(), 2, "ASCII 记 0.55 ⇒ 十个字母两行，不是四行");
        assert_eq!(wrap("", 3.0), Vec::<String>::new(), "空串不产空行");
        assert_eq!(fit("中文中文", 3.0), "中文…", "超预算才截断");
        assert_eq!(fit("中文", 99.0), "中文", "没超预算就一字不动（不许顺手加省略号）");
    }

    /// Boss 规则句**整句**要进图：折行可以，丢字不行。那句是规则本体，不是装饰文案。
    #[test]
    fn the_boss_rule_line_loses_no_characters() {
        let b = Battle::new_boss(2026, Faction::Ember, BossId::YanBing, Vec::new(), 48);
        let v = b.view();
        let rule = v.rule.as_ref().expect("炎与冰有特殊规则");
        let want = esc(&format!("特殊规则【{}】{}", rule.label, rule.text));
        let frame = svg_frame(&v, 2026, "boss");
        // 只认 `<text>` 标签自己带这个颜色：整段文档里中线那条 `<rect>` 也是同一个色值，
        // 拿"包含"去筛会把那条色带也算成规则行——筛错一侧，这条测就成了永真。
        let tag = format!(r##"font-size="18" fill="{MIDLINE}""##);
        let got: String = frame
            .split("<text ")
            .filter_map(|t| {
                let (head, rest) = t.split_once('>')?;
                head.contains(&tag).then(|| rest.split('<').next().unwrap_or(""))
            })
            .collect();
        assert_eq!(got, want, "把规则行的各段拼回去要逐字等于原句（少字＝被裁，多字＝串行）");
        assert!(frame.matches(&tag).count() > 1, "这句长到必须折行（本条测的就是折行不丢字）");
    }

    /// 甲 这条路的前提：引擎零外部依赖。它是本仓自立的立场，不是别人的要求，
    /// 所以立在这儿当尺——破了就红，别让"顺手加一个 crate"在无人裁决的情况下过去。
    #[test]
    fn the_engine_still_ships_zero_external_dependencies() {
        let lock = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
        let src = std::fs::read_to_string(&lock).expect("没有 Cargo.lock：这仓已经不是单 crate 形态了");
        let crates = src.lines().filter(|l| l.trim() == "name = \"midline\"").count();
        let all = src.lines().filter(|l| l.starts_with("name = ")).count();
        assert_eq!(crates, 1, "Cargo.lock 里应当恰好一个 midline");
        assert_eq!(all, 1, "Cargo.lock 里有 {all} 个 crate：零依赖立场被破了（走 丙 需要人来裁，不是加一行依赖）");
    }
}
