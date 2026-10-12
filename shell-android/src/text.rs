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
//! 壳侧 text 面：把引擎发下来的 **em 数**换成像素，并把字形栅格存进一张 8 位 alpha 图集。
//!
//! ## 为什么换算只能在这一层做
//! `ab_glyph` 的 `PxScale` 分母**不是** `units_per_em`，而是 `ascent − descent`
//! （`scale.rs:85-95`，`h_scale_factor = scale.x / font().height_unscaled()`）。思源黑 CN 是
//! `(1160+288)/1000 = 1.448`，所以把 `typo::TITLE_LARGE` 那种"22"直接当 PxScale 传进去，
//! 字身会小 **30.9%**（本档 §3.7 正证三，实测反推出来的）。于是分工是：引擎只发 em 口径的数
//! （`md3::typo::*` 保持 CSS/MD3 语义，不掺密度、不掺这条链的怪癖），**壳负责把它变成像素**。
//! 换算漏进 token 表，那张表就不再是"版式真值的唯一住所"，而是一台每换一次字体就要跟着变的机器。
//!
//! ## 为什么摆放取整、栅格锚在 (0,0)
//! 栅格一律按 `position = point(0,0)` 出图，落笔时 `pen_x` 四舍五入到整数。这不是精度上的偷懒，
//! 是为了让**同一份代码在 host 与真机两侧逐字节可比**：一旦引入亚像素位移，截屏对撞就变成
//! "比谁都像但谁都不等"，那格读数就白付了。真实版式要做 subpixel 定位时，这条得重写而不是打补丁。
//!
//! ## 这一层不含平台代码
//! 窗口锁、`ANativeWindow_Buffer`、AAssetManager 都在 `entry.rs`——本模块只吃 `&mut [u8]`
//! 与 `&[u8]`，所以它能在 host 上单独跑（首帧/换帧、图集大小这类代价不必等设备就能量）。

use ab_glyph::{Font as _, FontArc, Glyph, GlyphId, InvalidFont, PxScale, ScaleFont, point};
use std::collections::HashMap;

/// 图集行宽。栅格按 shelf 打包：一行放不下就另起一行，所以单字宽度上限就是这个数——
/// 57 dp 的汉字（墨宽约 53 px）离上限还很远，撑不下的那一档是给连字/斜体的西文留的余量。
const ATLAS_W: u32 = 512;

// ── 本帧载荷：两行字、字号、版面三数、底色 ──────────────────────────────────
//
// 这组常量住这里而**不住 `entry.rs`**，理由只有一个：**复制一份数字的对撞不算对撞**。
// host 侧的比对工具（`#[path]` 引本模块）与真机走的必须是同一份源码，否则两侧各自自洽、
// 差值读出来却不知该怪谁——那正是本仓在布局尺上付过学费的那一格。
// 平台侧（窗口锁、资产读、logcat）仍全在 `entry.rs`。

/// 盘面第一行。选字的原则是"必含最难的那几档"：全角方块字、笔画密的 `平衡`、以及中英混排。
pub const LINE_HEAD: &str = "中文平衡";
/// 第二行：`2D` 是半角拉丁（走 `ADVANCE_ASCII` 那档），结尾的 `衡` 再回到全角，用来露出行尾对齐。
pub const LINE_BODY: &str = "中线 2D 衡";

/// 一帧两行的字号，全部取自引擎那张阶梯——壳不许自己造字号数，那是 `md3::typo` 唯一的住所。
pub const EM_HEAD: f32 = crate::md3::typo::DISPLAY_SMALL as f32; // 36
pub const EM_BODY: f32 = crate::md3::typo::TITLE_LARGE as f32; // 22

/// 左上角那颗定位点的边长。它**不是版式数**（不进 `md3`），用途只有一个：截屏里找到它的颜色，
/// 就拿到了"缓冲(0,0) 落在屏幕哪一行"的现读位移，字形对撞先减它。
pub const DOT: usize = 8;

/// 版面三数：左边距、第一行基线、第二行基线。真机与 host 各调一次，读回来的整数必须相同。
pub fn layout() -> (i32, i32, i32) {
    let x = crate::md3::space::XL;
    let b1 = (DOT + EM_HEAD.round() as usize) as i32;
    let b2 = b1 + EM_BODY.round() as i32 + crate::md3::space::M;
    (x, b1, b2)
}

/// 底色＝`md3::color::SURFACE`；左上角那颗 `DOT×DOT` 是 `md3::color::MIDLINE`。
/// 只写 RGB 三格、alpha 恒 255——RGBA8888 的 alpha 若留 0，合成层会把整帧当全透明丢掉。
pub fn fill_bg(dst: &mut [u8], w: usize, h: usize, stride: usize) {
    let bg = crate::md3::color::rgb(crate::md3::color::SURFACE);
    let dot = crate::md3::color::rgb(crate::md3::lane::MIDLINE);
    for (y, row) in dst.chunks_mut(stride * 4).take(h).enumerate() {
        for (x, px) in row.chunks_exact_mut(4).enumerate().take(w) {
            let c = if y < DOT && x < DOT { dot } else { bg };
            px[..3].copy_from_slice(&c);
            px[3] = 255;
        }
    }
}

/// 覆盖率 → 8 位 alpha。`round` 而不是截断：截断会让最淡的那一档 AA（f<1/255）整批归零，
/// 字边缘出现肉眼可见的锯齿台阶。host 侧的对撞脚本用同一个式子，两侧必须逐字节一样。
#[inline]
pub fn coverage(f: f32) -> u8 {
    (f.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// 单层 alpha 合成。`+127` 是四舍五入，不是防溢出——`a=0` 时它照样落回 `bg`
/// （`bg*255+127 < bg*255+255`），所以调用方不必为空格单独开分支。
#[inline]
pub fn blend(bg: u8, ink: u8, a: u8) -> u8 {
    let a = u32::from(a);
    let v = (u32::from(bg) * (255 - a) + u32::from(ink) * a + 127) / 255;
    v as u8
}

/// 一份字体字节＋那把 em→PxScale 的乘子。
pub struct Face {
    font: FontArc,
    /// `height_unscaled / units_per_em`（思源黑 CN ＝ 1.448）。
    ratio: f32,
    upem: f32,
    height_unscaled: f32,
    ascent: f32,
    descent: f32,
}

/// 读不出可用度量。字体字节来自 APK 资产，是系统边界，所以这两条不是防御性的摆设。
#[derive(Debug, PartialEq, Eq)]
pub enum FaceError {
    /// 轮廓表读不出来（截断、非 sfnt、版本不认识）。
    BadBytes(InvalidFont),
    /// `head.unitsPerEm` 缺失 ⇒ 那把乘子无从算起。
    NoUpem,
}

/// 一个字的栅格：`w×h` 的 alpha 网格，加上它在基线坐标系里的左上角偏移。
pub struct Raster {
    pub w: u32,
    pub h: u32,
    /// 相对落笔点的水平偏移（`px_bounds.min.x`， italic/边距可为负）。
    pub dx: i32,
    /// 相对基线的垂直偏移（`px_bounds.min.y`，y 向下为正 ⇒ 绝大多数汉字为负）。
    pub dy: i32,
    pub alpha: Vec<u8>,
}

impl Raster {
    /// 墨的包围盒（alpha>0 的那一版），在栅格自身坐标里。空格/未映射返回 `None`。
    pub fn ink(&self) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (i, &a) in self.alpha.iter().enumerate() {
            if a == 0 {
                continue;
            }
            let (x, y) = ((i as u32) % self.w, (i as u32) / self.w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        if x0 > x1 {
            None
        } else {
            Some((x0, y0, x1, y1))
        }
    }
}

/// 图集里的一格：位置在图集缓冲内，`dx/dy` 是它的落笔偏移，`advance` 是排版推进。
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub dx: i32,
    pub dy: i32,
    pub advance: f32,
}

/// 一次摆完一串字之后被写过的最外面那块矩形（缓冲坐标，含端点右开区间）。
#[derive(Clone, Copy, Debug, Default)]
pub struct Touched {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
    /// 实际参与排版的字数（未映射的字符不计）。
    pub drawn: u32,
}

impl Touched {
    pub fn width(&self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }
    pub fn height(&self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }
}

impl Face {
    pub fn try_from_vec(bytes: Vec<u8>) -> Result<Self, FaceError> {
        let font = FontArc::try_from_vec(bytes).map_err(FaceError::BadBytes)?;
        let upem = font.units_per_em().ok_or(FaceError::NoUpem)?;
        Ok(Face {
            ratio: font.height_unscaled() / upem,
            upem,
            height_unscaled: font.height_unscaled(),
            ascent: font.ascent_unscaled(),
            descent: font.descent_unscaled(),
            font,
        })
    }

    /// 设计单位里的三格：em 方、ascent、descent。`ratio` 由前两者算出，写在这儿是为了让
    /// "为什么是 1.448"这句话在被问的时候有出处，而不是一个从文档抄来的数。
    pub fn upem(&self) -> f32 {
        self.upem
    }
    pub fn ascent_unscaled(&self) -> f32 {
        self.ascent
    }
    pub fn descent_unscaled(&self) -> f32 {
        self.descent
    }
    pub fn ratio(&self) -> f32 {
        self.ratio
    }
    pub fn glyph_count(&self) -> usize {
        self.font.glyph_count()
    }

    /// em（MD3 那档字号，dp 口径）→ 这条链要的 PxScale。**本仓唯一该做这次乘法的地方。**
    pub fn px_scale(&self, em: f32) -> PxScale {
        PxScale {
            x: em * self.ratio,
            y: em * self.ratio,
        }
    }

    /// 码点 → 字形号。0 是 notdef：按"没这个字"处理，让调用方决定跳还是画豆腐块
    /// （本壳跳，因为子集之外的字该由版式那条路发现，而不是被一个方框藏起来）。
    pub fn glyph_of(&self, ch: char) -> Option<u16> {
        let id = self.font.glyph_id(ch);
        (id.0 != 0).then_some(id.0)
    }

    pub fn advance(&self, gid: u16, em: f32) -> f32 {
        self.font.as_scaled(self.px_scale(em)).h_advance(GlyphId(gid))
    }

    /// 栅格一个字。锚在 (0,0)，所以 `dx/dy` 就是 `px_bounds.min`。
    pub fn raster(&self, gid: u16, em: f32) -> Option<Raster> {
        let outlined = self.font.outline_glyph(Glyph {
            id: GlyphId(gid),
            scale: self.px_scale(em),
            position: point(0.0, 0.0),
        })?;
        let b = outlined.px_bounds();
        let w = b.width().max(0.0).ceil() as u32;
        let h = b.height().max(0.0).ceil() as u32;
        if w == 0 || h == 0 {
            return None; // 空格这类无轮廓的字
        }
        let mut alpha = vec![0u8; (w * h) as usize];
        outlined.draw(|x, y, f| {
            if let Some(slot) = alpha.get_mut((y as usize) * (w as usize) + (x as usize)) {
                *slot = coverage(f);
            }
        });
        Some(Raster {
            w,
            h,
            dx: b.min.x.floor() as i32,
            dy: b.min.y.floor() as i32,
            alpha,
        })
    }
}

/// 一张 8 位 alpha 图集＋「(字形, em) → 格子」的索引。
///
/// 为什么要图集而不是每次现栅格：一次 `outline_glyph` 要走 CFF 那台栈机，而一帧里同一个字会
/// 被画很多次（血条上的数字、牌名）。图集把这件事变成**每字号每字一次**，代价换成内存——
/// 两档的差价就是 `fill_eager` 与 `rasters()` 那两格读数。
#[derive(Default)]
pub struct Atlas {
    buf: Vec<u8>,
    rows: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_h: u32,
    cells: HashMap<(u16, u32), Cell>,
    rasters: u32,
}

impl Atlas {
    pub fn new() -> Self {
        Self::default()
    }

    /// 已发生过的真栅格次数。首帧全栅格与懒建的差价全靠它对账。
    pub fn rasters(&self) -> u32 {
        self.rasters
    }

    /// 图集缓冲字节数（alpha 一格一字节，行宽 `ATLAS_W`）。
    pub fn bytes(&self) -> usize {
        (self.rows as usize) * (ATLAS_W as usize)
    }

    pub fn cell(&self, gid: u16, em: f32) -> Option<&Cell> {
        self.cells.get(&(gid, em.to_bits()))
    }

    /// 取格，没有就栅格一张并塞进图集（shelf 打包：当前行放不下就另起一行）。
    pub fn get_or_rasterize(&mut self, face: &Face, gid: u16, em: f32) -> Option<&Cell> {
        let key = (gid, em.to_bits());
        if self.cells.contains_key(&key) {
            return self.cells.get(&key);
        }
        let r = face.raster(gid, em)?;
        let advance = face.advance(gid, em);
        self.rasters += 1;
        if self.cursor_x + r.w > ATLAS_W {
            self.cursor_y += self.row_h;
            self.cursor_x = 0;
            self.row_h = 0;
        }
        let x = self.cursor_x;
        let y = self.cursor_y;
        self.cursor_x += r.w;
        self.row_h = self.row_h.max(r.h);
        let need = usize::try_from((y + r.h) * ATLAS_W).ok()?;
        if self.buf.len() < need {
            self.buf.resize(need, 0);
        }
        self.rows = self.rows.max(y + r.h);
        for j in 0..r.h as usize {
            let src = j * r.w as usize;
            let dst = ((y as usize + j) * (ATLAS_W as usize)) + x as usize;
            self.buf[dst..dst + r.w as usize].copy_from_slice(&r.alpha[src..src + r.w as usize]);
        }
        self.cells.insert(
            key,
            Cell {
                x,
                y,
                w: r.w,
                h: r.h,
                dx: r.dx,
                dy: r.dy,
                advance,
            },
        );
        self.cells.get(&key)
    }

    /// 把字体里所有字形一次性栅格进来（"首帧全栅格"那一档），返回新栅格了几个字。
    pub fn fill_eager(&mut self, face: &Face, em: f32) -> u32 {
        let before = self.rasters;
        for gid in 1..face.glyph_count() as u16 {
            self.get_or_rasterize(face, gid, em);
        }
        self.rasters - before
    }

    /// 摆一行字进 `dst`（32 位 RGBA、行步长按**像素**计）。`baseline` 是缓冲坐标里那行的基线。
    ///
    /// `None` ＝ 一个字都没摆进去（整串都不在子集里），与"摆了但都落在裁剪外"区分开。
    #[allow(clippy::too_many_arguments, reason = "目标缓冲四格是一套，字串三格是一套，拆开只会多一层结构")]
    pub fn blit(
        &mut self,
        face: &Face,
        dst: &mut [u8],
        dst_w: usize,
        stride: usize,
        dst_h: usize,
        text: &str,
        x: i32,
        baseline: i32,
        em: f32,
        ink: [u8; 3],
    ) -> Option<Touched> {
        let mut pen = x as f32;
        let mut t = Touched::default();
        let mut box_: Option<(u32, u32, u32, u32)> = None;
        for ch in text.chars() {
            let Some(gid) = face.glyph_of(ch) else { continue };
            let Some(cell) = self.get_or_rasterize(face, gid, em) else { continue };
            let cell = *cell;
            let ox = pen.round() as i32 + cell.dx;
            let oy = baseline + cell.dy;
            pen += cell.advance;
            for j in 0..cell.h as usize {
                let py = oy + j as i32;
                if py < 0 || py as usize >= dst_h {
                    continue;
                }
                for i in 0..cell.w as usize {
                    let px = ox + i as i32;
                    if px < 0 || px as usize >= dst_w {
                        continue;
                    }
                    let a = self.buf[(cell.y as usize + j) * ATLAS_W as usize + cell.x as usize + i];
                    if a == 0 {
                        continue;
                    }
                    let o = (py as usize) * (stride * 4) + (px as usize) * 4;
                    dst[o] = blend(dst[o], ink[0], a);
                    dst[o + 1] = blend(dst[o + 1], ink[1], a);
                    dst[o + 2] = blend(dst[o + 2], ink[2], a);
                    dst[o + 3] = 255;
                }
            }
            let (x0, y0) = (ox.max(0), oy.max(0));
            let (x1, y1) = (
                (ox + cell.w as i32).min(dst_w as i32),
                (oy + cell.h as i32).min(dst_h as i32),
            );
            if x1 > x0 && y1 > y0 {
                box_ = Some(match box_ {
                    None => (x0 as u32, y0 as u32, x1 as u32, y1 as u32),
                    Some((bx0, by0, bx1, by1)) => (
                        bx0.min(x0 as u32),
                        by0.min(y0 as u32),
                        bx1.max(x1 as u32),
                        by1.max(y1 as u32),
                    ),
                });
            }
            t.drawn += 1;
        }
        if let Some((x0, y0, x1, y1)) = box_ {
            t.x0 = x0;
            t.y0 = y0;
            t.x1 = x1;
            t.y1 = y1;
        }
        (t.drawn > 0).then_some(t)
    }
}
