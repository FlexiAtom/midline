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
//! 战斗状态机：业力 / 献祭 / 放置挤压 / 抽牌 / 我方立即结算攻击 /
//! 敌方累积攻击 + 统一结算 + 伤害回滚 / 业火触发 / 推进 / 死亡返还递减 / 保底 / 30回合。
//! 规则歧义解释见 model.rs 模块注释。

use crate::model::{CardDef, CardInst, Faction, Skill, TraitKind, faction_cards, short_card};
use crate::rng::Rng;

pub const CANDLE_HP: i32 = 20;  // §廿三:1012 持业者 HP 20（Boss 用 profile 覆写，见 boss.rs）；§十四:591 我方持业者初始长度 20 单位；§十四:596 敌方持业者初始长度 20 单位——两侧读同一常量，这就是"对称"的数值面
pub const HAND_LIMIT: usize = 8;
pub const TURN_LIMIT: i64 = 30;
/// 章强化的封顶档数 = §十一:404「每张卡牌最多升级3次」。
pub const CHAPTER_STRENGTH_CAP: u32 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    PlayerWin,
    PlayerLose,
    Draw,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeathCause {
    Battle,
    /// §十七:763 「任何卡牌不能越过中线」在类型层的落点：越线只有"死"这一种后果，枚举里**不存在**
    /// "越过之后落到对面／落到更前一格"这类第四因。残留盲区照旧：这条是**否定式条款**，
    /// 锚点指向的是"这里没有某个变体"，机检证伪不了（同 §十五:621），能证伪的部分见
    /// `a_squeezed_card_dies_at_its_own_line_and_never_reaches_the_other_side`。
    Cross,
    Sacrifice,
}

/// 击持业者的两种入口（文案不同：直击 / 超额分配）。
#[derive(Clone, Copy)]
enum HolderHit {
    Direct,
    Excess(i32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
    Expert,
}

impl Difficulty {
    pub fn parse(s: &str) -> Option<Difficulty> {
        match s {
            "easy" => Some(Difficulty::Easy),
            "normal" => Some(Difficulty::Normal),
            "hard" => Some(Difficulty::Hard),
            "expert" => Some(Difficulty::Expert),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "简单",
            Difficulty::Normal => "普通",
            Difficulty::Hard => "困难",
            Difficulty::Expert => "专家",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SideK {
    Player,
    Enemy,
}

impl SideK {
    pub fn other(self) -> SideK {
        match self {
            SideK::Player => SideK::Enemy,
            SideK::Enemy => SideK::Player,
        }
    }
}

fn refund_pct(deaths_before: u32) -> i32 {  // §廿三:984 递减档 100/50/25/10%，入参是每张牌实例自己的死亡数
    match deaths_before {
        0 => 100,
        1 => 50,
        2 => 25,
        _ => 10,
    }
}

#[derive(Default, Clone)]
pub struct SideFlags {
    pub sacrifice_used: bool,
    pub sacrificed_names: Vec<&'static str>,
    pub manual_draws: i32,
    pub starter_draws: i32,
    pub starter_gains: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    /// 我方唯一行 / 敌方前排 E5-E8
    Front,
    /// 敌方后排 E1-E4
    Back,
}

#[derive(Clone)]
pub struct Battle {
    pub rng: Rng,
    pub next_id: u64,
    pub seq: u64,
    pub turn: i64,
    pub difficulty: Difficulty,
    pub over: Option<Outcome>,
    pub log: Vec<String>,

    pub player_faction: Faction,

    pub p_karma: i32, // §七:263 业力＝资源，用于放置卡牌/融合（消耗点：place 与 fuse_cards）
    pub e_karma: i32,
    pub p_candle: i32,
    pub e_candle: i32,

    pub p_front: [Option<CardInst>; 4],
    pub e_back: [Option<CardInst>; 4],
    pub e_front: [Option<CardInst>; 4],

    pub hand: Vec<CardInst>,
    pub draw_pile: Vec<CardInst>,
    pub starter_pile: u32,  // §廿三:994 开端堆＝剩余可抽次数，抽出的恒为开端
    pub discard_pile: Vec<CardInst>,
    pub enemy_hand: Vec<CardInst>,
    pub enemy_pile: Vec<CardInst>,
    pub e_discard: Vec<CardInst>,

    pub pf: SideFlags,
    pub ef: SideFlags,

    /// §十五:638 回滚代价之二＝下回合业力-1。这里只**登记**，落地在 `player_turn_start`（含"最低0"的钳位）。
    pub karma_penalty_next: i32,
    /// §廿二:955 伤害回滚每局最多触发2次（我方每局独立计数，不跨关累积）。§廿三:1017 同一预算的速查复述。
    /// §十五:612 同一预算的原文触发条件第4条：初始 2、每次触发在本函数之外扣 1。
    pub rollback_left: i32,
    /// §十五:618 口径 A＝"我方本回合攻击阶段已打出的伤害总和"，回滚时当作抵消额度。
    /// §十五:621 每回合开始清零（`player_turn_start`）⇒ 文档那句"不包括上一回合遗留的伤害"是靠这个清零兑现的。
    pub dealt_this_turn: i32,
    pub in_player_attack_phase: bool,
    /// 守夜人式"本回合友方累积+N"增益窗口：(阵营, 生效回合, 增量)。
    pub boosts: Vec<(SideK, i64, i32)>,
    pub attack_order: Vec<u64>,
    /// §十五:611 伤害在 pending 里躺着＝文档触发条件第3条"伤害尚未结算"；统一结算才 `take` 走它。
    pub pending_candle_d: i32,
    pub pending_card_d: Vec<(usize, i32)>,

    // ---------- Boss 遭遇轴（与难度档正交；None ⇒ 下面各项全为默认，普通对局零影响） ----------
    /// 本局 Boss（脚本 + 特殊规则的真值来源）。
    pub boss: Option<crate::boss::BossId>,
    /// Some ⇒ 双持业者（炎与冰）；`e_candle` 恒为 1 号。
    pub e_candle2: Option<i32>,
    /// 持业者称呼（render / 直击文案用），默认 ["敌方持业者", ""]。
    pub holder_names: [&'static str; 2],
    /// 回合上限（默认 30；字段留给后续模式，Boss 不改）。
    pub turn_limit: i64,
}


impl Battle {
    pub fn new(
        rng_seed: u64,
        player_faction: Faction,
        enemy_faction: Faction,
        difficulty: Difficulty,
        inherit: Vec<CardInst>,
        level: u32,
    ) -> Self {
        let mut rng = Rng::seeded(rng_seed ^ (level as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut id = 1u64;
        let mk = |id: &mut u64, rng: &mut Rng, def: CardDef, skill: bool| -> CardInst {  // §廿三:998 卡牌＝固定特性(def.tr)＋随机附加技能，在这里合成
            let mut c = CardInst::new(*id, def);
            *id += 1;
            if skill && !c.is_starter() {
                let pool = Skill::list();
                let s = pool[rng.below(pool.len())];
                c.skills.push(s);
            }
            c
        };
        let mut draw_pile: Vec<CardInst>;
        // §廿二:966 开局手牌来源：第1关从基础牌堆抽，第2关起从继承堆抽。
        // `inherit.is_empty()` 这一支是**静默回落**——跳关/新开打第2关以上时，头报写着第N关、牌序却是第1关的。
        // 存档包（meta-main）因此要求载入端先校验阵营与关卡一致性，不许拿空堆假装续关。
        if level <= 1 || inherit.is_empty() {
            let mut pool: Vec<CardDef> = faction_cards(player_faction)[1..].to_vec();
            rng.shuffle(&mut pool);
            draw_pile = pool.into_iter().map(|d| mk(&mut id, &mut rng, d, true)).collect();
        } else {
            draw_pile = inherit;  // §廿三:993 继承堆即牌库；上限10见 progress.rs:87，开端不入堆见 battle.rs:1457
            for c in draw_pile.iter_mut() {
                c.hp = c.def.power; // 每关血量重置满格（升级已写入实例定义）
                c.flame = 0;
                c.seq = 0;
                c.placed_turn = i64::MIN;
                c.triggered_turn = i64::MIN;
            }
            // 开局手牌：继承堆不足3张 → 从基础牌堆补齐
            if draw_pile.len() < 3 {
                let mut pool: Vec<CardDef> = faction_cards(player_faction)[1..].to_vec();
                rng.shuffle(&mut pool);
                let need = 3 - draw_pile.len();
                draw_pile.extend(pool.into_iter().take(need).map(|d| mk(&mut id, &mut rng, d, true)));
            }
        }
        let mut enemy_pile: Vec<CardInst> = {
            let mut defs: Vec<CardDef> = faction_cards(enemy_faction)[1..].to_vec();
            rng.shuffle(&mut defs);
            defs.into_iter().map(|d| mk(&mut id, &mut rng, d, true)).collect()
        };

        let p_starter = mk(&mut id, &mut rng, faction_cards(player_faction)[0], false);
        let e_starter = mk(&mut id, &mut rng, faction_cards(enemy_faction)[0], false);

        let mut hand = vec![p_starter];  // §十二:421 手牌＝开端固定发放（每关 1 张，不从堆里抽）
        for _ in 0..3 {  // §十二:421 开局从继承堆抽 3 张（裁定11：按堆顶顺序取）；§廿三:992 开局手牌（固定开端＋3张，不经 manual_draws）
            if !draw_pile.is_empty() {
                hand.push(draw_pile.remove(0)); // 开局手牌按继承堆顺序取（裁定11）
            }
        }
        let mut enemy_hand = vec![e_starter];
        for _ in 0..3 {
            if !enemy_pile.is_empty() {
                let i = rng.below(enemy_pile.len());
                enemy_hand.push(enemy_pile.remove(i));
            }
        }

        let mut b = Battle {
            rng,
            next_id: id,
            seq: 1,
            turn: 0,
            difficulty,
            over: None,
            log: vec![format!(
                "— 战斗开始：我方【{}】 vs 敌方【{}】 第{level}关 —",
                player_faction.name(),
                enemy_faction.name()
            )],
            player_faction,
            p_karma: 0,  // §十二:422 战斗开始业力 0；§十二:504 进入下一关重新起算；§廿三:982 业力是唯一资源，不自动恢复（回合开始无任何补给）
            e_karma: 0,
            p_candle: CANDLE_HP,  // §十四:590 我方持业者一侧的起点
            e_candle: CANDLE_HP,  // §十四:595 敌方持业者一侧的起点
            p_front: Default::default(),  // §十二:423 战斗开始场上为空
            e_back: Default::default(),
            e_front: Default::default(),
            hand,
            draw_pile,
            starter_pile: 1,
            discard_pile: Vec::new(),
            enemy_hand,
            enemy_pile,
            e_discard: Vec::new(),
            pf: SideFlags { manual_draws: 2, starter_draws: 1, ..Default::default() },
            ef: SideFlags { manual_draws: 2, starter_draws: 1, ..Default::default() },
            karma_penalty_next: 0,
            rollback_left: 2,
            dealt_this_turn: 0,
            in_player_attack_phase: false,
            boosts: Vec::new(),
            attack_order: Vec::new(),
            pending_candle_d: 0,
            pending_card_d: Vec::new(),
            boss: None,
            e_candle2: None,
            holder_names: ["敌方持业者", ""],
            turn_limit: TURN_LIMIT,
        };
        b.player_turn_start();  // §廿三:978 我方先手：战斗一开局就进我方回合
        b
    }

    /// Boss 战构造器（不改 `new` 签名 ⇒ 既有调用点与测试零改动）。
    pub fn new_boss(rng_seed: u64, player_faction: Faction, boss: crate::boss::BossId, inherit: Vec<CardInst>, level: u32) -> Self {
        let p = boss.profile();
        let mut b = Battle::new(rng_seed, player_faction, p.faction, Difficulty::Normal, inherit, level);
        b.boss = Some(boss);
        b.e_candle = p.holder_hp;
        b.e_candle2 = p.holder_hp2;
        b.holder_names = p.holder_names;
        b.turn_limit = TURN_LIMIT;
        b.e_karma = p.start_karma;
        b.log.push(format!("— Boss 登场：{}·{} —", p.name, p.title));
        b.log.push(format!("特殊规则【{}】：{}", p.rule.label(), p.rule_text));
        b
    }

    pub fn boss_profile(&self) -> Option<&'static crate::boss::BossProfile> {
        self.boss.map(|b| b.profile())
    }

    pub fn boss_rule(&self) -> crate::boss::BossRule {
        self.boss_profile().map_or(crate::boss::BossRule::None, |p| p.rule)
    }

    /// 敌方"较长的那根蜡烛"（30 回合判定、我方烛尽是否翻成平局都用它；双烛取和会形同必败，故取 max）。
    pub fn enemy_candle_ref(&self) -> i32 {
        match self.e_candle2 {
            None => self.e_candle,
            Some(c2) => self.e_candle.max(c2),
        }
    }

    /// §廿一:914「每章新阵营」＋§廿一「强化」（裁定24 人授权"数值先写，后续有问题再改"）。
    /// 强化量取 `章号-1`，上限直接复用 §十一:404 的"每张卡牌最多升级3次" ⇒ 第1/2/3/4章＝0/1/2/3，第4章起平台。
    pub fn chapter_strength(level: u32) -> u32 {
        (crate::boss::chapter_of(level) - 1).min(CHAPTER_STRENGTH_CAP)
    }

    /// 把章强化落到敌方牌面上：三档累积，逐档对应 §十一:403 的三种升级效果。
    /// 开端卡不吃——持业者是蜡烛本体，强化它会顺带动到烛尽判定。
    /// 阈值档只对 `is_threshold_trait` 的卡生效：每阵营 12 张里只有 5 张带阈值特性，
    /// 无门控时另外 7 张改的是行为读取点（`battle.rs:1265`）永不看的字段，属空转。
    /// 技能档取"给该卡已有技能再叠一层"：不按 `c.id` 索引技能池——id 随 `next_id` 分配漂移，
    /// 同一张牌换一关就会拿到不同技能，那不是强化而是换身份。
    /// 幂等：`upgrades` 记为已强化档数，重复调用不再叠加（§十一:404 的计数器由此真正生效）。
    /// **不掷 rng** ⇒ 同 seed 逐帧复现不破；第1章强化量为 0 时整函数是空操作 ⇒ 首关与旧行为逐字节相同。
    pub fn apply_chapter_strengthening(&mut self, level: u32) {
        let s = Self::chapter_strength(level);
        if s == 0 {
            return;
        }
        let mut touched = 0usize;
        let (mut dp, mut dt, mut ds) = (0usize, 0usize, 0usize);
        for c in self.enemy_hand.iter_mut().chain(self.enemy_pile.iter_mut()) {
            if c.is_starter() || c.upgrades >= s as u8 {
                continue;
            }
            if s >= 1 {
                c.def.power += 1;
                c.hp = c.def.power;
                dp += 1;
            }
            if s >= 2 && is_threshold_trait(c.def.tr) {
                c.def.threshold = (c.def.threshold - 1).max(1);
                dt += 1;
            }
            if s >= 3 && let Some(sk) = c.skills.first().copied() {
                c.skills.push(sk);
                ds += 1;
            }
            c.upgrades = s as u8;
            touched += 1;
        }
        if touched == 0 {
            return;
        }
        let ch = crate::boss::chapter_of(level);
        self.log.push(format!(
            "— 第{ch}章·敌方强化 {touched} 张：数值+1 共{dp}｜阈值-1 共{dt}｜技能+1层 共{ds} —"
        ));
    }

    /// 主线/普通爬关的构造函数：建好即应用章强化，调用点不必记得单独调一次。
    /// Boss 关不走这里（豁免理由见 `meta::one_level`）。
    pub fn new_mainline(
        rng_seed: u64,
        player_faction: Faction,
        enemy_faction: Faction,
        difficulty: Difficulty,
        inherit: Vec<CardInst>,
        level: u32,
    ) -> Self {
        let mut b = Self::new(rng_seed, player_faction, enemy_faction, difficulty, inherit, level);
        b.apply_chapter_strengthening(level);
        b
    }

    fn make_card(&mut self, def: CardDef, with_skill: bool) -> CardInst {
        let mut c = CardInst::new(self.next_id, def);
        self.next_id += 1;
        if with_skill && !c.is_starter() {
            let pool = Skill::list();
            let s = pool[self.rng.below(pool.len())];
            c.skills.push(s);
        }
        c
    }

    // ---------- 抽牌 ----------

    /// §廿二:948 手牌上限 8 张，超出弃置**最早进入手牌**的牌（`hand` 尾插 ⇒ 堆头即最早）；§十二:429 满 8 张时弃置最早进入手牌的牌。
    /// §廿二:949 弃的牌进弃牌堆、本关不再使用（自造牌例外：任何离场永久消失，§十:372）；§廿三:996 弃牌堆的定义行。
    fn push_hand(&mut self, c: CardInst) {
        self.hand.push(c);
        while self.hand.len() > HAND_LIMIT {
            let old = self.hand.remove(0);
            if old.crafted {
                self.log.push(format!("手牌溢出8张：自造牌 {} 被弃永久消失", short_card(&old)));
            } else {
                self.log.push(format!("手牌溢出8张，弃置最早的 {}（弃牌堆，下关回归）", short_card(&old)));
                self.discard_pile.push(old);
            }
        }
    }

    pub fn player_turn_start(&mut self) {  // §十二:426 我方回合「抽牌」步的入口
        self.turn += 1; // §十六:681 跨回合保留：回合推进只动 turn，这里没有 flame 重置（否定式锚，同 §十五:621 一类）
        if self.karma_penalty_next > 0 {
            let before = self.p_karma;
            self.p_karma = (self.p_karma - self.karma_penalty_next).max(0);
            if before != self.p_karma {
                self.log.push(format!("回滚代价：本回合业力-{}", before - self.p_karma));
            }
        }
        self.karma_penalty_next = 0;
        if !self.draw_pile.is_empty() {  // §十二:427 每回合自动从继承堆抽 1 张
            let mut c = self.draw_pile.remove(0); // 堆顶抽取，继承堆顺序有意义（裁定11）
            if c.skills.is_empty() && !c.is_starter() {
                let pool = Skill::list();
                let s = pool[self.rng.below(pool.len())];
                c.skills.push(s);
            }
            self.log.push(format!("回合{}·自动抽牌：{}", self.turn, short_card(&c)));
            self.push_hand(c);
        } else {
            self.log.push(format!("回合{}·继承堆已空，无法抽牌（开端堆仍可抽）", self.turn));
        }
        self.pf.manual_draws = 2;  // §十二:428 主动抽牌每回合 2 次，两堆可混合来源
        self.pf.starter_draws = 1;
        self.pf.sacrifice_used = false;
        self.pf.sacrificed_names.clear();
        // §十五:637 "触发后：本回合已打出的伤害全部消耗"——引擎里 A 是一次性额度：每个敌方攻击阶段只结算一次
        // （`enemy_settle` 在 battle.rs:849 被调用一次），A 在本回合用完后于下回合开始处清零，不会带给第二次回滚。
        self.dealt_this_turn = 0;
        self.attack_order.clear();
        // §廿二:951 手牌为0且场上无卡 → 免费补1张开端（走 `grant_free_starter`，不占 `manual_draws` 额度）。
        if self.hand.is_empty() && self.p_front.iter().all(|s| s.is_none()) {
            self.grant_free_starter();
        }
    }

    /// §廿二:951 保底补开端；§廿二:952 开端堆为空时自动生成1张临时开端（不退还堆计数）。
    fn grant_free_starter(&mut self) {
        if self.starter_pile > 0 {
            self.starter_pile -= 1;
            self.log.push("保底机制：免费抽1张开端".to_string());
        } else {
            self.log.push("保底机制：开端堆为空，自动生成1张临时开端".to_string());
        }
        let def = faction_cards(self.player_faction)[0];
        let c = self.make_card(def, false);
        self.push_hand(c);
    }

    /// §廿二:968 继承堆耗尽 → `di` 只能失败，牌仍可走 `ds` 从开端堆抽；
    /// §廿二:952 开端堆为空 → 自动生成1张临时开端（不扣堆计数）。
    pub fn action_draw(&mut self, from_starter_pile: bool) -> Result<(), String> {
        if self.pf.manual_draws <= 0 {
            return Err("本回合主动抽牌次数已用完（每回合2次，可混合来源）".into());
        }
        if from_starter_pile {
            if self.pf.starter_draws <= 0 {
                return Err("开端堆每回合只能抽1次".into());
            }
            self.pf.starter_draws -= 1;
            self.pf.manual_draws -= 1;
            if self.starter_pile == 0 {
                self.log.push("开端堆为空 → 自动生成1张临时开端".to_string());
            } else {
                self.starter_pile -= 1;
            }
            let def = faction_cards(self.player_faction)[0];
            let c = self.make_card(def, false);
            self.push_hand(c);
            Ok(())
        } else {
            if self.draw_pile.is_empty() {
                return Err("继承堆已空，只能从开端堆抽牌".into());
            }
            self.pf.manual_draws -= 1;
            let mut c = self.draw_pile.remove(0); // 堆顶抽取（裁定11）
            if c.skills.is_empty() && !c.is_starter() {
                let pool = Skill::list();
                let s = pool[self.rng.below(pool.len())];
                c.skills.push(s);
            }
            self.push_hand(c);
            Ok(())
        }
    }

    // ---------- 献祭 ----------

    /// 场上献祭（P 格 0..3）：每回合1次、在场≥1回合、全额不递减、禁同名牌回置。§十二:431 献祭己方 P1-P4、在场 1 回合以上、全额不递减。
    /// §廿二:964 的"阵营限制"（§四:160 不能献祭敌方阵营的卡）由 API 形状保证：只有我方 `p_front`
    /// 与 `hand` 可寻址，敌方卡传不进来，故无需运行期检查。
    pub fn player_sacrifice_field(&mut self, col: usize) -> Result<(), String> {
        if col >= 4 {
            return Err("格位为 P1-P4".into());
        }
        if self.pf.sacrifice_used {
            return Err("本回合献祭次数已用完（每回合最多1次）".into());
        }
        let c = self.p_front[col].take().ok_or("该格没有卡牌")?;
        if self.turn - c.placed_turn < 1 {  // §十二:431 在场不足 1 回合不可献祭
            self.p_front[col] = Some(c);
            return Err("在场不足1回合，不可献祭".into());
        }
        self.pf.sacrifice_used = true;
        self.pf.sacrificed_names.push(c.def.name);
        self.log.push(format!("献祭（场上）{}", c.def.name));
        self.on_death(c, SideK::Player, Some(col), DeathCause::Sacrifice);
        Ok(())
    }

    /// 手牌献祭：不受在场限制；次数豁免**只适用开端**（裁定10，人裁定 2026-09-28）——
    /// 普通手牌献祭仍占每回合1次额度。
    pub fn player_sacrifice_hand(&mut self, idx: usize) -> Result<(), String> {
        if idx >= self.hand.len() {
            return Err("手牌下标越界".into());
        }
        let is_starter = self.hand[idx].is_starter();  // §廿三:991 手牌献开端不占每回合献祭额度（裁定10）
        if !is_starter {
            if self.pf.sacrifice_used {
                return Err("本回合献祭次数已用完（每回合最多1次，开端手牌献祭除外）".into());
            }
            self.pf.sacrifice_used = true;
        }
        let c = self.hand.remove(idx);
        self.pf.sacrificed_names.push(c.def.name);
        self.log.push(format!("献祭（手牌）{}", c.def.name));
        self.on_death(c, SideK::Player, None, DeathCause::Sacrifice);
        Ok(())
    }

    pub fn enemy_sacrifice_field(&mut self, row: Row, col: usize) -> Result<(), String> {
        if self.ef.sacrifice_used {
            return Err("敌方本回合献祭已用".into());
        }
        let slot = match row {
            Row::Front => &mut self.e_front[col],
            Row::Back => &mut self.e_back[col],
        };
        let c = slot.take().ok_or("敌方格为空")?;
        if self.turn - c.placed_turn < 1 {
            *slot = Some(c);
            return Err("敌方在场不足1回合".into());
        }
        self.ef.sacrifice_used = true;
        self.ef.sacrificed_names.push(c.def.name);
        self.log.push(format!("敌方献祭（{}）{}", if row == Row::Front { "前排" } else { "后排" }, c.def.name));
        self.on_death(c, SideK::Enemy, Some(col), DeathCause::Sacrifice);
        Ok(())
    }

    /// 敌方手牌献祭（供 AI 开局决策：业力0 且手牌有开端 → 献祭开端）。
    pub fn enemy_sacrifice_hand(&mut self, idx: usize) -> Result<(), String> {
        if idx >= self.enemy_hand.len() {
            return Err("敌方手牌越界".into());
        }
        let is_starter = self.enemy_hand[idx].is_starter();
        if !is_starter {
            if self.ef.sacrifice_used {
                return Err("敌方本回合献祭次数已用完（开端手牌献祭除外）".into());
            }
            self.ef.sacrifice_used = true;
        }
        let c = self.enemy_hand.remove(idx);
        self.ef.sacrificed_names.push(c.def.name);
        self.log.push(format!("敌方献祭（手牌）{}", c.def.name));
        self.on_death(c, SideK::Enemy, None, DeathCause::Sacrifice);
        Ok(())
    }

    // ---------- 放置 ----------

    /// 通用放置核心（我方/敌方共用）。压上已占用格 → 原占位卡越线死亡；
    /// 谁能压由 `enemy_place_legal` / `player_place` 的格位检查决定（影长「暗渡」是敌方唯一放行方）。
    /// §廿二:944 越线死亡统一走 `on_death(.., DeathCause::Cross)`：触发亡语、按死亡返还递减获得业力。
    fn place_side(&mut self, side: SideK, card: CardInst, col: usize, row: Row) {
        let cost = if card.is_starter() { 0 } else { card.def.cost };  // §十二:433 放置消耗业力＝卡牌费用（开端 0）；§廿三:987 业力消耗之一：放置；§七:264 费用＝卡牌消耗的业力
        match side {
            SideK::Player => self.p_karma -= cost,
            SideK::Enemy => self.e_karma -= cost,
        }
        let mut c = card;
        c.seq = self.seq;
        self.seq += 1;
        c.placed_turn = self.turn;
        let slot: &mut Option<CardInst> = match (side, row) {  // §十七:765 挤只挤在同一侧的格子里，两套数组无任何交叉赋值
            (SideK::Player, _) => &mut self.p_front[col],
            (SideK::Enemy, Row::Front) => &mut self.e_front[col],
            (SideK::Enemy, Row::Back) => &mut self.e_back[col],
        };
        let old = slot.take();  // §十二:434 空格直接放；§十二:435 有卡则新卡占位、旧卡待越线；§十七:732 若格为空→直接放置
        let (tr, skills) = (c.def.tr, c.skills.clone());
        self.log.push(format!(
            "{}放置 {} → {}{}",
            if side == SideK::Player { "我方" } else { "敌方" },
            c.def.name,
            if side == SideK::Player { "P" } else if row == Row::Front { "E" } else { "E后" },
            col + 1
        ));
        *slot = Some(c);
        let _ = slot;
        if let Some(old) = old {  // §十七:755 我方挤压＝新卡挤旧卡，越线死亡（同一条路径对两侧通用）
            self.log.push(format!("挤压：{} 越线死亡", short_card(&old)));  // §十二:435 旧卡越线死亡
            self.on_death(old, side, Some(col), DeathCause::Cross);  // §十二:435 越线死亡按死亡返还递减返业力；§廿三:1006 挤压；§十七:733 新卡占位＋旧卡越线＋业力＝旧卡费用（按递减）；§十七:764 越过＝直接死亡（本行是唯一出口，没有"越过后再落到某格"的分支）
        }
        let slot_now = match (side, row) {
            (SideK::Player, _) => &self.p_front[col],
            (SideK::Enemy, Row::Front) => &self.e_front[col],
            (SideK::Enemy, Row::Back) => &self.e_back[col],
        };
        if slot_now.is_none() {
            return;
        }
        if tr == TraitKind::BattleCrySameColFlame2 {
            self.add_flame_col(side, col, 2, None);
        }
        for s in skills {
            match s {
                Skill::PlaySameColFlame1 => self.add_flame_col(side, col, 1, None),  // §八:293 放置时同列+1业火
                Skill::PlayAdjColFlame1 => {  // §八:300 放置时相邻列+1业火
                    for ac in adj_cols(col) {
                        self.add_flame_col(side, ac, 1, None);
                    }
                }
                _ => {}
            }
        }
        self.check_all_triggers();
    }

    pub fn player_place(&mut self, hand_idx: usize, col: usize) -> Result<(), String> {  // §十二:432 放置到 P1-P4 的唯一入口；§十七:758 玩家可以挤压我方卡牌越线＝主动选择，故这条入口**不查目标格占用**（与敌方 enemy_place_legal 的占用闸相反）
        if col >= 4 {
            return Err("格位为 P1-P4".into());
        }
        if hand_idx >= self.hand.len() {
            return Err("手牌下标越界".into());
        }
        let starter = self.hand[hand_idx].is_starter();
        let cost = if starter { 0 } else { self.hand[hand_idx].def.cost };
        if self.pf.sacrificed_names.contains(&self.hand[hand_idx].def.name) {
            return Err(format!("本回合献祭过同名牌「{}」，不能再放置", self.hand[hand_idx].def.name));
        }
        if self.p_karma < cost {  // §十二:433 业力不足即拒绝
            return Err(format!("业力不足：需{cost}，当前{}", self.p_karma));
        }
        let c = self.hand.remove(hand_idx);
        self.place_side(SideK::Player, c, col, Row::Front);
        Ok(())
    }

    /// 敌方放置的全部合法性检查（业力 / 同名禁回置 / 格位占用）。
    /// 影长「暗渡」例外：允许压上已占用的前排，复用 `place_side` 既有的越线死亡 + 递减返还路径。
    fn enemy_place_legal(&self, c: &CardInst, col: usize, row: Row) -> Result<(), String> {
        if col >= 4 {
            return Err("格位为 E1-E8（列 0-3）".into());
        }
        let cost = if c.is_starter() { 0 } else { c.def.cost };
        if self.e_karma < cost {
            return Err("敌方业力不足".into());
        }
        if self.ef.sacrificed_names.contains(&c.def.name) {
            return Err("敌方同名牌限制".into());
        }
        let occupied = match row {
            Row::Front => self.e_front[col].is_some(),
            Row::Back => self.e_back[col].is_some(),
        };
        let squeeze = self.boss_rule() == crate::boss::BossRule::ShadowPush && row == Row::Front;  // §十七:756 敌方挤压（后排挤前排、被挤者越线死）在引擎里真实存在的唯一入口＝影长「暗渡」，普通 AI 走不到这条路径
        if occupied && !squeeze {  // §十二:490 AI 不会主动把 E5-E8 挤越线；§十七:748 同上（本行就是那条自律的实现处，普通敌方放置一律拒绝压 occupied）
            return Err("AI 不挤压（不会主动挤越线）".into());
        }
        Ok(())
    }

    pub fn enemy_place(&mut self, hand_idx: usize, col: usize, row: Row) -> Result<(), String> {
        if hand_idx >= self.enemy_hand.len() {
            return Err("敌方手牌越界".into());
        }
        self.enemy_place_legal(&self.enemy_hand[hand_idx], col, row)?;
        let c = self.enemy_hand.remove(hand_idx);
        self.place_side(SideK::Enemy, c, col, row);
        Ok(())
    }

    /// 放置一个现成实例（Boss 脚本直放：不经敌方手牌/牌堆、技能由脚本声明）。
    pub fn enemy_place_inst(&mut self, c: CardInst, col: usize, row: Row) -> Result<(), String> {
        self.enemy_place_legal(&c, col, row)?;
        self.place_side(SideK::Enemy, c, col, row);
        Ok(())
    }

    // ---------- 回合推进 ----------

    /// 结束我方回合：我方攻击（立即结算）→ 开端回合末 → 敌方整回合 → 回我方回合开始。
    pub fn end_player_turn(&mut self) {
        if self.over.is_some() {
            return;
        }
        self.player_attack_phase();
        self.starter_turn_end(SideK::Player);
        if self.over.is_none() {
            self.enemy_turn();
        }
        if self.over.is_none() {
            // §廿二:958 满 30 回合未分胜负 → 比蜡烛长度，长者胜、相等平局。 §廿三:1026 平局条件。
            // 敌方有双烛时比"较长的那根"（`enemy_candle_ref`），取和会形同必败（裁定19）。
            if self.turn >= self.turn_limit {
                let foe = self.enemy_candle_ref();
                self.over = Some(match self.p_candle.cmp(&foe) {
                    std::cmp::Ordering::Greater => Outcome::PlayerWin,
                    std::cmp::Ordering::Less => Outcome::PlayerLose,
                    std::cmp::Ordering::Equal => Outcome::Draw,
                });
                let twin = if self.e_candle2.is_some() { "（敌方取两根较长值）" } else { "" };
                self.log.push(format!("{}回合终：蜡烛 {} vs {}{twin} → {:?}", self.turn_limit, self.p_candle, foe, self.over.unwrap()));
                return;
            }
            self.player_turn_start();
        }
    }

    fn slot(&self, side: SideK, row: Row, col: usize) -> &Option<CardInst> {
        match (side, row) {
            (SideK::Player, _) => &self.p_front[col],
            (SideK::Enemy, Row::Front) => &self.e_front[col],
            (SideK::Enemy, Row::Back) => &self.e_back[col],
        }
    }

    /// 敌方某格（Boss 脚本按名找献祭目标用）。
    pub(crate) fn enemy_slot(&self, row: Row, col: usize) -> Option<&CardInst> {
        self.slot(SideK::Enemy, row, col).as_ref()
    }

    pub(crate) fn enemy_slot_mut(&mut self, row: Row, col: usize) -> Option<&mut CardInst> {
        match row {
            Row::Front => self.e_front[col].as_mut(),
            Row::Back => self.e_back[col].as_mut(),
        }
    }

    fn slot_mut(&mut self, side: SideK, row: Row, col: usize) -> &mut Option<CardInst> {
        match (side, row) {
            (SideK::Player, _) => &mut self.p_front[col],
            (SideK::Enemy, Row::Front) => &mut self.e_front[col],
            (SideK::Enemy, Row::Back) => &mut self.e_back[col],
        }
    }

    /// §廿二:962 攻击顺序＝按卡牌入场顺序（`seq` 单调递增，见 `place_side`），不按列位。§十二:440 P1-P4 按卡牌入场顺序依次攻击。
    fn row_seq_order(&self, side: SideK, row: Row) -> Vec<u64> {
        let mut v: Vec<u64> = (0..4)  // §十二:448 hp≤0 的卡不进序列＝攻击前已死亡则跳过
            .filter_map(|c| self.slot(side, row, c).as_ref().filter(|x| x.hp > 0).map(|x| x.seq))
            .collect();
        v.sort();
        v
    }

    fn col_of_seq(&self, side: SideK, row: Row, sq: u64) -> Option<usize> {
        (0..4).find(|i| self.slot(side, row, *i).as_ref().is_some_and(|c| c.seq == sq))
    }

    /// 九章那条叠加等式的实测入口（仅测试构建）：走**真实的**我方攻击阶段，
    /// 不在测试里另抄一份「技能逐条 +1」的累积逻辑——抄了就等于测壳不测引擎。
    #[cfg(test)]
    pub(crate) fn s9_run_player_attack_phase(&mut self) {
        self.player_attack_phase();
    }

    fn player_attack_phase(&mut self) {
        let order = self.row_seq_order(SideK::Player, Row::Front);
        self.attack_order = order.clone();
        self.in_player_attack_phase = true;
        for sq in order {
            if self.over.is_some() {
                break;
            }
            let col = match self.col_of_seq(SideK::Player, Row::Front, sq) {
                Some(c) => c,
                None => continue,
            };
            let mut atk = self.p_front[col].take().unwrap();
            let tr = atk.def.tr;
            let base = atk.hp; // §七:265 数值＝攻击力/生命值；§七:267 伤害＝这个 base（同一份数值出招时当伤害读）
            let id = atk.id;
            let target = self.pick_target(SideK::Player, col, tr);  // §十二:441 同列中线对面有敌卡→攻击该卡
            self.log.push(format!("⚔ 我方 {} 攻击", short_card(&atk)));
            let dmg = match target {
                Some(dcol) => self.card_hit_damage(SideK::Player, id, col, dcol, base, atk.is_starter()),
                None => self.holder_hit_damage(SideK::Player, id, col, base),  // §十二:442 同列对面无卡→攻击中线；§廿三:1010 攻击优先级
            };
            if let Some(dcol) = target {  // §十二:443 我方攻击阶段伤害立即结算；§廿三:1023 我方立即结算，敌方累积到回合末统一结算（battle.rs:889）
                let eb = self.boost_for(SideK::Enemy);
                if let Some(def) = self.e_front[dcol].as_mut() {
                    def.hp -= dmg;  // §十二:444 目标数值降低；§十五:622 "A已结算（敌方数值已降低）"就是这一句——回滚只是把它当额度，不再打第二遍
                    def.flame += dmg + eb;  // §十二:445 目标业火值 += 伤害；§十六:677 业火条的作用就是累积伤害
                }
                self.dealt_this_turn += dmg;  // §十五:619 口径 A 的入账处：本回合攻击阶段**所有卡牌攻击时**造成的伤害总和
                self.log.push(format!("  → 敌第{}列受{dmg}", dcol + 1));
            } else {
                self.dealt_this_turn += dmg;  // §十五:620 直击持业者的那发攻击同样进口径 A（"基础攻击伤害"的一部分，不是外添项）
                self.damage_enemy_holder(dmg, Some(col), HolderHit::Direct);  // §十二:442 直击中线→敌方持业者掉血
            }
            self.attacker_aftermath(&mut atk, target.unwrap_or(col), SideK::Player);  // §十二:447 攻击时自动触发特性+技能
            let atk_dead = atk.hp <= 0;
            self.p_front[col] = Some(atk);
            if let Some(dcol) = target {
                if let Some(d) = self.e_front[dcol].take() {
                    if d.hp <= 0 {
                        self.on_death(d, SideK::Enemy, Some(dcol), DeathCause::Battle);
                    } else {
                        self.e_front[dcol] = Some(d);
                    }
                }
            }
            if atk_dead {  // §十二:449 攻击后死亡不影响下一张（for 继续）
                let a = self.p_front[col].take().unwrap();
                self.on_death(a, SideK::Player, Some(col), DeathCause::Battle);
            }
            self.check_all_triggers();
        }
        self.in_player_attack_phase = false;
    }

    pub(crate) fn enemy_turn(&mut self) {
        crate::boss::on_enemy_turn_start(self);
        if !self.enemy_pile.is_empty() {
            let i = self.rng.below(self.enemy_pile.len());
            let c = self.enemy_pile.remove(i);
            self.enemy_hand.push(c);
        }
        self.ef.sacrifice_used = false;
        self.ef.sacrificed_names.clear();
        if self.boss.is_some() {
            crate::boss::run(self);
        } else {
            crate::ai::run(self);  // §十二:455 敌方回合 AI 行动入口
        }
        self.enemy_resolve_turn_end();
    }

    /// 敌方回合尾段（攻击→统一结算→推进→开端回合末）。AI 搜索的叶子推进与实战共用同一实现。
    /// 路径内不消耗 rng（除抽牌外的 rng 消费点为零），因此克隆推进是 rng 中性的。
    pub fn enemy_resolve_turn_end(&mut self) {
        self.enemy_attack_phase();
        self.enemy_settle();
        self.enemy_advance();  // §十二:459 推进阶段；§十二:488 前排被击杀后由下一次推进补上；§十七:746 前排被击杀→下回合后排推进（本帧没有"死格立即补位"的旁路，补位只有这一处时机）
        self.starter_turn_end(SideK::Enemy);
    }

    fn enemy_attack_phase(&mut self) {
        let order = self.row_seq_order(SideK::Enemy, Row::Front);  // §十二:458 所有可攻击卡依次攻击；§十二:462 E5-E8 每张
        let mut card_d: Vec<(usize, i32)> = Vec::new();
        let mut candle_d: i32 = 0;
        for sq in order {
            let col = match self.col_of_seq(SideK::Enemy, Row::Front, sq) {
                Some(c) => c,
                None => continue,
            };
            let mut atk = self.e_front[col].take().unwrap();
            let tr = atk.def.tr;
            let base = atk.hp; // §七:265 数值＝攻击力/生命值；§七:267 伤害＝这个 base（同一份数值出招时当伤害读）
            let id = atk.id;
            let target = self.pick_target(SideK::Enemy, col, tr);  // §十二:463 同列对面有我方卡→攻击该卡
            self.log.push(format!("⚔ 敌方 {} 攻击", short_card(&atk)));
            match target {
                Some(dcol) => {
                    // 伤害累积（攻击时点即时计算攻击+受伤修正，不立即扣血/业火）§十二:465 伤害累积不立即结算；§十二:467 业火值此时暂不增加。
                    let dmg = self.card_hit_damage(SideK::Enemy, id, col, dcol, base, atk.is_starter());
                    card_d.push((dcol, dmg));
                }
                None => {
                    let dmg = self.holder_hit_damage(SideK::Enemy, id, col, base);
                    candle_d += dmg;  // §十二:464 同列无卡→打中线，累积到我方持业者
                }
            }
            self.attacker_aftermath(&mut atk, target.unwrap_or(col), SideK::Enemy);  // §十二:466 攻击时触发的特性/技能立即生效
            let atk_dead = atk.hp <= 0;
            self.e_front[col] = Some(atk);
            if atk_dead {
                let a = self.e_front[col].take().unwrap();
                self.on_death(a, SideK::Enemy, Some(col), DeathCause::Battle);
            }
            self.check_all_triggers();
        }
        self.pending_card_d = card_d;  // §十二:465 累积额留到回合末统一结算才落地
        self.pending_candle_d = candle_d;
    }

    /// 我方回合末的敌方伤害统一结算。四条形径都钉在这一个函数里，改动会同时破坏多条边界条款：
    /// §十五:609 时点＝"敌方攻击阶段结束，统一结算前"——本函数就是那道统一结算，回滚判定排在任何数值落地之前；
    /// §廿二:954 回滚消耗来源 A＝本回合攻击阶段已打出的伤害总和（`dealt_this_turn`）；
    /// §廿二:956 代价＝A 全额抵掉这一发 D，另记下回合业力-1（`karma_penalty_next`）；
    /// §廿二:957 顺序＝先回滚判定 → 后蜡烛减短 → 后业火增加（下面严格按此三段排列）；
    /// §廿二:953 超额部分按我方攻击顺序分配，优先攻击敌方卡牌、无卡则攻击敌方持业者（`distribute_excess`）；
    /// §廿二:943 我方烛尽的同一结算里敌方烛也已尽 → 平局（双方同时致命不判一方胜）。
    fn enemy_settle(&mut self) {  // §廿三:1014 回滚只存在于「我方受击」这一侧，敌方没有对应物
        let d = std::mem::take(&mut self.pending_candle_d);  // §十二:470 统一结算先算我方持业者受到的总伤害 D
        if d > 0 && self.over.is_none() {
            if d >= self.p_candle && self.rollback_left > 0 {  // §十二:471 D ≥ 当前蜡烛长度 → 触发伤害回滚；§廿三:1015 回滚触发；§十五:610 累积伤害（D）≥ 我方持业者HP
                let a = self.dealt_this_turn;  // §十二:472 A＝我方本回合攻击阶段已打出的伤害总和；§廿三:1016 回滚来源；§十五:622 A 早已结算过，这里只当抵消额度用
                let remain = (d - a).max(0);  // §十二:473 剩余伤害 = max(0, D - A)；§十五:624 A 抵消 D；§十五:625 剩余伤害 = max(0, D - A)
                let excess = (a - d).max(0);  // §十二:474 超额伤害 = max(0, A - D)；§十五:626 超额伤害 = max(0, A - D)
                self.rollback_left -= 1;
                self.karma_penalty_next = 1;  // §十五:638 代价之二登记处（落地与"最低0"见 player_turn_start）
                self.p_candle -= remain;  // §十二:475 回滚后蜡烛减短＝剩余伤害；§十五:627 剩余伤害结算 → 蜡烛减短
                self.log.push(format!(
                    "[红光亮起·伤害回滚 余{}] D={d} A={a} → 剩余{remain}削烛（我方蜡烛剩 {}）",
                    self.rollback_left, self.p_candle
                ));
                if excess > 0 {
                    self.distribute_excess(excess);  // §十二:476 超额伤害另行分配；§十五:629 只派超额这一份，A 已造成的伤害不重复结算
                }
            } else {
                self.p_candle -= d;  // §十四:584 伤害结算＝烛身被削去一截；§十四:592 我方每受到 1 伤害 → 蜡烛减短 1 单位
                self.log.push(format!("我方蜡烛 -{d}（剩 {}）", self.p_candle));
            }
            if self.p_candle <= 0 && self.over.is_none() {  // §十二:496 持业者蜡烛燃尽 → 游戏结束；§十四:593 我方蜡烛长度≤0 → 烛尽 → 持业者死亡
                self.log.push("我方烛尽…".into());
                self.over = Some(if self.enemy_candle_ref() <= 0 { Outcome::Draw } else { Outcome::PlayerLose });
            }
        }
        // 统一结算累积卡牌伤害：先数值降低，后业火增加§十二:477 统一结算＝目标数值降低 + 业火值增加。
        let card_d = std::mem::take(&mut self.pending_card_d);
        for (col, dmg) in &card_d {
            if let Some(def) = self.p_front[*col].as_mut() {
                def.hp -= dmg;  // §十二:477 数值降低
                self.log.push(format!("  结算：{} 受{dmg} → hp {}", def.def.name, def.hp));
            }
        }
        let pb = self.boost_for(SideK::Player);
        for (col, dmg) in &card_d {
            if let Some(def) = self.p_front[*col].as_mut() {
                def.flame += dmg + pb;  // §十二:477 业火值增加
            }
        }
        for col in 0..4 {
            if let Some(c) = self.p_front[col].take() {
                if c.hp <= 0 {
                    self.on_death(c, SideK::Player, Some(col), DeathCause::Battle);
                } else {
                    self.p_front[col] = Some(c);
                }
            }
        }
        self.check_all_triggers();  // §十二:478 结算后业火 ≥ 阈值 → 触发特性
    }

    /// 超额伤害：按我方攻击顺序轮转，每名存活攻击者至多分配其当前数值点。§十二:476 按我方攻击顺序依次分配，优先敌方卡牌、无卡则打敌方持业者。 §廿三:1020 超额伤害分配。
    /// §十五:628 超额伤害按**我方本回合的攻击顺序**依次分配：该列有敌卡就打敌卡，该列无卡就打敌方持业者。
    /// §十五:630 本函数只花掉传进来的 `excess`，**不回头累加 `dealt_this_turn`**——所以这一发超额不会变成
    /// 下一次回滚的额度 A（"回滚产生的超额伤害不参与后续回滚"就是靠这里不入账兑现的）。
    fn distribute_excess(&mut self, mut excess: i32) {
        let order = self.attack_order.clone();
        let eb = self.boost_for(SideK::Enemy);
        loop {
            if excess <= 0 || self.over.is_some() {
                break;
            }
            let mut progressed = false;
            for &sq in &order {
                if excess <= 0 {
                    break;
                }
                let col = match self.col_of_seq(SideK::Player, Row::Front, sq) {
                    Some(c) => c,
                    None => continue,
                };
                let hp = self.p_front[col].as_ref().unwrap().hp;
                let give = hp.min(excess).max(0);
                if give == 0 {
                    continue;
                }
                progressed = true;
                excess -= give;
                if self.e_front[col].is_some() {  // §十二:476 该列有敌卡→优先分配给卡牌
                    if let Some(def) = self.e_front[col].as_mut() {
                        def.hp -= give;
                        def.flame += give + eb;
                        self.log.push(format!("  超额分配{give} → {}", short_card(def)));
                    }
                    if self.e_front[col].as_ref().is_some_and(|d| d.hp <= 0) {
                        let dd = self.e_front[col].take().unwrap();
                        self.on_death(dd, SideK::Enemy, Some(col), DeathCause::Battle);
                    }
                } else {
                    self.damage_enemy_holder(give, Some(col), HolderHit::Excess(give));  // §十二:476 该列无卡→攻击敌方持业者
                }
            }
            if !progressed {
                self.log.push(format!("超额伤害{excess}无可分配攻击者，消散"));
                break;
            }
        }
    }

    /// 对敌方持业者造成伤害。单烛＝原文案（逐字不变）；双烛（炎冰同源）＝按列分区落到对应那根，
    /// 另一根同步承受 ⌊dmg/2⌋，两根皆尽才判胜（model.rs 裁定19）。
    fn damage_enemy_holder(&mut self, dmg: i32, col: Option<usize>, how: HolderHit) {
        let Some(c2) = self.e_candle2 else {
            self.e_candle -= dmg;  // §十四:597 敌方每受到 1 伤害 → 蜡烛减短 1 单位（与 §十四:592 同一条规则的另一侧）
            match how {
                HolderHit::Direct => {
                    self.log.push(format!("  → 直击中线，敌方蜡烛 -{dmg}（剩 {}）", self.e_candle));
                    if self.e_candle <= 0 {  // §十四:598 敌方蜡烛长度≤0 → 烛尽 → 持业者死亡（此处判胜）
                        self.log.push("敌方烛尽！".into());  // §廿三:1025 胜利＝击杀敌方持业者
                        self.over = Some(Outcome::PlayerWin);
                    }
                }
                HolderHit::Excess(g) => {
                    self.log.push(format!("  超额分配{g} → 敌方持业者（剩 {}）", self.e_candle));
                    if self.e_candle <= 0 {
                        self.over = Some(Outcome::PlayerWin);
                    }
                }
            }
            return;
        };
        let (n0, n1) = (self.holder_names[0], self.holder_names[1]);
        let mirror = dmg / 2;
        let hit_first = crate::boss::holder_index(col) == 0;
        self.e_candle -= if hit_first { dmg } else { mirror };
        self.e_candle2 = Some(c2 - if hit_first { mirror } else { dmg });
        let (head, shown) = match how {
            HolderHit::Direct => (format!("  → 直击中线「{}」持业者", if hit_first { n0 } else { n1 }), dmg),
            HolderHit::Excess(g) => (format!("  超额分配{g} → 「{}」持业者", if hit_first { n0 } else { n1 }), g),
        };
        self.log.push(format!("{head} -{shown}（{n0}{} {n1}{}·同源-{mirror}）", self.e_candle, self.e_candle2.unwrap()));
        if crate::boss::both_holders_out(self) {
            self.log.push("双烛皆尽！".into());
            self.over = Some(Outcome::PlayerWin);
        }
    }

    /// §廿二:963 推进＝单卡、按列独立：只有前排空的列才把该列后排顶上来，每列每次至多1张。§十二:482 单卡推进按列独立结算；§十二:483 每列各判一次；§十七:740 同一条（敌方回合结束推进，按列独立）。
    fn enemy_advance(&mut self) {
        for col in 0..4 {  // §十七:741 「每列：」＝这条循环体，四列各判一次，列与列之间不传状态
            if self.e_front[col].is_none() {  // §十二:484 前排空→后排推进；§十二:485 前排有卡→不推进；§十七:742 后有新、前空→推进；§十七:743 前后都有→不推进
                if let Some(c) = self.e_back[col].take() {  // §十二:486 后排空→不推进；§十二:487 推进后 E1-E4 空出；§十二:489 每列后排仅 1 格⇒天然只推最靠近前排的那张；§十七:744 后排空→不推进；§十七:745 推进后 E1-E4 空出（`take` 走的就是这一格）；§十七:747 「多张只推最靠近前排」在本棋盘结构下天然成立（每列后排单格），无第四档可推
                    self.log.push(format!("敌方推进：第{}列后排 {} → 前排", col + 1, c.def.name));
                    self.e_front[col] = Some(c);  // §十二:491 落到 E5-E8 即紧贴中线，没有再往前一步的路径；§十七:749 推进后 E5-E8 紧贴中线、停在这里
                }
            }
        }
    }

    /// §十四:583 正常燃烧＝烛火摇曳、持续燃烧，**不减短**：回合推进只发开端业力，全程不碰两根蜡烛。
    /// 蜡烛的全部写点（谁有资格让它减短）由 §十四 推导器按名单钉住。
    fn starter_turn_end(&mut self, side: SideK) {
        let rows = match side {
            SideK::Player => vec![Row::Front],
            SideK::Enemy => vec![Row::Front, Row::Back],
        };
        let has = rows.iter().any(|r| (0..4).any(|c| self.slot(side, *r, c).as_ref().is_some_and(|x| x.is_starter() && x.hp > 0)));
        if !has {
            return;
        }
        let flags = match side {
            SideK::Player => &mut self.pf,
            SideK::Enemy => &mut self.ef,
        };
        if flags.starter_gains >= 2 {  // §十二:452 开端回合末业力每关上限 2 次
            return;
        }
        flags.starter_gains += 1;
        match side {
            SideK::Player => {
                self.p_karma += 1;  // §十二:452 开端在场→我方获得 1 业力
                self.log.push(format!("开端在场：我方业力+1（每关上限2，已用 {}/2）", self.pf.starter_gains));
            }
            SideK::Enemy => {
                self.e_karma += 1;
                self.log.push(format!("敌方开端在场：敌方业力+1（已用 {}/2）", self.ef.starter_gains));
            }
        }
    }

    // ---------- 死亡统一入口 ----------

    /// 死亡统一入口。去向路由（裁定7）：
    /// 开端 → 离场（每关固定发放，永不入堆）；自造牌 → 永久消失（§十:372）；
    /// 其余基础牌 → 弃牌堆，本关不再使用，下关并入继承堆（§廿二:949 使返还递减跨关可达）。
    /// §廿二:961 亡语在本函数内即刻结算，调用方的 `check_all_triggers` 在其后 ⇒ 亡语先于特性。§十二:494 数值≤0 与 §十二:495 越线共用本入口：亡语→业力＝费用→业火清零。
    /// §廿二:960 业火跨回合保留（没有任何按回合清空的路径），只在死亡这一处清零。
    pub fn on_death(&mut self, mut c: CardInst, side: SideK, col: Option<usize>, cause: DeathCause) {
        let devour = cause == DeathCause::Sacrifice
            && side == SideK::Player
            && self.boss_rule() == crate::boss::BossRule::DevourName;
        let gain = match cause {  // §廿三:983 死亡/献祭的业力收益只在这一处算，基数＝卡牌费用
            DeathCause::Sacrifice => {  // §廿三:986 开端献祭定额 2 业力，不走递减（裁定1）
                if c.is_starter() {
                    2
                } else if devour {
                    // 终影「吞名」：我方主动献祭改按**当前**死亡返还档位计。
                    // 不推进 deaths——速查:985「主动献祭…不触发死亡返还递减」；推进档位会让单场惩罚
                    // 顺着裁定7 的跨关台账永久压低该实例后续的自然返还，那是第二重未授权的罚。
                    let pct = refund_pct(c.deaths);
                    c.def.cost * pct / 100
                } else {
                    c.def.cost
                }
            }
            DeathCause::Battle | DeathCause::Cross => {
                if c.is_starter() {
                    2
                } else {
                    let pct = refund_pct(c.deaths);  // §十二:494 业力＝卡牌费用，按死亡返还递减；§廿三:989 业力＝费用
                    c.deaths += 1;
                    c.def.cost * pct / 100
                }
            }
        };
        match side {
            SideK::Player => self.p_karma += gain,
            SideK::Enemy => self.e_karma += gain,
        }
        let tag = if devour { "（吞名·按死亡返还递减）" } else { "" };
        self.log.push(format!(
            "💀 {} 死亡（{cause:?}）→ {}业力+{gain}{tag}",
            short_card(&c),
            if side == SideK::Player { "我方" } else { "敌方" }
        ));
        if let Some(col) = col {  // §十二:494 触发亡语（若有）
            if c.def.tr == TraitKind::DeathRattleSameColFlame3 {
                self.add_flame_col(side, col, 3, None);
            }
            for s in c.skills.clone() {
                if s == Skill::DeathSameColFlame1 {  // §八:294 死亡时同列+1业火
                    self.add_flame_col(side, col, 1, None);
                }
            }
        }
        c.flame = 0;  // §十二:494 业火值清零；§十六:682 死亡后清零
        c.seq = 0;
        c.placed_turn = i64::MIN;
        c.triggered_turn = i64::MIN;
        let recycle = !c.is_starter() && !c.crafted;
        match (side, recycle) {
            (SideK::Player, true) => self.discard_pile.push(c),
            (SideK::Enemy, true) => self.e_discard.push(c),
            (SideK::Player, false) => {
                if c.crafted {
                    self.log.push(format!("  自造牌 {} 永久消失", c.def.name));
                }
            }
            (SideK::Enemy, false) => {}
        }
        self.check_all_triggers();
    }

    // ---------- 目标/伤害计算 ----------

    pub(crate) fn pick_target(&self, side: SideK, col: usize, tr: TraitKind) -> Option<usize> {  // §廿三:1008 默认只打同列
        let def_front_empty = |c: usize| match side {
            SideK::Player => self.e_front[c].is_none(),
            SideK::Enemy => self.p_front[c].is_none(),
        };
        if !def_front_empty(col) {
            return Some(col);
        }
        if tr == TraitKind::AttackAdjacent {  // §廿三:1009 全仓唯一的跨列攻击路，且只在特性允许时
            for c in adj_cols(col) {
                if !def_front_empty(c) {
                    return Some(c);
                }
            }
        }
        None
    }

    pub(crate) fn col_cards(&self, side: SideK, col: usize) -> Vec<&CardInst> {
        let mut v = Vec::new();
        if let Some(c) = self.slot(side, Row::Front, col).as_ref() {
            v.push(c);
        }
        if side == SideK::Enemy {
            if let Some(c) = self.slot(side, Row::Back, col).as_ref() {
                v.push(c);
            }
        }
        v
    }

    fn col_cards_mut(&mut self, side: SideK, col: usize) -> Vec<&mut CardInst> {
        let mut v = Vec::new();
        if side == SideK::Enemy {
            if let Some(c) = self.e_back[col].as_mut() {
                v.push(c);
            }
            if let Some(c) = self.e_front[col].as_mut() {
                v.push(c);
            }
        } else {
            if let Some(c) = self.p_front[col].as_mut() {
                v.push(c);
            }
        }
        v
    }

    /// §九354：同名技能/特性效果按出现次数叠加（特性计1层）。
    fn skill_count(cards: &[&CardInst], sk: Skill) -> i32 {
        cards.iter().filter(|x| x.hp > 0).flat_map(|x| x.skills.iter()).filter(|s| **s == sk).count() as i32
    }

    pub(crate) fn attack_power(&self, side: SideK, atk_id: u64, acol: usize, base: i32) -> i32 {
        let mut dmg = base;
        dmg += Self::skill_count(&self.col_cards(side, acol).into_iter().filter(|a| a.id != atk_id).collect::<Vec<_>>(), Skill::AllyColAtk1);  // §八:295 同列友方攻击+1
        let foes = self.col_cards(side.other(), acol);
        let mut debuff = 0;
        for f in &foes {
            if f.hp > 0 && f.def.tr == TraitKind::EnemyColAttackMinus1 {
                debuff += 1;
            }
        }
        debuff += Self::skill_count(&foes, Skill::EnemyColAtkM1);  // §八:296 同列敌方攻击-1
        (dmg - debuff).max(0)
    }

    pub(crate) fn card_hit_damage(&self, side: SideK, atk_id: u64, acol: usize, dcol: usize, base: i32, atk_is_starter: bool) -> i32 {  // §七:267 伤害＝数值经修正后的落点（base 由调用方从攻击者数值给出）
        let mut d = self.attack_power(side, atk_id, acol, base);
        let allies_of_def = self.col_cards(side.other(), dcol);
        let mut reduce = 0;
        for f in &allies_of_def {
            if f.hp > 0 && f.def.tr == TraitKind::AllyColDamageTakenMinus1 {
                reduce += 1;
            }
        }
        reduce += Self::skill_count(&allies_of_def, Skill::AllyColDmgTakenM1);  // §八:297 同列友方受伤-1
        let buffs_of_atk = self.col_cards(side, dcol);
        d -= reduce;
        d += Self::skill_count(&buffs_of_atk, Skill::EnemyColDmgTakenP1);  // §八:298 同列敌方受伤+1（含持有者自身）
        // Boss 特殊规则（霜封）；业火按减免后伤害计（调用方用本函数返回值加业火）
        crate::boss::card_damage_adjust(self, side, atk_is_starter, d.max(0))
    }

    pub(crate) fn holder_hit_damage(&self, side: SideK, atk_id: u64, acol: usize, base: i32) -> i32 {
        self.attack_power(side, atk_id, acol, base)
    }

    /// 攻击后追加结算。`col` = 目标列（直击中线时回退为攻击者列）——§八"攻击后对同列+1"锚定目标列。
    fn attacker_aftermath(&mut self, atk: &mut CardInst, col: usize, side: SideK) {  // §十二:447/466 攻击时点的特性+技能触发
        let bo = self.boost_for(side);
        match atk.def.tr {  // §七:278 特性的作用＝定义卡牌定位（这个 match 就是"定位"的落点）；同处的 `atk.flame += …` 是技能的"额外效果"半边
            TraitKind::SelfFlameOnAttack1 => atk.flame += 1 + bo,
            TraitKind::SelfDmgOnAttack => {
                atk.hp -= 1; // §廿三:1024 自损不触发业火
                if atk.hp <= 0 {
                    self.log.push(format!("  {} 自损而亡", atk.def.name));
                }
            }
            _ => {}
        }
        let skills = atk.skills.clone();
        for s in skills {
            match s {
                Skill::AtkSelfFlame1 => atk.flame += 1 + bo,  // §八:291 攻击后自身业火+1
                Skill::AtkSameColFlame1 => self.add_flame_col(side, col, 1, None),  // §八:292 攻击后同列+1业火
                Skill::AtkAdjColFlame1 => {  // §八:299 攻击后相邻列+1业火
                    for ac in adj_cols(col) {
                        self.add_flame_col(side, ac, 1, None);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn add_flame_col(&mut self, side: SideK, col: usize, amt: i32, except: Option<u64>) {
        let pb = self.boost_for(SideK::Player);
        let eb = self.boost_for(SideK::Enemy);
        for s in [side, side.other()] {
            let bo = if s == SideK::Player { pb } else { eb };
            for c in self.col_cards_mut(s, col) {
                if Some(c.id) == except {
                    continue;
                }
                c.flame += (amt + bo).max(0);
            }
        }
    }

    // ---------- 业火触发 ----------

    pub fn effective_threshold(&self, side: SideK, col: usize, c: &CardInst) -> i32 {
        let mut thr = c.base_threshold();
        let allies: Vec<&CardInst> = self.col_cards(side, col).into_iter().filter(|a| a.id != c.id).collect();
        thr -= Self::skill_count(&allies, Skill::AllyColThreshM1);  // §八:301 同列友方阈值-1
        thr += Self::skill_count(&self.col_cards(side.other(), col), Skill::EnemyColThreshP1);  // §八:302 同列敌方阈值+1
        thr.max(1)
    }

    /// 阵营本回合的"友方累积+N"增益（守夜人窗口，裁定5 literal 化）。
    fn boost_for(&self, side: SideK) -> i32 {
        let t = self.turn;
        self.boosts.iter().filter(|(s, tt, _)| *s == side && *tt == t).map(|(_, _, v)| *v).sum()
    }

    pub fn check_all_triggers(&mut self) {
        let turn = self.turn;
        for _round in 0..12 {
            let before = self.log.len();
            for col in 0..4 {
                self.try_trigger_col(SideK::Player, col, turn);
                self.try_trigger_col(SideK::Enemy, col, turn);
            }
            if self.log.len() == before {
                return;
            }
        }
    }

    /// §廿二:959 每张卡每回合至多触发1次特性（`triggered_turn` 记账）。
    fn try_trigger_col(&mut self, side: SideK, col: usize, turn: i64) {
        let rows = match side {
            SideK::Player => vec![Row::Front],
            SideK::Enemy => vec![Row::Back, Row::Front],
        };
        for row in rows {
            let snap = match self.slot(side, row, col).as_ref() {
                Some(c) => c.clone(),
                None => continue,
            };
            // §廿二:945 死亡后业火达阈值 → **不触发**特性（业火由 `on_death` 清零）。
            // 这裁定了文档自身的序冲突：§十三:547「业火≥阈值 → 触发特性」排在 §十三:548「数值≤0 → 死亡」之前，
            // 按字面顺序读会得出"致命一击仍先触发特性"；§廿二 边界表明写不触发，采边界表读法（裁定26）。
            // §十六:680 触发上限＝每卡每回合最多1次（`triggered_turn` 记账）
            if snap.hp <= 0 || snap.triggered_turn == turn || !is_threshold_trait(snap.def.tr) {
                continue;
            }
            let thr = self.effective_threshold(side, col, &snap);  // §十二:446 业火 ≥ 阈值才触发特性；§七:268 阈值＝触发特性所需业火值（这里取的是该格生效阈值）
            if snap.flame < thr {
                continue; // §十六:678 未达阈值不触发（达阈值即在本检查点触发）
            }
            {
                let c = self.slot_mut(side, row, col).as_mut().unwrap();
                c.flame -= thr;  // §十二:446 触发后业火值 -= 阈值；§十六:679 溢出保留在同一句里；§十六:720 爆发步骤第6动作；§廿三:1021 业火条（跨回合保留见下一行动作）
                c.triggered_turn = turn;
            }
            let tr = snap.def.tr;
            let id = snap.id;
            let bo = self.boost_for(side);
            self.log.push(format!("🔥 {} 业火爆发 → {}", snap.def.name, tr.label())); // §十六:719 爆发步骤第5动作＝特性效果释放（下面那个 match 就是释放处）；§七:271 业火爆发＝业火值达阈值时触发特性（这句日志就是那个时刻）
            match tr {
                TraitKind::ThresholdSameColFlame2 => self.add_flame_col(side, col, 2, Some(id)),
                TraitKind::ThresholdAllyColFlame2 => {
                    for a in self.col_cards_mut(side, col) {
                        if a.id != id {
                            a.flame += 2 + bo;
                        }
                    }
                }
                TraitKind::ThresholdAdjColFlame3 => {
                    for ac in adj_cols(col) {
                        self.add_flame_col(side, ac, 3, None);
                    }
                }
                TraitKind::ThresholdFullColDamage3 => {
                    let mut hits: Vec<(SideK, usize, u64)> = Vec::new();
                    for s in [SideK::Player, SideK::Enemy] {
                        let sb = self.boost_for(s);
                        let counts = s == SideK::Enemy && self.in_player_attack_phase;  // §十五:621 口径 A 的"不包括"就落在这个闸上：非攻击阶段打出的伤害（含敌方侧、结算期外溢）不进 A
                        let mut dealt = 0i32;
                        let srows = match s {
                            SideK::Player => vec![Row::Front],
                            SideK::Enemy => vec![Row::Back, Row::Front],
                        };
                        for r in srows {
                            if let Some(card) = self.slot_mut(s, r, col).as_mut() {
                                if card.id == id {
                                    continue;
                                }
                                card.hp -= 3;
                                card.flame += 3 + sb; // 伤害即业火（三位一体）
                                dealt += 3;
                                if card.hp <= 0 {
                                    hits.push((s, col, card.id));
                                }
                            }
                        }
                        if counts {
                            self.dealt_this_turn += dealt; // §十二：攻击阶段内造成的伤害计入 A；§十五:620 "攻击时触发的特性/技能额外伤害"就在这一句进口径
                        }
                    }
                    for (s, c, cid) in hits {
                        let rows = match s {
                            SideK::Player => vec![Row::Front],
                            SideK::Enemy => vec![Row::Back, Row::Front],
                        };
                        for r in rows {
                            if self.slot(s, r, c).as_ref().is_some_and(|x| x.id == cid) {
                                let card = self.slot_mut(s, r, c).take().unwrap();
                                self.on_death(card, s, Some(c), DeathCause::Battle);
                            }
                        }
                    }
                }
                TraitKind::ThresholdAllyTurnFlame2 => {
                    // §十"本回合友方累积+2"＝回合内增益窗口：本回合该侧每次业火获得事件额外 +2（非瞬时发放）
                    self.boosts.push((side, turn, 2));
                    self.log.push(format!("  守夜人：本回合{side}友方每次累积业火 +2", side = if side == SideK::Player { "我方" } else { "敌方" }));
                }
                _ => {}
            }
        }
    }

    // ---------- 对外视图（CLI/AI） ----------

    /// 搜索用克隆：完整局面 + 丢弃日志（克隆只服务评估，落地时仍用原局）。
    pub fn clone_for_search(&self) -> Battle {
        let mut s = self.clone();
        s.log = Vec::new();
        s
    }

    /// 某格**下一次被攻击**会吃到的最大伤害（可达性感知：只有前排可被直接攻击，
    /// 敌方后排须先推进到前排才挨打 → 记 0，见 §二 布局与 ai.rs 献祭条件3）。
    pub fn threat_to(&self, side: SideK, row: Row, col: usize) -> i32 {
        if row != Row::Front {
            return 0;
        }
        let foe = side.other();
        let mut worst = 0i32;
        for acol in 0..4 {
            let Some(a) = self.slot(foe, Row::Front, acol).as_ref().filter(|c| c.hp > 0) else {
                continue;
            };
            if self.pick_target(foe, acol, a.def.tr) != Some(col) {
                continue;
            }
            worst = worst.max(self.card_hit_damage(foe, a.id, acol, col, a.hp, a.is_starter()));
        }
        worst
    }

    /// 只读谓词：敌方场上献祭此刻是否合法（镜像 enemy_sacrifice_field 的校验，不复制结算）。
    pub fn enemy_sac_field_allowed(&self, row: Row, col: usize) -> bool {
        if self.ef.sacrifice_used {
            return false;
        }
        self.slot(SideK::Enemy, row, col)
            .as_ref()
            .is_some_and(|c| self.turn - c.placed_turn >= 1)
    }

    /// 只读谓词：敌方手牌献祭此刻是否合法（开端豁免每回合额度，裁定10）。
    pub fn enemy_sac_hand_allowed(&self, idx: usize) -> bool {
        match self.enemy_hand.get(idx) {
            None => false,
            Some(c) => c.is_starter() || !self.ef.sacrifice_used,
        }
    }

    /// 跨关继承 = 上一关剩余：场上未阵亡 + 手牌 + 牌堆未抽 + 弃牌堆基础牌；开端不入堆（§五185/§十382）。
    pub fn battle_survivors(&mut self) -> Vec<CardInst> {
        let mut v = std::mem::take(&mut self.draw_pile);
        v.extend(std::mem::take(&mut self.hand));
        v.extend(self.p_front.iter_mut().map(|s| s.take()).flatten());
        v.extend(std::mem::take(&mut self.discard_pile));
        v.retain(|c| !c.is_starter());
        v
    }
}

pub fn is_threshold_trait(tr: TraitKind) -> bool {
    matches!(
        tr,
        TraitKind::ThresholdSameColFlame2
            | TraitKind::ThresholdAllyColFlame2
            | TraitKind::ThresholdAdjColFlame3
            | TraitKind::ThresholdFullColDamage3
            | TraitKind::ThresholdAllyTurnFlame2
    )
}

pub fn adj_cols(col: usize) -> Vec<usize> {
    let mut v = Vec::new();
    if col > 0 {
        v.push(col - 1);
    }
    if col < 3 {
        v.push(col + 1);
    }
    v
}

// ---------- §十五 示例块的文档驱动实测（只在测试里编译，运行时行为零变更） ----------

/// §十五「示例」围栏（文档 643–660）里的每一行，按**形态**读成一条可核对的断言。
/// 为什么示例走实测而不走挂锚：这 14 行是 618–630 那些规则的同一件事重说一遍，挂锚只能证明
/// "有代码行指过它"，跑一遍才证明"引擎算出来的数与文档写的那串数真的相等"。
/// 认不出的行**当场 panic，绝不平跳**——平跳＝文档加了第三种写法的例子而尺子量不到那一行。
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum S15Claim {
    /// `我方蜡烛：3` —— 场景输入：我方持业者长度
    Candle(i32),
    /// `敌方攻击阶段：敌方卡牌攻击中线，总伤害5` —— 场景输入：累积伤害 D
    EnemyDamage(i32),
    /// `我方本回合已打出伤害：4` —— 场景输入：口径 A
    Dealt(i32),
    /// `→ 5 ≥ 3，触发回滚` —— 断言这一发真触发了，且两个操作数就是本场景的 D 与蜡烛
    Triggered(i32, i32),
    /// `→ 剩余伤害：5 - 4 = 1` —— (D, A, 文档写的结果)
    Remaining(i32, i32, i32),
    /// `→ 我方蜡烛减短1 → 长度2` —— (减短量, 结果长度)
    CandleShorn(i32, i32),
    /// `→ 我方伤害被消耗，不作用于敌方` —— A 全额抵进 D，敌方一点没挨到
    DealtConsumed,
    /// `→ 下回合业力-1`
    KarmaMinusNext(i32),
    /// `→ 超额伤害：7 - 5 = 2` —— (A, D, 文档写的结果)
    Excess(i32, i32, i32),
    /// `→ 敌方伤害完全抵消`
    FullyOffset,
    /// `→ 超额2 → 对敌方正常造成2伤害`
    ExcessHitsHolder(i32),
    /// `→ 我方蜡烛不减短 → 长度3`
    CandleIntact(i32),
}

#[cfg(test)]
pub(crate) struct S15Line {
    pub line: usize,
    pub claim: S15Claim,
}

/// 取一行里所有连续 ASCII 数字段。示例行的算术全是半角数字，符号（`：` `≥` `－`）不参与解析，
/// 所以"数出几个数"本身就是形态判据的一部分：多写或少写一个数都当场 panic，不会静默走错分支。
#[cfg(test)]
pub(crate) fn s15_digits(s: &str) -> Vec<i32> {
    let mut out = Vec::new();
    let mut run = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            run.push(c);
        } else if !run.is_empty() {
            out.push(run.parse().unwrap());
            run.clear();
        }
    }
    if !run.is_empty() {
        out.push(run.parse().unwrap());
    }
    out
}

#[cfg(test)]
fn s15_classify(t: &str, line: usize) -> S15Claim {
    let nums = s15_digits(t);
    let want = |n: usize| -> Vec<i32> {
        assert_eq!(nums.len(), n, "md:{line}「{t}」解析出 {} 个数字，该形态期望 {n} 个 ⇒ 示例算术被改写或形态判据失效", nums.len());
        nums.clone()
    };
    if t.starts_with("我方蜡烛：") {
        S15Claim::Candle(want(1)[0])
    } else if t.starts_with("敌方攻击阶段：") {
        S15Claim::EnemyDamage(want(1)[0])
    } else if t.starts_with("我方本回合已打出伤害：") {
        S15Claim::Dealt(want(1)[0])
    } else if t.starts_with("→ 剩余伤害：") {
        let v = want(3);
        S15Claim::Remaining(v[0], v[1], v[2])
    } else if t.starts_with("→ 超额伤害：") {
        let v = want(3);
        S15Claim::Excess(v[0], v[1], v[2])
    } else if t.contains("不减短") {
        S15Claim::CandleIntact(want(1)[0])
    } else if t.contains("蜡烛减短") {
        let v = want(2);
        S15Claim::CandleShorn(v[0], v[1])
    } else if t.contains("触发回滚") {
        let v = want(2);
        S15Claim::Triggered(v[0], v[1])
    } else if t.contains("我方伤害被消耗") {
        S15Claim::DealtConsumed
    } else if t.contains("完全抵消") {
        S15Claim::FullyOffset
    } else if t.contains("对敌方正常造成") {
        S15Claim::ExcessHitsHolder(want(2)[1])
    } else if t.starts_with("→ 下回合业力-") {
        S15Claim::KarmaMinusNext(want(1)[0])
    } else {
        panic!(
            "md:{line}「{t}」不像 §十五 示例里的任何一种行形态 ⇒ 文档加了新写法。请先扩本解析器与它对应的实测断言，\
             别让示例行从尺子外面漏过去（挂锚对示例行不算数，见 model.rs 的 §十五 推导器）"
        );
    }
}

/// 定位并解析 §十五「示例」围栏：章标题 → 其后第一个 trim==`示例` 的标签行 → 之后第一道 ``` 到下一道 ```。
/// 返回**带文档行号**的断言列表；行号口径与 `model::doc_or_skip` 一致（1 起）。
#[cfg(test)]
pub(crate) fn parse_section15_examples(lines: &[String]) -> Vec<S15Line> {
    let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
    let head = lines
        .iter()
        .position(|l| l.trim() == "十五、伤害回滚（仅玩家拥有）")
        .expect("§十五 标题必须存在（文档结构变了就要同步改本解析器与 model.rs 的推导器）");
    let label = ((head + 2)..=lines.len())
        .find(|&n| at(n).trim() == "示例")
        .expect("§十五 里必须有一行块首标签「示例」");
    let open = ((label + 1)..=lines.len())
        .find(|&n| at(n).trim() == "```")
        .expect("「示例」标签后必须有开围栏 ```");
    for n in (open + 1)..=lines.len() {
        let t = at(n).trim();
        if t == "```" {
            return (open + 1..n)
                .filter(|&k| !at(k).trim().is_empty())
                .map(|k| S15Line { line: k, claim: s15_classify(at(k).trim(), k) })
                .collect();
        }
    }
    panic!("§十五 示例围栏没有闭围栏（{open} 之后找不到 ```）")
}

/// §十六「触发示例」围栏（687–689）里一行读成的场景。三行只有一种形态
/// （`阈值T，业火值F → 触发N次 → 业火值 A-B=R（注）`），所以这里是结构体不是 enum——
/// 但**形态对不上照样 panic**，理由同 §十五：平跳＝文档加了第二种写法而尺子量不到那一行。
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct S16Case {
    pub line: usize,
    /// 行首 `阈值T`
    pub threshold: i32,
    /// 行首 `业火值F`——引擎侧的起始业火值
    pub before: i32,
    /// `→ 触发`／`→ 触发N次`，不带数字按 1 次
    pub triggers: i32,
    /// 行尾减法 `A-B=R` 的三个数：A 必须等于 before、B 必须等于 threshold（在实测里核对）
    pub lhs: i32,
    pub rhs: i32,
    pub after: i32,
}

/// §十六 示例行的形态错报出口。**写成普通 fn 而不是闭包**：闭包返回 `!` 在 Rust 里不算发散点，
/// panic 之后代码继续往下走，"绝不平跳"就成了空话。
#[cfg(test)]
fn s16_bad(tag: &str, why: &str) -> ! {
    panic!("{tag} 不像 §十六 示例行的形态（期望「阈值T，业火值F → 触发[N次] → 业火值 A-B=R」）：{why}。\
            请先扩本解析器与它对应的实测断言，别让示例行从尺子外面漏过去（挂锚对示例行不算数）");
}

#[cfg(test)]
fn s16_classify(t: &str, line: usize) -> S16Case {
    let tag = format!("md:{line}「{t}」");
    let (seg0, rest) = match t.split_once('，') {
        Some(p) => p,
        None => s16_bad(&tag, "没有「，」分隔阈值与业火值"),
    };
    let one = |s: &str, expect: &str| -> i32 {
        if !s.starts_with(expect) {
            s16_bad(&tag, &format!("「{s}」该以「{expect}」开头"));
        }
        let v = s15_digits(s);
        if v.len() != 1 {
            s16_bad(&tag, &format!("「{s}」解析出 {} 个数字，该形态期望 1 个", v.len()));
        }
        v[0]
    };
    let threshold = one(seg0, "阈值");
    let before = one(rest.split_whitespace().next().unwrap_or(""), "业火值");
    let parts: Vec<&str> = t.split('→').collect();
    if parts.len() != 3 {
        s16_bad(&tag, &format!("应有两段「→」，实测 {} 段", parts.len()));
    }
    let mid = parts[1].trim();
    if !mid.starts_with("触发") {
        s16_bad(&tag, "第二段不以「触发」开头");
    }
    let m = s15_digits(mid);
    let triggers = match m.len() {
        0 => 1,
        1 => m[0],
        n => s16_bad(&tag, &format!("「{mid}」解析出 {n} 个数字，触发次数最多一个")),
    };
    let tail_full = parts[2].trim();
    if !tail_full.starts_with("业火值") {
        s16_bad(&tag, "第三段不以「业火值」开头");
    }
    let tail = tail_full.split('（').next().unwrap_or(tail_full);
    let v = s15_digits(tail);
    if v.len() != 3 {
        s16_bad(&tag, &format!("「{tail}」解析出 {} 个数字，减法式期望 3 个（A、B、R）", v.len()));
    }
    S16Case { line, threshold, before, triggers, lhs: v[0], rhs: v[1], after: v[2] }
}

/// 定位并解析 §十六「触发示例」围栏：章标题 → 其后第一个 trim==`触发示例` 的标签行 →
/// 之后第一道 ``` 到下一道 ```。行号口径与 `model::doc_or_skip` 一致（1 起）。
/// 它同时是 §十六 推导器第三条认领路的**唯一口径**：那边的行集合必须由这里的行号构成。
#[cfg(test)]
pub(crate) fn parse_section16_examples(lines: &[String]) -> Vec<S16Case> {
    let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
    let head = lines
        .iter()
        .position(|l| l.trim() == "十六、业火条")
        .expect("§十六 标题必须存在（文档结构变了就要同步改本解析器与 model.rs 的推导器）");
    let label = ((head + 2)..=lines.len())
        .find(|&n| at(n).trim() == "触发示例")
        .expect("§十六 里必须有一行块首标签「触发示例」");
    let open = ((label + 1)..=lines.len())
        .find(|&n| at(n).trim() == "```")
        .expect("「触发示例」标签后必须有开围栏 ```");
    for n in (open + 1)..=lines.len() {
        let t = at(n).trim();
        if t == "```" {
            return (open + 1..n)
                .filter(|&k| !at(k).trim().is_empty())
                .map(|k| s16_classify(at(k).trim(), k))
                .collect();
        }
    }
    panic!("§十六 示例围栏没有闭围栏（{open} 之后找不到 ```）")
}

/// §十四「蜡烛视觉」围栏（590–599）一行读成的断言。本章没有示例算式，但规则行自己带着三个数
/// （初始长度／每受 X 伤减短 Y／判死上界）——那就是可跑的输入，所以走 §十五/§十六 那条「文档数字驱动引擎」的路。
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum S14Claim {
    /// `我方持业者：`／`敌方持业者：`——分侧标签，其后所有规则行归这一侧。
    Side(&'static str),
    /// `- 初始长度20单位`
    Init(i32),
    /// `- 每受到1伤害 → 蜡烛减短1单位`
    PerHit { dmg: i32, shrink: i32 },
    /// `- 蜡烛长度≤0 → 烛尽 → …死亡`，存的是那个上界
    DeathAt(i32),
    /// `- 视觉与玩家持业者对称`——本章唯一不带数字的规则行，实测里只核对两侧走同一个渲染函数。
    Symmetry,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct S14Line {
    pub line: usize,
    /// 归属侧：`我方`／`敌方`
    pub side: &'static str,
    pub claim: S14Claim,
}

/// §十四 围栏行的形态错报出口。**写成普通 fn 而不是闭包**，理由同 §十五/§十六 那两处。
#[cfg(test)]
fn s14_bad(tag: &str, why: &str) -> ! {
    panic!(
        "{tag} 不像 §十四 蜡烛围栏行的形态（`我方／敌方持业者：`｜`- 初始长度N单位`｜`- 每受到X伤害 → 蜡烛减短Y单位`｜\
         `- 蜡烛长度≤N → 烛尽 → …死亡`｜`- 视觉与玩家持业者对称`）：{why}。\
         请先扩本解析器与它对应的实测断言，别让规则行从尺子外面漏过去（挂锚对这类行不算实测，见 model.rs 的 §十四 推导器）"
    );
}

#[cfg(test)]
fn s14_classify(t: &str, line: usize) -> S14Claim {
    let tag = format!("md:{line}「{t}」");
    if t.ends_with('：') {
        let words: Vec<&str> = t.split_whitespace().collect();
        if words.len() != 1 {
            s14_bad(&tag, &format!("侧标签该是单 token，实测 {} 个", words.len()));
        }
        return if t.starts_with("我方") {
            S14Claim::Side("我方")
        } else if t.starts_with("敌方") {
            S14Claim::Side("敌方")
        } else {
            s14_bad(&tag, "以「：」收尾却不是我方／敌方任何一侧的标签");
        };
    }
    let Some(body) = t.strip_prefix("- ") else {
        s14_bad(&tag, "规则行该以「- 」开头");
    };
    let nums = s15_digits(body);
    if body.starts_with("初始长度") {
        if nums.len() != 1 {
            s14_bad(&tag, &format!("「{body}」解析出 {} 个数字，初始长度该有 1 个", nums.len()));
        }
        return S14Claim::Init(nums[0]);
    }
    if body.contains("每受到") {
        if !body.contains("蜡烛减短") {
            s14_bad(&tag, "有「每受到」却没有「蜡烛减短」，这一行读不出减多少");
        }
        if nums.len() != 2 {
            s14_bad(&tag, &format!("「{body}」解析出 {} 个数字，伤害与减短量各 1 个、共 2 个", nums.len()));
        }
        return S14Claim::PerHit { dmg: nums[0], shrink: nums[1] };
    }
    if body.starts_with("蜡烛长度") {
        if !body.contains("烛尽") {
            s14_bad(&tag, "有「蜡烛长度」却没有「烛尽」，判死对象读不出来");
        }
        if nums.len() != 1 {
            s14_bad(&tag, &format!("「{body}」解析出 {} 个数字，判死上界该有 1 个", nums.len()));
        }
        return S14Claim::DeathAt(nums[0]);
    }
    if body.starts_with("视觉") {
        if !nums.is_empty() {
            s14_bad(&tag, &format!("对称行不该带数字，实测解析出 {}", nums.len()));
        }
        return S14Claim::Symmetry;
    }
    s14_bad(&tag, "五种形态一种都不像")
}

/// 定位并解析 §十四「蜡烛视觉」围栏：章标题 → 其后第一道 ``` → 到下一道 ```。
/// 它同时是 §十四 推导器实测路的**唯一口径**：那边的行集合必须由这里的行号构成（同 §十五/§十六）。
#[cfg(test)]
pub(crate) fn parse_section14_candle_rules(lines: &[String]) -> Vec<S14Line> {
    let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
    let head = lines
        .iter()
        .position(|l| l.trim() == "十四、持业者 · 蜡烛")
        .expect("§十四 标题必须存在（文档结构变了就要同步改本解析器与 model.rs 的推导器）");
    let open = ((head + 2)..=lines.len())
        .find(|&n| at(n).trim() == "```")
        .expect("§十四 必须有一道 ``` 围栏（蜡烛视觉那一段）");
    let mut cur: Option<&'static str> = None;
    let mut out: Vec<S14Line> = Vec::new();
    for n in (open + 1)..=lines.len() {
        let t = at(n).trim();
        if t == "```" {
            return out;
        }
        if t.is_empty() {
            continue;
        }
        let claim = s14_classify(t, n);
        if let S14Claim::Side(s) = claim {
            cur = Some(s);
        }
        let side = match cur {
            Some(s) => s,
            None => panic!("md:{n}「{t}」出现在任何「持业者：」侧标签之前 ⇒ §十四 围栏结构变了"),
        };
        out.push(S14Line { line: n, side, claim });
    }
    panic!("§十四 蜡烛视觉围栏没有闭围栏（{open} 之后找不到 ```）")
}

/// §十四 一侧（我方／敌方）折叠出来的数字。**缺任何一项都当场 panic**，不接受"少一行就算了"：
/// 少一行意味着那条规则不再有实测，而它看着 Still 像被覆盖过。
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct S14Rule {
    pub init: Option<i32>,
    pub per_hit: Option<(i32, i32)>,
    pub death_at: Option<i32>,
    pub symmetry: bool,
    pub lines: Vec<usize>,
}

/// 把围栏行折叠成两侧的规则。`need` 的措辞故意写成"缺这行会怎样"，让哪天文档真删了一行时，
/// 红字直接说出该补哪一行，而不是留一个 `None` 悄悄跳过实测。
#[cfg(test)]
pub(crate) fn s14_fold(side: &'static str, rows: &[S14Line]) -> S14Rule {
    let mut r = S14Rule { init: None, per_hit: None, death_at: None, symmetry: false, lines: Vec::new() };
    for c in rows.iter().filter(|c| c.side == side) {
        r.lines.push(c.line);
        match c.claim {
            S14Claim::Side(_) => {}
            S14Claim::Init(v) => r.init = Some(v),
            S14Claim::PerHit { dmg, shrink } => r.per_hit = Some((dmg, shrink)),
            S14Claim::DeathAt(v) => r.death_at = Some(v),
            S14Claim::Symmetry => r.symmetry = true,
        }
    }
    let need = |what: &str| -> ! { panic!("§十四「{side}持业者」一侧缺「{what}」这一行 ⇒ 该侧无法复现，先把文档补回来") };
    r.init.unwrap_or_else(|| need("初始长度"));
    r.per_hit.unwrap_or_else(|| need("每受到X伤害 → 蜡烛减短Y单位"));
    r.death_at.unwrap_or_else(|| need("蜡烛长度≤N → 烛尽"));
    r
}

#[cfg(test)]
mod rule_tests {
    use super::*;
    use crate::model::CardId;

    fn fresh_battle() -> Battle {
        Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1)
    }

    #[test]
    fn refund_pct_schedule_100_50_25_10() {
        assert_eq!(
            (refund_pct(0), refund_pct(1), refund_pct(2), refund_pct(3), refund_pct(9)),
            (100, 50, 25, 10, 10)
        );
    }

    #[test]
    fn death_refund_decays_per_instance() {
        let mut b = fresh_battle();
        let def = faction_cards(Faction::Ember)[8]; // 雷烬 4费
        let mut c = CardInst::new(900, def);
        for (d, want) in [(0u32, 4i32), (1, 2), (2, 1), (3, 0)] {
            c.deaths = d;
            let before = b.p_karma;
            b.on_death(c.clone(), SideK::Player, None, DeathCause::Battle);
            assert_eq!(b.p_karma - before, want, "deaths_before={d}");
        }
    }

    #[test]
    fn sacrifice_refunds_full_without_decay() {
        let mut b = fresh_battle();
        let def = faction_cards(Faction::Ember)[8];
        let mut c = CardInst::new(901, def);
        c.deaths = 5; // 即使已死很多次，献祭仍全额
        let before = b.p_karma;
        b.on_death(c, SideK::Player, None, DeathCause::Sacrifice);
        assert_eq!(b.p_karma - before, 4);
    }

    #[test]
    fn rollback_d5_a4_shaves_one() {
        let mut b = fresh_battle();
        b.p_candle = 3;
        b.dealt_this_turn = 4;
        b.pending_candle_d = 5;
        b.enemy_settle();
        assert_eq!(b.p_candle, 2);
        assert_eq!(b.rollback_left, 1);
        assert_eq!(b.karma_penalty_next, 1);
        assert!(b.over.is_none());
    }

    #[test]
    fn rollback_d5_a7_excess_two_hits_holder() {
        let mut b = fresh_battle();
        b.p_candle = 3;
        b.dealt_this_turn = 7;
        let mut atk = CardInst::new(902, faction_cards(Faction::Ember)[1]);
        atk.seq = b.seq;
        b.seq += 1;
        atk.hp = 5;
        b.attack_order.push(atk.seq);
        b.p_front[0] = Some(atk);
        b.pending_candle_d = 5;
        b.enemy_settle();
        assert_eq!(b.p_candle, 3, "A≥D 蜡烛不减");
        assert_eq!(b.e_candle, 18, "超额2按攻击顺序分配→敌方持业者");
        assert_eq!(b.rollback_left, 1);
    }

    #[test]
    fn rollback_d5_a5_exact_cancels_all() {
        let mut b = fresh_battle();
        b.p_candle = 3;
        b.dealt_this_turn = 5;
        b.pending_candle_d = 5;
        b.enemy_settle();
        assert_eq!(b.p_candle, 3, "A=D 全抵消，蜡烛不减不超");
        assert_eq!(b.e_candle, CANDLE_HP, "无超额可分配");
        assert_eq!(b.rollback_left, 1);
    }

    /// 按 §十五 示例自己给的输入搭引擎并跑一次统一结算。
    /// `dealt > d` 时必须摆一个"按攻击顺序在场的攻击者"：否则超额伤害按 §廿二 判给不到攻击者而消散，
    /// 示例 657「对敌方正常造成2伤害」就复现不出来。这不是测试自由发挥——文档说 A 是"本回合**攻击阶段**
    /// 打出的伤害总和"，有 A 就意味着那回合真有过攻击者（`§十五:619`）。
    fn settle_from_doc_inputs(candle: i32, d: i32, dealt: i32) -> Battle {
        let mut b = fresh_battle();
        b.p_candle = candle;
        b.dealt_this_turn = dealt;
        b.pending_candle_d = d;
        if dealt > d {
            let mut atk = CardInst::new(902, faction_cards(Faction::Ember)[1]);
            atk.seq = b.seq;
            b.seq += 1;
            atk.hp = 5;
            b.attack_order.push(atk.seq);
            b.p_front[0] = Some(atk);
        }
        b.enemy_settle();
        b
    }

    /// §十五 示例块（文档 644–659）的**黄金实测**：期望值一行都不写在代码里，全部从文档读进来
    /// （`parse_section15_examples`），再用文档自己给的输入驱动 `enemy_settle`，逐条核对文档自己写的数。
    /// 于是"文档改了示例、引擎没跟着改"与"引擎改了口径、文档还写着旧数"**两边都会红**——这两条挂锚都拦不住：
    /// 锚点只证明有代码行指过来，不证明那一行算出的数等于文档写的数。
    /// 推导口径与 `model.rs` 的 §十五 反向覆盖共用同一个解析器（两处各写一遍会朝不同方向错）。
    #[test]
    fn section15_worked_examples_reproduce_on_the_engine() {
        let Some(lines) = crate::model::doc_or_skip() else { return };
        let claims = parse_section15_examples(&lines);
        assert_eq!(claims.len(), 14, "§十五 示例围栏按形态应解析出 14 条断言，实测 {} 条 ⇒ 文档增删了示例行", claims.len());
        assert_eq!(
            claims.iter().map(|c| c.line).collect::<Vec<_>>(),
            vec![644, 645, 646, 648, 649, 650, 651, 652, 654, 655, 656, 657, 658, 659],
            "§十五 示例的行号名单变了 ⇒ 文档重排过示例，请连 model.rs 推导器里的钉一起改"
        );

        let mut candle = CANDLE_HP;
        let mut d = 0;
        let mut dealt = 0;
        let mut eng: Option<Battle> = None;
        for c in &claims {
            let tag = || format!("md:{}「{}」", c.line, lines[c.line - 1].trim());
            match c.claim {
                S15Claim::Candle(v) => {
                    eng = None;
                    candle = v;
                }
                S15Claim::EnemyDamage(v) => {
                    eng = None;
                    d = v;
                }
                S15Claim::Dealt(v) => {
                    eng = None;
                    dealt = v;
                }
                out => {
                    if eng.is_none() {
                        eng = Some(settle_from_doc_inputs(candle, d, dealt));
                    }
                    let b = eng.as_ref().unwrap();
                    let remain = (d - dealt).max(0);
                    let excess = (dealt - d).max(0);
                    match out {
                        S15Claim::Triggered(lhs, rhs) => {
                            assert_eq!((lhs, rhs), (d, candle), "{} 写的两个操作数与本场景输入（D={d}，蜡烛={candle}）不符 ⇒ 文档内部就不自洽", tag());
                            assert!(lhs >= rhs, "{} 写着 {lhs} ≥ {rhs} 却触发回滚？", tag());
                            assert_eq!(b.rollback_left, 1, "{} 文档说触发回滚，引擎的每局预算却没被扣（2 应剩 1）", tag());
                        }
                        S15Claim::Remaining(dd, aa, result) => {
                            assert_eq!((dd, aa), (d, dealt), "{} 剩余伤害的操作数与本场景输入不符", tag());
                            assert_eq!(result, remain, "{} 文档写的结果 ≠ max(0, D-A)={}，引擎实算蜡烛 {}→{}", tag(), remain, candle, b.p_candle);
                            assert_eq!(b.p_candle, candle - result, "{} 引擎的蜡烛没按文档写的剩余伤害减短", tag());
                        }
                        S15Claim::CandleShorn(shaved, after) => {
                            assert_eq!(shaved, remain, "{} 减短量 ≠ max(0, D-A)={remain}", tag());
                            assert_eq!(after, candle - shaved, "{} 文档自己的算术就不自洽（{candle} 减 {shaved} 应得 {}）", tag(), candle - shaved);
                            assert_eq!(b.p_candle, after, "{} 引擎算出的蜡烛长度 ≠ 文档写的 {after}", tag());
                        }
                        S15Claim::DealtConsumed => {
                            assert_eq!(excess, 0, "{} 说 A 全额消耗掉且不外溢，本场景却有超额 {excess}", tag());
                            assert_eq!(b.e_candle, CANDLE_HP, "{} 我方已打出的伤害外溢到了敌方持业者", tag());
                        }
                        S15Claim::KarmaMinusNext(n) => {
                            assert_eq!(b.karma_penalty_next, n, "{} 引擎没有登记\"下回合业力-{n}\"", tag());
                            let mut after = settle_from_doc_inputs(candle, d, dealt);
                            after.player_turn_start();
                            assert_eq!(after.p_karma, (b.p_karma - n).max(0), "{} 下回合业力没有按 -{n}（最低0）落地", tag());
                            assert_eq!(after.karma_penalty_next, 0, "{} 代价登记没有被消费，会每回合重复扣", tag());
                        }
                        S15Claim::Excess(aa, dd, result) => {
                            assert_eq!((aa, dd), (dealt, d), "{} 超额伤害的操作数（写的 {aa}−{dd}）与本场景输入（A={dealt}，D={d}）不符", tag());
                            assert_eq!(result, excess, "{} 文档写的结果 ≠ max(0, A-D)={excess}", tag());
                        }
                        S15Claim::FullyOffset => {
                            assert_eq!(remain, 0, "{} 说完全抵消，本场景 D={d} A={dealt} 却还剩 {remain}", tag());
                            assert_eq!(b.p_candle, candle, "{} 完全抵消后我方蜡烛仍被减短", tag());
                        }
                        S15Claim::ExcessHitsHolder(x) => {
                            assert_eq!(x, excess, "{} 超额写的 {x} ≠ 引擎的超额 {excess}", tag());
                            assert_eq!(b.e_candle, CANDLE_HP - x, "{} 超额 {x} 没有按\"无卡则攻击敌方持业者\"打到持业者身上", tag());
                        }
                        S15Claim::CandleIntact(after) => {
                            assert_eq!(remain, 0, "{} 说不减短，却有剩余伤害 {remain}", tag());
                            assert_eq!(b.p_candle, after, "{} 引擎的蜡烛长度 ≠ 文档写的 {after}", tag());
                        }
                        S15Claim::Candle(_) | S15Claim::EnemyDamage(_) | S15Claim::Dealt(_) => unreachable!(),
                    }
                }
            }
        }
    }

    /// §十六「触发示例」三行（687/688/689）用**文档自己写的数字**驱动引擎跑一遍：每行给一个阈值与一个
    /// 起始业火值，要求引擎的触发次数与剩余业火值都等于文档写的那两个数。
    /// 为什么这三行走实测不走挂锚：见 `model.rs` 的 §十六 推导器注释——示例行是规则行的重说，
    /// 锚点证不了"算出的数＝写的数"。
    /// 689 那行额外咬住 §十六:680 的"每回合最多1次"：12−6=6 仍 ≥ 阈值，闸若被摘掉引擎会连爆两次，
    /// 文档说 1 次 ⇒ 当场红。这是本章唯一一条"行为"级的牙（其余示例行咬的是减法本身）。
    #[test]
    fn section16_worked_examples_reproduce_on_the_engine() {
        let Some(lines) = crate::model::doc_or_skip() else { return };
        let cases = parse_section16_examples(&lines);
        assert_eq!(cases.len(), 3, "§十六 示例围栏应有 3 行可实测，实测 {} 行", cases.len());
        for c in cases {
            let tag = format!("md:{}「{}」", c.line, lines[c.line - 1].trim());
            assert_eq!(
                (c.lhs, c.rhs),
                (c.before, c.threshold),
                "{tag} 写的减法操作数与本行给的输入（业火值={}，阈值={}）不符 ⇒ 文档内部就不自洽",
                c.before,
                c.threshold
            );
            assert_eq!(c.lhs - c.rhs, c.after, "{tag} 文档自己的算术不自洽：{}−{}＝{}，却写着 {}", c.lhs, c.rhs, c.lhs - c.rhs, c.after);
            let mut b = fresh_battle();
            let def = faction_cards(Faction::Ember)
                .iter()
                .copied()
                .find(|d| d.threshold == c.threshold && is_threshold_trait(d.tr))
                .unwrap_or_else(|| panic!("{tag} 需要一个阈值＝{} 且带阈值特性的烬火教团卡，代码里没有 ⇒ 换卡或扩本测的取卡面", c.threshold));
            let mut card = CardInst::new(900, def);
            card.flame = c.before;
            b.p_front[0] = Some(card);
            b.check_all_triggers();
            let bursts = b.log.iter().filter(|l| l.contains("业火爆发") && l.contains(def.name)).count();
            assert_eq!(bursts, c.triggers as usize, "{tag} 文档说触发 {} 次，引擎实际触发 {bursts} 次", c.triggers);
            let after = b.p_front[0].as_ref().unwrap_or_else(|| panic!("{tag} 触发后这张卡从场上消失了")).flame;
            assert_eq!(after, c.after, "{tag} 引擎剩余业火值 ≠ 文档写的 {}", c.after);
        }
    }

    /// §十四「蜡烛视觉」围栏（590–599）的**黄金实测**：初始长度、每受 X 伤减短 Y、判死上界三个数全部从文档
    /// 读进来（`parse_section14_candle_rules`），再用它们逐发驱动引擎，核对蜡烛序列与判死时点——
    /// 期望值一行都不写在代码里。挂锚证不了"算出的数＝写的数"，所以这条路必须真跑。
    /// 与 §十五/§十六 的示例行不同，这九行**同时**也有锚点指回代码（本仓第一次同章双记账）；
    /// 为什么这不是把「必须跑一遍」降级成「有人指过来就行」，见 `model.rs` 的 §十四 推导器注释。
    #[test]
    fn section14_candle_numbers_reproduce_on_the_engine() {
        let Some(lines) = crate::model::doc_or_skip() else { return };
        let rows = parse_section14_candle_rules(&lines);
        assert_eq!(rows.len(), 9, "§十四 蜡烛围栏按形态应解析出 9 行（2 个侧标签＋7 条规则行），实测 {} 行 ⇒ 文档增删了围栏行", rows.len());
        assert_eq!(
            rows.iter().map(|c| c.line).collect::<Vec<_>>(),
            vec![590, 591, 592, 593, 595, 596, 597, 598, 599],
            "§十四 围栏的行号名单变了 ⇒ 文档重排过蜡烛视觉，请连 model.rs 推导器里的钉一起改"
        );
        let p = s14_fold("我方", &rows);
        let e = s14_fold("敌方", &rows);
        assert_eq!(p.lines, vec![590, 591, 592, 593], "§十四 我方侧应正好四行（标签＋3 规则），实测 {:?}", p.lines);
        assert_eq!(e.lines, vec![595, 596, 597, 598, 599], "§十四 敌方侧应正好五行（标签＋3 规则＋对称），实测 {:?}", e.lines);
        let (p_init, p_death) = (p.init.unwrap(), p.death_at.unwrap());
        let (p_dmg, p_shrink) = p.per_hit.unwrap();
        let (e_init, e_death) = (e.init.unwrap(), e.death_at.unwrap());
        let (e_dmg, e_shrink) = e.per_hit.unwrap();
        assert!(e.symmetry, "md:599 的「视觉与玩家持业者对称」没被折成 Symmetry ⇒ 解析器与本测口径分叉");
        // 两侧数字必须一致——那是 md:599「对称」在数字层面的意思，也是本测敢用同一套驱动跑两侧的前提。
        assert_eq!(
            (p_init, p_dmg, p_shrink, p_death),
            (e_init, e_dmg, e_shrink, e_death),
            "§十四 两侧数字不一致：我方 初始{p_init}／每受{p_dmg}减{p_shrink}／≤{p_death}判死，敌方 初始{e_init}／每受{e_dmg}减{e_shrink}／≤{e_death}判死 ⇒ 文档自己不再对称，先定文档口径"
        );
        assert!(p_shrink > 0, "md:592 的减短量是 {p_shrink}，非正数推不出判死所需发数");
        assert_eq!(p_init, CANDLE_HP, "文档写的初始长度 {p_init} ≠ 引擎常量 CANDLE_HP {CANDLE_HP}（md:591／md:596 两侧同读这一个常量）");
        let hits = (p_init - p_death + p_shrink - 1) / p_shrink;

        // 我方一侧：§十五 的回滚闸排在裸减烛之前，先把预算清零——本章复现的是"每受 X 伤减 Y"这条裸规则本身。
        // 判死要**两个方向**都咬：蜡烛还没到上界引擎不许结束（早死），一旦到了上界引擎必须立刻结束（晚死）。
        // 只核对最后那一发是咬不住"文档把判死线抬高"的（本帧 M45 实测到才补上）。
        let mut b = fresh_battle();
        b.rollback_left = 0;
        let mut candle = p_init;
        let mut died = None;
        for k in 1..=hits {
            let before = b.p_candle;
            assert_eq!(before, candle, "我方第 {k} 发前：引擎蜡烛 {before} ≠ 文档推得的 {candle}");
            b.dealt_this_turn = 0;
            b.pending_candle_d = p_dmg;
            b.enemy_settle();
            candle -= p_shrink;
            assert_eq!(b.p_candle, candle, "我方第 {k} 发：文档说每受 {p_dmg} 伤害减短 {p_shrink}，引擎实减 {}", before - b.p_candle);
            if candle <= p_death {
                assert_eq!(b.over, Some(Outcome::PlayerLose), "第 {k} 发后蜡烛剩 {candle}，已到文档写的「≤{p_death} 即烛尽」，引擎却没判死（over={:?}）", b.over);
                died = Some(k);
                break;
            }
            assert_eq!(b.over, None, "第 {k} 发后蜡烛还有 {candle}，没到「≤{p_death}」这条线，引擎却已结束在 {:?}", b.over);
        }
        assert_eq!(died, Some(hits), "判死发数没落在文档数字推定的第 {hits} 发（初始 {p_init}／每受 {p_dmg} 减 {p_shrink}／≤{p_death} 判死），实测第 {died:?} 发");

        // 敌方一侧：同一组数字，走 `damage_enemy_holder` 那条直击路（单烛，Boss 双烛另见 model.rs 的边界登记）。
        let mut b = fresh_battle();
        let mut candle = e_init;
        let mut died = None;
        for k in 1..=hits {
            let before = b.e_candle;
            assert_eq!(before, candle, "敌方第 {k} 发前：引擎蜡烛 {before} ≠ 文档推得的 {candle}");
            b.damage_enemy_holder(e_dmg, None, HolderHit::Direct);
            candle -= e_shrink;
            assert_eq!(b.e_candle, candle, "敌方第 {k} 发：文档说每受 {e_dmg} 伤害减短 {e_shrink}，引擎实减 {}", before - b.e_candle);
            if candle <= e_death {
                assert_eq!(b.over, Some(Outcome::PlayerWin), "第 {k} 发后敌方蜡烛剩 {candle}，已到「≤{e_death} 即烛尽」，引擎却没判我方胜（over={:?}）", b.over);
                died = Some(k);
                break;
            }
            assert_eq!(b.over, None, "第 {k} 发后敌方蜡烛还有 {candle}，没到「≤{e_death}」，引擎却已结束在 {:?}", b.over);
        }
        assert_eq!(died, Some(hits), "敌方判死发数没落在文档推定的第 {hits} 发，实测第 {died:?} 发");

        // md:599「对称」的行为面：同一长度在两侧渲染出**逐字节相同**的那一串蜡烛（不是"看着差不多"）。
        let mut b = fresh_battle();
        let shown = p_init / 2;
        b.p_candle = shown;
        b.e_candle = shown;
        let text = crate::render::render_board(&b.view());
        let bar = crate::render::candle_bar(shown, CANDLE_HP);
        for who in ["我方持业者", "敌方持业者"] {
            let line = text
                .lines()
                .find(|l| l.starts_with(who))
                .unwrap_or_else(|| panic!("渲染里没有以「{who}」开头的一行 ⇒ 渲染层改动会先撞这里"));
            assert!(line.contains(&bar), "「{who}」那一行里没有 `candle_bar({shown}, {CANDLE_HP})` 的字节 ⇒ 两侧渲染路径已分叉，md:599 的「对称」破了（实测「{line}」期望含「{bar}」）");
        }
    }

    /// §十五:630「回滚产生的超额伤害不参与后续回滚」是一条**否定式**条款：锚点只能指到"这里没有 `dealt_this_turn +=`"，
    /// 机检抓不住哪天有人把那一行加回去。这条测就是给那句缺席补上的牙——连着触发两次回滚，
    /// 第二次的额度必须还是 7；若第一次的 2 点超额回流进口径 A（变成 9），第二次会多打出 2 点。
    #[test]
    fn rollback_excess_does_not_flow_back_into_the_dealt_ledger() {
        let mut b = settle_from_doc_inputs(3, 5, 7);
        assert_eq!(b.dealt_this_turn, 7, "第一次结算本身不许改动口径 A（超额是派生量，不是新打出的伤害）");
        assert_eq!(b.e_candle, CANDLE_HP - 2, "先确认这一发确实产生了 2 点超额，否则本测什么也没证");
        assert_eq!(b.rollback_left, 1);
        b.pending_candle_d = 5;
        b.enemy_settle();
        assert_eq!(b.rollback_left, 0, "第二次回滚应真的触发（每局 2 次预算见底）");
        assert_eq!(b.e_candle, CANDLE_HP - 4, "第二次仍按 A=7 只外溢 2；若拿到 {} 就说明第一次的超额回流进了 A", b.e_candle);
        assert_eq!(b.dealt_this_turn, 7, "两次回滚后口径 A 仍是那发攻击的 7");
    }

    #[test]
    fn flame_12_over_thr6_triggers_once_keeps_6() {
        let mut b = fresh_battle();
        let mut c = CardInst::new(903, faction_cards(Faction::Ember)[3]); // 焚稿人 阈6
        c.flame = 12;
        b.p_front[1] = Some(c);
        b.check_all_triggers();
        let f = b.p_front[1].as_ref().unwrap();
        assert_eq!(f.flame, 6, "12-6=6 溢出保留，本回合不再触发");
        assert_eq!(f.triggered_turn, b.turn);
    }

    #[test]
    fn death_ladder_observable_via_recycle() {
        let mut b = fresh_battle();
        let c = CardInst::new(911, faction_cards(Faction::Ember)[8]); // 雷烬 4费
        b.on_death(c, SideK::Player, Some(0), DeathCause::Battle);
        assert_eq!(b.p_karma, 4, "第一次死亡 100%");
        let returned = b.discard_pile.pop().expect("基础牌阵亡应进弃牌堆");
        assert_eq!(returned.deaths, 1);
        assert_eq!(returned.flame, 0, "死亡业火清零");
        let before = b.p_karma;
        b.on_death(returned, SideK::Player, Some(0), DeathCause::Battle);
        assert_eq!(b.p_karma - before, 2, "同一实例第二次死亡→50%（§三91行）");
        assert_eq!(b.discard_pile.pop().unwrap().deaths, 2);
    }

    #[test]
    fn starter_and_crafted_never_recycle() {
        let mut b = fresh_battle();
        b.on_death(CardInst::new(912, crate::model::STARTER), SideK::Player, Some(0), DeathCause::Battle);
        assert_eq!(b.p_karma, 2, "开端死亡定额2");
        assert!(b.discard_pile.is_empty(), "开端离场不入任何堆");
        let mut fused = CardInst::new(913, faction_cards(Faction::Ember)[3]);
        fused.crafted = true;
        let k0 = b.p_karma;
        b.on_death(fused, SideK::Player, Some(0), DeathCause::Battle);
        assert_eq!(b.p_karma, k0 + 3, "自造牌死亡返还照给（3费100%）");
        assert!(b.discard_pile.is_empty(), "自造牌阵亡永久消失（§十372）");
    }

    #[test]
    fn sacrifice_recycles_without_decay() {
        let mut b = fresh_battle();
        let mut c = CardInst::new(914, faction_cards(Faction::Ember)[8]);
        c.deaths = 1;
        let k0 = b.p_karma;
        b.on_death(c, SideK::Player, None, DeathCause::Sacrifice);
        assert_eq!(b.p_karma - k0, 4, "献祭全额");
        let d = &b.discard_pile[0];
        assert_eq!(d.deaths, 1, "献祭不使死亡计数+1（返还互斥递减，§三100行）");
    }

    #[test]
    fn field_sacrifice_rejects_out_of_range_slot_instead_of_panicking() {
        // 真机崩溃复现（`play --seed 7` 战斗阶段输入 `s P5` ⇒ battle.rs index out of bounds，rc=101）：
        // 当年命令行把格位读成"取末位数字"，P5/P6/P9/非数字一律折成 9 或 4-8，而本函数直接下标 `p_front`。
        // 那道读法现已在 `command::parse_slot` 改成严格集合（P1-P4），但引擎这道闸照留：
        // 壳层挡不住的所有调用方（TUI/2D、以后的脚本）都只信任这里。
        // 放置侧 `player_place` 早有 `col >= 4` 闸（§十二:431 的"献祭"步只写了规则没写边界）。
        let mut b = fresh_battle();
        for col in [4, 5, 8, 9] {
            assert_eq!(b.player_sacrifice_field(col).err().as_deref(), Some("格位为 P1-P4"), "col={col}");
        }
        assert!(!b.pf.sacrifice_used, "越界请求不得消耗每回合1次的献祭额度");
    }

    #[test]
    fn hand_sacrifice_quota_only_starter_exempt() {
        let mut b = fresh_battle();
        let st = b.hand.iter().position(|c| c.is_starter()).expect("手牌应有开端");
        assert!(b.player_sacrifice_hand(st).is_ok());
        assert!(!b.pf.sacrifice_used, "开端手牌献祭不占额度（裁定10）");
        b.hand.push(CardInst::new(950, faction_cards(Faction::Ember)[1]));
        let i = b.hand.len() - 1;
        assert!(b.player_sacrifice_hand(i).is_ok());
        assert!(b.pf.sacrifice_used, "普通手牌献祭占每回合1次额度");
        b.hand.push(CardInst::new(951, faction_cards(Faction::Ember)[2]));
        let j = b.hand.len() - 1;
        assert!(b.player_sacrifice_hand(j).is_err(), "额度用尽后普通手牌献祭应被拒");
        b.hand.push(CardInst::new(952, crate::model::STARTER));
        assert!(b.player_sacrifice_hand(b.hand.len() - 1).is_ok(), "开端豁免不受额度影响");
    }

    #[test]
    fn draw_is_from_top_of_pile() {
        let mut b = fresh_battle();
        let expect_id = b.draw_pile[0].id;
        b.pf.manual_draws = 1;
        b.action_draw(false).unwrap();
        assert_eq!(b.hand.last().unwrap().id, expect_id, "继承堆顺序决定抽牌（裁定11）");
    }

    #[test]
    fn adjacent_attack_flame_skill_follows_target_col() {
        // §八技能2（292行）：攻击后**目标**同列+1业火（s06-24 修复回归）
        let mut b = fresh_battle();
        let mut atk = CardInst::new(960, faction_cards(Faction::Ember)[2]); // 引燃者 可攻击相邻列
        atk.skills.push(Skill::AtkSameColFlame1);
        atk.seq = b.seq;
        b.seq += 1;
        b.p_front[0] = Some(atk); // hp3，col0 无敌前排 → 相邻列攻击 col1
        let mut guard = CardInst::new(961, faction_cards(Faction::Frost)[1]);
        guard.seq = b.seq;
        b.seq += 1;
        guard.hp = 9;
        b.e_front[1] = Some(guard);
        b.player_attack_phase();
        let g = b.e_front[1].as_ref().expect("守卫应存活");
        assert_eq!(g.flame, 3 + 1, "伤害3 + 技能2对目标列+1");
        assert_eq!(b.p_front[0].as_ref().unwrap().flame, 0, "攻击者自身列不应吃到+1");
    }

    #[test]
    fn stacked_skills_count_per_copy() {
        // 裁定12（§九"技能可叠加"）：被动光环按出现次数计层
        let mut b = fresh_battle();
        let mut atk = CardInst::new(970, faction_cards(Faction::Ember)[1]); // 火苗 数值2
        atk.seq = b.seq;
        b.seq += 1;
        let mut buddy = CardInst::new(971, faction_cards(Faction::Ember)[1]);
        buddy.skills = vec![Skill::AllyColAtk1, Skill::AllyColAtk1];
        buddy.seq = b.seq;
        b.seq += 1;
        b.e_front[0] = Some(atk);
        b.e_back[0] = Some(buddy);
        assert_eq!(b.attack_power(SideK::Enemy, 970, 0, 2), 4, "双份同列友方攻击+1 → +2");
    }

    #[test]
    fn stacked_threshold_reduction_counts_and_floors_at_one() {
        let mut b = fresh_battle();
        let mut tgt = CardInst::new(972, faction_cards(Faction::Ember)[11]); // 守夜人 阈值6
        tgt.seq = b.seq;
        b.seq += 1;
        let mut holder = CardInst::new(973, faction_cards(Faction::Ember)[1]);
        holder.skills = vec![Skill::AllyColThreshM1; 2];
        holder.seq = b.seq;
        b.seq += 1;
        b.e_front[0] = Some(tgt);
        b.e_back[0] = Some(holder);
        let t = b.e_front[0].as_ref().unwrap();
        assert_eq!(b.effective_threshold(SideK::Enemy, 0, t), 4, "双份阈值-1 → −2");
        b.e_back[0].as_mut().unwrap().skills = vec![Skill::AllyColThreshM1; 10];
        let t = b.e_front[0].as_ref().unwrap();
        assert_eq!(b.effective_threshold(SideK::Enemy, 0, t), 1, "阈值下限1");
    }

    #[test]
    fn holder_hit_damage_is_debuffed_by_same_column_enemy() {
        // §八技能6 锚攻击者列：直击持业者时同列敌方"攻击-1"仍生效（s06-28）
        let mut b = fresh_battle();
        let mut atk = CardInst::new(974, faction_cards(Faction::Ember)[3]); // 焚稿人 数值4
        atk.seq = b.seq;
        b.seq += 1;
        b.p_front[0] = Some(atk);
        let mut wall = CardInst::new(975, faction_cards(Faction::Frost)[4]); // 寒哨
        wall.seq = b.seq;
        b.seq += 1;
        b.e_back[0] = Some(wall);
        assert_eq!(b.holder_hit_damage(SideK::Player, 974, 0, 4), 3, "后排寒哨削弱直击");
    }

    #[test]
    fn skill8_holder_self_adds_to_own_damage() {
        // 裁定13：同列敌方受伤+1 不排除持有者自身
        let mut b = fresh_battle();
        let mut atk = CardInst::new(976, faction_cards(Faction::Ember)[1]);
        atk.skills = vec![Skill::EnemyColDmgTakenP1];
        atk.seq = b.seq;
        b.seq += 1;
        let mut def = CardInst::new(977, faction_cards(Faction::Frost)[1]);
        def.seq = b.seq;
        b.seq += 1;
        b.p_front[0] = Some(atk);
        b.e_front[0] = Some(def);
        assert_eq!(b.card_hit_damage(SideK::Player, 976, 0, 0, 2, false), 3, "持有者自己吃+1");
    }

    #[test]
    fn night_watch_boost_window_amplifies_later_flame_gains() {
        // 裁定5 字面化：守夜人＝本回合该侧每次业火获得事件额外+2
        let mut b = fresh_battle();
        let mut watch = CardInst::new(978, faction_cards(Faction::Ember)[11]);
        watch.flame = watch.base_threshold();
        watch.seq = b.seq;
        b.seq += 1;
        b.p_front[0] = Some(watch);
        let mut gain = CardInst::new(979, faction_cards(Faction::Ember)[1]);
        gain.seq = b.seq;
        b.seq += 1;
        b.p_front[1] = Some(gain);
        let mut foe = CardInst::new(980, faction_cards(Faction::Frost)[1]);
        foe.seq = b.seq;
        b.seq += 1;
        b.e_front[1] = Some(foe);
        b.check_all_triggers();
        assert_eq!(b.boost_for(SideK::Player), 2, "增益窗口已开启");
        assert_eq!(b.boost_for(SideK::Enemy), 0, "敌方不受益");
        b.add_flame_col(SideK::Player, 1, 1, None);
        assert_eq!(b.p_front[1].as_ref().unwrap().flame, 3, "我方后续+1实得+3");
        assert_eq!(b.e_front[1].as_ref().unwrap().flame, 1, "敌方无窗口，按原值");
        b.turn += 1;
        assert_eq!(b.boost_for(SideK::Player), 0, "窗口只在触发当回合有效");
    }

    #[test]
    fn full_col_damage3_in_attack_phase_counts_into_rollback_a() {
        // §十二：攻击阶段内造成的敌方伤害计入回滚口径 A（s12-12）
        let mut b = fresh_battle();
        let mut blaze = CardInst::new(981, faction_cards(Faction::Ember)[10]); // 山火
        blaze.flame = blaze.base_threshold();
        blaze.seq = b.seq;
        b.seq += 1;
        b.p_front[0] = Some(blaze);
        let mut foe = CardInst::new(982, faction_cards(Faction::Frost)[1]);
        foe.hp = 9;
        foe.seq = b.seq;
        b.seq += 1;
        b.e_front[0] = Some(foe);
        b.in_player_attack_phase = true;
        b.check_all_triggers();
        assert_eq!(b.e_front[0].as_ref().unwrap().hp, 6, "整列3伤");
        assert_eq!(b.dealt_this_turn, 3, "攻击阶段内的敌方伤害计入A");
    }
    /// §十七:763／§十七:765 里**可证伪**的那半：挤压的后果全部留在**本侧**——被挤的牌进本侧弃牌堆、
    /// 业力回本侧，对面棋盘逐格不动、对面业力与蜡烛一字节不动。`DeathCause` 里不存在"越过之后落到对面"
    /// 的第四因，但那半是否定式条款、机检证伪不了（同 §十五:621）；这条测钉住它可测的部分。
    #[test]
    fn a_squeezed_card_dies_at_its_own_line_and_never_reaches_the_other_side() {
        // 对面四列前排＋四列后排的逐格身份快照（`CardInst` 没有 `PartialEq`，比 id 就够，不为一条测去改类型）
        let enemy_ids = |b: &Battle| -> Vec<Option<CardId>> {
            let mut v: Vec<Option<CardId>> = Vec::new();
            for row in [&b.e_front[..], &b.e_back[..]] {
                for cell in row {
                    v.push(cell.as_ref().map(|c| c.id));
                }
            }
            v
        };
        let mut b = fresh_battle();
        // 两张都取**非开端**牌：开端的死亡返还是定额 2（§十五 裁定1 同源），混进来会让业力断言测的不再是递减路径。
        let victim = b.hand.remove(b.hand.iter().position(|c| !c.is_starter()).expect("开局手牌含非开端牌"));
        let (victim_id, victim_cost) = (victim.id, victim.def.cost);
        b.p_front[0] = Some(victim);
        let (enemy0, e_karma0, candle0) = (enemy_ids(&b), b.e_karma, b.e_candle);
        let push = b.hand.remove(b.hand.iter().position(|c| !c.is_starter()).expect("还剩一张非开端牌作挤入者"));
        let (push_id, push_cost) = (push.id, push.def.cost);
        let hand_idx = b.hand.len();
        b.hand.push(push);
        b.p_karma = 99;
        b.player_place(hand_idx, 0).expect("我方放置不查目标格占用＝主动挤压");
        assert_eq!(b.p_front[0].as_ref().unwrap().id, push_id, "新卡占位");
        let dead = b.discard_pile.last().expect("被挤牌进本侧弃牌堆");
        assert_eq!(dead.id, victim_id, "弃的是被挤那张本人");
        assert_eq!(dead.deaths, 1, "越线死亡推进死亡档位（走递减）");
        assert_eq!(
            b.p_karma,
            99 - push_cost + victim_cost * refund_pct(0) / 100,
            "付新卡费；被挤牌新实例 100% 返还，且回的是**我方**业力"
        );
        assert_eq!(enemy_ids(&b), enemy0, "对面八格逐格不动");
        assert_eq!(b.e_karma, e_karma0, "对面业力不动");
        assert_eq!(b.e_candle, candle0, "对面蜡烛不减——挤压不是攻击");
        assert!(
            !enemy_ids(&b).contains(&Some(victim_id)),
            "被挤的牌出现在对面＝越过中线还在场上，§十七:763 的直接反例"
        );
    }
}

/// Boss 五条特殊规则 + 双烛 + 30 回合判定的单元测试。
/// 与 `rule_tests` 分开：这里全部以 `Battle::new_boss` 构造，普通对局的零影响由 `boss::rule_tests` 守。
#[cfg(test)]
mod boss_rule_tests {
    use super::*;
    use crate::boss::{BossId, BossRule, LEVELS_PER_CHAPTER};

    fn boss_battle(id: BossId) -> Battle {
        Battle::new_boss(2026, Faction::Ember, id, Vec::new(), id.chapter() * LEVELS_PER_CHAPTER)
    }

    /// 造一张场上卡实例（自增 seq，与真实放置同构），由调用方放进目标格。按卡名取，免数下标。
    fn card_of(b: &mut Battle, f: Faction, name: &str) -> CardInst {
        let mut c = CardInst::new(b.seq + 500, crate::model::card_by_name(f, name).expect("脚本卡名须可解析"));
        c.seq = b.seq;
        b.seq += 1;
        c
    }

    #[test]
    fn furnace_heat_flames_every_enemy_card_and_pays_no_karma() {
        let mut b = boss_battle(BossId::Luzhu);
        assert_eq!(b.boss_rule(), BossRule::FurnaceHeat);
        let mut m = card_of(&mut b, Faction::Ember, "火苗");
        m.flame = 2;
        b.e_front[0] = Some(m);
        let karma0 = b.e_karma;
        crate::boss::on_enemy_turn_start(&mut b);
        assert_eq!(b.e_front[0].as_ref().unwrap().flame, 3, "在场卡业火+1");
        assert!(
            b.log.iter().any(|l| l.contains("【炉温】敌方在场 1 张卡业火+1")),
            "炉温须留痕：{:?}",
            b.log.last()
        );
        assert_eq!(b.e_karma, karma0, "脚本自养：回合开始不发业力（裁定20）");
    }

    #[test]
    fn frost_brand_shaves_one_off_card_damage_but_not_starter() {
        let mut b = boss_battle(BossId::Xuejue);
        b.e_front[0] = Some(card_of(&mut b, Faction::Frost, "初霜")); // tr None，不会减攻
        assert_eq!(b.card_hit_damage(SideK::Player, 1, 0, 0, 2, false), 1, "霜封：伤害-1");
        assert_eq!(b.card_hit_damage(SideK::Player, 1, 0, 0, 1, true), 1, "开端不豁免就会归零 ⇒ 不可解");
        // 真走一遍攻击阶段：业火按**减免后**伤害计
        b.p_front[0] = Some(card_of(&mut b, Faction::Ember, "火苗")); // power2
        b.dealt_this_turn = 0;
        b.player_attack_phase();
        let d = b.e_front[0].as_ref().unwrap();
        assert_eq!(d.hp, 1, "初霜 power2 受 1 伤");
        assert_eq!(d.flame, 1, "业火累的是减免后的 1，不是原始 2");
        assert_eq!(b.dealt_this_turn, 1, "减免后伤害才进口径 A");
    }

    #[test]
    fn shadow_push_squeezes_own_occupied_front_and_refunds_cross() {
        let mut b = boss_battle(BossId::Yingzhang);
        let victim = card_of(&mut b, Faction::Shadow, "影仆"); // cost1
        let victim_id = victim.id;
        b.e_front[0] = Some(victim);
        let karma0 = b.e_karma;
        let push = card_of(&mut b, Faction::Shadow, "暗哨"); // cost2
        b.enemy_place_inst(push, 0, Row::Front).expect("暗渡放行压已占前排");
        assert_eq!(b.e_front[0].as_ref().unwrap().def.name, "暗哨");
        assert!(
            b.log.iter().any(|l| l.contains("挤压：") && l.contains("越线死亡")),
            "被压卡须走越线死亡路径"
        );
        assert_eq!(b.e_karma, karma0 - 2 + 1, "付暗哨2费；影仆新实例100%返还=1（裁定20后果）");
        let dead = b.e_discard.last().expect("基础牌离场进弃牌堆");
        assert_eq!(dead.id, victim_id, "被挤的是影仆本人");
        assert_eq!(dead.deaths, 1);
    }

    #[test]
    fn squeeze_needs_the_boss_rule_control_group_is_ordinary_battle() {
        // 无 Boss 的普通对局：同一手放置必须被拒，且棋盘一格不动。
        let mut b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        let keeper = card_of(&mut b, Faction::Frost, "初霜");
        let keeper_id = keeper.id;
        b.e_front[0] = Some(keeper);
        b.e_karma = 99; // 排除"业力不足"这条先撞的检查
        let karma0 = b.e_karma;
        let push = card_of(&mut b, Faction::Frost, "望哨");
        let err = b.enemy_place_inst(push, 0, Row::Front).unwrap_err();
        assert!(err.contains("AI 不挤压"), "普通 AI 不该主动挤越线：{err}");
        assert_eq!(b.e_front[0].as_ref().unwrap().id, keeper_id, "拒绝即无副作用");
        assert_eq!(b.e_karma, karma0, "拒绝即不扣费");
    }

    #[test]
    fn twin_candles_route_by_column_and_mirror_half() {
        let mut b = boss_battle(BossId::YanBing);
        assert_eq!((b.e_candle, b.e_candle2), (20, Some(20)), "炎/冰各20");
        b.damage_enemy_holder(3, Some(0), HolderHit::Direct); // 第1列 → 冰
        assert_eq!(b.e_candle2, Some(17), "冰吃直击3");
        assert_eq!(b.e_candle, 19, "炎吃同源⌊3/2⌋=1");
        assert!(b.over.is_none(), "只削一根不算胜");
        assert!(
            b.log
                .iter()
                .any(|l| l == "  → 直击中线「冰」持业者 -3（炎19 冰17·同源-1）"),
            "文案须同时报两根与同源折损"
        );
        b.damage_enemy_holder(4, Some(3), HolderHit::Excess(4)); // 第4列 → 炎
        assert_eq!(b.e_candle, 15, "炎吃直击4");
        assert_eq!(b.e_candle2, Some(15), "冰吃同源⌊4/2⌋=2");
    }

    #[test]
    fn twin_victory_requires_both_candles_out() {
        let mut b = boss_battle(BossId::YanBing);
        b.e_candle = 1;
        b.e_candle2 = Some(6);
        b.damage_enemy_holder(3, Some(3), HolderHit::Direct); // 炎先尽
        assert!(b.e_candle <= 0 && b.e_candle2.unwrap() > 0);
        assert_eq!(b.over, None, "一根烛尽仍在战（皆尽才胜，裁定19）");
        b.damage_enemy_holder(9, Some(0), HolderHit::Direct); // 冰先尽 → 两根皆尽
        assert_eq!(b.over, Some(Outcome::PlayerWin));
        assert!(b.log.iter().any(|l| l == "双烛皆尽！"));
    }

    #[test]
    fn single_candle_holder_keeps_the_original_wording() {
        let mut b = boss_battle(BossId::Luzhu);
        assert_eq!(b.e_candle, 20);
        b.damage_enemy_holder(20, Some(0), HolderHit::Direct);
        assert!(
            b.log
                .iter()
                .any(|l| l == "  → 直击中线，敌方蜡烛 -20（剩 0）"),
            "单烛路径逐字不变，Boss 不改普通文案"
        );
        assert_eq!(b.over, Some(Outcome::PlayerWin));
    }

    #[test]
    fn turn_limit_compares_against_the_longer_candle() {
        let mut b = boss_battle(BossId::YanBing);
        b.e_candle = 3;
        b.e_candle2 = Some(8);
        assert_eq!(b.enemy_candle_ref(), 8, "取较长值：既非首根3，也非和11");
        b.e_candle = 8;
        b.e_candle2 = Some(3);
        assert_eq!(b.enemy_candle_ref(), 8, "较长值与哪一根在场无关");

        // 接线验证：30 回合判定确实读这个引用，并把口径写进日志。
        b.e_candle = 3;
        b.e_candle2 = Some(8);
        b.p_candle = 100_000; // 沙包：只问判定口径，不让脚本提前把我方打爆
        b.hand.clear();
        for i in 0..4 {
            b.p_front[i] = None;
        }
        let mut guard = 0;
        while b.over.is_none() && guard < 40 {
            b.end_player_turn();
            guard += 1;
        }
        let line = b.log.iter().find(|l| l.contains("回合终：")).expect("应有回合终判定行");
        assert!(line.contains("vs 8（敌方取两根较长值）"), "判定值/口径不符：{line}");
        assert_eq!(b.over, Some(Outcome::PlayerWin), "10万烛 vs 较长值8 → 我方胜");
    }

    #[test]
    fn devour_name_reprices_player_sacrifice_only() {
        let mut b = boss_battle(BossId::ZhongYing);
        assert_eq!(b.boss_rule(), BossRule::DevourName);
        let shadow_shard = crate::model::card_by_name(Faction::Shadow, "蚀").unwrap(); // cost3
        let mut yi = CardInst::new(800, shadow_shard);
        yi.deaths = 1;
        let k0 = b.p_karma;
        b.on_death(yi, SideK::Player, None, DeathCause::Sacrifice);
        assert_eq!(b.p_karma - k0, 1, "吞名：献祭改按死亡递减 3×50%=1（原本全额3）");
        assert_eq!(b.discard_pile.last().unwrap().deaths, 1, "吞名只改本手收益，不推进死亡档位（速查:985 献祭不触发递减）");
        assert!(b.log.iter().any(|l| l.contains("（吞名·按死亡返还递减）")));

        let k1 = b.p_karma;
        b.on_death(CardInst::new(801, crate::model::STARTER), SideK::Player, None, DeathCause::Sacrifice);
        assert_eq!(b.p_karma - k1, 2, "开端献祭仍是定额2（裁定1/10）");

        let k2 = b.e_karma;
        b.on_death(CardInst::new(802, shadow_shard), SideK::Enemy, None, DeathCause::Sacrifice);
        assert_eq!(b.e_karma - k2, 3, "吞名只咬我方，敌方脚本仍全额返还");

        let mut hu = CardInst::new(803, shadow_shard);
        hu.deaths = 1;
        let k3 = b.p_karma;
        b.on_death(hu, SideK::Player, None, DeathCause::Battle);
        assert_eq!(b.p_karma - k3, 1, "战斗死亡本就递减，不该被吞名二次打折");
    }

    #[test]
    fn final_boss_is_thirty_candle_and_announces_its_rule() {
        let b = boss_battle(BossId::ZhongYing);
        assert_eq!(b.e_candle, 30, "终影蜡烛30");
        assert!(b.e_candle2.is_none());
        assert_eq!(b.enemy_candle_ref(), 30);
        assert_eq!(b.holder_names, ["终影", ""]);
        assert_eq!(b.e_karma, b.boss_profile().unwrap().start_karma, "开场业力＝一次性预算");
        assert!(b.log.iter().any(|l| l.contains("— Boss 登场：终影")));
        assert!(b.log.iter().any(|l| l.contains("特殊规则【吞名】")));
    }
}

/// 裁定24「每章新阵营＝三阵营循环 + 强化」的数值层。
/// 这里只锁**结构与不变量**（阶梯、豁免、rng 零扰动、与难度正交），不锁平衡结论：
/// 胜率读数在 meta_tests 里跑，并回填 projects/midline/pool/chapter-strengthening.md。
#[cfg(test)]
mod chapter_strength_tests {
    use super::*;
    use crate::boss::BossId;

    type Snap = Vec<(u64, &'static str, i32, i32, i32, usize)>;

    fn enemy_snap(b: &Battle) -> Snap {
        b.enemy_hand
            .iter()
            .chain(b.enemy_pile.iter())
            .map(|c| (c.id, c.def.name, c.def.power, c.def.threshold, c.hp, c.skills.len()))
            .collect()
    }

    fn ids(b: &Battle) -> (Vec<u64>, Vec<u64>) {
        (
            b.enemy_hand.iter().map(|c| c.id).collect(),
            b.enemy_pile.iter().map(|c| c.id).collect(),
        )
    }

    fn base_power(f: Faction, name: &str) -> (i32, i32) {
        faction_cards(f)
            .iter()
            .find(|d| d.name == name)
            .map(|d| (d.power, d.threshold))
            .unwrap()
    }

    #[test]
    fn strength_ladder_is_chapter_minus_one_capped_at_three() {
        assert_eq!(Battle::chapter_strength(1), 0, "第1章＝基准，必须为零扰动留位");
        assert_eq!(Battle::chapter_strength(12), 0, "第12关仍属第1章");
        assert_eq!(
            (
                Battle::chapter_strength(13),
                Battle::chapter_strength(25),
                Battle::chapter_strength(37),
                Battle::chapter_strength(49),
                Battle::chapter_strength(60)
            ),
            (1, 2, 3, 3, 3),
            "阶梯 1/2/3 后按 §十一:404 的上限封顶"
        );
    }

    #[test]
    fn chapter_one_is_byte_identical_no_op() {
        let mut b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 1);
        let before = enemy_snap(&b);
        let log_len = b.log.len();
        b.apply_chapter_strengthening(1);
        assert_eq!(enemy_snap(&b), before, "第1章不得改动任何敌方牌面");
        assert_eq!(b.log.len(), log_len, "第1章不该多打日志");
    }

    #[test]
    fn strengthening_never_consumes_rng_or_reorders() {
        let plain = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 37);
        let mut s = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 37);
        s.apply_chapter_strengthening(37);
        assert_eq!(ids(&s), ids(&plain), "卡片身份与顺序不得因强化而变动（=没掷过 rng）");
        assert_eq!(s.next_id, plain.next_id);
        assert_eq!(
            s.hand.iter().map(|c| c.id).collect::<Vec<_>>(),
            plain.hand.iter().map(|c| c.id).collect::<Vec<_>>(),
            "我方起手也不得被敌方强化波及"
        );
    }

    #[test]
    fn three_tiers_map_to_the_three_upgrade_effects() {
        let mut b = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), 37);
        b.apply_chapter_strengthening(37);
        let mut seen = 0;
        for c in b.enemy_hand.iter().chain(b.enemy_pile.iter()) {
            if c.is_starter() {
                let st = faction_cards(Faction::Frost)[0];
                assert_eq!((c.def.power, c.def.threshold, c.skills.len()), (st.power, st.threshold, 0), "开端卡不吃强化");
                continue;
            }
            let (bp, bt) = base_power(Faction::Frost, c.def.name);
            seen += 1;
            assert_eq!(c.def.power, bp + 1, "{}：第4章＝三档 ⇒ 数值+1", c.def.name);
            let exp_thr = if is_threshold_trait(c.def.tr) { (bt - 1).max(1) } else { bt };
            assert_eq!(c.def.threshold, exp_thr, "{}：阈值档只该动带阈值特性的卡", c.def.name);
            assert_eq!(c.skills.len(), 2, "{}：第4章＝三档 ⇒ 技能强化（构造1 + 追加1）", c.def.name);
            assert!(
                c.skills[0] == c.skills[1],
                "{}：技能档是给已有技能叠层，不是换一个新技能",
                c.def.name
            );
            assert_eq!(c.hp, c.def.power, "强化后血量须跟着定义重置满格");
        }
        // 每阵营 13 张定义（1 开端 + 12 普通）；敌方 4 在手、9 在堆。
        assert_eq!((seen, b.enemy_hand.len() + b.enemy_pile.len(), b.enemy_hand.len(), b.enemy_pile.len()), (12, 13, 4, 9));
        // 日志必须报**实际效果**而不是"+3 档"这种含糊说法（阈值只有 5 张命中）。
        assert!(
            b.log.iter().any(|l| l.contains("数值+1 共12｜阈值-1 共5｜技能+1层 共12")),
            "强化日志与牌面不符：{:?}",
            b.log.last()
        );
    }

    /// 同一关内「强化后 - 强化前」的逐牌差值，按卡名归档。
    fn deltas(level: u32) -> Vec<(&'static str, i32, i32, i32)> {
        let plain = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), level);
        let mut s = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), level);
        s.apply_chapter_strengthening(level);
        let mut v: Vec<(&'static str, i32, i32, i32)> = plain
            .enemy_hand
            .iter()
            .chain(plain.enemy_pile.iter())
            .zip(s.enemy_hand.iter().chain(s.enemy_pile.iter()))
            .filter(|(p, _)| !p.is_starter())
            .map(|(p, s)| (p.def.name, s.def.power - p.def.power, s.def.threshold - p.def.threshold, s.skills.len() as i32 - p.skills.len() as i32))
            .collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v
    }

    /// 该卡名的阈值档真实差值：带阈值特性才 -1，阈值已是 1 时不动。
    fn thr_delta(f: Faction, name: &str) -> i32 {
        let d = faction_cards(f).iter().find(|d| d.name == name).unwrap();
        if is_threshold_trait(d.tr) { (d.threshold - 1).max(1) - d.threshold } else { 0 }
    }

    #[test]
    fn chapter_five_plateaus_at_chapter_four() {
        let c4 = deltas(37);
        let c5 = deltas(49);
        assert_eq!(c4.len(), 12);
        let gated: Vec<&str> = c4.iter().filter(|(_, _, dt, _)| *dt != 0).map(|(n, ..)| *n).collect();
        assert!(
            c4.iter().all(|(n, dp, dt, ds)| (*dp, *ds) == (1, 1) && *dt == thr_delta(Faction::Frost, n)),
            "第4章＝三档：每张 +1 数值、+1 技能层，阈值仅带特性的卡 -1，实际差值 {c4:?}"
        );
        assert_eq!(gated.len(), 5, "每阵营 12 张里恰有 5 张带阈值特性，其余 7 张阈值档必须空转");
        assert_eq!(c4, c5, "第4章起按 §十一:404 封顶：第5章的逐牌差值不得再涨");
    }

    /// 幂等：主线循环里"建好即强化"和调用点再补一次，不该叠成 2 倍强化。
    #[test]
    fn applying_twice_equals_applying_once() {
        for level in [13u32, 25, 37] {
            let once = Battle::new_mainline(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), level);
            let mut twice = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Normal, Vec::new(), level);
            twice.apply_chapter_strengthening(level);
            twice.apply_chapter_strengthening(level);
            assert_eq!(enemy_snap(&twice), enemy_snap(&once), "第{level}关二次调用不得再改牌面");
            assert_eq!(twice.log.len(), once.log.len(), "第二次调用不该再叠一条强化日志");
        }
    }

    /// 构造即强化（`new_mainline`）与"构造后手动强化"必须是同一件事，且与不强化有区别。
    #[test]
    fn new_mainline_wraps_new_plus_strengthening_and_chapter_one_stays_raw() {
        let manual = {
            let mut b = Battle::new(9, Faction::Ember, Faction::Shadow, Difficulty::Hard, Vec::new(), 25);
            b.apply_chapter_strengthening(25);
            b
        };
        let wrapped = Battle::new_mainline(9, Faction::Ember, Faction::Shadow, Difficulty::Hard, Vec::new(), 25);
        assert_eq!(enemy_snap(&wrapped), enemy_snap(&manual));
        let raw = Battle::new(9, Faction::Ember, Faction::Shadow, Difficulty::Hard, Vec::new(), 5);
        let wrapped1 = Battle::new_mainline(9, Faction::Ember, Faction::Shadow, Difficulty::Hard, Vec::new(), 5);
        assert_eq!(enemy_snap(&wrapped1), enemy_snap(&raw), "第1章 new_mainline 必须等价于 new");
        let ramped = Battle::new_mainline(9, Faction::Ember, Faction::Shadow, Difficulty::Hard, Vec::new(), 25);
        assert_ne!(enemy_snap(&ramped), enemy_snap(&raw), "反向对照：非第1章确实变了（防空实现假绿）");
    }

    #[test]
    fn strengthening_is_orthogonal_to_difficulty() {
        let mut easy = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Easy, Vec::new(), 37);
        easy.apply_chapter_strengthening(37);
        let mut expert = Battle::new(7, Faction::Ember, Faction::Frost, Difficulty::Expert, Vec::new(), 37);
        expert.apply_chapter_strengthening(37);
        assert_eq!(
            enemy_snap(&easy),
            enemy_snap(&expert),
            "难度只管决策质量（§廿），章强化只管数值——两者不得互相渗透"
        );
    }

    #[test]
    fn boss_levels_stay_out_of_the_chapter_ramp() {
        for id in BossId::all() {
            let level = id.chapter() * crate::boss::LEVELS_PER_CHAPTER;
            let boss = Battle::new_boss(2026, Faction::Ember, id, Vec::new(), level);
            let plain = Battle::new(2026, Faction::Ember, id.profile().faction, Difficulty::Normal, Vec::new(), level);
            assert_eq!(
                enemy_snap(&boss),
                enemy_snap(&plain),
                "{}（第{}章末）的牌面必须是未强化的基准——B1 的贪心全败读数要逐帧可比",
                id.name(),
                id.chapter()
            );
        }
    }
}
