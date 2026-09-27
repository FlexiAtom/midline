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
//! 《中线》核心逻辑 CLI。
//! 用法：
//!   cargo run -- play [--seed N] [--faction 1|2|3] [--difficulty easy|normal|hard|expert]
//!   cargo run -- daily            （每日挑战：以日期为种子）
//!   cargo run -- auto [N] [--difficulty X]   （AI 对 AI 快速模拟 N 局，冒烟验证；默认 normal）
//! 战斗内命令：
//!   p <手牌idx> <P1-P4>   放置    s <P1-P4> | sh <手牌idx>   献祭
//!   di | ds               抽继承堆/开端堆                     e  结束回合
//!   l                   看日志   r  重绘   q  退出
//! 结算/准备阶段：fuse <主牌idx> <副牌idx> / up <idx> power|thr / drop <idx> / go

mod ai;
mod battle;
mod meta;
mod model;
mod render;
mod rng;

use battle::Difficulty;
use model::Faction;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("play");
    match cmd {
        "auto" => {
            let n: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
            let diff = parse_difficulty(&args);
            meta::auto_battles(n, diff);
        }
        "daily" => {
            let seed = meta::daily_seed();
            println!("每日挑战 seed={seed}");
            meta::interactive_run(seed);
        }
        _ => {
            let mut seed: u64 = 7;
            let mut faction = Faction::Ember;
            let diff = parse_difficulty(&args);
            let mut it = args.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--seed" => seed = it.next().and_then(|s| s.parse().ok()).unwrap_or(7),
                    "--faction" => {
                        faction = match it.next().map(|s| s.as_str()) {
                            Some("2") => Faction::Frost,
                            Some("3") => Faction::Shadow,
                            _ => Faction::Ember,
                        }
                    }
                    _ => {}
                }
            }
            meta::interactive_run_with(seed, faction, diff);
        }
    }
}

/// `--difficulty easy|normal|hard|expert`；未给 → 普通；未知值 → 告警后回退普通（不再静默）。
fn parse_difficulty(args: &[String]) -> Difficulty {
    let Some(i) = args.iter().position(|a| a == "--difficulty") else {
        return Difficulty::Normal;
    };
    let raw = args.get(i + 1).map(|s| s.as_str()).unwrap_or("");
    match Difficulty::parse(raw) {
        Some(d) => d,
        None => {
            println!("✖ 未知难度：{raw}（可选 easy|normal|hard|expert），已回退普通");
            Difficulty::Normal
        }
    }
}
