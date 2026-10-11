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
//! 形状声明：它只画**静态一帧**——没有动画、没有音效。存在的理由不是"做个好看界面"，
//! 而是给设计交一件**共有的实物**：同一句文字描述有两个读者就有两份想象，一张图只有一份。
//!
//! 上游只有三条，且都不许绕过：
//! - **数据**：所有数都从 `view::Board` 拿，一个引擎字段都不直读——口径与 `render`／`tui` 相同，不开第二真相源。
//!   列数不写死在版式里，而是从快照的数组长度取（`enemy_back.len()`）：那一维本来就是盘面形状，
//!   在编码器里再钉一次，就成了"引擎改了棋盘列数、图还在画四列"这种最难看的失真。
//! - **几何**：每个矩形、每条基线都从 `crate::touch::layout_of` 拿。本模块**一行位置算术都不写**——
//!   从前它一边画一边推 y 游标，于是"画在哪"与"点在哪"是两个模块里各算一遍的两份算术。
//!   合成一处之后，`touch` 那条 `the_regions_that_take_a_tap_are_the_regions_that_got_drawn` 才量得出东西。
//! - **版式**：所有视觉常数（色值、圆角、字号、线宽、虚线段长）都从 `crate::md3` 拿，本层不发明数
//!   （`md3::tests::visual_constants_have_one_home` 当场红，理由与口径见那个模块的抬头）。
//!
//! 规则锚：本模块**不写**文档锚点，理由与 `tui` 那条相同——排布不新增任何规则真值，写锚会去动反向覆盖尺的生产面读数。
//! 版式里几句像规则的措辞（后排不可攻击、中线不可越过）**不是**这里定的，是照抄 `render` 既有文案。

use crate::md3::{color, lane, layout, shape, state, svg, typo};
use crate::touch::{self, Hand, Layer, Rect, Region, layout_of};
use crate::view::{Board, CardView};

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

/// 一块矩形的画法：填充、描边色、圆角、描边宽**四件一起传**。
/// 合成一个类型是为了让 `rect` 不再靠位置参数数到第九个；四件全由调用方从 `shape`／`layout` 取，
/// 本层不默认任何一档——默认值就是第二个家，它会让"这格是 8dp 圆角"这件事在两个地方各写一遍。
/// `fill` 取 `&str` 而不是 `&'static str`：态层混出来的是运行时算的一个色串，不是表里的常数。
#[derive(Copy, Clone)]
struct Paint<'a> {
    fill: &'a str,
    stroke: &'a str,
    radius: i32,
    width: i32,
}

fn rect(cell: Rect, p: Paint<'_>, dashed: bool) -> String {
    let dash = if dashed { format!(r##" stroke-dasharray="{} {}""##, layout::DASH_ON, layout::DASH_OFF) } else { String::new() };
    format!(r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{}" fill="{}" stroke="{}" stroke-width="{}"{dash}/>"##, p.radius, p.fill, p.stroke, p.width, x = cell.x, y = cell.y, w = cell.w, h = cell.h)
}

/// 一条水平条：底槽满宽，填充按 value/cap 截断（负数与超上限都夹住，不画成溢出）。
fn bar(cell: Rect, value: i32, cap: i32, fill: &str) -> String {
    let filled = if cap <= 0 { 0 } else { value.max(0).min(cap) * cell.w / cap };
    let track = format!(r##"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="{}" stroke="{}" stroke-width="{}"/>"##, cell.x, cell.y, cell.w, cell.h, shape::EXTRA_SMALL, color::SURFACE_CONTAINER, color::OUTLINE_VARIANT, layout::STROKE_HAIRLINE);
    let spent = format!(r##"<rect x="{}" y="{}" width="{filled}" height="{}" rx="{}" fill="{fill}"/>"##, cell.x, cell.y, cell.h, shape::EXTRA_SMALL);
    track + &spent
}

/// 持业者读数：负值夹到 0、见底要标"烛尽"。这不是美化——文本路 `candle_bar` 就是这么印的，
/// 同一状态在两个壳里读出 `-1` 与 `0 烛尽` 两个数，等于悄悄开了第二套呈现口径。
fn candle_readout(hp: i32, cap: i32) -> String {
    let shown = hp.max(0);
    format!("{shown}/{cap}{}", if hp <= 0 { " · 烛尽" } else { "" })
}

/// 业火足额标记块左界相对格右边的内缩：块宽加内缩，一处算清，别在两处各写一遍同一个和。
fn badge_inset() -> i32 {
    layout::FLAME_MARK + layout::FLAME_INSET
}

/// 底色 × 态层 = 一个**实心**色。没有层时原样回到底色串（那一帧因此与控件层毫不相干，可比 md5）；
/// 有层时按 MD3 口径混色。混色算式住在 `md3::state::blend_hex`，桌面与安卓共用它——
/// 桌面从前可以走 `fill-opacity`，但真机那条路是帧缓冲写像素、没有合成器替做 alpha，
/// 一个走混合一个走属性，就是"同一状态在两个壳里读出两个色"那种第二真值源。
fn layered(base: &str, layer: Option<Layer>) -> String {
    match layer {
        None => base.to_string(),
        Some(l) => state::blend_hex(base, color::ON_SURFACE, l.alpha()),
    }
}

/// 这一格该盖哪一层。`marks` 按优先级**升序**给出（见 `touch::Picker::marks`）⇒ 这里取最后一个命中：
/// 焦点与按下在 MD3 里同值，只有次序能让按下仍然看得见。
pub(crate) fn layer_of(marks: &[(Region, Layer)], region: Region) -> Option<Layer> {
    marks.iter().rev().find(|(g, _)| *g == region).map(|(_, l)| *l)
}

/// 一张在场牌的格子：名字、值/阈值、费/焰、标记（造·开端·升级）、技能串。
/// 位置与面积全在 `cell` 里（它就是命中框），本层只往上面摆字。
/// 描边**不随态层变**：MD3 的状态层盖在容器上，描边是边界，跟着亮会读成"边框也在动"那种新信号。
fn card_cell(cell: Rect, c: &CardView, fill: &str, edge: &'static str) -> String {
    let x = cell.x;
    let y = cell.y;
    let w = cell.w;
    let paint = Paint { fill, stroke: if c.crafted { lane::CRAFT } else { edge }, radius: shape::SMALL, width: layout::STROKE_BOUNDARY };
    let mut s = rect(cell, paint, false);
    s.push_str(&text(x + layout::CELL_PAD, y + layout::CELL_ROW1, typo::TITLE_LARGE, color::ON_SURFACE, false, c.name));
    s.push_str(&text(x + layout::CELL_PAD, y + layout::CELL_ROW2, typo::HEADLINE_MEDIUM, color::ON_SURFACE, false, &format!("{} / {}", c.hp, c.threshold)));
    s.push_str(&text(x + layout::CELL_PAD, y + layout::CELL_ROW3, typo::BODY_LARGE, lane::META, false, &format!("费{} 焰{}", c.cost, c.flame)));
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
        let marks_x = x + w - if lit { badge_inset() + layout::CELL_PAD_NARROW } else { layout::CELL_PAD };
        s.push_str(&text(marks_x, y + layout::CELL_ROW1, typo::BODY_LARGE, lane::CRAFT, true, &marks.join("·")));
    }
    if !c.skills.is_empty() {
        s.push_str(&text(x + w - layout::CELL_PAD, y + layout::CELL_ROW3, typo::LABEL_LARGE, lane::META, true, &c.skills.join("+")));
    }
    if lit {
        let badge = Paint { fill: lane::FLAME, stroke: lane::FLAME, radius: shape::NONE, width: layout::STROKE_HAIRLINE };
        let mark = Rect::new(x + w - badge_inset(), y + layout::FLAME_INSET, layout::FLAME_MARK, layout::FLAME_MARK);
        s.push_str(&rect(mark, badge, false));
    }
    s
}

/// 空格子：虚线框加一个点。它**照样有面积**——"这儿能放"是位置信息，接得住一次命中。
fn empty_cell(cell: Rect, layer: Option<Layer>) -> String {
    let base = layered(lane::EMPTY_FILL, layer);
    let paint = Paint { fill: &base, stroke: color::OUTLINE_VARIANT, radius: shape::SMALL, width: layout::STROKE_BOUNDARY };
    let mut s = rect(cell, paint, true);
    s.push_str(&text(cell.x + cell.w / 2, cell.y + cell.h / 2 + layout::CELL_PAD_NARROW, typo::TITLE_LARGE, lane::EMPTY_MARK, false, "·"));
    s
}

/// 一横排若干格（空格照画，位置本身就是信息）。
/// 矩形来自几何表、卡来自快照，两者按序并排：长度对不上时静默丢掉一半就是"少画一格不报错"那条老坑，
/// 所以这里当场断言（`debug_assert`），而不是留给目视。
fn card_row(cells: &[(Region, Rect)], cards: &[Option<CardView>], fill: &str, edge: &'static str, marks: &[(Region, Layer)]) -> String {
    debug_assert_eq!(cells.len(), cards.len(), "几何表与快照的格数不一致：两份盘面");
    let mut s = String::new();
    for ((region, cell), card) in cells.iter().zip(cards.iter()) {
        let layer = layer_of(marks, *region);
        s.push_str(&match card {
            Some(c) => card_cell(*cell, c, &layered(fill, layer), edge),
            None => empty_cell(*cell, layer),
        });
    }
    s
}

/// 手牌行：一格一张，序号就是落点命令里的那个 idx。截断预算来自几何（它是格宽的函数）。
fn hand_row(hand: &Hand, cards: &[CardView], marks: &[(Region, Layer)]) -> String {
    debug_assert_eq!(hand.cells.len(), cards.len(), "几何表与快照的手牌数不一致：两份盘面");
    let mut s = String::new();
    for (i, ((region, cell), c)) in hand.cells.iter().zip(cards.iter()).enumerate() {
        let x = cell.x;
        let y = cell.y;
        let fill = layered(lane::PLAYER_FILL, layer_of(marks, *region));
        let paint = Paint { fill: &fill, stroke: lane::PLAYER_EDGE, radius: shape::SMALL, width: layout::STROKE_HAIRLINE };
        let pad = x + layout::CELL_PAD_NARROW;
        let mut card = rect(*cell, paint, false);
        card.push_str(&text(pad, y + layout::HAND_ROW1, typo::LABEL_LARGE, color::ON_PRIMARY_CONTAINER, false, &format!("[{}]", i)));
        card.push_str(&text(pad, y + layout::HAND_ROW2, typo::TITLE_LARGE, color::ON_SURFACE, false, &touch::fit(c.name, hand.name_budget)));
        card.push_str(&text(pad, y + layout::HAND_ROW3, typo::BODY_MEDIUM, lane::META, false, &format!("费{} 值{} 阈{}", c.cost, c.hp, c.threshold)));
        if c.flame > 0 {
            card.push_str(&text(x + cell.w - layout::CELL_PAD_NARROW, y + layout::HAND_ROW3, typo::BODY_MEDIUM, lane::FLAME, true, &format!("焰{}", c.flame)));
        }
        s.push_str(&card);
    }
    s
}

/// 一帧盘面 → 一份完整 SVG 文档。**纯函数、零 IO**。
/// `seed` 与 `scene` 只进标题行：帧文件要能自证是哪一桌打出来的，否则一堆图摆在一起无人能对上号。
pub(crate) fn svg_frame(v: &Board, seed: u64, scene: &str) -> String {
    svg_frame_marked(v, seed, scene, &[])
}

/// 同一帧，但把控件层要盖的态层画上（`marks` 由 `touch::Picker::marks` 按优先级升序给出）。
/// `marks` 为空 ⇒ 与 `svg_frame` **逐字节**相同：控件层不许反过来污染呈现真值，
/// 那件事由 `touch::tests::the_control_layer_adds_nothing_when_nothing_is_pressed` 钉住，
/// 而改前／改后两批帧文件的 md5 对撞记在 档 §2。
/// 画布高度**不写死**——它由 `layout_of` 从"这一帧实际排到哪儿"推出来：Boss 规则句比盘面任何一行都长，
/// 钉死高度就等于要么留一截空白、要么把那句挤成看不见的半句。
pub(crate) fn svg_frame_marked(v: &Board, seed: u64, scene: &str, marks: &[(Region, Layer)]) -> String {
    let l = layout_of(v);
    let mut s = String::new();

    s.push_str(&text(l.left_x, l.title_y, typo::HEADLINE_SMALL, color::ON_SURFACE, false, &format!("《中线》2D 帧 · {scene} · seed {seed}")));
    s.push_str(&text(l.right_x, l.title_y, typo::HEADLINE_SMALL, lane::META, true, &format!("第 {} 回合", v.player.turn)));

    for (line, baseline) in &l.rule {
        s.push_str(&text(l.left_x, *baseline, typo::TITLE_LARGE, lane::MIDLINE, false, line));
    }

    // 敌方持业者 1~2 根并排：业力只在第一根（共用一池），第二根带受击列区。
    for (holder, h) in l.holders.iter().zip(v.enemy_holders.iter()) {
        let mut label = format!("{} {}", h.name, candle_readout(h.hp, h.cap));
        if let Some(k) = h.karma {
            label.push_str(&format!(" · 业力 {}", k));
        }
        if let Some((a, b)) = h.column_zone {
            label.push_str(&format!(" · 第{}-{}列直击此柱", a, b));
        }
        s.push_str(&text(holder.x, holder.y, typo::TITLE_LARGE, lane::ENEMY_FRONT_EDGE, false, &label));
        s.push_str(&bar(holder.rect(), h.hp, h.cap, lane::ENEMY_FRONT_EDGE));
    }

    s.push_str(&text(l.left_x, l.back.label_y, typo::BODY_LARGE, color::OUTLINE_VARIANT, false, "敌方后排 · 不可攻击"));
    s.push_str(&card_row(&l.back.cells, &v.enemy_back, lane::ENEMY_BACK_FILL, color::OUTLINE_VARIANT, marks));
    s.push_str(&text(l.left_x, l.enemy.label_y, typo::BODY_LARGE, lane::ENEMY_FRONT_EDGE, false, "敌方前排 · 可攻击"));
    s.push_str(&card_row(&l.enemy.cells, &v.enemy_front, lane::ENEMY_FRONT_FILL, lane::ENEMY_FRONT_EDGE, marks));

    // 中线：一条实色带，不是描边——它要的是"过不去"那种视觉重量，描边给不了。
    let band = format!(r##"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="{}" opacity="{}"/>"##, l.midline.x, l.midline.y, l.midline.w, l.midline.h, shape::NONE, lane::MIDLINE, lane::MIDLINE_BAND_SPOT);
    s.push_str(&band);
    s.push_str(&text(l.center_x, l.midline_text_y, typo::BODY_MEDIUM, lane::ON_MIDLINE, false, "中线 · 绝对边界（任何卡牌不能越过）"));

    s.push_str(&text(l.left_x, l.player.label_y, typo::BODY_LARGE, lane::PLAYER_EDGE, false, "我方 P1-P4 · 全部可攻击"));
    s.push_str(&card_row(&l.player.cells, &v.player_front, lane::PLAYER_FILL, lane::PLAYER_EDGE, marks));

    // 我方持业者：与敌方对称，同一支 bar、同一套读数口径。
    let mut me = format!("我方持业者 {}", candle_readout(v.player.hp, v.player.cap));
    me.push_str(&format!(" · 业力 {}", v.player.karma));
    s.push_str(&text(l.player_holder.x, l.player_holder.y, typo::TITLE_LARGE, lane::PLAYER_EDGE, false, &me));
    s.push_str(&bar(l.player_holder.rect(), v.player.hp, v.player.cap, lane::PLAYER_EDGE));

    s.push_str(&text(l.left_x, l.hand.label_y, typo::BODY_LARGE, lane::META, false, &format!("手牌 {} 张（序号＝落点命令里的 idx）", v.hand.len())));
    match l.hand.placeholder {
        Some(place) => s.push_str(&text(place.x, place.y, typo::BODY_LARGE, color::OUTLINE_VARIANT, false, "（空）")),
        None => s.push_str(&hand_row(&l.hand, &v.hand, marks)),
    }

    s.push_str(&text(l.left_x, l.piles_y, typo::BODY_LARGE, lane::META, false, &format!("继承堆 {} 张（自动抽） · 开端堆 {} 张", v.draw_pile_len, v.starter_pile_len)));

    // 日志尾：引擎日志本来就是文本，这里只做转义、截行，不重排。
    s.push_str(&format!(r##"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="{}" stroke="{}" stroke-width="{}"/>"##, l.log.x, l.log.y, l.log.w, l.log.h, shape::SMALL, color::SURFACE_CONTAINER, color::OUTLINE_VARIANT, layout::STROKE_HAIRLINE));
    let start = v.log.len().saturating_sub(layout::LOG_LINES);
    for (i, entry) in v.log[start..].iter().enumerate() {
        let line_y = l.log_first_y + i as i32 * l.log_step;
        s.push_str(&text(l.log_text_x, line_y, typo::BODY_MEDIUM, color::ON_SURFACE, false, &touch::fit(entry.trim_start(), l.log_budget)));
    }

    let mut out = String::new();
    out.push_str(svg::PROLOG);
    out.push('\n');
    out.push_str(&format!(r##"<svg xmlns="{}" width="{}" height="{}" viewBox="0 0 {} {}" font-family="{}">"##, svg::NS, l.width, l.height, l.width, l.height, typo::FONT_FAMILY));
    out.push_str(&format!(r##"<rect width="{}" height="{}" fill="{}"/>"##, l.width, l.height, color::SURFACE));
    out.push_str(&s);
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::layout::*;
    use super::typo::*;
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
        let cell_box = format!(r##"height="{CELL_H}""##);
        assert_eq!(frame.matches(&cell_box).count(), slots, "格子总数＝槽位数（空格也要画框）");
    }

    /// 业火足额标记：**恰好**在 flame ≥ threshold 时出现，一格一块。
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

    /// 足额块的搜索串由 `FLAME_MARK` 算出，不抄字面量——否则改了 token 这条测照样绿，就成了给旧读数背书。
    fn frame_markers(frame: &str) -> usize {
        let needle = format!(r##"width="{FLAME_MARK}" height="{FLAME_MARK}""##);
        frame.matches(&needle).count()
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
        let mark_box = format!(r##"width="{FLAME_MARK}" height="{FLAME_MARK}""##);
        let marks_needle = format!(r##"y="{CELL_ROW1}" font-size="{BODY_LARGE}""##);

        let s = card_cell(Rect::new(0, 0, cell_w, CELL_H), &lit, lane::PLAYER_FILL, lane::PLAYER_EDGE);
        let badge_x = attr_before(&s, &mark_box).expect("足额格要画方块");
        let marks_x = attr_before(&s, &marks_needle).expect("有属性串就要画出来");
        assert!(marks_x <= badge_x, "属性串右界 {marks_x} 不得越过方块左界 {badge_x}（越过去＝方块压在字上）");

        // 不点亮时不该白让这块宽度：字要贴着常态的右边距。
        let t = card_cell(Rect::new(0, 0, cell_w, CELL_H), &dim, lane::PLAYER_FILL, lane::PLAYER_EDGE);
        assert_eq!(attr_before(&t, &marks_needle).expect("属性串要画出来"), cell_w - CELL_PAD, "没点亮就不该让位");
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
        let s = card_cell(Rect::new(0, 0, 300, CELL_H), &starter, lane::PLAYER_FILL, lane::PLAYER_EDGE);
        assert_eq!(s.matches("开端").count(), 1, "同一个词印两遍＝噪声：{s}");

        // 反面对照：不同来源的两个词该都留下，不许把 dedup 写成"只留第一个标记"。
        let both = CardView { name: "余烬", cost: 0, hp: 1, threshold: 4, flame: 0, upgrades: 1, crafted: true, starter: false, trait_label: "战吼:同列+2焰", skills: vec![] };
        let t = card_cell(Rect::new(0, 0, 300, CELL_H), &both, lane::PLAYER_FILL, lane::PLAYER_EDGE);
        assert!(t.contains("造·升级+1·战吼:同列+2焰"), "三个不同来源的标记要连起来全在：{t}");
    }

    /// Boss 规则句**整句**要进图：折行可以，丢字不行。那句是规则本体，不是装饰文案。
    /// 折行本身住在 `touch::layout_of`（行数决定后面所有 y），这条从帧那侧证明一个字没丢。
    #[test]
    fn the_boss_rule_line_loses_no_characters() {
        let b = Battle::new_boss(2026, Faction::Ember, BossId::YanBing, Vec::new(), 48);
        let v = b.view();
        let rule = v.rule.as_ref().expect("炎与冰有特殊规则");
        let want = esc(&format!("特殊规则【{}】{}", rule.label, rule.text));
        let frame = svg_frame(&v, 2026, "boss");
        // 只认 `<text>` 标签自己带这个颜色：整段文档里中线那条 `<rect>` 也是同一个色值，
        // 拿"包含"去筛会把那条色带也算成规则行——筛错一侧，这条测就成了永真。
        let tag = format!(r##"font-size="{TITLE_LARGE}" fill="{midline}""##, midline = lane::MIDLINE);
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

    /// 版式常数不许在这一文件里各写一遍：帧上的每个色都必须能在 `md3` 里找到同名 token。
    /// 这条与 `md3` 那条机检成对——那条从"表"这侧数漏出来的字面量，这条从"帧"这侧数点名的 token 真的进了文档没有。
    #[test]
    fn the_frame_paints_with_the_tokens_it_names() {
        let v = normal().view();
        let frame = svg_frame(&v, 7, "tokens");
        for (name, token) in [("ON_SURFACE", color::ON_SURFACE), ("OUTLINE_VARIANT", color::OUTLINE_VARIANT), ("SURFACE", color::SURFACE), ("lane::MIDLINE", lane::MIDLINE)] {
            assert!(frame.contains(token), "{name} 没进这一帧：版式表与编码器已经分叉");
        }
        let default_corner = format!(r##"rx="{}""##, shape::SMALL);
        assert!(frame.contains(&default_corner), "格子圆角没取 shape::SMALL：有人改了默认角却没改表");
        let unused_corner = format!(r##"rx="{}""##, shape::EXTRA_LARGE);
        assert!(!frame.contains(&unused_corner), "帧里冒出了表里没派上用场的 {unused_corner} 圆角");
    }
}
