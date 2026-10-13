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
#[path = "../../../shell-android/src/input.rs"]
mod input;
#[path = "../../../shell-android/src/text.rs"]
mod text;

use std::io::Write;

/// 缓冲里那一格"没人该碰"的哨兵值。取一个画面上不会自然出现的字节，好让失败一眼可辨。
const SENTINEL: u8 = 0xA5;

/// 打一帧 → 四条断言 → PPM 字节。**两条路（静态证人／点按复放）共用这一支**：
/// 差别只许在"帧从哪来"，换算、栅格、判红的那些式子若各写一遍，对撞就读不出对错分了。
/// 返回（现读量, 红项, PPM 字节）——红项在这里只**收集**不退出，因为 tap 模式要先打完两帧
/// 再一起报，中途 `exit` 会把 before 那张 PPM 留在盘上却没有 after，看着像"跑了一半"。
fn render(
    board: &display::Frame,
    face: &text::Face,
    atlas: &mut text::Atlas,
    w: usize,
    h: usize,
    stride: usize,
) -> (frame::Painted, Vec<String>, Vec<u8>) {
    let mut dst = vec![SENTINEL; stride * h * 4];
    text::fill_bg(&mut dst, w, h, stride);
    let p = frame::paint(board, face, atlas, &mut dst, w, h, stride);

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
    (p, bad, ppm)
}

/// 开局那一桌（一回合都没走）——与 `input.rs` 测里那支 `starter_table()` 同一段构造。
/// 为什么这里也要一份：**只有这一桌存在 0 费的「开端」**，"点两下画面真变了"那句在装机帧那桌
/// 永远只能得到"业力不足"。它在 host 侧可证，所以不必等真机。
fn starter_table() -> input::Live {
    let b = battle::Battle::new_mainline(
        frame::DEMO_SEED,
        model::Faction::Ember,
        meta::enemy_faction_for(1),
        battle::Difficulty::Normal,
        Vec::new(),
        1,
    );
    input::Live::from_battle(b)
}

fn main() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // 选项先摘出来，位置参数（尺寸／步长／输出／字体）保持原序不变——那四格已有脚本在喂，
    // 改了位置就等于改了它们的读法。
    let mut pos: Vec<String> = Vec::new();
    let mut taps: Vec<(i32, i32)> = Vec::new();
    let mut start = false;
    let mut raw = std::env::args().skip(1);
    while let Some(a) = raw.next() {
        match a.as_str() {
            "--tap" => {
                let Some(v) = raw.next() else {
                    eprintln!("host-frame: --tap 要带 x,y（设备像素，与真机 AMotionEvent 同一档）");
                    std::process::exit(2);
                };
                match v.split_once(',') {
                    Some((x, y)) => match (x.parse::<i32>(), y.parse::<i32>()) {
                        (Ok(x), Ok(y)) => taps.push((x, y)),
                        _ => {
                            eprintln!("host-frame: --tap 的两个数要写成整数（读到 {v}）");
                            std::process::exit(2);
                        }
                    },
                    None => {
                        eprintln!("host-frame: --tap 要写成 x,y（读到 {v}）");
                        std::process::exit(2);
                    }
                }
            }
            "--start" => start = true,
            other => pos.push(other.to_string()),
        }
    }
    let mut argv = pos.into_iter();
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
    let mut atlas = text::Atlas::new();

    // 同一张指令表：真机走 `frame::demo_frame()`，这里走同一个函数、同一桌牌、同一摘要算法。
    // 带 --tap／--start 时改走 `input::Live`——那是真机上被点的那一份状态，判定源码两侧同源。
    let mut shots: Vec<(String, display::Frame)>;
    let lines: Vec<String>;
    if taps.is_empty() && !start {
        shots = Vec::new();
        lines = Vec::new();
    } else {
        let mut live = if start { starter_table() } else { input::Live::new() };
        // 尺与帧同批：拿**这一帧**算出的 Geom 去命中，与真机 `paint_board` 里那一次调用一字不差。
        let geom = input::Geom::of(w, h, &live.frame());
        live.set_geom(geom);
        // 先把**设备坐标下**的可点区打出来。这一格的意义在"坐标不是这里抄的"：区表来自
        // `touch::Layout`，换算来自 `Geom::pairs`——与真机命中用的那两支出自同一份源码，
        // 所以这一串数与 `--tap` 喂进来的数是同一把尺量出来的。
        for (r, rect) in geom.pairs(&live.layout()) {
            if r.is_control() {
                println!("  区 {r:?} 设备矩形 x={} y={} w={} h={}", rect.x, rect.y, rect.w, rect.h);
            }
        }
        // **每个事件后各存一张帧**，不是只存首尾。真机每收到一个 AMotionEvent 就重绘一次：
        // 按下那一下亮的是"选中层"（还没走引擎判定），抬起才喂命令。所以 logcat 里的 digest 是
        // **一串**——探针只出两端的话，中间那一层就没有 host 侧证人，而对撞要的正是"两侧同一串数"。
        shots = vec![("pre".to_string(), live.frame())];
        let mut ls: Vec<String> = Vec::new();
        for (x, y) in &taps {
            for act in [input::Action::Down, input::Action::Up] {
                let t = live.on_touch(act, *x, *y);
                let tag = match &t.region {
                    Some(r) => format!("{}@{x},{y}→{r:?}", act.name()),
                    None => format!("{}@{x},{y}→none", act.name()),
                };
                ls.push(t.log_line());
                shots.push((tag, live.frame()));
            }
        }
        lines = ls;
    }

    let mut bad: Vec<String>;
    let mut paths: Vec<(std::path::PathBuf, usize)> = Vec::new();
    let mut write_ppm = |name: &str, ppm: &[u8]| {
        let path = if std::path::Path::new(name).is_absolute() {
            std::path::PathBuf::from(name)
        } else {
            repo.join(name)
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&path, ppm).expect("host-frame: PPM 写不出去");
        paths.push((path, ppm.len()));
    };

    let p;
    let pre_digest: u64;
    if shots.is_empty() {
        let board = frame::demo_frame();
        let (pp, bb, mpp) = render(&board, &face, &mut atlas, w, h, stride);
        bad = bb;
        write_ppm(&out, &mpp);
        p = pp;
        pre_digest = frame::frame_digest(&board);
    } else {
        let stem = out.strip_suffix(".ppm").unwrap_or(&out);
        bad = Vec::new();
        let mut head: Option<frame::Painted> = None;
        let mut digests: Vec<u64> = Vec::new();
        for (k, (tag, board)) in shots.iter().enumerate() {
            let (pp, bb, ppm) = render(board, &face, &mut atlas, w, h, stride);
            for b in bb {
                bad.push(format!("帧{k}（{tag}）：{b}"));
            }
            if k == 0 {
                head = Some(pp);
            }
            let d = frame::frame_digest(board);
            digests.push(d);
            let name = format!("{stem}-{k}.ppm");
            write_ppm(&name, &ppm);
            println!("  帧{k} {tag:<28} digest={d:016x}  {name}");
        }
        p = head.expect("host-frame: shots 至少含 pre 那一帧");
        pre_digest = digests[0];
        // 第一条是"闭环"那两个字的形状：**引擎答了 Done，画面就必须动**。
        if lines.iter().any(|l| l.contains("fed=done")) && digests.first() == digests.last() {
            bad.push(format!(
                "引擎答了 Done 而末帧摘要与首帧相同（{:016x}）——状态改了而画面没跟上",
                digests[0]
            ));
        }
        // 第二条是本帧真正卖的那件事：**点在一个控件上，按下那一下画面就得有反应**。
        // 引擎拒了（业力不足）也该亮选中层——那一层是"我收到你这下"的唯一答复。
        // 点空区（region=none）不算：那里本来就没有该亮的东西。
        for (k, (tag, _)) in shots.iter().enumerate().skip(1) {
            if tag.starts_with("down@") && !tag.ends_with("→none") && digests[k] == digests[0] {
                bad.push(format!("{tag} 按下前后摘要相同（{:016x}）——控件不亮，屏上看不出点着过", digests[0]));
            }
        }
        for l in &lines {
            println!("  {l}");
        }
    }

    let (adv_cjk, adv_latin) = frame::advance_probe(&face);
    println!(
        "host-frame  {}x{} stride={}  digest={pre_digest:016x}",
        w, h, stride,
    );
    println!(
        "  k={:.6} off={}x{}  prims={}+{}  blank={}  empty={}  rasters={}  atlas={}B",
        p.scale, p.off_x, p.off_y, p.blocks, p.labels, p.blank_labels, p.empty_blocks, p.rasters, atlas.bytes()
    );
    println!(
        "  advance 全角={adv_cjk:.4} 半角={adv_latin:.4}（引擎表 {}/{}）",
        crate::md3::typo::ADVANCE_CJK,
        crate::md3::typo::ADVANCE_ASCII,
    );
    for (path, n) in &paths {
        println!("  ppm={}  {n} 字节", path.display());
    }
    for b in &bad {
        eprintln!("host-frame: [红] {b}");
    }
    if !bad.is_empty() {
        std::process::exit(1);
    }
}
