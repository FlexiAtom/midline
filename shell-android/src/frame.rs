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
//! 壳侧**像素编码器**：把 `display::Frame` 那张指令表写进一块 RGBA8888 缓冲。
//!
//! ## 这是第二份编码器，不是第二个真值源
//! 上一帧的两行字探针（`text.rs` 里那两个 `LINE_*`）验的是"字能不能上屏"，本件验的是
//! "**这张盘面**能不能上屏"。区别在于本文件一行画面决定都不写：色值、圆角、字号、线宽、
//! 虚线段长全由 `Prim` 带下来，位置全由 `touch::layout_of` 带下来。
//! 于是"同一状态在两个壳里读出两个色"这一族债在结构上无处发生——SVG 与像素读的是同一份表。
//!
//! ## 对应表（SVG 语义 → 像素语义）
//! | 指令 | SVG | 这里 |
//! |---|---|---|
//! | `Block{fill}` | `fill` 属性 | 圆角内区域逐像素覆色 |
//! | `Block{edge}` | `stroke`／`stroke-width`／`stroke-dasharray` | 沿轮廓**两侧各让 width/2** 的环（与 SVG 同口径，不是内描边） |
//! | `Block{opacity}` | `opacity` 属性（合成器做 alpha） | 帧缓冲没有合成器 ⇒ 就地按系数混色 |
//! | `Label{Anchor::Start}` | `text-anchor="start"` | 落笔点在 x 右移 |
//! | `Label{Anchor::End}` | `text-anchor="end"` | 先量推进宽，落笔点在 x **左移** |
//! | `Label{em}` | `font-size`（dp） | `em × k` 再交给 `Face::px_scale`（那条 1.448 的乘法只住 `text.rs`） |
//!
//! ## 为什么圆角判定全整数
//! 真机与 host 的**逐像素相同**是本帧的验收尺。浮点那条路能不能走到相同，取决于编译器的两个
//! 默认值：`mul` 与 `add` 会不会被折成一条 FMA（折了就是**一次**舍入，不折是**两次**），以及
//! 有没有 fast-math 把 `round` 附近的运算重排。Rust 默认两者都不做，但我不打算把一格像素对撞
//! 押在"默认值恰好如此"上——所以这块的判据一律走**双倍整数坐标**（像素中心写成 `2p+1`，半宽与
//! 半径各乘二），判据里一次浮点、一次除法都没有。
//! 剩下的浮点只有两处，都在**换算**而不是在判定：`at`/`len`/`em` 那两次乘 `k`，和 `scale_of`
//! 里那一次居中除法。它们是纯乘除，两侧同规范同结果；真哪天两侧读出 1 像素的差，先在这两处找，
//! 别在 `covers` 里找——那里没有浮点可查。

use crate::display::{self, Anchor, Frame, Prim};
use crate::md3::{color, layout};
use crate::text::{self, Atlas, Face};

/// 一帧的**唯一**一把尺：用户单位（`touch::Layout` 那 1240 一族）→ 设备像素。
///
/// 只许等比、不许拉伸；两侧留的边是 letterbox 的结果，不是"版式里有个边距"——
/// 那件事仍然只住在 `md3::layout::MARGIN` 里。
#[derive(Copy, Clone, Debug)]
pub struct Scale {
    /// 每单位多少像素。
    pub k: f32,
    /// letterbox 留边。**已经 `round` 过，所以是整数**——一条落在半像素上的留边没有意义，
    /// 而把它记成 `f32` 只会让每个读它的人再问一遍"那小数谁吃掉了"。
    pub off_x: i32,
    pub off_y: i32,
}

/// **本仓唯一的 Scale 构造口**：横向纵向各算一档，取小的那个（谁都别被裁掉），再居中。
///
/// 输入是**缓冲**尺寸（`ANativeWindow_Buffer` 给的 `width×height`），不是窗口尺寸：
/// 真机现读 `buffer 1548x720 / stride=1552`，那 4 列填充属于步长、不属于画面（§3.8.1 的失手
/// 就是把步长当宽度用）。触摸坐标接上时（#132）也要走这把尺的反向，所以"只有一处构造"这件事
/// 是命中与绘制能对上的前提。
pub fn scale_of(buf_w: usize, buf_h: usize, board_w: i32, board_h: i32) -> Scale {
    let (bw, bh) = (board_w.max(1) as f32, board_h.max(1) as f32);
    let k = ((buf_w as f32 / bw).min(buf_h as f32 / bh)).max(0.0);
    Scale {
        k,
        off_x: ((buf_w as f32 - bw * k) / 2.0).round() as i32,
        off_y: ((buf_h as f32 - bh * k) / 2.0).round() as i32,
    }
}

impl Scale {
    /// 一个单位坐标 → 缓冲里的整数像素（含本侧的 letterbox 位移）。
    ///
    /// `pub` 是给**尺**用的：host 探针与命中换算都要问"用户单位 (x,y) 落在哪个像素"，
    /// 而这条乘法一旦在外面被重写一遍，就有了第二个真值源——命中与绘制对不上时先在那里找。
    pub fn at(&self, o: i32, u: i32) -> i32 {
        (o as f32 + u as f32 * self.k).round() as i32
    }

    /// 一个单位**长度** → 像素长度（不掺位移：位移进左边、不进宽度）。
    pub fn len(&self, u: i32) -> i32 {
        (u as f32 * self.k).round() as i32
    }

    /// MD3 那档 em → 本缓冲里的 em（`text.rs` 再乘它那 1.448）。
    fn em(&self, em: i32) -> f32 {
        em as f32 * self.k
    }
}

/// 用户单位矩形 → 设备像素矩形 `[x, y, w, h]`。**绘制侧与命中侧共用这一支**。
///
/// 原点走 `at`（letterbox 位移进左边），宽高走 `len`（不掺位移）——这不是两种写法里挑了一种，
/// 而是"像素是谁落的"决定的：`paint` 往缓冲里落的正是这四个数，命中框要是不照它们算，
/// 就会有一格像素画出来了却点不到（或反过来）。两端各问一次 `at` 再取差那种写法看着更准，
/// 现量在本机型这档 k 上 13 块控件里有 6 块与绘制侧差一格（PlayerCell 四块差在最后一行、
/// Hand(1)/Hand(2) 差在最后一列；其六 新加的那枚按钮不在这 6 块里，命中框与绘制框同数），而那正是"眼睛看到的最外一列"与"判据说的最外一列"的分歧。
pub fn dev_rect(s: &Scale, x: i32, y: i32, w: i32, h: i32) -> [i32; 4] {
    [s.at(s.off_x, x), s.at(s.off_y, y), s.len(w), s.len(h)]
}

/// 一次栅格化的现读量（进 logcat，也进档 §2 那张表）。
#[derive(Copy, Clone, Debug, Default)]
pub struct Painted {
    pub scale: f32,
    pub off_x: i32,
    pub off_y: i32,
    pub blocks: u32,
    pub labels: u32,
    /// 一个字形都没落成的行数——这是 #134（子集缺字）在画面上的出口：缺字不报错，只少像素。
    pub blank_labels: u32,
    /// 填色与描边**都没落到一个像素**的块数。这是同一族出口的另一面：块画没墨也不报错。
    /// 它与"整块落在缓冲外"共用一个读数——那正是 #133 换版式后最可能的失败形态。
    pub empty_blocks: u32,
    /// 落笔时图集里已有的格数（懒建：一帧付一次栅格代价，之后同字号同字免费）。
    pub rasters: u32,
}

/// 演示用的那一桌牌。**种子与回合数住在这里而不是 `entry.rs`**，因为 host 侧比对工具
/// 与真机必须跑同一桌：写进平台文件就成了"真机那一侧自己选了一桌"，两侧读出的差值无从归因。
pub const DEMO_SEED: u64 = 7;
pub const DEMO_TURNS: usize = 6;
pub const DEMO_SCENE: &str = "装机帧";

/// 打一桌、走若干回合，**把这一桌交出去**（不编帧）。
///
/// 拆出来是因为第十八步·其四起壳里那份盘面是**活的**：`input::Live` 要持住这个 `Battle`
/// 才能接住一行命令。留在这里而不是搬进 `input.rs`，是因为"哪一桌牌"这个选择必须两侧同源
/// ——`demo_frame()`（静态证人）与 `Live::new()`（交互起点）调的是同一支函数，
/// 于是"改成活帧后第一帧摘要没变"这句话量的才是语义，而不是两次各自打桌的巧合。
pub fn demo_battle() -> crate::battle::Battle {
    let mut b = crate::battle::Battle::new_mainline(DEMO_SEED, crate::model::Faction::Ember, crate::meta::enemy_faction_for(1), crate::battle::Difficulty::Normal, Vec::new(), 1);
    for _ in 0..DEMO_TURNS {
        crate::meta::auto_turn(&mut b);
    }
    b
}

/// 打一桌、走若干回合、把终盘的快照编成指令表。纯引擎调用，零平台依赖、零 IO。
///
/// `auto_turn` 走的是战斗自己的 `rng`（不是系统时间也不是线程随机数），所以同 seed 在
/// x86 与 aarch64 上必然打出同一个盘面——这一条是本帧像素对撞能成立的前提，写在码旁边是因为
/// 它一旦破了（哪天引擎换成 `SystemTime` 做种），症状是"对撞差满屏"而根因在这里。
pub fn demo_frame() -> Frame {
    let v = demo_battle().view();
    display::frame_prims(&v, DEMO_SEED, DEMO_SCENE, &[])
}

/// 一帧指令表 → 缓冲里的一块画面。**纯写像素**，不锁窗口、不碰平台。
///
/// 底色由调用方先铺（`text::fill_bg`，那里还有一颗给截屏找位移的定位点）——本函数只往上叠，
/// 因为它不知道那颗点是壳的尺而不是画面的内容。
pub fn paint(frame: &Frame, face: &Face, atlas: &mut Atlas, dst: &mut [u8], buf_w: usize, buf_h: usize, stride: usize) -> Painted {
    let s = scale_of(buf_w, buf_h, frame.width, frame.height);
    let mut st = Painted {
        scale: s.k,
        off_x: s.off_x,
        off_y: s.off_y,
        ..Default::default()
    };
    for p in &frame.prims {
        match p {
            Prim::Block { x, y, w, h, radius, fill, edge, opacity } => {
                st.blocks += 1;
                let [px, py, pw, ph] = dev_rect(&s, *x, *y, *w, *h);
                let rect = Round::new(px, py, pw, ph, s.len(*radius));
                let ink = color::rgb(fill);
                // SVG 里 `opacity` 是整个节点的不透明度，且**同时作用于描边**——这里同口径。
                let a = opacity_alpha(*opacity);
                let mut ink_px = stamp(dst, buf_w, buf_h, stride, rect.outer_box(), ink, a, |px, py| rect.covers(px, py));
                if let Some(e) = edge {
                    let width = s.len(e.width).max(1);
                    // 段长跟着 k 走：SVG 那边 dasharray 写在 viewBox 单位里，浏览器替它缩放；这里没人替，
                    // 用未缩放的段长就等于"描边缩放、虚线不缩放"，横屏小 k 时虚线明显偏粗。
                    let dash = e.dashed.then(|| {
                        (
                            s.len(layout::DASH_ON).max(1),
                            s.len(layout::DASH_ON).max(1) + s.len(layout::DASH_OFF).max(1),
                        )
                    });
                    ink_px += stamp(dst, buf_w, buf_h, stride, rect.outset(width).outer_box(), color::rgb(&e.stroke), a, |px, py| {
                        ring_hit(&rect, width, dash, px, py)
                    });
                }
                if ink_px == 0 {
                    st.empty_blocks += 1;
                }
            }
            Prim::Label { x, y, em, fill, anchor, text } => {
                st.labels += 1;
                let em_px = s.em(*em);
                let left = match anchor {
                    Anchor::Start => s.at(s.off_x, *x),
                    Anchor::End => s.at(s.off_x, *x) - face.advance_width(text, em_px).round() as i32,
                };
                let drawn = atlas.blit(face, dst, buf_w, stride, buf_h, text, left, s.at(s.off_y, *y), em_px, color::rgb(fill));
                if drawn.is_none() {
                    st.blank_labels += 1;
                }
            }
        }
    }
    st.rasters = atlas.rasters();
    st
}

/// 一帧指令表的**内容摘要**（FNV-1a 64 位，逐字段小端）。
///
/// 这是像素对撞之前那道更便宜的闸：两侧各自打出同一桌、各自算这一个数——数不同 ⇒ 分叉在
/// **引擎或编码**（种子、排布、色值串），数同而像素不同 ⇒ 分叉在**栅格器**。少了这一格，
/// "满屏都对不上"这句话只能靠猜归因。摘要只吃 `Prim` 里的字段，不吃缓冲尺寸，所以它证的是
/// "同一张指令表"，不是"同一张画面"——后者由逐像素对撞负责。
pub fn frame_digest(frame: &Frame) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hv = OFFSET;
    let mut eat = |b: &[u8]| {
        for x in b {
            hv = (hv ^ *x as u64).wrapping_mul(PRIME);
        }
    };
    eat(&frame.width.to_le_bytes());
    eat(&frame.height.to_le_bytes());
    eat(&[frame.prims.len() as u8]);
    for p in &frame.prims {
        match p {
            Prim::Block { x, y, w, h, radius, fill, edge, opacity } => {
                eat(&[0]);
                for v in [*x, *y, *w, *h, *radius] {
                    eat(&v.to_le_bytes());
                }
                eat(fill.as_bytes());
                match edge {
                    None => eat(&[0]),
                    Some(e) => {
                        eat(&[1]);
                        eat(e.stroke.as_bytes());
                        eat(&e.width.to_le_bytes());
                        eat(&[e.dashed as u8]);
                    }
                }
                // f64 走 bits：同一个 0.85 两侧字节相同，但格式化后再解析可能引入一次舍入。
                eat(&opacity.map_or(0u64.to_le_bytes(), |o| o.to_bits().to_le_bytes()));
            }
            Prim::Label { x, y, em, fill, anchor, text } => {
                eat(&[1]);
                for v in [*x, *y, *em] {
                    eat(&v.to_le_bytes());
                }
                eat(fill.as_bytes());
                eat(&[match anchor {
                    Anchor::Start => 0,
                    Anchor::End => 1,
                }]);
                eat(text.as_bytes());
            }
        }
    }
    hv
}

/// 折行那张表与**真字体**推进宽度的对账读数：返回 `(全角比值, 半角比值)`，都是 `advance ÷ em`。
///
/// `md3::typo::ADVANCE_{CJK,ASCII}` 是我自定的比值，整张牌面的截断预算全按它算；真字体里这两档
/// 实际是多少，只有引上字体这条链才量得出来（上一帧这两格读数在 `entry.rs` 的探针里，本帧探针
/// 撤了，读数不能跟着丢）。字串与 em 住在**这里**而不是 `entry.rs`：host 探针量的是同一串，
/// 两侧各选各的字，比值就没有可比性。比值与选哪档 em 无关（推进宽度线性于 `PxScale`），
/// 取 `TITLE_LARGE` 只是为了与牌面上最多的那档字号同源。
pub fn advance_probe(face: &Face) -> (f64, f64) {
    let em = crate::md3::typo::TITLE_LARGE as f32;
    (
        (face.advance_width("中", em) / em) as f64,
        (face.advance_width("D", em) / em) as f64,
    )
}

/// `opacity` → 8 位覆盖率。没有这一档就是实心（`u8::MAX`），`blend` 在满覆盖处原样落回目标色。
fn opacity_alpha(opacity: Option<f64>) -> u8 {
    match opacity {
        None => u8::MAX,
        Some(o) => text::coverage(o as f32),
    }
}

/// 描边环的判定：轮廓两侧各让 `width/2`，所以是"放大后的外沿"减"缩小后的内沿"。
/// 虚线只切**沿边方向**的相位：横边走 x、竖边走 y（角上按远的那一档归，视觉上是一根连续的线）。
/// `dash` 是 `Some((实段, 周期))`，两者都已经是**设备像素**（在 `paint` 里过 `Scale`）。
fn ring_hit(rect: &Round, width: i32, dash: Option<(i32, i32)>, px: i32, py: i32) -> bool {
    let outer = rect.outset(width);
    if !outer.covers(px, py) {
        return false;
    }
    let inner = rect.inset(width);
    if inner.covers(px, py) {
        return false;
    }
    let Some((on, period)) = dash else { return true };
    let (qx, qy) = outer.q(px, py);
    // 谁的返回值**更接近 0**，这一点就贴着谁的边界：贴上下边（qy 大）虚线就该沿 x 走，
    // 贴左右边（qx 大）该沿 y 走。写反的症状很好笑——一整条横边同时亮、同时灭，竖边的段长在原地跳。
    let (along, from) = if qx >= qy { (py, rect.y0) } else { (px, rect.x0) };
    (along - from).rem_euclid(period) < on
}

/// 一块缓冲上的逐像素覆色（含裁边）。`hit` 拿的是缓冲坐标；写像素本身交给 `text::paint_px`，
/// 这里只管**哪些格子归这块**。裁剪走 `skip/take` 而不是逐点 `if`，为的是循环界一眼可读：
/// 行取 `[y0,y1)∩[0,buf_h)`，列取 `[x0,x1)∩[0,buf_w)`，`hi.max(lo)` 兜住整块在界外那一格。
///
/// 返回**实际落笔的格数**。这个数不是给画面用的，是给"这块到底有没有墨"那句断言用的：
/// 一个 `hit` 恒假、或整块落在缓冲外的矩形，在屏上的症状只是"少了一格"，谁也不会报修。
#[allow(clippy::too_many_arguments, reason = "目标缓冲四格是一套，矩形与笔各是一套，拆开只会多一层结构")]
fn stamp(dst: &mut [u8], buf_w: usize, buf_h: usize, stride: usize, (x0, y0, x1, y1): (i32, i32, i32, i32), ink: [u8; 3], a: u8, hit: impl Fn(i32, i32) -> bool) -> u32 {
    let mut written = 0u32;
    for py in y0.max(0)..y1.min(buf_h as i32) {
        let row = (py as usize) * (stride * 4);
        let lo = x0.max(0) as usize;
        let hi = (x1.min(buf_w as i32) as usize).max(lo);
        for (px, out) in dst[row..].chunks_exact_mut(4).enumerate().skip(lo).take(hi - lo) {
            if hit(px as i32, py) {
                text::paint_px(out, ink, a);
                written += 1;
            }
        }
    }
    written
}

/// 圆角矩形，**全部按双倍整数坐标记**（半径也是双倍，字段名里的 `r` 即 `2×radius`）。
///
/// 双倍坐标的用处是让"像素中心"落在整数 `2p+1` 上、让"让出 width/2"落在整数 `±width` 上，
/// 于是判据里一次除法、一次浮点比较都没有。抬到 `i64` 是为了平方不溢出（缓冲再大也远够）。
#[derive(Copy, Clone, Debug)]
struct Round {
    cx: i64,
    cy: i64,
    hw: i64,
    hh: i64,
    r: i64,
    /// 虚线相位的原点（左右边按 `y0` 起算、上下边按 `x0`），与 `covers` 无关，只服务段长。
    x0: i32,
    y0: i32,
}

impl Round {
    /// 由**像素**矩形（左上、宽、高、圆角半径）造出来。负宽高按 0 处理，半径钳到半宽半高之内。
    fn new(x: i32, y: i32, w: i32, h: i32, radius: i32) -> Round {
        let (w, h) = (w.max(0) as i64, h.max(0) as i64);
        let r = (radius.max(0) as i64 * 2).min(w.min(h));
        Round {
            cx: 2 * x as i64 + w,
            cy: 2 * y as i64 + h,
            hw: w,
            hh: h,
            r,
            x0: x,
            y0: y,
        }
    }

    /// 描边的**外沿**：SVG 口径是"轮廓让出 width/2"，而 `width` 在这里进的正是双倍坐标——
    /// `+width` 个双倍单位＝`width/2` 个像素。半像素的描边（`k` 小的时候 `round` 出来就是 1）
    /// 因此不需要先舍入再乘，也不会因为四舍五入把一条细线抹掉。
    fn outset(&self, width: i32) -> Round {
        self.with(width as i64, width as i64)
    }

    /// 描边的**内沿**。缩到零以下这块就没了，`covers` 会因此恒假（见 `hw <= 0` 那格）。
    fn inset(&self, width: i32) -> Round {
        self.with(-(width as i64), -(width as i64))
    }

    fn with(&self, dw: i64, dr: i64) -> Round {
        let (hw, hh) = (self.hw + dw, self.hh + dw);
        Round {
            cx: self.cx,
            cy: self.cy,
            hw,
            hh,
            r: (self.r + dr).max(0).min(hw.min(hh)),
            x0: self.x0,
            y0: self.y0,
        }
    }

    /// 循环界：整块（含外扩那半条描边）的最小/最大像素，右开区间。四边各由 `cx±hw`／`cy±hh`
    /// **折一次半**得到——双倍坐标里 `hw` 已经是半宽，再折一次就成了"右界落在水平中心"，
    /// 于是每块只画出左上四分之一（真机现读：`working/device-blocks-paint-only-top-left-quadrant.md`）。
    fn outer_box(&self) -> (i32, i32, i32, i32) {
        let (x0, y0) = ((self.cx - self.hw) as f64 / 2.0, (self.cy - self.hh) as f64 / 2.0);
        let (x1, y1) = ((self.cx + self.hw) as f64 / 2.0, (self.cy + self.hh) as f64 / 2.0);
        (x0.floor() as i32, y0.floor() as i32, x1.ceil() as i32, y1.ceil() as i32)
    }

    /// SDF 的两分量（双倍坐标）：离"内切直角框"还有多远。都 ≤ 0 就是在正中那块方里。
    fn q(&self, px: i32, py: i32) -> (i64, i64) {
        let qx = (2 * px as i64 + 1 - self.cx).abs() - (self.hw - self.r);
        let qy = (2 * py as i64 + 1 - self.cy).abs() - (self.hh - self.r);
        (qx, qy)
    }

    /// 这个像素中心在不在圆角矩形里。
    fn covers(&self, px: i32, py: i32) -> bool {
        if self.hw <= 0 || self.hh <= 0 {
            return false;
        }
        let (qx, qy) = self.q(px, py);
        let (ox, oy) = (qx.max(0), qy.max(0));
        ox * ox + oy * oy <= self.r * self.r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md3::lane;

    /// 真机 §3.8.1 现读的那块横屏缓冲：**1548×720**（`stride=1552` 那 4 列属于步长不属于画面，
    /// 所以这里传的是宽度而不是步长——上一帧的失手就是把步长当宽度，整行错位）。
    #[test]
    fn the_fit_takes_the_smaller_ratio_and_centers_the_rest() {
        let s = scale_of(1548, 720, 1240, 948);
        let ky = 720.0 / 948.0;
        assert!((s.k - ky).abs() < 1e-6, "纵向更挤，k 必须取 ky；读到 {} 说明取成了横向", s.k);
        assert_eq!(s.off_y, 0, "贴满的那一侧不许再留边");
        assert_eq!(s.off_x, ((1548.0 - 1240.0 * ky) / 2.0).round() as i32, "多出来的宽度只许居中摊");
        assert!(s.off_x >= 0 && s.at(s.off_x, 1240) <= 1548, "fit 之后横向必须整张容得下");
    }

    /// 盘面比缓冲还高时**缩**而不是裁：这条是横屏那格唯一的失败形态（哪天 #133 把版式改高，
    /// 症状就是底边那行字不见了，而丢的那一行不会自己报修）。
    #[test]
    fn a_board_taller_than_the_buffer_shrinks_instead_of_cropping() {
        let s = scale_of(1548, 720, 1240, 2000);
        assert!((s.k - 720.0 / 2000.0).abs() < 1e-6);
        assert!(s.at(0, 2000) <= 720, "纵向溢出：贴到底算 {} 像素", s.at(0, 2000));
        assert!(s.at(s.off_x, 1240) <= 1548, "横向不该反而溢出");
    }

    #[test]
    fn an_empty_buffer_scales_to_nothing_rather_than_dividing_by_zero() {
        let s = scale_of(0, 0, 1240, 948);
        assert_eq!(s.k, 0.0, "零宽缓冲上一切长度都该是 0，而不是 NaN");
        assert_eq!(s.at(s.off_x, 100), 0);
        assert_eq!(s.len(500), 0);
    }

    /// 挖孔区那条 52 不许压在盘面上。这是**画面真正到边之后**才出现的一格：从前缓冲是 1548 宽，
    /// 系统替我们把那 52 列留成了安全区；现在缓冲是 1600 宽，安全区归我们自己负责。
    ///
    /// 为什么这里不拿那个 ARect 当输入：NDK **没有**可以在排布时同步读缺口的口。唯一通道是
    /// `onContentRectChanged` 那一路回调，而它给的是**可画矩形**、不是缺口本体——要判"哪一侧被让开"
    /// 还得拿它与 buffer 宽对读。本帧现读两把（同一臂、换个手持方向）：`l=52 t=0 r=1600` 与
    /// `l=0 t=0 r=1548` ⇒ 那道 52 会左右翻，它不是版面参数，所以生产里不许出现"拿 52 当输入"。
    ///
    /// 于是这颗尺钉的是**几何事实**而不是补偿逻辑：等比 fit 把 1240×1028 的盘面缩到 868 宽，
    /// 居中摊完剩下两侧各 366 列，本来就把那条 52 整个让在了画面之外（底色到边，内容不到边）。
    /// `NOTCH_INSET_PX` 是**测出来的**常数、只在测里出现：它是这台机（720×1600，
    /// `DisplayCutout insets=Rect(0,0 - 52,0)`／随旋转翻边）的现读值。
    ///
    /// 反面那颗（同一段里）证的是这条尺不是在量空气：把缓冲换成 900 宽，同一套 fit 只余 16 列边，
    /// 16 < 52 ⇒ 盘面会伸进挖孔区。所以"边距够"这句话是有条件的，条件由 `scale_of` 那一次除法给出。
    #[test]
    fn the_letterbox_margin_keeps_the_board_out_of_the_measured_cutout() {
        const NOTCH_INSET_PX: i32 = 52;
        let s = scale_of(1600, 720, 1240, 1028);
        assert_eq!(s.off_x, 366, "1600×720 上这帧的居中边距现读");
        assert!(
            s.off_x >= NOTCH_INSET_PX,
            "边距 {} < 挖孔 {} ⇒ 盘面被系统缺口咬住",
            s.off_x,
            NOTCH_INSET_PX
        );
        assert!(s.at(s.off_x, 1240) <= 1600, "让出缺口之后仍须整张容得下");
        // 非空转对照：同一颗断言换到窄缓冲上必须**成立不了**，否则上面那句是在量空气。
        let n = scale_of(900, 720, 1240, 1028);
        assert_eq!(n.off_x, 16, "窄缓冲的边距现读（这是对照，不是产品形状）");
        assert!(n.off_x < NOTCH_INSET_PX, "对照失效：窄缓冲也够边距，那句断言就没牙");
    }

    /// SVG 的描边是"轮廓两侧各让 width/2"，**不是内描边**。width=2（像素）时 10×10 那块的上沿
    /// 正好压在 y=-1 与 y=0 两行：y=1 已经回到块内，y=-2 已经在块外。
    #[test]
    fn a_stroke_sits_half_a_pixel_on_each_side_of_the_outline() {
        let r = Round::new(0, 0, 10, 10, 0);
        assert!(ring_hit(&r, 2, None, 5, 0), "轮廓上那一行必须有墨");
        assert!(ring_hit(&r, 2, None, 5, -1), "外让的那半像素也得有墨");
        assert!(!ring_hit(&r, 2, None, 5, 1), "内让只有半像素，第二行已经回到块里");
        assert!(!ring_hit(&r, 2, None, 5, -2), "外让同样不许越过半像素");
        assert!(!ring_hit(&r, 2, None, 5, 5), "正中间是填色，不是描边");
    }

    #[test]
    fn the_carved_corner_gets_no_fill() {
        let r = Round::new(0, 0, 10, 10, 4);
        assert!(!r.covers(0, 0), "半径 4 的角是空的，涂上它就变成了方角");
        assert!(r.covers(0, 5) && r.covers(5, 0), "边的中点必须有墨");
        assert!(r.covers(5, 5), "正中必须有墨");
        assert!(!r.covers(10, 5), "矩形是右开区间：x=10 已在块外");
        let sharp = Round::new(0, 0, 10, 10, 0);
        assert!(sharp.covers(0, 0), "半径 0 时角就是直角，别把圆角判定漏成空洞");
    }

    /// 循环界必须**包住整块**，不许只到中心。这一格量出来的是一条形状教训：`covers()` 是对的、
    /// `empty_blocks` 也只数"一个像素都没落"的块，于是"每块只画出左上四分之一"这种坏法五把尺全绿。
    /// 同界断言是它的可读形式，最后那一段"判据说该有墨的格子循环必须走到"才是要钉的不变式。
    #[test]
    fn the_loop_bounds_wrap_the_whole_rect_not_its_top_left_quadrant() {
        for (x, y, w, h, r) in [(0, 0, 10, 10, 0), (1, 2, 5, 3, 1), (-3, 4, 7, 2, 3), (1089, 420, 112, 34, 24), (0, 0, 1, 1, 0)] {
            assert_eq!(Round::new(x, y, w, h, r).outer_box(), (x, y, x + w, y + h), "({x},{y} {w}×{h} r{r}) 的循环界必须与那块像素矩形同界");
        }
        let square = Round::new(0, 0, 10, 10, 0);
        assert_eq!(square.outset(2).outer_box(), (-1, -1, 11, 11), "描边外让的那半像素在界内，环才闭合");
        assert_eq!(square.inset(2).outer_box(), (1, 1, 9, 9), "内沿同样不许少一圈");
        assert_eq!(square.outset(3).outer_box(), (-2, -2, 12, 12), "奇数描边落在半像素上：左界只许 floor、右界只许 ceil，谁也不许 trunc");
        // 判据说"该有墨"的每一格，循环都必须走到——这一句才是本帧真正要钉的不变式，上面那些同界
        // 断言只是它的可读形式。取带奇数描边的这块，是因为它的四边正落在半像素上：换 trunc 就少一列。
        let rect = Round::new(3, 5, 10, 10, 0).outset(3);
        let (x0, y0, x1, y1) = rect.outer_box();
        for py in 0..20 {
            for px in 0..20 {
                if rect.covers(px, py) {
                    assert!((x0..x1).contains(&px) && (y0..y1).contains(&py), "covers 判 ({px},{py}) 该有墨，循环界却走不到它");
                }
            }
        }
    }

    /// 虚线切的是**沿边方向**的相位：上边一行按 x 走，段长 6、周期 10。这条测同时抓两件事——
    /// 相位轴选错（那时整条横边会同时亮灭）与段长没跟着 k 缩放（`paint` 里那一格）。
    #[test]
    fn a_dash_is_on_for_its_segment_and_off_for_the_gap() {
        let r = Round::new(0, 0, 200, 40, 0);
        for period in 0..3 {
            for d in 0..10 {
                let x = period * 10 + d + 40;
                assert_eq!(ring_hit(&r, 2, Some((6, 10)), x, 0), d < 6, "x={x} 落在段内第 {d} 格，判错了");
            }
        }
        assert!(ring_hit(&r, 2, None, 40, 0), "不虚的那档：环上处处有墨");
    }

    #[test]
    fn opacity_only_softens_the_band_it_names() {
        assert_eq!(opacity_alpha(None), u8::MAX, "没有 opacity 就是实心");
        assert_eq!(opacity_alpha(Some(1.0)), u8::MAX);
        let spot = opacity_alpha(Some(lane::MIDLINE_BAND_SPOT));
        assert!(spot > 0 && spot < u8::MAX, "带中线的点彩档读成了 {spot}：它必须既非全透也非实心");
        assert_eq!(spot, text::coverage(lane::MIDLINE_BAND_SPOT as f32), "混色系数不该在这里再算一遍");
    }

    /// 越界与空转的兜底：整块矩形落在缓冲外时一个字节都不许写，也不许 panic（真机的块坐标来自
    /// `layout_of`，缓冲尺寸来自平台，两者今天对得上不代表明天对得上）。
    #[test]
    fn stamping_clips_to_the_buffer_and_writes_alpha_opaque() {
        let mut dst = vec![0u8; 4 * 4 * 4];
        stamp(&mut dst, 4, 4, 4, (-2, -2, 6, 6), [10, 20, 30], u8::MAX, |_, _| true);
        for px in dst.chunks_exact(4) {
            assert_eq!(&px[..3], &[10, 20, 30], "界内每一格都得覆到");
            assert_eq!(px[3], u8::MAX, "alpha 留 0 会被合成层当全透明整帧丢掉");
        }
        let mut untouched = vec![7u8; 4 * 4 * 4];
        assert_eq!(stamp(&mut untouched, 4, 4, 4, (9, 9, 20, 20), [1, 2, 3], u8::MAX, |_, _| true), 0);
        assert!(untouched.iter().all(|b| *b == 7), "整块在界外还写，就是踩了别人的缓冲");
        // 落笔格数是 `empty_blocks` 的唯一来源：恒假判据与界外同样读 0，界内满铺读 16。
        let mut none_hit = [0u8; 4 * 4 * 4];
        let mut all_hit = [0u8; 4 * 4 * 4];
        assert_eq!(stamp(&mut none_hit, 4, 4, 4, (0, 0, 4, 4), [1, 2, 3], u8::MAX, |_, _| false), 0);
        assert_eq!(stamp(&mut all_hit, 4, 4, 4, (0, 0, 4, 4), [1, 2, 3], u8::MAX, |_, _| true), 16);
    }

    /// 演示帧是**打两次同桌**：这一格证的不是"画得好"，是摘要这一路可复现——
    /// 真机与 host 各自独立跑 `demo_frame()`，只有引擎这条路确定，两侧那个数才允许相比。
    /// 哪天有人把种子换成系统时间，这里当场红（而像素对撞只会红满屏，归因不出）。
    #[test]
    fn the_same_seed_digests_to_the_same_number() {
        let mut a = demo_frame();
        let b = demo_frame();
        assert_eq!(frame_digest(&a), frame_digest(&b), "同 seed 同回合数，指令表却不同");
        assert!(a.prims.len() > 30, "一帧真盘面不该只有 {} 条指令", a.prims.len());
        // 改一个字段就得改数：否则摘要在量空气（比如它其实只吃了 width/height）。
        let idx = a.prims.iter().position(|p| matches!(p, Prim::Block { .. })).expect("演示帧里必有一块");
        let Prim::Block { x, .. } = &mut a.prims[idx] else { unreachable!() };
        *x += 1;
        assert_ne!(frame_digest(&a), frame_digest(&b), "改了坐标读数不变，摘要就没吃位置");
    }

    /// 演示帧的每一块都得落在它自己声明的画布里。这条量的是**几何**（不碰字体）：溢出在屏上的
    /// 表现是"最后一行没了"或"右边少一格"，那种症状没有证人，只能靠这里当场拦住。
    #[test]
    fn the_demo_frame_fits_inside_the_board_it_declares() {
        let f = demo_frame();
        assert_eq!(f.width, layout::BOARD_W, "盘面宽度只有一个真值");
        assert!(f.height > 0, "高度由排布推出，读出 0 说明排布没走");
        assert!(!f.prims.is_empty(), "指令表是空的，这条尺在量空气");
        let mut blocks = 0;
        let mut labels = 0;
        for p in &f.prims {
            match p {
                Prim::Block { x, y, w, h, .. } => {
                    blocks += 1;
                    assert!(
                        *x >= 0 && *y >= 0 && *w >= 0 && *h >= 0 && *x + *w <= f.width && *y + *h <= f.height,
                        "越界的块：{x},{y} {w}×{h}（画布 {}×{}）",
                        f.width,
                        f.height
                    );
                }
                Prim::Label { x, y, .. } => {
                    labels += 1;
                    assert!(*x >= 0 && *y >= 0, "负坐标的标签：{x},{y}");
                }
            }
        }
        assert!(blocks > 20, "一帧真盘面不该只有 {blocks} 块");
        assert!(labels > 10, "一帧真盘面不该只有 {labels} 行字");
    }
}
