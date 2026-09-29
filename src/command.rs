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
use crate::model::{CardInst, short_card};
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

/// L2 接缝：**dry-run**。问"这条输入引擎会不会接"，不改状态、不显示、不退出。
///
/// 实现是**在副本上真跑一遍**，不是第二套合法性规则：判据与 `execute` 同一个引擎，
/// 所以壳（TUI 悬停高亮 / 2D 拖拽预亮）不可能出现"显示能走、真按下去被拒"。
/// 立得住的前提是"引擎拒收时零副作用"——那不是一个信念，由
/// [`probe_agrees_with_execute_and_never_touches_the_battle`] 逐条机检。
/// 返回体刻意只有"会不会被拒 + 理由"，不含该显示的文本：帮助/日志这类正常输出不是合法性问题。
// 生产构建里今天没有调用方（调用方＝TUI/2D 壳），用 `expect` 而不是 `allow`：壳真接上那一刻它会报"未满足"，逼着删掉这条豁免。
// 挂成 `cfg_attr(not(test))` 是为了不在测试构建里反噬——测试正是当下的唯一调用方。
#[cfg_attr(not(test), expect(dead_code, reason = "L2 接缝等壳来调；壳接上后请删掉这条 expect"))]
pub fn probe(b: &Battle, line: &str) -> Verdict {
    let mut mirror = b.clone();
    match execute(&mut mirror, parse(line)) {
        Exec::Refused(reason) => Verdict::Illegal(reason),
        _ => Verdict::Legal,
    }
}

/// [`probe`] 的答复。只有两个值是有意义的：合法性；不合法时引擎自己的那句理由。
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(test), expect(dead_code, reason = "与 `probe` 同一条"))]
pub enum Verdict {
    Legal,
    /// 引擎给出的拒绝理由，与真跑一遍 `execute` 拿到的 `Exec::Refused` **同一句**（不是复述）。
    Illegal(String),
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

/// 下标（手牌位／继承堆位）：读不懂就交一个必定越界的数，让执行方去报它自己那句
/// 「手牌下标越界」／「下标无效」，命令行这一层不另造一套措辞（改前即如此，逐字节对账要它不变）。
fn parse_index(s: &str) -> usize {
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
            Ok(col) => Command::Place { hand_idx: parse_index(idx), slot: col },
            Err(why) => Command::Reject(why),
        },
        ("s", Some(slot), None, None) => match parse_slot(slot) {
            Ok(col) => Command::SacrificeField { slot: col },
            Err(why) => Command::Reject(why),
        },
        ("sh", Some(idx), None, None) => Command::SacrificeHand { hand_idx: parse_index(idx) },
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

// ---------------------------------------------------------------------------
// 准备／结算阶段（关与关之间）：第二张词表。
// ---------------------------------------------------------------------------

/// 结算阶段的一条命令。词表与 arity 逐条抄自改前 `meta.rs::settle_phase`。
#[derive(Debug)]
pub enum SettleCommand<'a> {
    /// `go`——进入下一关（准备阶段＝直接开战）。
    Go,
    /// `q`/`quit`/`exit`。旧实现在这里 `process::exit(0)` 且**不输出任何文本**，那个"无文案"也是行为的一部分。
    Quit,
    Fuse { main: usize, sub: usize },
    Drop(usize),
    Move { from: usize, to: usize },
    Up { idx: usize, kind: &'a str },
    Blank,
    Unknown(&'a str),
}

/// 结算阶段一条命令的产出。
#[derive(Debug)]
pub enum SettleStep {
    /// 阶段继续，先把这些行显示出去。
    Stay(Vec<String>),
    /// 阶段结束（`go`，与旧实现里 stdin 读到 EOF 走的是同一条路）。
    Go,
    /// 整个程序退出（旧实现在这里 `process::exit(0)` 且**不输出任何文本**）。
    Quit,
}

/// 一行输入 → 结算阶段的一条命令。
pub fn parse_settle(line: &str) -> SettleCommand<'_> {
    let mut it = line.split_whitespace();
    let Some(word) = it.next() else { return SettleCommand::Blank };
    let a = it.next();
    let b = it.next();
    match (word, a, b, it.next()) {
        ("go", _, _, _) => SettleCommand::Go,
        ("q", _, _, _) | ("quit", _, _, _) | ("exit", _, _, _) => SettleCommand::Quit,
        ("fuse", Some(m), Some(s), None) => SettleCommand::Fuse { main: parse_index(m), sub: parse_index(s) },
        ("drop", Some(i), None, None) => SettleCommand::Drop(parse_index(i)),
        ("move", Some(f), Some(t), None) => SettleCommand::Move { from: parse_index(f), to: parse_index(t) },
        ("up", Some(i), Some(k), None) => SettleCommand::Up { idx: parse_index(i), kind: k },
        _ => SettleCommand::Unknown(word),
    }
}

/// 执行结算阶段的一条命令。
///
/// 返回的是**成品行**（含各自的 `✓ `/`✖ `），不是"前缀＋理由"两截。理由：这张词表历史上就有三种写法
/// ——`✓ {msg}`、`✖ {e}`、以及 `drop` 那种无前缀的「弃置 …（进弃牌堆…）」，还有 `move` 把 ✓ 写在串内的
/// 「✓ 排序：…」。拆成两截要么改文案（撞逐字节尺），要么让壳去猜哪条该加什么。归一属"文案改造"另一帧的事，
/// 现在登记不实现。
/// `up_used` 是"本次结算限 1 张"的闸（§十一:402），由调用方按阶段持有——它不进存档，所以跨关自然归零。
pub fn execute_settle(
    inherit: &mut Vec<CardInst>,
    karma: &mut i32,
    up_used: &mut bool,
    post_battle: bool,
    cmd: SettleCommand<'_>,
) -> SettleStep {
    let one = |s: String| vec![s];
    match cmd {
        SettleCommand::Go => SettleStep::Go,
        SettleCommand::Quit => SettleStep::Quit,
        SettleCommand::Blank => SettleStep::Stay(Vec::new()),
        SettleCommand::Unknown(w) => SettleStep::Stay(one(format!("✖ 未知命令：{w}"))),
        SettleCommand::Fuse { main, sub } => match crate::progress::fuse_cards(inherit, main, sub, karma) {
            Ok(msg) => SettleStep::Stay(one(format!("✓ {msg}"))),
            Err(e) => SettleStep::Stay(one(format!("✖ {e}"))),
        },
        SettleCommand::Drop(i) => {
            if i < inherit.len() {
                let c = inherit.remove(i);
                SettleStep::Stay(one(format!("弃置 {}（进弃牌堆，本关不再使用；自造弃牌永久消失）", short_card(&c))))
            } else {
                SettleStep::Stay(one("✖ 下标无效".into()))
            }
        }
        SettleCommand::Move { from, to } => {
            if from < inherit.len() && to < inherit.len() {
                let c = inherit.remove(from);
                inherit.insert(to, c);
                SettleStep::Stay(one(format!("✓ 排序：第{from}位 → 第{to}位（抽牌从堆顶起）")))
            } else {
                SettleStep::Stay(one("✖ 下标无效".into()))
            }
        }
        SettleCommand::Up { idx, kind } => {
            if !post_battle {
                return SettleStep::Stay(one("✖ 升级是通关奖励，仅结算阶段可用".into()));
            }
            if *up_used {
                return SettleStep::Stay(one("✖ 每次通关只能升级1张（§十一:402 / §廿二:965）".into()));
            }
            match crate::progress::upgrade_card(inherit, idx, kind) {
                Ok(msg) => {
                    *up_used = true;
                    SettleStep::Stay(one(format!("✓ {msg}")))
                }
                Err(e) => SettleStep::Stay(one(format!("✖ {e}"))),
            }
        }
    }
}

/// 继承堆的一条清单行。改前住在 `meta.rs` 的循环里，文案逐字搬来。
pub fn inherit_line(i: usize, c: &CardInst) -> String {
    format!(
        "  [{i}] {} | 特性:{} | 死过{}次{}",
        short_card(c),
        c.def.tr.label(),
        c.deaths,
        if c.is_starter() { "〈开端·不可融合〉" } else { "" }
    )
}

/// 阶段提示行（`post_battle` 决定是通关奖励口径还是纯准备口径）。
pub fn settle_hint(karma: i32, post_battle: bool, up_used: bool) -> String {
    if post_battle {
        format!(
            "（可保留业力 {karma}：fuse <主> <副>；up <idx> power|thr|skill（本结算限1张{}）；drop <idx>；move <从> <到>；go）",
            if up_used { "已用完" } else { "余1" }
        )
    } else {
        "（准备阶段：可 fuse/drop/move 后 go；go 直接开战）".to_string()
    }
}

/// 一整个「继承堆清单 + 提示」文本块（每行自带换行）。调用方接 `> ` 提示符。
/// 它是逐字搬来的：改前三段 `println!` 的顺序、括号、全角冒号一个字没动。
pub fn settle_listing(inherit: &[CardInst], karma: i32, post_battle: bool, up_used: bool) -> String {
    let mut s = format!("继承堆（{}张）：\n", inherit.len());
    for (i, c) in inherit.iter().enumerate() {
        s.push_str(&inherit_line(i, c));
        s.push('\n');
    }
    s.push_str(&settle_hint(karma, post_battle, up_used));
    s.push('\n');
    s
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

    // ---------------- 结算阶段 ----------------

    fn pile(n: usize) -> Vec<CardInst> {
        (0..n).map(|i| CardInst::new(i as u64 + 1, crate::model::faction_cards(Faction::Ember)[i])).collect()
    }

    fn run(lines: &[&str], inherit: &mut Vec<CardInst>, karma: &mut i32, post_battle: bool) -> Vec<String> {
        let mut used = false;
        let mut out = Vec::new();
        for l in lines {
            match execute_settle(inherit, karma, &mut used, post_battle, parse_settle(l)) {
                SettleStep::Stay(v) => out.extend(v),
                SettleStep::Go => out.push("«GO»".into()),
                SettleStep::Quit => out.push("«QUIT»".into()),
            }
        }
        out
    }

    #[test]
    fn the_settle_word_table_maps_every_recognised_line() {
        use SettleCommand::*;
        assert!(matches!(parse_settle("go"), Go));
        assert!(matches!(parse_settle("go 1 2"), Go), "arity 不卡：改前 `\"go\" => return` 没有长度守卫");
        for w in ["q", "quit", "exit"] {
            assert!(matches!(parse_settle(w), Quit), "{w}");
        }
        assert!(matches!(parse_settle("fuse 1 2"), Fuse { main: 1, sub: 2 }));
        assert!(matches!(parse_settle("drop 3"), Drop(3)));
        assert!(matches!(parse_settle("move 0 1"), Move { from: 0, to: 1 }));
        assert!(matches!(parse_settle("up 2 thr"), Up { idx: 2, kind: "thr" }));
        assert!(matches!(parse_settle(""), Blank));
        assert!(matches!(parse_settle("  "), Blank));
        // arity 不足 → 落进 Unknown（改前 `"fuse" if parts.len()==3` 失配后走 `other` 分支）。
        for (line, word) in [("fuse 1", "fuse"), ("fuse", "fuse"), ("drop", "drop"), ("move 1", "move"), ("up 1", "up"), ("nope", "nope")] {
            assert!(matches!(parse_settle(line), Unknown(w) if w == word), "{line}");
            assert_eq!(run(&[line], &mut pile(2), &mut 0, true), vec![format!("✖ 未知命令：{word}")]);
        }
    }

    #[test]
    fn settle_output_lines_are_the_ones_the_cli_printed() {
        // 成品行逐字钉住：这张词表历史上就有三种前缀（`✓ x`／`✖ x`／`x`），归一属文案改造，不在本帧。
        let mut inherit = pile(4);
        let mut k = 9;
        // 下标 0 是开端，故这一对用 1(火苗) 与 3(焚稿人)——副牌 3 费 → 收 2 业力。
        let out = run(&["fuse 1 3"], &mut inherit, &mut k, true);
        assert!(out.len() == 1 && out[0].starts_with("✓ "), "{out:?}");
        assert_eq!(k, 7, "融合扣业力");
        assert_eq!(run(&["fuse 1 3"], &mut inherit, &mut k, true), vec!["✖ 下标无效"], "副牌已被摘走（不进弃牌堆，堆长 4→3）");
        assert_eq!(run(&["drop 9"], &mut inherit, &mut k, true), vec!["✖ 下标无效"]);
        assert_eq!(run(&["move 9 0"], &mut inherit, &mut k, true), vec!["✖ 下标无效"]);
        assert_eq!(run(&["", "xx 1"], &mut inherit, &mut k, true), vec!["✖ 未知命令：xx"], "空行零输出");
        assert_eq!(run(&["go"], &mut inherit, &mut k, true), vec!["«GO»"]);
        assert_eq!(run(&["q"], &mut inherit, &mut k, true), vec!["«QUIT»"], "退出口径＝无文案");

        let mut one = pile(1);
        let mut zero = 0;
        assert_eq!(run(&["drop 0"], &mut one, &mut zero, true), vec![format!("弃置 {}（进弃牌堆，本关不再使用；自造弃牌永久消失）", short_card(&CardInst::new(1, crate::model::faction_cards(Faction::Ember)[0])))]);
        assert!(one.is_empty());
    }

    #[test]
    fn the_upgrade_gate_is_the_phase_not_the_index() {
        // 顺序与改前一致：先判阶段、再判额度，最后才轮到 `progress::upgrade_card` 的下标检查。
        let mut inherit = pile(2);
        let mut k = 0;
        assert_eq!(run(&["up 0 power"], &mut inherit, &mut k, false), vec!["✖ 升级是通关奖励，仅结算阶段可用"]);
        assert_eq!(run(&["up 99 power"], &mut inherit, &mut k, false), vec!["✖ 升级是通关奖励，仅结算阶段可用"]);
        let mut probe = pile(2);
        let first = crate::progress::upgrade_card(&mut probe, 0, "power").unwrap();
        assert_eq!(
            run(&["up 0 power", "up 1 thr"], &mut inherit, &mut k, true),
            vec![format!("✓ {first}"), "✖ 每次通关只能升级1张（§十一:402 / §廿二:965）".to_string()]
        );
        assert_eq!(inherit[0].upgrades, 1, "限1张＝只升了一张");
    }

    #[test]
    fn the_listing_and_hint_keep_the_pre_refactor_wording() {
        let mut list = pile(2);
        list[0].deaths = 2;
        list[1] = CardInst::new(7, crate::model::STARTER);
        let starter = &list[1];
        let txt = settle_listing(&list, 5, true, false);
        assert_eq!(txt.lines().next().unwrap(), "继承堆（2张）：");
        assert!(txt.contains("  [0] ") && txt.contains("死过2次"), "{txt}");
        assert!(txt.contains("〈开端·不可融合〉"), "{txt}");
        assert!(txt.ends_with("（可保留业力 5：fuse <主> <副>；up <idx> power|thr|skill（本结算限1张余1）；drop <idx>；move <从> <到>；go）\n"), "{txt}");
        assert!(settle_listing(&[], 0, false, false).ends_with("（准备阶段：可 fuse/drop/move 后 go；go 直接开战）\n"));
        assert!(settle_listing(&[], 0, true, true).contains("本结算限1张已用完"));
        assert_eq!(inherit_line(3, &starter), format!("  [3] {} | 特性:{} | 死过0次〈开端·不可融合〉", short_card(&starter), starter.def.tr.label()));
    }
}

#[cfg(test)]
mod probe_tests {
    //! L2 的那两条尺子：问一次不许动盘；答复必须与真跑一遍**逐字**相同。
    //! 状态指纹**每字段一段**，于是不一致时直接看得见是哪个字段动的，而不是只拿到"两坨不等"。
    use super::{Exec, Verdict, execute, parse, probe};
    use crate::battle::{Battle, Difficulty};
    use crate::model::Faction;

    fn segs(b: &Battle) -> Vec<String> {
        let slot = |c: &Option<crate::model::CardInst>| c.as_ref().map(|x| format!("{}#{}", x.def.name, x.id)).unwrap_or_default();
        let ids = |v: &[crate::model::CardInst]| v.iter().map(|c| c.id.to_string()).collect::<Vec<_>>().join(",");
        let mut out = vec![
            format!("rng={:?}", b.rng.state()),
            format!("next_id={}", b.next_id),
            format!("seq={}", b.seq),
            format!("turn={}", b.turn),
            format!("over={:?}", b.over),
            format!("log={}", b.log.len()),
            format!("p_karma={}", b.p_karma),
            format!("e_karma={}", b.e_karma),
            format!("p_candle={}", b.p_candle),
            format!("e_candle={}", b.e_candle),
            format!("pf={}|{:?}|{}|{}|{}", b.pf.sacrifice_used, b.pf.sacrificed_names, b.pf.manual_draws, b.pf.starter_draws, b.pf.starter_gains),
            format!("ef={}", b.ef.sacrifice_used),
            format!("penalty={}", b.karma_penalty_next),
            format!("rollback={}", b.rollback_left),
            format!("dealt={}", b.dealt_this_turn),
            format!("atkphase={}", b.in_player_attack_phase),
            format!("boosts={:?}", b.boosts),
            format!("order={:?}", b.attack_order),
            format!("pend_candle={}", b.pending_candle_d),
            format!("pend_card={:?}", b.pending_card_d),
            format!("starter={}", b.starter_pile),
            format!("ehand={}", ids(&b.enemy_hand)),
            format!("epile={}", ids(&b.enemy_pile)),
            format!("ediscard={}", ids(&b.e_discard)),
            format!("discard={}", ids(&b.discard_pile)),
            format!("hand={}", ids(&b.hand)),
            format!("draw={}", ids(&b.draw_pile)),
        ];
        for (i, c) in b.p_front.iter().enumerate() {
            out.push(format!("p_front{i}={}", slot(c)));
        }
        for (i, c) in b.e_back.iter().enumerate() {
            out.push(format!("e_back{i}={}", slot(c)));
        }
        for (i, c) in b.e_front.iter().enumerate() {
            out.push(format!("e_front{i}={}", slot(c)));
        }
        out
    }

    fn diff(before: &[String], after: &[String]) -> Vec<String> {
        before.iter().zip(after).filter(|(x, y)| x != y).map(|(x, y)| format!("{x} → {y}")).collect()
    }

    /// 同一条脚本走两遍，`ask=true` 时每条都先 `probe` 再 `execute`。返回最终指纹与"被拒条数"。
    fn replay(script: &[&str], ask: bool) -> (Vec<String>, usize) {
        let mut b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        let mut refused = 0;
        for line in script {
            let verdict = ask.then(|| probe(&b, line));
            let exec = match execute(&mut b, parse(line)) {
                Exec::Done => Verdict::Legal,
                Exec::Print(_) => Verdict::Legal,
                Exec::Quit(_) => Verdict::Legal,
                Exec::Refused(reason) => {
                    refused += 1;
                    Verdict::Illegal(reason)
                }
            };
            // 理由**逐字**相同：UI 拿这句去提示玩家，不能是命令层自己另写的一套话。
            if let Some(v) = verdict {
                assert_eq!(v, exec, "probe({line:?}) 与真跑一遍不一致");
            }
        }
        (segs(&b), refused)
    }

    #[test]
    fn probe_agrees_with_execute_and_never_touches_the_battle() {
        // 脚本专挑"应当被拒"的那批：越界下标、空场献祭、同回第二次献祭、抽穿牌堆、arity 不对。
        let script = [
            "p 0 P1",
            "p 0 P1",
            "sh 1",
            "sh 9",
            "s P2",
            "e",
            "di",
            "di",
            "di",
            "di",
            "di",
            "ds",
            "ds",
            "ds",
            "ds",
            "ds",
            "p 0",
            "p 0 P1 x",
            "s P9",
            "s PX",
            "zz",
            "",
            "l",
            "h",
            "b",
        ];
        let (asked, refused) = replay(&script, true);
        // 不问的那一遍是对照组：`&Battle` 挡得住显式改盘，挡不住 `Cell`/`RefCell` 那类内部可变性
        // 漏出来的状态。两遍最终指纹全等，才是"问过不算数"的可执行版本。
        let (plain, _) = replay(&script, false);
        assert_eq!(asked, plain, "问过一遍与没问过一遍，棋盘不该有任何字段不同");
        // 少于这个数，说明上面那批"应当被拒"的没真被拒——尺子就空转了。
        assert!(refused >= 6, "本脚本实际只量到 {refused} 条拒绝，覆盖面不足以钉住 probe");
    }

    #[test]
    fn a_refusal_leaves_no_trace_and_a_legal_place_does_not_stay_legal_after_it_is_used() {
        // 前半截是 probe 立身的前提：**拒收零副作用**（不是"看起来没改"，是指纹全等）。
        let mut b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        let before = segs(&b);
        for line in ["p 99 P1", "s P1", "sh 99", "p 0 P5", "p 0 P10"] {
            assert!(matches!(probe(&b, line), Verdict::Illegal(_)), "{line} 该被判非法");
        }
        let dirty = diff(&before, &segs(&b));
        assert!(dirty.is_empty(), "五条拒绝之后一个字段都不该动，实际动了：{dirty:?}");
        // 后半截：合法性能量，但**问过不算数**——同一个空场，`p 0 P1` 先合法、放下后手牌没了就非法。
        assert_eq!(probe(&b, "p 0 P1"), Verdict::Legal);
        let again = probe(&b, "p 0 P1");
        assert_eq!(again, Verdict::Legal, "只 probe 不改盘 ⇒ 同一状态同一答复");
        execute(&mut b, parse("p 0 P1"));
        // P1 已被占 ⇒ 同一句 `p 0 P1` 现在该被拒。probe 读的是**当下**，不是上一次的缓存。
        assert_ne!(probe(&b, "p 0 P1"), again, "盘子前进了一步，答复必须跟着变");
    }
}
