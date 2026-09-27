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
//! 《中线》核心逻辑 CLI。用法见 `USAGE`（`cargo run -- help` 打印）。

mod ai;
mod battle;
mod boss;
mod meta;
mod model;
mod render;
mod rng;

use battle::Difficulty;
use boss::BossId;
use model::Faction;

const USAGE: &str = "\
《中线》核心规则 CLI
用法：
  play [--seed N] [--faction 1|2|3] [--difficulty X]        普通爬关（阵营三循环）
  play --boss <id> [--seed N] [--faction 1|2|3]             跳打某章末 Boss（同 boss <id>）
  mainline [--seed N] [--faction 1|2|3] [--difficulty X]     主线 60 关（5 章，章末 Boss）
  boss <id> [--seed N] [--faction 1|2|3]                    单关跳打 Boss
  daily                                                     每日挑战（以日期为种子）
  auto [N] [--difficulty X]                                 AI 托管模拟 N 局（默认 1）
  auto [N] --boss all|<id> [--faction 1|2|3]                Boss 脚本冒烟：N 轮 × 5 个（或指定）
  help                                                      本帮助
<id> ＝ 1-5 | luzhu|雪爵… 见下：1 炉主 / 2 雪爵 / 3 影长 / 4 炎与冰 / 5 终影
（Boss 战敌方由脚本接管，不吃难度档；--difficulty 只影响我方托管评分口径 —— 裁定21）
战斗内命令：
  p <手牌idx> <P1-P4> 放置 | s <P1-P4> 场上献祭 | sh <手牌idx> 手牌献祭
  di|ds 抽继承堆/开端堆 | e 结束回合 | l 日志 | b Boss档案 | q 退出
结算/准备阶段：fuse <主> <副> / up <idx> power|thr|skill / drop <idx> / move <从> <到> / go";

/// 吃一个值的长选项（其值不算位置参数）。
const VALUE_FLAGS: &[&str] = &["--seed", "--faction", "--difficulty", "--boss"];

/// 未知长选项一律报错：`auto 1 --bos all` 静默当普通对局跑完，比一条错误难查得多。
fn reject_unknown_flags(args: &[String]) {
    for a in args.iter() {
        if a.starts_with("--") && !VALUE_FLAGS.contains(&a.as_str()) && a != "--help" {
            println!("✖ 未知选项：{a}（可用 {}）", VALUE_FLAGS.join(" / "));
            std::process::exit(2);
        }
    }
}

/// 位置参数：跳过命令词、长选项及其值。
fn positionals(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if a.starts_with("--") {
            if VALUE_FLAGS.contains(&a.as_str()) {
                it.next();
            }
            continue;
        }
        out.push(a.clone());
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("play");
    if matches!(cmd, "help" | "--help" | "-h") {
        println!("{USAGE}");
        return;
    }
    reject_unknown_flags(&args);
    let (seed, faction) = parse_opts(&args);
    let diff = parse_difficulty(&args);
    // 无命令词、直接给标志（`midline --seed 42`）＝ play，旧版即如此，不当未知命令。
    let cmd = if cmd.starts_with('-') { "play" } else { cmd };
    match cmd {
        "auto" => {
            let n: u32 = positionals(&args).first().and_then(|s| s.parse().ok()).unwrap_or(1);
            match boss_flag(&args) {
                BossFlag::Absent => meta::auto_battles(n, diff),
                BossFlag::Value(raw) => auto_bosses(seed, faction, n, &raw),
                BossFlag::Missing => bad_boss_arg(),
            }
        }
        "daily" => {
            let seed = meta::daily_seed();
            println!("每日挑战 seed={seed}");
            meta::interactive_run(seed);
        }
        "mainline" => meta::mainline_run(seed, faction, diff),
        "play" => match boss_flag(&args) {
            BossFlag::Absent => meta::interactive_run_with(seed, faction, diff),
            BossFlag::Value(raw) => meta::boss_run(seed, faction, diff, require_boss(&raw)),
            BossFlag::Missing => bad_boss_arg(),
        },
        "boss" => match positionals(&args).first() {
            Some(raw) => meta::boss_run(seed, faction, diff, require_boss(raw)),
            None => {
                println!("✖ boss 需要一个参数：1-5 / luzhu xuejue yingzhang yanbing zhongying / 炉主 雪爵 影长 炎与冰 终影");
                std::process::exit(2);
            }
        },
        other => {
            println!("✖ 未知命令：{other}");
            println!("{USAGE}");
            std::process::exit(2);
        }
    }
}

/// `--boss` 出现了没有？带值没带值？（缺值必须报错，不能静默回落到普通对局。）
enum BossFlag {
    Absent,
    Value(String),
    Missing,
}

fn boss_flag(args: &[String]) -> BossFlag {
    match args.iter().position(|a| a == "--boss") {
        None => BossFlag::Absent,
        Some(i) => match args.get(i + 1) {
            Some(v) if !v.starts_with("--") => BossFlag::Value(v.clone()),
            _ => BossFlag::Missing,
        },
    }
}

fn bad_boss_arg() -> ! {
    println!("✖ --boss 需要一个值：all / 1-5 / luzhu … （不给 --boss 则是普通对局）");
    std::process::exit(2);
}

fn require_boss(raw: &str) -> BossId {
    match BossId::parse(raw) {
        Some(id) => id,
        None => {
            println!("✖ 未知 Boss：{raw}（可选 1-5 / luzhu xuejue yingzhang yanbing zhongying / 炉主 雪爵 影长 炎与冰 终影）");
            std::process::exit(2);
        }
    }
}

/// `auto [N] --boss all|<id>`：每轮换一个种子，托管对局打到分胜负。
/// 轮数语义与 `auto_battles` 对齐：`N=0` 就是零场，不偷偷保底一轮。
fn auto_bosses(seed: u64, faction: Faction, n: u32, raw: &str) {
    let ids: Vec<BossId> = if raw == "all" {
        BossId::all().to_vec()
    } else {
        vec![require_boss(raw)]
    };
    for round in 0..n {
        // wrapping_add：`--seed u64::MAX` 在 debug 下会因 + 溢出 panic，release 却静默回绕，
        // 同一命令跨 profile 出不同结果 ⇒ 用显式回绕把两边钉成同一种可复现行为。
        let s = seed.wrapping_add(round as u64);
        for id in &ids {
            let (out, turn) = boss::smoke_one(s, faction, *id);
            println!("boss#{round} [{}] seed={s} → {out:?} 第{turn}回合", id.name());
        }
    }
}

/// `--seed N` / `--faction 1|2|3`；未给 → 种子7·烬火。
fn parse_opts(args: &[String]) -> (u64, Faction) {
    let mut seed: u64 = 7;
    let mut faction = Faction::Ember;
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
    (seed, faction)
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
