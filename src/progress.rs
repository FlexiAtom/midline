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
//! 关与关之间的牌堆操作（融合 / 升级 / 收尸）——**纯函数层，一个字都不打印**。
//!
//! 为什么单开一个文件：这三条规则原先住在 `meta.rs`，而 `meta.rs` 同时是 CLI 驱动器（stdin、存档落点、
//! 托管模拟）。命令层 `command.rs` 要执行 `fuse/up` 就得回调 `meta`，于是 `meta → session → command → meta`
//! 成环——把壳摘出来时会连 CLI 一起拖走。摘出这三条只依赖 `model` 的操作后，依赖是单向的：
//! `command → progress → model`，`meta` 与将来的壳都在最上层。
//!
//! 返回值即文案：`Ok(String)` 是「✓ 」后面的那句，`Err(String)` 是「✖ 」后面的那句，
//! 前缀由调用方加（与改前的 `meta.rs` 同一分工）。

use crate::battle::Battle;
use crate::model::{CardInst, Skill, short_card};

/// §九:311 融合＝把两张牌的技能合并到一张新牌上（本函数是唯一落点）。
/// §廿二:946 融合后新牌费用＝主牌费用（副牌只贡献技能，不贡献费用）。§九:331 是同一句话在 §九 流程里的出处。
/// §廿二:950 副牌直接消失——**不进弃牌堆**，故也不会有死亡返还、不会再被抽到。§九:333 是同一句话在 §九 流程里的出处。
pub fn fuse_cards(inherit: &mut Vec<CardInst>, main: usize, sub: usize, karma: &mut i32) -> Result<String, String> {  // §廿三:1002 技能合并、特性取主牌、副牌消失；§七:279 融合＝特性不参与、技能是融合的核心（本函数体内出现 `skills` 而不出现 `.tr` 就是这两句的形状）
    if main >= inherit.len() || sub >= inherit.len() || main == sub {
        return Err("下标无效".into());
    }
    if inherit[main].is_starter() || inherit[sub].is_starter() {
        // §九:363 开端不可融合——主牌、副牌**两侧都查**，且不论它此刻在手牌还是继承堆；§五:184 可融合 ❌（同一条规则的两种文档措辞）
        return Err("开端不可融合（无论在手牌还是继承堆）".into());
    }
    let price = (inherit[sub].def.cost - 1).max(0);  // §廿三:1001 融合费用＝副牌费用-1，最低 0；§九:326 §九:350 同一条规则的两种文档措辞（流程第 3 步／设计意图行）
    if *karma < price {
        return Err(format!("业力不足：融合需{price}，当前{karma}"));
    }
    *karma -= price;
    let s = inherit.remove(sub); // §九:325 副牌的作用域到此为止：它只在这里被读，之后只贡献 skills
    // 先按原下标校正主牌位置：sub 被摘除后，其后的下标整体前移一位
    let m = &mut inherit[if main > sub { main - 1 } else { main }]; // §九:324 新牌占用主牌那一格；§九:327 生成新牌＝就地改写主牌；§九:334 新牌留在继承堆里
    let n = s.skills.len(); // §七:282 设计意图「融合只融合技能，不融合特性」——动笔处只读 `skills`，`m.def` 整块不动 ⇒ §九:328 特性／§九:329 数值／§九:330 阈值／§九:331 费用全部取主牌
    for sk in s.skills {
        // §九:332 技能＝主牌技能＋副牌技能：逐条 append，不去重、不折叠成集合
        m.skills.push(sk); // §九:352 §九:354 同名技能叠加（§九:357 那条 1+1=2 的等式就是它的落点）
    }
    m.crafted = true; // 自造牌：任何离场永久消失（§十:372）
    Ok(format!("{} 吸收副牌「{}」的{n}个技能 → {}", m.def.name, s.def.name, short_card(m)))
}

/// §十一:402 升级＝每通关一次的奖励；§廿二:965 上限 3 次。
pub fn upgrade_card(inherit: &mut [CardInst], idx: usize, kind: &str) -> Result<String, String> {
    if idx >= inherit.len() {
        return Err("下标无效".into());
    }
    let c = &mut inherit[idx];
    if c.upgrades >= 3 {
        return Err("该牌已达升级上限3次".into());
    }
    match kind {
        "power" => {
            c.def.power += 1;
            c.hp = c.def.power; // 升级即时可见（每关本就重置满格）
        }
        "thr" => c.def.threshold = (c.def.threshold - 1).max(1),
        "skill" => {
            let pool = Skill::list();
            c.skills.push(pool[c.upgrades as usize % pool.len()]);
        }
        _ => return Err("power|thr|skill".into()),
    }
    c.upgrades += 1;
    Ok(format!("升级后：{}", short_card(c)))
}

/// §廿二:947 继承堆上限 10 张，超出弃最早入堆的牌（永久消失）。
/// 幸存者回继承堆（手牌+堆底+场上），上限10，超出弃最早。
/// 返回那些"被上限挤出去"的牌该显示的文案——本层不打印，打印归调用方。
pub fn collect_survivors(b: &mut Battle, inherit: &mut Vec<CardInst>) -> Vec<String> {  // §十二:503 存活牌收进继承堆＝带入下一关；§廿三:1003 跨关继承（阵亡永久消失见 battle.rs:1134）
    let mut out = Vec::new();
    let mut survivors = b.battle_survivors();
    survivors.sort_by_key(|c| c.id);
    inherit.append(&mut survivors);
    while inherit.len() > 10 {
        let c = inherit.remove(0);
        out.push(format!("继承堆超10张 → 弃置（永久消失）：{}", short_card(&c)));
    }
    out
}

// ---------------------------------------------------------------------------
// §九 融合系统 反向覆盖设施（只存在于测试构建）：五道围栏的行解析器。
//
// 口径与 `battle.rs` 里 §十四／§十六 那两台解析器一致：**实测路的行集合由这里给**，`model.rs` 的
// 推导器只认这里数出来的行号。两处各判一次，同一串字就会朝不同方向错——而「这行有没有被核对过」
// 恰恰只在这种偏移上才会静默绿。
// ---------------------------------------------------------------------------

/// §九 围栏一行的主张。一个变体对应文档的一种行形，认不出来就当场 panic：「认不出来」正是目的——
/// 文档在 §九 添了新行时这里先红，而不是让那一行从尺子外面静默漏过去。
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum S9Claim {
    /// md:311 `融合 = 将两张牌的技能合并到一张新牌上`
    Definition,
    /// md:324 `1. 选择主牌（保留其特性+数值+阈值+费用）`——括号里按 `+` 切出的字段名
    MainKeeps(Vec<String>),
    /// md:325 `2. 选择副牌（提供技能）`
    SubProvides(String),
    /// md:326 `3. 消耗业力（融合费用 = 副牌费用-1，最低0）`
    Price { minus: i32, floor: i32 },
    /// md:327 `4. 生成新牌：`
    GenHead,
    /// md:328–331 `- 特性 = 主牌特性` 这一族，存左边的字段名
    Field(String),
    /// md:332 `- 技能 = 主牌技能 + 副牌技能`
    SkillSum,
    /// md:333 `5. 副牌消失（不进入弃牌堆）`
    SubGone { no_discard: bool },
    /// md:334 `6. 新牌进入继承堆`
    IntoInherit,
    /// md:335 `7. 继承堆总数不变（…）`——**文档这句与引擎相反**（引擎 2 张进 1 张出）。它躺在债表里，
    /// 由解析器数出来只为让它**可见**，实测不复现它（复现＝替文档把错的账做平）。
    PileTotalUnchanged,
    /// md:341／342 `主牌：焚稿人（3费，数值3，特性：…）`
    ExampleCard { which: &'static str, name: String, cost: i32, power: i32, trait_text: String },
    /// md:344 `融合消耗：3-1=2业力`
    ExamplePrice { a: i32, b: i32, out: i32 },
    /// md:345 `融合后：`
    ExampleOutHead,
    /// md:346 `新牌 = 焚稿人（3费，数值3，特性不变）+ 技能：攻击后+1累积`
    ExampleResult { name: String, cost: i32, power: i32, skill_text: String },
    /// md:347 `副牌消失`
    ExampleSubGone,
    /// md:357 `攻击后自身+1累积 + 攻击后自身+1累积 = 攻击后自身+2累积`
    StackEquation { terms: Vec<String>, lhs: Vec<i32>, rhs: i32 },
    /// md:363 `开端不可融合，无论在手牌还是继承堆`
    StarterForbidden,
}

#[cfg(test)]
impl S9Claim {
    /// 这一行是**示例**（文档拿一对具体的牌演示一遍）还是**规则**？
    /// 区分开是因为两条路的牙不一样：示例行只许走实测，**不许**用锚点顶替（同 §十五／§十六 那条纪律——
    /// 给示例挂锚等于把「必须跑一遍」降级成「有人指过来就行」）。判据取自变体本身，不手写行号清单。
    ///
    /// `StackEquation`（md:357）**不算示例**：它不点名任何一对牌，它是 md:354「若技能效果冲突…则叠加」那条
    /// 规则的**通式**（同名技能相加），和 §十四 围栏里那九行一样走双记账——锚点在 `progress.rs:51` 的
    /// `m.skills.push(sk)` 上，实测在等式两侧对撞引擎的累积值上。判据挪走的后果本章推导器会当场报：
    /// 把它算成示例 ⇒ 那条锚点违规；把它算成规则又摘掉锚 ⇒ 双记账层红。
    pub(crate) fn is_example(&self) -> bool {
        matches!(
            self,
            S9Claim::ExampleCard { .. }
                | S9Claim::ExamplePrice { .. }
                | S9Claim::ExampleOutHead
                | S9Claim::ExampleResult { .. }
                | S9Claim::ExampleSubGone
        )
    }
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct S9Line {
    pub line: usize,
    pub claim: S9Claim,
}

/// 形态错报出口。**写成普通 fn 而不是闭包**，理由同 §十四／§十六 那两处：闭包要在六条分支里各自
/// 借用 `t`，返回 `!` 时生命周期缠成一团。
#[cfg(test)]
fn s9_bad(tag: &str, why: &str) -> ! {
    panic!(
        "{tag} 不像 §九 围栏行的形态（`融合 = …`｜`N. 步骤（…）`｜`- 字段 = 主牌字段`｜`- 技能 = 主牌技能 + 副牌技能`｜\
         `主牌／副牌：名（N费，数值N，特性：…）`｜`融合消耗：A-B=C业力`｜`融合后：`｜`新牌 = 名（… 特性不变）+ 技能：…`｜\
         `副牌消失`｜`X + X = Y` 等式｜`开端不可融合，…`）：{why}。\
         请先扩本解析器与它对应的实测断言，别让规则行从尺子外面漏过去（挂锚对这类行不算实测，见 model.rs 的 §九 推导器）"
    )
}

/// 全角括号里的正文。§九 每行最多一对，取**第一对**（`（3费，数值3，特性：阈值技对同列+2累积）` 里没有嵌套）。
#[cfg(test)]
fn s9_paren(t: &str) -> Option<&str> {
    let a = t.find('（')?;
    let b = t.find('）')?;
    if b > a { Some(&t[a + '（'.len_utf8()..b]) } else { None }
}

/// 括号内按全角逗号切段。
#[cfg(test)]
fn s9_parts(inner: &str) -> Vec<String> {
    inner.split('，').map(|s| s.trim().to_string()).collect()
}

#[cfg(test)]
fn s9_classify(t: &str, line: usize) -> S9Claim {
    let tag = format!("md:{line}「{t}」");
    let one_num = |s: &str, who: &str| -> i32 {
        let v = crate::battle::s15_digits(s);
        if v.len() != 1 {
            s9_bad(&format!("{tag} 的「{who}」段"), &format!("解析出 {} 个数字，该有 1 个", v.len()));
        }
        v[0]
    };

    if let Some(rest) = t.strip_prefix("融合 = ") {
        if rest != "将两张牌的技能合并到一张新牌上" {
            s9_bad(&tag, &format!("定义行后半截现在是「{rest}」，与登记的那句不同"));
        }
        return S9Claim::Definition;
    }
    if t.starts_with("开端不可融合") {
        if !t.contains("在手牌还是继承堆") {
            s9_bad(&tag, "有「开端不可融合」却没写适用范围（手牌／继承堆），范围变了就得同时改 `fuse_cards` 的闸");
        }
        return S9Claim::StarterForbidden;
    }
    // 346 必须以「新牌 = 」优先于等式判据：它同样含 " + "、" = "，且以「累积」收尾。
    if let Some(rest) = t.strip_prefix("新牌 = ") {
        let Some(paren) = s9_paren(rest) else {
            s9_bad(&tag, "结果行没有全角括号给出新牌的字段");
        };
        let name = rest.split('（').next().unwrap_or("").trim().to_string();
        let ps = s9_parts(paren);
        if ps.len() != 3 || !ps[2].contains("特性不变") {
            s9_bad(&tag, &format!("括号里应是「N费／数值N／特性不变」三段，实测 {ps:?}"));
        }
        let cost = one_num(&ps[0], "费");
        let power = one_num(&ps[1], "数值");
        let Some((_, skill)) = rest.split_once("技能：") else {
            s9_bad(&tag, "「+ 技能：」这半截不见了，新牌到底多哪条技能读不出来");
        };
        return S9Claim::ExampleResult { name, cost, power, skill_text: skill.trim().to_string() };
    }
    if t.ends_with("累积") && t.contains(" + ") && t.contains(" = ") {
        let Some((lhs, rhs)) = t.split_once(" = ") else {
            s9_bad(&tag, "等式读不出左边／右边");
        };
        let terms: Vec<String> = lhs.split(" + ").map(|s| s.trim().to_string()).collect();
        if terms.len() < 2 || terms.iter().any(|x| x != &terms[0]) {
            s9_bad(&tag, &format!("叠加等式左边应是**同名**技能相加，实测 {terms:?}"));
        }
        let lhs_nums: Vec<i32> = terms.iter().flat_map(|s| crate::battle::s15_digits(s)).collect();
        let rhs_nums = crate::battle::s15_digits(rhs);
        if rhs_nums.len() != 1 {
            s9_bad(&tag, &format!("等式右边该有 1 个数，实测 {}", rhs_nums.len()));
        }
        return S9Claim::StackEquation { terms, lhs: lhs_nums, rhs: rhs_nums[0] };
    }
    if t.starts_with("主牌：") || t.starts_with("副牌：") {
        let which = if t.starts_with("主牌：") { "主牌" } else { "副牌" };
        let Some(body) = t.strip_prefix(which).and_then(|s| s.strip_prefix('：')) else {
            s9_bad(&tag, "「主牌：／副牌：」前缀切不动");
        };
        let Some(paren) = s9_paren(body) else {
            s9_bad(&tag, "示例牌没有全角括号给出费用／数值／特性");
        };
        let name = body.split('（').next().unwrap_or("").trim().to_string();
        let ps = s9_parts(paren);
        if ps.len() != 3 || !ps[1].starts_with("数值") || !ps[2].starts_with("特性") {
            s9_bad(&tag, &format!("括号里应是「N费／数值N／特性：…」三段，实测 {ps:?}"));
        }
        return S9Claim::ExampleCard {
            which,
            name,
            cost: one_num(&ps[0], "费"),
            power: one_num(&ps[1], "数值"),
            trait_text: ps[2].clone(),
        };
    }
    if let Some(rest) = t.strip_prefix("融合消耗：") {
        let v = crate::battle::s15_digits(rest);
        if v.len() != 3 {
            s9_bad(&tag, &format!("「{rest}」应给 A-B=C 三个数，实测 {}", v.len()));
        }
        return S9Claim::ExamplePrice { a: v[0], b: v[1], out: v[2] };
    }
    if t == "融合后：" {
        return S9Claim::ExampleOutHead;
    }
    if t == "副牌消失" {
        return S9Claim::ExampleSubGone;
    }
    if let Some((num, body)) = t.split_once(". ")
        && num.len() == 1
        && num.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        let inner = s9_paren(body);
        return match num {
            "1" => {
                if !body.starts_with("选择主牌") {
                    s9_bad(&tag, "第 1 步不再是「选择主牌」");
                }
                let i = inner.unwrap_or_else(|| s9_bad(&tag, "第 1 步没写主牌保留哪些字段"));
                let Some(list) = i.strip_prefix("保留其") else {
                    s9_bad(&tag, &format!("括号里不是「保留其…」起头，实测「{i}」"));
                };
                let fields: Vec<String> = list.split('+').map(|s| s.trim().to_string()).collect();
                if fields.len() != 4 {
                    s9_bad(&tag, &format!("主牌保留的字段该是 4 个（特性／数值／阈值／费用），实测 {:?}", fields));
                }
                S9Claim::MainKeeps(fields)
            }
            "2" => {
                if !body.starts_with("选择副牌") {
                    s9_bad(&tag, "第 2 步不再是「选择副牌」");
                }
                S9Claim::SubProvides(inner.unwrap_or_else(|| s9_bad(&tag, "第 2 步没写副牌提供什么")).to_string())
            }
            "3" => {
                if !body.starts_with("消耗业力") {
                    s9_bad(&tag, "第 3 步不再是「消耗业力」");
                }
                let i = inner.unwrap_or_else(|| s9_bad(&tag, "第 3 步没写价的算式"));
                let v = crate::battle::s15_digits(i);
                if v.len() != 2 || !i.contains("副牌费用") {
                    s9_bad(&tag, &format!("「{i}」应给「副牌费用-N，最低M」两个数，实测 {v:?}"));
                }
                S9Claim::Price { minus: v[0], floor: v[1] }
            }
            "4" => {
                if body != "生成新牌：" {
                    s9_bad(&tag, &format!("第 4 步现在是「{body}」"));
                }
                S9Claim::GenHead
            }
            "5" => {
                if !body.starts_with("副牌消失") {
                    s9_bad(&tag, "第 5 步不再是「副牌消失」");
                }
                S9Claim::SubGone { no_discard: inner == Some("不进入弃牌堆") }
            }
            "6" => {
                if body != "新牌进入继承堆" {
                    s9_bad(&tag, &format!("第 6 步现在是「{body}」"));
                }
                S9Claim::IntoInherit
            }
            "7" => {
                if !body.contains("继承堆总数不变") {
                    s9_bad(&tag, &format!("第 7 步现在是「{body}」"));
                }
                S9Claim::PileTotalUnchanged
            }
            other => s9_bad(&tag, &format!("流程里冒出了第 {other} 步，本解析器只登记过 1–7 步")),
        };
    }
    if let Some(rest) = t.strip_prefix("- ") {
        let Some((lhs, rhs)) = rest.split_once(" = ") else {
            s9_bad(&tag, "缩进的字段行读不出「X = Y」");
        };
        let lhs = lhs.trim();
        let rhs = rhs.trim();
        if lhs == "技能" {
            if rhs != "主牌技能 + 副牌技能" {
                s9_bad(&tag, &format!("技能行右边现在是「{rhs}」"));
            }
            return S9Claim::SkillSum;
        }
        if rhs != format!("主牌{lhs}") {
            s9_bad(&tag, &format!("「{lhs}」的右边不是「主牌{lhs}」而是「{rhs}」"));
        }
        return S9Claim::Field(lhs.to_string());
    }
    s9_bad(&tag, "上面每一种形态都不像")
}

/// 定位 §九 章题，把它后面**每一道**围栏里的行全数解析出来（不像 §十四 只读第一道：本章有五道围栏，
/// 定义／流程／示例／冲突等式／开端各一道，少读一道就是让那一道的行从两把尺外面一起漏过去）。
/// 它同时是 §九 推导器实测路的唯一口径：那边的行集合必须由这里的行号构成。
#[cfg(test)]
pub(crate) fn parse_section9_fuse_rules(lines: &[String]) -> Vec<S9Line> {
    let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
    let head = lines
        .iter()
        .position(|l| l.trim() == "九、融合系统")
        .expect("§九 标题必须存在（文档结构变了就要同步改本解析器与 model.rs 的推导器）");
    let mut out: Vec<S9Line> = Vec::new();
    let mut fence = false;
    let mut ticks = 0usize;
    for n in (head + 2)..=lines.len() {
        let t = at(n).trim();
        if !fence && t == "---" {
            break;
        }
        if t == "```" {
            fence = !fence;
            ticks += 1;
            continue;
        }
        if !fence || t.is_empty() {
            continue;
        }
        out.push(S9Line { line: n, claim: s9_classify(t, n) });
    }
    assert_eq!(ticks, 10, "§九 应有五道围栏（定义／流程／示例／冲突等式／开端）＝10 个 ``` 符，实测 {ticks} 个 ⇒ 文档加了（或删了）一道围栏，本解析器读的集合跟着错位");
    assert!(!fence, "§九 最后一道围栏没有闭合（数到奇数个 ```）");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::Difficulty;
    use crate::model::{Faction, Skill, faction_cards};

    fn ember(i: usize) -> CardInst {
        CardInst::new(i as u64 + 1, faction_cards(Faction::Ember)[i])
    }

    fn pile(n: usize) -> Vec<CardInst> {
        (0..n).map(ember).collect()
    }

    #[test]
    fn fuse_moves_skills_keeps_main_and_costs_sub_minus_one() {
        // 主牌 焚稿人 3费、副牌 雷烬 4费（本仓既有的取牌口，与 meta 时代的同一对）
        let mut inherit: Vec<CardInst> = vec![ember(3), ember(8)];
        inherit[1].skills.push(Skill::AtkSelfFlame1);
        let tr_main = inherit[0].def.tr;
        let tr_sub = inherit[1].def.tr;
        assert_ne!(tr_main, tr_sub, "主副牌特性必须不同，否则下面那条「特性取主牌」是空断言");
        let mut karma = 5;
        let msg = fuse_cards(&mut inherit, 0, 1, &mut karma).expect("融合应成功");
        assert!(msg.contains("吸收副牌「雷烬」的1个技能"), "{msg}");
        assert!(msg.ends_with(short_card(&inherit[0]).as_str()), "{msg}");
        assert_eq!(karma, 5 - (4 - 1), "融合消耗=副牌费用-1（§廿二:946）");
        assert_eq!(inherit.len(), 1, "副牌直接消失、不进弃牌堆（§廿二:950）");
        assert_eq!(inherit[0].def.cost, 3, "费用取主牌");
        assert_eq!(inherit[0].def.tr, tr_main, "特性取主牌——副牌特性不参与融合（§廿三:1002／§七:279）");
        assert_eq!(inherit[0].skills, vec![Skill::AtkSelfFlame1]);
        assert!(inherit[0].crafted, "融合产物转自造（§十:372）");
    }

    #[test]
    fn starter_cannot_fuse_and_refusal_touches_nothing() {
        let mut inherit = vec![CardInst::new(1, crate::model::STARTER), ember(1)];
        let mut k = 9;
        for (m, s) in [(0, 1), (1, 0), (1, 9), (1, 1)] {
            assert!(fuse_cards(&mut inherit, m, s, &mut k).is_err(), "({m},{s}) 该被拒");
        }
        assert_eq!(inherit.len(), 2, "被拒的融合不动堆");
        assert_eq!(k, 9, "被拒的融合不扣业力");
    }

    /// §九 的**实测路**：文档自己写下的那一对示例牌、那一条价式、那一道叠加等式，全部打进引擎再核对。
    /// 数一律从 `parse_section9_fuse_rules` 取，本测不手写 3／2／1——文档改了数这里跟着改，
    /// 文档没改而引擎改了，这里红（口径同 `battle.rs` 的 §十四／§十五／§十六 那三条复现测）。
    #[test]
    fn section9_fuse_rows_reproduce_on_the_engine() {
        let Some(lines) = crate::model::doc_or_skip() else { return };
        let rows = parse_section9_fuse_rules(&lines);

        fn example_card(rows: &[S9Line], which: &str) -> (usize, String, i32, i32, String) {
            for r in rows {
                if let S9Claim::ExampleCard { which: w, name, cost, power, trait_text } = &r.claim
                    && *w == which
                {
                    return (r.line, name.clone(), *cost, *power, trait_text.clone());
                }
            }
            panic!("§九 示例围栏里没有「{which}：…」这一行 ⇒ 文档换了示例牌，实测路失去对象（解析器与推导器都要跟着改）");
        }
        fn engine_defs() -> Vec<crate::model::CardDef> {
            [Faction::Ember, Faction::Frost, Faction::Shadow]
                .into_iter()
                .flat_map(faction_cards)
                .copied()
                .collect()
        }
        fn def_by_name(name: &str, md: usize) -> crate::model::CardDef {
            engine_defs()
                .into_iter()
                .find(|d| d.name == name)
                .unwrap_or_else(|| panic!("md:{md} 用的牌名「{name}」在引擎卡表里找不到 ⇒ 文档示例指向一张不存在的牌"))
        }

        let (l_main, name_main, cost_main, power_main, _) = example_card(&rows, "主牌");
        let (l_sub, name_sub, cost_sub, power_sub, trait_sub) = example_card(&rows, "副牌");
        let d_main = def_by_name(&name_main, l_main);
        let d_sub = def_by_name(&name_sub, l_sub);
        assert_eq!(d_main.cost, cost_main, "md:{l_main} 写主牌「{name_main}」{cost_main} 费，引擎卡表里它是 {} 费 ⇒ 示例与 §七 卡表其中一侧改了", d_main.cost);
        assert_eq!(d_sub.cost, cost_sub, "md:{l_sub} 写副牌「{name_sub}」{cost_sub} 费，引擎卡表里它是 {} 费 ⇒ 同上", d_sub.cost);

        let mut ex_price: Option<(usize, i32, i32, i32)> = None;
        let mut rule_price: Option<(usize, i32, i32)> = None;
        let mut result: Option<(usize, String, i32, i32, String)> = None;
        let mut eq: Option<(usize, String, Vec<i32>, i32)> = None;
        let mut l_total: Option<usize> = None;
        for r in &rows {
            match &r.claim {
                S9Claim::ExamplePrice { a, b, out } => ex_price = Some((r.line, *a, *b, *out)),
                S9Claim::Price { minus, floor } => rule_price = Some((r.line, *minus, *floor)),
                S9Claim::PileTotalUnchanged => {
                    if l_total.is_some() {
                        panic!("md:{} §九 流程里冒出第二条「继承堆总数不变」", r.line);
                    }
                    l_total = Some(r.line);
                }
                S9Claim::ExampleResult { name, cost, power, skill_text } => {
                    result = Some((r.line, name.clone(), *cost, *power, skill_text.clone()))
                }
                S9Claim::StackEquation { terms, lhs, rhs } => {
                    if eq.is_some() {
                        panic!("md:{} §九 围栏里冒出第二道叠加等式 ⇒ 本测只认一道，先决定拿哪条复现", r.line);
                    }
                    eq = Some((r.line, terms[0].clone(), lhs.clone(), *rhs));
                }
                _ => {}
            }
        }
        let l_total = l_total.expect("§九 流程围栏没有第 7 步「继承堆总数不变」");
        let (l_ex, a, b, out) = ex_price.expect("§九 示例围栏没有「融合消耗：A-B=C业力」这一行");
        let (l_rule, minus, floor) = rule_price.expect("§九 流程围栏没有「消耗业力（…）」这一行");
        let (l_res, res_name, res_cost, res_power, res_skill) = result.expect("§九 示例围栏没有「新牌 = …」这一行");
        let (l_eq, eq_label, eq_lhs, eq_rhs) = eq.expect("§九 没有那道叠加等式");

        // 示例的算术与规则行必须同式：A 是副牌费用、B 是流程第 3 步的减数、C 是差。
        assert_eq!((a, b), (cost_sub, minus), "md:{l_ex}「{a}-{b}={out}」与 md:{l_rule} 的价式对不上：副牌费用是 {cost_sub}、减数是 {minus}");
        assert_eq!(out, a - b, "md:{l_ex} 的等号右边自己不算数：{a}-{b}≠{out}");

        // —— 文档示例原样跑一遍：两张牌都没挂技能 ——
        let mut pile = vec![CardInst::new(1, d_main), CardInst::new(2, d_sub)];
        let before = (pile[0].def.tr, pile[0].def.power, pile[0].def.threshold, pile[0].def.cost, pile[0].skills.clone());
        let mut karma = out;
        let msg = fuse_cards(&mut pile, 0, 1, &mut karma).expect("文档这一对牌在文档算出的业力下应当能融");
        assert_eq!(karma, 0, "md:{l_ex} 说消耗 {out} 业力，引擎扣掉的不止／不足这个数（当前业力给的就是 {out}）");
        assert_eq!(pile.len(), 1, "引擎的账是 {} 张进 1 张出；md:{l_total} 写的却是「继承堆总数不变」⇒ 这一行走的是债表，实测不复现它（要改成真不变，先偿债再动 `fuse_cards`）", 2);
        assert!(!pile.iter().any(|c| c.def.name == name_sub), "副牌「{name_sub}」还留在堆里＝md:333 的「消失」没做到");
        assert!(msg.contains(&name_sub), "文案没提到副牌：{msg}");
        let new = &pile[0];
        assert_eq!(new.def.name, res_name, "md:{l_res} 写的新牌名是「{res_name}」，引擎给的是「{}」", new.def.name);
        assert_eq!((new.def.tr, new.def.power, new.def.threshold, new.def.cost), (before.0, before.1, before.2, before.3), "md:328–331：新牌的特性／数值／阈值／费用必须逐项等于主牌，引擎动了其中一项");
        assert!(new.skills.is_empty(), "md:332 说技能＝主牌＋副牌，文档这一对牌两侧都没挂技能，新牌却多了技能：{:?}", new.skills);

        // 业力不足那一侧：文档算出的价减去 1，引擎应当拒。
        if out > 0 {
            let mut p2 = vec![CardInst::new(3, d_main), CardInst::new(4, d_sub)];
            let mut k2 = out - 1;
            let e = fuse_cards(&mut p2, 0, 1, &mut k2).expect_err("业力差 1 应当被拒");
            assert!(e.contains("业力不足"), "拒绝理由不是「业力不足」而是「{e}」");
            assert_eq!(k2, out - 1, "被拒的融合动了业力");
        }

        // 「最低 floor」那一档真跑一次：找一张费用＝floor+1 的牌当副牌，0 业力也该放行。
        let cheap = engine_defs()
            .into_iter()
            .filter(|d| d.cost - 1 == floor)
            .find(|d| d.name != name_main)
            .unwrap_or_else(|| panic!("md:{l_rule} 写了「最低{floor}」，引擎卡表里却没有费用＝{} 的牌能跑到这一档", floor + 1));
        let mut p3 = vec![CardInst::new(5, d_main), CardInst::new(6, cheap)];
        let mut k3 = 0;
        fuse_cards(&mut p3, 0, 1, &mut k3).expect("副牌费用-1 落在最低档＝0 业力，0 业力就该融得动");
        assert_eq!(k3, 0, "md:{l_rule}「最低{floor}」＝不收钱，引擎却收了 {floor} 之外的数");

        // 开端不可融合（md:363）：主、副两侧都试。
        let starter = def_by_name("开端", 0);
        for (mi, si) in [(0usize, 1usize), (1, 0)] {
            let mut p4 = vec![CardInst::new(7, starter), CardInst::new(8, d_main)];
            let mut k4 = 99;
            let e = fuse_cards(&mut p4, mi, si, &mut k4).expect_err("开端无论当主牌还是副牌都该被拒");
            assert!(e.contains("开端不可融合"), "拒绝理由不是「开端不可融合」而是「{e}」");
            assert_eq!(k4, 99, "被拒的融合扣了业力");
            assert_eq!(p4.len(), 2, "被拒的融合动了堆");
        }

        // 叠加等式（md:{l_eq}）：同名技能两层，走**真实攻击阶段**看业火到底加几。
        // 文档长标签 → 引擎变体这一格是全仓唯一的**手 link**（没有从长标签到 Skill 的表），
        // 所以先把标签字面钉住：等式换了措辞，这里当场红，不会悄悄拿另一条技能去复现。
        assert_eq!(eq_label, "攻击后自身+1累积", "md:{l_eq} 那道等式的技能名不再是「攻击后自身+1累积」⇒ 本测复现的已经不是它了，先改这里再谈通过");
        assert_eq!(eq_lhs.iter().sum::<i32>(), eq_rhs, "md:{l_eq} 的等式自己算不平：{eq_lhs:?} ≠ {eq_rhs}");
        let sk = Skill::AtkSelfFlame1;
        let mut pa = CardInst::new(9, d_main);
        pa.skills.push(sk);
        let mut pb = CardInst::new(10, d_sub);
        pb.skills.push(sk);
        let mut p5 = vec![pa, pb];
        let mut k5 = (d_sub.cost - 1).max(0);
        fuse_cards(&mut p5, 0, 1, &mut k5).expect("带技能的这一对应当能融");
        assert_eq!(p5[0].skills, vec![sk, sk], "md:332：新牌的技能应是主牌＋副牌逐条串起来");
        let mut b = Battle::new(1, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        b.draw_pile.clear();
        b.discard_pile.clear();
        b.hand.clear();
        b.e_front = [None, None, None, None];
        b.e_back = [None, None, None, None];
        b.p_front = [Some(p5.remove(0)), None, None, None];
        let f_before = b.p_front[0].as_ref().unwrap().flame;
        b.s9_run_player_attack_phase();
        let f_after = b.p_front[0].as_ref().unwrap().flame;
        assert_eq!(f_after - f_before, eq_rhs, "md:{l_eq} 说两层同名技能攻击一次加 {eq_rhs} 累积，引擎加的是 {} ⇒ 叠加要么被去重、要么被多算", f_after - f_before);

        // 文档示例与引擎／卡表的**不一致账**：逐条量出来钉死，不替文档把账做平，也不许悄悄多出一条。
        // 谁改了文档（本仓不改 `中线.MD`）或改了引擎 ⇒ 这份名单红，逼着重新判定。
        let mut doc_gap: Vec<String> = Vec::new();
        if d_main.power != power_main {
            doc_gap.push(format!("md:{l_main} 把「{name_main}」写成数值{power_main}，§七 卡表与引擎里它是 {}", d_main.power));
        }
        if d_main.power != res_power {
            doc_gap.push(format!("md:{l_res} 把新牌写成数值{res_power}，而新牌数值＝主牌数值＝{}", d_main.power));
        }
        if d_sub.power != power_sub {
            doc_gap.push(format!("md:{l_sub} 把「{name_sub}」写成数值{power_sub}，§七 卡表与引擎里它是 {}", d_sub.power));
        }
        if res_cost != d_main.cost {
            doc_gap.push(format!("md:{l_res} 把新牌写成{res_cost} 费，主牌费用是 {}", d_main.cost));
        }
        if trait_sub.contains(res_skill.as_str()) {
            doc_gap.push(format!("md:{l_res} 说新牌多了「技能：{res_skill}」，可 md:{l_sub} 把同一串字写成副牌的**特性**；引擎只搬 skills（md:332），特性不参与融合（md:328）"));
        }
        assert_eq!(
            doc_gap.len(),
            3,
            "§九 示例与引擎／卡表的不一致条数变了（应当恰好 3 条：两处数值、一处把特性当技能）。现在实测到 {} 条：\n{}\n⇒ 有人改了文档或改了引擎，先重新判定这三处该按哪一侧，再改本数字",
            doc_gap.len(),
            doc_gap.join("\n")
        );
    }

    #[test]
    fn upgrade_three_kind_and_cap() {
        let mut inherit = pile(1);
        assert!(upgrade_card(&mut inherit, 0, "power").unwrap().starts_with("升级后："));
        assert!(upgrade_card(&mut inherit, 0, "thr").is_ok());
        assert!(upgrade_card(&mut inherit, 0, "skill").is_ok());
        assert_eq!(inherit[0].upgrades, 3);
        assert_eq!(upgrade_card(&mut inherit, 0, "power").err().unwrap(), "该牌已达升级上限3次");
        assert_eq!(upgrade_card(&mut inherit, 4, "thr").err().unwrap(), "下标无效");
        // 坏 kind 用的是**没升过级**的新牌：上限检查在 kind 检查之前，拿上面那张已升满的牌只会收到上限文案。
        assert_eq!(upgrade_card(&mut pile(1), 0, "pw").err().unwrap(), "power|thr|skill");
    }

    #[test]
    fn survivors_are_capped_at_ten_and_the_squeezed_out_ones_are_reported() {
        // §廿二:947：超上限弃**最早入堆**的那些，且每条都要有可见文案（永久消失，静默＝丢档无凭据）。
        let mut b = Battle::new(11, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        b.draw_pile.clear();
        b.discard_pile.clear();
        b.p_front = [None, None, None, None];
        b.hand = vec![ember(2), ember(4)];
        let mut inherit = pile(10);
        let lines = collect_survivors(&mut b, &mut inherit);
        assert_eq!(inherit.len(), 10, "上限恒 10");
        assert_eq!(lines.len(), 2, "两张幸存者挤掉两张");
        for l in &lines {
            assert!(l.starts_with("继承堆超10张 → 弃置（永久消失）："), "{l}");
        }
        assert_eq!(lines[0], format!("继承堆超10张 → 弃置（永久消失）：{}", short_card(&ember(0))), "弃的是最早入堆那张");
        assert_eq!(inherit[0].def.name, ember(2).def.name, "堆首前移两位");
    }
}

