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
//! 安卓壳的 crate 根。**源码级融合**：下面那张 `#[path]` 表把主仓 `src/*.rs` 的原文件逐模块
//! 直接编进本 crate，与入口胶水同属一个编译单元——所以本 crate 里没有引擎的一份副本，
//! 也就不存在"主仓动了、壳里那份还是旧的"这种失真（上一档那 14 份副本欠的同步债在此结构性消失）。
//!
//! 为什么不用 `use midline::...`：主仓是 bin crate，没有 lib 根可 `use`；而给它加
//! `[lib]` 会当场红掉主仓那条机检的禁令①②，那是**人给的**防绕过立场，我不能自决拆除。
//! `#[path]` 是把两边缝在同一个编译单元里，不是把引擎拆成可复用库——方向正相反。
//!
//! 导出面纪律：本 crate 对外只应露出**平台强制**的入口符号（`ANativeActivity_onCreate`）。
//! 守它的是 `scripts/shell-export-surface.sh`，不是主仓那条检——主仓绿不证明壳没拓宽。

// 壳现在只挂了入口回调，引擎那批模块的大部分项还没被调用，`dead_code` 会淹掉输出。
// 这是"壳尚未接上呈现面"的真实状态，不是需要修的缺陷，所以按整体豁免处理而非逐条 `#[allow]`。
#![allow(dead_code)]

#[path = "../../src/ai.rs"]
mod ai;
#[path = "../../src/battle.rs"]
mod battle;
#[path = "../../src/boss.rs"]
mod boss;
#[path = "../../src/canvas.rs"]
mod canvas;
#[path = "../../src/command.rs"]
mod command;
#[path = "../../src/md3.rs"]
mod md3;
#[path = "../../src/meta.rs"]
mod meta;
#[path = "../../src/model.rs"]
mod model;
#[path = "../../src/progress.rs"]
mod progress;
#[path = "../../src/render.rs"]
mod render;
#[path = "../../src/rng.rs"]
mod rng;
#[path = "../../src/save.rs"]
mod save;
#[path = "../../src/session.rs"]
mod session;
#[path = "../../src/touch.rs"]
mod touch;
#[path = "../../src/tui.rs"]
mod tui;
#[path = "../../src/view.rs"]
mod view;

mod entry;
