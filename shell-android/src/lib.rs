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

// 引擎那批模块里，本帧真被壳调用的只有 `display`／`battle`／`meta`／`model` 那条呈现链，
// 其余项还没人调，`dead_code` 会淹掉输出。这是"壳尚未接上全部面"的真实状态，不是需要修的
// 缺陷，所以按整体豁免处理而非逐条 `#[allow]`（哪天真要收窄，应该收窄的是调用而不是豁免）。
#![allow(dead_code)]

// 引擎那 17 个模块的 `#[path]` 表**不在这张文件里**，在 `engine-mods.rs`——因为 host 侧的
// 对撞工具 `probes/host-frame` 要 `include!` 同一张表。抄一份表，就等于允许壳与探针各引各的
// 模块集，那种漂移的症状是"对撞绿而真机红"，比没尺更糟。
include!("engine-mods.rs");

mod entry;
mod frame;
mod input;
mod text;
