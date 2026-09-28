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

pub const CANDLE_HP: i32 = 20;
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

fn refund_pct(deaths_before: u32) -> i32 {
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

    pub p_karma: i32,
    pub e_karma: i32,
    pub p_candle: i32,
    pub e_candle: i32,

    pub p_front: [Option<CardInst>; 4],
    pub e_back: [Option<CardInst>; 4],
    pub e_front: [Option<CardInst>; 4],

    pub hand: Vec<CardInst>,
    pub draw_pile: Vec<CardInst>,
    pub starter_pile: u32,
    pub discard_pile: Vec<CardInst>,
    pub enemy_hand: Vec<CardInst>,
    pub enemy_pile: Vec<CardInst>,
    pub e_discard: Vec<CardInst>,

    pub pf: SideFlags,
    pub ef: SideFlags,

    pub karma_penalty_next: i32,
    /// §廿二:955 伤害回滚每局最多触发2次（我方每局独立计数，不跨关累积）。
    pub rollback_left: i32,
    pub dealt_this_turn: i32,
    pub in_player_attack_phase: bool,
    /// 守夜人式"本回合友方累积+N"增益窗口：(阵营, 生效回合, 增量)。
    pub boosts: Vec<(SideK, i64, i32)>,
    pub attack_order: Vec<u64>,
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
        let mk = |id: &mut u64, rng: &mut Rng, def: CardDef, skill: bool| -> CardInst {
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
            draw_pile = inherit;
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

        let mut hand = vec![p_starter];
        for _ in 0..3 {
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
            p_karma: 0,
            e_karma: 0,
            p_candle: CANDLE_HP,
            e_candle: CANDLE_HP,
            p_front: Default::default(),
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
        b.player_turn_start();
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

    /// §廿一「每章新阵营」＝三阵营循环 + **强化**（裁定24 人授权"数值先写，后续有问题再改"）。
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

    /// §廿二:948 手牌上限 8 张，超出弃置**最早进入手牌**的牌（`hand` 尾插 ⇒ 堆头即最早）；
    /// §廿二:949 弃的牌进弃牌堆、本关不再使用（自造牌例外：任何离场永久消失，§十:372）。
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

    pub fn player_turn_start(&mut self) {
        self.turn += 1;
        if self.karma_penalty_next > 0 {
            let before = self.p_karma;
            self.p_karma = (self.p_karma - self.karma_penalty_next).max(0);
            if before != self.p_karma {
                self.log.push(format!("回滚代价：本回合业力-{}", before - self.p_karma));
            }
        }
        self.karma_penalty_next = 0;
        if !self.draw_pile.is_empty() {
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
        self.pf.manual_draws = 2;
        self.pf.starter_draws = 1;
        self.pf.sacrifice_used = false;
        self.pf.sacrificed_names.clear();
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

    /// 场上献祭（P 格 0..3）：每回合1次、在场≥1回合、全额不递减、禁同名牌回置。
    /// §廿二:964 的"阵营限制"（§四:160 不能献祭敌方阵营的卡）由 API 形状保证：只有我方 `p_front`
    /// 与 `hand` 可寻址，敌方卡传不进来，故无需运行期检查。
    pub fn player_sacrifice_field(&mut self, col: usize) -> Result<(), String> {
        if self.pf.sacrifice_used {
            return Err("本回合献祭次数已用完（每回合最多1次）".into());
        }
        let c = self.p_front[col].take().ok_or("该格没有卡牌")?;
        if self.turn - c.placed_turn < 1 {
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
        let is_starter = self.hand[idx].is_starter();
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
        let cost = if card.is_starter() { 0 } else { card.def.cost };
        match side {
            SideK::Player => self.p_karma -= cost,
            SideK::Enemy => self.e_karma -= cost,
        }
        let mut c = card;
        c.seq = self.seq;
        self.seq += 1;
        c.placed_turn = self.turn;
        let slot: &mut Option<CardInst> = match (side, row) {
            (SideK::Player, _) => &mut self.p_front[col],
            (SideK::Enemy, Row::Front) => &mut self.e_front[col],
            (SideK::Enemy, Row::Back) => &mut self.e_back[col],
        };
        let old = slot.take();
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
        if let Some(old) = old {
            self.log.push(format!("挤压：{} 越线死亡", short_card(&old)));
            self.on_death(old, side, Some(col), DeathCause::Cross);
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
                Skill::PlaySameColFlame1 => self.add_flame_col(side, col, 1, None),
                Skill::PlayAdjColFlame1 => {
                    for ac in adj_cols(col) {
                        self.add_flame_col(side, ac, 1, None);
                    }
                }
                _ => {}
            }
        }
        self.check_all_triggers();
    }

    pub fn player_place(&mut self, hand_idx: usize, col: usize) -> Result<(), String> {
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
        if self.p_karma < cost {
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
        let squeeze = self.boss_rule() == crate::boss::BossRule::ShadowPush && row == Row::Front;
        if occupied && !squeeze {
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
            // §廿二:958 满 30 回合未分胜负 → 比蜡烛长度，长者胜、相等平局。
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

    /// §廿二:962 攻击顺序＝按卡牌入场顺序（`seq` 单调递增，见 `place_side`），不按列位。
    fn row_seq_order(&self, side: SideK, row: Row) -> Vec<u64> {
        let mut v: Vec<u64> = (0..4)
            .filter_map(|c| self.slot(side, row, c).as_ref().filter(|x| x.hp > 0).map(|x| x.seq))
            .collect();
        v.sort();
        v
    }

    fn col_of_seq(&self, side: SideK, row: Row, sq: u64) -> Option<usize> {
        (0..4).find(|i| self.slot(side, row, *i).as_ref().is_some_and(|c| c.seq == sq))
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
            let base = atk.hp;
            let id = atk.id;
            let target = self.pick_target(SideK::Player, col, tr);
            self.log.push(format!("⚔ 我方 {} 攻击", short_card(&atk)));
            let dmg = match target {
                Some(dcol) => self.card_hit_damage(SideK::Player, id, col, dcol, base, atk.is_starter()),
                None => self.holder_hit_damage(SideK::Player, id, col, base),
            };
            if let Some(dcol) = target {
                let eb = self.boost_for(SideK::Enemy);
                if let Some(def) = self.e_front[dcol].as_mut() {
                    def.hp -= dmg;
                    def.flame += dmg + eb;
                }
                self.dealt_this_turn += dmg;
                self.log.push(format!("  → 敌第{}列受{dmg}", dcol + 1));
            } else {
                self.dealt_this_turn += dmg;
                self.damage_enemy_holder(dmg, Some(col), HolderHit::Direct);
            }
            self.attacker_aftermath(&mut atk, target.unwrap_or(col), SideK::Player);
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
            if atk_dead {
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
            crate::ai::run(self);
        }
        self.enemy_resolve_turn_end();
    }

    /// 敌方回合尾段（攻击→统一结算→推进→开端回合末）。AI 搜索的叶子推进与实战共用同一实现。
    /// 路径内不消耗 rng（除抽牌外的 rng 消费点为零），因此克隆推进是 rng 中性的。
    pub fn enemy_resolve_turn_end(&mut self) {
        self.enemy_attack_phase();
        self.enemy_settle();
        self.enemy_advance();
        self.starter_turn_end(SideK::Enemy);
    }

    fn enemy_attack_phase(&mut self) {
        let order = self.row_seq_order(SideK::Enemy, Row::Front);
        let mut card_d: Vec<(usize, i32)> = Vec::new();
        let mut candle_d: i32 = 0;
        for sq in order {
            let col = match self.col_of_seq(SideK::Enemy, Row::Front, sq) {
                Some(c) => c,
                None => continue,
            };
            let mut atk = self.e_front[col].take().unwrap();
            let tr = atk.def.tr;
            let base = atk.hp;
            let id = atk.id;
            let target = self.pick_target(SideK::Enemy, col, tr);
            self.log.push(format!("⚔ 敌方 {} 攻击", short_card(&atk)));
            match target {
                Some(dcol) => {
                    // 伤害累积（攻击时点即时计算攻击+受伤修正，不立即扣血/业火）
                    let dmg = self.card_hit_damage(SideK::Enemy, id, col, dcol, base, atk.is_starter());
                    card_d.push((dcol, dmg));
                }
                None => {
                    let dmg = self.holder_hit_damage(SideK::Enemy, id, col, base);
                    candle_d += dmg;
                }
            }
            self.attacker_aftermath(&mut atk, target.unwrap_or(col), SideK::Enemy);
            let atk_dead = atk.hp <= 0;
            self.e_front[col] = Some(atk);
            if atk_dead {
                let a = self.e_front[col].take().unwrap();
                self.on_death(a, SideK::Enemy, Some(col), DeathCause::Battle);
            }
            self.check_all_triggers();
        }
        self.pending_card_d = card_d;
        self.pending_candle_d = candle_d;
    }

    /// 我方回合末的敌方伤害统一结算。四条形径都钉在这一个函数里，改动会同时破坏多条边界条款：
    /// §廿二:954 回滚消耗来源 A＝本回合攻击阶段已打出的伤害总和（`dealt_this_turn`）；
    /// §廿二:956 代价＝A 全额抵掉这一发 D，另记下回合业力-1（`karma_penalty_next`）；
    /// §廿二:957 顺序＝先回滚判定 → 后蜡烛减短 → 后业火增加（下面严格按此三段排列）；
    /// §廿二:953 超额部分按我方攻击顺序分配，优先攻击敌方卡牌、无卡则攻击敌方持业者（`distribute_excess`）；
    /// §廿二:943 我方烛尽的同一结算里敌方烛也已尽 → 平局（双方同时致命不判一方胜）。
    fn enemy_settle(&mut self) {
        let d = std::mem::take(&mut self.pending_candle_d);
        if d > 0 && self.over.is_none() {
            if d >= self.p_candle && self.rollback_left > 0 {
                let a = self.dealt_this_turn;
                let remain = (d - a).max(0);
                let excess = (a - d).max(0);
                self.rollback_left -= 1;
                self.karma_penalty_next = 1;
                self.p_candle -= remain;
                self.log.push(format!(
                    "[红光亮起·伤害回滚 余{}] D={d} A={a} → 剩余{remain}削烛（我方蜡烛剩 {}）",
                    self.rollback_left, self.p_candle
                ));
                if excess > 0 {
                    self.distribute_excess(excess);
                }
            } else {
                self.p_candle -= d;
                self.log.push(format!("我方蜡烛 -{d}（剩 {}）", self.p_candle));
            }
            if self.p_candle <= 0 && self.over.is_none() {
                self.log.push("我方烛尽…".into());
                self.over = Some(if self.enemy_candle_ref() <= 0 { Outcome::Draw } else { Outcome::PlayerLose });
            }
        }
        // 统一结算累积卡牌伤害：先数值降低，后业火增加
        let card_d = std::mem::take(&mut self.pending_card_d);
        for (col, dmg) in &card_d {
            if let Some(def) = self.p_front[*col].as_mut() {
                def.hp -= dmg;
                self.log.push(format!("  结算：{} 受{dmg} → hp {}", def.def.name, def.hp));
            }
        }
        let pb = self.boost_for(SideK::Player);
        for (col, dmg) in &card_d {
            if let Some(def) = self.p_front[*col].as_mut() {
                def.flame += dmg + pb;
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
        self.check_all_triggers();
    }

    /// 超额伤害：按我方攻击顺序轮转，每名存活攻击者至多分配其当前数值点。
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
                if self.e_front[col].is_some() {
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
                    self.damage_enemy_holder(give, Some(col), HolderHit::Excess(give));
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
            self.e_candle -= dmg;
            match how {
                HolderHit::Direct => {
                    self.log.push(format!("  → 直击中线，敌方蜡烛 -{dmg}（剩 {}）", self.e_candle));
                    if self.e_candle <= 0 {
                        self.log.push("敌方烛尽！".into());
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

    /// §廿二:963 推进＝单卡、按列独立：只有前排空的列才把该列后排顶上来，每列每次至多1张。
    fn enemy_advance(&mut self) {
        for col in 0..4 {
            if self.e_front[col].is_none() {
                if let Some(c) = self.e_back[col].take() {
                    self.log.push(format!("敌方推进：第{}列后排 {} → 前排", col + 1, c.def.name));
                    self.e_front[col] = Some(c);
                }
            }
        }
    }

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
        if flags.starter_gains >= 2 {
            return;
        }
        flags.starter_gains += 1;
        match side {
            SideK::Player => {
                self.p_karma += 1;
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
    /// §廿二:961 亡语在本函数内即刻结算，调用方的 `check_all_triggers` 在其后 ⇒ 亡语先于特性。
    /// §廿二:960 业火跨回合保留（没有任何按回合清空的路径），只在死亡这一处清零。
    pub fn on_death(&mut self, mut c: CardInst, side: SideK, col: Option<usize>, cause: DeathCause) {
        let devour = cause == DeathCause::Sacrifice
            && side == SideK::Player
            && self.boss_rule() == crate::boss::BossRule::DevourName;
        let gain = match cause {
            DeathCause::Sacrifice => {
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
                    let pct = refund_pct(c.deaths);
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
        if let Some(col) = col {
            if c.def.tr == TraitKind::DeathRattleSameColFlame3 {
                self.add_flame_col(side, col, 3, None);
            }
            for s in c.skills.clone() {
                if s == Skill::DeathSameColFlame1 {
                    self.add_flame_col(side, col, 1, None);
                }
            }
        }
        c.flame = 0;
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

    pub(crate) fn pick_target(&self, side: SideK, col: usize, tr: TraitKind) -> Option<usize> {
        let def_front_empty = |c: usize| match side {
            SideK::Player => self.e_front[c].is_none(),
            SideK::Enemy => self.p_front[c].is_none(),
        };
        if !def_front_empty(col) {
            return Some(col);
        }
        if tr == TraitKind::AttackAdjacent {
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
        dmg += Self::skill_count(&self.col_cards(side, acol).into_iter().filter(|a| a.id != atk_id).collect::<Vec<_>>(), Skill::AllyColAtk1);
        let foes = self.col_cards(side.other(), acol);
        let mut debuff = 0;
        for f in &foes {
            if f.hp > 0 && f.def.tr == TraitKind::EnemyColAttackMinus1 {
                debuff += 1;
            }
        }
        debuff += Self::skill_count(&foes, Skill::EnemyColAtkM1);
        (dmg - debuff).max(0)
    }

    pub(crate) fn card_hit_damage(&self, side: SideK, atk_id: u64, acol: usize, dcol: usize, base: i32, atk_is_starter: bool) -> i32 {
        let mut d = self.attack_power(side, atk_id, acol, base);
        let allies_of_def = self.col_cards(side.other(), dcol);
        let mut reduce = 0;
        for f in &allies_of_def {
            if f.hp > 0 && f.def.tr == TraitKind::AllyColDamageTakenMinus1 {
                reduce += 1;
            }
        }
        reduce += Self::skill_count(&allies_of_def, Skill::AllyColDmgTakenM1);
        let buffs_of_atk = self.col_cards(side, dcol);
        d -= reduce;
        d += Self::skill_count(&buffs_of_atk, Skill::EnemyColDmgTakenP1); // 含持有者自身（§八"同列敌方受伤+1"）
        // Boss 特殊规则（霜封）；业火按减免后伤害计（调用方用本函数返回值加业火）
        crate::boss::card_damage_adjust(self, side, atk_is_starter, d.max(0))
    }

    pub(crate) fn holder_hit_damage(&self, side: SideK, atk_id: u64, acol: usize, base: i32) -> i32 {
        self.attack_power(side, atk_id, acol, base)
    }

    /// 攻击后追加结算。`col` = 目标列（直击中线时回退为攻击者列）——§八"攻击后对同列+1"锚定目标列。
    fn attacker_aftermath(&mut self, atk: &mut CardInst, col: usize, side: SideK) {
        let bo = self.boost_for(side);
        match atk.def.tr {
            TraitKind::SelfFlameOnAttack1 => atk.flame += 1 + bo,
            TraitKind::SelfDmgOnAttack => {
                atk.hp -= 1; // 自损不触发业火
                if atk.hp <= 0 {
                    self.log.push(format!("  {} 自损而亡", atk.def.name));
                }
            }
            _ => {}
        }
        let skills = atk.skills.clone();
        for s in skills {
            match s {
                Skill::AtkSelfFlame1 => atk.flame += 1 + bo,
                Skill::AtkSameColFlame1 => self.add_flame_col(side, col, 1, None),
                Skill::AtkAdjColFlame1 => {
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
        thr -= Self::skill_count(&allies, Skill::AllyColThreshM1);
        thr += Self::skill_count(&self.col_cards(side.other(), col), Skill::EnemyColThreshP1);
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
            if snap.hp <= 0 || snap.triggered_turn == turn || !is_threshold_trait(snap.def.tr) {
                continue;
            }
            let thr = self.effective_threshold(side, col, &snap);
            if snap.flame < thr {
                continue;
            }
            {
                let c = self.slot_mut(side, row, col).as_mut().unwrap();
                c.flame -= thr;
                c.triggered_turn = turn;
            }
            let tr = snap.def.tr;
            let id = snap.id;
            let bo = self.boost_for(side);
            self.log.push(format!("🔥 {} 业火爆发 → {}", snap.def.name, tr.label()));
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
                        let counts = s == SideK::Enemy && self.in_player_attack_phase;
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
                            self.dealt_this_turn += dealt; // §十二：攻击阶段内造成的伤害计入 A
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

#[cfg(test)]
mod rule_tests {
    use super::*;

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
