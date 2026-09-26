//! 《中线》核心逻辑 CLI。
//! 用法：
//!   cargo run -- play [--seed N] [--faction 1|2|3] [--difficulty easy|normal]
//!   cargo run -- daily            （每日挑战：以日期为种子）
//!   cargo run -- auto [N]         （AI 对 AI 快速模拟 N 局，冒烟验证）
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
            meta::auto_battles(n);
        }
        "daily" => {
            let seed = meta::daily_seed();
            println!("每日挑战 seed={seed}");
            meta::interactive_run(seed);
        }
        _ => {
            let mut seed: u64 = 7;
            let mut faction = Faction::Ember;
            let mut diff = Difficulty::Normal;
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
                    "--difficulty" => {
                        diff = match it.next().map(|s| s.as_str()) {
                            Some("easy") => Difficulty::Easy,
                            _ => Difficulty::Normal,
                        }
                    }
                    _ => {}
                }
            }
            meta::interactive_run_with(seed, faction, diff);
        }
    }
}
