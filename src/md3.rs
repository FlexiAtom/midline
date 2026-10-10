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
//! MD3 数值表：本仓**唯一**住视觉常数的那间屋子。
//!
//! ## 为什么单开一个模块，而不是把常数留在 `canvas.rs`
//! 人 2026-10-11 令「2D版本的美术你完全基于MD3风格自定，要的只是流程，美术无所谓」⇒ 版式裁决位回到实现侧。
//! 裁决位一换手，"版式真值住在哪儿"就变成必须能被指出来的问题：留在 `canvas.rs` 里，它和排版代码混成一团，
//! 谁改了哪一格、改完跟谁撞色，都没有一个可指的点。搬进这张表之后 `canvas.rs` 只做**算位置**这一件事，
//! 凡颜色、圆角、字号、间距、栅格都从这里取——于是文末那条机检 `visual_constants_have_one_home`
//! 能说清一句话：**呈现面不许有第二个真值源**。这不是洁癖。首版那张 2D 帧上，"后排不可攻击"的灰字
//! 与"中线"那条色带各是我随手写的两个数，两处撞色时我看不出来；单一来源让"这个数写着哪儿"变成一次 grep。
//!
//! ## 与真的 MD3 差在哪儿（这一格必须自己说破）
//! Material 3 的取色是 **HCT**（色相-彩度-色调）体系：角色色由源色沿色调阶梯（tone 0–100）派生，
//! 暗色方案再从中取 tone 80／40 一类配对。本仓零第三方依赖，且**不打算为取色背上几百行 CAM16/HCT 感知色彩数学**
//! ——那条代价换的是美术精度，而人给的口径是「美术无所谓，要的只是流程」。所以关系是三档，别混着吹：
//! - **结构照抄**：角色命名（primary／on-primary／…-container、surface 家族、outline 家族、inverse 家族）、
//!   elevation 用色调而非投影、状态层那五个透明度档、shape 圆角阶梯、4dp 栅格、断点三档、type scale。
//!   这些是 MD3 真正有约束力的部分，也是流程要用的部分。
//! - **数值自定**：具体十六进制是我按本仓既有牌面色（我方蓝、敌侧红、造紫、中线金、业火橙）写定的，
//!   **不是** HCT 派生读数，也不是官方 token 的拷贝。凡自定处都带 `### 自定` 标记，不许拿自定值冒充规范值。
//! - **配对可校**：自定 ≠ 随手。每一对"字色／底色"由本模块 `contrast` 按 WCAG 相对亮度当场算，
//!   低于 AA 正文档 4.5:1 就红。规范我不重实现，规范里可验证的那半边我钉死。
//!
//! ## dp 与像素
//! 尺寸一律按 MD3 的 **dp**（密度无关）记。桌面 SVG 取 1dp = 1px；安卓按 `breakpoint::dp_to_px` 乘
//! `density/160`——真机实测窗口 720×1548 @ density 320 ⇒ 宽 360dp，落 compact 档
//! （读数出处：working/android-rust-shell-fused-source 那 11 行生命周期回调）。
//!
//! ## 规则锚
//! 本模块**不写**文档锚点，理由与 `canvas`／`tui` 那两条相同：它不新增任何规则真值，写锚会去动反向覆盖尺的生产面读数。
#![allow(dead_code)] // 表里还没被读走的那几格是**待件**（安卓栅格后端、交互态），不是缺陷；后端接上即撤这条豁免。

/// 角色色。命名即语义：`on_X` 永远指"画在 X 之上的那层内容色"，配对关系由名字表达，不靠注释维持。
pub mod color {
    // ### 自定：以下十六进制由我方按本仓牌面色写定（暗色方案），非 HCT 派生、非官方 token 拷贝。

    /// 基准面：一帧最底那层，比所有容器都深。
    pub const SURFACE: &str = "#10131a";
    /// 比基准还沉的一档：贴底、需要退到背景之后。
    pub const SURFACE_DIM: &str = "#0b0e14";
    /// 最亮的一档：被抬到最高的那一层（对话框一类）。
    pub const SURFACE_BRIGHT: &str = "#383f4b";

    /// elevation 0 的容器档＝直接落基准面。
    pub const SURFACE_CONTAINER_LOWEST: &str = "#080b11";
    /// elevation 1。
    pub const SURFACE_CONTAINER_LOW: &str = "#181d26";
    /// elevation 2。
    pub const SURFACE_CONTAINER: &str = "#1d222c";
    /// elevation 3。
    pub const SURFACE_CONTAINER_HIGH: &str = "#272d38";
    /// elevation 4：本帧里最高的一档（日志面板、手牌托盘这类常驻容器）。
    pub const SURFACE_CONTAINER_HIGHEST: &str = "#323843";
    /// 中性变体面：与容器同族但带色相，用来分块而不抬 elevation。
    pub const SURFACE_VARIANT: &str = "#2f3540";

    /// 主文本色。
    pub const ON_SURFACE: &str = "#e4e7f0";
    /// 次级文本／图标色。
    pub const ON_SURFACE_VARIANT: &str = "#c2c7d4";
    /// 描边色：要被看见，但不抢内容。
    pub const OUTLINE: &str = "#8d93a1";
    /// 弱化描边：分隔线、空格虚线框。
    pub const OUTLINE_VARIANT: &str = "#434a57";

    /// 主强调色（本仓语义＝我方阵营）。
    pub const PRIMARY: &str = "#a9c9f8";
    /// 画在 `PRIMARY` 之上的内容色。
    pub const ON_PRIMARY: &str = "#0b3055";
    /// 主色容器：填充面积更大、对比更软那一档（我方格子底）。
    pub const PRIMARY_CONTAINER: &str = "#1f3f66";
    /// 画在 `PRIMARY_CONTAINER` 之上的内容色。
    pub const ON_PRIMARY_CONTAINER: &str = "#d6e6fb";

    /// 次要强调色（本仓语义＝中线那道绝对边界）。
    pub const SECONDARY: &str = "#e7c877";
    /// 画在 `SECONDARY` 之上的内容色。
    pub const ON_SECONDARY: &str = "#3f3110";
    /// 次色容器。
    pub const SECONDARY_CONTAINER: &str = "#4a3d1a";
    /// 画在 `SECONDARY_CONTAINER` 之上的内容色。
    pub const ON_SECONDARY_CONTAINER: &str = "#f7e6b8";

    /// 第三色（本仓语义＝造牌 `crafted`）。
    pub const TERTIARY: &str = "#d7b8f2";
    /// 画在 `TERTIARY` 之上的内容色。
    pub const ON_TERTIARY: &str = "#3f2359";
    /// 第三色容器。
    pub const TERTIARY_CONTAINER: &str = "#3a2a52";
    /// 画在 `TERTIARY_CONTAINER` 之上的内容色。
    pub const ON_TERTIARY_CONTAINER: &str = "#ecdffc";

    /// 错误／敌侧（本仓语义＝可攻击的敌方前排与敌方持业者条）。
    pub const ERROR: &str = "#ffb4ab";
    /// 画在 `ERROR` 之上的内容色。
    pub const ON_ERROR: &str = "#601a16";
    /// 错误容器。
    pub const ERROR_CONTAINER: &str = "#6e2a2f";
    /// 画在 `ERROR_CONTAINER` 之上的内容色。
    pub const ON_ERROR_CONTAINER: &str = "#ffdcd7";

    /// 反色面：整块内容要倒过来时用（选中态、提示气泡）。
    pub const INVERSE_SURFACE: &str = "#e4e7f0";
    /// 画在 `INVERSE_SURFACE` 之上的内容色。
    pub const INVERSE_ON_SURFACE: &str = "#2b3039";
    /// 反色主色（＝亮色方案的主色）。
    pub const INVERSE_PRIMARY: &str = "#2f6fb5";

    /// 把 `#rrggbb` 解成三通道。SVG 后端要字符串、安卓栅格要像素，两个后端共用上面那**一份**字面量。
    pub const fn rgb(hex: &str) -> [u8; 3] {
        let b = hex.as_bytes();
        [nib(b[1]) * 16 + nib(b[2]), nib(b[3]) * 16 + nib(b[4]), nib(b[5]) * 16 + nib(b[6])]
    }

    const fn nib(b: u8) -> u8 {
        match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            _ => 0,
        }
    }
}

/// 语义色：牌面上"这块是哪一族"。MD3 没有敌方／业火这种角色，所以这一格是**我方扩展**；
/// 但它只许由 `color` 里的角色色拼出，或按同一配对纪律（亮面＋深字）另写，且另写的地方只有这一处。
pub mod lane {
    use crate::md3::color::*;

    /// 敌方前排格底。
    pub const ENEMY_FRONT_FILL: &str = ERROR_CONTAINER;
    /// 敌方前排描边／标签。
    pub const ENEMY_FRONT_EDGE: &str = ERROR;
    /// 敌方后排格底：不可攻击，退到中性容器，不给它错误色那种高对比。
    pub const ENEMY_BACK_FILL: &str = SURFACE_CONTAINER_HIGH;
    /// 我方格底。
    pub const PLAYER_FILL: &str = PRIMARY_CONTAINER;
    /// 我方描边／标签。
    pub const PLAYER_EDGE: &str = PRIMARY;
    /// 造牌（`crafted`）标记：覆盖本格自己的描边色。
    pub const CRAFT: &str = TERTIARY;
    /// 中线色带。
    pub const MIDLINE: &str = SECONDARY;
    /// 画在中线色带之上的文字色。
    pub const ON_MIDLINE: &str = ON_SECONDARY;
    /// 业火（`flame`）足额标记块。MD3 无此角色 ⇒ `### 自定`，按同一配对纪律写（配对由 tests 校）。
    pub const FLAME: &str = "#ffb686";
    /// 画在 `FLAME` 之上的内容色。
    pub const ON_FLAME: &str = "#4a2600";
    /// 空格子：底与点都走弱化档——空格的意思是"这里没东西"，不该比有东西的格子更抢眼。
    pub const EMPTY_FILL: &str = SURFACE_CONTAINER_LOWEST;
    /// 空格的点。
    pub const EMPTY_MARK: &str = OUTLINE_VARIANT;
    /// 次级读数（费／焰／技能串）：它是被读的字，所以取内容色而不是描边色。
    pub const META: &str = ON_SURFACE_VARIANT;
    /// 中线色带的"抬起"浓度。MD3 用色调表达 elevation，本帧用一条实色带表达"绝对边界"，
    /// 这一格是它独有的读数 ⇒ `### 自定`，MD3 无对应档位。
    pub const MIDLINE_BAND_SPOT: f64 = 0.85;
}

/// 状态层。MD3 表达交互态的方式不是改底色，而是在底色上叠一层 `on_X` 色并按下面的透明度混合。
/// 这五个数是照规范记的 ⇒ #132 的事件驱动层只该出现这几个数，不该自己发明新档。
pub mod state {
    /// 悬停 8%。
    pub const HOVER: f64 = 0.08;
    /// 聚焦 10%（键盘／手柄焦点环同值，靠描边形状区分）。
    pub const FOCUS: f64 = 0.10;
    /// 按下 10%。
    pub const PRESS: f64 = 0.10;
    /// 拖拽中 16%。
    pub const DRAG: f64 = 0.16;
    /// 禁用态内容 38%：压淡，不是隐藏。
    pub const DISABLED_CONTENT: f64 = 0.38;
    /// 禁用态容器 12%。
    pub const DISABLED_CONTAINER: f64 = 0.12;

    /// 按 MD3 口径把状态层混进底色：`out = base*(1-a) + overlay*a`，四舍五入。
    /// 桌面 SVG 走 `fill-opacity`、安卓栅格走这支函数——两后端必须算出同一个颜色，
    /// 否则"同一状态在两个壳里读出两个色"就是开了第二真值源。
    pub fn blend(base: [u8; 3], overlay: [u8; 3], a: f64) -> [u8; 3] {
        let mut out = [0u8; 3];
        let mut i = 0;
        while i < 3 {
            let v = base[i] as f64 * (1.0 - a) + overlay[i] as f64 * a;
            out[i] = (v + 0.5) as u8;
            i += 1;
        }
        out
    }
}

/// 形状阶梯：MD3 corner radius 档位。圆角只许从这七档里取，别处不许出现新的圆角数。
pub mod shape {
    /// 直角。
    pub const NONE: i32 = 0;
    /// 4dp：血条这类小面积填充。
    pub const EXTRA_SMALL: i32 = 4;
    /// 8dp：格子、面板——本帧默认角。
    pub const SMALL: i32 = 8;
    /// 12dp。
    pub const MEDIUM: i32 = 12;
    /// 16dp。
    pub const LARGE: i32 = 16;
    /// 28dp。
    pub const EXTRA_LARGE: i32 = 28;
    /// 999dp：胶囊档（足够大即等价 50%）。
    pub const FULL: i32 = 999;
}

/// 间距栅格：MD3 的 4dp 网格。所有留白必须是这一族数，于是"这格为什么空 12 不空 13"有一个可指的答案。
pub mod space {
    /// 栅格基数。
    pub const GRID: i32 = 4;
    /// 零档：要表达"这里不留白"就写它，别拿裸 `0` 冒充间距。
    pub const XXXS: i32 = 0;
    pub const XXS: i32 = 4;
    pub const XS: i32 = 8;
    pub const S: i32 = 12;
    pub const M: i32 = 16;
    pub const L: i32 = 20;
    pub const XL: i32 = 24;
    pub const XXL: i32 = 32;
    pub const XXXL: i32 = 48;
    /// 超栅格那一档：整帧外边距这种大留白。
    pub const HUGE: i32 = 64;
}

/// 字号阶梯：MD3 type scale 的字号档（dp）。本帧不引字体度量，只把这些档当**唯一合法的字号来源**。
pub mod typo {
    pub const DISPLAY_LARGE: i32 = 57;
    pub const DISPLAY_MEDIUM: i32 = 45;
    pub const DISPLAY_SMALL: i32 = 36;
    pub const HEADLINE_LARGE: i32 = 32;
    pub const HEADLINE_MEDIUM: i32 = 28;
    pub const HEADLINE_SMALL: i32 = 24;
    pub const TITLE_LARGE: i32 = 22;
    /// 16dp：MD3 里 title-medium 与 body-large 撞在同一档，本帧按"读内容的字"归 body。
    pub const BODY_LARGE: i32 = 16;
    pub const BODY_MEDIUM: i32 = 14;
    pub const BODY_SMALL: i32 = 12;
    /// 14dp：MD3 里 label-large 与 body-medium 同值——同值是规范，不是巧合。
    pub const LABEL_LARGE: i32 = 14;
    pub const LABEL_MEDIUM: i32 = 12;
    pub const LABEL_SMALL: i32 = 11;

    /// 字体族回退串。MD3 对应 Roboto／Noto；真机上可读的那一档由 #134 那格实测决定，这里只留 CJK 能落地的回退序。
    pub const FONT_FAMILY: &str = "Source Han Sans CN, Noto Sans CJK SC, sans-serif";

    /// 单个字符的推进宽度，以"字号倍数"计：CJK 占满一格。
    /// 口径与文本路一致——按 `chars()` 计数会把一行 ASCII 当一行中文那么宽，那是首版折行失真的根。
    pub const ADVANCE_CJK: f64 = 1.0;
    /// ASCII 推进宽度。
    pub const ADVANCE_ASCII: f64 = 0.55;
    /// 截断时给省略号预留的那一格。
    pub const ELLIPSIS_COST: f64 = 1.0;
}

/// 组件度量。MD3 规定了按钮 40dp、列表项 56dp 那一类；本帧的格子／血条／面板不是 MD3 的组件，
/// 所以数值是**自定**的——但自定的数必须落在 `space` 的栅格上（文末机检钉这条），且只住这一间屋子。
pub mod layout {
    use crate::md3::space::*;

    /// 桌面帧画布宽。**只用于桌面 SVG**；安卓按窗口实际宽高排，不读这一格。
    pub const BOARD_W: i32 = 1240;
    /// 帧外边距。
    pub const MARGIN: i32 = XXXL;
    /// 一列格之间的缝。
    pub const COL_GAP: i32 = XS;
    /// 盘面格高。
    pub const CELL_H: i32 = 104;
    /// 手牌格高：比盘面格矮——手牌是待选，不是在场。
    pub const HAND_H: i32 = 92;
    /// 持业者条宽／高。
    pub const BAR_W: i32 = 360;
    pub const BAR_H: i32 = L;
    /// 两根持业者柱之间的步长（必须容得下一整根条加间隙）。
    pub const HOLDER_STRIDE: i32 = 460;

    /// 第一行字的基线（顶部留白之后）。
    pub const TITLE_BASELINE: i32 = 40;
    /// 标题行之后的一次下落。
    pub const HEADER_DROP: i32 = 28;
    /// 规则行行距。
    pub const RULE_DROP: i32 = XL;
    /// 规则块之后的分隔。
    pub const RULE_GAP: i32 = XXL;
    /// 小节标题到其内容的距离。
    pub const SECTION_DROP: i32 = M;
    /// 大块之间的分隔。
    pub const BLOCK_GAP: i32 = XL;

    /// 盘面格内三行字的基线：名／值-阈／费-焰。
    pub const CELL_ROW1: i32 = 28;
    pub const CELL_ROW2: i32 = 64;
    pub const CELL_ROW3: i32 = 92;
    /// 手牌格内三行字的基线：序号／名／读数。
    pub const HAND_ROW1: i32 = XL;
    pub const HAND_ROW2: i32 = XXXL;
    pub const HAND_ROW3: i32 = 72;
    /// 格内容左边距（右对齐时同样是收边距）。
    pub const CELL_PAD: i32 = S;
    /// 窄内边距：手牌这种矮格子用它。
    pub const CELL_PAD_NARROW: i32 = XS;

    /// 业火足额标记块的边长／内缩。
    pub const FLAME_MARK: i32 = S;
    pub const FLAME_INSET: i32 = XS;

    /// 中线色带高，与带内文字基线。
    pub const MIDLINE_BAND_H: i32 = XL;
    pub const MIDLINE_BASELINE: i32 = M;

    /// 排与排的缝：后排→前排比前排→中线更宽，"不可攻击"那一排要退得更远。
    pub const ROW_GAP_BACK: i32 = S;
    pub const ROW_GAP_FRONT: i32 = M;

    /// 日志面板：内边距、行距、行数、总高。总高由前三者算出（`2×LOG_PAD + LOG_LINES×LOG_LINE_STEP`），
    /// 改任一都要让它重算——手抄一个数就会出"面板比内容高出一截空白"那种失真。
    pub const LOG_PAD: i32 = M;
    pub const LOG_LINE_STEP: i32 = XL;
    pub const LOG_LINES: usize = 6;
    pub const LOG_H: i32 = 176;

    /// 描边两档：MD3 boundary 1dp／强边界 2dp。
    pub const STROKE_HAIRLINE: i32 = 1;
    pub const STROKE_BOUNDARY: i32 = 2;
    /// 空格虚线的段长与间隔。
    pub const DASH_ON: i32 = S;
    pub const DASH_OFF: i32 = S;

    /// 帧末留白。
    pub const FOOT_GAP: i32 = XXL;
}

/// 断点与密度换算。宽度的算法真值只在这儿，`canvas.rs` 与安卓壳都从这条尺上读。
pub mod breakpoint {
    /// compact 上界（dp）。
    pub const COMPACT_MAX_DP: i32 = 600;
    /// medium 上界（dp），再宽是 expanded。
    pub const MEDIUM_MAX_DP: i32 = 840;

    /// 三种窗口档：版式按它选列数与外边距。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Bucket {
        /// ≤600dp：手机竖屏（真机 720px@320dpi ⇒ 360dp，实测落这一档）。
        Compact,
        /// 601–840dp。
        Medium,
        /// >840dp：桌面帧（1240px @ 1dp=1px ⇒ expanded）。
        Expanded,
    }

    /// dp → px，按 `density/160` 四舍五入。安卓栅格后端唯一的换算入口。
    pub const fn dp_to_px(dp: i32, density_dpi: u32) -> i32 {
        (dp * density_dpi as i32 + 80) / 160
    }

    /// px → dp（整除偏保守：把窗口读窄一档比读宽一档安全）。
    pub const fn px_to_dp(px: i32, density_dpi: u32) -> i32 {
        px * 160 / density_dpi as i32
    }

    /// 按宽度像素与密度归档。density 为 0 时按 1dp=1px——桌面后端没有 density 这个量。
    pub const fn classify(width_px: i32, density_dpi: u32) -> Bucket {
        let dp = if density_dpi == 0 { width_px } else { px_to_dp(width_px, density_dpi) };
        if dp <= COMPACT_MAX_DP {
            Bucket::Compact
        } else if dp <= MEDIUM_MAX_DP {
            Bucket::Medium
        } else {
            Bucket::Expanded
        }
    }
}

/// 线格式常数：SVG 文档头那两串。搬进来的理由不是凑数——文末那条机检把 `canvas.rs` 生产面的数字
/// 全禁了（`0`／`1`／`2` 之外一个都不许留），而 XML 声明的 `version="1.0"` 与命名空间 URI 里的 `2000`
/// 都是**线上格式规定的字面量**，改不得也无处可去。要让"数字只住 md3"这条规则成立，得给它们一个家。
pub mod svg {
    /// XML 声明。
    pub const PROLOG: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
    /// SVG 命名空间 URI。
    pub const NS: &str = "http://www.w3.org/2000/svg";
}

/// 按 elevation 档取底色：MD3 暗色方案的抬升方式是换色调而不是加投影，所以这一支是呈现面
/// 唯一的"这块面板该多高"出口——调用方给档号，颜色由表给。
pub const fn surface_at_elevation(level: u8) -> &'static str {
    match level {
        0 => color::SURFACE,
        1 => color::SURFACE_CONTAINER_LOW,
        2 => color::SURFACE_CONTAINER,
        3 => color::SURFACE_CONTAINER_HIGH,
        4 => color::SURFACE_CONTAINER_HIGHEST,
        _ => color::SURFACE_BRIGHT,
    }
}

/// WCAG 2.x 相对亮度：sRGB 线性化后加权和。
fn luminance(rgb: [u8; 3]) -> f64 {
    let weights = [0.2126f64, 0.7152, 0.0722];
    let mut sum = 0.0f64;
    let mut i = 0;
    while i < 3 {
        let v = rgb[i] as f64 / 255.0;
        let lin = if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        sum += weights[i] * lin;
        i += 1;
    }
    sum
}

/// 两个色之间的对比度（1.0–21.0）。它是**校表用的尺**，不是运行时算色：成不成在跑测试时定死。
pub fn contrast(a: &str, b: &str) -> f64 {
    let (x, y) = (luminance(color::rgb(a)), luminance(color::rgb(b)));
    let (hi, lo) = if x >= y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::breakpoint::{Bucket, COMPACT_MAX_DP, MEDIUM_MAX_DP, classify, dp_to_px, px_to_dp};
    use super::color::*;
    use super::layout::*;
    use super::shape::*;
    use super::space::*;
    use super::state::*;
    use super::typo::*;
    use super::{contrast, luminance, surface_at_elevation};

    // 本 mod 里凡"两个常数之间的大小关系"一律装进数组再 `windows(2)` 比，不写 `assert!(A < B)`。
    // 这张表的本体就是常数，直接比两个 const 会被 clippy 记一条常量断言；它建议的 `const {}`
    // 会把红灯挪成编译错误、把这族读数从测试面摘掉——而这里要的正是**能点名喊红的测**。

    /// 色值字面量的形状：七个字符、`#` 起头、六位小写十六进制。这条红了意味着有人塞进了 `red`、
    /// `#FFF` 或大写字面量——三个后端的解析器对这三种写法给三种结果，是最贵的一类漂移。
    #[test]
    fn every_color_token_is_lowercase_six_digit_hex() {
        let table: Vec<(&str, &str)> = vec![
            ("SURFACE", SURFACE),
            ("SURFACE_DIM", SURFACE_DIM),
            ("SURFACE_BRIGHT", SURFACE_BRIGHT),
            ("SURFACE_CONTAINER_LOWEST", SURFACE_CONTAINER_LOWEST),
            ("SURFACE_CONTAINER_LOW", SURFACE_CONTAINER_LOW),
            ("SURFACE_CONTAINER", SURFACE_CONTAINER),
            ("SURFACE_CONTAINER_HIGH", SURFACE_CONTAINER_HIGH),
            ("SURFACE_CONTAINER_HIGHEST", SURFACE_CONTAINER_HIGHEST),
            ("SURFACE_VARIANT", SURFACE_VARIANT),
            ("ON_SURFACE", ON_SURFACE),
            ("ON_SURFACE_VARIANT", ON_SURFACE_VARIANT),
            ("OUTLINE", OUTLINE),
            ("OUTLINE_VARIANT", OUTLINE_VARIANT),
            ("PRIMARY", PRIMARY),
            ("ON_PRIMARY", ON_PRIMARY),
            ("PRIMARY_CONTAINER", PRIMARY_CONTAINER),
            ("ON_PRIMARY_CONTAINER", ON_PRIMARY_CONTAINER),
            ("SECONDARY", SECONDARY),
            ("ON_SECONDARY", ON_SECONDARY),
            ("SECONDARY_CONTAINER", SECONDARY_CONTAINER),
            ("ON_SECONDARY_CONTAINER", ON_SECONDARY_CONTAINER),
            ("TERTIARY", TERTIARY),
            ("ON_TERTIARY", ON_TERTIARY),
            ("TERTIARY_CONTAINER", TERTIARY_CONTAINER),
            ("ON_TERTIARY_CONTAINER", ON_TERTIARY_CONTAINER),
            ("ERROR", ERROR),
            ("ON_ERROR", ON_ERROR),
            ("ERROR_CONTAINER", ERROR_CONTAINER),
            ("ON_ERROR_CONTAINER", ON_ERROR_CONTAINER),
            ("INVERSE_SURFACE", INVERSE_SURFACE),
            ("INVERSE_ON_SURFACE", INVERSE_ON_SURFACE),
            ("INVERSE_PRIMARY", INVERSE_PRIMARY),
            ("lane::FLAME", super::lane::FLAME),
            ("lane::ON_FLAME", super::lane::ON_FLAME),
        ];
        for (name, hex) in table {
            assert_eq!(hex.len(), 7, "{name} 必须是 #rrggbb：读到 {hex}");
            assert!(hex.starts_with('#'), "{name} 得从 # 起头：{hex}");
            for b in hex.as_bytes()[1..].iter() {
                assert!(b.is_ascii_digit() || (*b >= b'a' && *b <= b'f'), "{name} 含非小写十六进制位：{hex}");
            }
            assert_eq!(rgb(hex), rgb(hex), "{name} 解析不可逆（同串两次读出不同像素＝后端分叉）");
        }
    }

    /// 配对色必须读得清：每一对 `on_X／X`（含所有容器档与 surface 家族）都按 WCAG AA 正文档 4.5:1 校。
    /// 这一条是本模块"自定不等于随手"的全部兑现处——数是我写的，成不成由公式判，不由我判。
    #[test]
    fn every_foreground_token_clears_wcag_aa_on_its_background() {
        let pairs: Vec<(&str, &str, &str)> = vec![
            ("ON_SURFACE／SURFACE", ON_SURFACE, SURFACE),
            ("ON_SURFACE／SURFACE_DIM", ON_SURFACE, SURFACE_DIM),
            ("ON_SURFACE／LOWEST", ON_SURFACE, SURFACE_CONTAINER_LOWEST),
            ("ON_SURFACE／LOW", ON_SURFACE, SURFACE_CONTAINER_LOW),
            ("ON_SURFACE／CONTAINER", ON_SURFACE, SURFACE_CONTAINER),
            ("ON_SURFACE／HIGH", ON_SURFACE, SURFACE_CONTAINER_HIGH),
            ("ON_SURFACE／HIGHEST", ON_SURFACE, SURFACE_CONTAINER_HIGHEST),
            ("ON_SURFACE／BRIGHT", ON_SURFACE, SURFACE_BRIGHT),
            ("ON_SURFACE_VARIANT／SURFACE", ON_SURFACE_VARIANT, SURFACE),
            ("ON_SURFACE_VARIANT／CONTAINER", ON_SURFACE_VARIANT, SURFACE_CONTAINER),
            ("ON_SURFACE_VARIANT／VARIANT", ON_SURFACE_VARIANT, SURFACE_VARIANT),
            ("ON_PRIMARY／PRIMARY", ON_PRIMARY, PRIMARY),
            ("ON_PRIMARY_CONTAINER／PRIMARY_CONTAINER", ON_PRIMARY_CONTAINER, PRIMARY_CONTAINER),
            ("ON_SECONDARY／SECONDARY", ON_SECONDARY, SECONDARY),
            ("ON_SECONDARY_CONTAINER／SECONDARY_CONTAINER", ON_SECONDARY_CONTAINER, SECONDARY_CONTAINER),
            ("ON_TERTIARY／TERTIARY", ON_TERTIARY, TERTIARY),
            ("ON_TERTIARY_CONTAINER／TERTIARY_CONTAINER", ON_TERTIARY_CONTAINER, TERTIARY_CONTAINER),
            ("ON_ERROR／ERROR", ON_ERROR, ERROR),
            ("ON_ERROR_CONTAINER／ERROR_CONTAINER", ON_ERROR_CONTAINER, ERROR_CONTAINER),
            ("INVERSE_ON_SURFACE／INVERSE_SURFACE", INVERSE_ON_SURFACE, INVERSE_SURFACE),
            ("lane::ON_FLAME／FLAME", super::lane::ON_FLAME, super::lane::FLAME),
            ("lane::ON_MIDLINE／MIDLINE", super::lane::ON_MIDLINE, super::lane::MIDLINE),
        ];
        let mut thin: Vec<String> = Vec::new();
        for (name, fg, bg) in pairs {
            let c = contrast(fg, bg);
            if c < 4.5 {
                thin.push(format!("{name} = {c:.2}"));
            }
        }
        assert!(thin.is_empty(), "以下配对低于 AA 正文档 4.5:1：{}", thin.join("；"));
    }

    /// 描边要能在它落的面上被看见；描边不是内容，所以校 3:1（WCAG 非文本对比档）而不是 4.5。
    #[test]
    fn outlines_are_perceptible_on_their_own_surfaces() {
        for bg in [SURFACE, SURFACE_CONTAINER, SURFACE_CONTAINER_HIGHEST] {
            let c = contrast(OUTLINE, bg);
            assert!(c >= 3.0, "OUTLINE on {bg} 只有 {c:.2}，描边会融进底色（非文本对比需 ≥3.0）");
        }
    }

    /// 亮度公式自己得先对：黑 0、白 1、中灰落进预期区间、同色 1、黑白 21、与参数顺序无关。
    /// 上面几条全靠它背书，它坏了就是**尺坏了**——所以单独钉这支尺，别让一把坏尺给整张色表发合格证。
    #[test]
    fn the_luminance_ruler_is_calibrated_before_it_judges_the_table() {
        assert_eq!(luminance(rgb("#000000")), 0.0, "纯黑亮度必须是 0");
        assert_eq!(luminance(rgb("#ffffff")), 1.0, "纯白亮度必须是 1");
        let mid = luminance(rgb("#808080"));
        assert!((0.20..0.23).contains(&mid), "中灰 #808080 亮度漂移：{mid}（WCAG 线性化后中灰本就落在 0.21 上下，别按直觉当 0.5）");
        assert_eq!(contrast("#000000", "#000000"), 1.0, "同色对比度必须是 1（不是 0）");
        assert_eq!(contrast("#000000", "#ffffff"), 21.0, "黑白对比必须封顶在 21");
        assert_eq!(contrast("#ffffff", "#000000"), contrast("#000000", "#ffffff"), "对比度不许随参数顺序变（那道 min／max 被拆了就会红在这儿）");
    }

    /// elevation 必须是单调的亮度阶梯：MD3 暗色方案靠色调抬升表达高度，一旦某档比它下面那档更暗，
    /// "这块面板被抬起来了"那条信息就反了——而反了的时候肉眼最难看出来。
    #[test]
    fn elevation_tiers_climb_monotonically() {
        let mut prev = luminance(rgb(surface_at_elevation(0)));
        for lvl in 1..=5u8 {
            let here = luminance(rgb(surface_at_elevation(lvl)));
            assert!(here > prev, "elevation {lvl} 没比上一档更亮：明度阶梯断了");
            prev = here;
        }
        assert_eq!(surface_at_elevation(0), SURFACE, "0 档就是基准面，不该另造一色");
        assert_eq!(surface_at_elevation(255), SURFACE_BRIGHT, "越界档要夹到最上，不许 panic 也不许回绕到 0 档");
    }

    /// 状态层五档的秩序与量纲：都在 0–1 开区间内、名字序即大小序且严格递增、聚焦与按下同值。
    /// 这几个数是照规范记的——随手改了它，#132 的交互态就与全行业的 MD3 实现分叉。
    #[test]
    fn state_layers_are_ordered_and_in_range() {
        // PRESS 不在这条梯上——它与 FOCUS 同值，单列一条 `assert_eq` 钉住（同值不是序）。
        let ladder = [("HOVER", HOVER), ("FOCUS", FOCUS), ("DISABLED_CONTAINER", DISABLED_CONTAINER), ("DRAG", DRAG), ("DISABLED_CONTENT", DISABLED_CONTENT)];
        for (name, a) in ladder {
            assert!(a > 0.0 && a < 1.0, "{name} = {a} 不在 0–1 透明度开区间内（1.0 是完全盖住，不是状态层）");
        }
        for w in ladder.windows(2) {
            assert!(w[0].1 < w[1].1, "状态层名字序不等于大小序：{}={:?} 不比 {}={:?} 淡（拖拽须重于聚焦、禁用容器须退在拖拽之后、禁用内容须压过拖拽）", w[0].0, w[0].1, w[1].0, w[1].1);
        }
        assert_eq!(FOCUS, PRESS, "MD3 里 focus 与 pressed 同为 10%：改一个就得改另一个，那是两处真值");
    }

    /// 混合函数：叠 0% 逐通道等于底色、叠 100% 等于叠加色、中间单调不减。
    /// 这条钉的是"它真的按透明度混色"，不是"看起来像混色"——桌面走 fill-opacity、安卓走这支函数，
    /// 两后端同色这一口径全靠它。
    #[test]
    fn the_state_layer_blend_is_linear_and_bounded() {
        let base = rgb(SURFACE_CONTAINER);
        let over = rgb(ON_SURFACE);
        assert_eq!(blend(base, over, 0.0), base, "0% 不许动一个通道");
        assert_eq!(blend(base, over, 1.0), over, "100% 要完全换成叠加色");
        let mut prev = base;
        let mut step = 1;
        while step <= 10 {
            let a = step as f64 / 10.0;
            let now = blend(base, over, a);
            for i in 0..3 {
                assert!(now[i] >= prev[i], "混合在通道 {i} 上非单调（a={a}：{prev:?} → {now:?}）");
            }
            prev = now;
            step += 1;
        }
        assert_ne!(blend(base, over, HOVER), base, "悬停不留痕＝交互态白做");
        assert_ne!(blend(base, over, DRAG), blend(base, over, HOVER), "拖拽与悬停同色＝两态合一态");
    }

    /// 间距与组件度量都落在 4dp 栅格上。这条把"组件度量自定"关进笼子：自定的数也得被栅格整除，
    /// 于是任意两格的留白差异永远是栅格整数倍，不会长出 13 与 16 那种肉眼说不清的缝。
    #[test]
    fn spacing_and_component_metrics_sit_on_the_four_dp_grid() {
        assert_eq!(GRID, 4, "基数不是 4 就不是 MD3 的栅格");
        let steps = [("XXS", XXS), ("XS", XS), ("S", S), ("M", M), ("L", L), ("XL", XL), ("XXL", XXL), ("XXXL", XXXL), ("HUGE", HUGE)];
        for (name, v) in steps {
            assert_eq!(v % GRID, 0, "{name} = {v} 脱离 {GRID} 栅格");
        }
        for w in steps.windows(2) {
            assert!(w[0].1 < w[1].1, "间距档名序必须等于大小序：{}={} 不小于 {}={}", w[0].0, w[0].1, w[1].0, w[1].1);
        }
        for (name, v) in [
            ("BOARD_W", BOARD_W),
            ("MARGIN", MARGIN),
            ("COL_GAP", COL_GAP),
            ("CELL_H", CELL_H),
            ("HAND_H", HAND_H),
            ("BAR_W", BAR_W),
            ("BAR_H", BAR_H),
            ("HOLDER_STRIDE", HOLDER_STRIDE),
            ("MIDLINE_BAND_H", MIDLINE_BAND_H),
            ("FLAME_MARK", FLAME_MARK),
            ("LOG_H", LOG_H),
            ("CELL_PAD", CELL_PAD),
            ("TITLE_BASELINE", TITLE_BASELINE),
            ("HEADER_DROP", HEADER_DROP),
            ("ROW_GAP_BACK", ROW_GAP_BACK),
            ("FOOT_GAP", FOOT_GAP),
        ] {
            assert!(v > 0, "{name} = {v} 不是正数");
            assert_eq!(v % GRID, 0, "组件度量 {name} = {v} 脱离 {GRID} 栅格（要改栅格，别改这一格）");
        }
        // 交叉度量（谁必须容得下谁）走同一形状：装进数组再比，断言读的才是"表里的大小关系"。
        for (small, big, note) in [
            (HAND_H, CELL_H, "手牌格该比盘面格矮"),
            (BAR_W, HOLDER_STRIDE, "柱步长必须容得下一整根条，否则双柱叠字"),
            (MARGIN, BOARD_W / 2, "外边距吃掉整个画布就没地方摆盘面了"),
        ] {
            assert!(small < big, "{note}：{small} 越过了 {big}");
        }
        assert_eq!(LOG_H, LOG_PAD * 2 + LOG_LINE_STEP * LOG_LINES as i32, "日志面板高没闭合到 2×内边距 + 行数×行距：{LOG_H}");
        for ladder in [
            [("CELL_ROW1", CELL_ROW1), ("CELL_ROW2", CELL_ROW2), ("CELL_ROW3", CELL_ROW3), ("CELL_H", CELL_H)],
            [("HAND_ROW1", HAND_ROW1), ("HAND_ROW2", HAND_ROW2), ("HAND_ROW3", HAND_ROW3), ("HAND_H", HAND_H)],
        ] {
            for w in ladder.windows(2) {
                assert!(w[0].1 < w[1].1, "行基线不同序或越出格子：{}={} ≥ {}={}", w[0].0, w[0].1, w[1].0, w[1].1);
            }
        }
    }

    /// 形状阶梯单调、起点为 0，且默认圆角是 8dp。圆角只有这七档可取；
    /// 钉死默认角是防有人把某一块悄悄换成 6——那正是首版那张图上两种圆角撞出来的失配。
    #[test]
    fn shape_ladder_is_monotonic_and_starts_at_zero() {
        assert_eq!(NONE, 0, "直角档必须是 0，不是 1");
        let ladder = [("NONE", NONE), ("EXTRA_SMALL", EXTRA_SMALL), ("SMALL", SMALL), ("MEDIUM", MEDIUM), ("LARGE", LARGE), ("EXTRA_LARGE", EXTRA_LARGE), ("FULL", FULL)];
        for w in ladder.windows(2) {
            assert!(w[0].1 < w[1].1, "圆角阶梯非单调：{}={} ≥ {}={}（名字顺序必须等于大小顺序；FULL 是胶囊档，须比一切具名半径更圆）", w[0].0, w[0].1, w[1].0, w[1].1);
        }
        assert_eq!(SMALL, 8, "本帧默认圆角 8dp：改它等于重排所有格子");
        assert_eq!(EXTRA_SMALL, 4, "小填充块（血条）圆角 4dp");
        for r in [CELL_H, HAND_H, BAR_H] {
            assert!(r > SMALL, "组件尺寸得容得下默认圆角，否则圆角吃掉整块：{r}");
        }
    }

    /// 字号阶梯单调、MD3 具名档的读数不许被我改动，推进宽度两档量纲正确。
    /// 18／26 这类不在阶梯上的字号是首版真实存在过的：这条红就是在拦"又写了一个没名字的字号"。
    #[test]
    fn the_type_scale_is_named_monotonic_and_matches_the_md3_ladder() {
        let ladder = [LABEL_SMALL, LABEL_MEDIUM, BODY_SMALL, BODY_MEDIUM, LABEL_LARGE, BODY_LARGE, TITLE_LARGE, HEADLINE_SMALL, HEADLINE_MEDIUM, HEADLINE_LARGE, DISPLAY_SMALL, DISPLAY_MEDIUM, DISPLAY_LARGE];
        for w in ladder.windows(2) {
            assert!(w[0] <= w[1], "字号阶梯非单调：{:?}", w);
        }
        assert_eq!([BODY_SMALL, BODY_MEDIUM, BODY_LARGE], [12, 14, 16], "MD3 具名档读数被动过：这一族不该自定");
        assert_eq!([TITLE_LARGE, HEADLINE_SMALL, HEADLINE_MEDIUM, HEADLINE_LARGE], [22, 24, 28, 32], "MD3 具名档读数被动过：这一族不该自定");
        assert_eq!(LABEL_LARGE, BODY_MEDIUM, "MD3 里 label-large 与 body-medium 同为 14：同值是规范不是巧合");
        assert_eq!(ADVANCE_CJK, 1.0, "CJK 占满一格，改这条等于改整个折行口径");
        assert!((0.2..1.0).contains(&ADVANCE_ASCII), "ASCII 推进宽度 {ADVANCE_ASCII} 量纲漂移");
        let widths = [("ADVANCE_ASCII", ADVANCE_ASCII), ("ADVANCE_CJK", ADVANCE_CJK)];
        for w in widths.windows(2) {
            assert!(w[0].1 < w[1].1, "{} 必须比 {} 窄，否则折行按字符数那老毛病复发：{:?} ≥ {:?}", w[0].0, w[1].0, w[0].1, w[1].1);
        }
        assert_eq!(ELLIPSIS_COST, ADVANCE_CJK, "省略号按一格宽预留：{ELLIPSIS_COST}");
        assert!(FONT_FAMILY.contains("Noto"), "字体回退串里连 Noto 都没了：{FONT_FAMILY}");
    }

    /// dp↔px 换算与三档断点：读数按**真机实测**钉——720×1548 @ density 320。
    /// 这条是安卓那条路的门槛：把 1548 当 1600 排版，断点判对而版式错，画布底边会被系统栏吃掉一截。
    #[test]
    fn the_density_math_matches_the_measured_phone() {
        assert_eq!(dp_to_px(360, 320), 720, "真机 720px 宽对应 360dp（实测 density 320）");
        assert_eq!(px_to_dp(1548, 320), 774, "窗口高 1548px ⇒ 774dp（不是 800dp：1600 里那 52px 是系统栏）");
        assert_eq!(dp_to_px(0, 320), 0, "0dp 必须 0px");
        assert_eq!(dp_to_px(MARGIN, 160), MARGIN, "density 160 时 1dp=1px：桌面口径的退化情形");
        assert_eq!(classify(720, 320), Bucket::Compact, "真机竖屏判 compact（宽 360dp）");
        assert_eq!(classify(600, 0), Bucket::Compact, "无 density 时按 1dp=1px，600 落 compact 上界");
        assert_eq!(classify(601, 0), Bucket::Medium, "601 起进 medium");
        assert_eq!(classify(840, 0), Bucket::Medium, "840 是 medium 上界");
        assert_eq!(classify(BOARD_W, 0), Bucket::Expanded, "桌面帧 1240 落 expanded");
        assert_eq!(classify(dp_to_px(COMPACT_MAX_DP, 320), 320), Bucket::Compact, "换算与归档要闭合：绕一圈回来同一档");
        assert_eq!(classify(dp_to_px(MEDIUM_MAX_DP + 1, 320), 320), Bucket::Expanded, "medium 上界之后一格就该跳 expanded");
        let tiers = [("COMPACT_MAX_DP", COMPACT_MAX_DP), ("MEDIUM_MAX_DP", MEDIUM_MAX_DP)];
        for w in tiers.windows(2) {
            assert!(w[0].1 < w[1].1, "断点顺序反了：{}={} ≥ {}={}", w[0].0, w[0].1, w[1].0, w[1].1);
        }
    }

    /// 线格式那两串得原样能用：XML 声明与命名空间 URI 是规定字面量，动了就是产出不可解析的文档。
    #[test]
    fn the_svg_wire_literals_are_intact() {
        assert!(super::svg::PROLOG.starts_with("<?xml version="), "XML 声明形状变了：{}", super::svg::PROLOG);
        assert!(super::svg::PROLOG.contains("UTF-8"), "编码声明丢了：产出在别的 locale 上就是另一份字节");
        assert_eq!(super::svg::NS, "http://www.w3.org/2000/svg", "命名空间 URI 是规定串，不是可自定的数");
    }

    /// **真值来源的机检**（#130 决策日志立的那颗牙）：视觉常数只许住在 md3。
    /// 扫 `src/canvas.rs` 的生产面，三条同时成立才算过：
    /// ① 没有 `#rrggbb`；② 没有 `const` 声明；③ 数字位只许是 `0`／`1`／`2` 这三个结构数（除以二、下标起算一类）。
    /// 第③条把色值、圆角、字号、行距、段长、透明度全关进表里——连写在 SVG 属性串里的裸数也一并拦。
    /// 两类豁免，都是**写明的**而不是"数不到"：标识符里的数字不算数（`i32`／`f64`／`u8` 这类类型名，
    /// 以及牌面文案里的 `P1`～`P4`），判据是"紧跟在字母或下划线之后"；单个 `0`／`1`／`2` 与形如 `x.0`
    /// 的小数溜得过去（`stroke-width="2"` 那一格就是），所以这条拦的是"漏进一个没名字的数"，
    /// 拦不住"把表里已有的数抄成字面量"——后者归引用纪律与本档目视。
    /// 末了那条反空虚检查拿"md3 自己确实有色"作对照：否则 `canvas.rs` 被删空也能骗过前三条。
    #[test]
    fn visual_constants_have_one_home() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let src = std::fs::read_to_string(manifest.join("src/canvas.rs")).expect("没有 src/canvas.rs：这条检扫谁？");

        let mut offenders: Vec<String> = Vec::new();
        for (no, raw) in src.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            if line.starts_with("#[cfg(test)]") {
                break;
            }
            let bytes = line.as_bytes();
            let ident = |b: u8| b.is_ascii_alphabetic() || b == b'_';
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'#' && bytes.len() >= i + 7 && bytes[i + 1..i + 7].iter().all(|b| b.is_ascii_hexdigit()) {
                    offenders.push(format!("{:>3}: 色值 {}", no + 1, line));
                }
                if bytes[i].is_ascii_digit() {
                    let mut j = i;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    let in_identifier = i > 0 && ident(bytes[i - 1]);
                    let structural = j - i == 1 && matches!(bytes[i], b'0' | b'1' | b'2');
                    if !in_identifier && !structural {
                        offenders.push(format!("{:>3}: 裸数 {} ← {}", no + 1, &line[i..j], line));
                    }
                    i = j;
                    continue;
                }
                i += 1;
            }
            if line.starts_with("const ") || line.starts_with("pub const ") {
                offenders.push(format!("{:>3}: 本地常数 {}", no + 1, line));
            }
        }
        assert!(offenders.is_empty(), "视觉常数漏出 md3 之外（版式真值只许一个家）：\n{}", offenders.join("\n"));

        let mine = std::fs::read_to_string(manifest.join("src/md3.rs")).expect("本模块读不到自己？");
        let colors = mine.lines().filter(|l| l.contains("pub const") && l.contains("\"#") && !l.trim().starts_with("//")).count();
        assert!(colors >= 20, "md3 里的色值常数只有 {colors} 条：这张表还是空的，上一条检因此是永真");
    }
}
