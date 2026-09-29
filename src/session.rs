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
//! 一关的会话推进：准备 → 战斗 → 结算这台阶段机。它**不碰 stdout、不读 stdin、不碰磁盘、不 exit**——
//! 这四件事全部经 `Host` 外抛，所以 CLI 与将来的 TUI/2D/3D 壳驱动的是同一台机器，而不是各抄一遍循环。
//!
//! 逐字节口径落在 `Host::write`：会话层给的是**原样输出的字串**，换行位置与改前 `println!`／`print!` 的
//! 混用一一对应，实现方不得增删换行。`board`／`log_tail` 只表达"此处该打一帧棋盘／一段日志尾"这个**时机**，
//! 画成什么样归实现方（CLI 用 `render::render`，2D 用 `Battle::view()` 自己画）。
//!
//! 依赖方向：`session → battle / command / progress / model`，**不认识 `meta`**。落盘槽（`SaveUse`／
//! `Progress`）、阵营轮转与 Boss 遭遇（`encounter_for`）、头报文案（`level_head`）都留在调用方——
//! 否则 `meta → session → meta` 成环，摘壳时又把 CLI 拖回来。

use crate::battle::{Battle, Outcome};
use crate::model::CardInst;

/// 会话层对外的全部接缝。实现方＝壳。
pub trait Host {
    /// 原样输出，不补换行。
    fn write(&mut self, s: &str);
    /// 打一帧棋盘。改前＝`print!("\n{}", render::render(b))`。
    /// 这里给 `&Battle` 而不是 `view::Board`：会话层只表达"该画一帧了"这个时机，画什么由壳自己取
    /// （CLI→`render`，2D/3D→`b.view()`）。传 Board 反而把 L1 的快照结构钉成唯一可画形态。
    fn board(&mut self, b: &Battle);
    /// 打战斗日志尾。改前＝`println!("{}", render::log_tail(b, 12))`。
    fn log_tail(&mut self, b: &Battle);
    /// 读一行（连同行尾换行，与 `read_line` 同一物）。`None`＝输入结束，两种 EOF 语义见 `run_level`。
    fn read(&mut self) -> Option<String>;
    /// 本关入口快照。时机是硬约束：准备阶段的 fuse/drop/move 是玩家真实决策，而 `new_battle` 会把堆搬空，
    /// 所以必须"编辑之后、搬空之前"。写不写、写到哪个档归实现方。
    fn snapshot_entry(&mut self, level: u32, inherit: &[CardInst], karma: i32);
    /// 下一关入口快照（打赢之后）。**终关不写**那条闸不在这里——它是存档形态的知识（`is_mainline_end`
    /// 配上 `SaveUse`），不是阶段机的，实现方自己判。
    fn snapshot_next(&mut self, level: u32, inherit: &[CardInst], karma: i32);
    /// 构造本关战斗（普通关与 Boss 关的差别、章强化豁免都在实现方）。
    fn new_battle(&mut self, inherit: &mut Vec<CardInst>, level: u32) -> Battle;
}

/// 一关的固定入参。头报文本由调用方算好传入（`level_head` 是 CLI 文案，且 2D 未必用它）。
pub struct LevelCfg {
    pub head: String,
    pub level: u32,
    /// 后面还有没有关：只影响败北那一句（阵亡自造牌永久消失／本局到此为止）。
    pub has_next: bool,
}

/// 一关是怎么结束的。
#[derive(Debug)]
pub enum Terminus {
    /// 打完，带战斗结果——关卡推进（level+1／终结文案）归调用方。
    Done(Outcome),
    /// 玩家要求离开这一局。`msg` 是走之前该打的那一行；`None`＝一个字节都不打
    /// （结算阶段 `q` 的改前口径：直接 `process::exit(0)`，"没有文案"也是行为的一部分）。
    Quit { msg: Option<String> },
}

/// 跑完一关。两条 EOF 语义分开对待，且各自与改前同一分支：
/// **战斗内** EOF＝输入结束 ⇒ `Quit{msg:Some("输入结束，退出。")}`；
/// **准备/结算** EOF＝视作 `go` ⇒ 正常离开该阶段，继续往下走（不退出）。
pub fn run_level<H: Host>(host: &mut H, cfg: &LevelCfg, inherit: &mut Vec<CardInst>, karma: &mut i32) -> Terminus {
    host.write(&format!("\n===== {} · 准备阶段 =====\n", cfg.head));
    if let Some(t) = settle_loop(host, inherit, karma, false) {
        return t;
    }
    host.snapshot_entry(cfg.level, inherit, *karma);
    let mut b = host.new_battle(inherit, cfg.level);  // §十二:418 进入战斗
    loop {
        host.board(&b);
        if let Some(out) = b.over {
            return after_battle(host, cfg, &mut b, inherit, karma, out);
        }
        host.write("\n> ");
        let Some(line) = host.read() else {
            return Terminus::Quit { msg: Some("输入结束，退出。".to_string()) };
        };
        if let Some(t) = battle_line(host, &mut b, &line) {  // §十二:430 行动阶段不限时；§十二:436 重复 a/b 直至业力用完或无操作（献祭另受 §四:162 每回合1次约束）
            return t;
        }
    }
}

/// 战斗内一行 → 命令层 → 落成 `Host` 的输出/退出意图。词表与语义都在 `command`，这里只做"输出/退出"的分派。
fn battle_line<H: Host>(host: &mut H, b: &mut Battle, line: &str) -> Option<Terminus> {
    use crate::command::{Exec, execute, parse};
    match execute(b, parse(line)) {
        Exec::Done => None,
        Exec::Print(s) => {
            host.write(&format!("{s}\n"));
            None
        }
        Exec::Refused(e) => {
            host.write(&format!("✖ {e}\n"));
            None
        }
        Exec::Quit(msg) => Some(Terminus::Quit { msg: Some(msg.to_string()) }),
    }
}

/// 战斗分出结果之后：尾报 → （还有下一关时）收尸 → 胜负判定 → 结算阶段 → 下一关入口快照。
/// 顺序照抄改前：`q` 走 `Terminus::Quit` 时**不会**执行 `snapshot_next`，档留在本关入口，
/// 于是下一次续上的是没打过的那一关——这与"败/平/弃局都不动盘"是同一条口径（裁定27）。
fn after_battle<H: Host>(
    host: &mut H,
    cfg: &LevelCfg,
    b: &mut Battle,
    inherit: &mut Vec<CardInst>,
    karma: &mut i32,
    out: Outcome,
) -> Terminus {
    host.log_tail(b);  // §十二:499 查看战果
    match out {
        Outcome::PlayerWin => host.write("胜：敌方烛尽（或蜡烛优势）。\n"),
        Outcome::PlayerLose => {
            let tail = if cfg.has_next { "阵亡自造牌永久消失。" } else { "本局到此为止。" };
            host.write(&format!("败：我方烛尽，人亡。{tail}\n"));
        }
        Outcome::Draw => host.write("平局（30回合蜡烛判定/双烛尽）。\n"),
    }
    if !cfg.has_next {
        return Terminus::Done(out);
    }
    for l in crate::progress::collect_survivors(b, inherit) {  // §十二:503 自造牌带入下一关
        host.write(&format!("{l}\n"));
    }
    // §廿二:967 保留战斗结束时的业力进结算阶段（融合定价花它）；下一关的战斗业力由 `Battle::new`
    // 重新起算＝"进入下一关重置为0"。这里只在这两个用途之间传递，不做跨关战斗业力累积。
    *karma = b.p_karma.max(0);
    if out == Outcome::PlayerWin {
        host.write(&format!("\n===== {} · 结算阶段 =====（融合/升级/弃置，go 进入下一关）\n", cfg.head));
        if let Some(t) = settle_loop(host, inherit, karma, true) {
            return t;
        }
        host.snapshot_next(cfg.level, inherit, *karma);
    }
    Terminus::Done(out)
}

/// 准备/结算阶段的命令循环。词表、语义、文案都在 `command`/`progress`，这里只剩"打一帧清单＋要一行输入"。
/// 返回 `Some` 只有一种可能：玩家 `q`。EOF 与 `go` 同路（返回 `None`）。
fn settle_loop<H: Host>(
    host: &mut H,
    inherit: &mut Vec<CardInst>,
    karma: &mut i32,
    post_battle: bool,
) -> Option<Terminus> {
    use crate::command::{SettleStep, execute_settle, parse_settle, settle_listing};
    let mut up_used = false;
    loop {
        host.write(&settle_listing(inherit, *karma, post_battle, up_used));
        host.write("> ");
        // `?` 在这里就是"EOF ⇒ 离开本阶段"＝改前那句 `return`（EOF 视作 `go`），不是错误传播。
        let line = host.read()?;
        match execute_settle(inherit, karma, &mut up_used, post_battle, parse_settle(&line)) {
            SettleStep::Stay(lines) => {
                for l in lines {
                    host.write(&format!("{l}\n"));
                }
            }
            SettleStep::Go => return None,
            SettleStep::Quit => return Some(Terminus::Quit { msg: None }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Difficulty;
    use crate::model::{Faction, faction_cards};

    /// 一台假壳：输入照脚本喂（跑完就用 EOF 垫），输出与 hook 调用序全记下来。
    /// 阶段机的可测性恰恰就在这里——同一台机器，CLI 只能整体跑，这里能单独断言时机与出口。
    struct Probe {
        ins: Vec<String>,
        at: usize,
        /// `write` 的累计输出（逐字节可比）。
        out: String,
        /// hook 调用序，`entry`／`battle`／`next` 各一条。
        trace: Vec<String>,
        /// 强制战斗结果：`Some` 时开局即以此结束（省掉真打完一关），用于测胜/败/平三条出口。
        force: Option<Outcome>,
        /// 打尾报那一刻的战斗业力——`after_battle` 里"结转业力"那行读的就是这个值，
        /// 从接缝处取样才能核对它搬得对不对，光看出口数字只是自己跟自己对账。
        seen_karma: Option<i32>,
    }

    impl Probe {
        fn new(ins: &[&str]) -> Probe {
            Probe {
                ins: ins.iter().map(|s| format!("{s}\n")).collect(),
                at: 0,
                out: String::new(),
                trace: Vec::new(),
                force: None,
                seen_karma: None,
            }
        }
        fn saw(&self, tag: &str) -> bool {
            self.trace.iter().any(|t| t == tag)
        }
        fn order(&self) -> String {
            self.trace.join(",")
        }
        /// 某个标记在第 N 次出现处的字节位置——用来断言**先后**，而不是"被调过"。
        fn nth(&self, marker: &str, n: usize) -> usize {
            let mut at = 0;
            for _ in 0..=n {
                at = self.out[at..].find(marker).unwrap_or_else(|| panic!("{} 只出现 {n} 次：{}", marker, self.out)) + at;
                at += marker.len();
            }
            at - marker.len()
        }
        fn count(&self, marker: &str) -> usize {
            self.out.matches(marker).count()
        }
    }

    /// 两个呈现接缝都**落字到 `out`**：若只往一个自增字段里记数，测的就不是"打没打、打在哪"，
    /// 而是"我给自己记了几笔"——删掉会话层的调用照样绿。
    impl Host for Probe {
        fn write(&mut self, s: &str) {
            self.out.push_str(s);
        }
        fn board(&mut self, _b: &Battle) {
            self.out.push_str("\n[棋盘]\n");
        }
        fn log_tail(&mut self, b: &Battle) {
            self.seen_karma = Some(b.p_karma);
            self.out.push_str("[尾报]\n");
        }
        fn read(&mut self) -> Option<String> {
            let l = self.ins.get(self.at).cloned();
            self.at += 1;
            l
        }
        fn snapshot_entry(&mut self, level: u32, inherit: &[CardInst], karma: i32) {
            self.trace.push(format!("entry@{level}/{}张/{karma}业", inherit.len()));
        }
        fn snapshot_next(&mut self, level: u32, inherit: &[CardInst], _karma: i32) {
            self.trace.push(format!("next@{level}/{}张", inherit.len()));
        }
        fn new_battle(&mut self, inherit: &mut Vec<CardInst>, level: u32) -> Battle {
            self.trace.push(format!("battle@{level}/{}张", inherit.len()));
            let mut b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, std::mem::take(inherit), level);
            if let Some(o) = self.force {
                b.over = Some(o);
            }
            b
        }
    }

    fn cfg(level: u32, has_next: bool) -> LevelCfg {
        LevelCfg { head: format!("第 {level} 关"), level, has_next }
    }

    fn pile(n: usize) -> Vec<CardInst> {
        (0..n).map(|i| CardInst::new(i as u64 + 1, faction_cards(Faction::Ember)[i])).collect()
    }

    #[test]
    fn eof_in_battle_quits_with_the_message_eof_in_settle_counts_as_go() {
        // 战斗内 EOF：改前是 `println!("输入结束，退出。")` 然后 `exit(0)`，会话层只把它表达成出口。
        let mut p = Probe::new(&[]);
        let mut inherit = pile(1);
        let mut k = 0;
        match run_level(&mut p, &cfg(1, true), &mut inherit, &mut k) {
            Terminus::Quit { msg } => assert_eq!(msg.as_deref(), Some("输入结束，退出。")),
            t => panic!("战斗内 EOF 该走 Quit，实到 {t:?}"),
        }
        assert!(p.out.ends_with("\n> "), "退出前只打到提示符，不替壳决定输出：{}", p.out);

        // 结算阶段 EOF＝视作 `go`：正常离开，且**照样**落"下一关入口"盘。
        let mut p2 = Probe::new(&[]);
        p2.force = Some(Outcome::PlayerWin);
        let mut inherit2 = pile(2);
        let mut k2 = 0;
        // 准备阶段 EOF 先放行，随后开局即胜 ⇒ 进结算阶段，那里的 EOF 才是本条要测的。
        let out = match run_level(&mut p2, &cfg(3, true), &mut inherit2, &mut k2) {
            Terminus::Done(o) => o,
            Terminus::Quit { .. } => panic!("结算阶段的 EOF 不该退出"),
        };
        assert_eq!(out, Outcome::PlayerWin);
        assert!(p2.saw("next@3/2张"), "EOF 视作 go ⇒ 落盘照做：{}", p2.order());
    }

    #[test]
    fn quit_in_settle_leaves_no_output_and_skips_the_next_entry_snapshot() {
        // 改前这里直接 `process::exit(0)`，一个字节都不输出；且 `q` 回到不了调用方 ⇒ 那次落盘不发生，
        // 档留在本关入口。两条都是行为，必须钉住。
        let mut p = Probe::new(&["go", "q"]);
        let mut inherit = pile(2);
        let mut k = 0;
        p.force = Some(Outcome::PlayerWin);
        match run_level(&mut p, &cfg(2, true), &mut inherit, &mut k) {
            Terminus::Quit { msg } => assert_eq!(msg, None, "结算 `q` 无文案"),
            t => panic!("该走 Quit，实到 {t:?}"),
        }
        assert!(p.saw("entry@2/2张/0业"), "本关入口那次已经写了：{}", p.order());
        assert!(!p.trace.iter().any(|t| t.starts_with("next")), "q 之后不该有下一关入口快照：{}", p.order());
    }

    #[test]
    fn the_entry_snapshot_happens_after_prep_edits_and_before_the_battle_drains_the_pile() {
        // 顺序错了就写不出玩家的准备阶段决策：`new_battle` 一搬空堆，之后再快照＝把 2 张写成 0 张。
        let mut p = Probe::new(&["drop 0", "go"]);
        let mut inherit = pile(2);
        let mut k = 0;
        p.force = Some(Outcome::PlayerWin);
        let _ = run_level(&mut p, &cfg(4, false), &mut inherit, &mut k);
        assert_eq!(p.order(), "entry@4/1张/0业,battle@4/1张", "快照在前、搬空在后，且张数是编辑后的 1：{}", p.order());
        assert!(p.out.contains("弃置 "), "弃置要有可见行（永久消失，静默＝丢档无凭据）：{}", p.out);
    }

    #[test]
    fn has_next_switches_only_the_loss_wording_and_a_loss_never_reaches_the_settle_leg() {
        // 跳打 Boss（has_next=false）没有收尸、没有结算阶段、没有下一关入口快照；
        // 而败局无论有没有下一关都不进结算——通关奖励只发给赢（§十一:402）。
        let mut t = Probe::new(&[]);
        t.force = Some(Outcome::PlayerLose);
        let mut inherit = pile(1);
        let mut k = 0;
        let out = run_level(&mut t, &cfg(2, true), &mut inherit, &mut k);
        assert!(matches!(out, Terminus::Done(Outcome::PlayerLose)), "败局该原样交回调用方");
        assert!(t.out.contains("败：我方烛尽，人亡。阵亡自造牌永久消失。\n"), "{}", t.out);
        assert!(!t.trace.iter().any(|x| x.starts_with("next")), "输了不该有下一关快照：{}", t.order());

        let mut f = Probe::new(&[]);
        f.force = Some(Outcome::PlayerLose);
        let mut inherit2 = pile(1);
        let mut k2 = 0;
        let _ = run_level(&mut f, &cfg(2, false), &mut inherit2, &mut k2);
        assert!(f.out.contains("败：我方烛尽，人亡。本局到此为止。\n"), "单关跳打的口径：{}", f.out);
        assert_eq!(f.order(), "entry@2/1张/0业,battle@2/1张", "has_next=false 时收尸整段该跳过：{}", f.order());

        let mut d = Probe::new(&[]);
        d.force = Some(Outcome::Draw);
        let mut inherit3 = pile(4);
        let mut k3 = 0;
        let _ = run_level(&mut d, &cfg(2, true), &mut inherit3, &mut k3);
        assert!(d.out.contains("平局（30回合蜡烛判定/双烛尽）。\n"), "{}", d.out);
        assert!(!inherit3.is_empty(), "平局也走收尸：幸存者该回填继承堆，否则白死一场");
        assert!(!d.out.contains("结算阶段"), "平局不发通关奖励：{}", d.out);
        assert!(!d.trace.iter().any(|x| x.starts_with("next")), "平局不落下一关快照：{}", d.order());
        assert_eq!(d.seen_karma.map(|v| v.max(0)), Some(k3), "结转业力取战斗结束时的 p_karma（§廿二:967）");
        // 时机断言落在**字串位置**上，不落自证计数：删掉任一接缝都会红，而不是只编译不过。
        assert_eq!(d.count("[棋盘]"), 1, "开局一帧棋盘，别多画");
        assert_eq!(d.count("[尾报]"), 1, "尾报正好好打一次");
        assert!(
            d.nth("[棋盘]", 0) < d.nth("[尾报]", 0) && d.nth("[尾报]", 0) < d.nth("平局", 0),
            "棋盘 → 尾报 → 胜负文案，这个次序就是改前的次序：{}",
            d.out
        );
    }

    #[test]
    fn battle_line_prints_refuses_and_carries_the_quit_message() {
        // `battle_line` 那几条臂此前零执行：h＝Print、未知词＝Refused、q＝Quit。
        // 改前 q 走的是"打印『弃局退出。』再 exit(0)"，那句文案必须还在出口里。
        let mut p = Probe::new(&["go", "h", "zz", "q"]);
        let mut inherit = pile(2);
        let mut k = 0;
        match run_level(&mut p, &cfg(1, true), &mut inherit, &mut k) {
            Terminus::Quit { msg } => assert_eq!(msg.as_deref(), Some("弃局退出。")),
            t => panic!("战斗内 q 该走 Quit，实到 {t:?}"),
        }
        assert!(p.out.contains(crate::command::HELP), "帮助要真打出去了：{}", p.out);
        assert!(p.out.contains("✖ 未知命令：zz（h 看帮助）\n"), "{}", p.out);
        assert_eq!(p.count("[棋盘]"), 3, "q 之前读了三行 ⇒ 三帧棋盘");
        assert_eq!(p.count("[尾报]"), 0, "弃局不进 after_battle，尾报一帧都不该有");
        assert!(p.saw("entry@1/2张/0业"), "本关入口快照在开战前已经写了：{}", p.order());
        assert!(!p.trace.iter().any(|x| x.starts_with("next")), "弃局不落下一关快照：{}", p.order());
    }

    #[test]
    fn quit_in_the_prep_phase_writes_not_even_the_entry_snapshot() {
        // 准备阶段在**本关入口快照之前**：在这里 q，档上一个字节都不动，下次续上的还是这一关的入口。
        // 与结算阶段的 q 同一条出口、同样无文案——但少写一次盘，这条差别只有单独用例才看得见。
        let mut p = Probe::new(&["q"]);
        let mut inherit = pile(2);
        let mut k = 0;
        match run_level(&mut p, &cfg(7, true), &mut inherit, &mut k) {
            Terminus::Quit { msg } => assert_eq!(msg, None, "准备阶段 q 也无文案"),
            t => panic!("该走 Quit，实到 {t:?}"),
        }
        assert!(p.trace.is_empty(), "没快照、没开战：{:?}", p.trace);
        assert_eq!(p.count("[棋盘]"), 0, "还没开战就退出，一帧棋盘都不该有");
    }

    #[test]
    fn the_session_layer_never_prints_reads_or_exits() {
        // 与 `command.rs` 同一条机检思路：摘壳的前提是这一层真的一个字都不往 stdout 写、不读 stdin、不 exit。
        let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/session.rs")).unwrap();
        let banned = [
            "println!",
            "print!(",
            "eprintln!",
            "eprint!(",
            "write!",
            "writeln!",
            "std::io",
            "io::stdin",
            "stdout()",
            "File::",
            "process::exit",
            "render::",
        ];
        let mut offenders: Vec<String> = Vec::new();
        for (i, l) in src.lines().enumerate() {
            let t = l.trim();
            // 清单自身那几行也得跳过——多行字面量里每个 `"println!",` 都会自指地命中自己。
            if t.starts_with("//") || t.starts_with('"') || t.contains("mod tests") || t.contains("let banned") {
                continue;
            }
            if banned.iter().any(|x| t.contains(x)) {
                offenders.push(format!("{}: {t}", i + 1));
            }
        }
        assert!(offenders.is_empty(), "session.rs 的非注释行里出现了输出/退出/stdin/render：{offenders:?}");
    }
}
