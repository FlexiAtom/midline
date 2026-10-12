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
//! host 侧对撞工具：跑**壳那份** `frame.rs`，出一张 PPM 与一份读数。
//!
//! 它存在的唯一理由是 §2 那条假设：同一份源码、同一桌牌，host 与真机应当逐像素相同。
//! 所以这里不许出现任何"再算一遍"的式子——换算、圆角、混色全走 `frame::paint`，
//! 本文件只做三件事：喂尺寸、要断言、写文件。
//!
//! ## 为什么 stride 在这里可以比宽度大
//! 真机现读 `buffer 1548×720 / stride=1552`，那 4 列是**步长填充**、不属于画面。
//! 这里按同一个口径走（默认 `stride = 宽度`，可传大），并当场验一条：填充列必须一格未动。
//! 这条是"stride 不影响版面"的 host 侧证人——它不需要真机就能红。

// 引擎那张表里大半的项本工具用不到（它只吃呈现链），`dead_code` 会淹掉读数。
// 豁免跟着壳走（`lib.rs` 里同一条），不是把警告按下去不看：这里没人打算调用那些项。
#![allow(dead_code)]

// 与壳共享的那张引擎模块表（`#[path]` 按**声明所在文件**的目录解析，所以两边都落在主仓 src/）。
include!("../../../shell-android/src/engine-mods.rs");
#[path = "../../../shell-android/src/frame.rs"]
mod frame;
#[path = "../../../shell-android/src/text.rs"]
mod text;

use std::io::Write;

/// 缓冲里那一格"没人该碰"的哨兵值。取一个画面上不会自然出现的字节，好让失败一眼可辨。
const SENTINEL: u8 = 0xA5;

fn main() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut argv = std::env::args().skip(1);
    // 尺寸默认取真机那块横屏缓冲（§3.8.1 现读），对撞时两侧同尺寸才不用二次缩放。
    let spec = argv.next().unwrap_or_else(|| "1548x720".to_string());
    let (w, h) = match spec.split_once('x') {
        Some((a, b)) => (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()),
        None => {
            eprintln!("host-frame: 尺寸要写成 WxH（读到 {spec}）");
            std::process::exit(2);
        }
    };
    // 第二格是**步长**，不是第二块的宽度：留它能在这里就验掉"填充列没被碰"那一格。
    let stride = argv.next().map(|s| s.parse::<usize>().unwrap()).unwrap_or(w);
    let out = argv.next().unwrap_or_else(|| "build/host-frame.ppm".to_string());
    let font = repo.join(argv.next().unwrap_or_else(|| "build/assets/fonts/ui.otf".to_string()));

    let bytes = match std::fs::read(&font) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "host-frame: 读不到字体 {font:?}（{e}）。先跑 `bash scripts/android-font.sh` 生成子集。"
            );
            std::process::exit(1);
        }
    };
    let face = text::Face::try_from_vec(bytes).expect("host-frame: 字体字节读不出 Face");

    // 同一张指令表：真机走 `frame::demo_frame()`，这里走同一个函数、同一桌牌、同一摘要算法。
    let board = frame::demo_frame();
    let digest = frame::frame_digest(&board);

    let mut dst = vec![SENTINEL; stride * h * 4];
    text::fill_bg(&mut dst, w, h, stride);
    let mut atlas = text::Atlas::new();
    let p = frame::paint(&board, &face, &mut atlas, &mut dst, w, h, stride);

    // 断言而不是"打印出来给人看"：这四条任何一条破，这张 PPM 就没有对撞资格，退出码非 0。
    // 缺字（blank）是 #134 那格子集的事，空块（empty）是几何或版式的事，两者在屏上都是"少一块"，
    // 都得在这里当场分清，别让它们到截图上再去猜。
    let mut bad: Vec<String> = Vec::new();
    if p.labels == 0 || p.blocks == 0 {
        bad.push(format!("指令表空跑：blocks={} labels={}", p.blocks, p.labels));
    }
    if p.blank_labels != 0 {
        bad.push(format!("{} 行字一个字形都没落成（子集缺字？）", p.blank_labels));
    }
    if p.empty_blocks != 0 {
        bad.push(format!("{} 块矩形没有落到一个像素（越界或零尺寸？）", p.empty_blocks));
    }
    if stride > w {
        // 逐格数，不借迭代器链：这一格红了要读的是"哪一列被动过"，绕一圈 map/filter 只是把
        // 那句判断藏起来。
        let mut touched = 0usize;
        let mut first = 0usize;
        for y in 0..h {
            for x in w..stride {
                let o = (y * stride + x) * 4;
                if dst[o..o + 4].iter().any(|b| *b != SENTINEL) {
                    if touched == 0 {
                        first = y * stride + x;
                    }
                    touched += 1;
                }
            }
        }
        if touched != 0 {
            bad.push(format!("{} 格步长填充被动过（第一格索引 {}）", touched, first));
        }
    }

    let mut ppm = Vec::with_capacity(w * h * 3 + 32);
    write!(ppm, "P6\n{w} {h}\n255\n").unwrap();
    for y in 0..h {
        for x in 0..w {
            let o = (y * stride + x) * 4;
            ppm.extend_from_slice(&dst[o..o + 3]);
        }
    }
    let path = if std::path::Path::new(&out).is_absolute() {
        std::path::PathBuf::from(&out)
    } else {
        repo.join(&out)
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(&path, &ppm).expect("host-frame: PPM 写不出去");

    let (adv_cjk, adv_latin) = frame::advance_probe(&face);
    println!(
        "host-frame  {}x{} stride={}  digest={digest:016x}\n  k={:.6} off={}x{}  prims={}+{}  blank={}  empty={}  rasters={}  atlas={}B\n  advance 全角={adv_cjk:.4} 半角={adv_latin:.4}（引擎表 {}/{}）\n  ppm={}  {r} 字节",
        w,
        h,
        stride,
        p.scale,
        p.off_x,
        p.off_y,
        p.blocks,
        p.labels,
        p.blank_labels,
        p.empty_blocks,
        p.rasters,
        atlas.bytes(),
        crate::md3::typo::ADVANCE_CJK,
        crate::md3::typo::ADVANCE_ASCII,
        path.display(),
        r = ppm.len(),
    );
    for b in &bad {
        eprintln!("host-frame: [红] {b}");
    }
    if !bad.is_empty() {
        std::process::exit(1);
    }
}
