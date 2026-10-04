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
//! 套壳验证帧（第十四步·其一）：第一个**非-CLI 壳**——纯终端 TUI。
//!
//! 存在理由不是"做个好看界面"，而是**证明那道缝接得住**：`session::run_level` 只认 `Host` 那七条接缝、
//! 不认识 `meta`，所以只要再实现一遍 `Host`（这里＝`TuiHost`）就能驱动同一台阶段机，引擎一个字节都不改。
//! 若这一帧成立，2D/3D 换的只是"怎么画、从哪读输入"，不再是"引擎要不要知道壳长什么样"。
//!
//! 本版刻意受限（三条人锁决策）：
//! - 只走 `--tui` 显式入口，默认一律文本路径（不自动探测、不误开）。
//! - **不上 raw/方向键**：逐帧重绘 + 行输入（回车提交），天然复用 `Host::read`（本就是阻塞读一行）⇒ **零新增终端控制依赖**。
//! - 只驱动**单关 Boss 跳打**（`has_next:false`、不落盘）：整局爬梯要先做 L3-d，见 `main.rs::gate_tui`。
//!
//! 依赖方向同 `meta`：`tui → battle / boss / model / session / view`，不认识 CLI 参数解析。
//! 呈现只经 `Battle::view()` 取 L1 快照（`view::Board`），不直读引擎字段——与 render 同一上游，不开第二真相源。

use crate::battle::{Battle, Outcome};
use crate::boss::BossId;
use crate::model::{CardInst, Faction};
use crate::view::CardView;

/// 一格的可见简述——只用 `CardView` 的字段，不认引擎内部。
fn cell_label(c: &CardView) -> String {
    let mut s = format!("{} {}/{}", c.name, c.hp, c.threshold);
    if c.flame != 0 {
        s.push_str(&format!(" 焰{}", c.flame));
    }
    if c.upgrades > 0 {
        s.push_str(&format!(" +{}", c.upgrades));
    }
    if c.crafted {
        s.push_str(" 造");
    }
    if !c.skills.is_empty() {
        s.push_str(&format!(" [{}]", c.skills.join(",")));
    }
    s
}

/// 把一帧快照排布成一份纯文本界面。**纯函数、零 IO**：只吃 `Board` 的可见字段，不碰 stdin/stdout。
/// 拆出来正是为了可测——头less 断言能直接核对"draw 真读了快照"，而不必跑整个壳。
/// 这里**不写** `§N:NNN` 规则锚：draw 只是排布，不新增任何规则真值（写锚会去动反向覆盖的生产面读数）。
pub(crate) fn draw(b: &Battle) -> String {
    let v = b.view();
    let mut s = String::new();
    // 帧头：回合 + 我方烛 + 业力 + 两堆计数。
    s.push_str(&format!(
        "回合 {} ｜ 我方烛 {}/{} ｜ 业力 {} ｜ 继承堆 {} ｜ 开端堆 {}\n",
        v.player.turn, v.player.hp, v.player.cap, v.player.karma, v.draw_pile_len, v.starter_pile_len
    ));
    if let Some(r) = &v.rule {
        s.push_str(&format!("规则【{}】{}\n", r.label, r.text));
    }
    // 敌方持业者 1~2 根（炎与冰双柱）。业力只在第一根（共用一池）。
    for (i, h) in v.enemy_holders.iter().enumerate() {
        let karma = h.karma.map(|k| format!(" ｜ 业力 {k}")).unwrap_or_default();
        let zone = h.column_zone.map(|(a, z)| format!(" ｜ 受击列 {a}-{z}")).unwrap_or_default();
        s.push_str(&format!("持业者#{i} ｜ {} ｜ {}/{}{}{}\n", h.name, h.hp, h.cap, karma, zone));
    }
    // 三行 × 四列盘面：敌后 / 敌前 / 我前。空格以 `·` 占位，列序固定 P1..P4。
    for (tag, row) in [("敌后", &v.enemy_back), ("敌前", &v.enemy_front), ("我前", &v.player_front)] {
        let cells: Vec<String> = row.iter().map(|c| c.as_ref().map(cell_label).unwrap_or_else(|| "·".to_string())).collect();
        s.push_str(&format!("{tag} ｜ {}\n", cells.join(" ｜ ")));
    }
    // 手牌行：带索引（`p <idx> <P1-P4>` 用的就是这个序号）。
    if v.hand.is_empty() {
        s.push_str("手牌 ｜ (空)\n");
    } else {
        let cards: Vec<String> = v.hand.iter().enumerate().map(|(i, c)| format!("{i}:{}", cell_label(c))).collect();
        s.push_str(&format!("手牌 ｜ {}\n", cards.join(" ｜ ")));
    }
    s.push_str("—— 日志 ——\n");
    s.push_str(&v.log_tail(12));
    s.push('\n');
    s
}

/// `session::Host` 的 TUI 实现——与 `CliHost` 驱动同一台阶段机，差别只在两处接缝：
/// `board` 清屏后打 `draw`（而非逐帧追加的 ASCII），`snapshot_*` 空实现（单关跳打不落盘）。
/// `write`／`read` 与 `CliHost` 口径一致：原样打印不补换行、读一行连行尾。
/// 本结构体是全仓除 `CliHost` 外另一个被允许 `stdin().read_line()` 的地方（`process::exit` 不在此——Quit 只收尾不 exit）。
pub(crate) struct TuiHost {
    seed: u64,
    faction: Faction,
    /// 本关 Boss（单关跳打固定是一场 Boss 战）。
    boss: BossId,
}

impl crate::session::Host for TuiHost {
    fn write(&mut self, s: &str) {
        use std::io::Write;
        print!("{s}");
        std::io::stdout().flush().ok();
    }

    fn board(&mut self, b: &Battle) {
        use std::io::Write;
        // 清屏 + 归位光标，再打这一帧。逐帧**重绘**（本版无 diff 刷新、无动画）。
        print!("\x1b[2J\x1b[H{}", draw(b));
        std::io::stdout().flush().ok();
    }

    fn log_tail(&mut self, b: &Battle) {
        println!("{}", b.view().log_tail(12));
    }

    fn read(&mut self) -> Option<String> {
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 { None } else { Some(line) }
    }

    fn snapshot_entry(&mut self, _level: u32, _inherit: &[CardInst], _karma: i32) {}

    fn snapshot_next(&mut self, _level: u32, _inherit: &[CardInst], _karma: i32) {}

    fn new_battle(&mut self, inherit: &mut Vec<CardInst>, level: u32) -> Battle {
        Battle::new_boss(self.seed, self.faction, self.boss, std::mem::take(inherit), level)
    }
}

/// TUI 单关跳打入口：与 `meta::boss_run` 同一关隘构造，但把呈现与输入交给 `TuiHost`。
/// `Terminus::Quit` 时**不 `process::exit`**、只收尾（TUI 不该把宿主进程半路截走，退出意图由最外层 main 决定）。
pub(crate) fn run(seed: u64, faction: Faction, id: BossId) {
    let level = id.chapter() * crate::boss::LEVELS_PER_CHAPTER;
    let cfg = crate::session::LevelCfg { head: crate::meta::level_head(level, Some(id)), level, has_next: false };
    let mut inherit: Vec<CardInst> = Vec::new();
    let mut karma = 0;
    let mut host = TuiHost { seed, faction, boss: id };
    match crate::session::run_level(&mut host, &cfg, &mut inherit, &mut karma) {
        crate::session::Terminus::Done(out) => match out {
            Outcome::PlayerWin => println!("\n✓ 击败 Boss「{}」（第{}章）。seed={seed} 可复现。", id.name(), id.chapter()),
            Outcome::Draw => println!("\n平局。Boss「{}」未被击败。", id.name()),
            Outcome::PlayerLose => println!("\n✖ 败北。Boss「{}」未被击败。", id.name()),
        },
        crate::session::Terminus::Quit { msg } => {
            if let Some(m) = msg {
                println!("{m}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Difficulty;
    use crate::model::{faction_cards, short_card};
    use crate::view::HolderView;

    fn ember_card(i: usize) -> CardInst {
        CardInst::new(900 + i as u64, faction_cards(Faction::Ember)[i])
    }

    /// draw 真读快照、不是自己造的一套数：改 `p_karma` / 往盘面放一张牌 / 换双柱 Boss，输出必须跟着动。
    /// 断言一律拿 `Board` 里的**实际字段值**去撞（不硬编码名字/数字），所以引擎文案改了也不会误红。
    #[test]
    fn draw_renders_from_the_snapshot_not_its_own_idea_of_the_state() {
        let mut b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        b.p_karma = 9;
        b.e_karma = 50; // 放敌前牌要够敌方业力（口径同 view.rs 的 on_board 测）
        let held = ember_card(1);
        let held_name = held.def.name;
        b.enemy_place_inst(held, 0, crate::battle::Row::Front).unwrap();
        let v = b.view();
        let out = draw(&b);

        assert!(out.contains(&format!("业力 {}", v.player.karma)), "帧头业力要用快照里的实际值：{out}");
        assert!(out.contains(v.enemy_holders[0].name), "持业者名要出现：{out}");
        assert!(out.contains(held_name), "放上的敌前牌名要出现：{out}");
        assert!(out.contains("手牌"), "手牌行要有标题");
        if let Some(h) = v.hand.first() {
            assert!(out.contains(h.name), "手牌首牌名要逐字出现（否则手牌行只画了个标题）：{out}");
        }
    }

    /// 双持业者（炎与冰）两柱都要画出来——单柱 Boss 只有一根，这条差别只有真取字段才测得到。
    #[test]
    fn draw_shows_both_holders_for_the_dual_holder_boss() {
        let b = Battle::new_boss(2026, Faction::Ember, BossId::YanBing, Vec::new(), 48);
        let v = b.view();
        assert_eq!(v.enemy_holders.len(), 2, "炎与冰是双持业者");
        let names: Vec<&HolderView> = v.enemy_holders.iter().collect();
        let out = draw(&b);
        assert!(out.contains(names[0].name), "第一柱名要出现：{out}");
        assert!(out.contains(names[1].name), "第二柱名也要出现（否则双柱被画漏）：{out}");
    }

    /// draw 不引入第二份"一行简述"：格子里的牌名与引擎自己的 `short_card` 同名（这里只核对名字部分，
    /// 数字排布是 draw 的版式、不是引擎口径，故不强求整串相等）。
    #[test]
    fn draw_uses_the_snapshot_card_names_verbatim() {
        let mut b = Battle::new(3, Faction::Shadow, Faction::Ember, Difficulty::Normal, Vec::new(), 1);
        let c = ember_card(2);
        let want = short_card(&c);
        let name_only = c.def.name;
        b.p_front[0] = Some(c);
        let out = draw(&b);
        assert!(out.contains(name_only), "我前格牌名要逐字出现：{out}");
        assert!(!want.is_empty(), "参照简述非空（防抽到空实现）");
    }

    /// AGPL 防绕过立场的第一批执法牙（见 pool/agpl-anti-circumvention-fused-shell.md）。
    /// 三条禁令：① bin-only（无 `src/lib.rs`）② 单可执行（Cargo.toml 无 `[lib]`、无第二 `[[bin]]`）
    /// ③ 源码不留对外可序列化契约。任一被违反 ⇒ 当场红并列位置，而不是靠文档口头声明。
    #[test]
    fn this_program_ships_no_reusable_api_surface() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        // ① 不许出现 lib.rs：外面一旦能 `use midline::...`，AGPL 内核就成了可插拔依赖。
        assert!(!manifest.join("src/lib.rs").exists(), "出现 src/lib.rs ＝ 对外可链接 API 面（AGPL 把手）");

        // ② Cargo.toml：无 [lib] 段；`[[bin]]` 必须为 0（可执行只由 main.rs 隐式产出，壳不得拆独立件）。
        let toml = std::fs::read_to_string(manifest.join("Cargo.toml")).unwrap();
        assert!(!toml.lines().any(|l| l.trim() == "[lib]"), "Cargo.toml 出现 [lib] 段：壳与内核须编译期融合");
        let bin_sections = toml.lines().filter(|l| l.trim() == "[[bin]]").count();
        assert_eq!(bin_sections, 0, "Cargo.toml 有 {bin_sections} 个 [[bin]]：TUI/2D/3D 不许拆成独立可执行（须同进程单态）");

        // ③ 扫被编译的 src 的非注释行，禁对外可序列化契约。needle 只写在下面这一条 `banned` 定义行里，
        //    该行被跳过；断言消息不含任何 needle，扫描器不会自指命中自己。
        let banned = ["serde", "Serialize", "Deserialize", "--json", "json!("];
        let mut offenders: Vec<String> = Vec::new();
        for f in compiled_src_files(manifest) {
            let Ok(src) = std::fs::read_to_string(&f) else { continue };
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            for (i, l) in src.lines().enumerate() {
                let t = l.trim();
                if t.starts_with("//") || t.contains("let banned") || t.contains("banned.iter") {
                    continue;
                }
                if banned.iter().any(|x| t.contains(x)) {
                    offenders.push(format!("{name}:{}: {t}", i + 1));
                }
            }
        }
        assert!(offenders.is_empty(), "源码出现可序列化/对外契约 token（违反 AGPL 防绕过立场）：{offenders:?}");
    }

    /// 只扫**被编译**的文件：模块清单取 `main.rs` 里的 `mod X;`（口径同 `model.rs::src_rs_files`）。
    /// 没被 `mod` 的 `src/*.rs` 是死文件，不参与构建，扫它只会给"永真"的假绿让路。
    fn compiled_src_files(manifest: &std::path::Path) -> Vec<std::path::PathBuf> {
        let main = std::fs::read_to_string(manifest.join("src/main.rs")).unwrap();
        let mut mods: Vec<String> = Vec::new();
        for l in main.lines() {
            let t = l.trim().trim_start_matches("pub ").trim_start_matches("pub(crate) ").trim();
            if let Some(name) = t.strip_prefix("mod ").and_then(|rest| rest.strip_suffix(';')) {
                mods.push(name.trim().to_string());
            }
        }
        assert!(mods.contains(&"tui".to_string()), "main.rs 里没有 `mod tui;` ⇒ 本壳根本没参与构建，这条机检等于没在筛");
        std::fs::read_dir(manifest.join("src"))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .filter(|p| {
                let stem = p.file_stem().unwrap().to_string_lossy().to_string();
                stem == "main" || mods.contains(&stem)
            })
            .collect()
    }
}
