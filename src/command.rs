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
//! 战斗内命令：把「用户敲进去的那一行」解析成动作，并把文案与退出意图**作为返回值**交回调用方。
//! 这是呈现接口 L3 的第一刀——TUI/2D 拿不到 stdin，只能拿到一个 `Command` 和一个 `Exec`。
//!
//! 三条边界线，每条都有测拦着：
//! - **不写 stdout、不 `process::exit`**：`l`/`b`/`h` 的文本、拒绝理由、退出提示全部走 `Exec`。
//!   CLI 那套 `println!` + `exit(0)` 原样留在 `meta.rs`，所以逐字节输出没变。
//! - **不读引擎簿记**：只调 `Battle` 的 pub 动作，不碰 `log`/`p_karma`/`hand` 这些字段。
//! - **格位是严格集合**：§十二:431 写的是「P1-P4」，旧实现却按「取末位数字」解释，于是
//!   `P10`／`P0`／`P-1` 全都落进 P1。实测 `s P10` 与 `s P1` 的输出 diff 0 行——用户打了一个不存在的格位，
//!   动的是场上的 P1。这里改成拒收，理由串沿用引擎自己那句「格位为 P1-P4」，所以本来就非法的
//!   `P5`/`PX` 一字不变，只有那三个原本被静默改写的写法开始报错。

use crate::battle::Battle;
use crate::boss;
use crate::render;

/// 帮助行。逐字抄自改前的 `meta.rs`，它是这局游戏里唯一被机检钉住的 UI 文案。
pub const HELP: &str = "p <手牌idx> <P1-P4> 放置 | s <Pn> 场上献祭 | sh <idx> 手牌献祭 | di/ds 抽继承/开端 | e 结束回合 | l 日志 | b Boss档案 | q 退出";

/// 一条已解析的战斗内命令。
#[derive(Debug)]
pub enum Command<'a> {
    Place { hand_idx: usize, slot: usize },
    SacrificeField { slot: usize },
    SacrificeHand { hand_idx: usize },
    DrawInherit,
    DrawStarter,
    EndTurn,
    ShowLog,
    ShowDossier,
    Help,
    /// 退出意图。**这里只是意图**——本模块不结束进程，怎么落地由调用方决定。
    Quit,
    /// 词表里没有（或 arity 不对）的第一个词。
    Unknown(&'a str),
    /// 空行／纯空白：什么都不做。
    Blank,
    /// 解析阶段就判死的输入。带着拒绝理由，`execute` 不会因此碰引擎。
    Reject(&'static str),
}

/// 执行结果：要么什么都没发生，要么是一段该显示的文本，要么是要退出。
#[derive(Debug)]
pub enum Exec {
    Done,
    /// 正常输出（日志、档案、帮助）。CLI 用 `println!` 落它。
    Print(String),
    /// 拒绝理由，**不带 `✖ ` 前缀**——前缀是 CLI 的显示风格，不是命令层的语义。
    Refused(String),
    /// 退出意图 + 该先显示的文本。
    Quit(&'static str),
}

/// 格位 = P1..P4（内部 0..3），或裸数字 1..4。旧的「取末位数字」读法会把 `P10` 折成 P1，见模块头。
fn parse_slot(s: &str) -> Result<usize, &'static str> {
    let bad = Err("格位为 P1-P4");
    let b = s.as_bytes();
    let one = |x: u8| x.is_ascii_digit();
    let n = if b.len() == 1 && one(b[0]) {
        (b[0] - b'0') as usize
    } else if b.len() == 2 && (b[0] == b'P' || b[0] == b'p') && one(b[1]) {
        (b[1] - b'0') as usize
    } else {
        return bad;
    };
    if (1..=4).contains(&n) {
        Ok(n - 1)
    } else {
        bad
    }
}

/// 手牌下标：读不懂就交一个必定越界的数，让引擎去报它自己那句「手牌下标越界」，
/// 命令行这一层不另造一套措辞（改前即如此，逐字节对账要它不变）。
fn parse_hand_idx(s: &str) -> usize {
    s.parse().unwrap_or(99)
}

/// 一行输入 → 一条命令。arity 判定与改前的字符串 match 逐条对齐：只有 `p`(3)／`s`(2)／`sh`(2) 卡长度，
/// `di`/`ds`/`e`/`l`/`b`/`q`/`h` 多带参数照旧忽略——「多余参数报错」是新行为，不在这次重构的范围里。
pub fn parse(line: &str) -> Command<'_> {
    let mut it = line.split_whitespace();
    let Some(word) = it.next() else { return Command::Blank };
    let a = it.next();
    let b = it.next();
    match (word, a, b, it.next()) {
        ("p", Some(idx), Some(slot), None) => match parse_slot(slot) {
            Ok(col) => Command::Place { hand_idx: parse_hand_idx(idx), slot: col },
            Err(why) => Command::Reject(why),
        },
        ("s", Some(slot), None, None) => match parse_slot(slot) {
            Ok(col) => Command::SacrificeField { slot: col },
            Err(why) => Command::Reject(why),
        },
        ("sh", Some(idx), None, None) => Command::SacrificeHand { hand_idx: parse_hand_idx(idx) },
        ("di", _, _, _) => Command::DrawInherit,
        ("ds", _, _, _) => Command::DrawStarter,
        ("e", _, _, _) => Command::EndTurn,
        ("l", _, _, _) => Command::ShowLog,
        ("b", _, _, _) => Command::ShowDossier,
        ("q", _, _, _) => Command::Quit,
        ("h", _, _, _) | ("help", _, _, _) => Command::Help,
        _ => Command::Unknown(word),
    }
}

/// 执行一条命令。`Reject`／`Unknown`／`Blank` 三条路径一个字都不改引擎状态。
pub fn execute(b: &mut Battle, cmd: Command<'_>) -> Exec {
    let r = match cmd {
        Command::Place { hand_idx, slot } => b.player_place(hand_idx, slot),
        Command::SacrificeField { slot } => b.player_sacrifice_field(slot),
        Command::SacrificeHand { hand_idx } => b.player_sacrifice_hand(hand_idx),
        Command::DrawInherit => b.action_draw(false),
        Command::DrawStarter => b.action_draw(true),
        Command::EndTurn => {
            b.end_player_turn();
            Ok(())
        }
        Command::ShowLog => return Exec::Print(render::log_tail(b, 40)),
        Command::ShowDossier => return Exec::Print(boss::dossier(b)),
        Command::Help => return Exec::Print(HELP.to_string()),
        Command::Quit => return Exec::Quit("弃局退出。"),
        Command::Blank => return Exec::Done,
        Command::Reject(why) => return Exec::Refused(why.to_string()),
        Command::Unknown(w) => return Exec::Refused(format!("未知命令：{w}（h 看帮助）")),
    };
    match r {
        Ok(()) => Exec::Done,
        Err(e) => Exec::Refused(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Difficulty;
    use crate::model::Faction;

    fn battle() -> Battle {
        Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1)
    }

    /// 拒绝理由文本（`Exec::Refused` 的内容），方便逐条比对措辞。
    fn refused(b: &mut Battle, line: &str) -> String {
        match execute(b, parse(line)) {
            Exec::Refused(s) => s,
            other => panic!("期望 Refused，实到 {other:?}（输入「{line}」）"),
        }
    }

    #[test]
    fn the_word_table_maps_to_commands_with_the_old_arity() {
        use Command::*;
        assert!(matches!(parse("di"), DrawInherit));
        assert!(matches!(parse("ds"), DrawStarter));
        assert!(matches!(parse("e"), EndTurn));
        assert!(matches!(parse("l"), ShowLog));
        assert!(matches!(parse("b"), ShowDossier));
        assert!(matches!(parse("q"), Quit));
        assert!(matches!(parse("h"), Help));
        assert!(matches!(parse("help"), Help));
        assert!(matches!(parse("di 1 2 3"), DrawInherit), "多带参数照旧忽略");
        assert!(matches!(parse("   "), Blank));
        assert!(matches!(parse(""), Blank));
        // arity 不够／过头都落进 Unknown，措辞与改前同一句。
        for line in ["p", "p 0", "p 0 P1 x", "s", "sh", "xx 1"] {
            let got = parse(line);
            let want = match line.split_whitespace().next().unwrap() {
                "p" => "p",
                "s" => "s",
                "sh" => "sh",
                _ => "xx",
            };
            assert!(matches!(&got, Unknown(w) if *w == want), "{line} → {got:?}");
            assert_eq!(refused(&mut battle(), line), format!("未知命令：{want}（h 看帮助）"));
        }
    }

    #[test]
    fn slot_is_a_strict_set_not_the_last_digit() {
        for (txt, col) in [("P1", 0), ("P4", 3), ("p2", 1), ("3", 2), ("4", 3)] {
            assert_eq!(parse_slot(txt), Ok(col), "{txt}");
        }
        // P0/P10/P-1 在旧实现里都折成 0＝P1（真被静默改写）；P5..P9/PX 本来就非法。
        for txt in ["P0", "P5", "P9", "P10", "P04", "PX", "P-1", "p", "", "１", "PP1"] {
            assert_eq!(parse_slot(txt), Err("格位为 P1-P4"), "{txt}");
        }
        assert!(matches!(parse("s P10"), Command::Reject("格位为 P1-P4")));
        assert!(matches!(parse("p 0 P1"), Command::Place { hand_idx: 0, slot: 0 }));
    }

    #[test]
    fn p10_stops_borrowing_p1() {
        // 改前实测：`go / s P10 / l / q` 与 `go / s P1 / l / q` 的输出 diff 0 行——
        // 一个不存在的格位动掉了 P1 上的牌。现在它必须被拒。
        let mut b = battle();
        let line = refused(&mut b, "s P10");
        assert_eq!(line, "格位为 P1-P4");
        assert_eq!(b.p_front[0].as_ref().map(|c| c.def.name), None, "P1 不该被动过");
    }

    #[test]
    fn a_refused_command_leaves_the_engine_untouched() {
        let mut b = battle();
        let (karma, hand, turn, loglen) = (b.p_karma, b.hand.len(), b.turn, b.log.len());
        // "sh x" 的 x 读不懂 → 下标 99 → 引擎自己报越界，同样不改状态。
        for line in ["s P10", "p 0 P0", "p 0 P-1", "sh x", "zz"] {
            assert!(!refused(&mut b, line).is_empty(), "{line} 该给出拒绝理由");
        }
        assert_eq!((b.p_karma, b.hand.len(), b.turn, b.log.len()), (karma, hand, turn, loglen));
        // 非法输入没吃掉「每回合 1 次」额度：P1 空着，合法输入的分支照旧是"该格没有卡牌"。
        assert_eq!(refused(&mut b, "s P1"), "该格没有卡牌");
        assert_eq!((b.p_karma, b.hand.len(), b.log.len()), (karma, hand, loglen));
    }

    #[test]
    fn quit_is_an_intent_not_a_process_exit() {
        let mut b = battle();
        match execute(&mut b, parse("q")) {
            Exec::Quit(msg) => assert_eq!(msg, "弃局退出。"),
            other => panic!("期望 Quit，实到 {other:?}"),
        }
        // 空行什么都不做，也不返回文本。
        assert!(matches!(execute(&mut b, parse("")), Exec::Done));
    }

    #[test]
    fn the_help_line_is_the_ones_the_cli_shipped() {
        // 这条测把「UI 文案漂移」钉住：改前它硬编码在 meta.rs 的 println! 里。
        assert_eq!(HELP, "p <手牌idx> <P1-P4> 放置 | s <Pn> 场上献祭 | sh <idx> 手牌献祭 | di/ds 抽继承/开端 | e 结束回合 | l 日志 | b Boss档案 | q 退出");
        assert!(matches!(execute(&mut battle(), parse("h")), Exec::Print(s) if s == HELP));
    }

    #[test]
    fn the_command_layer_never_prints_or_exits() {
        let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/command.rs")).unwrap();
        let banned = ["println!", "print!(", "process::exit", "std::io", "io::stdin"];
        let mut offenders: Vec<String> = Vec::new();
        for (i, l) in src.lines().enumerate() {
            let t = l.trim();
            if t.starts_with("//") || t.contains("mod tests") || t.contains("let banned") {
                continue;
            }
            if banned.iter().any(|x| t.contains(x)) {
                offenders.push(format!("{}: {t}", i + 1));
            }
        }
        assert!(offenders.is_empty(), "command.rs 的非注释行里出现了输出/退出/stdin：{offenders:?}");
    }
}
