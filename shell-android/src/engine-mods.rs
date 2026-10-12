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
// 引擎模块表。**这张表被两边 `include!`**：`lib.rs`（真机那条链）与
// `probes/host-frame/src/main.rs`（host 侧对撞工具）。
//
// 为什么单独成文件而不是写在 `lib.rs` 里：host 工具要的正是"和壳一字不差的那张表"。
// 抄一份表就意味着壳与探针可能各引各的模块集，那种漂移的症状是"对撞绿而真机红"——
// 比没有尺更糟。`include!` 让两边共享同一份文本，表一动两边一起动。
//
// 为什么 `#[path]` 在这里仍然指得对：rustc 按**声明所在的那个文件**的目录解析相对路径
// （不是按 `include!` 的宿主），所以这里的 `../../src/…` 在两边都落在主仓 `src/`。
// 这条行为不是约定，是实测（`probes/host-frame` 第一次编过就是它的证人）。

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
#[path = "../../src/display.rs"]
mod display;
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
