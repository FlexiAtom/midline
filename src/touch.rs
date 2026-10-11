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
//! 2D 控件层：**几何的唯一家** ＋ 把手势翻成一行命令。
//!
//! 为什么要有"几何的家"这一件事：`canvas.rs` 从前一边画一边算——y 游标、格宽、折行数全在画的那条路上顺手推。
//! 画只需要"这个东西在哪儿"，命中需要"这个点落在哪个东西里"。两份算术分在两个模块就会各自漂移，
//! 而漂移的表现形式是**真机上点不动**（画在 A 处、命中框在 B 处），断言全绿也照样看不见。
//! 现在绘制侧读 `layout_of` 的矩形、命中侧读同一份矩形，`the_regions_that_take_a_tap_are_the_regions_that_got_drawn`
//! 把"两份是同一件东西"钉成一条测。
//!
//! 三条边界线：
//! - **输出形状 = `Host::read` 的返回形状**（见 `session.rs` 那个 trait：一行连行尾换行）。控件层产出的就是一行
//!   这种字符串，于是阶段机一个字不改：它从前等人打字，现在等这一支函数。这与 `tui.rs` 那帧"输出可以换上游"对称。
//! - **词汇由 `command.rs` 那张词表决定，不由手感决定**。战斗内十个命令词里只有 `p <手牌idx> <P1-P4>` 带坐标，
//!   其余（`s`／`sh`／`di`／`ds`／`e`／`l`／`b`／`q`／`h`）一个都不以敌方格或柱号为参数 ⇒ 敌方一侧、中线、日志一律不接点。
//!   献祭（`s <Pn>`）属"永久消失"那一族，不给单指手势：文档里没有这条规则，那是**新规则**不是补空白，须走池。
//! - **态层算成实心色**，不在 SVG 里用 `fill-opacity`。真机那条路是 `ANativeWindow` 帧缓冲写像素，没有合成器
//!   替做 alpha；口径住在 `md3::state::blend`，两个后端共用它才能算出同一个颜色。
//!
//! 规则锚：本模块**不写**文档锚点，理由与 `canvas`／`tui` 相同——几何与手势不新增任何规则真值，
//! 写锚会去动反向覆盖尺的生产面读数。
//!
//! 运输（安卓输入队列 → 这里的 `press`／`release`）**不在本帧**：那是这一格里设备无关之外的另一半，
//! 要先把 `ANativeActivity` 那个 `AInputQueue` 接成事件。本帧先把"事件来了该变成哪一行"定死。

use crate::md3::{layout, state, typo};
use crate::view::{Board, CardView};

/// 一块矩形。
///
/// `contains` 含左上、**不含**右下（`x < self.x + self.w`）。这条边界语义不是风格：两列并排时
/// 若两侧都闭，缝上那一列宽的点会同时落进两格，于是"点在最左列却命中了隔壁"这类 off-by-one
/// 只能靠命中顺序掩盖。都按半开半闭处理，一格一格严丝合缝，重叠只可能是真的画重了。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub(crate) const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "控件层等真机输入队列（运输那半）；接上即删这条豁免"))]
    pub(crate) const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// 盘面上一个**有面积**的东西。括号里是它在快照里的位置（`Region::Hand(2)` 就是 `v.hand[2]`），
/// 所以它同时是"点中了哪一格"的答案与"该重画哪一格"的键——两侧共用一个名字，才有分不开的可能。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Region {
    Hand(usize),
    PlayerCell(usize),
    EnemyCell(usize),
    BackCell(usize),
    EnemyHolder(usize),
    PlayerHolder,
    Midline,
    Log,
}

impl Region {
    /// 这一区接不接点。判据是抬头那张词表，不是我的偏好：带坐标的命令只有落点那一条，
    /// 而落点的两个参数一个来自手牌、一个来自我方格。
    #[cfg_attr(not(test), expect(dead_code, reason = "控件层等真机输入队列；接上即删这条豁免"))]
    pub(crate) const fn is_control(self) -> bool {
        matches!(self, Region::Hand(_) | Region::PlayerCell(_))
    }
}

/// 三态层。**只许取 `md3::state` 里那一档系数**，这一层不许出现任何透明度数字。
/// 焦点与按下在 MD3 里同值（`md3` 那条秩序尺钉着），所以"按下仍要看得见"靠的是这里的
/// 优先级（`marks` 升序返回、绘制侧取最后一个），不是靠另配一个更深的数。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Layer {
    Hover,
    Focus,
    Press,
}

impl Layer {
    pub(crate) const fn alpha(self) -> f64 {
        match self {
            Layer::Hover => state::HOVER,
            Layer::Focus => state::FOCUS,
            Layer::Press => state::PRESS,
        }
    }
}

/// 一次点按的结论。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Tap {
    /// 要交回调用方的那一行，形状与 `Host::read` 的返回体一致（连行尾换行）。
    /// 产出的行必须能被 `command::parse` 读成一条真命令——那条由
    /// `the_gesture_table_is_a_subset_of_the_command_vocabulary` 机检，不许产出一行键盘上打不出来的字。
    Line(String),
    /// 选中／取消选中一张手牌。这两下不产命令，只改控件自己的态（画出来的焦点环）。
    Selected(usize),
    Deselected(usize),
    /// 落点不在控件上；或按在一处、抬在另一处（那是拖拽，本帧不立这条手势）。
    Ignore,
}

/// 一根持业者柱。绘制要的字基线与命中要的条矩形都从这两个数推，`rect()` 是它们唯一的算式。
#[derive(Copy, Clone, Debug)]
pub(crate) struct Holder {
    pub region: Region,
    pub x: i32,
    pub y: i32,
}

impl Holder {
    pub(crate) const fn rect(&self) -> Rect {
        Rect::new(self.x, self.y + layout::SECTION_DROP, layout::BAR_W, layout::BAR_H)
    }
}

/// 一横排：段标签基线、格子行起点、每格一个 `(区, 矩形)`。
/// 空格**也有矩形**——快照里 `None` 那一格照样画框，它的位置本身就是信息（"这儿能放"）。
pub(crate) struct Row {
    pub label_y: i32,
    pub y: i32,
    pub cells: Vec<(Region, Rect)>,
}

/// 手牌行。`name_budget` 是从格宽算出来的截断预算，**留在几何这一侧**：
/// 它是"这格多宽"的函数，挪到绘制侧就等于把格宽抄第二遍。
pub(crate) struct Hand {
    pub label_y: i32,
    pub y: i32,
    pub cells: Vec<(Region, Rect)>,
    pub name_budget: f64,
    /// 空手牌时那行「（空）」的落点；有牌时 `None`。宽高压着 0，所以它不进 `regions()`——
    /// 那行字不是控件，点上去什么都不该发生。
    pub placeholder: Option<Rect>,
}

/// 一帧盘面的全部几何。**从快照单向算出，不含任何绘制调用**：这就是"剥出来"的意思。
pub(crate) struct Layout {
    pub width: i32,
    pub height: i32,
    pub left_x: i32,
    pub right_x: i32,
    pub center_x: i32,
    pub title_y: i32,
    /// Boss 规则句的折行结果（每行一行字＋它的基线）。折行在这儿做完，因为**行数决定后面所有的 y**：
    /// 那件事留在绘制侧，几何就不完整，而不完整的表现是整张图的命中框错位一截。
    pub rule: Vec<(String, i32)>,
    pub holders: Vec<Holder>,
    pub back: Row,
    pub enemy: Row,
    pub player: Row,
    pub midline: Rect,
    pub midline_text_y: i32,
    pub player_holder: Holder,
    pub hand: Hand,
    pub piles_y: i32,
    pub log: Rect,
    pub log_text_x: i32,
    pub log_first_y: i32,
    pub log_step: i32,
    pub log_budget: f64,
}

/// 快照 → 几何。列数从数组长度取（口径与 `canvas` 抬头那条相同：编码器不许替引擎再钉一次盘面形状），
/// 画布高度由"这一帧实际排到哪儿"推出。
pub(crate) fn layout_of(v: &Board) -> Layout {
    let width = layout::BOARD_W;
    let left_x = layout::MARGIN;
    let content_w = width - layout::MARGIN * 2;
    let cols = v.enemy_back.len().max(1) as i32;
    let cell_w = (content_w - layout::COL_GAP * (cols - 1)) / cols;

    let mut y = layout::TITLE_BASELINE;
    let title_y = y;
    y += layout::HEADER_DROP;

    let mut rule: Vec<(String, i32)> = Vec::new();
    if let Some(r) = &v.rule {
        let text = format!("特殊规则【{}】{}", r.label, r.text);
        let budget = content_w as f64 / typo::TITLE_LARGE as f64 - typo::ELLIPSIS_COST;
        for line in wrap(&text, budget) {
            rule.push((line, y));
            y += layout::RULE_DROP;
        }
        y += layout::RULE_GAP;
    }

    let holders: Vec<Holder> = v
        .enemy_holders
        .iter()
        .enumerate()
        .map(|(i, _)| Holder { region: Region::EnemyHolder(i), x: left_x + i as i32 * layout::HOLDER_STRIDE, y })
        .collect();
    y += layout::HEADER_DROP + layout::BAR_H + layout::BLOCK_GAP;

    let back = row_of(&v.enemy_back, Region::BackCell, y, left_x, cell_w);
    y = back.y + layout::CELL_H + layout::ROW_GAP_BACK;
    let enemy = row_of(&v.enemy_front, Region::EnemyCell, y, left_x, cell_w);
    y = enemy.y + layout::CELL_H + layout::ROW_GAP_FRONT;

    let midline = Rect::new(left_x, y, content_w, layout::MIDLINE_BAND_H);
    let midline_text_y = y + layout::MIDLINE_BASELINE;
    y += layout::MIDLINE_BAND_H + layout::BLOCK_GAP;

    let player = row_of(&v.player_front, Region::PlayerCell, y, left_x, cell_w);
    y = player.y + layout::CELL_H + layout::BLOCK_GAP;

    let player_holder = Holder { region: Region::PlayerHolder, x: left_x, y };
    y += layout::HEADER_DROP + layout::BAR_H + layout::BLOCK_GAP;

    let hand = hand_of(&v.hand, y, left_x, content_w);
    y = hand.y + layout::HAND_H + layout::BLOCK_GAP;

    let piles_y = y;
    y += layout::SECTION_DROP;

    let log = Rect::new(left_x, y, content_w, layout::LOG_H);
    let log_text_x = left_x + layout::CELL_PAD;
    let log_first_y = y + layout::LOG_LINE_STEP;
    let log_step = layout::LOG_LINE_STEP;
    let log_budget = content_w as f64 / typo::BODY_MEDIUM as f64 - typo::ELLIPSIS_COST;
    let height = y + layout::LOG_H + layout::FOOT_GAP;

    Layout {
        width,
        height,
        left_x,
        right_x: width - layout::MARGIN,
        center_x: width / 2,
        title_y,
        rule,
        holders,
        back,
        enemy,
        player,
        midline,
        midline_text_y,
        player_holder,
        hand,
        piles_y,
        log,
        log_text_x,
        log_first_y,
        log_step,
        log_budget,
    }
}

/// 一横排的几何。`kind` 只是给每格贴一个区名（传 `Region::BackCell` 这样一支构造器），
/// 排布的算术仍然只在这一处。
fn row_of(cards: &[Option<CardView>], kind: fn(usize) -> Region, label_y: i32, left_x: i32, cell_w: i32) -> Row {
    let y = label_y + layout::SECTION_DROP;
    let cells = cards.iter().enumerate().map(|(i, _)| (kind(i), Rect::new(left_x + (cell_w + layout::COL_GAP) * i as i32, y, cell_w, layout::CELL_H))).collect();
    Row { label_y, y, cells }
}

fn hand_of(cards: &[CardView], label_y: i32, left_x: i32, content_w: i32) -> Hand {
    let y = label_y + layout::SECTION_DROP;
    if cards.is_empty() {
        return Hand { label_y, y, cells: Vec::new(), name_budget: 0.0, placeholder: Some(Rect::new(left_x + layout::CELL_PAD_NARROW, y + layout::HAND_ROW1, 0, 0)) };
    }
    let n = cards.len() as i32;
    let hw = (content_w - layout::COL_GAP * (n - 1)) / n;
    let name_budget = (hw - layout::CELL_PAD_NARROW * 2) as f64 / typo::TITLE_LARGE as f64;
    let cells = cards.iter().enumerate().map(|(i, _)| (Region::Hand(i), Rect::new(left_x + (hw + layout::COL_GAP) * i as i32, y, hw, layout::HAND_H))).collect();
    Hand { label_y, y, cells, name_budget, placeholder: None }
}

impl Layout {
    /// 全部有面积的区，按**绘制顺序**给出（后画的盖在先画的上面，所以 `hit` 从尾往头读）。
    /// 段标签、堆行那两行字、空手牌占位都不在这里：词表里没有它们的命令。
    fn regions(&self) -> Vec<(Region, Rect)> {
        let mut out: Vec<(Region, Rect)> = Vec::new();
        for h in &self.holders {
            out.push((h.region, h.rect()));
        }
        out.extend(self.back.cells.iter().copied());
        out.extend(self.enemy.cells.iter().copied());
        out.push((Region::Midline, self.midline));
        out.extend(self.player.cells.iter().copied());
        out.push((Region::PlayerHolder, self.player_holder.rect()));
        out.extend(self.hand.cells.iter().copied());
        out.push((Region::Log, self.log));
        out
    }

    /// 点在这儿 ⇒ 哪儿。取绘制序里最靠后（＝最上面）的那块。
    #[cfg_attr(not(test), expect(dead_code, reason = "控件层等真机输入队列（运输那半）；接上即删这条豁免"))]
    pub(crate) fn hit(&self, x: i32, y: i32) -> Option<Region> {
        self.regions().into_iter().rev().find(|(_, r)| r.contains(x, y)).map(|(g, _)| g)
    }

    /// 区名 → 矩形。**一张表两个读者**：绘制侧用它查"这一格该盖哪一层"，
    /// 控件侧用它证明"这个区还在盘上"（手牌缩短后残留的选中序号查不到矩形，因此什么也不会被点亮）。
    pub(crate) fn rect_of(&self, want: Region) -> Option<Rect> {
        self.regions().into_iter().find(|(g, _)| *g == want).map(|(_, r)| r)
    }
}

/// 选中／按下／悬停三份态，加上一张盘面几何，就能说出"这一行该交回阶段机"。
/// 它不碰引擎：合法性一律交给 `command::parse`（同一个引擎判），这里只负责把点变成字。
#[derive(Default)]
pub(crate) struct Picker {
    selected: Option<usize>,
    pressed: Option<Region>,
    hovered: Option<Region>,
}

impl Picker {
    #[cfg_attr(not(test), expect(dead_code, reason = "控件层等真机输入队列（运输那半）；接上即删这条豁免"))]
    pub(crate) const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// 按下与悬停只记态，不产命令——产命令的那一刻在抬起。
    #[cfg_attr(not(test), expect(dead_code, reason = "控件层等真机输入队列；接上即删这条豁免"))]
    pub(crate) fn press(&mut self, at: Option<Region>) {
        self.pressed = at;
    }

    #[cfg_attr(not(test), expect(dead_code, reason = "控件层等真机输入队列；接上即删这条豁免"))]
    pub(crate) fn hover(&mut self, at: Option<Region>) {
        self.hovered = at;
    }

    /// 抬起。一次点成的判据是**按与抬落在同一区**——不同区就是拖拽，本帧不立那条手势，
    /// 所以它的正确结局是"什么都没发生"，而不是"按哪儿算哪儿"。
    #[cfg_attr(not(test), expect(dead_code, reason = "控件层等真机输入队列；接上即删这条豁免"))]
    pub(crate) fn release(&mut self, at: Option<Region>) -> Tap {
        let pressed = self.pressed.take();
        if pressed != at {
            return Tap::Ignore;
        }
        match at {
            Some(Region::Hand(i)) => {
                if self.selected == Some(i) {
                    self.selected = None;
                    Tap::Deselected(i)
                } else {
                    self.selected = Some(i);
                    Tap::Selected(i)
                }
            }
            Some(Region::PlayerCell(slot)) => match self.selected.take() {
                Some(idx) => Tap::Line(format!("p {idx} P{}\n", slot + 1)),
                None => Tap::Ignore,
            },
            _ => Tap::Ignore,
        }
    }

    /// 一次完整点按（按下与抬起同点）——运输层真正会调的那一支。
    #[cfg_attr(not(test), expect(dead_code, reason = "控件层等真机输入队列（运输那半）；接上即删这条豁免"))]
    pub(crate) fn tap(&mut self, l: &Layout, x: i32, y: i32) -> Tap {
        let at = l.hit(x, y);
        self.press(at);
        self.release(at)
    }

    /// 该盖哪些态层，按**优先级升序**返回（绘制侧取最后一个 ⇒ 按下压过焦点压过悬停）。
    /// 两条筛：只有控件区产层（悬到日志上不该把日志刷亮）；区查不到矩形就不产层（陈旧序号）。
    #[cfg_attr(not(test), expect(dead_code, reason = "控件层等真机输入队列；接上即删这条豁免"))]
    pub(crate) fn marks(&self, l: &Layout) -> Vec<(Region, Layer)> {
        let mut out: Vec<(Region, Layer)> = Vec::new();
        if let Some(h) = self.hovered && h.is_control() {
            out.push((h, Layer::Hover));
        }
        if let Some(i) = self.selected {
            out.push((Region::Hand(i), Layer::Focus));
        }
        if let Some(p) = self.pressed && p.is_control() {
            out.push((p, Layer::Press));
        }
        out.retain(|(g, _)| l.rect_of(*g).is_some());
        out
    }
}

// ── 文本度量：几何要靠它，所以它住在几何这一侧 ────────────────────────────

/// 单个字符占多宽（以字号为单位）：CJK 记满一格，ASCII 记 `typo::ADVANCE_ASCII` 格。
/// 版式要的是"这行字占多宽"，而快照里的文案全是人读的中文——按 `chars()` 计数会把一行 ASCII
/// 当成一行中文那么宽，那正是文本渲染层在定宽格子上踩过的坑。
pub(crate) fn advance(c: char) -> f64 {
    if c.is_ascii() { typo::ADVANCE_ASCII } else { typo::ADVANCE_CJK }
}

pub(crate) fn display_width(s: &str) -> f64 {
    s.chars().map(advance).sum()
}

/// 按显示宽度折行。Boss 规则行走这条：那句整文是规则本体，**截断等于把规则改短**，宁可占两行。
pub(crate) fn wrap(s: &str, budget: f64) -> Vec<String> {
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

/// 按显示宽度截断、补省略号。日志行与手牌名走这条：它们是滚过去的记录与待选的牌，
/// 没人需要在这一帧看完一整条。
pub(crate) fn fit(s: &str, budget: f64) -> String {
    if display_width(s) <= budget {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        if w + advance(c) > budget - typo::ELLIPSIS_COST {
            break;
        }
        out.push(c);
        w += advance(c);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::{Battle, Difficulty};
    use crate::command;
    use crate::md3::{color, lane};
    use crate::model::Faction;

    fn normal() -> Battle {
        Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1)
    }

    /// **本模块存在的全部理由**：接点的区与画出来的区是同一批矩形。
    /// 每个区的矩形都要能在帧里逐字找到（`x`／`y`／`width`／`height` 四个数一起对上），反过来
    /// 盘面格区数必须恰等于快照三排格数——两边都数，才堵得住"只列了一半"这种假绿。
    #[test]
    fn the_regions_that_take_a_tap_are_the_regions_that_got_drawn() {
        let b = normal();
        let v = b.view();
        let l = layout_of(&v);
        let frame = crate::canvas::svg_frame(&v, 7, "hit");
        let regions = l.regions();
        let board_cells = v.enemy_back.len() + v.enemy_front.len() + v.player_front.len();

        assert!(regions.len() >= board_cells, "区表比盘面格数还少＝有一半格子没人认领");
        for (region, r) in &regions {
            let box_needle = format!(r##"x="{}" y="{}" width="{}" height="{}""##, r.x, r.y, r.w, r.h);
            assert!(frame.contains(&box_needle), "{region:?} 的矩形没进帧：几何与绘制已经分叉（找 {box_needle}）");
        }
        let drawn_board = regions.iter().filter(|(g, _)| matches!(g, Region::BackCell(_) | Region::EnemyCell(_) | Region::PlayerCell(_))).count();
        assert_eq!(drawn_board, board_cells, "盘面格区数＝快照三排格数（少一格就少一个落点）");
        assert_eq!(regions.iter().filter(|(g, _)| g.is_control()).count(), v.hand.len() + v.player_front.len(), "接点的区只有手牌与我方格：词表里只有落点那一条带坐标");

        // 每个区名只许出现一次：两个区共用一个名字的话，`rect_of` 会静默返回先查到的那个。
        for (i, (a, _)) in regions.iter().enumerate() {
            assert!(!regions[i + 1..].iter().any(|(b, _)| b == a), "{a:?} 在区表里出现了两次");
        }
    }

    /// 半开半闭不是风格：右下角那一格**不属于**这块矩形，缝上的点归右边那格。
    /// 把 `<` 改成 `<=` 这条当场红——那是"点在最左列却命中了隔壁"那类 off-by-one 唯一的抓法。
    #[test]
    fn contains_takes_the_top_left_and_leaves_the_bottom_right() {
        let r = Rect::new(10, 20, 30, 40);
        assert!(r.contains(10, 20), "左上角含在内");
        assert!(r.contains(39, 59), "右下角往里一格还算");
        assert!(!r.contains(9, 20), "左边框外不算");
        assert!(!r.contains(40, 20), "右边那条边是与隔壁的接缝，不算");
        assert!(!r.contains(10, 60), "下边同理");
        assert!(!Rect::new(5, 5, 0, 0).contains(5, 5), "零面积矩形谁都接不住（空手牌占位因此不是控件）");
    }

    /// 点的坐标要落回**画它的那一格**：拿每格矩形的中心去命中，必须取回同一个区名；
    /// 拿右边界那一格去命中，不许再取回它。
    #[test]
    fn a_tap_lands_on_the_cell_that_was_drawn_there() {
        let b = normal();
        let v = b.view();
        let l = layout_of(&v);
        assert!(!v.hand.is_empty(), "这一桌要有手牌，否则下面那半是空的");
        for i in 0..v.hand.len() {
            let r = l.rect_of(Region::Hand(i)).expect("每张手牌都有矩形");
            assert_eq!(l.hit(r.x + r.w / 2, r.y + r.h / 2), Some(Region::Hand(i)), "第 {i} 张手牌的中心命中的却不是它");
            assert_ne!(l.hit(r.x + r.w, r.y + r.h / 2), Some(Region::Hand(i)), "右边界那一格不该还算这张牌");
        }
        for slot in 0..v.player_front.len() {
            let r = l.rect_of(Region::PlayerCell(slot)).expect("我方每格都有矩形");
            assert_eq!(l.hit(r.x + r.w / 2, r.y + r.h / 2), Some(Region::PlayerCell(slot)), "第 {} 格（P{}）的中心命中的却不是它", slot + 1, slot + 1);
        }
    }

    /// 手势词汇必须是键盘词汇的**子集**：两次点按产出的那一行，交进 `command::parse` 要读成一条真命令
    /// （不是 `Unknown`／`Reject`），并且与"人手敲这一行"走出同一个局面。
    /// 这条是"输入可以换来源"那半句的实证——阶段机吃的形状没变，走的路径也不能变。
    #[test]
    fn the_gesture_table_is_a_subset_of_the_command_vocabulary() {
        let b = normal();
        let v = b.view();
        let l = layout_of(&v);
        let mut picker = Picker::default();

        let first = l.rect_of(Region::Hand(0)).expect("手牌有矩形");
        assert_eq!(picker.tap(&l, first.x + 1, first.y + 1), Tap::Selected(0), "点第一张手牌＝选中它");

        let cell = l.rect_of(Region::PlayerCell(1)).expect("P2 有矩形");
        let line = match picker.tap(&l, cell.x + 1, cell.y + 1) {
            Tap::Line(s) => s,
            other => panic!("选中手牌再点我方格该产出一行落点命令，实到 {other:?}"),
        };
        assert_eq!(line, "p 0 P2\n", "产出的行要与人手敲的一字不差（连行尾换行）");
        assert_eq!(picker.selected(), None, "落点成了就别留着那份选中态");

        let mut gestured = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        let mut typed = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        match command::parse(&line) {
            command::Command::Place { hand_idx, slot } => assert_eq!((hand_idx, slot), (0, 1), "parse 读出来的落点参数要与之对应"),
            other => panic!("这一行在词表里读不成 Place：{other:?}"),
        }
        command::execute(&mut typed, command::parse("p 0 P2\n"));
        command::execute(&mut gestured, command::parse(&line));
        assert_eq!(crate::render::render_board(&typed.view()), crate::render::render_board(&gestured.view()), "同一行两次执行，盘面必须一字不差");
    }

    /// 从快照自己数第一个空格——**不读 `Layout`**，所以这一支与手势那侧的算式是两份独立推导。
    /// 两份推导落到同一个数字上才有对撞的意义；共用一份几何的话，这条测就只是在测空气。
    fn first_free_slot(v: &Board) -> Option<usize> {
        v.player_front.iter().position(|c| c.is_none())
    }

    /// 端到端的那台**同一机器**：A 局逐手敲键盘行，B 局逐手两次点按产出那一行。
    /// 每一步先比**产出的串**（两侧各自独立算出 idx 与列号），再比 `render` 全文、再比 `svg_frame` 全帧字节。
    /// 这条量的是"输入可以换来源"里最难的一种错：坐标换算差一格，产出的行仍是合法形状、仍能 parse，
    /// 只是落进了隔壁那列——单点测（`the_gesture_table_is_a_subset_of_the_command_vocabulary`）只走一手，
    /// 看不出一路走下来的漂移；这里连打若干手，一次分叉就当场红。
    ///
    /// 每三手敲一次 `e`：手势表只立两指落子，过回合**没有**手势，两侧都走打字——这恰恰是"输出形状＝
    /// `Host::read` 那一行"换来的：没有手势的动作不必先造一个事件类型。过回合也让抽牌、攻击、阵亡
    /// 真进到这条尺里（不落 `e` 的话 18 手全挤在同一回合，盘面根本不动，比的其实是两次同样的拒绝）。
    #[test]
    fn the_tap_path_and_the_typed_path_drive_one_and_the_same_battle() {
        let mut gestured = normal();
        let mut typed = normal();
        let mut picker = Picker::default();
        let (mut tapped, mut accepted) = (0, 0);
        let opening = crate::render::render_board(&gestured.view());

        for step in 0..18 {
            let (gv, tv) = (gestured.view(), typed.view());
            let l = layout_of(&gv);
            let free = step % 3 != 2;
            let gesture_line = match (free, first_free_slot(&gv), gv.hand.is_empty()) {
                (true, Some(slot), false) => {
                    let idx = step % gv.hand.len();
                    let card = l.rect_of(Region::Hand(idx)).expect("有手牌就有矩形");
                    assert_eq!(picker.tap(&l, card.x + 1, card.y + 1), Tap::Selected(idx), "第 {step} 手：点第 {idx} 张手牌没选中它");
                    let cell = l.rect_of(Region::PlayerCell(slot)).expect("我方每格都有矩形");
                    match picker.tap(&l, cell.x + 1, cell.y + 1) {
                        Tap::Line(s) => {
                            tapped += 1;
                            Some(s)
                        }
                        other => panic!("第 {step} 手：选中再点 P{} 该产出一行，实到 {other:?}", slot + 1),
                    }
                }
                _ => None,
            };
            let typed_line = if free {
                match (first_free_slot(&tv), tv.hand.is_empty()) {
                    // 先取列号再格式化：直接把 `slot + 1` 当最后一个实参传进 format! 就会与生产行同形，
                    // 撞掉 M162 那颗牙的「全仓恰命中一次」锚（预检当场喊「补丁失败」，不是静默转绿）。
                    (Some(slot), false) => {
                        let col = slot + 1;
                        Some(format!("p {} P{}\n", step % tv.hand.len(), col))
                    }
                    _ => None,
                }
            } else {
                None
            };

            let (g, t) = match (gesture_line.as_deref(), typed_line.as_deref()) {
                (Some(g), Some(t)) => (g, t),
                (None, None) => ("e\n", "e\n"),
                (g, t) => panic!("第 {step} 手：一侧有落点一侧没有（手势 {g:?}／打字 {t:?}）——两局到这里已经分叉"),
            };
            assert_eq!(g, t, "第 {step} 手：两次点按产出的那行与手敲的那行不是同一串");
            if matches!(command::execute(&mut gestured, command::parse(g)), command::Exec::Done) {
                accepted += 1;
            }
            command::execute(&mut typed, command::parse(t));

            assert_eq!(crate::render::render_board(&gestured.view()), crate::render::render_board(&typed.view()), "第 {step} 手之后 文本面已分叉");
            assert_eq!(crate::canvas::svg_frame(&gestured.view(), 7, "two-paths"), crate::canvas::svg_frame(&typed.view(), 7, "two-paths"), "第 {step} 手之后 帧字节已分叉");
        }
        assert!(tapped >= 2, "反空虚：全程几乎没有一次是两次点按产出的行（实到 {tapped}）——手势那半在空转");
        assert!(accepted > 0, "反空虚：一次都没成交 ⇒ 这条比的只是两次同样的拒绝");
        assert_ne!(opening, crate::render::render_board(&gestured.view()), "反空虚：18 手打完盘面仍是开局那一幅 ⇒ 两条路都在原地，逐手比字节等于没比");
        assert_eq!(picker.selected(), None, "一路点下来不该留下一枚没人认领的选中态");
    }

    /// 敌方一侧、中线、日志接不了点。理由在词表上：没有一条战斗内命令以它们为参数。
    /// 这条同时是反空虚：它**逐区**过，把 `is_control` 写成恒真会在第一个敌方格上红。
    #[test]
    fn nothing_on_the_enemy_side_answers_a_tap() {
        let b = normal();
        let v = b.view();
        let l = layout_of(&v);
        let mut picker = Picker::default();
        let card = l.rect_of(Region::Hand(0)).expect("手牌有矩形");
        picker.tap(&l, card.x + 1, card.y + 1);
        assert_eq!(picker.selected(), Some(0), "先选一张，否则下面读的是「没选中所以不动」，不是「这一区不接点」");

        for region in [Region::EnemyCell(0), Region::BackCell(0), Region::EnemyHolder(0), Region::PlayerHolder, Region::Midline, Region::Log] {
            let rect = l.rect_of(region).unwrap_or_else(|| panic!("{region:?} 该有矩形（画出来了就该接得住一次探测）"));
            let at = l.hit(rect.x + rect.w / 2, rect.y + rect.h / 2);
            assert_eq!(at, Some(region), "{region:?} 的中心命中了别的东西：区与区重叠了");
            assert_eq!(picker.release(at), Tap::Ignore, "{region:?} 接了点：词表里没有一条命令以它为参数");
        }
        assert_eq!(picker.selected(), Some(0), "点在非控件上不该把已选中的牌吞掉");
    }

    /// 按在一处、抬在另一处不算一次点按（那是拖拽，本帧不立）。同时要清掉按下态——
    /// 不清的话，下一次 `marks` 还会把上一指的按下层留在画面上。
    #[test]
    fn press_and_release_on_different_regions_is_not_a_tap() {
        let b = normal();
        let v = b.view();
        let l = layout_of(&v);
        let mut picker = Picker::default();
        picker.press(Some(Region::Hand(0)));
        assert_eq!(picker.release(Some(Region::Hand(1))), Tap::Ignore, "换区抬起不许点成");
        assert!(picker.marks(&l).is_empty(), "抬起之后还留着按下层：上一指的痕迹没清");
        assert_eq!(picker.selected(), None, "没点成却选中了牌");
    }

    /// 三态层各自取自己那一档系数，且**优先级**决定画面上看见哪一层。
    /// 焦点与按下在 MD3 里同值，所以"按下仍要看得见"只能靠次序——这一条就是那件次序的牙。
    #[test]
    fn the_layers_read_their_own_coefficients_and_press_wins() {
        assert_eq!(Layer::Hover.alpha(), state::HOVER, "悬停层没接 md3 那一档");
        assert_eq!(Layer::Focus.alpha(), state::FOCUS, "焦点层没接 md3 那一档");
        assert_eq!(Layer::Press.alpha(), state::PRESS, "按下层没接 md3 那一档");
        assert_ne!(Layer::Hover.alpha(), Layer::Focus.alpha(), "悬停与聚焦同值＝两态合一态");

        let v = normal().view();
        let l = layout_of(&v);
        let mut picker = Picker::default();
        picker.hover(Some(Region::Hand(0)));
        picker.press(Some(Region::Hand(0)));
        picker.selected = Some(0);
        let marks = picker.marks(&l);
        assert_eq!(marks, vec![(Region::Hand(0), Layer::Hover), (Region::Hand(0), Layer::Focus), (Region::Hand(0), Layer::Press)], "升序返回：绘制侧取最后一个");
        assert_eq!(crate::canvas::layer_of(&marks, Region::Hand(0)), Some(Layer::Press), "最后一层不是按下＝优先级反了");

        // 只有控件区产层：悬在日志上、按在中线上都不该刷亮什么。
        let mut idle = Picker::default();
        idle.hover(l.hit(l.log.x + 1, l.log.y + 1));
        idle.press(l.hit(l.midline.x + 1, l.midline.y + 1));
        assert_eq!(idle.marks(&l), Vec::<(Region, Layer)>::new(), "非控件区产出了态层");
    }

    /// 手牌变短之后残留的选中序号，什么也不许点亮：区名查不到矩形 ⇒ 层被筛掉。
    /// 这条是"陈旧索引"那一族 bug 的通用抓法——它不检查引擎有多小心，它检查画面上没有鬼影。
    #[test]
    fn a_stale_selection_lights_nothing() {
        let v = normal().view();
        let l = layout_of(&v);
        let stale = Picker { selected: Some(v.hand.len() + 3), ..Picker::default() };
        assert!(stale.marks(&l).is_empty(), "盘上没这张牌，焦点环却画了出来");
        let live = Picker { selected: Some(0), ..Picker::default() };
        assert_eq!(live.marks(&l), vec![(Region::Hand(0), Layer::Focus)], "对照：还在盘上的那张要亮");
    }

    /// 空手牌那一行是字，不是控件：它有落点（要画）、没有面积（接不住点）。
    #[test]
    fn the_empty_hand_says_so_without_becoming_a_target() {
        let b = normal();
        let mut v = b.view();
        let frame = crate::canvas::svg_frame(&v, 7, "full");
        assert!(!layout_of(&v).hand.placeholder.is_some(), "有牌时不该留占位落点");

        v.hand.clear();
        let l = layout_of(&v);
        let place = l.hand.placeholder.expect("空手牌要给出占位落点");
        assert_eq!((place.w, place.h), (0, 0), "占位行不该有面积");
        assert_eq!(l.rect_of(Region::Hand(0)), None, "没有手牌却冒出一个手牌区");
        assert!(crate::canvas::svg_frame(&v, 7, "empty").contains("（空）"), "空手牌要写明它空");
        assert!(frame.contains("手牌 "), "对照：有牌那一帧也写着张数，占位不是唯一的路");
    }

    /// 帧的几何全部来自这一份 `Layout`：一个矩形同时决定像素与命中。
    /// 这条把"一个家"证成可判的东西——挑一格按下，帧上那一格的底色要变，而且变的正是那个矩形。
    #[test]
    fn one_rect_drives_both_the_pixels_and_the_hit() {
        let v = normal().view();
        let l = layout_of(&v);
        let cell = l.rect_of(Region::Hand(0)).expect("第一张手牌有矩形");
        let bare = crate::canvas::svg_frame(&v, 7, "one");
        let pressed = crate::canvas::svg_frame_marked(&v, 7, "one", &[(Region::Hand(0), Layer::Press)]);
        assert_ne!(bare, pressed, "按下了画面却一模一样＝态层白做");
        let box_needle = format!(r##"x="{}" y="{}" width="{}" height="{}""##, cell.x, cell.y, cell.w, cell.h);
        assert!(pressed.contains(&box_needle), "盖上态层之后那块矩形的四个数换了");
        assert_eq!(l.hit(cell.x + 1, cell.y + 1), Some(Region::Hand(0)), "同一个点两次读不出同一个区");
    }

    /// 没有可画层的那一帧，与控件层根本不存在时**逐字节**相同：控件层不许反过来改动呈现。
    /// 这是"零字节漂移"那条反证的测面版本（盘面对撞记在档 §2）。
    #[test]
    fn the_control_layer_adds_nothing_when_nothing_is_pressed() {
        let v = normal().view();
        assert_eq!(crate::canvas::svg_frame(&v, 7, "quiet"), crate::canvas::svg_frame_marked(&v, 7, "quiet", &[]), "空 marks 走出另一份字节：呈现被控件层污染");
        assert_eq!(crate::canvas::svg_frame(&v, 7, "quiet"), crate::canvas::svg_frame_marked(&v, 7, "quiet", &[(Region::Log, Layer::Hover)]), "悬在非控件区不该动一个字节");
    }

    /// 文本度量：折行按显示宽度，不按字符数。同样一行预算，全 ASCII 装得下更多字符；
    /// 中文要在画布边缘之前断开——断不开就是最容易看见的失真（规则句被裁掉半句）。
    #[test]
    fn text_wraps_by_display_width_not_by_char_count() {
        assert_eq!(wrap("中文中文中文", 3.0).len(), 2, "每行 3 个汉字 ⇒ 六个字两行");
        assert_eq!(wrap("abcdefghij", 3.0).len(), 2, "ASCII 记 {} ⇒ 十个字母两行，不是四行", typo::ADVANCE_ASCII);
        assert_eq!(wrap("", 3.0), Vec::<String>::new(), "空串不产空行");
        assert_eq!(fit("中文中文", 3.0), "中文…", "超预算才截断");
        assert_eq!(fit("中文", 99.0), "中文", "没超预算就一字不动（不许顺手加省略号）");
        assert_eq!(display_width("中a"), typo::ADVANCE_CJK + typo::ADVANCE_ASCII, "一个汉字加一个字母＝两档之和");
    }

    /// 底色与态层混出来的是**另一个实心色**，且写法与这张表同一个表示法。
    /// 这条是"算成实心色而不是 opacity"的可判形式：拿它写安卓帧缓冲，与拿它写 SVG 的 `fill`，得到同一个串。
    #[test]
    fn the_state_layer_bakes_into_one_solid_color() {
        let base = lane::PLAYER_FILL;
        let hover = state::blend_hex(base, color::ON_SURFACE, Layer::Hover.alpha());
        let focus = state::blend_hex(base, color::ON_SURFACE, Layer::Focus.alpha());
        assert_ne!(hover, base, "悬停不留痕＝交互态白做");
        assert_ne!(hover, focus, "两态算出同一个色＝少了一态");
        assert!(hover.starts_with('#') && hover.len() == base.len(), "混色产出的必须是同一个表示法：{hover}");
        assert_eq!(state::blend_hex(base, color::ON_SURFACE, 0.0), base, "0% 系数要原样回到底色（否则无 marks 那一帧就不可比）");
    }
}
