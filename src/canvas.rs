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
//! 2D 帧的 **SVG 编码器**：把 `display::Frame` 那张呈现指令表编成一份 XML 文档。
//!
//! 形状声明：它只画**静态一帧**——没有动画、没有音效。存在的理由不是"做个好看界面"，
//! 而是给设计交一件**共有的实物**：同一句文字描述有两个读者就有两份想象，一张图只有一份。
//!
//! ## 本层只许做一件事：把指令写成语法
//! "画什么"全部住在 `crate::display`，本模块一行都不参与。这不是分层好看，是**双输出**的前提：
//! 桌面上的 SVG 与真机上的像素必须读同一份决策，否则两边各自自洽、差值读出来不知该怪谁
//! （同一族坑在几何那侧付过学费，见 `touch::layout_of` 的抬头）。所以本层能出现的判断只有
//! "这种指令用哪个标签写"，不能有"这一格该不该画""这个数取哪一档"。
//!
//! 三条上游（口径与 `display` 完全相同，因为本层只碰最后一条）：
//! - **数据**与**几何**：都经 `display::frame_prims` 拿，本模块一个快照字段都不直读。
//! - **版式**：只有 `md3::svg`（XML 声明与命名空间）与 `md3::layout`（虚线段长）两族本层必须点名，
//!   因为它们是**语法**的一部分而不是画面的一个决定；色值、圆角、字号一律由指令表带下来。
//!   `md3::tests::visual_constants_have_one_home` 扫本文件非注释区，裸数与 `#rrggbb` 都算违反。
//!
//! 规则锚：本模块**不写**文档锚点，理由与 `display`／`tui` 那两条相同——编码不新增任何规则真值，
//! 写锚会去动反向覆盖尺的生产面读数。

use crate::display::{self, Anchor, Edge, Frame, Prim};
use crate::md3::{color, layout, svg, typo};
use crate::touch::{Layer, Region};
use crate::view::Board;

/// XML 文本转义。牌名／技能名是 `&'static str`，但快照是数据不是字面量——
/// 转义这一层不许省：哪天一张卡名里出现 `&`，整张图就从"画错了"降级成"后面全不渲染"。
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 一条指令 → 一个节点。**属性顺序是文档形状的一部分**，改动它等于改动每一张已归档的帧文件。
fn emit(p: &Prim) -> String {
    match p {
        Prim::Block { x, y, w, h, radius, fill, edge, opacity } => {
            let mut s = format!(r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{radius}" fill="{fill}""##);
            if let Some(e) = edge {
                s.push_str(&stroke(e));
            }
            if let Some(o) = opacity {
                s.push_str(&format!(r##" opacity="{o}""##));
            }
            s.push_str("/>");
            s
        }
        Prim::Label { x, y, em, fill, anchor, text } => {
            let a = match anchor {
                Anchor::Start => "start",
                Anchor::End => "end",
            };
            format!(r##"<text x="{x}" y="{y}" font-size="{em}" fill="{fill}" text-anchor="{a}">{}</text>"##, esc(text))
        }
    }
}

/// 描边怎么写：`stroke-dasharray` 只在虚线时出现，且排在 `stroke-width` 之后（同从前）。
fn stroke(e: &Edge) -> String {
    let Edge { stroke, width, dashed } = e;
    let dash = if *dashed {
        format!(r##" stroke-dasharray="{} {}""##, layout::DASH_ON, layout::DASH_OFF)
    } else {
        String::new()
    };
    format!(r##" stroke="{stroke}" stroke-width="{width}"{dash}"##)
}

/// 一张指令表 → 一份完整 SVG 文档。**纯函数、零 IO。**
/// 外壳（XML 声明、`<svg>` 根、铺满画布的底色矩形）在编码器这一侧：那是 XML 的写法，不是画面的决定。
fn svg_of(f: &Frame) -> String {
    let mut out = String::new();
    out.push_str(svg::PROLOG);
    out.push('\n');
    out.push_str(&format!(r##"<svg xmlns="{}" width="{}" height="{}" viewBox="0 0 {} {}" font-family="{}">"##, svg::NS, f.width, f.height, f.width, f.height, typo::FONT_FAMILY));
    out.push_str(&format!(r##"<rect width="{}" height="{}" fill="{}"/>"##, f.width, f.height, color::SURFACE));
    for p in &f.prims {
        out.push_str(&emit(p));
    }
    out.push_str("</svg>\n");
    out
}

/// 一帧盘面 → 一张 SVG 文档。`seed` 与 `scene` 只进标题行（口径见 `display::frame_prims`）。
pub(crate) fn svg_frame(v: &Board, seed: u64, scene: &str) -> String {
    svg_frame_marked(v, seed, scene, &[])
}

/// 同一帧，但把控件层要盖的态层画上（`marks` 由 `touch::Picker::marks` 按优先级升序给出）。
/// `marks` 为空 ⇒ 与 `svg_frame` **逐字节**相同：控件层不许反过来污染呈现真值，
/// 那件事由 `touch::tests::the_control_layer_adds_nothing_when_nothing_is_pressed` 钉住，
/// 而改前／改后两批帧文件的 md5 对撞记在 档 §2。
pub(crate) fn svg_frame_marked(v: &Board, seed: u64, scene: &str, marks: &[(Region, Layer)]) -> String {
    svg_of(&display::frame_prims(v, seed, scene, marks))
}

#[cfg(test)]
mod tests {
    use super::layout::*;
    use super::typo::*;
    use super::*;
    use crate::md3::lane;
    use crate::md3::shape::*;
    use crate::battle::{Battle, Difficulty, Row};
    use crate::boss::BossId;
    use crate::model::{faction_cards, CardInst, Faction};
    use crate::touch::Rect;
    use crate::view::CardView;

    fn normal() -> Battle {
        Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1)
    }

    /// 编码器只许吃快照：格子里出现的每个数，都必须能在**同一份 `Board`** 里逐字找到。
    /// 断言一律拿快照的实际字段去撞（不硬编码名字/数字），所以引擎文案改了不会误红，
    /// 而"编码器自己造了一套数"这种形状当场红。
    #[test]
    fn every_number_on_the_frame_comes_from_the_snapshot() {
        let mut b = normal();
        b.e_karma = 50;
        let held = CardInst::new(900, faction_cards(Faction::Ember)[1]);
        let held_name = held.def.name;
        b.enemy_place_inst(held, 0, Row::Front).unwrap();
        let v = b.view();
        let frame = svg_frame(&v, 7, "test");

        assert!(frame.contains(held_name), "在场牌名要出现：{held_name}");
        assert!(frame.contains(&format!("{} / {}", v.enemy_front[0].as_ref().unwrap().hp, v.enemy_front[0].as_ref().unwrap().threshold)), "值/阈值那一对要用快照的实际值");
        assert!(frame.contains(&format!("第 {} 回合", v.player.turn)));
        assert!(frame.contains(&format!("业力 {}", v.player.karma)));
        assert!(frame.contains(&format!("继承堆 {} 张", v.draw_pile_len)));
        for c in &v.hand {
            assert!(frame.contains(c.name), "每张手牌都要画出来（漏一张＝手牌行只画了标题）：{}/{}", c.name, v.hand.len());
        }
    }

    /// 纯函数：同一份快照连编两次必须逐字节相同。帧文件一旦可比 md5，"同 seed 同帧"这条口径才立得住。
    #[test]
    fn the_encoder_is_pure() {
        let b = normal();
        let v = b.view();
        assert_eq!(svg_frame(&v, 7, "same"), svg_frame(&v, 7, "same"), "同快照同标题 ⇒ 同字节");
        assert_ne!(svg_frame(&v, 7, "a"), svg_frame(&v, 8, "a"), "换 seed 要换字节（标题带 seed）");
    }

    /// 双柱 Boss 两根都要画，且第二根不许带业力（快照给 None，编码器不许自己补一个数）。
    #[test]
    fn dual_holder_draws_both_bars_and_inherits_the_karma_absence() {
        let b = Battle::new_boss(2026, Faction::Ember, BossId::YanBing, Vec::new(), 48);
        let v = b.view();
        let frame = svg_frame(&v, 2026, "boss");
        assert_eq!(v.enemy_holders.len(), 2);
        assert!(frame.contains(v.enemy_holders[0].name), "第一柱名要出现");
        assert!(frame.contains(v.enemy_holders[1].name), "第二柱名也要出现（画漏一根就成了一柱图）");
        assert!(frame.contains("直击此柱"), "第二根的受击列区要带上");
    }

    /// 转义不是装饰：快照里出现 XML 元字符时，图必须还是合法文档。
    /// `Board` 的字段是 `pub(crate)`，所以这里能直接造一张带 `&`/`<` 的假牌——真实卡表里没有这种名字，
    /// 但编码器不该依赖"卡表永远干净"这个它自己看不见的性质。
    #[test]
    fn snapshot_text_is_escaped_not_interpolated() {
        let mut v = normal().view();
        let mut c = CardView {
            name: "甲&乙<丙>",
            cost: 1,
            hp: 2,
            threshold: 3,
            flame: 0,
            upgrades: 0,
            crafted: false,
            starter: false,
            trait_label: "无",
            skills: vec![],
        };
        v.hand.push(c.clone());
        c.name = "丁\"戊";
        v.player_front[0] = Some(c);
        let frame = svg_frame(&v, 1, "escape");
        assert!(frame.contains("甲&amp;乙&lt;丙&gt;"), "手牌里的元字符要转义后出现");
        assert!(frame.contains("丁&quot;戊"), "在场格同样转义");
        assert!(!frame.contains("甲&乙<"), "原始元字符不许进文档（那会撕掉后面的节点）");
    }

    /// 一帧里格子的总数＝盘面 12 格，空格也要画格子（虚线框）。
    /// 列数从快照长度取，所以这条钉的是"取长度这条路真的走通了"：少画一格不会报错，只会静默变成一张缺一列的图。
    #[test]
    fn the_board_draws_one_cell_per_slot_not_per_hardcoded_count() {
        let v = normal().view();
        let frame = svg_frame(&v, 7, "grid");
        let slots = v.enemy_back.len() + v.enemy_front.len() + v.player_front.len();
        assert_eq!(slots, 12, "盘面固定 4 列 × 3 排");
        let empties = v.enemy_back.iter().chain(v.enemy_front.iter()).chain(v.player_front.iter()).filter(|c| c.is_none()).count();
        assert_eq!(frame.matches("stroke-dasharray").count(), empties, "虚线框数＝空格数（少一个＝有一格没画）");
        let cell_box = format!(r##"height="{CELL_H}""##);
        assert_eq!(frame.matches(&cell_box).count(), slots, "格子总数＝槽位数（空格也要画框）");
    }

    /// Boss 规则句**整句**要进图：折行可以，丢字不行。那句是规则本体，不是装饰文案。
    /// 折行本身住在 `touch::layout_of`（行数决定后面所有 y），这条从帧那侧证明一个字没丢。
    #[test]
    fn the_boss_rule_line_loses_no_characters() {
        let b = Battle::new_boss(2026, Faction::Ember, BossId::YanBing, Vec::new(), 48);
        let v = b.view();
        let rule = v.rule.as_ref().expect("炎与冰有特殊规则");
        let want = esc(&format!("特殊规则【{}】{}", rule.label, rule.text));
        let frame = svg_frame(&v, 2026, "boss");
        // 只认 `<text>` 标签自己带这个颜色：整段文档里中线那条 `<rect>` 也是同一个色值，
        // 拿"包含"去筛会把那条色带也算成规则行——筛错一侧，这条测就成了永真。
        let tag = format!(r##"font-size="{TITLE_LARGE}" fill="{midline}""##, midline = lane::MIDLINE);
        let got: String = frame
            .split("<text ")
            .filter_map(|t| {
                let (head, rest) = t.split_once('>')?;
                head.contains(&tag).then(|| rest.split('<').next().unwrap_or(""))
            })
            .collect();
        assert_eq!(got, want, "把规则行的各段拼回去要逐字等于原句（少字＝被裁，多字＝串行）");
        assert!(frame.matches(&tag).count() > 1, "这句长到必须折行（本条测的就是折行不丢字）");
    }

    /// 版式常数不许在这一文件里各写一遍：帧上的每个色都必须能在 `md3` 里找到同名 token。
    /// 这条与 `md3` 那条机检成对——那条从"表"这侧数漏出来的字面量，这条从"帧"这侧数点名的 token 真的进了文档没有。
    #[test]
    fn the_frame_paints_with_the_tokens_it_names() {
        let v = normal().view();
        let frame = svg_frame(&v, 7, "tokens");
        for (name, token) in [("ON_SURFACE", color::ON_SURFACE), ("OUTLINE_VARIANT", color::OUTLINE_VARIANT), ("SURFACE", color::SURFACE), ("lane::MIDLINE", lane::MIDLINE)] {
            assert!(frame.contains(token), "{name} 没进这一帧：版式表与编码器已经分叉");
        }
        let default_corner = format!(r##"rx="{}""##, SMALL);
        assert!(frame.contains(&default_corner), "格子圆角没取 shape::SMALL：有人改了默认角却没改表");
        let unused_corner = format!(r##"rx="{}""##, EXTRA_LARGE);
        assert!(!frame.contains(&unused_corner), "帧里冒出了表里没派上用场的 {unused_corner} 圆角");
    }

    /// **本层存在的全部理由**那一格的另一半：编码器不许自己增删画面。
    /// 指令表里几条，文档里就有几个节点，一块对一块——尺寸与颜色逐条撞，不是"整体看起来一样"。
    /// 从前这两件事写在同一个函数里所以无从分离，这条检也就无从成立；分层之后它才有对象。
    #[test]
    fn the_document_has_exactly_one_node_per_instruction() {
        let b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        let v = b.view();
        let f = crate::display::frame_prims(&v, 7, "one-to-one", &[]);
        let frame = svg_of(&f);
        let rects = frame.matches("<rect ").count();
        let texts = frame.matches("<text ").count();
        let blocks = f.prims.iter().filter(|p| matches!(p, Prim::Block { .. })).count();
        let labels = f.prims.iter().filter(|p| matches!(p, Prim::Label { .. })).count();
        // 外壳自己那一块铺底矩形不算指令（它是文档形状的一部分，口径见 `svg_of` 抬头）。
        assert_eq!(rects, blocks + 1, "块数对不上：文档 {rects}／指令 {blocks}＋外壳 1");
        assert_eq!(texts, labels, "字数对不上：文档 {texts}／指令 {labels}");
        for p in &f.prims {
            let node = emit(p);
            assert!(frame.contains(&node), "这条指令没被原样写进文档：{node}");
        }
    }

    /// 右对齐那条锚点要真的落到 XML 上：`Anchor::End` ⇒ `text-anchor="end"`，反过来也一样。
    /// 这一件在像素侧是"先量宽再往回让"，在语法侧是一个属性——两份实现是否等价，只有对撞知道，
    /// 但**属性有没有丢**是本层自己能当场证的，所以留在这儿。
    #[test]
    fn both_anchors_survive_the_encoding() {
        let f = Frame {
            width: 1,
            height: 1,
            prims: vec![
                Prim::Label { x: 10, y: 20, em: BODY_LARGE, fill: lane::META.to_string(), anchor: Anchor::Start, text: "左".to_string() },
                Prim::Label { x: 10, y: 20, em: BODY_LARGE, fill: lane::META.to_string(), anchor: Anchor::End, text: "右".to_string() },
            ],
        };
        let out = svg_of(&f);
        // 期望串由 token 拼出，不抄色值——抄来的那份在 token 改了以后照样绿，那是给旧读数背书。
        let at = format!(r##"x="10" y="20" font-size="{BODY_LARGE}" fill="{meta}""##, meta = lane::META);
        assert!(out.contains(&format!(r##"{at} text-anchor="start">左</text>"##)), "start 锚丢了：{out}");
        assert!(out.contains(&format!(r##"{at} text-anchor="end">右</text>"##)), "end 锚丢了");
    }

    /// 空格那格的虚线在 SVG 里是一串属性、在像素里是一段一段的描边可画区。
    /// 本层只钉"这条串写在 `stroke-width` 之后、段长取自表"，因为它一旦写成别的次序，
    /// 归档里那 17 个帧文件的 md5 会整批漂（尺见 `scripts/byte-baseline.sh`）。
    #[test]
    fn the_dash_array_is_written_after_the_width_with_table_values() {
        let cell = Rect::new(0, 0, 100, 100);
        let p = Prim::Block {
            x: cell.x,
            y: cell.y,
            w: cell.w,
            h: cell.h,
            radius: SMALL,
            fill: lane::EMPTY_FILL.to_string(),
            edge: Some(Edge { stroke: color::OUTLINE_VARIANT.to_string(), width: STROKE_BOUNDARY, dashed: true }),
            opacity: None,
        };
        let want = format!(r##"<rect x="0" y="0" width="100" height="100" rx="{SMALL}" fill="{}" stroke="{}" stroke-width="{STROKE_BOUNDARY}" stroke-dasharray="{DASH_ON} {DASH_OFF}"/>"##, lane::EMPTY_FILL, color::OUTLINE_VARIANT);
        assert_eq!(emit(&p), want, "属性次序或段长漂了");
    }
}
