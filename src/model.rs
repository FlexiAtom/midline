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
//! 核心类型：卡牌三段模型（定义 / 实例 / 随机技能）、特性与技能枚举、三阵营卡牌表。
//!
//! ## 规则歧义解释清单（文档未穷尽处，本实现的裁定；对局验证后可再议）
//! 1. 开端"死亡获2业力"是特性定额：不随死亡返还递减（递减只作用于"返还=费用"路径）；
//!    主动献祭同样拿 2（全额=定额）。
//! 2. "可攻击相邻列"：同列中线对面无卡时，改打相邻列敌方前排卡（先左后右），
//!    仍无卡才攻击中线（持业者）。
//! 3. 阈值技"对同列+N累积"：作用于攻击者所在列的所有卡（敌我皆算，不含自身）。
//!    "同列友方"仅己方。"相邻列"＝攻击者列的左右两列全部卡（敌我）。
//! 4. "对整列造成3伤"＝对同列所有卡各造成3点伤害（敌我皆算，不含自身；不触及持业者）。
//! 5. "本回合友方累积+2"＝触发回合内的**增益窗口**：该侧每一次"业火获得事件"额外 +2
//!    （攻击后自增、阈值技分发、受击转焰、超额分配皆算），不是触发瞬间的一次性发放。
//! 6. 超额伤害分配＝按我方攻击顺序轮转：每名存活攻击者至多分配其当前数值点伤害，
//!    目标为其同列敌前排（越线序优先），无卡则击敌方持业者，直至超额耗尽。
//! 7. 离场去向（文档未写死去处，由 §三"火苗A第二次死亡→50%"与 §十"自造牌阵亡永久消失"反推闭合）：
//!    基础牌任何离场（自然死亡/越线/献祭/溢出弃置）→ 弃牌堆，本关不再使用，下关并入继承堆，
//!    实例死亡计数随卡跨关保留（这是递减梯队唯一可达路径）；献祭不使死亡计数+1（返还互斥递减）；
//!    开端一切离场不入继承堆（每关固定发放）；自造牌（融合产物 crafted）任何离场永久消失。
//! 8. 业火触发检查点：业火值每次增加后立即检查（受"每回合每卡最多1次"约束）。
//! 9. 跨关"上一关剩余"＝场上未阵亡 + 手牌 + 牌堆未抽 + 弃牌堆（本关已离场的基础牌）；
//!    开局继承堆不足3张时按文档从基础牌堆补齐（同名卡因此可多实例并存，佐证裁定7的实例独立计数）。
//! 10. 手牌献祭的次数豁免**只适用开端**（§四第4条写在"开局献祭开端"块下，人裁定 2026-09-28）：
//!     普通手牌献祭仍占每回合1次额度；"手牌献祭不受在场限制"对一切手牌卡适用。
//! 11. 继承堆顺序有意义（§十"可调整继承堆顺序"）：我方抽牌一律取堆顶；准备阶段 `move` 调序。
//! 12. 技能叠加（§九"融合后技能可叠加"）：被动光环类技能按**出现次数**计层（同名两个＝+2），
//!     特性不叠加（一张卡只有一个特性，计 1 层）。
//! 13. "同列敌方受伤+1"（技能8）**不排除持有者自身**——持有者自己攻击该列敌人时伤害同样 +1；
//!     而"同列友方攻击+1 / 友方阈值-1"类友方增益排除持有者自身（自己不给自己的加成，避免自增强循环）。
//!     后果（待人向设计确认）：我方只有前排4格，"同列友方"类技能（5/7/11）对我方结构性空转，
//!     敌方因前后排同列才可生效。
//! 14. §二十 AI 搜索的**「步」＝敌方 1 个原子动作 ply**（放置 / 场上献祭 / 手牌献祭 / 不动作），
//!     不是 §十二 的一整回合四阶段（另一读法）；难度档 → 深度：简单·普通＝0（贪心），困难＝2，专家＝3，
//!     每落地 1 个 ply 后以 depth-1 **重规划**。同条一并自定的口径（文档未给算法，待人向设计确认）：
//!     「列威胁度」＝该列玩家前排存活卡的生命值；放置得分取文档字面 `卡牌价值 + 本列威胁度×2`
//!     （＝优先去压力大的列守），取代早期按 0.25 折减的猜测式实现。
//! 15. 献祭得分（§廿:871）＝获得业力×1.5 + 亡语价值×2 − 失去卡牌价值：文档把亡语**同时**算进
//!     "失去卡牌价值"里的特性与独立的"亡语×2"，属重复计价；本实现维持字面不修正（要改须由设计定删哪一项）。
//! 16. AI 献祭的**区域＝手牌 + 场上**：5 条献祭条件按语义分派到场两侧（文档未写"只其其一"）——
//!     手牌侧只可能命中条件 1（开局业力0有开端）、2（解锁高费卡）；场上侧命中 2/3/4/5
//!     （3 需行感知：只有前排能被直接攻击；4 需"祭后该列还能落子"；5 用 `场上数值总和 < 手牌可放置最高价值`）。
//!     场上献祭一律要求"得分>0 或命中任一条件"才入枚举；其合法性与额度（每回合1次、在场≥1回合、同名禁回置）
//!     与玩家侧走同一套规则函数，AI 不自开例外。
//!     难度差异：简单/普通另沿用决策树门控「有可负担的牌就不祭」（放不出才祭），困难/专家在合法范围内全枚举、由搜索定夺。
//! 17. 推进得分（§廿:872 `挤压敌方次数×3 − 自身被挤风险×2`）对敌方 AI 的对应物＝**后排落子在回合末经
//!     `enemy_advance` 抬到前排拦截**：故搜索叶子统一推进到回合末后再评估，不另计奖励项；
//!     "挤压敌方"以烛差体现（`（玩家烛−敌方烛）×3`，烧对方烛即挤压），"自身被挤风险"以
//!     无前排拦截的列的威胁度计入负项。
//! 18. §二十"专家档含融合决策"与§十一"融合只发生在准备/结算阶段"的接合：专家 AI 的融合决策**落在战斗外**
//!     （准备·结算阶段），战斗阶段任何一侧都不可融合（人裁定 2026-09-28）。实现上 `ai::plan_meta` 只产出**意图**
//!     （`MetaMove`），回灌 `meta::fuse_cards` / `meta::upgrade_card` 执行，AI 不自第二套结算规则。
//! 19. §二十一"炎与冰 双Boss"文档没有任何棋盘与判定定义（缺口 s21-未列），本实现裁定：**共用同一张 4×2 棋盘**
//!     （不为双 Boss 扩格），中线按列分两段——第1-2列归 2 号「冰」、第3-4列归 1 号「炎」；直击该段即削对应
//!     持业者，同时另一根同步承受 ⌊削减/2⌋（炎冰同源）；**两根皆尽才算玩家胜**；30 回合判定取两根中的
//!     **较长值**（不取和——取和会让"熬到回合终"形同必败）；单根尽时不结束，日志显式标注剩哪一根。
//! 20. Boss 经济（§三"业力不自动恢复"与"Boss 脚本必须有预算"的接合）：脚本卡**直放**（不经敌方手牌/双牌堆、
//!     不吃 rng、技能由脚本显式声明），预算只给一次 `start_karma`（＝intro 花费＋周期最大下凹，实测反推），
//!     **没有**"每回合发钱"——5 份脚本按 FIFO 传送带写成周期自我闭合，30 回合零跳过（见
//!     `boss::boss_tests::boss_ledger_is_self_funding_over_30_turns`）。
//!     本条原有两笔债，人已裁（2026-09-28「你的修改提议即可，可以开始修改」，授权范围由实现侧读定并逐字记入 voice 台账）：
//!     - **②（已落）** 改 §三:64 使"初始业力"按侧区分。改前原文逐字存档：`初始业力 0`；
//!       改后：`初始业力 我方 0；敌方按遭遇定义（普通关 0，Boss 关有开场脚本预算）`。
//!       这是**设计文档正文**的改动，就地替换单行 ⇒ 全文仍 1034 行、其余锚点零位移（`anchor_tests` 实测通过）。
//!       效果＝`start_karma` 从"与 §三 相抵的自造豁免"升格为**文档授权值**。
//!       历史如实保留：改前那行确实写着 0，`start_karma != 0` **当时就是**自造豁免，不是"没违反 §三"；
//!       动文档的理由不变——在场≥1回合 + 每回合1次献祭 两条闸使 0 起手无法在第 2 周期前供养任何脚本，
//!       要么给起手预算，要么整层脚本不成立。
//!       未采的两个方向登记备查：① 承认 Boss 例外并给数（不动文档，但 §三 仍与实现相抵）；
//!       ③ 脚本降级为"0 起手也只放得起 1-2 费"的弱脚本（等于放弃 §廿一 的战术设计）。
//!     - **④（已实验，量出与裁定23 互斥 ⇒ 待人给数，未合入）** 脚本直放每次都是新实例 ⇒ `deaths` 恒 0
//!       ⇒ 影长「暗渡」的越线死亡**恒按 100% 返还**，玩家侧的 100/50/25/10 递减梯队对 Boss 不可达。
//!       处置＝脚本卡改走敌方手牌/牌堆/弃牌堆的**现成实例**（取不到才新造），同一张名卡复用同一实例。
//!       实验在分支 `exp/ruling20-4-script-instances`（commit ed8274a），实测读数（sandbox 30 回合）：
//!       炉主/雪爵/炎与冰/终影 **零跳过、零额外业力需求**；**影长 8 条「业力不足」＋15 条「已在场」**，
//!       消掉业力不足需 `start_karma` 10 → **28**；若放宽"同名单第二张"（在场时另造新实例），
//!       已在场归零但业力不足仍 8 条且需 10 → **37**（同时在场更多 ⇒ 收入衰减更深）。
//!       ⇒ ④ 与 裁定23「五条特殊规则的语义与全部数值我确认没有问题」互斥：梯队一可达，影长 的预算就不闭合。
//!       改 `start_karma` 或改 影长 脚本周期都属**数值改动**，授权在人侧 ⇒ 本头寸不动，等裁。
//!       注意这不是"实验没做出来"：四种组合都做了，两个方向（严：不摆第二张／宽：另造）都收敛到同一个结论。
//! 21. §二十 难度表把"Boss"与 简单/普通/困难/专家 并列，同一行又写"Boss＝特殊规则"——本实现读作
//!     **遭遇轴**而非难度档：`Battle::boss: Option<BossId>` 是真值，Boss 战的敌方回合由脚本接管（贪心/搜索 AI
//!     不参与），难度档照常可选（决定我方托管与评分口径）。因此不存在"难度＝Boss 但无 Boss"的退化态，
//!     设计件要求的"`Difficulty::Boss` 且 `boss==None` → 退化 `run_normal`"无需实现。
//!     如实补一句边界：Boss 战里 `--difficulty` **实际不改变任何行为**——敌方是脚本，我方交互局是人类，
//!     `auto --boss` 的托管走贪心 `auto_turn` 不读难度档。USAGE 已按此写明，不再暗示它能调 Boss 强度。
//! 22. Boss 战**平局＝未通关**：不解锁下一章、不记 `boss_down`（否则可用平局白嫖章节奖励）。本实现自造条款，
//!     文档 §22"30回合未分胜负→蜡烛长者胜"未区分 Boss 战。
//! 23. **五条 Boss 特殊规则与其全部数值，都是实现自造，文档没有真值可核对**：§廿一 Boss 表只有
//!     「章节 Boss 特征」三列（md:931-936），"特殊规则"全文只出现两次——md:907「Boss 脚本Combo + 特殊规则」
//!     与 md:922（每日挑战规则「固定卡组+特殊规则」），两处都**一字未定义**。
//!     本实现按 特征 名字反推规则，逐条**故意覆盖**下列文档硬规则（待人向设计逐条确认或换掉）：
//!     - 炉温：敌方回合开始全体在场卡业火+1 —— 与 md:564「业火只由攻击造成的伤害触发」相反（非伤害来源发焰）。
//!     - 霜封：我方卡对敌前排伤害-1（开端不削）。口径：减免只作用于**攻击落子那一发**；§十五 超额分配
//!       是同一发已减免伤害的再路由，故不二次减免（另一读法＝每一发命中都减，未采）。
//!     - 暗渡：脚本主动把后排压上已占用前排 —— 覆盖的是 md:757「AI不会主动将E5-E8挤越线（避免自杀）」
//!       这条 **AI 自律**，不覆盖 §十七 挤压机制本身（机制在 md:756「敌方挤压：后排挤前排，越线死亡」里本来就有）。
//!     - 炎冰同源：见裁定19（双烛/列分区/⌊伤害/2⌋ 反弹/皆尽判胜/回合终取较长值，全为自造）。
//!     - 吞名：我方主动献祭改按**当前**死亡返还档位计 —— 与 md:99、速查 md:985「主动献祭 → 获得全额费用，
//!       不触发死亡返还递减」相反。不推进档位：推进会把单场惩罚顺着裁定7 的跨关台账变成永久惩罚。
//!     - 终影 持业者 30：与 md:596「敌方持业者…初始长度20单位」相反；后果是 30 回合判对我方结构性不利（须净多打 >10）。
//!       平衡现状（实测，非结论）：贪心托管对 5 份脚本 `auto 5 --boss all` 全败（4-11 回合），
//!       按台账「手感调试先挂起」只登记不擅改。
//!       **人裁定 2026-09-28：「五条特殊规则的语义与全部数值我确认没有问题，后续有问题再」** ⇒ 本条由"待人逐条确认"
//!       转为**已认可的实施口径**（pending B2 关闭）。"后续有问题再改"＝改动授权保留在人侧，AI 仍不擅自调数。
//! 24. §廿一 主线"每章新阵营"＝三阵营循环 + 强化（人裁定 2026-09-28）。**循环与强化都已落**（原缺口：`level`
//!     只进 rng 种子与头报，无任何按章递增的敌方数值）。
//!     人裁定原话：「数值先写，后续有问题再改」⇒ 数值由实现侧先给口径，改动权仍在人侧。
//!     强化量 `chapter_strength(level) = min(章号-1, 3)`，三档**累积**，逐档复用 §十一:403 的三种升级效果：
//!     +1 档＝数值+1；+2 档＝**带阈值特性的卡**再阈值-1（下限1）；+3 档＝再给该卡已有技能叠 1 层。
//!     封顶 3 复用 §十一:404「每张卡牌最多升级3次」。⇒ 第1/2/3/4/5章＝0/1/2/3/3（第4章起平台）。
//!     落在 `battle.rs::apply_chapter_strengthening`；②③两档的口径由 **裁定25** 更正（原文写的是"阈值-1"与"追加1个技能"）。
//!     四条自设边界（都是"不擅动别的东西"，不是新规则）：
//!     - **开端卡不吃强化**：持业者是蜡烛本体，动它会串到烛尽判定（§十四 闸）。
//!     - **Boss 关整体豁免**：由调用方 `one_level` 决定不施加强化，保 B1「贪心对 5 份脚本全败」读数逐帧可比。
//!     - **不掷 rng**：三档只改既有牌面（不新增卡、不换卡、不索引技能池，见 裁定25②）⇒ 同 seed 逐帧复现不破；
//!       第1章强化量为 0 时整函数空操作 ⇒ 首关与改动前逐字节相同（`chapter_one_is_byte_identical_no_op` 守）。
//!     - **与难度正交**：§廿 难度只改决策质量（`ai.rs search_depth`），章强化只改数值，互不渗透
//!       （`strengthening_is_orthogonal_to_difficulty` 守）。
//!
//!     实测读数（`cargo test -- --nocapture chapter_ramp`，seed=4242，我方＝贪心托管、空继承堆单关，
//!     每格"未强化→强化后"的败北回合）。**这批是旧取样**：敌方取 `encounter_for`，而章首关全落霜阵营，
//!     五章其实是同一种敌人 ⇒ 已被 裁定25 的取样修正作废，数值只留作过程痕迹，不再当读数引用。
//!     第2章 10→8 / 7→6 / 9→**6**，第3章 11→5，第4章 9→6 / 9→7 / 9→6，
//!     第5章 7→6。方向正确：强化越高我方死得越早，且**不空转**（同 seed 回合数确实变动）。
//!     **但这批读数不能当平衡结论**：第1章未强化的基准就已经 30 格全败（6-8 回合），⇒ 贪心托管＋空继承堆
//!     对任何普通关都不是有效玩家。这条同时反证了 `boss-followups` B1 的猜测：Boss 全败**不是 Boss 专属强度**，
//!     而是基准策略太弱的共病；B1 要的是"更强的托管/多关继承"读数法，不是先削 Boss 数值。
//! 25. 裁定24 的**两处实现更正 + 一处取样法更正**（人授权原话 2026-09-29：「好，我许准自行决断，继续推进」）。
//!     起因＝对本方已提交代码的三条指控逐条实测，全部为真；这不是手感调整，是"实现与自己写下的声明不符"。
//!     - **① 阈值档空转**：旧实现无条件 `threshold -= 1`，但 `threshold` 的唯一行为读取点
//!       （`battle.rs::try_trigger_col`）前面有 `is_threshold_trait` 闸 ⇒ 每阵营 12 张里 7 张改的是永不被读的字段。
//!       现加同一道闸：只有带阈值特性的卡才 -1（实测 5/12 张命中，`chapter_five_plateaus_at_chapter_four` 锁死这个数）。
//!     - **② 技能档换身份而非强化**：旧实现 `pool[c.id % 12]`，而 `id` 是 `next_id` 顺发的战斗内序号
//!       ⇒ 同一张牌换个关会拿到**不同技能**；原注释却写成"与玩家侧 `upgrade_card` 同一索引法"（那是 `upgrades % 12`，
//!       与卡片身份无关）。现按 §十一:403「技能强化」的中文语义 + md:354/md:357「同名技能叠加」读法，
//!       改为**给该卡已有技能再叠一层**（`skill_count` 按出现次数计层，叠层即刻生效且不改身份）。
//!     - **③ 幂等**：`upgrades` 记为已施加档数，重复调用不再叠加（§十一:404 的计数器在敌方侧也真正生效）。
//!       同时新增 `Battle::new_mainline()`＝构造即强化，消掉"调用点记得再补一刀"这个可漏步骤。
//!     - **取样法更正**（这条改的是**读数**不是规则）：章首关 `[1,13,25,37,49]` 全部 `level % 3 == 1`
//!       ⇒ 旧矩阵五章打的都是同一个霜阵营，而阵营差本身就有 0.00%–5.21% 的胜负幅度，比强化效应还大。
//!       矩阵补上阵营轴（5 章 × 3 阵营 × 3 难度 × 对照＝90 场）。新读数（seed=4242，贪心托管·空继承堆·单关）：
//!       基线 45 格里 **3 胜**（全在幽影议会，第3章普通/专家 20 回合、第5章普通 17 回合），强化臂 **0 胜**；
//!       33/45 格回合数变化，**1 格反向**（第5章幽影困难 6→7 回合：敌方评分吃自身数值，加强会改变它的选择）。
//!       仍是"测法与对照"，不是平衡结论——基准臂本身仍 42/45 全败（B1 未解）。
//! 26. 文档自身的序冲突 + 锚点机检的反向缺口（同一授权下的自决）。
//!     - **序冲突采 §廿二 读法**：§十三:547「业火≥阈值 → 触发特性」排在 §十三:548「数值≤0 → 死亡」之前，
//!       按字面顺序读会得出"致命一击仍先触发特性"；§廿二:945 明写"死亡后业火值达阈值 不触发特性，业火值清零"。
//!       边界表是对流程表的例外说明，故采 945。实现本来就是这个行为（`hp <= 0` 闸 + `on_death` 清零），
//!       缺的只是把这条例外**写进锚点**，否则下一个人顺着 §十三 的序号"修正"它就会改掉规则。
//!     - **反向覆盖机检**：正向机检只问"已有锚点指对了吗"，问不出"整条规则没落地"——945 正是这样漏掉的
//!       （有实现、零锚点，正向一路绿灯）。新增 `every_edge_case_row_of_section22_is_anchored_back_from_src`：
//!       §廿二 边界表每一行都必须被 src/ 的锚点指回。必检集**从文档结构推导**（表头之后到 `---` 的连续非空行），
//!       不是手挑清单；唯一人工入口是排除表，当前两条：md:970「商业模式 挂Github」＝发行备注不是规则、
//!       md:971「激励系统 收集/成就/每日奖励」＝规则层范围外（存档包只存进度，金币成就一个字节不存）。
//!       本轮补齐 §廿二:943..969 共 26 行的锚点。叙述不算锚点：`//!` 裁定登记区与机检自身的文本都被排除在计数外。
//!     - **假绿反证（实测两次，都留了痕迹）**：① 第一版机检把自己注释里的 "md:945" 也算成锚点 ⇒
//!       手动删掉 `try_trigger_col` 的真锚点后测试**照样绿**；加排除逻辑后重做同一注入 ⇒ 红，报出
//!       `md:945 ← 卡牌死亡后业火值达阈值…`，撤销注入 ⇒ 绿。② 阈值门控用"12 张里恰好 5 张命中"反向锁死，
//!       防止将来把闸去掉时只看到"更多卡被强化"这种看着合理的假象。
//!     - **残留盲区（明写，不假装已解决）**：反向覆盖已纳入 §廿二 与 §十八 两张表（见第 30 条）；
//!       §廿三 速查表、§十二 的编号步骤、以及 §一/§二/§五/§六/§七/§八/§十四/§十五/§十六 的散文行
//!       仍未纳入，"整条规则没落地"在那些章仍查不出；语义是否被曲解始终不可机检，仍靠人向设计逐条核对。
//! 27. 存档点＝**关隘入口**（裁定25/26 同一授权下的自决，人原话 2026-09-29：「推，我授权推进」）。
//!     文档对"进度"零规定（全文 grep「存档」＝0 命中），只给了体量 §廿一:914「主线 60关，5章」与一条
//!     硬要求 §廿一:926「每日挑战完成后记录日期，防止重复完成」。取"只在关隘入口写"的理由：
//!     要打中途态就得把 §廿二 全部中途规则（场上/手牌/业力/烛/序列）再序列化一遍，而"能续"只需要
//!     回到**没过关的那一关门口**——于是 败/平/弃 三种退路天然等价于"不动盘"，写盘点也只有两个：
//!     本关入口、通关后的下一关入口。第60关不写第61关：`is_mainline_end` 是 `>=`，写了就是伪造进度
//!     （下一次续档赢一关就再报一次通关）。闸写在 `persist_next_entry` 里而不是调用点，
//!     是为了让"记得判一下"这个会漏的步骤没有地方可漏（`next_entry_advances_chapter_ends_and_stops_at_sixty`）。
//!     - **声明与实现不符一处（本批实测坐实并修）**：`SaveUse::DailyDoneOnly` 的注释承诺"关卡与继承堆
//!       一个字节都不动"，实际 `daily_run` 用 `resume=false` 开槽，冷启动分支把内存档换成 `Progress::new()`，
//!       而 `mark_daily_done` 是**整文件重写** ⇒ 打赢一次每日就把主线抹回第1关，且全程不响一声。
//!       真机双臂（同一入口档：第5关·堆2张·结转业力3；`/tmp` 副本里把"每日第一关打赢"换成直接调
//!       `mark_daily_done`，其余路径全走生产代码）：修前落盘 `level=1 carry_karma=0` + 牌堆两行消失，
//!       修后除 `daily_done` 外逐字节不变。回归测试 `daily_done_mark_preserves_an_existing_mainline_progress`
//!       先跑红（`left: 1, right: 5`）再跑绿；旧的 `daily_slot_never_records_progress` 是**假绿**——
//!       它只测 `snapshot_entry` 那一侧的门禁，管不到记每日这条写路径。
//!     - 每日与主线共档的代价说清楚：`daily` 读同一份文件来判断"今天打过没有"，所以它必须能读主线档；
//!       反过来它一个字都不该改主线，这条不变量现在由上面那条测试守着。
//!     - **已知代价（本批实测，不改行为、待人给口径）**：冷启动（不给 `--resume`）撞上既有进度时，
//!       警告之后**第一次入口快照就把旧档整份重写**，而原位**不留证**。真机（档面＝第7关·烬火·堆3张·业力3）：
//!       `mainline --seed 2 --save <档>` 接 EOF ⇒ 先打「⚠ 档上已有主线进度…第一次快照就覆盖它」，
//!       落盘变成 `level=1` + 牌堆三行消失 + 业力0，且目录里**没有** `.bad-N`。这与 裁定28 对坏档
//!       "改名留证、绝不删"的立场**不对称**：坏档有物证，手滑少打一个 `--resume` 反而没有。
//!       三个备选与拒绝理由：① 覆盖前也走 `backup_and_clear` —— 与"留证"一致，但玩家每次**故意**重开
//!       都会在数据目录堆一份 `.old-N`，把 裁定27 允许的常规操作变成垃圾制造；② 撞上既有进度就拒
//!       （rc=2，须显式 `--force-restart`）—— 最安全，但直接推翻 USAGE 已写死的"不给 --resume 则从第1关重开"，
//!       且管道/脚本里少一个确认位就打断自动化；③ 交互确认（按 y）—— 非交互时 EOF 即拒，能挡住本批这种
//!       手滑，但给"警告 + 覆盖"这条已获授权的路径加了第三个概念。AI 的倾向是 ③（本批这条手滑正是它
//!       要防的形态），但 ③ 是**新交互 + USAGE 变更**，不在已授权的存档语义范围内 ⇒ 本批只登记不实现，
//!       要哪一条由人给。
//!       附带教训（同一条实测里踩到的）：验证"败/平/弃不动盘"**必须用一份还没被覆盖过的档**——
//!       第一版双臂先跑了冷启动、后拿被重置成 `level=1 空堆` 的档去比 md5，于是"不动盘"成了一条
//!       自我循环的空断言（它只证明"空档还是空档"）。改用 第7关·堆3张 的档重做：`--resume` + `q`
//!       前后 `cmp` 逐字节相同，这条结论才算真。
//! 28. 坏档一律 **`exit 2`，绝不静默当空档**（同一授权下的自决）。理由是可定位的失真而非洁癖：
//!     `Battle::new` 有"空继承堆 ⇒ 发基础牌序"的兜底分支，把读不懂的档当空档，玩家以为在续第N关、
//!     实际拿到第1关牌序——那是丢进度之后还错打一整局。校验因此全是**拒载**而不是修正
//!     （重复键/缺必填/未知 schema/level 越界/power·threshold<1 或超基线/upgrades>3/含开端/
//!     第1关带堆/堆>10 ⇒ `Err`）；未知**键**忽略是前向兼容的最小口径（新字段不该让旧程序判整份档为坏）。
//!     - 本批把"坏档＝非零"这条**承诺**真正铺到所有出口：`progress` 以前打印 ✖ 却返回 0（脚本读到的
//!       永远是"一切正常"，那句 ✖ 成了给眼睛看的装饰）⇒ `describe` 多返回一个 bool，`progress_cmd` 据此
//!       `exit 2`；`--seed`/`--faction` 以前读不懂就回退成 种子7·烬火 ⇒ 改报错（真机：`--seed abc play`
//!       与 `--faction 9 play` 都 rc=2 并原样回显收到的值）。实测 rc=2 的入口共五条：
//!       `daily`/`mainline --resume` 撞坏档、跨阵营续档、`progress` 读坏档、未知长选项、`--save` 缺值。
//!     - **不做的一条：载入时不校验继承堆内 id 唯一性**。审查建议加这道闸，实测不可加：
//!       `Battle::new` 每关从 `id = 1` 重新发号（`battle.rs:185`），继承堆里的旧牌保留上关的号，
//!       本作新发的号必然撞上 ⇒ 引擎自己写出的合法档就有重号（实测：
//!       `engine_reissues_id_one_so_load_must_not_require_unique_ids`——带 id 1、2 两张牌进第2关，
//!       补进来的第三张必然也叫 1）。加闸的结果是拒载真档，比它要防的问题更糟。
//!       重号真正的代价在战斗内（`battle.rs:1363` 的同列自排除按 id 比 ⇒ 我方一张牌会让同号敌牌免伤），
//!       那是发号方案的问题，登记在 `pool/cross-side-id-collision.md`，不在存档侧修。
//! 29. 「每日挑战**完成**」的边界：文档只说 §廿一:926「完成后记录日期，防止重复完成」，
//!     一字未定义"完成"，也没给 md:922 那条「固定卡组+特殊规则」的内容 ⇒ 取最小可核读法：
//!     **本局第一次打赢一关**就算完成，只记 `daily_done`（日种子，不是日期字符串——种子＝`epoch秒/86400`，
//!     日界是 UTC 零点而非本地零点，这一条是既有权衡不是新坑）。诚实缺口照抄在两处注释里：
//!     现在的 `daily` 只是"换日期种子的 play"，文档意义上的每日挑战还差固定卡组与特殊规则两项未定义。
//! 30. 反向覆盖从「§廿二 一张表」扩到 **§十八 卡牌总表**（同一授权下的自决，人原话 2026-09-29：「建议落入全局坑，
//!     然后接着推进」）。入池前按 `cn2int`/排除口径重算的读数（**旧读数 14 章零锚点／48 锚点行／112 次引用全部作废**，
//!     偏差来自章号映射缺 `廿→20` 一支 + 把 `//!` 叙述与测试断言里的行号也算了进去）：文档 1034 行／23 章，
//!     机检可见锚点指向 **53 个不同文档行／75 次引用**，**10 章零锚点合计 349 正文行**（§一 12、§二 27、§六 23、
//!     §七 29、§八 15、§十二 86、§十四 18、§十五 51、§十六 42、§十八 46）。四个只读代理逐行核对的结论：
//!     §十八 39 卡 **漏 0／多 0／数值不符 0**；§十二 65 已实现／3 部分／2 未实现／30 非规则行；§十五 16/2/2；
//!     §十六 11/1/8；§八 技能池 12↔12 一一对应。⇒ 规则层几乎全落地，缺口集中在**呈现层**（render.rs 只有"焰N/阈值"
//!     纯数字，无百分比、无色档、无震屏，全仓无音频）与两处真校验缺失（§二:47-48「后排先放」只在 `ai.rs` 候选生成
//!     生效、引擎与 Boss 不查；§一:17 成就+每日奖励零实现）。**D3 已落**：这 **17** 条真未实现的文档行现在以
//!     `NOT_IMPLEMENTED` 债表的形式住在下面的锚点机检里（呈现层 14／规则层 2／豁免改登记 1，逐条带原文、证据与去处，
//!     偿一条就当场红）。提案原文写"18 行／呈现层 15 行"，与它自己逐行列举的 14 差一，本表以文档实测为准。
//!     计划、读数与 6 条待人裁都在
//!     `~/.Athena/projects/midline/working/reverse-coverage-multi-chapter.md`，本处只登记口径。
//!     - 本章**不能只查锚点存在**：三行开端（774/791/808）逐字节相同、代码里是同一个 `STARTER`，只查锚点会让
//!       1 个定义满足 3 行必检＝假绿。故新增 `every_card_row_of_section18_is_anchored_back_and_matches_field_by_field`：
//!       卡行按**形态**推导（行首三个空白分隔 token 全为 ASCII 数字 ⇒ 卡行；列头、阵营小节标题、`---` 自然排除，
//!       不像 §廿二 那样硬写 `"情况 处理"` 字面量——那条只跳一次，文档一加第四阵营就静默漏），再与 `faction_cards`
//!       逐字段（费/数值/阈值/卡名）比死，并断言三行开端逐字节相同、小节顺序与 `Faction` 顺序一致。
//!     - 假绿反证实测两次：改 `影仆` 阈值 3→4 ⇒ 红 `md:809 文档[…阈3] ≠ 代码[…阈4]`；摘掉 `// md:809` ⇒ 红
//!       `md:809 ← 1 2 3 影仆 无 从技能池随机 没有任何锚点指回`；复原 ⇒ 106 全绿（基线 104 ＋ 本测 ＋ 献祭越界回归测）。
//!     - **仍未纳入**：§廿三 速查表、§十二 的编号步骤（411-509 是 ``` 围栏内的 `1.` 与 `   a.` 两级步骤，要第三档
//!       推导器）、以及九张散文章。语义是否被曲解始终不可机检，仍靠人向设计逐条核对。

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Faction {
    Ember,  // 烬火教团
    Frost,  // 霜誓守卫
    Shadow, // 幽影议会
}

impl Faction {
    pub fn name(self) -> &'static str {
        match self {
            Faction::Ember => "烬火教团",
            Faction::Frost => "霜誓守卫",
            Faction::Shadow => "幽影议会",
        }
    }
}

/// 固定特性（不参与融合）。开端为聚合特性（含免费/定额2业力/在场每回合+1业力·每关上限2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraitKind {
    None,
    Starter,
    AttackAdjacent,
    SelfDmgOnAttack,
    AllyColDamageTakenMinus1,
    EnemyColAttackMinus1,
    ThresholdSameColFlame2,
    ThresholdAllyColFlame2,
    DeathRattleSameColFlame3,
    BattleCrySameColFlame2,
    ThresholdAdjColFlame3,
    SelfFlameOnAttack1,
    ThresholdFullColDamage3,
    ThresholdAllyTurnFlame2,
}

/// 随机附加技能（12 种，可融合、可叠加）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skill {
    AtkSelfFlame1,       // 1 攻击后自身+1累积
    AtkSameColFlame1,    // 2 攻击后对同列+1累积
    PlaySameColFlame1,   // 3 放置时对同列+1累积
    DeathSameColFlame1,  // 4 死亡时对同列+1累积
    AllyColAtk1,         // 5 同列友方攻击+1
    EnemyColAtkM1,       // 6 同列敌方攻击-1
    AllyColDmgTakenM1,   // 7 同列友方受伤-1
    EnemyColDmgTakenP1,  // 8 同列敌方受伤+1
    AtkAdjColFlame1,     // 9 攻击后相邻列+1累积
    PlayAdjColFlame1,    // 10 放置时相邻列+1累积
    AllyColThreshM1,     // 11 同列友方阈值-1
    EnemyColThreshP1,    // 12 同列敌方阈值+1
}

impl Skill {
    pub fn list() -> [Skill; 12] {
        [
            Skill::AtkSelfFlame1,
            Skill::AtkSameColFlame1,
            Skill::PlaySameColFlame1,
            Skill::DeathSameColFlame1,
            Skill::AllyColAtk1,
            Skill::EnemyColAtkM1,
            Skill::AllyColDmgTakenM1,
            Skill::EnemyColDmgTakenP1,
            Skill::AtkAdjColFlame1,
            Skill::PlayAdjColFlame1,
            Skill::AllyColThreshM1,
            Skill::EnemyColThreshP1,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Skill::AtkSelfFlame1 => "攻+己焰1",
            Skill::AtkSameColFlame1 => "攻+同焰1",
            Skill::PlaySameColFlame1 => "放+同焰1",
            Skill::DeathSameColFlame1 => "亡+同焰1",
            Skill::AllyColAtk1 => "同友攻+1",
            Skill::EnemyColAtkM1 => "同敌攻-1",
            Skill::AllyColDmgTakenM1 => "同友伤-1",
            Skill::EnemyColDmgTakenP1 => "同敌伤+1",
            Skill::AtkAdjColFlame1 => "攻+邻焰1",
            Skill::PlayAdjColFlame1 => "放+邻焰1",
            Skill::AllyColThreshM1 => "同友阈-1",
            Skill::EnemyColThreshP1 => "同敌阈+1",
        }
    }
}

impl TraitKind {
    pub fn label(self) -> &'static str {
        match self {
            TraitKind::None => "无",
            TraitKind::Starter => "开端",
            TraitKind::AttackAdjacent => "可攻击相邻列",
            TraitKind::SelfDmgOnAttack => "攻击后自身-1",
            TraitKind::AllyColDamageTakenMinus1 => "同列友方受伤-1",
            TraitKind::EnemyColAttackMinus1 => "同列敌方攻击-1",
            TraitKind::ThresholdSameColFlame2 => "阈值:同列+2焰",
            TraitKind::ThresholdAllyColFlame2 => "阈值:同友+2焰",
            TraitKind::DeathRattleSameColFlame3 => "亡语:同列+3焰",
            TraitKind::BattleCrySameColFlame2 => "战吼:同列+2焰",
            TraitKind::ThresholdAdjColFlame3 => "阈值:邻列+3焰",
            TraitKind::SelfFlameOnAttack1 => "攻后自身+1焰",
            TraitKind::ThresholdFullColDamage3 => "阈值:整列3伤",
            TraitKind::ThresholdAllyTurnFlame2 => "阈值:本回合友+2焰",
        }
    }

    /// 评分用特性价值（§十九:832 量化表，逐条数值见 835-842）。
    pub fn ai_value(self) -> f64 {
        match self {
            TraitKind::None | TraitKind::Starter => 0.0,
            TraitKind::AttackAdjacent => 0.5,
            TraitKind::EnemyColAttackMinus1 => 1.0,
            TraitKind::AllyColDamageTakenMinus1 => 1.5,
            TraitKind::ThresholdSameColFlame2 | TraitKind::ThresholdAllyColFlame2 => 1.0,
            TraitKind::DeathRattleSameColFlame3 => 1.5,
            TraitKind::BattleCrySameColFlame2 => 0.5,
            TraitKind::SelfDmgOnAttack => -0.5,
            _ => 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CardDef {
    pub name: &'static str,
    #[allow(dead_code)] // 数据完整性：卡表按阵营归档，读取方在渲染层（后置）
    pub faction: Faction,
    pub cost: i32,
    pub power: i32,
    pub threshold: i32,
    pub tr: TraitKind,
}

pub const STARTER: CardDef = CardDef {
    name: "开端",
    faction: Faction::Ember,
    cost: 0,
    power: 1,
    threshold: 4,
    tr: TraitKind::Starter,
};

/// 卡牌表（§十八:769）。索引 0 为开端（各阵营共用同一开端定义，三行开端逐字节相同）。
pub fn faction_cards(f: Faction) -> &'static [CardDef] {
    use Faction::*;
    match f {
        Ember => &[
            STARTER, // md:774
            CardDef { name: "火苗", faction: Ember, cost: 1, power: 2, threshold: 4, tr: TraitKind::None }, // md:775
            CardDef { name: "引燃者", faction: Ember, cost: 2, power: 3, threshold: 5, tr: TraitKind::AttackAdjacent }, // md:776
            CardDef { name: "焚稿人", faction: Ember, cost: 3, power: 4, threshold: 6, tr: TraitKind::ThresholdSameColFlame2 }, // md:777
            CardDef { name: "余温", faction: Ember, cost: 2, power: 2, threshold: 4, tr: TraitKind::SelfDmgOnAttack }, // md:778
            CardDef { name: "续炭", faction: Ember, cost: 2, power: 1, threshold: 3, tr: TraitKind::ThresholdAllyColFlame2 }, // md:779
            CardDef { name: "炉壁", faction: Ember, cost: 3, power: 3, threshold: 8, tr: TraitKind::AllyColDamageTakenMinus1 }, // md:780
            CardDef { name: "火星", faction: Ember, cost: 2, power: 3, threshold: 3, tr: TraitKind::BattleCrySameColFlame2 }, // md:781
            CardDef { name: "雷烬", faction: Ember, cost: 4, power: 4, threshold: 7, tr: TraitKind::ThresholdAdjColFlame3 }, // md:782
            CardDef { name: "燎原", faction: Ember, cost: 3, power: 3, threshold: 5, tr: TraitKind::SelfFlameOnAttack1 }, // md:783
            CardDef { name: "山火", faction: Ember, cost: 5, power: 6, threshold: 10, tr: TraitKind::ThresholdFullColDamage3 }, // md:784
            CardDef { name: "守夜人", faction: Ember, cost: 3, power: 2, threshold: 6, tr: TraitKind::ThresholdAllyTurnFlame2 }, // md:785
            CardDef { name: "回燃", faction: Ember, cost: 4, power: 3, threshold: 7, tr: TraitKind::DeathRattleSameColFlame3 }, // md:786
        ],
        Frost => &[
            STARTER, // md:791
            CardDef { name: "初霜", faction: Frost, cost: 1, power: 2, threshold: 4, tr: TraitKind::None }, // md:792
            CardDef { name: "望哨", faction: Frost, cost: 2, power: 3, threshold: 5, tr: TraitKind::AttackAdjacent }, // md:793
            CardDef { name: "霜序", faction: Frost, cost: 3, power: 4, threshold: 6, tr: TraitKind::ThresholdSameColFlame2 }, // md:794
            CardDef { name: "寒哨", faction: Frost, cost: 2, power: 3, threshold: 4, tr: TraitKind::EnemyColAttackMinus1 }, // md:795
            CardDef { name: "暖誓", faction: Frost, cost: 2, power: 1, threshold: 3, tr: TraitKind::ThresholdAllyColFlame2 }, // md:796
            CardDef { name: "壁", faction: Frost, cost: 3, power: 5, threshold: 8, tr: TraitKind::AllyColDamageTakenMinus1 }, // md:797
            CardDef { name: "冰刺", faction: Frost, cost: 2, power: 3, threshold: 3, tr: TraitKind::BattleCrySameColFlame2 }, // md:798
            CardDef { name: "极光", faction: Frost, cost: 4, power: 4, threshold: 7, tr: TraitKind::ThresholdAdjColFlame3 }, // md:799
            CardDef { name: "雪线", faction: Frost, cost: 3, power: 3, threshold: 5, tr: TraitKind::SelfFlameOnAttack1 }, // md:800
            CardDef { name: "冰川", faction: Frost, cost: 5, power: 8, threshold: 10, tr: TraitKind::ThresholdFullColDamage3 }, // md:801
            CardDef { name: "冻时", faction: Frost, cost: 3, power: 2, threshold: 6, tr: TraitKind::ThresholdAllyTurnFlame2 }, // md:802
            CardDef { name: "霜葬", faction: Frost, cost: 4, power: 4, threshold: 7, tr: TraitKind::DeathRattleSameColFlame3 }, // md:803
        ],
        Shadow => &[
            STARTER, // md:808
            CardDef { name: "影仆", faction: Shadow, cost: 1, power: 2, threshold: 3, tr: TraitKind::None }, // md:809
            CardDef { name: "暗哨", faction: Shadow, cost: 2, power: 2, threshold: 5, tr: TraitKind::AttackAdjacent }, // md:810
            CardDef { name: "蚀", faction: Shadow, cost: 3, power: 3, threshold: 6, tr: TraitKind::ThresholdSameColFlame2 }, // md:811
            CardDef { name: "低语", faction: Shadow, cost: 2, power: 2, threshold: 4, tr: TraitKind::EnemyColAttackMinus1 }, // md:812
            CardDef { name: "余光", faction: Shadow, cost: 2, power: 1, threshold: 3, tr: TraitKind::ThresholdAllyColFlame2 }, // md:813
            CardDef { name: "渊壁", faction: Shadow, cost: 3, power: 4, threshold: 8, tr: TraitKind::AllyColDamageTakenMinus1 }, // md:814
            CardDef { name: "无痕", faction: Shadow, cost: 2, power: 3, threshold: 3, tr: TraitKind::BattleCrySameColFlame2 }, // md:815
            CardDef { name: "幽雷", faction: Shadow, cost: 4, power: 3, threshold: 7, tr: TraitKind::ThresholdAdjColFlame3 }, // md:816
            CardDef { name: "夜行", faction: Shadow, cost: 3, power: 3, threshold: 5, tr: TraitKind::SelfFlameOnAttack1 }, // md:817
            CardDef { name: "深壑", faction: Shadow, cost: 5, power: 6, threshold: 10, tr: TraitKind::ThresholdFullColDamage3 }, // md:818
            CardDef { name: "止时", faction: Shadow, cost: 3, power: 2, threshold: 6, tr: TraitKind::ThresholdAllyTurnFlame2 }, // md:819
            CardDef { name: "引渡", faction: Shadow, cost: 4, power: 3, threshold: 7, tr: TraitKind::DeathRattleSameColFlame3 }, // md:820
        ],
    }
}

/// 按名查卡（Boss 脚本解析卡名的唯一入口；开端用 `STARTER` 名查任意阵营表均可命中）。
pub fn card_by_name(f: Faction, name: &str) -> Option<CardDef> {
    faction_cards(f).iter().find(|c| c.name == name).copied()
}

pub type CardId = u64;

/// 场上/手中/继承堆里的卡牌实例。业火、死亡计数、升级数都挂在实例上。
#[derive(Clone, Debug)]
pub struct CardInst {
    pub id: CardId,
    pub def: CardDef,
    pub skills: Vec<Skill>,
    /// 当前数值（= 血量）。每关开始重置满格。
    pub hp: i32,
    pub flame: i32,
    /// 自然死亡次数（用于返还递减：100/50/25/10%）。献祭不走此计数。
    pub deaths: u32,
    pub upgrades: u8,
    /// 入场序（攻击顺序）；不在场时为 0。
    pub seq: u64,
    /// 入场回合（献祭在场时长判定）；不在场时为 i64::MIN。
    pub placed_turn: i64,
    /// 本回合是否已触发过特性（每卡每回合上限 1 次）。
    pub triggered_turn: i64,
    /// 融合产物标记（自造牌）：任何离场永久消失（§十"自造牌阵亡后永久消失"）。
    pub crafted: bool,
}

impl CardInst {
    pub fn new(id: CardId, def: CardDef) -> Self {
        CardInst {
            id,
            def,
            skills: Vec::new(),
            hp: def.power,
            flame: 0,
            deaths: 0,
            upgrades: 0,
            seq: 0,
            placed_turn: i64::MIN,
            triggered_turn: i64::MIN,
            crafted: false,
        }
    }

    pub fn is_starter(&self) -> bool {
        self.def.tr == TraitKind::Starter
    }

    /// 阈值存于实例内嵌定义（升级直接改定义值），此处不再叠加计数。
    pub fn base_threshold(&self) -> i32 {
        self.def.threshold.max(1)
    }
}

/// 卡牌一行简述（渲染用）。
pub fn short_card(c: &CardInst) -> String {
    let mut s = format!("{}{}({}费 值{} 阈{})[{}]", c.def.name, if c.crafted { "〈造〉" } else { "" }, c.def.cost, c.hp, c.base_threshold(), c.flame);
    if !c.skills.is_empty() {
        s.push_str(&format!("{{{}}}", c.skills.iter().map(|k| k.label()).collect::<Vec<_>>().join("+")));
    }
    s
}

impl fmt::Display for CardInst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", short_card(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_tables_have_13_each() {
        for faction in [Faction::Ember, Faction::Frost, Faction::Shadow] {
            assert_eq!(faction_cards(faction).len(), 13, "{faction:?}");
        }
    }

    #[test]
    fn skill_pool_has_12() {
        assert_eq!(Skill::list().len(), 12);
    }
}

/// 文档锚点机检。此前一轮全量核对发现裁定清单里有 8 处引用指错行（含一处指向**全文不存在的行号**、
/// 三处指向空行、两处指到别的章节），故立此不变量：注释里的每一处文档引用必须
/// ① 行号不越界 ② 该行非空 ③ 凡带章名的锚点，该行确实属于所声称的那一章。
/// 规则文档不在仓库内（与本仓同级 `中线.MD`），文件缺失时跳过而非失败。
/// 语义是否被曲解无法机检——那仍靠人向设计逐条核对。
#[cfg(test)]
mod anchor_tests {
    use super::{Faction, faction_cards};

    const NUM: &[char] = &['一', '二', '三', '四', '五', '六', '七', '八', '九', '十', '廿'];

    fn cn2int(s: &str) -> Option<u32> {
        let digit = |c: char| match c {
            '一' => Some(1),
            '二' => Some(2),
            '三' => Some(3),
            '四' => Some(4),
            '五' => Some(5),
            '六' => Some(6),
            '七' => Some(7),
            '八' => Some(8),
            '九' => Some(9),
            _ => None,
        };
        let cs: Vec<char> = s.chars().collect();
        if cs.is_empty() || !cs.iter().all(|c| NUM.contains(c)) {
            return None;
        }
        if cs[0] == '廿' {
            return Some(20 + cs.get(1).and_then(|&c| digit(c)).unwrap_or(0));
        }
        if cs.len() == 1 && cs[0] == '十' {
            return Some(10);
        }
        if cs[0] == '十' {
            return Some(10 + cs.get(1).and_then(|&c| digit(c))?);
        }
        if cs.get(1) == Some(&'十') {
            let tens = digit(cs[0])?;
            return Some(tens * 10 + cs.get(2).and_then(|&c| digit(c)).unwrap_or(0));
        }
        digit(cs[0])
    }

    fn digits_at(cs: &[char], from: usize) -> Option<(u32, usize)> {
        let mut j = from;
        while j < cs.len() && cs[j].is_ascii_digit() {
            j += 1;
        }
        if j == from { None } else { Some((cs[from..j].iter().collect::<String>().parse().ok()?, j)) }
    }

    /// 文档不随仓库分发。候选：环境变量 → 仓库同级 → 上两级（本仓在 ~/rust/midline，文档在 ~/）。
    fn locate_doc() -> Option<std::path::PathBuf> {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        std::env::var("MIDLINE_DOC")
            .map(std::path::PathBuf::from)
            .ok()
            .into_iter()
            .chain([
                manifest.join("中线.MD"),
                manifest.parent().unwrap_or(manifest).join("中线.MD"),
                manifest.ancestors().nth(2).unwrap_or(manifest).join("中线.MD"),
            ])
            .find(|p| p.exists())
    }

    /// 找不到文档时的统一处置：只有 `MIDLINE_DOC_SKIP=1` 才允许跳过（默认不跳过，防静默假绿）。
    /// 返回 `None` ＝本测试已被显式豁免，调用方直接 `return`。
    fn doc_or_skip() -> Option<Vec<String>> {
        if locate_doc().is_none() && std::env::var("MIDLINE_DOC_SKIP").is_ok() {
            return None;
        }
        let doc = locate_doc().unwrap_or_else(|| {
            panic!(
                "找不到规则文档 中线.MD（候选含 $MIDLINE_DOC 与仓库同级/上两级）。它不在仓库里；\
                 设 MIDLINE_DOC=<路径> 指定，或 MIDLINE_DOC_SKIP=1 明确跳过本检查。"
            )
        });
        Some(std::fs::read_to_string(&doc).unwrap().lines().map(str::to_string).collect())
    }

    /// 只扫**被编译的**文件：模块清单取 `main.rs` 里的 `mod X;`。没被 `mod` 的 `src/*.rs` 是死文件，
    /// 一个字都不读——让一份不参与构建的副本替整条规则作证，是覆盖类机检最舒服的假绿形态。
    /// 代价说清楚：正向检查因此也不再管死文件里的锚点写没写错（它反正不参与构建，写错了也不影响任何输出）。
    /// 实测双臂：把 `md:943` 的唯一活锚点删掉、原样搬进 `src/zz-dead.rs` ⇒ 反向检查 RED（搬不动真值）；
    /// 同一份伪造行号（9999）放进死文件 ⇒ 正向 GREEN，放进 `rng.rs` ⇒ 正向 RED。
    fn src_rs_files() -> Vec<std::path::PathBuf> {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let main = std::fs::read_to_string(manifest.join("src/main.rs")).unwrap();
        let mut modules: Vec<String> = Vec::new();
        for l in main.lines() {
            let t = l.trim().trim_start_matches("pub ").trim_start_matches("pub(crate) ").trim();
            let Some(rest) = t.strip_prefix("mod ") else { continue };
            let Some(name) = rest.strip_suffix(';') else { continue };
            modules.push(name.trim().to_string());
        }
        assert!(modules.len() >= 8, "main.rs 里只抓到 {} 条 `mod X;` ⇒ 判据失效，本检查等于没在筛", modules.len());
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(manifest.join("src"))
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .filter(|p| {
                let stem = p.file_stem().unwrap().to_string_lossy().to_string();
                stem == "main" || modules.contains(&stem)
            })
            .collect();
        files.sort();
        files
    }

    /// 许可证头机检。人原话（09-28）：「代码头要加AGPL-v3头，这个需要检查，如果没有的话补，署名用FlexiAtom」。
    /// 判据＝文件开头那段**连续的行注释**（`//` 起头、排除 `//!` 文档注释）必须与 `main.rs` 的那段逐字相同，
    /// 且其中必须含 AGPL 与署名两句话。
    /// 为什么不逐文件写死一份期望：期望文本抄两遍，改年份时会出现"改了九个忘改模板"的第十四种错法；
    /// 拿 `main.rs` 当参照物，新增文件漏头当场红，改头时九个文件一起动才不会漏。
    #[test]
    fn every_compiled_src_file_opens_with_the_same_agpl_header() {
        fn block(src: &str) -> Vec<String> {
            src.lines().take_while(|l| l.starts_with("//") && !l.starts_with("//!")).map(str::to_string).collect()
        }
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let main = std::fs::read_to_string(manifest.join("src/main.rs")).unwrap();
        let want = block(&main);
        assert!(want.len() >= 10, "参照头只有 {} 行，判据失效", want.len());
        let joined = want.join("\n");
        assert!(joined.contains("GNU Affero General Public License"), "参照头不含 AGPL 条款，本检查在拿错的模板上自证");
        assert!(joined.contains("Copyright (C) 2026 FlexiAtom"), "参照头不含约定署名（人原话：署名用FlexiAtom）");

        let mut bad: Vec<String> = Vec::new();
        for fp in src_rs_files() {
            let name = fp.file_name().unwrap().to_string_lossy().into_owned();
            let got = block(&std::fs::read_to_string(&fp).unwrap());
            if got != want {
                let at = got.iter().zip(want.iter()).position(|(a, b)| a != b).unwrap_or(got.len().min(want.len()));
                bad.push(format!("  {name}：头第 {} 行起与参照不同（该文件头 {} 行 / 参照 {} 行）", at + 1, got.len(), want.len()));
            }
        }
        assert!(bad.is_empty(), "许可证头不一致：\n{}", bad.join("\n"));
    }

    /// 章头表与"某文档行属第几章"的判据，正向锚点机检与未实现债表**共用这一份**。
    /// 编号必须严格递增，否则文中任何「一、二」式散文都会被误认成章节头。
    /// 为什么摘出来：两处各写一遍章号映射，一旦 `cn2int` 少一支（`廿→20` 就是这么缺过一次的），
    /// 两边会**朝不同方向**错，而"债条归属第几章"这种断言恰恰要靠两边同错才算成立。
    fn doc_sections(lines: &[&str]) -> impl Fn(u32) -> Option<u32> {
        let mut heads: Vec<(u32, u32)> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            let Some((num, _)) = l.trim().split_once('、') else { continue };
            let Some(v) = cn2int(num) else { continue };
            if heads.last().is_none_or(|&(last, _)| v > last) {
                heads.push((v, (i + 1) as u32));
            }
        }
        move |line: u32| heads.iter().filter(|(_, at)| *at <= line).map(|(v, _)| *v).last()
    }

    #[test]
    fn every_doc_anchor_lands_on_a_nonempty_line_of_its_claimed_section() {
        let Some(lines_s) = doc_or_skip() else { return };
        let lines: Vec<&str> = lines_s.iter().map(String::as_str).collect();
        let section_at = doc_sections(&lines);

        let mut bad: Vec<String> = Vec::new();
        for fp in src_rs_files() {
            let name = fp.file_name().unwrap().to_string_lossy().into_owned();
            let src = std::fs::read_to_string(&fp).unwrap();
            for (idx, line) in src.lines().enumerate() {
                let cs: Vec<char> = line.chars().collect();
                let mut i = 0;
                while i < cs.len() {
                    let prev_ok = i == 0 || !cs[i - 1].is_ascii_alphanumeric();
                    // md:NNN —— 纯行号锚点，只验越界与空行
                    let md = prev_ok && cs[i] == 'm' && cs.get(i + 1) == Some(&'d') && cs.get(i + 2) == Some(&':');
                    // 速查:NNN —— 指 §廿三，顺带验章归属
                    let quick = cs[i] == '速' && cs.get(i + 1) == Some(&'查') && cs.get(i + 2) == Some(&':');
                    if md || quick {
                        if let Some((n, end)) = digits_at(&cs, i + 3) {
                            let claim = if quick { Some(23) } else { None };
                            check(&mut bad, &name, idx + 1, n, claim, &lines, &section_at);
                            i = end;
                            continue;
                        }
                    }
                    // §章[:：]NNN —— 带章名，三条件全验
                    if cs[i] == '§' {
                        let mut j = i + 1;
                        while j < cs.len() && NUM.contains(&cs[j]) {
                            j += 1;
                        }
                        let claim = cn2int(&cs[i + 1..j].iter().collect::<String>());
                        if cs.get(j) == Some(&':') || cs.get(j) == Some(&'：') {
                            j += 1;
                        }
                        if let (Some(claim), Some((n, end))) = (claim, digits_at(&cs, j)) {
                            check(&mut bad, &name, idx + 1, n, Some(claim), &lines, &section_at);
                            i = end;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
        }
        assert!(bad.is_empty(), "{} 处文档锚点错位：\n{}", bad.len(), bad.join("\n"));
    }

    /// 一行里出现的所有文档锚点行号（`md:` / `速查:` / `§章:`，全角冒号也算）。
    fn anchor_numbers(line: &str) -> Vec<u32> {
        let cs: Vec<char> = line.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < cs.len() {
            let prev_ok = i == 0 || !cs[i - 1].is_ascii_alphanumeric();
            let plain = prev_ok
                && ((cs[i] == 'm' && cs.get(i + 1) == Some(&'d') && cs.get(i + 2) == Some(&':'))
                    || (cs[i] == '速' && cs.get(i + 1) == Some(&'查') && cs.get(i + 2) == Some(&':')));
            if plain && let Some((n, end)) = digits_at(&cs, i + 3) {
                out.push(n);
                i = end;
                continue;
            }
            if cs[i] == '§' {
                let mut j = i + 1;
                while j < cs.len() && NUM.contains(&cs[j]) {
                    j += 1;
                }
                if j > i + 1 && matches!(cs.get(j), Some(&':') | Some(&'：')) && let Some((n, end)) = digits_at(&cs, j + 1) {
                    out.push(n);
                    i = end;
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    /// 机检可见面上的全部文档锚点行号。排除口径同裁定26：
    /// ① 测试代码自身——断言文案里的 `md:945` 和 `//!` 一样是叙述，算进来＝谁都能在测试里补一句行号，
    ///    把没实现的行判成已覆盖（本轮实测踩过一次：存档模块的 `///` 文档写了 §廿二 排除行的行号，
    ///    机检立刻红在"排除表过期"上）。判据取"`#[cfg(test)]` 之后紧跟 `mod`"，不是"看见 `#[cfg(test)]`
    ///    就跳过后半份文件"：函数级的那一个（`rng.rs` 的 `state()`）后面还有几十行真实现，
    ///    一并跳过等于机检自己造出一个假缺口——比漏锚点更难发现，因为它看起来像是照规则排除掉了。
    /// ② 裁定登记区的 `//!` 行里也写行号，但那是**叙述**不是实现锚点——算进来的话，
    ///    在注释里补一句"md:947"就能把一条没实现的规则判成已覆盖。
    fn referenced_doc_lines() -> Vec<u32> {
        let mut referenced: Vec<u32> = Vec::new();
        for fp in src_rs_files() {
            let src = std::fs::read_to_string(&fp).unwrap();
            let ls: Vec<&str> = src.lines().map(str::trim_start).collect();
            let mut idx = 0;
            while idx < ls.len() {
                let t = ls[idx];
                let next = ls[idx + 1..].iter().copied().find(|l| !l.is_empty()).unwrap_or("");
                if (t.starts_with("#[cfg(test)]") && next.starts_with("mod ")) || t.starts_with("mod anchor_tests") {
                    break;
                }
                if !t.starts_with("//!") {
                    referenced.extend(anchor_numbers(t));
                }
                idx += 1;
            }
        }
        referenced
    }

    /// 反向覆盖机检的**唯一人工入口**（§廿二 表体里不算规则的行），逐条必须写理由。
    /// 提到模块级而不藏在测试函数里：债表机检要拿它做交叉核对——同一条缺口不能既躺在排除表里
    /// 「不算规则」、又躺在债表里「算债」，两头下注等于两头都不负责。
    const NOT_A_RULE: &[(usize, &str)] = &[
        (970, "商业模式 挂Github——不是游戏规则，是发行渠道备注"),
        (971, "激励系统 通关进度+收集+成就+每日奖励——§廿二 这一行只是 §一:17 的复述，同一缺口不在排除表里洗白，已挂成有去处的显式债（见 NOT_IMPLEMENTED）"),
    ];

    /// **反向**覆盖机检（裁定26）：§廿二 边界表的每一行，src/ 里必须有至少一个锚点指回它。
    /// 正向机检只问"已有锚点指对了吗"，天生问不出"整条规则没落地"——md:945（死亡后业火达阈值不触发特性）
    /// 就是这么漏掉的：实现早在 `try_trigger_col` 的 `hp <= 0` 闸里，锚点一个没有，正向一路绿灯。
    /// 必检集合**从文档结构推导**（§廿二 表头之后的连续非空行，到 `---` 为止），不是手挑清单；
    /// 唯一的人工入口是下面的 `NOT_A_RULE` 排除表，每条必须写理由。
    /// 残留盲区（如实登记）：本检查只覆盖 §廿二 一张表；§十八 由
    /// `every_card_row_of_section18_is_anchored_back_and_matches_field_by_field` 单独覆盖（它额外要求逐字段等值，
    /// 原因见该测试的注释），§廿三 速查表与其它章的同类清单行仍未反向纳入。
    #[test]
    fn every_edge_case_row_of_section22_is_anchored_back_from_src() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "二十二、边界情况处理")
            .expect("§廿二 标题必须存在（文档结构变了就要同步改本检查）");
        // 表体＝标题之后到第一条 `---` 之间的非空行；`情况 处理` 是列头，不是规则。
        let mut rows: Vec<usize> = Vec::new();
        for (off, l) in lines[head + 1..].iter().enumerate() {
            let t = l.trim();
            if t == "---" {
                break;
            }
            if t.is_empty() || t == "情况 处理" {
                continue;
            }
            rows.push(head + 2 + off);
        }
        // 人工排除表见模块级 `NOT_A_RULE`（债表机检要交叉核对它）。
        let excluded: Vec<usize> = NOT_A_RULE.iter().map(|(n, _)| *n).collect();
        let required: Vec<usize> = rows.iter().copied().filter(|n| !excluded.contains(n)).collect();
        assert!(required.len() >= 25, "§廿二 表体至少 25 行，实测 {} 行 ⇒ 结构推导失效", required.len());

        let referenced = referenced_doc_lines();
        let missing: Vec<String> = required
            .iter()
            .filter(|&&n| !referenced.contains(&(n as u32)))
            .map(|&n| format!("  md:{n} ← {}", at(n)))
            .collect();
        assert!(
            missing.is_empty(),
            "§廿二 有 {} 行边界规则在 src/ 里没有任何锚点指回：\n{}",
            missing.len(),
            missing.join("\n")
        );
        // 假绿反证：锚点集合必须是**选择性**的，不能"什么行号都算命中"。
        for (n, why) in NOT_A_RULE {
            assert!(!referenced.contains(&(*n as u32)), "{why}，所以 md:{n} 本不该有锚点；现在有了⇒ 排除表过期，请连理由一起删掉");
        }
    }

    /// §十八 卡牌总表：每一卡行既要有锚点指回，又要把 费/数值/阈值/卡名 与代码逐字段比死。
    /// 只查"有没有锚点"在这一章会假绿——三行开端共用同一个 `STARTER` 定义，1 个定义能满足 3 行必检；
    /// 加上逐字段等值比对，"照抄零遗漏"才成为可证的（抄错一个阈值即红）。
    /// 行集合仍从**结构**推导：行首三个空白分隔 token 全为 ASCII 数字 ⇒ 卡行；阵营小节标题、三条列头、
    /// `---` 都不符合该形态 ⇒ 文档增删阵营或改列头措辞时，推导器不用改（不需要像 §廿二 那样硬写 `"情况 处理"`）。
    #[test]
    fn every_card_row_of_section18_is_anchored_back_and_matches_field_by_field() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "十八、卡牌总表")
            .expect("§十八 标题必须存在（文档结构变了就要同步改本检查）");
        let is_num = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let factions = [Faction::Ember, Faction::Frost, Faction::Shadow];
        // (文档行, 阵营序号, 卡名, 费, 数值, 阈值)
        let mut rows: Vec<(usize, usize, &str, i32, i32, i32)> = Vec::new();
        let mut heads: Vec<&str> = Vec::new();
        for (off, l) in lines[head + 1..].iter().enumerate() {
            let n = head + 2 + off;
            let t = l.trim();
            if t == "---" {
                break;
            }
            if t.is_empty() {
                continue;
            }
            let tk: Vec<&str> = t.split_whitespace().collect();
            let is_card = tk.len() >= 4 && is_num(tk[0]) && is_num(tk[1]) && is_num(tk[2]);
            if is_card {
                rows.push((n, heads.len() - 1, tk[3], tk[0].parse().unwrap(), tk[1].parse().unwrap(), tk[2].parse().unwrap()));
            } else if !t.starts_with("费 ") {
                heads.push(t); // 阵营小节标题：每出现一次换一张表
            }
        }
        assert_eq!(heads.len(), factions.len(), "§十八 阵营小节应有 {} 个，实测 {} 个 ⇒ 文档加了阵营，本检查的卡表映射要同步", factions.len(), heads.len());
        assert_eq!(rows.len(), 39, "§十八 卡行按形态推导应得 39 行，实测 {} 行", rows.len());
        // 小节顺序与代码 `faction_cards` 的阵营顺序绑定：文档换序即红，避免"按位置取表"静默指向另一阵营。
        for (g, h) in heads.iter().enumerate() {
            assert!(h.starts_with(factions[g].name()), "§十八 第 {} 个小节是「{h}」，代码 faction_cards 的第 {} 个阵营是 {} ⇒ 两边顺序不一致", g + 1, g + 1, factions[g].name());
        }
        // 三行开端必须逐字节相同——这是"代码只有一份 STARTER 定义"的前提。文档一旦给某阵营的开端加了差异，
        // 共享定义立刻不成立；这条比"每行都有锚点"更早发现。
        for n in [791usize, 808] {
            assert_eq!(at(n), at(774), "开端行 {n} 与 774 不再逐字节相同 ⇒ 三阵营共用 STARTER 的前提破了，须拆成三份定义");
        }
        let referenced = referenced_doc_lines();
        let mut bad: Vec<String> = Vec::new();
        for (n, g, name, cost, power, threshold) in &rows {
            if !referenced.contains(&(*n as u32)) {
                bad.push(format!("  md:{n} ← {} 没有任何锚点指回", at(*n)));
                continue;
            }
            let table = faction_cards(factions[*g]);
            match table.iter().find(|d| d.name == *name) {
                None => bad.push(format!("  md:{n} 文档卡「{name}」在 {} 表里不存在", factions[*g].name())),
                Some(d) if (d.name, d.cost, d.power, d.threshold) != (*name, *cost, *power, *threshold) => bad.push(
                    format!("  md:{n} 文档[{name} 费{cost} 值{power} 阈{threshold}] ≠ 代码[{} 费{} 值{} 阈{}]", d.name, d.cost, d.power, d.threshold),
                ),
                Some(_) => {}
            }
        }
        assert!(bad.is_empty(), "{} 处 §十八 卡表与文档不符：\n{}", bad.len(), bad.join("\n"));
    }

    /// §十二 完整回合流程：``` 围栏内的**每一条内容行**都必须被锚点指回，或挂成债——排除表在本围栏不作数。
    /// 集合从结构推导：章标题之后第一对 ``` 之间，非空且不以 `【` 开头的行全算必检——
    /// `【…】` 是阶段版式不是规则，其余（编号步、`a.` 子步、`- ` 列表、`→` 子步）一律是。
    /// **为什么不用行首形态筛**（`^\d+\. ` / `^\s+[a-z]\. ` / `^\s+- ` 三种，本帧先前量出的 55 行）：
    /// 那种筛法静默漏掉 md:452「- 若开端在场，我方获得1业力」与 §结算 的 494/495/496（行首无缩进）、
    /// 以及 443-449 七条 `→` 子步——共 17 行。一把用来堵"漏登记"盲区的尺子，自己不能带漏登的口径。
    /// 它补的是债表**唯一**证不到的那一面：`the_unimplemented_debt_table...` 能证债条没过期、不自我洗白，
    /// 证不了债表**完整**；这里按章推导"该登记的行"，漏记且漏锚 ⇒ 当场红。
    /// 残留盲区（如实登记）：① 只覆盖 §十二 一章，其余章同类清单仍未纳入（§十八 卡表另有逐字段等值检查）；
    /// ② 它只问"这一行有没有被认领"，不问"认领得对不对"——锚点指错实现由正向机检与卡表比对分担；
    /// ③ **假债仍然证伪不了**：一行已实现却被挂成"未实现"，去处只要格式合规（点名 `.rs` 或壳）机器就放行。
    ///    围栏**外**的章也仍能被排除表单边认领——那是①的推论：没有推导器的章，没有"这条认领走不通"的红。
    #[test]
    fn every_step_line_of_section12_is_anchored_back_or_debited() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "十二、完整回合流程")
            .expect("§十二 标题必须存在（文档结构变了就要同步改本检查）");
        let open = head + 1 + lines[head + 1..]
            .iter()
            .position(|l| l.trim() == "```")
            .expect("§十二 标题之后应有 ``` 围栏（改了版式就要同步改本检查）");
        let inner = &lines[open + 1..];
        let close = inner
            .iter()
            .position(|l| l.trim() == "```")
            .expect("§十二 的 ``` 围栏应有收尾");
        let mut steps: Vec<usize> = Vec::new();
        let mut headers = 0usize;
        for (off, l) in inner[..close].iter().enumerate() {
            let n = open + 2 + off; // 1 起的文档行号
            let t = l.trim();
            if t.is_empty() {
                continue;
            }
            if t.starts_with('【') {
                headers += 1;
                continue;
            }
            steps.push(n);
        }
        assert_eq!(steps.len(), 72, "§十二 围栏内内容行按结构推导应得 72 行，实测 {} 行 ⇒ 文档改了流程条目，或推导口径失效", steps.len());
        assert_eq!(headers, 11, "§十二 应有 11 个【…】阶段标题，实测 {} 个", headers);

        let referenced = referenced_doc_lines();
        let debited: Vec<usize> = NOT_IMPLEMENTED.iter().map(|d| d.doc).collect();
        // **排除表在本围栏不作数**：§十二 围栏内的行只有"锚点指回"与"挂债"两条认领路。
        // 这条不是设想，是量出来的：曾做过一次两段式洗白——删掉 md:452 的两处行尾锚点、同时把 452 塞进
        // `NOT_A_RULE`——全量 136 测一路绿。把排除表从认领路径摘掉后，同一条变异当场红（见下面的断言）。
        for (n, why) in NOT_A_RULE {
            assert!(
                !steps.contains(n),
                "md:{n} 落在 §十二 围栏内却被 `NOT_A_RULE` 认领＝排除表能吞掉真规则。要说它不算规则，请挂债并写去处；登记的排除理由：{why}"
            );
        }
        let missing: Vec<String> = steps
            .iter()
            .filter(|&&n| !referenced.contains(&(n as u32)) && !debited.contains(&n))
            .map(|&n| format!("  md:{n} ← {}", at(n)))
            .collect();
        assert!(
            missing.is_empty(),
            "§十二 有 {} 行回合流程既无锚点指回、也不在债表里（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );

        // 假绿反证：推导器两头都要有牙——标题确实被剔出集合、三种形态的规则行确实留在集合里。
        // 只钉"总行数 72"不够：把筛选条件同时放宽/收紧的改动可以让总数不变而成员换掉。
        for h in [413usize, 420, 425, 439, 451, 454, 461, 469, 481, 493, 498] {
            assert!(!steps.contains(&h), "md:{h} 是【…】阶段标题，不该进必检集（现在进了⇒ 筛选条件失效）");
        }
        for (n, form) in [(414, "编号步"), (431, "a. 子步"), (427, "缩进 - 步"), (443, "→ 子步"), (452, "行首无缩进的 - 步"), (479, "编号步尾条")] {
            assert!(steps.contains(&n), "md:{n}（{form}）被剔出必检集 ⇒ 推导器漏了这种形态");
        }
        // 债表/排除表只允许"认领"确实存在的行：认领到空行或标题行＝登记与推导口径不一致。
        for n in &debited {
            if *n > open + 1 && *n < open + 1 + close {
                assert!(steps.contains(n), "债表条目 md:{n} 落在 §十二 围栏里却不在必检集⇒ 两边推导口径不一致");
            }
        }
    }

    /// 债的**分档**——混档就是改写缺口的性质：呈现层欠的是设施（画不出颜色、没有音频），
    /// 规则层欠的是校验（引擎收了它不该收的走法）。后者会让同一局打出不同结果，前者不会。
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Tier {
        Presentation,
        Rule,
        /// 曾经躺在 `NOT_A_RULE` 里当"规则层范围外"，按裁定26 改挂成有去处的债。
        Washed,
    }

    impl Tier {
        fn label(self) -> &'static str {
            match self {
                Tier::Presentation => "呈现层·设施缺失",
                Tier::Rule => "规则层·真校验缺失",
                Tier::Washed => "豁免改登记",
            }
        }
    }

    /// 一条**显式债**：文档里有这一行，代码里没有它。
    struct Debt {
        /// 文档行号（1 起）。
        doc: usize,
        /// 该行所属章——与 `row` 一起把条目钉死在文档内容上，行号漂移当场红。
        section: u32,
        /// 文档该行 `trim()` 后的**逐字**原文。
        row: &'static str,
        tier: Tier,
        /// 现状证据：现有的闸只管到哪一步，为什么这一行不算已落地。
        evidence: &'static str,
        /// 去处：偿这条债要动的位置/设施。**没有去处的债＝把缺口换个地方再洗一次**。
        dest: &'static str,
    }

    /// 未实现债表（`reverse-coverage-multi-chapter` 的 D3）。逐条按"文档行 + 原文 + 证据 + 去处"录，
    /// 读数由本帧现取：呈现层 14 行（§十五 665/666 ＋ §十六 702/705/706/707/708/709/715/716/717/718/721
    /// ＋ §十二 479；704 是列头「状态 表现」，属非规则行不入债表）、规则层 4 行（§二 47/48 ＋ §十二 416/456）、
    /// 豁免改登记 1 行（§一 17）＝**19 条**。提案原文写"18 行／呈现层 15 行"，与它自己逐行列举的 14 差一，
    /// 本表以文档实测为准。§十二 那三条由
    /// `every_step_line_of_section12_is_anchored_back_or_debited` 从文档结构推导出来，不是手挑。
    const NOT_IMPLEMENTED: &[Debt] = &[
        Debt {
            doc: 17,
            section: 1,
            row: "激励 通关进度 + 卡牌收集 + 成就系统 + 每日挑战奖励",
            tier: Tier::Washed,
            evidence: "存档只落进度与每日完成位，成就/卡牌收集/奖励三样一个字段都没有；文档也没给出成就条目表",
            dest: "save.rs 扩表 + 呈现层展示；前置是文档先给成就与奖励清单（已挂 pool/）",
        },
        Debt {
            doc: 47,
            section: 2,
            row: "E1-E4 ✅（先放） ❌",
            tier: Tier::Rule,
            evidence: "敌方放置闸 battle.rs:663-683 只查业力／同名牌／本格占用，不查「该列后排还空着」；这条只在 ai.rs 的候选生成里成立",
            dest: "已按自决 #4（reverse-coverage-multi-chapter 实现侧自决第 4 行）定成 AI 行为约束、不加引擎闸；缺口照挂不洗白。若设计回话改判，开工点在 pool/back-row-placement-gate，闸位 battle.rs::enemy_place_legal",
        },
        Debt {
            doc: 48,
            section: 2,
            row: "E5-E8 ✅（后排满后） ✅",
            tier: Tier::Rule,
            evidence: "上一条同一道闸——「后排满后才能放前排」没有任何引擎校验，敌方（含 Boss）可直接往前排落子",
            dest: "与上一条同一道闸、同一条自决 #4：不加引擎闸。改判时与 47 一次改两处，闸位 battle.rs::enemy_place_legal，开工点 pool/back-row-placement-gate",
        },
        Debt {
            doc: 416,
            section: 12,
            row: "3. 卡牌升级（可选）",
            tier: Tier::Rule,
            evidence: "command.rs:264-267 的 `if !post_battle` 直接回「升级是通关奖励，仅结算阶段可用」——§十二 把「卡牌升级」同时列在 416 准备阶段与 501 结算阶段，引擎只认后者",
            dest: "已按自决 #1（reverse-coverage-multi-chapter 实现侧自决第 1 行）定为仅结算阶段可升；缺口照挂不洗白。真要两处都开＝把 `up_used` 持久化进 save.rs 的 progress.kv（撞裁定27 的存档语义，须单独一帧）",
        },
        Debt {
            doc: 456,
            section: 12,
            row: "1. 献祭阶段：评估献祭得分，决定是否献祭",
            tier: Tier::Rule,
            evidence: "ai.rs::run 把献祭与放置放进同一个候选池逐轮择优（`greedy_key` 沿用 §廿「放不出才祭」的门控），文档这里把献祭写成先于放置的独立一步；只有业力 0 时的 `starter_sac_at_zero` 算一个前置特例",
            dest: "两条路只能择一：按文档拆成「先献祭后放置」两段（改 ai.rs 决策序 ⇒ 敌方走法变，整族逐字节基线要有意重铺），或以 §廿 决策树为准把这一行降为描述性措辞。择哪条属设计侧，本帧只登记",
        },
        Debt {
            doc: 479,
            section: 12,
            row: "7. 红光渐渐消失",
            tier: Tier::Presentation,
            evidence: "battle.rs:910 的回滚日志打了「红光亮起」四个字，但没有「渐渐消失」这一步——全仓没有时间轴/渐隐设施",
            dest: "呈现层壳的一帧过渡；CLI 侧不重复计账（日志字串已在）",
        },
        Debt {
            doc: 665,
            section: 15,
            row: "触发时：屏幕周围变红",
            tier: Tier::Presentation,
            evidence: "render.rs 全文只输出字符棋盘，没有「屏幕周围」这个区域概念，更没有按事件染红",
            dest: "TUI/2D 壳的屏幕后处理；CLI 无对应物",
        },
        Debt {
            doc: 666,
            section: 15,
            row: "回滚完成后：红光渐渐消失（约0.5~1秒过渡）",
            tier: Tier::Presentation,
            evidence: "同上，且这一条带时长（0.5~1 秒渐退）——全仓无时间轴设施",
            dest: "壳的动画层；时长区间文档已给，可直接用",
        },
        Debt {
            doc: 702,
            section: 16,
            row: "百分比 = 当前业火值 / 阈值。颜色随百分比动态变化。",
            tier: Tier::Presentation,
            evidence: "render.rs 的业火条是「焰N/阈值」两个纯数字，无百分比",
            dest: "render.rs：现成的 N/M 相除即可，这一条 CLI 就能偿",
        },
        Debt {
            doc: 705,
            section: 16,
            row: "0-30% 暗红色，火焰微弱",
            tier: Tier::Presentation,
            evidence: "四档色带（暗红/橙红/亮橙/金红）在 src/ 全文 0 命中",
            dest: "CLI 可偿一半（四档字符条），真颜色留给壳",
        },
        Debt {
            doc: 706,
            section: 16,
            row: "30-60% 橙红色，火焰稳定",
            tier: Tier::Presentation,
            evidence: "同上——色档判定与配色都不存在",
            dest: "CLI 四档字符条 + 壳的颜色表",
        },
        Debt {
            doc: 707,
            section: 16,
            row: "60-90% 亮橙色，火焰旺盛",
            tier: Tier::Presentation,
            evidence: "同上——色档判定与配色都不存在",
            dest: "CLI 四档字符条 + 壳的颜色表",
        },
        Debt {
            doc: 708,
            section: 16,
            row: "90-99% 金红色，火焰跳动",
            tier: Tier::Presentation,
            evidence: "同上；「跳动」还是一帧动画，render.rs 没有帧序列概念",
            dest: "壳（颜色档 + 动画）",
        },
        Debt {
            doc: 709,
            section: 16,
            row: "100% 白金色，火焰爆发，屏幕微震",
            tier: Tier::Presentation,
            evidence: "白金色与震屏设施都没有，且文档未给震幅/时长",
            dest: "壳的屏幕后处理；震幅与时长待人给数，不自行取默认值",
        },
        Debt {
            doc: 715,
            section: 16,
            row: "1. 业火条火焰由橙红转为紫红",
            tier: Tier::Presentation,
            evidence: "特性触发只有一行日志，无「橙红→紫红」的过渡态",
            dest: "壳的过渡动画",
        },
        Debt {
            doc: 716,
            section: 16,
            row: "2. 卡牌爆发出紫红色火焰",
            tier: Tier::Presentation,
            evidence: "同上：src/ 全文无「紫红」",
            dest: "壳的爆发特效",
        },
        Debt {
            doc: 717,
            section: 16,
            row: "3. 屏幕四周出现玻璃裂纹",
            tier: Tier::Presentation,
            evidence: "「裂纹」在 src/ 全文 0 命中",
            dest: "2D/3D 壳的覆盖层；render.rs 不出图形，CLI 无等价物",
        },
        Debt {
            doc: 718,
            section: 16,
            row: "4. 裂纹扩散，伴随碎裂音效",
            tier: Tier::Presentation,
            evidence: "全仓无一行音频代码（「音效」0 命中），文档也未给音效资源名与时长",
            dest: "2D/3D 壳 + 资源清单待人给",
        },
        Debt {
            doc: 721,
            section: 16,
            row: "7. 玻璃碎片消散，火焰回落",
            tier: Tier::Presentation,
            evidence: "「碎片」0 命中——裂纹既不存在，也就没有消散",
            dest: "壳的粒子层（与 md:717 同一套设施）",
        },
    ];

    /// 未实现债表的机检（裁定26 的同族要求：缺口不许靠排除表洗白，必须挂成**有去处的显式债**）。四条：
    /// ① 每条目指向的文档行**逐字**仍是登记时那一行、且归属同一章——行号漂移当场红，债条不会悄悄换了对象；
    /// ② 机检可见面上**不许有任何锚点指回**这些行：谁实现并挂上锚点，本测就当场红"债已偿，请删条目"，
    ///    这是"债只减不增"唯一能被机器看见的形式；
    /// ③ 每条必须写去处，且去处要点名文件（`.rs`）或点名壳——"以后再说"不是去处；
    /// ④ 同一条文档行不许既躺在 `NOT_A_RULE`（不算规则）又躺在债表（算债），两头下注等于两头都不负责。
    /// 条数与分档各钉一个死数：加一条债必须同时改这两个数，等于每次加债都被迫看一眼它属于哪一档。
    /// **诚实盲区（登记，不在本测解）**：这张表是**手录**的。机器能证它没过期、不自我洗白，证不了它**完整**——
    /// 一行没实现、又没被记进来的文档行，本测照样绿。补这个盲区的前置是按章推导"规则行集合"（D2 的第三步），
    /// 读数与计划记在 `~/.Athena/projects/midline/working/reverse-coverage-multi-chapter.md`。
    #[test]
    fn the_unimplemented_debt_table_is_pinned_paid_off_and_never_washes_itself() {
        let Some(lines_s) = doc_or_skip() else { return };
        let lines: Vec<&str> = lines_s.iter().map(String::as_str).collect();
        let section_at = doc_sections(&lines);
        let referenced = referenced_doc_lines();
        let mut bad: Vec<String> = Vec::new();
        let mut prev: usize = 0;
        for d in NOT_IMPLEMENTED {
            let tag = format!("债条目 md:{}「{}」（{}）", d.doc, d.row, d.tier.label());
            if d.doc <= prev {
                bad.push(format!("{tag}：未按文档行号严格升序（上一条 md:{prev}）——重复或乱序会让「还剩几条债」数不出来"));
            }
            prev = d.doc;
            match lines.get(d.doc - 1) {
                None => bad.push(format!("{tag}：越界，全文仅 {} 行", lines.len())),
                Some(l) if l.trim() != d.row => bad.push(format!(
                    "{tag}：文档该行现在是「{}」⇒ 债条指向的对象变了，先分清是文档改了措辞还是行号漂移",
                    l.trim()
                )),
                Some(_) => {
                    let got = section_at(d.doc as u32);
                    if got != Some(d.section) {
                        bad.push(format!("{tag}：登记写第 {} 章，实际归属第 {got:?}", d.section));
                    }
                }
            }
            if referenced.contains(&(d.doc as u32)) {
                bad.push(format!(
                    "{tag}：src/ 已有锚点指回 ⇒ 债已偿，请把这条删掉，并把条数与分档两个数字一起改小。登记的证据是「{}」——先确认那份实现真的覆盖了这一行",
                    d.evidence
                ));
            }
            if NOT_A_RULE.iter().any(|(n, _)| *n == d.doc) {
                bad.push(format!("{tag}：同时躺在 NOT_A_RULE 里＝既「不算规则」又「算债」"));
            }
            if !(d.dest.contains(".rs") || d.dest.contains("壳")) {
                bad.push(format!("{tag}：去处没点名文件也没点名壳：{}", d.dest));
            }
        }
        assert!(bad.is_empty(), "未实现债表有 {} 处失效：\n{}", bad.len(), bad.join("\n"));
        assert_eq!(
            NOT_IMPLEMENTED.len(),
            19,
            "债表条数变了。偿了债 ⇒ 删条目并把本数字与下面的分档数一起改小；真要新增债 ⇒ 连同文档出处、证据、去处一起写"
        );
        let pres = NOT_IMPLEMENTED.iter().filter(|d| d.tier == Tier::Presentation).count();
        let rule = NOT_IMPLEMENTED.iter().filter(|d| d.tier == Tier::Rule).count();
        let washed = NOT_IMPLEMENTED.iter().filter(|d| d.tier == Tier::Washed).count();
        // 分档不许互相挪：把规则层挪进呈现层＝把"引擎收了它不该收的走法"说成"只是没画出来"，缺口的性质就变了。
        assert_eq!((pres, rule, washed), (14, 4, 1), "债表应为 呈现层14／规则层4／豁免改登记1，实测 ({pres},{rule},{washed})");
    }

    fn check(
        bad: &mut Vec<String>,
        file: &str,
        line: usize,
        n: u32,
        claim: Option<u32>,
        lines: &[&str],
        section_at: &dyn Fn(u32) -> Option<u32>,
    ) {
        let tag = format!("{file}:{line} → 文档行 {n}");
        if n as usize > lines.len() {
            bad.push(format!("{tag} 越界（全文仅 {} 行）", lines.len()));
            return;
        }
        if lines[n as usize - 1].trim().is_empty() {
            bad.push(format!("{tag} 指向空行"));
            return;
        }
        if let Some(claim) = claim
            && section_at(n) != Some(claim)
        {
            bad.push(format!("{tag} 声称第 {claim} 章，实际归属第 {:?} 章", section_at(n)));
        }
    }
}
