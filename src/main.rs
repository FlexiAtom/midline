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
mod save;

use battle::Difficulty;
use boss::BossId;
use model::Faction;
use std::path::PathBuf;

const USAGE: &str = "\
《中线》核心规则 CLI
用法：
  play [--seed N] [--faction 1|2|3] [--difficulty X]        普通爬关（阵营三循环，不落盘）
  play --boss <id> [--seed N] [--faction 1|2|3]             跳打某章末 Boss（同 boss <id>，不落盘）
  mainline [--seed N] [--faction 1|2|3] [--difficulty X]     主线 60 关（5 章，章末 Boss，自动存档）
  mainline --resume [--save F] [--faction 1|2|3]            从存档的「本关入口」续打；不给 --resume 则从第1关重开（档上已有进度会先警告再覆盖）
  boss <id> [--seed N] [--faction 1|2|3]                    单关跳打 Boss
  daily [--save F]                                          每日挑战（日期为种子；打赢一关即记当天已做，不动主线进度）
  progress [--save F]                                       看当前存档（只读：不写、不修、不猜；档读不懂则 rc=2）
  progress recover [--save F]                               把当前档改名留证 .bad-N，下次从空档开始
  auto [N] [--difficulty X]                                 AI 托管模拟 N 局（默认 1，不落盘）
  auto [N] --boss all|<id> [--faction 1|2|3]                Boss 脚本冒烟：N 轮 × 5 个（或指定）
  help                                                      本帮助
<id> ＝ 1-5 | luzhu|雪爵… 见下：1 炉主 / 2 雪爵 / 3 影长 / 4 炎与冰 / 5 终影
（Boss 战敌方由脚本接管，不吃难度档；--difficulty 只影响我方托管评分口径 —— 裁定21）
（--seed / --faction 不给＝种子7·烬火；给了却读不懂 ⇒ 直接报错退出 rc=2，不静默回退成别的值）
存档（只挂在 mainline / daily 上；落点＝--save ＞ $MIDLINE_SAVE ＞ $XDG_DATA_HOME/midline/ ＞ $HOME/.local/share/midline/）：
  --save <文件>   换档的位置（缺值直接报错，不静默落回默认位置）
  --resume        读档续关；档损坏一律拒绝并指向 progress recover，不会当成空档静默重开
  存档点＝关隘入口（裁定27）：败/平/弃局都不动盘，下次续上的就是没过关的那一关
战斗内命令：
  p <手牌idx> <P1-P4> 放置 | s <P1-P4> 场上献祭 | sh <手牌idx> 手牌献祭
  di|ds 抽继承堆/开端堆 | e 结束回合 | l 日志 | b Boss档案 | q 退出
结算/准备阶段：fuse <主> <副> / up <idx> power|thr|skill / drop <idx> / move <从> <到> / go";

/// 吃一个值的长选项（其值不算位置参数）。
const VALUE_FLAGS: &[&str] = &["--seed", "--faction", "--difficulty", "--boss", "--save"];

/// 不带值的长选项（出现即为真）。它必须单列一张表：漏了它，`--resume` 会被当成未知选项，
/// 而它的值（如果紧跟位置参数）又不会被吞掉——两处错法互不相干，一起改才对齐。
const BOOL_FLAGS: &[&str] = &["--resume"];

/// 未知长选项一律报错：`auto 1 --bos all` 静默当普通对局跑完，比一条错误难查得多。
fn reject_unknown_flags(args: &[String]) {
    for a in args.iter() {
        if a.starts_with("--") && !VALUE_FLAGS.contains(&a.as_str()) && !BOOL_FLAGS.contains(&a.as_str()) && a != "--help" {
            println!(
                "✖ 未知选项：{a}（可用 {} / {}）",
                VALUE_FLAGS.join(" / "),
                BOOL_FLAGS.join(" / ")
            );
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
    gate_save_flags(cmd, &args);
    match cmd {
        "auto" => {
            let n: u32 = positionals(&args).first().and_then(|s| s.parse().ok()).unwrap_or(1);
            match value_flag(&args, "--boss") {
                Flag::Absent => meta::auto_battles(n, diff),
                Flag::Value(raw) => auto_bosses(seed, faction, n, raw),
                Flag::Missing => bad_boss_arg(),
            }
        }
        "daily" => meta::daily_run(save_path(&args)),
        "mainline" => meta::mainline_run(seed, faction, diff, save_path(&args), has_flag(&args, "--resume")),
        "play" => match value_flag(&args, "--boss") {
            Flag::Absent => meta::play_run(seed, faction, diff),
            Flag::Value(raw) => meta::boss_run(seed, faction, diff, require_boss(raw)),
            Flag::Missing => bad_boss_arg(),
        },
        "boss" => match positionals(&args).first() {
            Some(raw) => meta::boss_run(seed, faction, diff, require_boss(raw)),
            None => {
                println!("✖ boss 需要一个参数：1-5 / luzhu xuejue yingzhang yanbing zhongying / 炉主 雪爵 影长 炎与冰 终影");
                std::process::exit(2);
            }
        },
        "progress" => progress_cmd(&args),
        other => {
            println!("✖ 未知命令：{other}");
            println!("{USAGE}");
            std::process::exit(2);
        }
    }
}

/// 存档选项的作用域。`--save` 是"档放哪"，`progress` 也得认（否则只能去猜默认路径）；
/// `--resume` 只对主线有意义（每日不落进度）。带着不认的标志必须报错——
/// "以为存了、其实没存"是这一族里最难查的失败。
fn gate_save_flags(cmd: &str, args: &[String]) {
    let save_ok = matches!(cmd, "mainline" | "daily" | "progress");
    let resume_ok = cmd == "mainline";
    for (flag, ok) in [("--save", save_ok), ("--resume", resume_ok)] {
        if !ok && has_flag(args, flag) {
            println!("✖ {flag} 不用于「{cmd}」：只有 mainline / daily 落盘，play 与 boss/auto 是跳打。");
            std::process::exit(2);
        }
    }
}

/// `progress` 与 `progress recover`。
fn progress_cmd(args: &[String]) {
    let path = save_path(args);
    match positionals(args).first().map(String::as_str) {
        None => {
            let (report, corrupt) = save::describe(&path);
            print!("{report}");
            // 坏档＝非零退出，与 `SaveSlot::open`、`progress recover` 同一口径。
            // 只打印 ✖ 却返回 0，脚本读到的永远是"一切正常"，那句 ✖ 就成了只给眼睛看的装饰。
            // "没有这份档"不是坏档：它没有说谎，返回 0。
            if corrupt {
                std::process::exit(2);
            }
        }
        Some("recover") => match save::backup_and_clear(&path) {
            Ok(bak) => println!("✓ 存档 {} 已改名留证 → {}\n下一次运行从空档开始（原档一个字节都没丢，人眼确认后再处置）。", path.display(), bak.display()),
            Err(e) => {
                println!("✖ {e}");
                std::process::exit(2);
            }
        },
        Some(other) => {
            println!("✖ progress 只认识 recover（收到「{other}」）；其它情况请人自己看那个文件，本程序不替你猜坏在哪一行");
            std::process::exit(2);
        }
    }
}

/// 带值长选项的三态：没给 / 给了值 / 给了但缺值。缺值必须报错，不能静默当成没给。
enum Flag<'a> {
    Absent,
    Value(&'a str),
    Missing,
}

fn value_flag<'a>(args: &'a [String], name: &str) -> Flag<'a> {
    match args.iter().position(|a| a == name) {
        None => Flag::Absent,
        Some(i) => match args.get(i + 1) {
            Some(v) if !v.starts_with("--") => Flag::Value(v),
            _ => Flag::Missing,
        },
    }
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// 档的落点。`--save` 缺值＝报错退出：路径静默落到默认位置，会写进别人的文件。
/// 现在 `--seed`/`--faction` 也是同一口径（见 `parse_opts`）——这一族里没有任何一个标志配得上"猜一个继续"。
fn save_path(args: &[String]) -> PathBuf {
    match value_flag(args, "--save") {
        Flag::Absent => save::default_path().unwrap_or_else(|| {
            println!("✖ 定不出存档位置（$MIDLINE_SAVE / $XDG_DATA_HOME / $HOME 都没有）：请显式 --save <文件>");
            std::process::exit(2);
        }),
        Flag::Missing => {
            println!("✖ --save 需要一个文件路径（想放默认位置就别写 --save）");
            std::process::exit(2);
        }
        Flag::Value(v) => save::path_for(Some(v)).unwrap_or_else(|| {
            println!("✖ --save 的值是空白，不是路径");
            std::process::exit(2);
        }),
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

/// 标志给值却没给对：一律 `exit 2`，不替人改写命令。
fn bad_opt(got: Option<&str>, flag: &str, want: &str) -> ! {
    match got {
        None => println!("✖ {flag} 需要一个值（{want}）"),
        Some(v) => println!("✖ {flag} 的值「{v}」读不懂，要 {want}"),
    }
    std::process::exit(2);
}

/// `--seed N` / `--faction 1|2|3`；**不给** → 种子7·烬火，**给了但读不懂** → 报错退出。
/// 静默回退的代价：`--seed 4x` 跑出来的那局根本不是你要复现的那局，而输出里没有任何地方说命令被改写过；
/// `--faction 9` 更糟——回退到烬火是换掉整张卡表，头报却照样写着"烬火教团"，看着就像你打的就是那个阵营。
/// 与 `--save` 同一口径（见 `save_path`）。
fn parse_opts(args: &[String]) -> (u64, Faction) {
    let mut seed: u64 = 7;
    let mut faction = Faction::Ember;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--seed" => {
                let raw = it.next();
                match raw.and_then(|s| s.parse::<u64>().ok()) {
                    Some(v) => seed = v,
                    None => bad_opt(raw.map(String::as_str), "--seed", "非负整数"),
                }
            }
            "--faction" => {
                let raw = it.next();
                faction = match raw.map(String::as_str) {
                    Some("1") => Faction::Ember,
                    Some("2") => Faction::Frost,
                    Some("3") => Faction::Shadow,
                    other => bad_opt(other, "--faction", "1|2|3"),
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
