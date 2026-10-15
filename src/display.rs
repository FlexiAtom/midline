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
//! 呈现指令表：一帧**该画什么**的唯一住所，与"用什么语法画"彻底分开。
//!
//! ## 为什么要有这一层
//! 从前"该画什么"和"画成 SVG"写在同一个函数里，于是像素那条路只剩两个选择：
//! **复制一份**（两处各自自洽，差值读出来不知该怪谁），或**不画**（2D 段永远停在一张图上）。
//! 中间那条路是把决策剥成一张指令表：这里只说"这块矩形这个色、这行字落在这个基线"，
//! `canvas` 把它编成 XML，`shell-android/src/frame.rs` 把它写进帧缓冲。**两份编码器读同一份表**，
//! 所以"同一状态在两个壳里读出两个色"这种债在结构上无处发生——不是靠两边都记得改。
//!
//! ## 三条上游，一条都不许绕过（与 `canvas` 从前那三条相同，只是搬了家）
//! - **数据**：所有数都从 `view::Board` 快照拿，一个引擎字段都不直读。
//! - **几何**：每个矩形、每条基线都从 `touch::layout_of` 拿，本模块**一行位置算术都不写**。
//! - **版式**：色值、圆角、字号、线宽、虚线段长全从 `md3` 拿（`md3::tests::visual_constants_have_one_home`
//!   把本模块与 `canvas`／`touch`／壳侧 `frame.rs` 一起扫）。
//!
//! ## 坐标口径
//! `Frame` 里的数全是**用户单位**（与 `touch::Layout` 同口径的 1240 那一族），不是设备像素。
//! 换算只归壳侧那唯一一把 `Scale`（`frame::scale_of` 是它的唯一构造口）——把 dp→px 掺进这张表，
//! 等于让引擎知道屏幕密度，那是 `md3` 抬头里明令不许的第二件事。
//!
//! ## 规则锚
//! 本模块**不写**文档锚点，理由与 `canvas`／`touch`／`md3`／`tui` 那四条相同：排布不新增任何规则真值，
//! 写锚会去动反向覆盖尺的生产面读数。

use crate::md3::{color, lane, layout, shape, state, typo};
use crate::touch::{self, Hand, Layer, Rect, Region, layout_of};
use crate::view::{Board, CardView};

/// 一行字的锚点：`x` 说的是这行字的**左界**还是**右界**。
///
/// SVG 里它是 `text-anchor` 的一个属性值；像素侧没有这个属性，只得起码前把字串量一遍再往回让。
/// 所以锚点必须进这张表——不进，右对齐那几处（牌名角标、回合数、技能串）到了像素侧就静默变成左对齐。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Anchor {
    Start,
    End,
}

/// 描边：颜色＋线宽＋是否虚线，三件一起。
///
/// `dashed` 挂在描边上是**语义**而不是风格：无描边的块（进度条已消耗段、中线色带）在 SVG 里
/// 不带 `stroke-dasharray`，像素侧也就不该有断续可排。
#[derive(Clone, Debug)]
pub(crate) struct Edge {
    pub stroke: String,
    pub width: i32,
    pub dashed: bool,
}

/// 一条呈现指令。**只有两种**：一块矩形、一行字。本帧的盘面确实只用这两种。
#[derive(Clone, Debug)]
pub(crate) enum Prim {
    /// 一块矩形。`fill` 是**已经混完的那串色值**（态层在这张表里就结清了），不是 token 名——
    /// 两个编码器各自再去查表，就是两份态层算术，那是本层要消灭的东西。
    Block {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        radius: i32,
        fill: String,
        edge: Option<Edge>,
        opacity: Option<f64>,
    },
    /// 一行字。`y` 是**基线**，`em` 是 MD3 那档字号（dp 口径），换算成像素归壳。
    Label { x: i32, y: i32, em: i32, fill: String, anchor: Anchor, text: String },
}

/// 一帧：画布尺寸（用户单位）＋按绘制序排好的指令。
///
/// 绘制序有意义：后画的盖在先画的上面，而 `touch::Layout::hit` 正是从尾往头读同一批矩形。
/// 两处的次序若分家，"点在色带上却选中了它底下的格子"这种错就只能靠运气复现。
pub(crate) struct Frame {
    pub width: i32,
    pub height: i32,
    pub prims: Vec<Prim>,
}

/// 有描边的矩形：`stroke` 与 `fill` 一样是定色串。
fn bordered(cell: Rect, fill: &str, radius: i32, stroke: &str, width: i32, dashed: bool) -> Prim {
    Prim::Block {
        x: cell.x,
        y: cell.y,
        w: cell.w,
        h: cell.h,
        radius,
        fill: fill.to_string(),
        edge: Some(Edge { stroke: stroke.to_string(), width, dashed }),
        opacity: None,
    }
}

/// 只有一块面的矩形（进度条已消耗段）：没有描边，也就没有线宽可传。
fn filled(cell: Rect, fill: &str, radius: i32) -> Prim {
    Prim::Block {
        x: cell.x,
        y: cell.y,
        w: cell.w,
        h: cell.h,
        radius,
        fill: fill.to_string(),
        edge: None,
        opacity: None,
    }
}

/// 半透明的面（中线那条带）：`opacity` 是整体不透明度，SVG 走属性、像素侧走混色，两边各自兑现。
fn banded(cell: Rect, fill: &str, radius: i32, opacity: f64) -> Prim {
    Prim::Block {
        x: cell.x,
        y: cell.y,
        w: cell.w,
        h: cell.h,
        radius,
        fill: fill.to_string(),
        edge: None,
        opacity: Some(opacity),
    }
}

fn label(x: i32, y: i32, em: i32, fill: &str, anchor: Anchor, text: &str) -> Prim {
    Prim::Label {
        x,
        y,
        em,
        fill: fill.to_string(),
        anchor,
        text: text.to_string(),
    }
}

/// 一条水平条：底槽满宽，填充按 value/cap 截断（负数与超上限都夹住，不画成溢出）。
fn bar(cell: Rect, value: i32, cap: i32, fill: &str) -> Vec<Prim> {
    let filled_len = if cap <= 0 { 0 } else { value.max(0).min(cap) * cell.w / cap };
    vec![
        bordered(cell, color::SURFACE_CONTAINER, shape::EXTRA_SMALL, color::OUTLINE_VARIANT, layout::STROKE_HAIRLINE, false),
        filled(Rect::new(cell.x, cell.y, filled_len, cell.h), fill, shape::EXTRA_SMALL),
    ]
}

/// 持业者读数：负值夹到 0、见底要标"烛尽"。这不是美化——文本路 `candle_bar` 就是这么印的，
/// 同一状态在两个壳里读出 `-1` 与 `0 烛尽` 两个数，等于悄悄开了第二套呈现口径。
fn candle_readout(hp: i32, cap: i32) -> String {
    let shown = hp.max(0);
    format!("{shown}/{cap}{}", if hp <= 0 { " · 烛尽" } else { "" })
}

/// 值/阈值那一对。生效≠基础时两枚都印（`12 / 4→3`）：只给生效值，玩家看不出这一格正被人削着阈值，
/// 而"为什么这格突然满了"是那条规则的全部后果。相等时不补 `→`——同一对数写两遍是噪声不是信息。
/// 文本路（TUI/CLI）仍只印一枚，那是刻意的口径分叉，理由记在档 §5 D2。
fn threshold_pair(c: &CardView) -> String {
    if c.threshold == c.base_threshold {
        format!("{} / {}", c.hp, c.threshold)
    } else {
        format!("{} / {}→{}", c.hp, c.base_threshold, c.threshold)
    }
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
/// 位置与面积全在 `cell` 里（它就是命中框），本函数只往上摆指令。
/// 描边**不随态层变**：MD3 的状态层盖在容器上，描边是边界，跟着亮会读成"边框也在动"那种新信号。
///
/// 边框在这一帧里有**两条正交的通道**：颜色说"这格是被造出来的"（`lane::CRAFT_EDGE` 金），
/// 粗细说"这格升过级"（`layout::STROKE_EMPHASIS`）。从前两者挤在同一枚 `lane::CRAFT` 名下，
/// 于是想给造牌换个色，就得先问一遍"那右上角那串跟造牌无关的标记怎么办"——一枚 token 两个读者。
fn card_cell(cell: Rect, c: &CardView, fill: &str, edge: &'static str) -> Vec<Prim> {
    let x = cell.x;
    let y = cell.y;
    let w = cell.w;
    let mut out = vec![
        bordered(
            cell,
            fill,
            shape::SMALL,
            if c.crafted { lane::CRAFT_EDGE } else { edge },
            if c.upgrades > 0 { layout::STROKE_EMPHASIS } else { layout::STROKE_BOUNDARY },
            false,
        ),
        label(x + layout::CELL_PAD, y + layout::CELL_ROW1, typo::TITLE_LARGE, color::ON_SURFACE, Anchor::Start, c.name),
        label(
            x + layout::CELL_PAD,
            y + layout::CELL_ROW2,
            typo::HEADLINE_MEDIUM,
            color::ON_SURFACE,
            Anchor::Start,
            &threshold_pair(c),
        ),
        label(
            x + layout::CELL_PAD,
            y + layout::CELL_ROW3,
            typo::BODY_LARGE,
            lane::META,
            Anchor::Start,
            &format!("费{} 焰{}", c.cost, c.flame),
        ),
    ];
    let marks = card_marks(c, true);
    let lit = flame_full(c);
    // 标记占右上角那一格，属性串是右对齐的：不腾出这块宽度，长属性（`战吼:同列+2焰`）会被方块压住尾字。
    if !marks.is_empty() {
        out.push(label(marks_right(cell, lit), y + layout::CELL_ROW1, typo::BODY_LARGE, lane::MARK_TEXT, Anchor::End, &marks.join("·")));
    }
    if !c.skills.is_empty() {
        out.push(label(x + w - layout::CELL_PAD, y + layout::CELL_ROW3, typo::LABEL_LARGE, lane::META, Anchor::End, &c.skills.join("+")));
    }
    if lit {
        out.push(bordered(flame_badge(cell), lane::FLAME, shape::NONE, lane::FLAME, layout::STROKE_HAIRLINE, false));
    }
    out
}

/// 右上角那一串标记的**词表**：在场格与手牌格共用这一支，两处不许各写一份 if 链。
/// `with_trait` 是**宽度**口径而不是词表口径：手牌八格每格净宽 124，ROW1 右侧扣掉 `[i]` 与那枚足额方块
/// 只剩 80.9 单位，最宽的特性串在最省的一档字号下也要 103.8（算术底账见档 §2 正证），所以手牌只摆前三个旗子。
/// 去重按**词**不按相邻：开端牌的 `starter` 与 `trait_label` 会撞成同一个词（快照两个字段、中文一个说法），
/// 而「升级」正好插在两者中间——相邻去重在那一格漏印成「开端·升级+3·开端」（可达性见档 §2 反证 4）。
fn card_marks(c: &CardView, with_trait: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |word: String| {
        if !out.contains(&word) {
            out.push(word);
        }
    };
    if c.crafted {
        push("造".to_string());
    }
    if c.starter {
        push("开端".to_string());
    }
    if c.upgrades > 0 {
        push(format!("升级+{}", c.upgrades));
    }
    if with_trait && c.trait_label != "无" {
        push(c.trait_label.to_string());
    }
    out
}

/// 业火足额：这一格的焰已到阈值。在场格与手牌格读同一条判据，别在两边各写一遍 `>=`。
fn flame_full(c: &CardView) -> bool {
    c.flame >= c.threshold
}

/// 标记串的右界：亮块在场时要让出方块那一格宽（加窄内缩），否则方块压住尾字。
fn marks_right(cell: Rect, lit: bool) -> i32 {
    cell.x + cell.w - if lit { layout::badge_inset() + layout::CELL_PAD_NARROW } else { layout::CELL_PAD }
}

/// 业火足额方块：一个实心小块，不配文字——它是给眼睛看的状态，不是第三条数值。
fn flame_badge(cell: Rect) -> Rect {
    Rect::new(cell.x + cell.w - layout::badge_inset(), cell.y + layout::FLAME_INSET, layout::FLAME_MARK, layout::FLAME_MARK)
}

/// 空格子：虚线框加一个点。它**照样有面积**——"这儿能放"是位置信息，接得住一次命中。
fn empty_cell(cell: Rect, layer: Option<Layer>) -> Vec<Prim> {
    let base = layered(lane::EMPTY_FILL, layer);
    vec![
        bordered(cell, &base, shape::SMALL, color::OUTLINE_VARIANT, layout::STROKE_BOUNDARY, true),
        label(
            cell.x + cell.w / 2,
            cell.y + cell.h / 2 + layout::CELL_PAD_NARROW,
            typo::TITLE_LARGE,
            lane::EMPTY_MARK,
            Anchor::Start,
            "·",
        ),
    ]
}

/// 一横排若干格（空格照画，位置本身就是信息）。
/// 矩形来自几何表、卡来自快照，两者按序并排：长度对不上时静默丢掉一半就是"少画一格不报错"那条老坑，
/// 所以这里当场断言（`debug_assert`），而不是留给目视。
fn card_row(cells: &[(Region, Rect)], cards: &[Option<CardView>], fill: &str, edge: &'static str, marks: &[(Region, Layer)]) -> Vec<Prim> {
    debug_assert_eq!(cells.len(), cards.len(), "几何表与快照的格数不一致：两份盘面");
    let mut out = Vec::new();
    for ((region, cell), card) in cells.iter().zip(cards.iter()) {
        let layer = layer_of(marks, *region);
        out.extend(match card {
            Some(c) => card_cell(*cell, c, &layered(fill, layer), edge),
            None => empty_cell(*cell, layer),
        });
    }
    out
}

/// 手牌行：一格一张，序号就是落点命令里的那个 idx。截断预算来自几何（它是格宽的函数）。
/// 边框那两条通道（金＝造过、粗＝升过）与在场格用**同一枚 token**：结算里刚造出来／刚升上去的那张牌，
/// 接下来几帧大概率还躺在手牌里（放牌要占业力，贵牌常常一整关没上手），只认在场格就等于"证人从不上镜"。
///
/// 右上角那串旗子（造／开端／升级+N）与那枚业火足额方块同理：**在场格看得见、手牌看不见**，而"要不要落这张"
/// 恰恰是落子之前那一帧的事。字号取 `LABEL_MEDIUM` 而非在场格那档 `BODY_LARGE`——不是审美，是 ROW1 只剩 80.9 单位
/// 宽（`[i]` 与方块各占一角），算术底账与口径改口都记在档 §2。特性与技能两串**不在这里补**：它们要的是详情面，
/// 不是往 140 单位的格里塞 103.8 单位的字，去处记在档 §3。
fn hand_row(hand: &Hand, cards: &[CardView], marks: &[(Region, Layer)]) -> Vec<Prim> {
    debug_assert_eq!(hand.cells.len(), cards.len(), "几何表与快照的手牌数不一致：两份盘面");
    let mut out = Vec::new();
    for (i, ((region, cell), c)) in hand.cells.iter().zip(cards.iter()).enumerate() {
        let x = cell.x;
        let y = cell.y;
        let fill = layered(lane::PLAYER_FILL, layer_of(marks, *region));
        let pad = x + layout::CELL_PAD_NARROW;
        out.push(bordered(
            *cell,
            &fill,
            shape::SMALL,
            if c.crafted { lane::CRAFT_EDGE } else { lane::PLAYER_EDGE },
            if c.upgrades > 0 { layout::STROKE_EMPHASIS } else { layout::STROKE_HAIRLINE },
            false,
        ));
        out.push(label(pad, y + layout::HAND_ROW1, typo::LABEL_LARGE, color::ON_PRIMARY_CONTAINER, Anchor::Start, &format!("[{}]", i)));
        out.push(label(pad, y + layout::HAND_ROW2, typo::TITLE_LARGE, color::ON_SURFACE, Anchor::Start, &touch::fit(c.name, hand.name_budget)));
        out.push(label(
            pad,
            y + layout::HAND_ROW3,
            typo::BODY_MEDIUM,
            lane::META,
            Anchor::Start,
            &format!("费{} 值{} 阈{}", c.cost, c.hp, c.threshold),
        ));
        if c.flame > 0 {
            out.push(label(
                x + cell.w - layout::CELL_PAD_NARROW,
                y + layout::HAND_ROW3,
                typo::BODY_MEDIUM,
                lane::FLAME,
                Anchor::End,
                &format!("焰{}", c.flame),
            ));
        }
        let lit = flame_full(c);
        let flags = card_marks(c, false);
        if !flags.is_empty() {
            out.push(label(marks_right(*cell, lit), y + layout::HAND_ROW1, typo::LABEL_MEDIUM, lane::MARK_TEXT, Anchor::End, &touch::fit(&flags.join("·"), hand.marks_budget)));
        }
        if lit {
            out.push(bordered(flame_badge(*cell), lane::FLAME, shape::NONE, lane::FLAME, layout::STROKE_HAIRLINE, false));
        }
    }
    out
}

/// 一帧盘面 → 一张呈现指令表。**纯函数、零 IO**；`marks` 是控件层要盖的态层（`touch::Picker::marks`，
/// 按优先级升序；空表 ⇒ 与无控件层**逐字节**等价，那件事由 `touch` 那两条钉着）。
///
/// 画布高度**不写死**——它由 `layout_of` 从"这一帧实际排到哪儿"推出来：Boss 规则句比盘面任何一行都长，
/// 钉死高度就等于要么留一截空白、要么把那句挤成看不见的半句。
///
/// `seed` 与 `scene` 只进标题行：帧文件要能自证是哪一桌打出来的，否则一堆图摆在一起无人能对上号。
pub(crate) fn frame_prims(v: &Board, seed: u64, scene: &str, marks: &[(Region, Layer)]) -> Frame {
    let l = layout_of(v);
    let mut prims: Vec<Prim> = Vec::new();

    prims.push(label(
        l.left_x,
        l.title_y,
        typo::HEADLINE_SMALL,
        color::ON_SURFACE,
        Anchor::Start,
        &format!("《中线》2D 帧 · {scene} · seed {seed}"),
    ));
    prims.push(label(
        l.right_x,
        l.title_y,
        typo::HEADLINE_SMALL,
        lane::META,
        Anchor::End,
        &format!("第 {} 回合", v.player.turn),
    ));

    for (line, baseline) in &l.rule {
        prims.push(label(l.left_x, *baseline, typo::TITLE_LARGE, lane::MIDLINE, Anchor::Start, line));
    }

    // 敌方持业者 1~2 根并排：业力只在第一根（共用一池），第二根带受击列区。
    for (holder, h) in l.holders.iter().zip(v.enemy_holders.iter()) {
        let mut text = format!("{} {}", h.name, candle_readout(h.hp, h.cap));
        if let Some(k) = h.karma {
            text.push_str(&format!(" · 业力 {}", k));
        }
        if let Some((a, b)) = h.column_zone {
            text.push_str(&format!(" · 第{}-{}列直击此柱", a, b));
        }
        prims.push(label(holder.x, holder.y, typo::TITLE_LARGE, lane::ENEMY_FRONT_EDGE, Anchor::Start, &text));
        prims.extend(bar(holder.rect(), h.hp, h.cap, lane::ENEMY_FRONT_EDGE));
    }

    prims.push(label(l.left_x, l.back.label_y, typo::BODY_LARGE, color::OUTLINE_VARIANT, Anchor::Start, "敌方后排 · 不可攻击"));
    prims.extend(card_row(&l.back.cells, &v.enemy_back, lane::ENEMY_BACK_FILL, color::OUTLINE_VARIANT, marks));
    prims.push(label(l.left_x, l.enemy.label_y, typo::BODY_LARGE, lane::ENEMY_FRONT_EDGE, Anchor::Start, "敌方前排 · 可攻击"));
    prims.extend(card_row(&l.enemy.cells, &v.enemy_front, lane::ENEMY_FRONT_FILL, lane::ENEMY_FRONT_EDGE, marks));

    // 中线：一条实色带，不是描边——它要的是"过不去"那种视觉重量，描边给不了。
    prims.push(banded(l.midline, lane::MIDLINE, shape::NONE, lane::MIDLINE_BAND_SPOT));
    prims.push(label(
        l.center_x,
        l.midline_text_y,
        typo::BODY_MEDIUM,
        lane::ON_MIDLINE,
        Anchor::Start,
        "中线 · 绝对边界（任何卡牌不能越过）",
    ));

    prims.push(label(l.left_x, l.player.label_y, typo::BODY_LARGE, lane::PLAYER_EDGE, Anchor::Start, "我方 P1-P4 · 全部可攻击"));
    prims.extend(card_row(&l.player.cells, &v.player_front, lane::PLAYER_FILL, lane::PLAYER_EDGE, marks));

    // 我方持业者：与敌方对称，同一支 bar、同一套读数口径。
    let mut me = format!("我方持业者 {}", candle_readout(v.player.hp, v.player.cap));
    me.push_str(&format!(" · 业力 {}", v.player.karma));
    prims.push(label(l.player_holder.x, l.player_holder.y, typo::TITLE_LARGE, lane::PLAYER_EDGE, Anchor::Start, &me));
    prims.extend(bar(l.player_holder.rect(), v.player.hp, v.player.cap, lane::PLAYER_EDGE));

    // 「结束回合」：人裁 ③ 那一枚，占持业者同一横带的**右端**（左边是柱与读数，那块本来就空着）。
    // 态层只动容器色、不动描边——与 `card_cell` 同口径，否则按下会多读出一句"边框也在动"。
    prims.push(bordered(l.end_turn, &layered(color::PRIMARY_CONTAINER, layer_of(marks, Region::EndTurn)), shape::FULL, color::PRIMARY, layout::STROKE_BOUNDARY, false));
    prims.push(label(l.end_turn_text_x, l.end_turn_text_y, typo::LABEL_LARGE, color::ON_PRIMARY_CONTAINER, Anchor::Start, touch::END_TURN_LABEL));

    prims.push(label(
        l.left_x,
        l.hand.label_y,
        typo::BODY_LARGE,
        lane::META,
        Anchor::Start,
        &format!("手牌 {} 张（序号＝落点命令里的 idx）", v.hand.len()),
    ));
    match l.hand.placeholder {
        Some(place) => prims.push(label(place.x, place.y, typo::BODY_LARGE, color::OUTLINE_VARIANT, Anchor::Start, "（空）")),
        None => prims.extend(hand_row(&l.hand, &v.hand, marks)),
    }

    prims.push(label(
        l.left_x,
        l.piles_y,
        typo::BODY_LARGE,
        lane::META,
        Anchor::Start,
        &format!("继承堆 {} 张（自动抽） · 开端堆 {} 张", v.draw_pile_len, v.starter_pile_len),
    ));

    // 日志尾：引擎日志本来就是文本，这里只截行，转义留给 SVG 那侧（像素侧本来就没有标记语言）。
    prims.push(bordered(l.log, color::SURFACE_CONTAINER, shape::SMALL, color::OUTLINE_VARIANT, layout::STROKE_HAIRLINE, false));
    let start = v.log.len().saturating_sub(layout::LOG_LINES);
    for (i, entry) in v.log[start..].iter().enumerate() {
        let line_y = l.log_first_y + i as i32 * l.log_step;
        prims.push(label(l.log_text_x, line_y, typo::BODY_MEDIUM, color::ON_SURFACE, Anchor::Start, &touch::fit(entry.trim_start(), l.log_budget)));
    }

    Frame {
        width: l.width,
        height: l.height,
        prims,
    }
}

#[cfg(test)]
mod tests {
    use super::layout::*;
    use super::typo::*;
    use super::*;
    use crate::battle::{Battle, DailyRule, Difficulty, Row, HAND_LIMIT};
    use crate::boss::BossId;
    use crate::model::{CardInst, Faction, Skill, faction_cards};

    fn normal() -> Battle {
        Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1)
    }

    fn frame_of(v: &Board) -> Frame {
        frame_prims(v, 7, "t", &[])
    }

    /// 底槽块：满宽满高**且带描边**的那一类（`bar` 的两条里只有底槽有描边，已消耗段是纯面）。
    /// 只按尺寸筛会连"血量正好满"那一条已消耗段一起数进去——那是同一根柱的第二次读数，不是第二根柱。
    fn bar_tracks(f: &Frame) -> usize {
        f.prims
            .iter()
            .filter(|p| matches!(p, Prim::Block { w, h, edge, .. } if *w == BAR_W && *h == BAR_H && edge.is_some()))
            .count()
    }

    /// 块（按尺寸筛）与字的计数都直接读指令表，不经任何序列化——这一层管的是"画了什么"，
    /// 拿 SVG 串去数会把编码器的形状误记成决策的形状。
    fn blocks(f: &Frame) -> Vec<(i32, i32, i32, i32)> {
        f.prims
            .iter()
            .filter_map(|p| match p {
                Prim::Block { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
                Prim::Label { .. } => None,
            })
            .collect()
    }

    fn texts(f: &Frame) -> Vec<String> {
        f.prims
            .iter()
            .filter_map(|p| match p {
                Prim::Label { text, .. } => Some(text.clone()),
                Prim::Block { .. } => None,
            })
            .collect()
    }

    /// 烛的读数与文本路同口径：负值夹到 0、见底标"烛尽"。
    /// 引擎在超额削烛那一刻确实把 `-1` 交出来（帧 r7 末格实测），原样印上去就是拿一个眼睛读不懂的数当呈现。
    #[test]
    fn the_candle_readout_clamps_below_zero_and_marks_the_extinguished_wick() {
        assert_eq!(candle_readout(14, 20), "14/20", "常态读数不加任何装饰");
        assert_eq!(candle_readout(0, 20), "0/20 · 烛尽", "见底要标出来");
        assert_eq!(candle_readout(-1, 20), "0/20 · 烛尽", "负值夹到 0，不把超额伤害原样画出去");
    }

    /// 业火足额标记：**恰好**在 flame ≥ threshold 时出现，一格一块。
    /// 这条钉的是"表上那个记号真的对应引擎那两个数"，不是"记号好不好看"。
    #[test]
    fn the_flame_marker_appears_exactly_when_the_cell_is_at_threshold() {
        let mut v = normal().view();
        assert_eq!(marker_blocks(&frame_of(&v)).len(), 0, "空盘没有可标的格");

        let mut lit = CardView { name: "足", cost: 1, hp: 5, threshold: 3, base_threshold: 3, flame: 3, upgrades: 0, crafted: false, starter: false, trait_label: "无", skills: vec![] };
        v.player_front[0] = Some(lit.clone());
        assert_eq!(marker_blocks(&frame_of(&v)).len(), 1, "焰＝阈 算足额");

        lit.flame = 2;
        v.player_front[1] = Some(lit);
        assert_eq!(marker_blocks(&frame_of(&v)).len(), 1, "焰<阈 不许亮");

        let other = CardView { name: "差", cost: 1, hp: 5, threshold: 9, base_threshold: 9, flame: 0, upgrades: 0, crafted: false, starter: false, trait_label: "无", skills: vec![] };
        v.enemy_front[0] = Some(other);
        assert_eq!(marker_blocks(&frame_of(&v)).len(), 1, "敌方格也走同一支笔画，不许多出一块");
    }

    /// 生效≠基础时那一格要**同时**给出两枚数（`12 / 4→3`），相等时不许多那道箭头。
    /// 只印生效值，玩家看不出这格正被人削着阈值——而"这格怎么突然就满了"正是那条改阈值的后果。
    #[test]
    fn the_cell_prints_both_thresholds_only_when_the_effective_one_moves() {
        let mut v = normal().view();
        let moved = CardView { name: "削", cost: 1, hp: 12, threshold: 3, base_threshold: 4, flame: 0, upgrades: 0, crafted: false, starter: false, trait_label: "无", skills: vec![] };
        v.player_front[0] = Some(moved.clone());
        v.player_front[1] = Some(CardView { threshold: 4, ..moved });
        let t = texts(&frame_of(&v));
        assert!(t.iter().any(|s| s == "12 / 4→3"), "生效≠基础那一格没成一对：{t:?}");
        assert!(t.iter().any(|s| s == "12 / 4"), "两枚相等那一格该只印一枚：{t:?}");
        assert!(!t.iter().any(|s| s.contains("4→4")), "相等那格不许补那道箭头");
    }

    /// 走引擎真路的证人：**业火狂潮**把阈值减半 ⇒ 那一格在帧上必须成对。
    /// 上一条手搭快照、钉打印规则；这一条钉"这对数在真实盘面上会出现"——出现不了的呈现等于没呈现
    /// （其七 D4 那枚金边就是这么抓出来的：画法一直在，通路从来没有）。
    #[test]
    fn the_daily_flame_tide_makes_the_cell_print_a_pair() {
        let def = *faction_cards(Faction::Ember)
            .iter()
            .find(|d| d.threshold >= 2)
            .expect("牌池里没有阈值≥2 的牌 ⇒ 减半这一族在盘面上无从谈起，本条要改前提");
        let base = def.threshold;
        let mut b = normal().with_daily_rule(DailyRule::FlameTide);
        b.enemy_place_inst(CardInst::new(900, def), 0, Row::Front)
            .expect("这张牌该落得进敌方前排");
        let v = b.view();
        let cell = v.enemy_front[0].as_ref().expect("落子之后格里要有牌");
        assert_eq!(cell.base_threshold, base, "基础值仍是牌面那个数：减半不许改写它");
        assert_eq!(cell.threshold, (base / 2).max(1), "生效值＝减半");
        let want = format!("{} / {}→{}", cell.hp, base, cell.threshold);
        let t = texts(&frame_of(&v));
        assert!(t.iter().any(|s| s == &want), "帧上那一格没成对：想要 {want}，实测 {t:?}");
    }

    /// 技能串那行画法的**唯一**帧证人，走发牌真路：`Battle::new` 里 `mk(…, true)` 给每张非开端牌抽一支
    /// 附加技能，快照 `card_view` 折成 label 列表，`card_cell` 再 `join("+")` 印出去——三段都在，
    /// 可 2026-10-10 那张清单复验到今日才发现：这一格九个 CardView 夹具与 canvas 夹具全用 `skills: vec![]`，
    /// 电池 M32/M125 都砍在引擎层，所以**画法一直在、证人从来没有**（手牌行 `hand_row` 又不画这行，连"顺手带过"的机会都没有）。
    /// 第一支的期望串从快照现拿，第二支向 `Skill::label()` 现取——两处都不抄字面量，抄了就是给某一份旧读数背书。
    #[test]
    fn a_skill_bearing_card_placed_by_the_engine_prints_its_string_on_the_cell() {
        let mut b = normal();
        b.p_karma = 99; // 只买放置那道业力闸的通路，本条不测业力
        let opener = b.view();
        let idx = opener
            .hand
            .iter()
            .position(|c| !c.skills.is_empty())
            .expect("开局手牌里没有一张带技能的牌 ⇒ 发牌抽技能那一路断了，本条要改前提");
        let want = opener.hand[idx].skills.join("+");
        drop(opener);

        b.player_place(idx, 0).expect("业力已给足，这张牌该落得进我方前排");
        let mut v = b.view();
        assert_eq!(v.player_front[0].as_ref().expect("落子之后格里要有那张牌").skills.join("+"), want, "落子路上快照把技能串改了样：帧上的差异不该记到呈现层头上");
        let one = texts(&frame_of(&v));
        assert!(one.contains(&want), "帧上没有引擎抽给这张牌的技能串 {want}：{one:?}");

        // 叠加那一维只有真路撞得出来：第 4 章「技能强化」给同一张牌再追加一支（常常是同一支叠两遍），
        // 于是帧上读的是**一串**而不是一个。分隔符在这里写死——它是玩家数得清"这牌有几个技能"的唯一依据，
        // 换成点或逗号也算改了口径，不许以"版式调整"名义漂走。
        let second = Skill::AllyColAtk1.label();
        v.player_front[0].as_mut().expect("格子里那张牌还在").skills.push(second);
        let pair = format!("{want}+{second}");
        let two = texts(&frame_of(&v));
        assert!(two.contains(&pair), "补上第二支之后帧上没成串：想要 {pair}，实测 {two:?}");
        assert!(!two.contains(&want), "帧上只剩第一支 ⇒ 叠加的技能不可见，强化那一路在画面上等于没发生");

        // 负半：同一格清空技能，那一行必须跟着消失——摘掉 `if !c.skills.is_empty()` 那层守卫时这里当场红
        // （空串也是一种"画了"，所以断的是少一行，不是少一个字符）。
        v.player_front[0].as_mut().expect("还在").skills.clear();
        let none = texts(&frame_of(&v));
        assert!(!none.iter().any(|s| s == &pair), "技能清空后帧上还印着 {pair} ⇒ 那行字不是读快照");
        assert_eq!(two.len(), none.len() + 1, "清空技能只该少那一行字，别并着别的改动一起进来");
    }

    /// 足额块由 `FLAME_MARK` 认，不抄字面量——否则改了 token 这条测照样绿，就成了给旧读数背书。
    fn marker_blocks(f: &Frame) -> Vec<(i32, i32)> {
        blocks(f)
            .into_iter()
            .filter(|(_, _, w, h)| *w == FLAME_MARK && *h == FLAME_MARK)
            .map(|(x, y, _, _)| (x, y))
            .collect()
    }

    /// 业火方块占右上角，属性串是右对齐的 ⇒ 锚点必须让到方块左界以左，否则长属性（`战吼:同列+2焰`）
    /// 的尾字会被方块压住——这一条就是那次压字事故的牙：锚点只要往回挪到常态那个数，断言当场红。
    #[test]
    fn the_flame_badge_never_sits_on_top_of_the_trait_text() {
        let lit = CardView { name: "火星", cost: 2, hp: 1, threshold: 3, base_threshold: 3, flame: 3, upgrades: 0, crafted: false, starter: false, trait_label: "战吼:同列+2焰", skills: vec![] };
        let dim = CardView { flame: 0, ..lit.clone() };
        let cell_w = 300;
        let cell = Rect::new(0, 0, cell_w, CELL_H);

        let out = card_cell(cell, &lit, lane::PLAYER_FILL, lane::PLAYER_EDGE);
        let badge_x = marker_of(&out).expect("足额格要画方块").0;
        let marks_x = trait_label(&out).expect("有属性串就要画出来");
        assert!(marks_x <= badge_x, "属性串右界 {marks_x} 不得越过方块左界 {badge_x}（越过去＝方块压在字上）");

        // 不点亮时不该白让这块宽度：字要贴着常态的右边距。
        let out = card_cell(cell, &dim, lane::PLAYER_FILL, lane::PLAYER_EDGE);
        assert_eq!(marker_of(&out), None, "没点亮就不该画方块");
        assert_eq!(trait_label(&out).expect("属性串要画出来"), cell_w - CELL_PAD, "没点亮就不该让位");
    }

    fn marker_of(prims: &[Prim]) -> Option<(i32, i32)> {
        prims.iter().find_map(|p| match p {
            Prim::Block { x, y, w, h, .. } if *w == FLAME_MARK && *h == FLAME_MARK => Some((*x, *y)),
            _ => None,
        })
    }

    /// 属性串是 `Anchor::End` 那几行里第一个 `·` 之外带冒号的那行——本测只关心它的 `x`。
    fn trait_label(prims: &[Prim]) -> Option<i32> {
        prims.iter().find_map(|p| match p {
            Prim::Label { x, anchor: Anchor::End, text, .. } if text.contains(':') => Some(*x),
            _ => None,
        })
    }

    /// 开端牌不许把"开端"印两遍：快照里 `starter` 与 `trait_label` 是两个字段、中文是同一个说法，
    /// 各自往右上角推一次就成 `开端 · 开端`（首版实测就是这样，交付图上一眼可见）。
    #[test]
    fn a_starter_card_says_it_once_not_twice() {
        let starter = CardView { name: "余烬", cost: 0, hp: 1, threshold: 4, base_threshold: 4, flame: 0, upgrades: 0, crafted: false, starter: true, trait_label: "开端", skills: vec![] };
        let out = card_cell(Rect::new(0, 0, 300, CELL_H), &starter, lane::PLAYER_FILL, lane::PLAYER_EDGE);
        assert_eq!(marks_joined(&out), Some("开端".to_string()), "同一个词印两遍＝噪声：{out:?}");

        // 反面对照：不同来源的两个词该都留下，不许把 dedup 写成"只留第一个标记"。
        let both = CardView { name: "余烬", cost: 0, hp: 1, threshold: 4, base_threshold: 4, flame: 0, upgrades: 1, crafted: true, starter: false, trait_label: "战吼:同列+2焰", skills: vec![] };
        let out = card_cell(Rect::new(0, 0, 300, CELL_H), &both, lane::PLAYER_FILL, lane::PLAYER_EDGE);
        assert_eq!(marks_joined(&out).as_deref(), Some("造·升级+1·战吼:同列+2焰"), "三个不同来源的标记要连起来全在");
    }

    /// 边框的两条通道：**色**说"这格是被造出来的"、**粗细**说"这格升过级"。
    /// 从前两条通道挤在同一枚 `lane::CRAFT` 名下——给造牌换金色那天，会把跟造牌毫无关系的
    /// 「开端」「战吼:同列+2焰」一起染了（档 §4 记的就是这次拒绝）。分开之后四格各自独立：
    /// 没造没升走本侧原色，只造换色，只升换粗细，两者都有则两笔同时读到。
    #[test]
    fn the_cell_border_carries_craft_in_colour_and_upgrade_in_weight() {
        let plain = CardView { name: "余烬", cost: 1, hp: 2, threshold: 4, base_threshold: 4, flame: 0, upgrades: 0, crafted: false, starter: false, trait_label: "无", skills: vec![] };
        let cell = Rect::new(0, 0, 300, CELL_H);
        let border = |c: &CardView| cell_border(&card_cell(cell, c, lane::PLAYER_FILL, lane::PLAYER_EDGE));

        assert_eq!(border(&plain), (lane::PLAYER_EDGE.to_string(), STROKE_BOUNDARY), "什么都没发生的格子就该是本侧原色原粗");
        assert_eq!(border(&CardView { crafted: true, ..plain.clone() }), (lane::CRAFT_EDGE.to_string(), STROKE_BOUNDARY), "造过只换色");
        assert_eq!(border(&CardView { upgrades: 1, ..plain.clone() }), (lane::PLAYER_EDGE.to_string(), STROKE_EMPHASIS), "升过只换粗细");
        assert_eq!(border(&CardView { crafted: true, upgrades: 2, ..plain.clone() }), (lane::CRAFT_EDGE.to_string(), STROKE_EMPHASIS), "两件事都发生 ⇒ 两条通道同时读得到");
    }

    /// 拆 token 这一步是不是**真拆**了：金只许落在边框上，右上角那串字不许跟着变。
    /// 这条有两条牙——① 两枚 token 一旦并回一枚，`assert_ne` 当场红；
    /// ② `MARK_TEXT` 的**值**被钉成旧的那枚紫：改名可以，顺手把读数一起换掉不行（帧上别的格子会跟着变脸）。
    #[test]
    fn the_gold_reaches_the_border_only_and_never_the_mark_words() {
        assert_eq!(lane::CRAFT_EDGE, color::SECONDARY, "金边取的就是表里那枚金，不是又写了一个十六进制");
        assert_eq!(lane::MARK_TEXT, color::TERTIARY, "标记墨色是**改名**不是改读数");
        assert_ne!(lane::CRAFT_EDGE, lane::MARK_TEXT, "两枚 token 又并成一枚了：拆 token 那一步没真做");

        let starter = CardView { name: "余烬", cost: 0, hp: 1, threshold: 4, base_threshold: 4, flame: 0, upgrades: 0, crafted: false, starter: true, trait_label: "战吼:同列+2焰", skills: vec![] };
        let out = card_cell(Rect::new(0, 0, 300, CELL_H), &starter, lane::PLAYER_FILL, lane::PLAYER_EDGE);
        assert_eq!(marks_fill(&out).as_deref(), Some(lane::MARK_TEXT), "没造过的格子的标记串不许染金：{out:?}");
        assert_eq!(cell_border(&out).0, lane::PLAYER_EDGE.to_string(), "这格没造过，边框就该是本侧原色");
    }

    /// 档 §3 给"升级"这一格立的判据：**遮住文字也认得出**。所以这一条只比两块格子的**面**，
    /// 一个字都不看——若升级只留下 `升级+1` 那行字，下面的 `assert_ne` 就还是绿的，那才是真出事。
    #[test]
    fn an_upgraded_cell_is_tellable_from_its_plain_twin_with_the_words_covered() {
        let plain = CardView { name: "同", cost: 1, hp: 2, threshold: 4, base_threshold: 4, flame: 0, upgrades: 0, crafted: false, starter: false, trait_label: "无", skills: vec![] };
        let up = CardView { upgrades: 3, ..plain.clone() };
        let cell = Rect::new(0, 0, 300, CELL_H);
        let faces = |c: &CardView| cell_faces(&card_cell(cell, c, lane::PLAYER_FILL, lane::PLAYER_EDGE));
        assert_eq!(faces(&plain).len(), faces(&up).len(), "两块格子的面数该一样（升级不加块，只改那一圈的粗细）");
        assert_ne!(faces(&plain), faces(&up), "遮住字之后两块面完全相同＝升级在帧上只剩一行字");
        assert_eq!(cell_border(&card_cell(cell, &up, lane::PLAYER_FILL, lane::PLAYER_EDGE)).1, STROKE_EMPHASIS, "粗的那一档要真是 STROKE_EMPHASIS");
        assert_eq!(cell_border(&card_cell(cell, &plain, lane::PLAYER_FILL, lane::PLAYER_EDGE)).1, STROKE_BOUNDARY, "另一块该是常态那一档");
    }

    /// 格子那块底：`card_cell` 的第一条指令，读它的 (描边色, 线宽)。取不到就 panic，
    /// 免得哪天顺序变了，上面几条测悄悄变成"没有边框可比"的空转。
    fn cell_border(prims: &[Prim]) -> (String, i32) {
        match prims.first() {
            Some(Prim::Block { edge: Some(e), .. }) => (e.stroke.clone(), e.width),
            other => panic!("格子的第一条指令该是那块带描边的底，读到 {other:?}"),
        }
    }

    /// 手牌行也带这两条通道（金＝造过、粗＝升过）。这一格不是顺手加的：其七 D4 那 46 帧实测，
    /// 结算里刚造出来／刚升上去的那张牌，**其后每一帧都躺在手牌里**（放牌要占业力，贵牌常常一整关没上手），
    /// 只让在场格认得到的话，金边在交付帧里就是 0 命中——通道开了，证人却从不上镜。
    /// 四格对照同时钉住"手牌没有第三种读法"：那圈线不许变成别的颜色或别的粗细。
    #[test]
    fn the_two_border_channels_reach_the_hand_row_too() {
        let mut v = normal().view();
        assert!(v.hand.len() >= 4, "手牌不足四张就摆不出四格对照：实得 {}", v.hand.len());
        let plain = v.hand[0].clone();
        v.hand[0] = CardView { crafted: false, upgrades: 0, ..plain.clone() };
        v.hand[1] = CardView { crafted: true, upgrades: 0, ..plain.clone() };
        v.hand[2] = CardView { crafted: false, upgrades: 2, ..plain.clone() };
        v.hand[3] = CardView { crafted: true, upgrades: 3, ..plain };
        let hand = layout_of(&v).hand;
        let prims = hand_row(&hand, &v.hand, &[]);

        let borders = hand_borders(&prims);
        assert_eq!(borders.len(), v.hand.len(), "一格一圈边：边数与手牌数对不上就是有人在漏画或并画");
        assert_eq!(borders[0], (lane::PLAYER_EDGE.to_string(), STROKE_HAIRLINE), "没造没升的手牌就该是本侧原色那一圈");
        assert_eq!(borders[1], (lane::CRAFT_EDGE.to_string(), STROKE_HAIRLINE), "造过只换色");
        assert_eq!(borders[2], (lane::PLAYER_EDGE.to_string(), STROKE_EMPHASIS), "升过只换粗细");
        assert_eq!(borders[3], (lane::CRAFT_EDGE.to_string(), STROKE_EMPHASIS), "两件事都发生 ⇒ 两条通道同时读得到");

        // 金只许落在边框上：手牌那三行字（序号／牌名／费值阈）里有一行染金就是又并回一枚 token 了。
        assert!(
            prims.iter().all(|p| !matches!(p, Prim::Label { fill, .. } if fill == lane::CRAFT_EDGE)),
            "手牌里出现了金字：{prims:?}"
        );
    }

    /// 手牌行里每格那一圈边：按序取出 (描边色, 线宽)，第 i 块对应第 i 张手牌。
    /// 认的是**满格高**那块底——足额那枚方块也带边（与在场格同一写法），它不是边框通道，混进来
    /// 就会把"一格一圈边"读成"一格两圈边"。
    fn hand_borders(prims: &[Prim]) -> Vec<(String, i32)> {
        prims
            .iter()
            .filter_map(|p| match p {
                Prim::Block { h, edge: Some(e), .. } if *h == HAND_H => Some((e.stroke.clone(), e.width)),
                _ => None,
            })
            .collect()
    }

    /// 手牌某一格右上角那串旗子（认 `MARK_TEXT` ＋右锚＋`LABEL_MEDIUM` 三件一起命中，与在场格那串差一档字号）。
    /// 按格子矩形认领，不按出现次序：不是每张牌都有旗子，序会自己错位。
    fn hand_flags_at(prims: &[Prim], cell: &Rect) -> Option<String> {
        prims.iter().find_map(|p| match p {
            Prim::Label { x, y, em, fill, anchor, text }
                if *anchor == Anchor::End && *em == LABEL_MEDIUM && fill == lane::MARK_TEXT && *x >= cell.x && *x <= cell.x + cell.w && *y >= cell.y && *y <= cell.y + cell.h =>
                Some(text.clone()),
            _ => None,
        })
    }

    /// 手牌格右上角那串旗子：在场格摆的词，手牌一格不许少摆、也不许多摆特性那一串。
    /// 多摆的那一串本来就该出局——ROW1 右侧扣掉 `[i]` 与足额方块只剩 `80.9` 单位，最宽特性串`103.8`（档 §2 正证）。
    #[test]
    fn the_hand_row_prints_the_flag_words_the_board_row_prints() {
        let mut v = normal().view();
        let base = v.hand[0].clone();
        v.hand[0] = CardView { crafted: false, starter: false, upgrades: 0, trait_label: "战吼:同列+2焰", flame: 0, ..base.clone() };
        v.hand[1] = CardView { crafted: true, starter: false, upgrades: 0, trait_label: "战吼:同列+2焰", flame: 0, ..base.clone() };
        v.hand[2] = CardView { crafted: false, starter: true, upgrades: 3, trait_label: "开端", flame: 0, ..base.clone() };
        v.hand[3] = CardView { crafted: false, starter: false, upgrades: 2, trait_label: "无", flame: 0, ..base };
        let hand = layout_of(&v).hand;
        let prims = hand_row(&hand, &v.hand, &[]);
        let got: Vec<String> = hand.cells.iter().map(|(_, c)| hand_flags_at(&prims, c).unwrap_or_default()).collect();
        assert_eq!(got[..4], ["", "造", "开端·升级+3", "升级+2"], "手牌那串旗子与在场格不同词表，或多摆/漏摆了某张：{got:?}");
        assert!(
            got.iter().all(|g| !g.contains("战吼")),
            "特性串挤进手牌那一格了（它要的是详情面，不是这 140 单位）：{got:?}"
        );
    }

    /// 业火足额那枚方块：同一张牌摆在在场格与摆在手牌格，方块**大小与内缩**必须一模一样，
    /// 否则"满了"这件事在两处读出两个尺寸。顺带钉住未到阈那一格不画块（它不该凭`焰>0`就亮）。
    #[test]
    fn the_full_flame_badge_is_the_same_square_in_both_rows() {
        let mut v = normal().view();
        let base = v.hand[0].clone();
        let lit = CardView { flame: base.threshold, ..base.clone() };
        let dim = CardView { flame: base.threshold - 1, ..base.clone() };
        let badge = |prims: &[Prim], cell: &Rect| -> Option<(i32, i32, i32)> {
            prims.iter().find_map(|p| match p {
                Prim::Block { x, w, h, fill, .. } if fill == lane::FLAME && *w == FLAME_MARK && x + w > cell.x => Some((*w, *h, cell.x + cell.w - (x + w))),
                _ => None,
            })
        };
        v.hand[0] = lit.clone();
        let hand = layout_of(&v).hand;
        let cell = Rect::new(0, 0, 288, CELL_H);
        let on_board = badge(&card_cell(cell, &lit, lane::PLAYER_FILL, lane::PLAYER_EDGE), &cell).expect("在场格那枚足额方块没画");
        let in_hand = badge(&hand_row(&hand, &v.hand, &[]), &hand.cells[0].1).expect("手牌那枚足额方块没画");
        assert_eq!(in_hand, on_board, "手牌与在场格读出的足额方块不是同一块：手牌 {in_hand:?}／在场格 {on_board:?}");

        v.hand[0] = dim;
        let hand = layout_of(&v).hand;
        assert!(badge(&hand_row(&hand, &v.hand, &[]), &hand.cells[0].1).is_none(), "焰差一格就到阈也画了方块：这一格的判据不是「有焰」而是「到阈」");
    }

    /// 手牌那一格里的每一行字都得有地方站：ROW1 那三样（`[i]`、旗子串、足额方块）按左右次序不许相接，
    /// 且整排不许越出格子。这把尺是本帧立的——在此之前"补字会不会压住别的字"全凭眼看。
    /// 两档桌面对照：今日可达的最坏串**不许**被截断；人造的超预算串**必须**被截住（否则这条检与那道 `fit` 都在空转）。
    #[test]
    fn nothing_in_a_hand_cell_overlaps_its_neighbours() {
        fn worst(base: &CardView, upgrades: u8) -> CardView {
            CardView { starter: true, upgrades, trait_label: "开端", flame: base.threshold, threshold: base.threshold, base_threshold: base.threshold, ..base.clone() }
        }
        for (tag, upgrades, want_ellipsis) in [("今日可达最坏（开端＋升级三档＋焰已到阈）", 3u8, false), ("人造超预算（三位数档数，今天不可达）", 255u8, true)] {
            let mut v = normal().view();
            let base = v.hand[0].clone();
            // 摆满 8 格：`hw` 是手牌数的函数，少于 8 就量不到最窄那一档（`HAND_LIMIT` 是上限，开局手牌只有几张）。
            v.hand = (0..HAND_LIMIT).map(|_| worst(&base, upgrades)).collect();
            let hand = layout_of(&v).hand;
            let prims = hand_row(&hand, &v.hand, &[]);
            let mut saw_ellipsis = false;
            for (i, (_, cell)) in hand.cells.iter().enumerate() {
                let flags = hand_flags_at(&prims, cell).unwrap_or_else(|| panic!("{tag}：第 {i} 格连旗子都没画，这条检空转"));
                saw_ellipsis |= flags.contains('…');
                let idx_lo = cell.x + CELL_PAD_NARROW;
                let idx_hi = idx_lo + (touch::display_width(&format!("[{i}]")) * LABEL_LARGE as f64) as i32;
                let flags_hi = cell.x + cell.w - (badge_inset() + CELL_PAD_NARROW);
                let flags_lo = flags_hi - (touch::display_width(&flags) * LABEL_MEDIUM as f64) as i32;
                let badge_lo = cell.x + cell.w - badge_inset();
                let badge_hi = badge_lo + FLAME_MARK;
                for (name, lo, hi) in [("[i]", idx_lo, idx_hi), ("旗子", flags_lo, flags_hi), ("方块", badge_lo, badge_hi)] {
                    assert!(lo >= cell.x && hi <= cell.x + cell.w, "{tag}：第 {i} 格的「{name}」越出格子（{lo}..{hi} 不在 {}..{}）", cell.x, cell.x + cell.w);
                }
                assert!(idx_hi <= flags_lo, "{tag}：第 {i} 格「[i]」压到旗子（[i] 右界 {idx_hi} ／ 旗子左界 {flags_lo}）");
                assert!(flags_hi <= badge_lo, "{tag}：第 {i} 格旗子压到足额方块（旗子右界 {flags_hi} ／ 方块左界 {badge_lo}）");
            }
            assert_eq!(saw_ellipsis, want_ellipsis, "{tag}：省略号该不该出现读错了（今日词表不许截断，超预算那一档必须真截住）");
        }
    }

    /// 开端牌升过级：在场格那串不许再印两遍「开端」。旧写法用相邻去重，而`升级`正好插在两者中间，
    /// 于是码面上会出「开端·升级+2·开端」——可达性见档 §2 反证 4（`upgrade_card` 三道闸不查 `is_starter()`）。
    #[test]
    fn the_board_row_does_not_print_opening_twice_when_the_starter_is_upgraded() {
        let v = normal().view();
        let c = CardView { starter: true, upgrades: 2, trait_label: "开端", flame: 0, ..v.hand[0].clone() };
        let joined = marks_joined(&card_cell(Rect::new(0, 0, 288, CELL_H), &c, lane::PLAYER_FILL, lane::PLAYER_EDGE)).expect("在场格那串标记没画");
        assert_eq!(joined, "开端·升级+2", "「开端」在同一串里出现两次（或去重把别的词一起吃掉）：{joined}");
    }

    fn cell_faces(prims: &[Prim]) -> Vec<(i32, i32, i32, i32, String, i32)> {
        prims
            .iter()
            .filter_map(|p| match p {
                Prim::Block { x, y, w, h, edge, .. } => Some((*x, *y, *w, *h, edge.as_ref().map_or(String::new(), |e| e.stroke.clone()), edge.as_ref().map_or(0, |e| e.width))),
                Prim::Label { .. } => None,
            })
            .collect()
    }

    /// 标记那行的填色（认字号与锚点，与 `marks_joined` 同一把钥匙，只是取色不取字）。
    fn marks_fill(prims: &[Prim]) -> Option<String> {
        prims.iter().find_map(|p| match p {
            Prim::Label { fill, anchor, em, .. } if *anchor == Anchor::End && *em == BODY_LARGE => Some(fill.clone()),
            _ => None,
        })
    }

    /// 右上角那一串标记：认 `lane::MARK_TEXT` ＋ `Anchor::End` ＋ 字号 `BODY_LARGE` 这三件一起命中的那行。
    /// 单认颜色或单认锚点都会把别的行误当它（属性串与它是同一格同一锚，只有字号不同）。
    fn marks_joined(prims: &[Prim]) -> Option<String> {
        prims.iter().find_map(|p| match p {
            Prim::Label { fill, anchor, em, text, .. } if *anchor == Anchor::End && *em == BODY_LARGE && fill == lane::MARK_TEXT => Some(text.clone()),
            _ => None,
        })
    }

    /// 双柱 Boss 两根都要进表，且第二根的柱名带列区（业力只在第一根）。
    /// 这条原本住在 `canvas`：它钉的是"两根"这个**决策**，不是 XML 里两个 `<rect>`，所以搬到这里。
    #[test]
    fn dual_holder_emits_both_bars_and_inherits_the_karma_absence() {
        let b = Battle::new_boss(2026, Faction::Ember, BossId::YanBing, Vec::new(), 48);
        let v = b.view();
        let f = frame_prims(&v, 2026, "boss", &[]);
        assert_eq!(v.enemy_holders.len(), 2);
        let words = texts(&f);
        assert!(words.iter().any(|t| t.contains(v.enemy_holders[0].name)), "第一柱名要出现");
        assert!(words.iter().any(|t| t.contains(v.enemy_holders[1].name)), "第二柱名也要出现（画漏一根就成了一柱图）");
        assert!(words.iter().any(|t| t.contains("直击此柱")), "第二根的受击列区要带上");
        // 一根柱一条底槽，我方持业者再加一条——**根数从快照取**，不写死，与 `card_row` 那条口径相同。
        assert_eq!(bar_tracks(&f), v.enemy_holders.len() + 1, "底槽条数＝柱数＋我方一条：少一条就是画漏一根");
        // 快照给 None 的那个数，编码器不许自己补一个上去（这条原本是"两根都画"那格的另一半）。
        let second = words.iter().find(|t| t.contains(v.enemy_holders[1].name)).expect("第二柱那一行");
        assert_eq!(v.enemy_holders[1].karma, None, "夹具变了：这条测的前提是快照第二柱不给业力");
        assert!(!second.contains("业力"), "快照没给的数不许出现在帧上：{second}");
    }

    /// 指令表本身是纯函数：同一份快照连编两次，逐条指令必须相同（帧可比 md5 的前提在这一层就得成立）。
    /// 另一半钉的是**控件层的形状**：盖一层态只改色，不增删指令——多一条少一条都意味着控件层伸进了布局。
    #[test]
    fn the_instruction_list_is_pure_and_the_control_layer_only_recolors() {
        let v = normal().view();
        assert!(!v.hand.is_empty(), "夹具要有手牌，否则下面那半截永真");
        assert_eq!(format!("{:?}", frame_of(&v).prims), format!("{:?}", frame_of(&v).prims), "同快照 ⇒ 同指令表");
        let bare = frame_of(&v);
        let pressed = frame_prims(&v, 7, "t", &[(Region::Hand(0), Layer::Press)]);
        assert_eq!(bare.prims.len(), pressed.prims.len(), "盖一层态不许增删指令");
        assert_ne!(format!("{:?}", bare.prims), format!("{:?}", pressed.prims), "按下那一格要变色");
    }

    /// 空格也进表：虚线框是一块**带描边**的块，点数＝12 格。列数从快照长度取，
    /// 所以这条钉的是"取长度这条路真的走通了"：少画一格不会报错，只会静默变成一张缺一列的图。
    #[test]
    fn every_slot_of_the_board_becomes_a_block_not_a_hardcoded_count() {
        let v = normal().view();
        let f = frame_of(&v);
        let slots = v.enemy_back.len() + v.enemy_front.len() + v.player_front.len();
        assert_eq!(slots, 12, "盘面固定 4 列 × 3 排");
        let cell_blocks = blocks(&f).into_iter().filter(|(_, _, _, h)| *h == CELL_H).count();
        assert_eq!(cell_blocks, slots, "格子块数＝槽位数（空格也要画框）");
        let empties = v.enemy_back.iter().chain(v.enemy_front.iter()).chain(v.player_front.iter()).filter(|c| c.is_none()).count();
        let dashed = f.prims.iter().filter(|p| matches!(p, Prim::Block { edge: Some(e), .. } if e.dashed)).count();
        assert_eq!(dashed, empties, "虚线块数＝空格数（少一个＝有一格没画）");
    }
}
