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
mod canvas;
mod command;
mod md3;
mod meta;
mod model;
mod progress;
mod render;
mod rng;
mod save;
mod session;
mod tui;
mod view;

use battle::Difficulty;
use boss::BossId;
use model::Faction;
use std::path::PathBuf;

const USAGE: &str = "\
《中线》核心规则 CLI
用法：
  play [--seed S] [--faction 1|2|3] [--difficulty X]        普通爬关（阵营三循环，不落盘）
  play --boss <id> [--seed S] [--faction 1|2|3]             跳打某章末 Boss（同 boss <id>，不落盘）
  mainline [--seed S] [--faction 1|2|3] [--difficulty X]     主线 60 关（5 章，章末 Boss，自动存档）
  mainline --resume [--save F] [--faction 1|2|3]            从存档的「本关入口」续打；不给 --resume 则从第1关重开（档上已有进度会先警告再覆盖）
  boss <id> [--seed S] [--faction 1|2|3]                    单关跳打 Boss
  daily [--save F]                                          每日挑战（本地日期为种子；一天 3 次，一次一场 30 回合生存战，每场抽一条当日特殊规则；只有通关扣额度，不动主线进度）
  progress [--save F]                                       看当前存档（只读：不写、不修、不猜；档读不懂则 rc=2）
  progress recover [--save F]                               把当前档改名留证 .bad-N，下次从空档开始
  auto [轮数] [--seed S] [--faction 1|2|3] [--difficulty X]  AI 托管模拟若干局（轮数默认 1，不给种子默认 1000 起·每轮 +1，不落盘）
  auto [轮数] --boss all|<id> [--seed S] [--faction 1|2|3]  Boss 脚本冒烟：N 轮 × 5 个（或指定）
  frames --out DIR [--seed S] [--faction 1|2|3] [--difficulty X] [--boss <id>]
                                                          2D 离线帧：托管打一关，每次写入一张 SVG 到 DIR（序号＝写入次序，不是回合数；--out 必给；不落存档、不动 --save 那条面）
  help  /  -h  /  --help                                    本帮助（-h、--help 在选项位都有效；值位上它仍是值，不当帮助）
<id> ＝ 1-5 | luzhu|雪爵… 见下：1 炉主 / 2 雪爵 / 3 影长 / 4 炎与冰 / 5 终影
（--seed / --faction 不给＝种子7·烬火；只有不带 --boss 的 auto 用它自己的夹具默认（1000 起·每轮 +1·烬火），auto --boss 那一支照旧按种子7·烬火）
（参数面三条口径，任何一条违反都直接 rc=2，不存在「警告一句再回退」：
 ① 给了值却读不懂 ⇒ 拒；② 这条命令不读的旗标 ⇒ 拒（谁读它，报错里会列出来）；③ 多余的位置参数 ⇒ 拒。
 其中 --difficulty 有三个消费者：敌方 AI 的搜索档、我方托管的融合/升级决策、以及档面（mainline 落盘 difficulty、progress 回显）；
 Boss 战里它实际不改变任何行为——敌方是脚本、我方交互局是人类、auto --boss 的托管走贪心不读难度档（裁定21）。
 所以 boss／play --boss／frames --boss 仍然接受这个旗标：不是没读它，是读了也不换东西，那句是裁定21 的设计而不是失真。）
存档（只挂在 mainline / daily 上；落点＝--save ＞ $MIDLINE_SAVE ＞ $XDG_DATA_HOME/midline/ ＞ $HOME/.local/share/midline/）：
  --save <文件>   换档的位置（缺值直接报错，不静默落回默认位置）
  --resume        读档续关；档损坏一律拒绝并指向 progress recover，不会当成空档静默重开
  存档点＝关隘入口（裁定27）：败/平/弃局都不动盘，下次续上的就是没过关的那一关
战斗内命令：
  p <手牌idx> <P1-P4> 放置 | s <P1-P4> 场上献祭 | sh <手牌idx> 手牌献祭
  di|ds 抽继承堆/开端堆 | e 结束回合 | l 日志 | b Boss档案 | q 退出
结算/准备阶段：fuse <主> <副> / up <idx> power|thr|skill / drop <idx> / move <从> <到> / go
交互界面：
  --tui               TUI 清屏重绘 + 行输入（对局命令全程可用：play / mainline / daily / boss；auto、progress 不适用）";

/// 吃一个值的长选项（其值不算位置参数）。
const VALUE_FLAGS: &[&str] = &["--seed", "--faction", "--difficulty", "--boss", "--save", "--out"];

/// 不带值的长选项（出现即为真）。它必须单列一张表：漏了它，`--resume` 先被 `reject_unknown_flags` 按未知选项拒掉；
/// 而紧跟其后的那个词也不会被按值跳过（`option_tokens` 只跳 `VALUE_FLAGS` 那六条的值），于是它会被 `positionals`
/// 数成多余位置参数。**一处漏表、两种错文案**，所以这张表与 `VALUE_FLAGS` 合起来才是"什么算选项"的全集——
/// 从前它只被 `gate_tui` 那半段读，写这句注释时两条支路的后果确实互不相干；装上 arity 闸之后不再是了。
const BOOL_FLAGS: &[&str] = &["--resume", "--tui"];

/// 全局旗标：出现在**选项位**上就印帮助、rc=0；带值旗标的**值位**上它仍是那个值（判据在 `option_tokens`）。
/// 它不进 `READS`（作用域表），因为它的"作用域"就是全部 argv，
/// 包括那条命令本身不认识的情况。从前 `-h` 只在首词有效，而 `reject_unknown_flags` 又把它排除在闸外——
/// 于是 `play -h` 既不报错、也不印帮助，直接开打一局。
const HELP_FLAGS: &[&str] = &["-h", "--help"];

/// 命令 × 这条命令接哪些旗标。原先是 `gate_save_flags`／`gate_tui` 两道闸各写各的名单，
/// 加一道旗标就得记住改几处——表收成一处，新增闸与既有闸才不可能漂开。
///
/// 表里写的是**接进这条命令的路径**，不等于"必有可观察后果"：`--difficulty` 在 Boss 局不改变任何行为，
/// 那是 `裁定21` 正文（`model.rs:99-101`）自己写明的设计，且 `frames [--difficulty X] [--boss <id>]`
/// 一行里两种局都在 ⇒ 这里不拒那个组合，改由 `USAGE` 那句摘要对齐正文来负责说实话。
const READS: &[(&str, &[&str])] = &[
    ("--seed", &["auto", "play", "mainline", "boss", "frames"]),
    ("--faction", &["auto", "play", "mainline", "boss", "frames"]),
    ("--difficulty", &["auto", "play", "mainline", "boss", "frames"]),
    ("--boss", &["auto", "play", "frames"]),
    ("--save", &["mainline", "daily", "progress"]),
    ("--resume", &["mainline"]),
    ("--tui", &["play", "mainline", "daily", "boss"]),
    ("--out", &["frames"]),
];

/// 认得的命令词。它是**未知命令那条真值的唯一来源**：`main()` 在两道闸之前先按它拒一次，
/// 于是 `bogus --seed 1` 听到的是"未知命令：bogus"，而不是"--seed 不用于「bogus」"或"--seed 的值读不懂"——
/// 后两句都把真问题（命令打错了）说成了假问题。表与 `main()` 里那个 match 的分支一一对上（对不上时 `unreachable!` 当场暴露）。
const CMDS: &[&str] = &["auto", "boss", "daily", "frames", "help", "mainline", "play", "progress"];

/// 编译期用的字符串相等。`==` 在 const 里是 trait 调用、还不许（rustc 1.97 报 E0658），
/// 而这份检查要的就是"有人漏填表时编译不过"，所以它不能是运行时断言。
const fn same(a: &str, b: &str) -> bool {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    if x.len() != y.len() {
        return false;
    }
    let mut i = 0;
    while i < x.len() {
        if x[i] != y[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 两张表的**编译期**不变量。这两格不是整洁癖，是本帧口径自己的两条前提：
/// ① 表里出现的旗标名必须同时被"带值/布尔"那两张表认得——否则它既不进 `option_tokens` 的跳值规则、
///    也不被任何解析口读，新加一条旗标只写进 `READS` 就正好造出口径要杀的那个形状（人人可给、无人读）。
/// ② 每行的命令名单不得为空——空名单会印出「它只被  读」这种半句话，而它下面那个 `exit(2)` 照跑。
/// 写在 const 里＝编译期就炸，不占一次测试运行，也不依赖有人去跑。
const _: () = {
    let mut i = 0;
    while i < READS.len() {
        let (name, ok) = READS[i];
        assert!(!ok.is_empty(), "READS 里有一行的命令名单是空的");
        let mut in_table = false;
        let mut j = 0;
        while j < VALUE_FLAGS.len() {
            if same(VALUE_FLAGS[j], name) {
                in_table = true;
            }
            j += 1;
        }
        let mut k = 0;
        while k < BOOL_FLAGS.len() {
            if same(BOOL_FLAGS[k], name) {
                in_table = true;
            }
            k += 1;
        }
        assert!(in_table, "READS 里的旗标不在 VALUE_FLAGS／BOOL_FLAGS：它没人解析，闸却会放它过去");
        i += 1;
    }
};

/// 命令 × 这条命令吃几个位置参数。表里没有的命令＝0 个。
/// 依据是 §2 那格 `progress recover extra`：多余参数今天被按"recover"办，**真把档改了名**。
const ARITY: &[(&str, usize)] = &[("auto", 1), ("boss", 1), ("progress", 1)];

/// 难度档的可选值，抄进每一条"读不懂"的报错里。解释口只有 `Difficulty::parse` 一个，
/// 但可选值在报错文案里出现过两处（旧 `parse_difficulty` 与 USAGE 各写一遍）就会漂 ⇒ 收成一处。
const DIFFICULTIES: &str = "easy|normal|hard|expert";

/// argv 里**参与"这是不是一个选项"判断**的 token：跳过命令词，也跳过带值旗标的值。
/// 这条跳值规则从前有三个读手——`reject_unknown_flags` 一份、`positionals` 一份、帮助扫描**一份都没写**——
/// 于是 `progress --save -h` 里那个 `-h` 既是 `--save` 的值、又被当成帮助旗标：rc=0 印完帮助、一个字节不做事，
/// 正是本帧口径要杀的「你给了，程序接了，但没人按你给的意思办」那一格。
/// 现在它是这三处唯一的判据：帮助扫描与 `reject_unknown_flags` 直接用它，`positionals` 由它派生。
fn option_tokens(args: &[String]) -> Vec<&String> {
    let mut out = Vec::new();
    let mut it = args.iter();
    // 首词只有在不是选项时才是命令词——与 `positionals` 同一条判据，两处不许各写一份。
    if args.first().is_some_and(|a| !a.starts_with('-')) {
        it.next();
    }
    while let Some(a) = it.next() {
        if VALUE_FLAGS.contains(&a.as_str()) {
            it.next();
            continue;
        }
        out.push(a);
    }
    out
}

/// 未知选项一律当场拒。两个洞一起补：
/// ① 从前只测 `--` 前缀 ⇒ `play -x` 静默开打一局；② 带值旗标的**值**不参与判断，
/// 所以 `--out -tmp` 里那个 `-tmp` 仍是目录名、不会被误判成选项（判断面走的是 `option_tokens`）。
fn reject_unknown_flags(args: &[String]) {
    for a in option_tokens(args) {
        if a.starts_with('-') && !BOOL_FLAGS.contains(&a.as_str()) && !HELP_FLAGS.contains(&a.as_str()) {
            println!(
                "✖ 未知选项：{a}（可用 {} / {}）",
                VALUE_FLAGS.join(" / "),
                BOOL_FLAGS.join(" / ")
            );
            std::process::exit(2);
        }
    }
}

/// 位置参数：`option_tokens` 里那些不以破折号开头的 token。
/// 它从前自己再写一遍"跳命令词＋跳带值旗标的值"——两条判据与 `option_tokens` 逐字等价
/// （`VALUE_FLAGS` 里每一条都以 `--` 开头，所以那句 `starts_with("--")` 外壳是空守卫），
/// 于是同一件事有两处可以各自漂开。收成一处派生后，`positionals` 与闸只剩**读法**不同：
/// 闸要看所有非值 token（不认得的得拒），它只看不是选项的那一族（多出来的得拒）。
/// 首词只有在**不是选项**时才算命令词——旧写法无条件 `skip(1)`，于是 `midline --difficulty expert`
/// 这种"无命令词"的合法调用会把它自己的值 `expert` 漏成位置参数（今天没人读它所以看不出来；
/// 一旦装上"多余位置参数就拒"的闸，这条泄漏就会**误拒合法调用**，所以闸之前先修这里）。
fn positionals(args: &[String]) -> Vec<String> {
    option_tokens(args).into_iter().filter(|a| !a.starts_with('-')).cloned().collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("play");
    // `help` 仍是命令词；`-h`／`--help` 是全局旗标，在选项位上出现都印帮助——但**值位不算**（判据在 `option_tokens`），
    // 否则 `progress --save -h` 会把帮助打印出来、一个字节不做事，而用户以为他在读一个叫 `-h` 的档。
    if cmd == "help" || option_tokens(&args).iter().any(|a| HELP_FLAGS.contains(&a.as_str())) {
        println!("{USAGE}");
        return;
    }
    reject_unknown_flags(&args);
    // 无命令词、直接给标志（`midline --seed 42`）＝ play，旧版即如此，不当未知命令。
    let cmd = if cmd.starts_with('-') { "play" } else { cmd };
    // **未知命令先说完，再动任何值闸**。这一格是本帧自己造出来的回归：作用域与 arity 两道闸排在值解析之前，
    // 而它们对不认识的命令词直接 `return`；如果不在这里也拦一道，`bogus --seed abc` 就会先听到「--seed 的值读不懂」——
    // 拿"值"抢走"命令打错了"那句真话，正是本帧在别处逐条杀掉的那种失真。CMDS 与 match 的分支必须一一对上。
    if !CMDS.contains(&cmd) {
        println!("✖ 未知命令：{cmd}");
        println!("{USAGE}");
        std::process::exit(2);
    }
    // **先问作用域，再问值读不读得懂**：`daily --difficulty 1` 该听到的是"daily 不读 --difficulty"，
    // 而不是"1 读不懂"——后者暗示换成 `--difficulty easy` 就行，而它对 daily 一样什么都不做。
    // 顺序反过来时，越界的旗标会先按值报错，把"这条命令根本不接它"这层信息吃掉。
    gate_flag_scope(cmd, &args);
    gate_arity(cmd, &args);
    let (seed_given, faction_given) = parse_opts(&args);
    // 对局那一族的默认＝种子 7·烬火（`USAGE` 里写死的那两个数）；`auto` 用它自己的夹具默认，见 `meta::auto_battles`。
    let seed = seed_given.unwrap_or(7);
    let faction = faction_given.unwrap_or(Faction::Ember);
    let diff = parse_difficulty(&args);
    // 画帧档：Text 是默认，`--tui` 只切这一档，其余（遭遇/落盘/阶段机）两条路共用同一台 host。
    let frame = if has_flag(&args, "--tui") { meta::Frame::Tui } else { meta::Frame::Text };
    match cmd {
        "auto" => {
            let n = auto_rounds(&args);
            match value_flag(&args, "--boss") {
                Flag::Absent => meta::auto_battles(seed_given, faction_given, n, diff),
                Flag::Value(raw) => auto_bosses(seed, faction, n, raw),
                Flag::Missing => bad_boss_arg(),
            }
        }
        "daily" => meta::daily_run(save_path(&args), frame),
        "frames" => meta::frames_dump(seed, faction, diff, frames_boss(&args), out_dir(&args)),
        "mainline" => meta::mainline_run(seed, faction, diff, save_path(&args), has_flag(&args, "--resume"), frame),
        "play" => match value_flag(&args, "--boss") {
            Flag::Absent => meta::play_run(seed, faction, diff, frame),
            Flag::Value(raw) => meta::boss_run(seed, faction, diff, require_boss(raw), frame),
            Flag::Missing => bad_boss_arg(),
        },
        "boss" => match positionals(&args).first() {
            Some(raw) => meta::boss_run(seed, faction, diff, require_boss(raw), frame),
            None => {
                println!("✖ boss 需要一个参数：1-5 / luzhu xuejue yingzhang yanbing zhongying / 炉主 雪爵 影长 炎与冰 终影");
                std::process::exit(2);
            }
        },
        "progress" => progress_cmd(&args),
        // 走到这里只可能是 `CMDS` 与这些分支脱钩（加了命令词却没加分支），那是改表的编程错、不是用户输入。
        // 这里不再印第二句「未知命令」——上面那道闸是它唯一一处，两处同话就是本帧要消的那一族病。
        other => unreachable!("命令词「{other}」在 CMDS 里却没有分支（CMDS 与 match 脱钩＝改表的编程错）"),
    }
}

/// 作用域闸：旗标不在 `READS` 给这条命令的名单里 ⇒ 当场拒，报出它到底被谁读。
/// "以为存了、其实没存"、"以为开了 TUI、其实没开"、"以为换了阵营、其实没换"是同一种失真，
/// 用一处表挡住比三道闸各挡一段可靠。
fn gate_flag_scope(cmd: &str, args: &[String]) {
    for (flag, ok) in READS {
        if !ok.contains(&cmd) && has_flag(args, flag) {
            println!("✖ {flag} 不用于「{cmd}」：它只被 {} 读。", ok.join(" / "));
            std::process::exit(2);
        }
    }
}

/// 位置参数超量 ⇒ 当场拒。多余的参数不猜、不忽略：`progress recover extra` 那一格今天会把
/// `recover` 之外的词连同 `recover` 一起办，那不是显示失真，是**动了盘**。
fn gate_arity(cmd: &str, args: &[String]) {
    let want = ARITY.iter().find(|(c, _)| *c == cmd).map_or(0, |(_, n)| *n);
    let got = positionals(args).len();
    if got > want {
        println!("✖ 「{cmd}」最多接 {want} 个参数（收到 {got} 个）：多余的参数不猜、不忽略。");
        std::process::exit(2);
    }
}

/// 帧文件的落点，`--out` 的唯一解释口。`frames` **不给就报错**——没有"当前目录"这种默认落点可猜，
/// 一次倒几十个文件到别人正在干活的地方，比一条错误难查得多。
/// （原先这里还有一道 `gate_out` 管"别的命令不许带 `--out`"，那半句已进 `READS`；
/// 而"不给就报错"这半句与本函数第一支完全同话 ⇒ 两处同话就是第二套真值，闸整体删掉。）
fn out_dir(args: &[String]) -> PathBuf {
    match value_flag(args, "--out") {
        Flag::Absent => bad_out(None),
        Flag::Missing => bad_out(Some("值空了")),
        Flag::Value(v) if v.trim().is_empty() => bad_out(Some("值是空白")),
        Flag::Value(v) => PathBuf::from(v),
    }
}

fn bad_out(why: Option<&str>) -> ! {
    println!("✖ frames 需要一个 --out <目录>{}：帧文件不往当前目录倒，落点由你给。", why.map(|w| format!("（{w}）")).unwrap_or_default());
    std::process::exit(2);
}

/// `frames` 的 `--boss` 是**可选**的（不给＝打一关普通爬关），与 `play --boss` 那个"给了就必须认"同一口径。
fn frames_boss(args: &[String]) -> Option<BossId> {
    match value_flag(args, "--boss") {
        Flag::Absent => None,
        Flag::Value(raw) => Some(require_boss(raw)),
        Flag::Missing => bad_boss_arg(),
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

/// 带值长选项的**唯一**读法：没给 / 给了值 / 给了但缺值。缺值必须报错，不能静默当成没给。
/// 重复给同一个旗标＝**最后一个赢**（`rposition`）。从前这里按第一个赢，而 `parse_opts` 那个循环按最后一个赢——
/// 两套规则并存时，`--save a --save b` 与 `--seed 1 --seed 2` 的赢家不同，而没有任何一处文档说过这件事。
fn value_flag<'a>(args: &'a [String], name: &str) -> Flag<'a> {
    match args.iter().rposition(|a| a == name) {
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

/// 标志给值却没给对：一律 `exit 2`，不替人改写命令。两面都要有读数——
/// 给了但读不懂（`Some(值)`）、给了旗标却没给值（`None`）。
fn bad_opt(got: Option<&str>, flag: &str, want: &str) -> ! {
    match got {
        None => println!("✖ {flag} 需要一个值（{want}）"),
        Some(v) => println!("✖ {flag} 的值「{v}」读不懂，要 {want}"),
    }
    std::process::exit(2);
}

/// `--seed N` / `--faction 1|2|3` 的读数。**不给** ⇒ `None`；**给了但读不懂** ⇒ 报错退出 rc=2。
/// 静默回退的代价：`--seed 4x` 跑出来的那局根本不是你要复现的那局，而输出里没有任何地方说命令被改写过；
/// `--faction 9` 更糟——回退到烬火是换掉整张卡表，头报却照样写着"烬火教团"，看着就像你打的就是那个阵营。
/// 与 `--save` 同一口径（见 `save_path`）。走的是与其余带值旗标**同一个** `value_flag`（last-wins 因此只有一套规则）。
///
/// 默认值**不在这里**：`play` 那一族默认「种子 7·烬火」，`auto` 那一族默认「1000 起、每轮 +1」。
/// 两个是各自命令的口径，谁也不许拿对方的默认当自己的——从前 `auto` 就是不看这里，
/// 于是 `auto --seed 7` 的头报印 `seed=1000`：一行自称"可复现"的输出，复现的是别人那一局。
fn parse_opts(args: &[String]) -> (Option<u64>, Option<Faction>) {
    let seed = match value_flag(args, "--seed") {
        Flag::Absent => None,
        Flag::Missing => bad_opt(None, "--seed", "非负整数"),
        Flag::Value(raw) => match raw.parse::<u64>() {
            Ok(v) => Some(v),
            Err(_) => bad_opt(Some(raw), "--seed", "非负整数"),
        },
    };
    let faction = match value_flag(args, "--faction") {
        Flag::Absent => None,
        Flag::Missing => bad_opt(None, "--faction", "1|2|3"),
        Flag::Value(raw) => match raw {
            "1" => Some(Faction::Ember),
            "2" => Some(Faction::Frost),
            "3" => Some(Faction::Shadow),
            other => bad_opt(Some(other), "--faction", "1|2|3"),
        },
    };
    (seed, faction)
}

/// `--difficulty easy|normal|hard|expert`；**不给** → 普通；**给了却读不懂 ⇒ rc=2**。
/// 从前这里"印一句警告再回退普通"，与 `--seed 4x`／`--faction 9`／`--boss 99`（那三条一律 rc=2）是同族两种口径。
/// 回退的代价与静默同量：一个读不懂的难度值悄悄换掉"你正在玩哪一局"，而输出里没有任何地方说命令被改写过。
/// 缺值那一面（`--difficulty` 放在最后）旧写法会当成"未知值 空串"再回退，现在与 `--seed` 同口径直接报错。
fn parse_difficulty(args: &[String]) -> Difficulty {
    let raw = match value_flag(args, "--difficulty") {
        Flag::Absent => return Difficulty::Normal,
        Flag::Missing => bad_opt(None, "--difficulty", DIFFICULTIES),
        Flag::Value(raw) => raw,
    };
    match Difficulty::parse(raw) {
        Some(d) => d,
        None => bad_opt(Some(raw), "--difficulty", DIFFICULTIES),
    }
}

/// `auto [N]` 的轮数。**不给＝1；给 0 就是零场**（与 `auto_bosses` 同一口径，不偷偷保底一轮）；
/// 读不懂 ⇒ rc=2。旧写法 `parse().ok().unwrap_or(1)` 让 `auto 3x` **静默按 1 局**跑完，
/// 屏幕上一个字都没说"你要的那几局没跑"。
/// 文案不套 `bad_opt`：那个模板是"{旗标} 的值「x」"，轮数是**位置参数**，没有旗标可指。
fn auto_rounds(args: &[String]) -> u32 {
    match positionals(args).first().map(String::as_str) {
        None => 1,
        Some(raw) => match raw.parse::<u32>() {
            Ok(n) => n,
            Err(_) => {
                println!("✖ auto 的轮数「{raw}」读不懂，要 非负整数（不给＝1）");
                std::process::exit(2);
            }
        },
    }
}
