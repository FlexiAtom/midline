// midline - 《中线》安卓薄壳（丁′：壳也用 Rust，源码级融合）
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
//! 输入运输：一根手指 → 引擎的一行 → 新的一帧。档面 `working/shell-input-queue-to-command-loop.md`。
//!
//! **本模块不含一句平台调用**，这是刻意的：`entry.rs` 那边负责把 `AMotionEvent` 拆成
//! `(Action, x, y)` 三个数，剩下全部判定——命中哪一格、这一下算不算点成、这一行引擎接不接——
//! 都住在这里，于是 host 侧的对撞探针能用**同一份源码**把同一串坐标走到底（`probes/host-frame --tap`）。
//! 判定的那半如果只活在 android 门控里，它的证人就只剩"真机上点一下看看"，而那种证人没法进电池。
//!
//! 三条口径线：
//! - **换算只有一处**。手指在设备像素里，`touch::Layout` 在用户单位里，中间那座桥只由
//!   `frame::scale_of` 与 `Scale::at` 造（绘制侧用的就是这两支）。这里**没有** `(x - off) / k`
//!   那种反向除法：把矩形**正向**映到设备坐标再比，取整口径与画它时那一次是同一个，
//!   "画在 A 处、命中框在 B 处"这格病就没有落脚处。
//! - **命中判据只有一处**。倒着读 `Layout::regions()`（绘制序＝盖在上面的后画）与
//!   `Rect::contains`（半开半闭），都是引擎侧那一套，壳里不写第三套。
//! - **合法性一律交回引擎**。这里不立第二条规矩：`command::parse` ＋ `command::execute`
//!   就是 CLI 打同一行时走的同一对门，被拒的理由原样带回日志。

use crate::command::{self, Exec};
use crate::display::Frame;
use crate::frame::{self, Scale};
use crate::touch::{self, Picker, Rect, Region, Tap};

/// 一次触摸的动作档。真机给的是 `AMotionEvent` 那个 action 的低 8 位，这里只收壳真用得上的三档：
/// DOWN 记按下、UP 判点成、CANCEL 清态。MOVE／多点／滚动手势**不建模**——运输侧根本不会
/// 造出那几个值，所以这里也不为它们留分支（留了就是"看着接了、其实没人喂"）。
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Down,
    Up,
    Cancel,
}

impl Action {
    pub const fn name(self) -> &'static str {
        match self {
            Action::Down => "down",
            Action::Up => "up",
            Action::Cancel => "cancel",
        }
    }
}

/// **画这一帧时**用的几何。命中只认它，不认现算的另一份缓冲尺寸。
///
/// 为什么是"画的时候记下来"而不是"点的时候再算"：屏上那一格的位置就是上一次绘制定下来的，
/// 拿窗口现读尺寸重算一遍，在 resized 与触摸交错的时刻会得到一把**没画过**的尺。
#[derive(Copy, Clone, Debug)]
pub struct Geom {
    pub scale: Scale,
    pub board_w: i32,
    pub board_h: i32,
}

impl Geom {
    /// 全仓唯一的 `Scale` 构造口在这里被**调用**（不是被复制）：输入是缓冲尺寸，画布尺寸取自帧本身。
    pub fn of(buf_w: usize, buf_h: usize, frame: &Frame) -> Geom {
        Geom {
            scale: frame::scale_of(buf_w, buf_h, frame.width, frame.height),
            board_w: frame.width,
            board_h: frame.height,
        }
    }

    /// 用户单位矩形 → 设备像素矩形。**与绘制侧共用 `frame::dev_rect`**：像素是那两个 `at` 加两个
    /// `len` 落进缓冲的，命中框只有照同一支算才谈得上"点到的就是看见的那一格"。
    ///
    /// 两端各问一次 `at` 再取差的写法看着更准，实际会把画出来的最外一列（行）判成别人的：
    /// 现量本机型这档 k 下 13 块控件有 6 块差一格（差额没随 其六 那枚按钮变：它不在这 6 块里）。
    /// 这条由测 `the_hit_rect_is_the_painted_rect` 钉住。
    fn to_dev(self, r: &Rect) -> Rect {
        let [x, y, w, h] = frame::dev_rect(&self.scale, r.x, r.y, r.w, r.h);
        Rect::new(x, y, w, h)
    }

    /// 设备像素 → 盖在最上面的那块区。
    pub fn hit_dev(&self, l: &touch::Layout, x: i32, y: i32) -> Option<Region> {
        self.pairs(l)
            .into_iter()
            .rev()
            .find(|(_, r)| r.contains(x, y))
            .map(|(g, _)| g)
    }

    /// 设备坐标下的全部区（绘制序）。测试与日志都读它，所以它是**算出来的**而不是存下来的第二份表。
    pub fn pairs(&self, l: &touch::Layout) -> Vec<(Region, Rect)> {
        l.regions().into_iter().map(|(g, r)| (g, self.to_dev(&r))).collect()
    }
}

/// 一行的引擎答复，形状照 `session::battle_line` 那四支分派（`Exec` 的全部可能）。
#[derive(Debug, PartialEq, Eq)]
pub enum Fed {
    Done,
    Print(String),
    Refused(String),
    Quit(&'static str),
}

impl Fed {
    pub const fn name(&self) -> &'static str {
        match self {
            Fed::Done => "done",
            Fed::Print(_) => "print",
            Fed::Refused(_) => "refused",
            Fed::Quit(_) => "quit",
        }
    }
}

/// 一次触摸的结论。它只为 logcat 那一行与测的断言存在——**判定不看它**，态与引擎都已改完。
#[derive(Debug)]
pub struct Touch {
    pub action: Action,
    pub x: i32,
    pub y: i32,
    pub region: Option<Region>,
    pub line: Option<String>,
    pub fed: Option<Fed>,
}

impl Touch {
    /// 那一行日志的正文。**在输入侧格式化**而不是在平台侧：两边（真机与 host 探针）读同一串，
    /// 对撞时才分得清"没收到事件"与"收到了但没判成"。
    pub fn log_line(&self) -> String {
        use std::fmt::Write as _;
        let mut s = format!("touch {} x={} y={}", self.action.name(), self.x, self.y);
        match &self.region {
            Some(r) => {
                let _ = write!(s, " region={r:?}");
            }
            None => {
                let _ = write!(s, " region=none");
            }
        }
        if let Some(line) = &self.line {
            let _ = write!(s, " line={:?}", line.trim_end());
        }
        if let Some(fed) = &self.fed {
            let _ = write!(s, " fed={}", fed.name());
            match fed {
                Fed::Refused(why) => {
                    let _ = write!(s, "({why})");
                }
                Fed::Quit(why) => {
                    let _ = write!(s, "({why})");
                }
                Fed::Print(text) => {
                    let _ = write!(s, "( {} 字)", text.chars().count());
                }
                Fed::Done => {}
            }
        }
        s
    }
}

/// 活的那一桌牌：引擎状态 ＋ 控件状态 ＋ 画它时那把尺。
///
/// 从前壳每次绘制现打一桌（`frame::demo_frame()`，画完就丢），所以屏上的东西**不可被操作**。
/// 这一格把它持住：`p 0 P1` 那样的行落地后要有人变、变了要有一帧新画面。
pub struct Live {
    battle: crate::battle::Battle,
    picker: Picker,
    seed: u64,
    scene: &'static str,
    geom: Option<Geom>,
}

impl Live {
    /// 起点与静态证人**同源**：`frame::demo_battle()` 打一桌、走 `DEMO_TURNS` 回合。
    /// 于是"第一帧摘要等于 其一 那个数"量的才是重构没改语义，而不是两次各自打桌的巧合。
    pub fn new() -> Live {
        Live::from_battle(frame::demo_battle())
    }

    /// 换一桌牌起活。`new()` 只管装机帧那一桌（它得与静态证人同源，所以不许被顺手改掉）；
    /// 这一支存在的理由是**测**：开局那一桌才有 0 费的开端牌，"点两下真能落子"那句话只有在
    /// 有可落之窗时才是可证的，拿 karma=0 的中盘桌去证，永远只能得到"业力不足"。
    pub fn from_battle(battle: crate::battle::Battle) -> Live {
        Live {
            battle,
            picker: Picker::default(),
            seed: frame::DEMO_SEED,
            scene: frame::DEMO_SCENE,
            geom: None,
        }
    }

    /// 当前这一帧的指令表（含态层）。与 `demo_frame()` 唯一的差别是 `marks` 不再恒空。
    pub fn frame(&self) -> Frame {
        let v = self.battle.view();
        let l = touch::layout_of(&v);
        let marks = self.picker.marks(&l);
        crate::display::frame_prims(&v, self.seed, self.scene, &marks)
    }

    pub fn set_geom(&mut self, geom: Geom) {
        self.geom = Some(geom);
    }

    pub const fn geom(&self) -> Option<Geom> {
        self.geom
    }

    /// 当前盘面的区表。与 `frame()` 走的是同一支 `layout_of`，所以"点得着的"就是"画得出的"。
    /// `pub` 是给 host 侧对撞工具的：它要把同一批区换算成设备矩形打出来，好让 `--tap` 的坐标
    /// 不是手抄的数（抄一份坐标，就等于允许两侧各点各的）。
    pub fn layout(&self) -> touch::Layout {
        touch::layout_of(&self.battle.view())
    }

    /// 一次触摸。DOWN 只记态，UP 才可能产命令——抬起时"按与抬同区"才算点成，
    /// 所以一根手指划过去不会下单（`Picker::release` 那条判据，电池 M165 钉着）。
    pub fn on_touch(&mut self, action: Action, x: i32, y: i32) -> Touch {
        let region = self.geom.and_then(|g| g.hit_dev(&self.layout(), x, y));
        match action {
            Action::Cancel => {
                self.picker.press(None);
                Touch { action, x, y, region, line: None, fed: None }
            }
            Action::Down => {
                self.picker.press(region);
                Touch { action, x, y, region, line: None, fed: None }
            }
            Action::Up => {
                let tap = self.picker.release(region);
                let line = match &tap {
                    Tap::Line(s) => Some(s.clone()),
                    _ => None,
                };
                let fed = line.as_deref().map(|s| self.feed(s));
                Touch { action, x, y, region, line, fed }
            }
        }
    }

    /// 一行 → 命令层 → 引擎。触屏只产 `p <idx> <Pn>`，但这一支对任何行都如实分派，
    /// 因为 host 侧要用它复放同一串点按。
    pub fn feed(&mut self, line: &str) -> Fed {
        match command::execute(&mut self.battle, command::parse(line)) {
            Exec::Done => Fed::Done,
            Exec::Print(s) => Fed::Print(s),
            Exec::Refused(e) => Fed::Refused(e),
            Exec::Quit(msg) => Fed::Quit(msg),
        }
    }

    /// 选中的那张手牌（只为日志与测断言）。
    pub const fn selected(&self) -> Option<usize> {
        self.picker.selected()
    }
}

impl Default for Live {
    fn default() -> Self {
        Live::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::Prim;
    use crate::md3::{color, lane, layout, state};

    /// 现读机型 23124RN87C 的那档缓冲（第十八步·其三 之后真机给的就是它）。
    const BUF: (usize, usize) = (1600, 720);

    /// 本帧在这档缓冲上量出的几何，下面所有硬编码坐标都由它推出：
    /// 画布 1240×1028 → k=0.7004（受高度限制）、off=(366,0)（只有横向留边）。
    /// 留在这里而不是散在各测里，是因为**改了它就必改这些测**——那正是这两句要的效果。
    const K: f32 = 0.7004;
    const OFF_X: i32 = 366;

    fn live_with_geom() -> (Live, Geom) {
        let mut live = Live::new();
        let frame = live.frame();
        let geom = Geom::of(BUF.0, BUF.1, &frame);
        live.set_geom(geom);
        (live, geom)
    }

    /// 开局那一桌（一回合都没走）。它是**唯一**能让落子真被引擎接住的起点：手牌第一张是 0 费的
    /// 「开端」，而装机帧那桌 karma 已经花光（现读 0），点什么都只会得到"业力不足"。
    /// 所以"两下点成、画面真变了"那句话只能在这儿证；那句话不许换成 demo 桌去将就。
    fn starter_table() -> Live {
        let b = crate::battle::Battle::new_mainline(
            frame::DEMO_SEED,
            crate::model::Faction::Ember,
            crate::meta::enemy_faction_for(1),
            crate::battle::Difficulty::Normal,
            Vec::new(),
            1,
        );
        let mut live = Live::from_battle(b);
        live.set_geom(Geom::of(BUF.0, BUF.1, &live.frame()));
        live
    }

    /// 区 → 设备矩形与它的中心。
    fn center(geom: &Geom, l: &touch::Layout, want: Region) -> (i32, i32) {
        let r = geom.pairs(l).into_iter().find(|(g, _)| *g == want).map(|(_, r)| r);
        let r = r.expect("这一区在设备上应有落点");
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    /// 一次完整点按（按下＋抬起同点）。测里反复要用它，写开一次比抄八遍强。
    fn tap(live: &mut Live, at: (i32, i32)) -> Touch {
        live.on_touch(Action::Down, at.0, at.1);
        live.on_touch(Action::Up, at.0, at.1)
    }

    /// 指令表里那块**恰好覆盖** r 的矩形的填充色（`bordered`/`card_cell` 用的就是区矩形本身）。
    fn fill_of(frame: &Frame, r: &Rect) -> String {
        frame
            .prims
            .iter()
            .find_map(|p| match p {
                Prim::Block { x, y, w, h, fill, .. } if (*x, *y, *w, *h) == (r.x, r.y, r.w, r.h) => Some(fill.clone()),
                _ => None,
            })
            .expect("那一格的矩形没进指令表")
    }

    #[test]
    fn the_live_first_frame_is_the_static_frame_not_a_new_one() {
        // 持住状态这一改动**不许**改语义：静态证人（其一 那个 digest）与活帧起点必须同数。
        let live = Live::new();
        let statics = frame::demo_frame();
        let lived = live.frame();
        assert_eq!(
            frame::frame_digest(&lived),
            frame::frame_digest(&statics),
            "改成活帧后第一帧的摘要变了——那说明起点不再与静态那一桌同源"
        );
        assert_eq!(lived.prims.len(), statics.prims.len(), "prim 条数也须一致");
        // 这一读数是**重新基准**过的：其六 那人裁 ③ 多画了一枚控件（一块胶囊＋一行字），帧必然变。
        // 旧值 `0xe501cd91a1df7681`（其一 立下的那一帧，板上还没有按钮）记在这儿，是为了改完之后
        // 还能问一句"到底变了多少"；它不许被就地覆盖成"反正变了"那种无据的数。
        // 其七 D2 又动了一次：我方前排「火星」那一行从 `2 / 4` 变成 `2 / 3→4`（牌面阈 3、同列敌牌带
        // 【同敌阈+1】⇒ 生效 4），帧上**只多了那一行的三个字符**，prim 条数一格没变（上面那条断言就是
        // 数它的）。归因是逐帧 diff 出来的：seed 7 那一桌只有 r4/r6/r7/r8 这四帧的文本面各差一行。
        assert_eq!(frame::frame_digest(&lived), 0x5e455ded8b459152, "其七 D2 阈值成对后的现读数（上一档 0x214fb059bca0765b＝其六 按钮；再上一档 0xe501cd91a1df7681＝其一 无按钮）");
    }

    #[test]
    fn the_scale_and_offset_are_the_ones_the_pixels_were_drawn_with() {
        // 命中用的是**画这一帧时**那把尺。这两个数是下面每条硬编码坐标的地基，也必须是真机日志里
        // 打出来的那两个（`paint_board` 的 `k=`／`off=`），否则"测里对、屏上错"就没人管。
        let (_, geom) = live_with_geom();
        assert!((geom.scale.k - K).abs() < 5e-4, "k 现读 {:.4}，与写下的 {K} 不同档", geom.scale.k);
        assert_eq!(geom.scale.off_x, OFF_X);
        assert_eq!(geom.scale.off_y, 0, "1600×720 上这档画布没有纵向留边");
        assert_eq!((geom.board_w, geom.board_h), (1240, 1028), "画布尺寸变了，下面那些坐标全要重算");
    }

    #[test]
    fn the_tap_lands_on_the_cell_that_was_drawn_there() {
        // 「点的就是看见的那格」：拿绘制侧同一支 `at` 正算出的手牌矩形中心去命中，必须回到那一格。
        let live = Live::new();
        let l = live.layout();
        let geom = Geom::of(BUF.0, BUF.1, &live.frame());
        for want in [Region::Hand(0), Region::Hand(1)] {
            let (x, y) = center(&geom, &l, want);
            assert_eq!(geom.hit_dev(&l, x, y).as_ref(), Some(&want), "{want:?} 的中心点到了隔壁");
        }
    }

    #[test]
    fn every_drawn_pixel_of_a_control_hits_that_control() {
        // 不只中心：把设备矩形**逐格**扫一遍。半开半闭与取整的接缝都在这条尺的射程里——
        // 反向除法那种写法错的正是边缘那一列，而那种错只在这条尺上现形（中心点永远是对的）。
        let live = Live::new();
        let l = live.layout();
        let geom = Geom::of(BUF.0, BUF.1, &live.frame());
        let pairs = geom.pairs(&l);
        for (want, r) in pairs.iter().filter(|(g, _)| g.is_control()) {
            assert!(r.w > 0 && r.h > 0, "{want:?} 在设备上没有面积");
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    assert_eq!(geom.hit_dev(&l, x, y).as_ref(), Some(want), "{want:?} 里像素 ({x},{y}) 点到了隔壁");
                }
            }
            // 右下那条**不含**的边：格住自己的范围外一格（缝上那点归下一格或谁也不归）。
            let outside = geom.hit_dev(&l, r.x + r.w, r.y + r.h);
            assert_ne!(outside.as_ref(), Some(want), "{want:?} 把右下那条边也算进了自己");
        }
    }

    /// 命中框 == 绘制框，按**读数**钉：这 13 块是 `paint` 往缓冲里落的那四个一组（`at` 原点＋`len`
    /// 宽高），不是从 `to_dev` 现算出来再比给自己——那样两边一起错也照绿。改任何一档 k、版面或
    /// `dev_rect` 的写法，这条必红，红了就重量一遍"点得到的是不是画出来的那一格"。
    /// 红的时候把**每一块**都列出并报数：一次换算漂移通常只咬在边缘那一格，第一处不是全部落点。
    #[test]
    fn the_hit_rect_is_the_painted_rect() {
        let live = Live::new();
        let l = live.layout();
        let geom = Geom::of(BUF.0, BUF.1, &live.frame());
        let pairs = geom.pairs(&l);
        let want: [(Region, (i32, i32, i32, i32)); 13] = [
            (Region::PlayerCell(0), (400, 331, 196, 73)),
            (Region::PlayerCell(1), (601, 331, 196, 73)),
            (Region::PlayerCell(2), (803, 331, 196, 73)),
            (Region::PlayerCell(3), (1005, 331, 196, 73)),
            (Region::EndTurn, (1089, 420, 112, 34)),
            (Region::Hand(0), (400, 482, 95, 64)),
            (Region::Hand(1), (500, 482, 95, 64)),
            (Region::Hand(2), (601, 482, 95, 64)),
            (Region::Hand(3), (702, 482, 95, 64)),
            (Region::Hand(4), (803, 482, 95, 64)),
            (Region::Hand(5), (904, 482, 95, 64)),
            (Region::Hand(6), (1005, 482, 95, 64)),
            (Region::Hand(7), (1106, 482, 95, 64)),
        ];
        let mut off: Vec<(Region, (i32, i32, i32, i32), (i32, i32, i32, i32))> = Vec::new();
        for (region, edge) in want {
            let got = pairs
                .iter()
                .find(|(g, _)| *g == region)
                .map(|(_, r)| (r.x, r.y, r.w, r.h));
            if got != Some(edge) {
                off.push((region, got.unwrap_or((-1, -1, -1, -1)), edge));
            }
        }
        assert!(
            off.is_empty(),
            "命中框与绘制框差一格的控件共 {} 块（实测 ≠ 画出的读数）：{off:?}",
            off.len()
        );
        assert_eq!(
            pairs.iter().filter(|(g, _)| g.is_control()).count(),
            want.len(),
            "控件区多了一块或少了一块：上面那张表只钉了 8 张手牌＋4 个我方格＋一枚结束回合按钮"
        );
    }
    #[test]
    fn the_letterbox_and_the_boards_own_margin_are_not_targets() {
        // 两道"看起来能点"的空处：画布外的留边（366 列）与画布内的外边距（MARGIN=48 单位）。
        // 前者是黑边、后者是底色，屏上都没有内容，所以手指落进去必须是什么也不接。
        let live = Live::new();
        let l = live.layout();
        let geom = Geom::of(BUF.0, BUF.1, &live.frame());
        let canvas_right = geom.scale.at(OFF_X, geom.board_w);
        for x in 0..OFF_X {
            assert_eq!(geom.hit_dev(&l, x, 367), None, "留边第 {x} 列也接了点");
        }
        let first_column = geom.scale.at(OFF_X, layout::MARGIN);
        for x in OFF_X..first_column {
            assert_eq!(geom.hit_dev(&l, x, 367), None, "画布自己的外边距第 {x} 列也接了点");
        }
        for x in canvas_right..BUF.0 as i32 {
            assert_eq!(geom.hit_dev(&l, x, 367), None, "画布右侧留边第 {x} 列也接了点");
        }
        // 第一列真有区的地方，就是绘制侧真正放下第一个控件的那一列。
        assert_eq!(geom.hit_dev(&l, first_column, 367), Some(Region::PlayerCell(0)));
    }

    #[test]
    fn a_tap_before_any_paint_hits_nothing() {
        // 没有几何就没有命中——这一格防的是"拿现读尺寸临场造一把没画过的尺"。
        let mut live = Live::new();
        let t = live.on_touch(Action::Down, 699, 367);
        assert_eq!(t.region, None);
        assert!(live.geom().is_none());
        live.set_geom(Geom::of(BUF.0, BUF.1, &live.frame()));
        assert_eq!(live.on_touch(Action::Down, 699, 367).region, Some(Region::PlayerCell(1)), "记下尺之后同一处仍点不着，那尺是假的");
    }

    #[test]
    fn every_tap_pair_reaches_the_engine_and_answers_in_its_own_words() {
        // 装机帧那桌**下不动子**（现读 karma=0，手牌无 0 费开端牌）。这测钉的正是这件事：
        // 运输必须把每一对点按都送到引擎门口，并且把引擎那句原话带回来——一句都不许壳自己编。
        let (live, geom) = live_with_geom();
        let l = live.layout();
        let empties: Vec<usize> = live
            .battle
            .view()
            .player_front
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_none())
            .map(|(i, _)| i)
            .collect();
        assert!(!empties.is_empty(), "装机帧那桌该有空位——没空位的话这条尺就空跑了");
        let mut lines = 0;
        for card in 0..l.hand.cells.len() {
            for slot in &empties {
                let mut trial = Live::new();
                trial.set_geom(geom);
                tap(&mut trial, center(&geom, &l, Region::Hand(card)));
                let up = tap(&mut trial, center(&geom, &l, Region::PlayerCell(*slot)));
                let line = up.line.expect("两下点成却没产行——命中或词表断了");
                assert_eq!(line, format!("p {card} P{}\n", slot + 1), "产出的行不是那一条词表形状");
                match up.fed {
                    Some(Fed::Done) => {}
                    Some(Fed::Refused(why)) => assert!(!why.is_empty(), "引擎拒了却没给理由"),
                    other => panic!("落子行的答复不该是 {other:?}"),
                }
                lines += 1;
            }
        }
        // 现读：这 24 对（8 张手牌 × 3 个空位）全部被拒，理由都是"业力不足：需X，当前0"。
        assert_eq!(lines, 8 * 3, "扫到的对数与手牌×空位的乘积不符");
        let mut live = Live::new();
        live.set_geom(geom);
        tap(&mut live, center(&geom, &l, Region::Hand(0)));
        let up = tap(&mut live, center(&geom, &l, Region::PlayerCell(1)));
        assert_eq!(up.fed, Some(Fed::Refused("业力不足：需5，当前0".into())), "装机帧那一发的现读答复漂了：{:?}", up.fed);
    }

    #[test]
    fn a_starter_card_lands_on_the_board_and_the_frame_moves() {
        // 闭环的另一半：真有一发被引擎接住、并且**画面跟着变**。这一句只能在开局那桌证（见 `starter_table`）。
        let mut live = starter_table();
        let geom = live.geom().expect("开局那桌也该有尺");
        let l = live.layout();
        let before = frame::frame_digest(&live.frame());
        tap(&mut live, center(&geom, &l, Region::Hand(0)));
        let up = tap(&mut live, center(&geom, &l, Region::PlayerCell(1)));
        assert_eq!(up.line, Some("p 0 P2\n".into()));
        assert_eq!(up.fed, Some(Fed::Done), "0 费的开端落 0 费都不行，那引擎侧断了：{:?}", up.fed);
        let after = frame::frame_digest(&live.frame());
        assert_ne!(after, before, "命令被引擎接住了，画面却没变——那说明帧没读活桌");
        // 落子之后牌从手上没了：手牌少一张，态层也该彻底清干净（选中那张已被 take）。
        let v = live.battle.view();
        assert_eq!(v.hand.len(), 4, "落子后手牌数没减");
        assert_eq!(live.selected(), None, "落子之后还留着选中态");
        assert!(live.picker.marks(&l).is_empty(), "落子之后仍要盖态层");
        // 且屏上那一格真换了内容：那格从空格变成有牌，指令表里多了一行牌名。
        let names = live
            .frame()
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::Label { text, .. } if text == "开端"))
            .count();
        assert!(names >= 1, "落下去的那张牌在指令表里没有名字");
    }

    /// 人裁 ③ 那一枚按钮的**运输证人**：手指落在它上面，产出的那一行要真走进引擎、
    /// 并且把回合翻过去。前一条测只证了带坐标的 `p`；`e` 是零参数命令，它没有别的入口，
    /// 所以"触屏能走完一整个回合"这句话只能在这儿证——手势侧只点得着落子、过回合仍要靠打字，
    /// 等于闭环没闭。
    #[test]
    fn the_end_turn_button_turns_the_page_in_the_engine() {
        let mut live = starter_table();
        let geom = live.geom().expect("开局那桌也该有尺");
        let l = live.layout();
        let before_turn = live.battle.view().player.turn;
        let before = frame::frame_digest(&live.frame());

        // 先选一张牌，再点按钮：按钮那一下必须把这份选中态一起清掉。
        // 留着它的话，下一格我方格会拿这张**上一回合的**牌去落子——那是比"点不动"更坏的一种错。
        tap(&mut live, center(&geom, &l, Region::Hand(0)));
        assert_eq!(live.selected(), Some(0), "起点：先让手里真的有一份选中态");

        let up = tap(&mut live, center(&geom, &l, Region::EndTurn));
        assert_eq!(up.region, Some(Region::EndTurn), "按钮中心点到的不是按钮");
        assert_eq!(up.line, Some("e\n".into()), "点按钮没产出结束回合那一行：{:?}", up.line);
        assert_eq!(up.fed, Some(Fed::Done), "那一行没被引擎侧接住：{:?}", up.fed);
        assert_eq!(live.selected(), None, "过完回合还留着选中态——下一格会落上一回合那张牌");

        let after = live.battle.view().player.turn;
        assert!(after > before_turn, "回合数没推进（前 {before_turn} 后 {after}）：产出了 `e` 却没走到引擎");
        assert_ne!(frame::frame_digest(&live.frame()), before, "引擎翻了回合，画面却没变");
        assert!(live.picker.marks(&l).is_empty(), "按钮抬起后仍盖着态层");
    }

    #[test]
    fn a_drag_never_becomes_an_order() {
        // 按在牌上、抬在格上＝拖拽。本帧不立那条手势，它的正确结局是什么也不做（M165 的同一条判据，
        // 这一格第一次有真调用方）。
        let (mut live, geom) = live_with_geom();
        let l = live.layout();
        live.on_touch(Action::Down, center(&geom, &l, Region::Hand(0)).0, center(&geom, &l, Region::Hand(0)).1);
        let (px, py) = center(&geom, &l, Region::PlayerCell(1));
        let up = live.on_touch(Action::Up, px, py);
        assert_eq!(up.line, None, "划过去被读成了一次下单");
        assert_eq!(up.fed, None, "没产行却喂了引擎");
    }

    #[test]
    fn a_cancel_clears_the_press_without_producing_a_line() {
        let (mut live, geom) = live_with_geom();
        let l = live.layout();
        let at = center(&geom, &l, Region::Hand(0));
        live.on_touch(Action::Down, at.0, at.1);
        let cancel = live.on_touch(Action::Cancel, at.0, at.1);
        assert_eq!(cancel.line, None);
        // 抬起时按下态已清：同区抬起不该点成（pressed=None 与 at=Some 不等）。
        let up = live.on_touch(Action::Up, at.0, at.1);
        assert_eq!(up.line, None, "取消之后补一次抬起却点成了");
        assert_eq!(live.selected(), None, "取消不该顺手选中");
    }

    #[test]
    fn the_press_lays_the_md3_state_layer_on_that_cell_only() {
        // 态层第一次有真调用方，也是它第一次**被手指驱动**。判据不是"帧摘要变了"那么宽：
        // 按下那张格的填充必须正好等于 `md3` 那一档混色，其余格一个都不许变。
        let (mut live, geom) = live_with_geom();
        let l = live.layout();
        let empty = live.frame();
        let rects = l.hand.cells.clone();
        let (hx, hy) = center(&geom, &l, Region::Hand(0));
        live.on_touch(Action::Down, hx, hy);
        let pressed = live.frame();
        let base = lane::PLAYER_FILL;
        let want = state::blend_hex(base, color::ON_SURFACE, touch::Layer::Press.alpha());
        assert_ne!(base, want, "MD3 那档按下混色与底色同值＝态层白做，这条尺也就没内容");
        for (i, (region, r)) in rects.iter().enumerate() {
            let (got, was) = (fill_of(&pressed, r), fill_of(&empty, r));
            if *region == Region::Hand(0) {
                assert_eq!(got, want, "手牌 0 按下了，填充却不是 MD3 那一档");
            } else {
                assert_eq!(got, was, "只按了手牌 0，第 {i} 张却跟着变了");
            }
        }
        // 抬起（选中那张牌）后态层不消失，但**换的是焦点那一档**——MD3 里 focus 与 pressed 同值，
        // 所以这里必须同色；那条等值由 md3 自己的秩序尺钉着，这一格只负责"运输之后它真在屏上"。
        live.on_touch(Action::Up, hx, hy);
        assert_eq!(fill_of(&live.frame(), &rects[0].1), want, "按下与焦点同值这条 MD3 规矩在像素侧漂了");
    }

    #[test]
    fn a_refused_line_is_the_engines_own_words() {
        // 拒绝理由必须**原样**是引擎那句，不是这里另写一句。
        let mut live = Live::new();
        let fed = live.feed("zz 1 P1");
        match fed {
            Fed::Refused(why) => assert!(why.contains("未知命令"), "{why}"),
            other => panic!("未知词该被拒，拿到 {other:?}"),
        }
    }

    #[test]
    fn the_log_line_names_the_action_coords_the_region_and_the_answer() {
        let (mut live, geom) = live_with_geom();
        let l = live.layout();
        let at = center(&geom, &l, Region::Hand(0));
        let down = live.on_touch(Action::Down, at.0, at.1);
        let text = down.log_line();
        assert_eq!(text, format!("touch down x={} y={} region=Hand(0)", at.0, at.1), "按下那一行的形状变了");
        let up = live.on_touch(Action::Up, at.0, at.1);
        assert!(!up.log_line().contains("line="), "只选中没产行，日志里不该有 line=：{}", up.log_line());
        let (px, py) = center(&geom, &l, Region::PlayerCell(1));
        let placed = live.on_touch(Action::Down, px, py);
        assert_eq!(placed.log_line(), format!("touch down x={px} y={py} region=PlayerCell(1)"));
        let done = live.on_touch(Action::Up, px, py);
        let text = done.log_line();
        assert!(text.contains("line=\"p 0 P2\""), "{text}");
        assert!(text.contains("fed=refused("), "{text}");
        assert!(text.contains("业力不足"), "{text} 该带上引擎那句");
        // 落空的那一下：区是 none，且没有 line=／fed=（那才是"什么都没发生"的读法）。
        let none = live.on_touch(Action::Down, 10, 367).log_line();
        assert_eq!(none, "touch down x=10 y=367 region=none");
    }
}

