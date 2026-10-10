// AGPL v3 头部（与 src/*.rs 同一份；署名与许可见 版权 行）
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
//! CLI 参数面的端到端尺：`一个旗标要么被这条命令读，要么当场拒；给了值却读不懂 ⇒ rc=2`。
//!
//! 为什么开 `tests/` 这个目录：`src/main.rs` 从前**一个字节的测都没有**，而它是这条口径唯一的执行处——
//! `bad_opt`／`gate_*` 全都直接 `process::exit`，在单元测试里断不到。零依赖立场不破：只用 std。
//!
//! 三条硬规矩：
//! ① **绝不碰默认档位**。`~/.local/share/midline/` 那份是人的存档，任何一条测都显式 `--save` 到临时目录，
//!    或根本不带 `--save`（那些命令不落盘）。
//! ② stdin 一律 `null`：EOF 在准备阶段视作 `go`，对局命令会自己走完，不需要喂脚本。
//! ③ 每条测跑**两次对撞**的不做（慢），但断言 rc **与**首行文案——只断 rc 会放过"报错但报错内容漂移"。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_midline")
}

/// 一次调用的读数：退出码 + 合在一起的 stdout/stderr。
struct Run {
    rc: i32,
    out: String,
}

impl Run {
    /// 首行（报错文案的断言面）。空输出时返回空串，让 `starts_with` 自己判失败。
    fn first(&self) -> &str {
        self.out.lines().next().unwrap_or("")
    }
}

fn run(args: &[&str]) -> Run {
    let p = Command::new(bin())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("起不来二进制");
    let mut out = String::from_utf8_lossy(&p.stdout).to_string();
    out.push_str(&String::from_utf8_lossy(&p.stderr));
    Run { rc: p.status.code().unwrap_or(-1), out }
}

/// 每条测自己的临时档目录。名字由调用点给（**不用时钟、不用随机数**：那会让失败复现不了），
/// 末尾再拼**进程号**——电池在 `mktemp` 出来的副本里跑同一套测，两个 `cargo test` 并发时
/// 同名沙盒会互相 `remove_dir_all`，把对方的读数打成假红/假绿（这条是第二轮审查实测到的形状）。
fn sandbox(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("midline-cli-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("建不出临时目录");
    root
}

/// 断言"这条命令根本没碰那个文件"：内容一字不动，且没有 `.bad-N` 留证文件被造出来。
fn assert_untouched(path: &Path) {
    let dir = path.parent().expect("沙盒里该有父目录");
    let junk: Vec<String> = std::fs::read_dir(dir)
        .expect("读不出沙盒目录")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".bad-"))
        .collect();
    assert!(junk.is_empty(), "沙盒里冒出留证文件 {junk:?} —— 这条命令本不该动盘");
    assert!(path.exists(), "{path:?} 该还在原地（被改名或删掉就是动了盘）");
}

// ---------------------------------------------------------------- 读不懂 ⇒ rc=2

#[test]
fn difficulty_unknown_value_is_rejected_not_defaulted() {
    let r = run(&["play", "--difficulty", "turbo"]);
    assert_eq!(r.rc, 2, "读不懂的难度值不该静默换成普通：输出＝{}", r.out);
    assert!(r.first().contains("读不懂"), "文案该说清是读不懂，收到首行＝{}", r.first());
    assert!(r.first().contains("easy|normal|hard|expert"), "文案要抄上可选值：{}", r.first());
}

#[test]
fn difficulty_missing_value_is_rejected() {
    let r = run(&["play", "--difficulty"]);
    assert_eq!(r.rc, 2, "--difficulty 缺值不该按不给办：{}", r.out);
}

#[test]
fn difficulty_absent_still_defaults_to_normal() {
    // 不给仍然是普通局，rc=0——这一条钉住"闸只拦读不懂的，不拦没给的"。
    let r = run(&["auto", "0", "--difficulty", "expert"]);
    assert_eq!(r.rc, 0, "合法难度值不该被拦：{}", r.out);
    let r = run(&["auto", "0"]);
    assert_eq!(r.rc, 0, "不给难度该照常跑（零局）：{}", r.out);
}

#[test]
fn auto_rounds_unknown_is_rejected() {
    let r = run(&["auto", "3x"]);
    assert_eq!(r.rc, 2, "`auto 3x` 从前静默按 1 局跑完：{}", r.out);
    assert!(r.first().contains("轮数"), "要指名是轮数的问题：{}", r.first());
}

#[test]
fn auto_zero_rounds_is_zero_battles_not_one() {
    let r = run(&["auto", "0"]);
    assert_eq!(r.rc, 0, "`auto 0` 是合法读数，不该报错：{}", r.out);
    assert!(r.out.is_empty(), "`auto 0` 该零场，输出＝{:?}", r.out);
}

#[test]
fn seed_and_faction_unknown_values_rejected() {
    for (args, want) in [
        (vec!["play", "--seed", "4x"], "非负整数"),
        (vec!["play", "--faction", "9"], "1|2|3"),
        (vec!["play", "--faction"], "1|2|3"),
    ] {
        let r = run(&args);
        assert_eq!(r.rc, 2, "{args:?} 该被拒：{}", r.out);
        assert!(r.first().contains(want), "文案该抄上可选值 {want}：{}", r.first());
    }
}

// ---------------------------------------------------------------- 作用域：不读就拒

#[test]
fn flags_out_of_scope_are_rejected_per_command() {
    for (cmd, flag) in [
        ("daily", "--seed"),
        ("daily", "--faction"),
        ("daily", "--difficulty"),
        ("progress", "--seed"),
        ("mainline", "--boss"),
        ("play", "--save"),
        ("auto", "--tui"),
        ("play", "--out"),
        ("boss", "--save"),
    ] {
        let r = run(&[cmd, flag, "1"]);
        assert_eq!(r.rc, 2, "「{cmd}」不读 {flag}，该当场拒：{}", r.out);
        assert!(
            r.first().contains(&format!("{flag} 不用于")),
            "文案要点名旗标与命令：{cmd} {flag} → {}",
            r.first()
        );
    }
}

#[test]
fn in_scope_flags_are_accepted() {
    // 闸误拒合法调用比漏拒更坏：这几条是 READS 名单的正反面里的"反面"。
    // 落盘那两条显式指到本测自己的沙盒——默认档位是人的存档，测不许碰。
    let dir = sandbox("in-scope");
    let save = dir.join("m.kv");
    let save_s = save.to_str().unwrap();
    for args in [
        vec!["play", "--seed", "7"],
        vec!["auto", "0", "--seed", "7", "--faction", "2"],
        vec!["mainline", "--save", save_s, "--resume"],
        vec!["progress", "--save", save_s],
        vec!["daily", "--tui"],
    ] {
        let r = run(&args);
        assert!(!r.first().contains("不用于"), "{args:?} 被作用域闸误拒了：{}", r.first());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- 位置参数：多了就拒

#[test]
fn extra_positionals_rejected() {
    for args in [
        vec!["play", "hello"],
        vec!["auto", "1", "extra"],
        vec!["boss", "3", "extra"],
        vec!["mainline", "extra"],
        vec!["frames", "extra"],
    ] {
        let r = run(&args);
        assert_eq!(r.rc, 2, "多余位置参数不该被忽略：{}", r.out);
        assert!(r.first().contains("最多接"), "文案该说清接几个：{}", r.first());
    }
}

#[test]
fn progress_recover_with_extra_arg_does_not_touch_the_save() {
    // 这一格从前不是"显示失真"而是**副作用**：`progress recover extra` 把 recover 办了，
    // 于是人的档被改名成 .bad-N。现在它该在动盘之前就被拒。
    let dir = sandbox("recover-extra");
    let save = dir.join("progress.kv");
    let before = "schema=4\nfaction=1\ndifficulty=normal\nlevel=1\ncarry_karma=0\ncompleted=0\ndaily_done=0\ndaily_attempts=0\ndaily_draw_state=0\ndaily_rule_last=1\ndaily_rule_run=0\ntokens=0\n";
    std::fs::write(&save, before).expect("造不出夹具档");
    let r = run(&["progress", "recover", "extra", "--save", save.to_str().unwrap()]);
    assert_eq!(r.rc, 2, "`recover` 后面多一个词不该被按 recover 办：{}", r.out);
    assert_untouched(&save);
    assert_eq!(std::fs::read_to_string(&save).expect("读不回夹具档"), before, "档内容被动过");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- 单破折号与 help

#[test]
fn single_dash_unknown_option_rejected() {
    let r = run(&["play", "-x"]);
    assert_eq!(r.rc, 2, "`play -x` 从前静默开打一局：{}", r.out);
    assert!(r.first().contains("未知选项"), "文案要说是未知选项：{}", r.first());
}

#[test]
fn help_works_anywhere() {
    for args in [vec!["help"], vec!["-h"], vec!["--help"], vec!["play", "-h"], vec!["auto", "1", "--help"]] {
        let r = run(&args);
        assert_eq!(r.rc, 0, "{args:?} 该印帮助并 0 退出：{}", r.out);
        assert!(r.out.contains("《中线》核心规则 CLI"), "{args:?} 没印出帮助首行");
    }
}

// ---------------------------------------------------------------- auto 接线：被读要能被看见

#[test]
fn auto_reads_seed() {
    let r = run(&["auto", "1", "--seed", "7"]);
    assert_eq!(r.rc, 0, "{}", r.out);
    assert!(r.out.contains("seed=7"), "`auto --seed 7` 的头报该印 7，实得：{}", r.out);

    // 不给 ⇒ 夹具自己的默认（这条是"接线与 HEAD 逐字节相同"的反证面）。
    let d = run(&["auto", "1"]);
    assert!(d.out.contains("seed=1000"), "不给 --seed 该按 1000 起，实得：{}", d.out);
}

#[test]
fn auto_reads_faction() {
    // 300 种子扫下命中 2 个可观察差异，seed=22 是其一：烬火止于第1关、霜誓到第2关。
    // 换 seed 就量不到了——那是夹具的性质，不是这条测的毛病。
    let ember = run(&["auto", "1", "--seed", "22"]);
    let frost = run(&["auto", "1", "--seed", "22", "--faction", "2"]);
    assert_eq!(ember.rc, 0);
    assert_eq!(frost.rc, 0);
    assert_ne!(ember.out, frost.out, "--faction 在 auto 的读数面上不可观察：{}", ember.out);
    assert!(ember.out.contains("到达第1关"), "烬火这局的读数变了：{}", ember.out);
    assert!(frost.out.contains("到达第2关"), "霜誓这局的读数变了：{}", frost.out);
}

#[test]
fn repeated_flag_last_wins() {
    // `--seed` 与其余带值旗标现在走同一个 helper，重复给一律最后一个赢。
    let r = run(&["auto", "1", "--seed", "1", "--seed", "2"]);
    assert_eq!(r.rc, 0, "{}", r.out);
    assert!(r.out.contains("seed=2"), "重复给该最后一个赢，实得：{}", r.out);
}

// ---------------------------------------------------------------- 无命令词那条路

#[test]
fn unknown_command_still_reports_itself() {
    // 作用域闸排在值解析之前，就必须让开未知命令：`bogus --seed 1` 的真问题是命令词打错了，
    // 不是"bogus 不读 --seed"。`--seed abc` 那一格是本帧自己造出来的回归：值闸比 dispatch 先开口，
    // 「--seed 的值读不懂」会抢走「未知命令」——所以闸不是"让开"就够，得当场把命令词拒掉。
    for args in [
        vec!["bogus"],
        vec!["bogus", "--seed", "1"],
        vec!["bogus", "--seed", "abc"],
        vec!["bogus", "--tui"],
        vec!["bogus", "extra", "more"],
    ] {
        let r = run(&args);
        assert_eq!(r.rc, 2, "{args:?} 该非零退出：{}", r.out);
        assert!(r.first().contains("未知命令"), "{args:?} 没报出未知命令：{}", r.first());
    }
}

#[test]
fn help_flag_in_a_value_slot_stays_a_value() {
    // 帮助扫描从前逐 token 命中、不跳值 ⇒ `progress --save -h` 印完帮助 rc=0，一个字节不做事：
    // 用户以为自己在读一个叫 `-h` 的档。同一条"带值旗标的值不参与判断"的规则必须三处共读（见 `option_tokens`）。
    let dir = sandbox("help-in-value");
    let r = run(&["progress", "--save", &dir.join("-h").to_string_lossy()]);
    assert!(!r.out.contains("《中线》核心规则 CLI"), "把 -h 当帮助旗标吞掉了：{}", r.out);
    assert_eq!(r.rc, 0, "progress 读一个不存在的档该照旧报路径：{}", r.out);
    let d = run(&["auto", "--seed", "-h"]);
    assert!(!d.out.contains("《中线》核心规则 CLI"), "把 -h 当帮助旗标吞掉了：{}", d.out);
    assert_eq!(d.rc, 2, "-h 不是合法种子值：{}", d.out);
    assert!(d.first().contains("读不懂"), "该报「--seed 的值读不懂」，实得：{}", d.first());
    // 反过来：真在选项位的 -h 仍然有效，别把闸做成"值位置的判断顺手吃掉选项位"。
    let h = run(&["play", "-h"]);
    assert_eq!(h.rc, 0, "play -h 该印帮助：{}", h.out);
    assert!(h.out.contains("《中线》核心规则 CLI"), "play -h 没印帮助：{}", h.out);
}

#[test]
fn bare_flags_are_play_and_do_not_leak_their_own_value() {
    // 从前 `positionals` 无条件 skip(1)，于是 `midline --difficulty expert` 把值 `expert` 漏成位置参数。
    // 装上 arity 闸后，那条泄漏会**误拒合法调用**——这一条钉住泄漏已修。
    let r = run(&["--seed", "42"]);
    assert!(!r.first().contains("最多接"), "--seed 的值被漏成位置参数了：{}", r.first());
    let d = run(&["--difficulty", "expert"]);
    assert!(!d.first().contains("最多接"), "--difficulty 的值被漏成位置参数了：{}", d.first());
}
