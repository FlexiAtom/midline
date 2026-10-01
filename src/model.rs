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
//!       （有实现、零锚点，正向一路绿灯）。新增 `every_edge_case_row_of_section22_is_anchored_back_or_debited`：
//!       §廿二 边界表每一行都必须被 src/ 的锚点指回，或挂成有去处的债。必检集**从文档结构推导**（表头之后到 `---` 的连续非空行），
//!       不是手挑清单；唯一人工入口是排除表，本帧由两条减到**一条**：md:970「商业模式 挂Github」＝发行备注不是规则。
//!       原第二条 md:971「激励系统」自己写着"已挂成有去处的显式债"却仍占着排除表＝同一缺口两头下注，
//!       现改由 §一:17 的债以**复述行**（`DEBT_RESTATEMENTS`）认领。
//!       本轮补齐 §廿二:943..969 共 26 行的锚点。叙述不算锚点：`//!` 裁定登记区与机检自身的文本都被排除在计数外。
//!     - **假绿反证（实测两次，都留了痕迹）**：① 第一版机检把自己注释里的 "md:945" 也算成锚点 ⇒
//!       手动删掉 `try_trigger_col` 的真锚点后测试**照样绿**；加排除逻辑后重做同一注入 ⇒ 红，报出
//!       `md:945 ← 卡牌死亡后业火值达阈值…`，撤销注入 ⇒ 绿。② 阈值门控用"12 张里恰好 5 张命中"反向锁死，
//!       防止将来把闸去掉时只看到"更多卡被强化"这种看着合理的假象。
//!     - **残留盲区（明写，不假装已解决）**：反向覆盖已纳入 **十四张表／章**——§廿二 边界表、§十八 卡牌总表、
//!       §十二 回合流程（第 30 条之后）、§廿三 规则总览速查（四条认领路：锚点／债／§廿二 同 key
//!       复述／否定行，见 `every_row_of_section23_…`）、§廿一 单机模式（模式表／每日挑战规则围栏／
//!       Boss 表三张子表共 14 行，只开锚点与挂债两条路，见 `every_row_of_section21_…`）、
//!       §十五 伤害回滚（**第一条非表的章**：34 行按形态推出来，认领路多开一条——「示例」围栏那 14 行
//!       只许走**用文档自身数字驱动引擎跑一遍**的实测，挂锚与挂债都不算数，见 `every_row_of_section15_…`
//!       与 `battle::parse_section15_examples`）、以及本帧的 §十七 挤压与推进（**第二张非表的章**：
//!       19 行＝我方挤压 2＋敌方推进 10＋双方对称 4＋中线绝对规则 3，标签三档形态见
//!       `every_row_of_section17_…`。本章**没有示例围栏**，所以 §十五 那第三条实测路在这里开不出来，
//!       只有锚点／挂债两路，实测配比钉死为 (19, 0)）、以及本帧的 §十六 业火条（**第一条同时走满三条路的章**：
//!       25 行按形态推出，配比钉死 锚点9／挂债13／示例实测3。标签判据在这章升到四档（新增「下一非空行是
//!       ≥3 行等元数连续 run ⇒ 那是表的小标题」），并首次需要**列头双条件**（结构上是一个 run 的首行 ＋
//!       字面命中「项目 说明」／「状态 表现」，两个方向都 fail-loud）与**纯制表符框线不算规则**（同一围栏内
//!       带文字的图注 697/698 照算，否则整张外观图会被当成"这不是规则"吞掉）。示例那 3 行由
//!       `battle::parse_section16_examples` 读文档自己的「阈值／业火值／触发次数／A-B=R」驱动引擎复现，
//!       见 `every_row_of_section16_…`）、以及本帧的 §八 技能池（**第一张整章走"锚点＋逐字段等值"两路并用的表**：
//!       13 行＝1 行「技能池（随机附加，12种）：」声明＋12 条技能行，全走锚点、零挂债。新在**比的两侧都不是
//!       手抄清单**：① 文档自己写的「12种」与 `Skill::list()` 的长度对撞；② 编号 ↔ `list()` 下标 ↔ 枚举声明序
//!       ↔ 变体 Debug 名四方同序；③「技能」列 ↔ 枚举行内注释**逐字节**（注释在这里是被当作被比对的数据读的，
//!       不是当锚点算）；④「效果」列 ↔ 「技能」列按**语义槽位**等值——这两列文档自己措辞就不同（md:291
//!       「攻击后自身+1累积」／「攻击后自身业火+1」：`+1` 漂了位置），量出来的口径是**「累积」与「业火」是引擎
//!       同一个 `flame` 的两种写法**，词表见 `s8_slots`，要求两侧全消费且范围／量纲两槽必填（吃不进的新措辞、
//!       或整槽缺失都当场红）；⑤ 变体名尾数与文档的增减量再对一次。这章**没跑引擎**，盲区照登：把
//!       「同列友方攻击+1」实现成 -1，这把尺量不到——12 个变体里只有 4 个有断言其效果的测（`AllyColAtk1`／
//!       `AllyColThreshM1`／`AtkSameColFlame1`／`EnemyColDmgTakenP1`），其余 8 个连"量纲落在哪个目标上"都没测过；
//!       补法（下一帧）＝把「效果」列解析出的 (时机, 目标, 量纲, 增减) 直接驱动引擎逐行复现，见 `every_skill_row_of_section8_…`）；
//!       以及本帧的 §七 卡牌模型（**第一条把尺子伸进"源码形状"的章**：22 行＝5 行字段块＋9 行术语表＋1 行小标题
//!       ＋5 行对照表＋1 行设计意图，全走锚点、零挂债。①② 仍对字面——字段名逐字节（注释在这里第二次被当数据读），
//!       且**宿主＝措辞**：文档写「自带／固定」的字段必须落在 `CardDef`，写「随机附加」的必须落在 `CardInst`。
//!       ③④⑤ 读的不是文本而是**代码形状**：③ 文档「每张牌1个」↔ `tr` 是标量、「每张牌0~N个」↔ `skills: Vec<…>`
//!       是容器；④ 示例行那两个引号串必须分别在 `TraitKind::label` 的返回值里、以及在 §八 表体「技能」列 12 行原文里
//!       原样命中（跨章等值）；⑤ md:279＋md:282 说「融合只融合技能，不融合特性」⇒ `fuse_cards` 函数体**剔注释后**
//!       必须出现 `skills` 且不得出现 `.tr`。形状尺带来一件前面九章没有的事：**行为回归第一次由推导器自己抓住**——
//!       M38 在融合里写 `m.def.tr = s.def.tr`（把副牌特性盖到主牌上），推导器当场红；而 §十七 M22／§十六 M26／§八 M32
//!       的同种变异推导器全绿、红的是别处的正向测。缺口也照登：M38 单跑时全仓**只有这一条红** ⇒「特性取主牌」当时
//!       没有任何正向测断言过，同帧已在 `fuse_moves_skills_keeps_main_and_costs_sub_minus_one` 补上断言（现为双红）。
//!       ② 单独不可达，如实登记：① 要求逐字节 ⇒ 宿主判据只有在**文档换掉措辞**时才开口，且那时它与 ① 同红（M35 实测）；
//!       留它的理由不是"多一道保险"，而是它报的是**原因**（字段错层）而不是症状（字面不符）。**仍未纳入**的是
//!       §一/§二/§三/§四/§十/§十一/§十三/§十九/§廿 那九章的散文行——那里"整条规则没落地"仍查不出；
//!       语义是否被曲解始终不可机检，仍靠人向设计逐条核对。
//!       以及本帧的 §十四 持业者·蜡烛（**第一条同章并用两条路、且九行双记账的章**：12 行＝围栏外表体 3
//!       （走锚点）＋围栏内 9（走"文档数字驱动引擎复现"，**同时**一行都不许缺锚点）。配比钉死 锚点3／挂债0／实测9，
//!       上面另钉一层「12 行全有锚」——分区计数看不见那一层，M39 实测：只摘围栏行 592 的锚，配比照旧 (3,0,9)、
//!       整族 149 全绿，红的是那条双记账断言。M39 顺带量出锚点扫描的一个口径：**交叉引用也算锚**
//!       （592 还被 battle.rs:1015「（与 §十四:592 同一条规则的另一侧）」提到一次，逐处独立取号 ⇒ 变异要两处一起摘）。
//!       形状判据在本帧改过一次，是实测撞出来的：§十六/§十五 那套「等元数 run」吃不进这张表——三列空格切出来是
//!       4/4/3（`❌ 不减短` 占两段），于是 580/582 都不再被认成标签／列头，580 反倒伪装成必检行（首跑就红在这里）；
//!       现判据＝「连续非空 run ≥ 2」＋字面词表，结构条件照样在，只是不再假设列间空格数一致。
//!       ② 把 §十四:583「正常燃烧…**不减短」**这条否定式条款做成了**封闭的蜡烛写点名单**（构造 3＋Boss 档案覆写 2＋
//!       伤害 5＝10 条，逐字钉死）——本仓第一次不给这类条款只挂锚，M41 加一条文档里没有的"每回合自动衰减"就红在这里。
//!       ③ 文档写的 20 必须等于引擎常量 `CANDLE_HP`，且两侧同读它（M42 把常量改成 21：③＋实测＋既有测共四红）；
//!       ④ 复现测必须还在；⑤ md:599「对称」＝`candle_bar(` 全仓恰 3 次（1 定义＋敌我各 1 调用，M43 摘掉敌方那侧即红）。
//!       电池 M39–M46 每条指定它红在**哪一层**；M44 反过来把这帧新加的尺改了——只数 `fn 名字(` 拦不住"摘掉 `#[test]`"
//!       （函数体一字不动、149 掉成 148 而尺子绿），现在还要求定义上方第一条非注释行恰是 `#[test]`。
//!       盲区照登：② 名单认的是字面上的 `p_candle`/`e_candle`，谁把蜡烛搬进别的字段名整族就看不见；md:598 只写单烛判死，
//!       Boss 双烛（`e_candle2`）与档案覆写初始长度都**超出 §十四 的措辞**，这两处只由 ② 的名单与 §廿一 各自钉着。
//!       §十五 那一帧暴露的**否定式条款**盲区在本章又添两例：§十七:763「任何卡牌不能越过中线」锚在
//!       `DeathCause` 的变体列表上（指向的是"这里没有第四因"），§十七:747「后排多张只推最靠近前排的」
//!       锚在"每列后排单格"这条结构事实上（指向的是"没有第四档"）——两条都机检证伪不了，同 §十五:621
//!       那类"不包括"（§十五:630 上一帧已补真测 `rollback_excess_does_not_flow_back_into_the_dealt_ledger`，
//!       621 仍只有锚点）。
//!       能证伪的那半本轮补了真测：`a_squeezed_card_dies_at_its_own_line_and_never_reaches_the_other_side`
//!       （挤压后果全留本侧：本侧弃堆＋本侧业力，对面八格逐格身份不动、蜡烛不减）。
//!       电池 M20–M23 另外量出一件比盲区更要紧的事：**锚点尺抓不住行为回归**。M22 把推进的前排占用闸
//!       改成"永远放行"，§十七 推导器**照样绿**（锚点还在原地，实现却变了），红的是两条既有 Boss 账本测
//!       ⇒ 反向覆盖只保证"这行有人指回过"，行为面仍靠正向测兜——两把尺不可互相替代。
//!       本帧 M24–M27 把**三条路各自的咬合力**分开量了一遍：M24 抹 §十六:678 的锚 ⇒ 只红在推导器（漏登记）；
//!       M25 把文档副本示例行 689 的「触发1次」改成「触发2次」⇒ **只**红在示例实测，推导器全绿
//!       （正证第三条路真在读文档的数字跑引擎，不是把期望硬编码在测里）；M26 摘掉同回合上限闸 ⇒ 示例实测与
//!       既有 `flame_12_over_thr6_triggers_once_keeps_6` 双双红而推导器绿（M22 那条边界在业火线上复现）；
//!       M27 把已挂债的 705 塞进排除表 ⇒ 排除表断言与债表交叉核对**同时**红（两头下注被两头拒）。
//!     - 以及本帧的 §九 融合系统（**第一条四条路在同一章并用满的章**：
//!       27 行＝围栏外 6（时机表体 3＋设计意图 350＋冲突处理 352／354，全走锚点）＋围栏内 21——其中 14 条
//!       **规则**行走实测**且**一行不许缺锚（双记账，同 §十四），6 条**示例**行走实测**且不许有锚**
//!       （§十五/§十六 那条纪律第一次落到"由解析器变体 `is_example` 判"，推导器不写行号清单），1 条挂债
//!       （md:335「继承堆总数不变」，引擎 2 张进 1 张出，正好相反）。配比钉死 锚点6／挂债1／实测20。
//!       两处形状判据是实测改的：① 标签判据加一条**否决「以全角冒号收尾」**——md:354 单 token 且下一非空正是
//!       ```，按 §十四 那条会被吞成标签，可它是条件句、取值在下一道围栏里（否决前 labels 多出 354、围栏外只剩 5 行）；
//!       ② 本章有五道围栏＝10 个 ``` 符，§十四「只读第一道」的口径在这里会把四道一起放尺外，所以围栏符数量本身钉死。
//!       一处**计数顺序**与前三章相反：债排在实测之前——唯一那条债就躺在流程围栏里，按 §十四 的顺序它会被记成
//!       "实测 21"，那条与文档相反的账就此隐身。md:357 那条等式判成**规则**不判成示例：它不点名任何一对牌，
//!       它是「冲突则叠加」的通式，于是它既在 progress.rs:51 有锚、又被等式两侧对撞引擎的累积值实测。
//!       本仓第一次在尺子里加了一层**文档自己两处对撞**（不碰引擎也能红）：第 1 步「保留其特性+数值+阈值+费用」
//!       ↔ 第 4 步逐行那四条 `- X = 主牌X`；示例的价 3-1=2 ↔ 副牌那一行的费用 ↔ 流程第 3 步的「-1，最低0」；
//!       等式左边合计 ↔ 右边。
//!       **本帧最大的收获不在这章，而在机检自身**：`referenced_doc_lines` 原先只把"`#[cfg(test)]` 紧跟 `mod`"
//!       之后当测试面，于是挂在**普通 item**（enum／struct／fn／impl）上的 cfg(test) 整段仍被算成生产码面——
//!       `progress.rs` 那个测试专用 enum 的变体注释「md:341」立刻被读成锚点，**一条发布构建里不存在的注释
//!       差点替一条示例"作证"**，而示例行的纪律恰恰是不许有锚（这条红是本帧第一次由推导器抓到机检自己漏计，
//!       不是抓到实现漏计）。现三段 walk（锚点面／字面次数面／蜡烛写点面）共用一份 `production_face`，
//!       item 边界按**与属性同缩进的收尾**判；缩进这一步也是实测逼出来的——首版按"顶格 `}`"找收尾，
//!       `battle.rs:786`（impl 块里的一个 cfg(test) 方法）一路吞到 impl 收尾，十章锚点集体消失、10 条测红，
//!       那批红正是注释里写着要避免、这帧仍然撞上的那种「机检自己造出的假缺口」。
//!       盲区照登：335 那条债**不复现**（复现＝替文档把反账做平）；⑤ 只核"命令词在不在词表里"这个形状，
//!       不核准备／结算阶段的执行序（属 §十二:415/500）；`crafted` 之后"离场永久消失"属 §十:372，不在本章配比；
//!       语义曲解仍不可机检——把「叠加」实现成「取最大」，④ 那层量不到，只有引擎实测那层与 §八 的效果尺能兜。
//!     - 以及本帧的 §五 开端与 §六 发牌（**连着的两帧同一形状：一张表＋一道围栏，两章都开了"文档数字驱动引擎"的实测**）。
//!       §五 19 行＝围栏外表体 8＋围栏内 11，配比钉死 锚点8／挂债0／实测11——第一次把文档那张卡**逐字段**撞回引擎的起始牌定义，
//!       又把文档自己写的上限拼成码面串去 grep（④ 与逐字段层 ③ 的分工由 M64／M66 分开量到：只红一层＝另一层不是它的重复）。
//!       §六 12 行**全**走实测，配比 锚点0／挂债0／实测12，是纳入尺子的十四章里唯一一条一行锚都不靠的章；本章围栏里的规则行
//!       **没有前缀**（不像 §十四 那样带 `- `），外壳判据失效，改用**八种内容读法**逐一认领、吃不进就当场 panic（M75 量的正是这颗牙）。
//!       两章各把一条否定式做成**封闭写点名单**：§五 那三行「无／❌／❌」与 §六 那句「开局手牌不计入每回合抽牌次数」——名单扫的是
//!       生产码面上所有**写** `manual_draws` 一类的行，多一条＝有人另起了一个扣额度的地方，少一条＝那条路被摘了。
//!       盲区照登：同一形状的规则仍写在两处（推导器的表体名单／解析器的形态判定，属手抄，待办 k）；「N 个数」类形状守卫
//!       **单独降级**时四把尺都看不见——M68／M69／M78 三次量到同一形状，只留字不立闸。
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
//!     生效、引擎与 Boss 不查；§一:17 成就+每日奖励零实现）。**D3 已落、D2 第三步续之**：这些真未实现的文档行现在以
//!     `NOT_IMPLEMENTED` 债表的形式住在下面的锚点机检里（**当时 23 条＝呈现层 15／规则层 4／豁免改登记 1／模式层 3**，
//!     逐条带原文、证据与去处，偿一条就当场红）。提案原文写"18 行／呈现层 15 行"，与它自己逐行列举的 14 差一，
//!     本表以文档实测为准；多出的几条是 §十二:456、§廿三:980（横屏）与本帧 §廿一:915/922/923（Roguelike 与
//!     每日挑战的固定卡组、金币），全部由各自章的推导器列出，不是手挑。
//!     计划、读数与 6 条待人裁都在
//!     `~/.Athena/projects/midline/working/reverse-coverage-multi-chapter.md`，本处只登记口径。
//!     - 本章**不能只查锚点存在**：三行开端（774/791/808）逐字节相同、代码里是同一个 `STARTER`，只查锚点会让
//!       1 个定义满足 3 行必检＝假绿。故新增 `every_card_row_of_section18_is_anchored_back_and_matches_field_by_field`：
//!       卡行按**形态**推导（行首三个空白分隔 token 全为 ASCII 数字 ⇒ 卡行；列头、阵营小节标题、`---` 自然排除，
//!       不像 §廿二 那样硬写 `"情况 处理"` 字面量——那条只跳一次，文档一加第四阵营就静默漏），再与 `faction_cards`
//!       逐字段（费/数值/阈值/卡名）比死，并断言三行开端逐字节相同、小节顺序与 `Faction` 顺序一致。
//!     - 假绿反证实测两次：改 `影仆` 阈值 3→4 ⇒ 红 `md:809 文档[…阈3] ≠ 代码[…阈4]`；摘掉 `// md:809` ⇒ 红
//!       `md:809 ← 1 2 3 影仆 无 从技能池随机 没有任何锚点指回`；复原 ⇒ 106 全绿（基线 104 ＋ 本测 ＋ 献祭越界回归测）。
//!     - 写这条时**未纳入**的是 §廿三 速查表、§十二 的编号步骤（411-509 是 ``` 围栏内的 `1.` 与 `   a.` 两级
//!       步骤，当时以为要第三档推导器）以及若干散文章。前两项后来各开了一档推导器就纳入进去了，清单每帧都在动
//!       ⇒ **不再在此复述**（写死一次就会像这样过期），以本文件「残留盲区」那条的当前清单为准。
//!       语义是否被曲解始终不可机检，仍靠人向设计逐条核对。

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
    pub fn list() -> [Skill; 12] {  // §廿三:999 技能池 12 种，数组长度即编译期计数；§八:288 文档写的是同一个数
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

    /// §七:280 示例行的技能列「攻击后自身+1累积」是文档 §八 表体里的原文，短标签由它派生；
    /// 两个 `label`（这里与 `TraitKind::label`）就是 §七:273「特性 vs 技能」并置的那两份定义。
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

// §七:273 「特性 vs 技能」——两张对照表把这两个概念并置，代码里也是这两个相邻的 `impl`。
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

// §七:251 「卡牌 = {」——玩家可见的五个字段就定义在这两个结构体上（另两个在 `CardInst`）。
#[derive(Clone, Copy, Debug)]
pub struct CardDef {
    pub name: &'static str,
    #[allow(dead_code)] // 数据完整性：卡表按阵营归档，读取方在渲染层（后置）
    pub faction: Faction,
    // §七:252 费用（字段面；消耗点见 battle.rs 的 place）
    pub cost: i32, // 费用
    // §七:253 数值（字段面；出招时当伤害读，见 battle.rs 的 `let base = atk.hp`）
    pub power: i32, // 数值（伤害 = 生命）
    // §七:254 阈值（字段面；比较点在 battle.rs 的触发闸）
    pub threshold: i32, // 阈值（触发特性所需业火值）
    // §七:255 特性（字段面）；§七:277 每张牌 1 个——`tr` 是**标量**类型就是这个断言的形状
    pub tr: TraitKind, // 特性（固定，卡牌自带）
}

pub const STARTER: CardDef = CardDef {  // §廿三:990 开端（0费/数值1/阈值4）；§五:176 这张卡＝§五 那张表的本体（表体逐字段等值见 §五 推导器；旧注释里那几个裸行号已换成带锚的落点）
    name: "开端",
    faction: Faction::Ember,
    cost: 0, // §五:179 费用 0
    power: 1, // §五:180 数值 1（文档同一行写「血量=伤害=1」⇒ 一个字段两处用，见 §七:265／§七:266）
    threshold: 4, // §五:181 阈值 4
    tr: TraitKind::Starter, // §五:182 特性＝四子句的类型标签；四子句各自的落点都带自己的锚（免费放置、献祭得 2、在场回合末 +1 且每关上限 2、死亡得 2）
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
    // §七:276 来源＝「卡牌自带，固定／随机附加，可融合」——这一行是两条读法的物理分界：
    //        定义（自带、固定）挂在 `def: CardDef` 上，随机附加的东西只能长在实例上。
    pub def: CardDef,
    // §七:256 技能（字段面）；§七:277 每张牌 0~N 个——`Vec<Skill>` 就是这个断言的形状
    pub skills: Vec<Skill>, // 技能（随机附加，可融合）
    /// 当前数值（= 血量）。每关开始重置满格。
    pub hp: i32,
    pub flame: i32, // §十六:676 文档叫「业火条」，引擎里就是这张卡身上的累积值；§七:269 文档术语行叫「业火值」＝累积伤害
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
            hp: def.power,  // §廿三:988 三位一体：数值落地即血量，出招时再当伤害读；§七:266 血量＝卡牌的生命值＝数值（这个初始化就是那个等号）
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
/// 文档不随仓库分发。候选：环境变量 → 仓库同级 → 上两级（本仓在 ~/rust/midline，文档在 ~/）。
/// 放在模块层而不是 `anchor_tests` 里，是因为 §十五 的示例实测测住在 `battle.rs`（它要动引擎），
/// 而它必须与 `model.rs` 的反向覆盖推导器**共用同一份读取口径**——两处各读一遍文档，
/// 一处按行号 1 起、一处按 0 起，就会朝不同方向错，而"这行有没有被核对过"恰恰只在这类偏移上才会静默绿。
#[cfg(test)]
pub(crate) fn locate_doc() -> Option<std::path::PathBuf> {
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
#[cfg(test)]
pub(crate) fn doc_or_skip() -> Option<Vec<String>> {
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

/// 规则文档不在仓库内（与本仓同级 `中线.MD`），文件缺失时跳过而非失败。
/// 语义是否被曲解无法机检——那仍靠人向设计逐条核对。
#[cfg(test)]
mod anchor_tests {
    use super::{Faction, Skill, doc_or_skip, faction_cards};

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

    /// **生产码面**的行序列（trim 后）。反向覆盖的三段 walk——`referenced_doc_lines`／`code_occurrences`／
    /// `s14_candle_writes`——共用这一份口径。为什么摘成一份：三段各写一遍时，改一处另两处不跟着红，
    /// 而"某个串在实现里出现几次""某行有没有锚点指回"这类断言恰恰要靠三份同口径才算成立
    /// （同 `doc_sections`／`table_body`／`s7_fn_body` 摘出来的理由）。
    ///
    /// 两条排除，性质不同，别合成一条：
    /// ① `#[cfg(test)]` **后面紧跟 `mod`** ⇒ 从这里到文件尾一律不算（测试模块总在文件尾部）。
    /// ② `#[cfg(test)]` 挂在**普通 item**（enum／struct／fn／impl）上 ⇒ 只跳掉那**一个** item：
    ///    属性行之后直到**与属性同缩进的那行 `}`**（顶层 item 即 0 列，判法是 `s7_fn_body` 那条"块内花括号
    ///    带缩进"的推广——只看缩进相等，不数花括号，字符串里的 `{}` 就骗不到它）。
    ///    缩进这一步是实测逼出来的：`battle.rs:786` 的 `#[cfg(test)] pub(crate) fn s9_run_player_attack_phase`
    ///    是 impl 块里的一个方法，按"顶格 `}`"去找会一路吞到 impl 收尾（1486 行），§十四～§廿三 十章的锚点
    ///    当场集体消失——**机检自己造出的假缺口**，正是这条口径原先写著要避免的那种错。
    /// ② 是本帧被 §九 逼出来的：`progress.rs` 的 `S9Claim` 是测试专用 enum，它的变体注释写「md:341」，
    ///    老口径（只认 ①）把这段注释算成锚点 ⇒ 一条**发布构建里不存在**的注释替 md:341 那条示例"作证"，
    ///    而示例行的纪律恰恰是**不许有锚**（§十五/§十六 立的那条）。这不是新增洗白口，是把它堵上：
    ///    判据只认「该 item 不参与发布构建」这一件事，不认措辞。
    /// 为什么不能简化成"看见 `#[cfg(test)]` 就跳过后半份文件"：函数级的那一个（`rng.rs` 的 `state()`）
    /// 后面还有几十行真实现，一并跳过等于机检自己造出一个假缺口——比漏锚点更难发现，因为它看起来像是照规则排除掉了。
    /// 找不到那个 item 的收尾就**当场 panic**，不平跳：静默吞掉后半份文件的代价比报一次错大。
    fn production_face(src: &str) -> Vec<String> {
        let raw: Vec<&str> = src.lines().collect();
        let indent_of = |l: &str| l.chars().take_while(|c| c.is_whitespace()).count();
        let mut out: Vec<String> = Vec::new();
        let mut idx = 0usize;
        while idx < raw.len() {
            let t = raw[idx].trim();
            let next = raw[idx + 1..].iter().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
            if t.starts_with("mod anchor_tests") || (t.starts_with("#[cfg(test)]") && next.starts_with("mod ")) {
                return out;
            }
            if t.starts_with("#[cfg(test)]") {
                let indent = indent_of(raw[idx]);
                let start = idx + 1;
                idx += 1;
                let mut opened = false;
                loop {
                    if idx >= raw.len() {
                        panic!("第 {start} 行起的 `#[cfg(test)]` item 找不到与它同缩进的收尾（一路读到文件尾）⇒ 测试专用设施请收进 `#[cfg(test)] mod …`，否则本尺分不清它有多长，会静默跳过后半份文件");
                    }
                    let l = raw[idx];
                    let lt = l.trim();
                    if indent_of(l) == indent && !lt.is_empty() {
                        if lt == "}" {
                            idx += 1;
                            break;
                        }
                        if !opened && lt.ends_with(';') {
                            idx += 1;
                            break;
                        }
                        opened |= lt.contains('{');
                    }
                    idx += 1;
                }
                continue;
            }
            out.push(t.to_string());
            idx += 1;
        }
        out
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

    /// **锚点语法的唯一定义**：`md:NNN`／`速查:NNN`／`§章:NNN`（章号必须中文数字；`§` 支冒号半角全角都认，
    /// `md`／`速查` 支只认半角——这是两把尺原先就一致的旧口径，今天全仓 0 个全角样本，差别只能靠验收单钉住）。
    /// 返回 `(声称的章号, 文档行号, 消费到的位置)`——`md` 不带章号故为 `None`，`速查` 固定 §廿三。
    /// 为什么摘成一份（同 `doc_sections`／`production_face` 摘出去的理由）：原先正向尺自己走一遍循环，
    /// 冒号写成**可选**（`if 是冒号 { j += 1 }`），而反向覆盖的 `anchor_numbers` 冒号**必需**——于是
    /// **章号后直接跟行号**（少一个冒号）这种写法被正向尺当成合法锚点验一遍判绿、反向覆盖却一个字没看见：
    /// **一把尺替另一把尺看不见的东西作证**，比两把都不认更难发现，因为全仓唯一的信号是"绿"。
    /// 实测（本帧）：§五 首次登记锚点一轮就积出 8 处那种形状，全仓归一之后才立得起下面那条判据。
    /// 少冒号的写法现在由 `no_doc_anchor_is_written_without_its_colon` 单独报红；两处都只认这一份语法。
    fn anchor_at(cs: &[char], i: usize) -> Option<(Option<u32>, u32, usize)> {
        // `.` 也排除：`README.md:12` 里那个 `md:12` 是**文件名＋行号**，不是 中线 第 12 行的指回。
        // 今天全仓 0 处这种写法（反例照下一条测钉住），但只要有天有人在注释里写一句某个 `.md` 的第几行，
        // 反向覆盖就会替 中线 的那一行"作证"——虚覆盖比漏锚点更难发现，因为它看起来是绿的。
        let prev_ok = i == 0 || (!cs[i - 1].is_ascii_alphanumeric() && cs[i - 1] != '.');
        const LABELS: [(&[char; 2], Option<u32>); 2] = [(&['m', 'd'], None), (&['速', '查'], Some(23))];
        if prev_ok {
            for (label, claim) in LABELS {
                if cs[i..].starts_with(label) {
                    let at = i + label.len();
                    if cs.get(at) == Some(&':')
                        && let Some((line, end)) = digits_at(cs, at + 1)
                    {
                        return Some((claim, line, end));
                    }
                    return None;
                }
            }
        }
        if cs[i] == '§' {
            let mut j = i + 1;
            while j < cs.len() && NUM.contains(&cs[j]) {
                j += 1;
            }
            if j == i + 1 {
                return None;
            }
            let claim = cn2int(&cs[i + 1..j].iter().collect::<String>())?;
            let at = match cs.get(j) {
                Some(&':' | &'：') => j + 1,
                _ => return None,
            };
            let (line, end) = digits_at(cs, at)?;
            return Some((Some(claim), line, end));
        }
        None
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
                    if let Some((claim, n, end)) = anchor_at(&cs, i) {
                        check(&mut bad, &name, idx + 1, n, claim, &lines, &section_at);
                        i = end;
                        continue;
                    }
                    i += 1;
                }
            }
        }
        assert!(bad.is_empty(), "{} 处文档锚点错位：\n{}", bad.len(), bad.join("\n"));
    }

    /// 一行里出现的所有文档锚点行号——直接走 `anchor_at`，不再自己认一遍语法（同一处定义、两个读法）。
    fn anchor_numbers(line: &str) -> Vec<u32> {
        let cs: Vec<char> = line.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < cs.len() {
            if let Some((_, n, end)) = anchor_at(&cs, i) {
                out.push(n);
                i = end;
                continue;
            }
            i += 1;
        }
        out
    }

    /// 机检可见面上的全部文档锚点行号。排除口径同裁定26：
    /// ① 测试代码自身——断言文案里的 `md:945` 和 `//!` 一样是叙述，算进来＝谁都能在测试里补一句行号，
    ///    把没实现的行判成已覆盖（本轮实测踩过一次：存档模块的 `///` 文档写了 §廿二 排除行的行号，
    ///    机检立刻红在"排除表过期"上）。码面边界由 `production_face` 给，那里写着两条排除各自的理由。
    /// ② 裁定登记区的 `//!` 行里也写行号，但那是**叙述**不是实现锚点——算进来的话，
    ///    在注释里补一句"md:947"就能把一条没实现的规则判成已覆盖。
    fn referenced_doc_lines() -> Vec<u32> {
        let mut referenced: Vec<u32> = Vec::new();
        for fp in src_rs_files() {
            let src = std::fs::read_to_string(&fp).unwrap();
            for t in production_face(&src) {
                if !t.starts_with("//!") {
                    referenced.extend(anchor_numbers(&t));
                }
            }
        }
        referenced
    }

    /// 反向覆盖机检的**唯一人工入口**（§廿二 表体里不算规则的行），逐条必须写理由。
    /// 提到模块级而不藏在测试函数里：债表机检要拿它做交叉核对——同一条缺口不能既躺在排除表里
    /// 「不算规则」、又躺在债表里「算债」，两头下注等于两头都不负责。
    /// 本帧从 2 条减到 1 条：原 (971, 激励系统) 那条的理由自己写着「已挂成有去处的显式债」，却仍占着排除表——
    /// 那是同一缺口两头下注的另一种写法。现在它改走 `DEBT_RESTATEMENTS`，排除表只留真正的"不是规则"一行。
    const NOT_A_RULE: &[(usize, &str)] = &[(970, "商业模式 挂Github——不是游戏规则，是发行渠道备注")];

    /// 一条债在**别处**的复述行：同一个缺口在文档里被打了好几次（§一 立债，§廿二/§廿三 各复述一遍），
    /// 债只记一次、其余登记为复述——否则一个缺口要背三条债，"还剩几条"这个数就失去意义。
    /// 这不是新开的洗白口，它比"每条自己挂债"更严：① 复述行的行首 key 必须与主债行同前缀（不许把不相干的
    /// 行塞进来抵账）；② 主债行必须仍躺在 `NOT_IMPLEMENTED` 里（偿了债 ⇒ 复述自动失去认领资格，当场红）；
    /// ③ 复述行自己不许有任何锚点（同"债已偿"那条判据）。三条校验在 `the_unimplemented_debt_table_…` 里跑。
    const DEBT_RESTATEMENTS: &[(usize, &[usize], &str)] = &[(
        17,
        &[971, 1029],
        "「激励」在 §一:17 立债（成就/收集/奖励零字段），§廿二:971 与 §廿三:1029 是同一缺口的两次复述",
    )];

    /// 反问式否认的措辞（否定行判据的一半，另一半是"该 key 在生产码面 0 命中"）。
    /// 两个字形都收，因为**文档自己两处写法不同**：§廿二:970 写「哪来的」、§廿三:1028 写「那来的」。
    /// 这不是把判据放宽——放宽的是拼写匹配，断言强度不变；顺带把这处不一致量出来登记在状态文档里。
    const RHETORICAL_DENIALS: &[&str] = &["哪来的", "那来的"];

    /// §廿三 的**否定行**：文档断言"没有这个东西"，于是既没有实现可锚、也不欠实现（不能挂债），
    /// 但也不能塞进排除表——那正是上一帧在 §十二 量出的洗白路。判据全部从文档措辞推导，不接受手写行号：
    /// 内容列**整列**是「无」，或含反问式否认。裸的「无」子串不算——「无卡→打中线」里的「无」是条件不是断言。
    fn is_absence_row(content: &str) -> bool {
        content == "无" || RHETORICAL_DENIALS.iter().any(|m| content.contains(m))
    }

    /// 债的认领面＝主债行 ∪ 复述行。三张表的"挂债"都走这一个函数，别让两份清单各数各的。
    fn debt_claimed_lines() -> Vec<usize> {
        let mut v: Vec<usize> = NOT_IMPLEMENTED.iter().map(|d| d.doc).collect();
        for (_, rs, _) in DEBT_RESTATEMENTS {
            v.extend(rs.iter().copied());
        }
        v
    }

    /// **生产码面**（口径同 `referenced_doc_lines`：走到该文件第一个 `#[cfg(test)] mod` 即停）里，
    /// 再剔掉纯注释行之后，`needle` 出现的次数。注释不算实现——否则"在注释里提一句"就能让否定行失去牙；
    /// 测试面不算实现——否则给"没有这东西"写一条断言，反倒把它变成有了。
    fn code_occurrences(needle: &str) -> usize {
        let mut hits = 0;
        for fp in src_rs_files() {
            let src = std::fs::read_to_string(&fp).unwrap();
            for t in production_face(&src) {
                if !t.starts_with("//") {
                    hits += t.matches(needle).count();
                }
            }
        }
        hits
    }

    /// 从文档结构推导一张「章标题 → 列头 → 连续非空行 → `---`」式表格的**表体行号**（1 起）。
    /// §廿二 与 §廿三 共用这一份走法：两条推导器各抄一遍围栏，改了标题字面量或停止条件时另一边不会跟着红——
    /// 那正是"两把尺朝不同方向错、而交叉断言偏要两边同错才算成立"的形态（同 `doc_sections` 摘出来的理由）。
    fn table_body(lines: &[String], title: &str, column_header: &str) -> Vec<usize> {
        let head = lines
            .iter()
            .position(|l| l.trim() == title)
            .unwrap_or_else(|| panic!("文档里找不到「{title}」这一章标题（结构变了就要同步改推导器）"));
        let mut rows = Vec::new();
        for (off, l) in lines[head + 1..].iter().enumerate() {
            let t = l.trim();
            if t == "---" {
                break;
            }
            if t.is_empty() || t == column_header {
                continue;
            }
            rows.push(head + 2 + off);
        }
        rows
    }

    /// **反向**覆盖机检（裁定26）：§廿二 边界表的每一行，要么被 src/ 的锚点指回，要么挂成债。
    /// 正向机检只问"已有锚点指对了吗"，天生问不出"整条规则没落地"——md:945（死亡后业火达阈值不触发特性）
    /// 就是这么漏掉的：实现早在 `try_trigger_col` 的 `hp <= 0` 闸里，锚点一个没有，正向一路绿灯。
    /// 必检集合**从文档结构推导**（§廿二 表头之后的连续非空行，到 `---` 为止），不是手挑清单；
    /// 唯一的人工入口是下面的 `NOT_A_RULE` 排除表，每条必须写理由。挂债不算人工入口——债条本身由
    /// `the_unimplemented_debt_table_…` 逐条验原文/章归属/去处，并且**不许有锚点**，所以这条路有牙。
    /// 残留盲区（如实登记）：§十八 由
    /// `every_card_row_of_section18_is_anchored_back_and_matches_field_by_field` 单独覆盖（它额外要求逐字段等值，
    /// 原因见该测试的注释）；§廿三 由 `every_row_of_section23_…` 覆盖；§廿一 由
    /// `every_row_of_section21_…` 覆盖（一张章里三张子表）；§十五 由 `every_row_of_section15_…`、
    /// §十七 由 `every_row_of_section17_…`、§十六 由 `every_row_of_section16_…` 覆盖——这三章不是表，
    /// 必检行按**形态**推导，口径与各自的标签／列头／框线判据见各自测试注释；§八 由
    /// `every_skill_row_of_section8_…` 覆盖（同 §十八 的逐字段等值形状，但比的四层不同，见该测试注释）；
    /// §十四 由 `every_row_of_section14_…` 覆盖（也是按形态推，但它开了**双记账**：围栏里那 9 行既走
    /// 文档数字驱动引擎的实测，也一行都不许缺锚点，见该测试注释）。
    /// **其余各章**（§一/§二/§三/§四/§十/§十一/§十三/§十九/§廿）那九章仍未反向纳入。
    #[test]
    fn every_edge_case_row_of_section22_is_anchored_back_or_debited() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let rows = table_body(&lines, "二十二、边界情况处理", "情况 处理");
        // 人工排除表见模块级 `NOT_A_RULE`（债表机检要交叉核对它）。
        let excluded: Vec<usize> = NOT_A_RULE.iter().map(|(n, _)| *n).collect();
        let required: Vec<usize> = rows.iter().copied().filter(|n| !excluded.contains(n)).collect();
        assert!(required.len() >= 25, "§廿二 表体至少 25 行，实测 {} 行 ⇒ 结构推导失效", required.len());

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        let missing: Vec<String> = required
            .iter()
            .filter(|&&n| !referenced.contains(&(n as u32)) && !debited.contains(&n))
            .map(|&n| format!("  md:{n} ← {}", at(n)))
            .collect();
        assert!(
            missing.is_empty(),
            "§廿二 有 {} 行边界规则既无锚点指回、也不在债表里：\n{}",
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

    /// §八 技能池：13 行必检（1 行「12种」声明 + 12 条技能行）**全部**要有锚点指回，并与代码逐字段比死。
    /// 与 §十八 同一个假绿形状：只数"这一行有没有被认领"，那么 12 枚锚全钉在同一个 `match` 分支上也算绿，
    /// 而文档把某个技能名抄错一个字不会红。所以这里比四层：
    /// ① 行集合从**结构**推导（章标题之后到 `---`，跳空行与列头），钉死 13 行＝1 声明＋12 数据；
    /// ② 「12种」是**文档自己写的数字**，与 `Skill::list()` 的长度对撞——文档改数即红，期望不写在测里；
    /// ③ 编号 ↔ `Skill::list()` 下标 ↔ 枚举声明顺序 ↔ 变体 Debug 名，四方同序；
    /// ④ 文档「技能」列 ↔ 枚举行内注释（生产面文本，**逐字节**）；「效果」列 ↔ 「技能」列按**语义槽位**等值。
    ///    ④ 分开两种比法是因为文档自己两处措辞不同：md:291 技能「攻击后自身+1累积」／效果「攻击后自身业火+1」——
    ///    「累积」与「业火」是引擎同一个 `flame` 的两种写法（本帧量出的口径），且 `+1` 漂了位置，逐字节比不了。
    /// 槽位词表是本检查唯一的人工入口，两侧都必须**全消费**：文档新增一种措辞（把「相邻列」写成「邻接列」）
    /// 词表吃不下 ⇒ 当场红，不许静默跳过。变体名尾部的数字再与文档的增减量对一次（`AllyColAtk1` ↔ ±1），
    /// 这样"文档改成 +2 而枚举名没跟着改"也会红。
    /// 残留盲区（如实登记）：④ 只证**文档自洽＋文档与命名对齐**，没跑引擎——把「同列友方攻击+1」实现成 -1，
    /// 本推导器与那 12 枚锚都不会红。补法（下一帧）：把「效果」列解析出的 (时机, 目标, 量纲, 增减) 直接驱动
    /// 引擎逐行复现，代价是要新加一组"量一个数"的测量入口（攻击／受伤／阈值／业火四种量纲各一个）。
    #[test]
    fn every_skill_row_of_section8_is_anchored_back_and_matches_field_by_field() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let rows = table_body(&lines, "八、技能池", "编号 技能 效果");
        assert_eq!(
            rows,
            vec![288, 291, 292, 293, 294, 295, 296, 297, 298, 299, 300, 301, 302],
            "§八 表体按结构推导出的行集合变了 ⇒ 文档加了／改了行，先看清是哪一行的措辞让形态判据换档，再同步这里"
        );
        let is_num = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        // (文档行, 编号, 技能列, 效果列)；声明行按"不是三列数字开头"落进 claim。
        let mut data: Vec<(usize, usize, String, String)> = Vec::new();
        let mut claim: Vec<usize> = Vec::new();
        for &n in &rows {
            let tk: Vec<&str> = at(n).split_whitespace().collect();
            if tk.len() == 3 && is_num(tk[0]) {
                data.push((n, tk[0].parse().unwrap(), tk[1].to_string(), tk[2].to_string()));
            } else {
                claim.push(n);
            }
        }
        assert_eq!(claim, vec![288], "§八 应只剩一行不是技能数据（「技能池（随机附加，12种）：」），实测 {claim:?}");
        // ② 文档自己写的种数 ↔ 代码数组长度（把这一行的全部 ASCII 数字取出来当数字，多于一段就红）
        let digits: String = at(288).chars().filter(char::is_ascii_digit).collect();
        assert_eq!(
            digits.parse::<usize>().ok(),
            Some(Skill::list().len()),
            "§八:288「{}」里的数字与 `Skill::list()` 长度 {} 不符",
            at(288),
            Skill::list().len()
        );

        // ④ 枚举行内注释＝被比对的另一侧。读源码文本，不读 `stringify!` 之类编译期产物：注释不是语义，
        //    但它**是**文档措辞的落点，比"再抄一份名字表"少一个会各自漂移的副本。
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let src = std::fs::read_to_string(manifest.join("src/model.rs")).unwrap();
        let sls: Vec<&str> = src.lines().map(str::trim).collect();
        let e = sls
            .iter()
            .position(|l| *l == "pub enum Skill {")
            .expect("§八 推导器要读 `pub enum Skill {` 的声明行；枚举改名或挪出 model.rs ⇒ 同步这里");
        let mut decls: Vec<(String, String)> = Vec::new();
        for l in &sls[e + 1..] {
            if *l == "}" {
                break;
            }
            if l.is_empty() || l.starts_with("//") {
                continue;
            }
            let (ident, comment) = l.split_once("//").unwrap_or((l, ""));
            decls.push((ident.trim().trim_end_matches(',').trim().to_string(), comment.trim().to_string()));
        }
        assert_eq!(decls.len(), Skill::list().len(), "枚举声明 {} 个变体 ≠ `Skill::list()` {} 个 ⇒ 漏进池子或漏出文档", decls.len(), Skill::list().len());

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        let pool: Vec<String> = Skill::list().iter().map(|s| format!("{s:?}")).collect();
        let mut bad: Vec<String> = Vec::new();
        let mut reworded: Vec<usize> = Vec::new();
        for (idx, (n, num, skill, effect)) in data.iter().enumerate() {
            let tag = format!("  md:{n} ← {}", at(*n));
            if !referenced.contains(&(*n as u32)) {
                bad.push(format!("{tag} 没有任何锚点指回"));
                continue;
            }
            if debited.contains(n) {
                bad.push(format!("{tag} 既有锚点指回又挂着债 ⇒ 同一行两头下注"));
                continue;
            }
            if *num != idx + 1 {
                bad.push(format!("{tag} 文档编号 {num} 不是第 {} 行 ⇒ 编号漂了，锚点会指向另一个变体", idx + 1));
                continue;
            }
            let (decl, comment) = &decls[*num - 1];
            if decl != &pool[*num - 1] {
                bad.push(format!("{tag} 枚举里第 {num} 个变体是 `{decl}`，`Skill::list()` 第 {num} 个是 `{}` ⇒ 两边顺序不一致", pool[*num - 1]));
            }
            if comment != &format!("{num} {skill}") {
                bad.push(format!("{tag} 枚举行内注释 {comment:?} ≠ 文档技能列（应写成 \"{num} {skill}\"）"));
            }
            let a = s8_slots(skill);
            let b = s8_slots(effect);
            if a != b {
                bad.push(format!("{tag} 「技能」列槽位 {a:?} ≠ 「效果」列槽位 {b:?} ⇒ 文档两处措辞不同义"));
            }
            let tail: String = pool[*num - 1].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<String>().chars().rev().collect();
            if tail.parse::<i32>() != Ok(a.delta.abs()) {
                bad.push(format!("{tag} 文档增减量 {} ↔ 变体名 `{}` 尾数 {tail:?} 不符", a.delta, pool[*num - 1]));
            }
            if skill != effect {
                reworded.push(*n);
            }
        }
        // 六行"技能≠效果"是文档自己的两种写法；把这份读数钉住，改措辞／增删行都会先在这里响。
        assert_eq!(reworded, vec![291, 292, 293, 294, 299, 300], "§八 里「技能」列与「效果」列不同字面的行变了：实测 {reworded:?}");
        assert!(bad.is_empty(), "{} 处 §八 技能表与代码不符：\n{}", bad.len(), bad.join("\n"));
    }

    /// §八「技能」／「效果」两列的语义槽位——本检查唯一的人工词表。
    /// 「业火」与「累积」同归一个量纲（文档对引擎 `flame` 的两种写法，本帧量出来的口径）；「对」「目标」是介词噪声。
    /// 解析要求**全消费**且**范围／量纲两槽必填**：吃不进的字要红，整槽缺失也要红——
    /// 否则文档把「同列友方攻击+1」简写成「同列友方+1」，两列照样"相等"，尺子就空转了。
    #[derive(Debug, PartialEq, Eq)]
    struct S8Slots {
        ev: &'static str,
        scope: &'static str,
        side: &'static str,
        qty: &'static str,
        delta: i32,
    }

    fn s8_eat(rest: &mut String, words: &[(&str, &'static str)]) -> Option<&'static str> {
        words.iter().find_map(|(w, cls)| match rest.find(*w) {
            Some(p) => {
                rest.replace_range(p..p + w.len(), "");
                Some(*cls)
            }
            None => None,
        })
    }

    fn s8_slots(s: &str) -> S8Slots {
        // 时机要在量纲之前吃：「攻击后」含「攻击」，反了就把时机读成了量纲。
        const EV: &[(&str, &str)] = &[("攻击后", "攻"), ("放置时", "放"), ("死亡时", "亡")];
        const SCOPE: &[(&str, &str)] = &[("相邻列", "邻"), ("自身", "己"), ("同列", "同")];
        const SIDE: &[(&str, &str)] = &[("友方", "友"), ("敌方", "敌")];
        const QTY: &[(&str, &str)] = &[("业火", "焰"), ("累积", "焰"), ("攻击", "攻量"), ("受伤", "伤量"), ("阈值", "阈")];
        let mut rest = s.to_string();
        let ev = s8_eat(&mut rest, EV).unwrap_or("常驻");
        let scope = s8_eat(&mut rest, SCOPE).unwrap_or_else(|| panic!("§八 槽位解析：「{s}」里没有范围（自身／同列／相邻列）"));
        let side = s8_eat(&mut rest, SIDE).unwrap_or("己");
        let qty = s8_eat(&mut rest, QTY).unwrap_or_else(|| panic!("§八 槽位解析：「{s}」里没有量纲（业火／累积／攻击／受伤／阈值）"));
        let cs: Vec<char> = rest.chars().collect();
        let mut found = None;
        for i in 0..cs.len() {
            if (cs[i] == '+' || cs[i] == '-') && cs.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                let mut j = i + 1;
                while j < cs.len() && cs[j].is_ascii_digit() {
                    j += 1;
                }
                let v: i32 = cs[i + 1..j].iter().collect::<String>().parse().unwrap();
                found = Some((i, j, if cs[i] == '+' { v } else { -v }));
                break; // 只吃一个；多出来的靠下面的全消费兜住
            }
        }
        let (from, to, delta) = found.unwrap_or_else(|| panic!("§八 槽位解析：「{s}」里没有 ±数字 的增减量"));
        rest = cs[..from].iter().chain(cs[to..].iter()).collect();
        for w in ["目标", "对"] {
            while rest.contains(w) {
                rest = rest.replace(w, "");
            }
        }
        let left: String = rest.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(left.is_empty(), "§八 槽位词表吃不下「{s}」剩下的「{left}」⇒ 文档新增了一种措辞，补全词表（不许静默跳过）");
        S8Slots { ev, scope, side, qty, delta }
    }

    /// §七 字段块解析：取 `marker` 这个结构体声明里每个**字段行**的三元组 (名字, 类型, 行尾注释)。
    /// 独立注释行（`//`／`///`）与属性行（`#[…]`）跳过——锚点就是这样独立成行挂在字段上方的，
    /// 字段行尾只留文档原文，才能让 ① 那层做逐字节比对。
    fn s7_fields(src: &str, marker: &str) -> Vec<(String, String, String)> {
        let sls: Vec<&str> = src.lines().map(str::trim).collect();
        let h = sls
            .iter()
            .position(|l| *l == marker)
            .unwrap_or_else(|| panic!("§七 推导器要读 `{marker}` 的声明行；结构体改名或挪出 model.rs ⇒ 同步这里（不许静默跳过）"));
        let mut out = Vec::new();
        for l in &sls[h + 1..] {
            if *l == "}" {
                break;
            }
            if l.is_empty() || l.starts_with("//") || l.starts_with("///") || l.starts_with("#[") {
                continue;
            }
            let (decl, comment) = l.split_once("//").unwrap_or((l, ""));
            let nt = decl.trim().trim_start_matches("pub ").trim_end_matches(',');
            let (name, ty) = nt
                .split_once(": ")
                .unwrap_or_else(|| panic!("§七 字段解析：「{}」不是 `pub 名字: 类型` 的形状", decl.trim()));
            out.push((name.to_string(), ty.trim().to_string(), comment.trim().to_string()));
        }
        assert!(!out.is_empty(), "§七 字段解析：`{marker}` 里一个字段都没读到");
        out
    }

    /// 取顶层函数（以 `sig` 起头的行）到第一个顶格 `}` 之间的函数体，**每行的行尾注释剔掉**。
    /// §七:279「特性不参与融合」这层要问的是"代码里动没动 `tr`"，注释里写"特性取主牌"不算动过。
    fn s7_fn_body(src: &str, sig: &str) -> String {
        let sls: Vec<&str> = src.lines().collect();
        let h = sls
            .iter()
            .position(|l| l.trim_start().starts_with(sig))
            .unwrap_or_else(|| panic!("§七 推导器要读 `{sig}` 的函数体；它改名、拆函数或挪出 ⇒ 同步这里（不许静默跳过）"));
        let mut body: Vec<String> = Vec::new();
        for l in &sls[h + 1..] {
            if *l == "}" {
                // 顶层函数的收尾花括号在 0 列；块内的 `}` 带缩进，`l.trim() == "}"` 会误判成函数结束。
                assert!(!body.is_empty(), "§七 推导器：`{sig}` 是空函数体 ⇒ 判据无从下手");
                return body.join("\n");
            }
            let t = l.trim();
            if t.starts_with("//") || t.starts_with("///") {
                continue;
            }
            body.push(l.split("//").next().unwrap_or("").to_string());
        }
        panic!("§七 推导器没找到 `{sig}` 的结尾花括号——函数体判据失效")
    }

    /// `impl X` 里 `label()` 的返回值名单（按源码顺序）：只收 `X::变体 => "串"` 这一形状。
    fn s7_labels(src: &str, enum_prefix: &str) -> Vec<String> {
        let pat = format!("{enum_prefix}::");
        let out: Vec<String> = src
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with(&pat) && l.contains("=> \""))
            .map(|l| l.split('"').nth(1).unwrap_or_default().to_string())
            .collect();
        assert!(!out.is_empty(), "§七 名字解析：`{enum_prefix}` 里一个 label 都没读到");
        out
    }

    /// 一行（已剔行尾注释、已 trim）是否在**写**蜡烛的长度。三种形状：
    /// ① `名字 -= 量`；② `名字 = 值`（token 必须落在 `=` 左侧，否则 `self.over = Some(match self.p_candle…`
    /// 这种"只在右边读"会被误计成写）；③ 构造期的 `名字: 值,`。
    /// 判据故意不含 `<=`／`==`：`if self.e_candle <= 0` 是读，把它算进写点名单等于让否定式条款失去意义。
    fn s14_is_candle_write(t: &str) -> bool {
        let name = ["p_candle", "e_candle"].iter().filter_map(|k| t.find(k)).min().unwrap_or(usize::MAX);
        t.starts_with("p_candle:") || t.starts_with("e_candle:") || t.starts_with("e_candle2:")
            || t.find("-=").is_some_and(|k| name < k)
            || t.find(" = ").is_some_and(|k| name < k)
    }

    /// 蜡烛的**写点全名单**（生产码面，口径同 `code_occurrences`＝都走 `production_face`，再剔掉行尾注释与
    /// 纯注释行）。§十四:583「正常燃烧…**不减短」**是条否定式
    /// 条款，锚点指回证不了"没有别的东西在减蜡烛"——只有这份**封闭名单**能证：名单里没有"回合推进／每回合
    /// 衰减"，那句"不减短"才不是空话。
    fn s14_candle_writes() -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for fp in src_rs_files() {
            let src = std::fs::read_to_string(&fp).unwrap();
            for raw in production_face(&src) {
                let t = raw.split("//").next().unwrap_or("").trim().to_string();
                if !t.is_empty() && !t.starts_with("//") && (t.contains("p_candle") || t.contains("e_candle")) && s14_is_candle_write(&t) {
                    out.push(t);
                }
            }
        }
        out.sort();
        out
    }

    /// §六:226「开局手牌不计入每回合抽牌次数」这条**否定式**的机器形式：生产码面上所有**写** `manual_draws` 的行，
    /// 逐字收集、排序。为什么不能只数「出现几次」：读点（`<= 0` 那道闸、AI 的 `> 0`、HUD 的 `format!`）与写点混在
    /// 一个计数里，摘掉开局那步的额度扣减和给 HUD 加一句打印会给出同一个数。
    /// 判写点的三条字面形状（`-= `／带空格的 ` = `／`manual_draws: ` 那种结构体字面量与字段声明）故意保守：
    /// `<= 0` 里的 `= ` 前面是 `<`，所以那条闸算读点，不算写点。
    fn s6_manual_draw_writes() -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for fp in src_rs_files() {
            let src = std::fs::read_to_string(&fp).unwrap();
            for raw in production_face(&src) {
                let t = raw.split("//").next().unwrap_or("").trim().to_string();
                if t.contains("manual_draws") && (t.contains("-= ") || t.contains(" = ") || t.contains("manual_draws: ")) {
                    out.push(t);
                }
            }
        }
        out.sort();
        out
    }

    /// 实测路的行，靠的是那条**引擎复现测**真的还在跑。本帧实测过：只钉"行集合由解析器给出"拦不住删测——
    /// 解析器照样被推导器调用、行数照样对，只是再没有人把那些数字打进引擎。于是按函数名钉一次定义，
    /// §十四／§十五／§十六 三章的实测路共用这把尺。
    /// 但"定义还在"也不等于"测还在"：把函数上方的 `#[test]` 摘掉，函数体一字不动、`fn 名字(` 照样命中一次，
    /// cargo 却再也不跑它（变异 M44 实测到的）。所以这里除了数定义，还要求定义上方第一条非空非注释行恰是 `#[test]`。
    fn engine_repro_test_exists(name: &str) {
        let pat = format!("fn {name}(");
        let mut hits = 0usize;
        for fp in src_rs_files() {
            let src = std::fs::read_to_string(fp).unwrap();
            let ls: Vec<&str> = src.lines().collect();
            for (i, l) in ls.iter().enumerate() {
                if !l.trim().starts_with(pat.as_str()) {
                    continue;
                }
                hits += 1;
                let mut k = i.saturating_sub(1);
                loop {
                    let above = ls[k].trim();
                    if above.is_empty() || above.starts_with("//") {
                        if k == 0 {
                            break;
                        }
                        k -= 1;
                        continue;
                    }
                    assert_eq!(
                        above,
                        "#[test]",
                        "`{name}` 定义上方的第一条非注释行是「{above}」而不是 `#[test]` ⇒ 这条复现测已经不再被 cargo 跑到，实测路那些行的数字没人验了"
                    );
                    break;
                }
            }
        }
        assert_eq!(hits, 1, "引擎复现测 `{name}` 应恰有 1 处定义，实测 {hits} 处 ⇒ 它被改名／删掉／复制了，实测路的行已经没人跑");
    }

    /// §七 卡牌模型反向覆盖：22 条必检行**全走锚点、零挂债**，另加五层等值。    ///
    /// 必检行由形态推导（不接受手写清单）：章标题到章末 `---` 之间，trim 后**含字母或汉字**的行全算——
    /// 这一条顺带剔掉 ``` 围栏符、单独成行的 `}` 与 `---`（它们没有字母数字），不必另设框线判据。
    /// 再剔两类：**列头**沿用 §十六 的列头双条件（结构上是等元数 run 的首行 ＋ 字面命中词表
    /// `S7_HEADERS`），**小标签**沿用 §十七/§十六 那条（单 token ＋ 下一非空行是列头，本章即 md:260
    /// 「术语明确定义」）。词表吃不进的新列头会掉进必检行 ⇒ 没人锚它就当场红，与 §八 词表同一条全消费纪律。
    /// md:273「特性 vs 技能」是三个 token，**不算**标签 ⇒ 它自己也要有锚点。
    ///
    /// 五层等值（§八 那把尺只做"文档↔命名"对齐、不碰实现形状；③④⑤ 直接读**代码形状**）：
    /// 实测结果 M38＝在融合里把副牌特性盖到主牌上 ⇒ 本章推导器**当场红**，这是十章里第一次行为回归由形状尺抓到
    /// （§十七 M22／§十六 M26／§八 M32 同种变异时，推导器全绿、红的是别处正向测）。M38 单跑时全仓只这一条红
    /// ⇒ 当时没有任何正向测断言过「特性取主牌」，同帧已补进 `progress.rs` 的融合测，现为双红。
    /// ① 字段名逐字节：fenced 块的 5 行（去尾逗号）↔ `CardDef`／`CardInst` 的字段行尾注释，
    ///    注释含锚点记号（`§`／`md:`）的不算名字注释；两侧**全消费**——没挂名字注释的字段名必须
    ///    等于钉死的 12 个内部记账字段，多一个少一个都红。
    /// ② 宿主＝文档措辞的性质：括号里写「自带／固定」的字段必须在 `CardDef`（定义层），
    ///    写「随机附加」的必须在 `CardInst`（实例层）。**单独不可达**，如实登记：① 要求逐字节，所以② 只有
    ///    在文档**换掉措辞**时才开口，那一刻与 ① 同红（M35 实测）。留它是因为它报的是原因（字段错层），不是症状（字面不符）。
    /// ③ 数量行 ↔ 类型形状：「每张牌1个」⇒ `tr` 是标量；「每张牌0~N个」⇒ `skills` 是 `Vec<…>`。
    /// ④ 示例行 ↔ 跨章等值：特性示例必须是 `TraitKind::label` 的某个返回值，技能示例必须是 §八 表体
    ///    「技能」列的某个原文——文档自己两处（§七 示例／§八 表体）不同步就红。
    /// ⑤ 融合行＋设计意图 ↔ `fuse_cards` 函数体：体内（剔注释）必须有 `skills`、不许有 `.tr`。
    ///
    /// 残留盲区（如实登记）：术语表「定义」列那 9 串话、以及 278「定义卡牌定位」这半句语义仍只走锚点，
    /// 曲解了量不出；③⑤ 读的是**源码文本形状**，改名式重构（`Vec<Skill>` 换成别的容器、`fuse_cards`
    /// 拆成两个函数）会红在解析器上——那是**要求同步**而不是判错，口径与本仓其他推导器一致。
    #[test]
    fn every_row_of_section7_card_model_is_anchored_back_and_matches_the_code_shape() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "七、卡牌模型")
            .expect("§七 推导器按标题找章；章标题改名或挪走 ⇒ 同步这里");
        let end = head
            + 2
            + lines[head + 1..]
                .iter()
                .position(|l| l.trim() == "---")
                .expect("§七 章末的 `---` 分隔线不见了 ⇒ 推导器的章界判据失效");
        let next_text = |n: usize| -> String {
            let mut i = n + 1;
            while i <= end && at(i).trim().is_empty() {
                i += 1;
            }
            at(i).trim().to_string()
        };
        const S7_HEADERS: [&str; 2] = ["术语 定义", "项目 特性 技能"];
        let arity = |s: &str| s.split_whitespace().count();

        let mut rows: Vec<usize> = Vec::new();
        for n in (head + 2)..=end {
            let t = at(n).trim();
            if t.is_empty() || !t.chars().any(char::is_alphanumeric) {
                continue;
            }
            if S7_HEADERS.contains(&t) {
                let q = next_text(n);
                assert!(
                    arity(t) == arity(&q) && arity(t) >= 2,
                    "md:{n}「{t}」在列头词表里却**不是**等元数 run 的首行 ⇒ 文档改了表形，词表得跟着改（结构与字面两个方向都要对得上）"
                );
                continue;
            }
            let q = next_text(n);
            if arity(t) == 1 && S7_HEADERS.contains(&q.as_str()) {
                continue; // 小标签：单 token ＋ 下一非空行是列头
            }
            rows.push(n);
        }
        assert_eq!(
            rows,
            vec![
                251, 252, 253, 254, 255, 256, 263, 264, 265, 266, 267, 268, 269, 270, 271, 273, 276, 277, 278, 279,
                280, 282
            ],
            "§七 必检行集合变了 ⇒ 文档加了／改了行，先看清是哪一行的形态让判据换档，再同步这里"
        );

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let model = std::fs::read_to_string(manifest.join("src/model.rs")).unwrap();
        let prog = std::fs::read_to_string(manifest.join("src/progress.rs")).unwrap();
        let mut bad: Vec<String> = Vec::new();
        let tag = |n: usize| format!("  md:{n} ← {}", at(n));

        // ②' 认领路：本章只许走锚点，一条都不许挂债（挂债＝"这行没实现"，而 §七 的字段／术语全在代码里）。
        for &n in &rows {
            if !referenced.contains(&(n as u32)) {
                bad.push(format!("{} 没有任何锚点指回", tag(n)));
            }
            if debited.contains(&n) {
                bad.push(format!("{} 既有锚点指回又挂着债 ⇒ 同一行两头下注", tag(n)));
            }
        }

        // —— 结构解析：fenced 字段块、术语表、特性vs技能表 ——
        let f1 = (head + 2..end)
            .find(|&n| at(n).trim() == "```")
            .expect("§七 找不到 fenced 块的开场 ```");
        let f2 = (f1 + 1..end)
            .find(|&n| at(n).trim() == "```")
            .expect("§七 找不到 fenced 块的收尾 ```");
        let block: Vec<String> = ((f1 + 1)..f2).map(|n| at(n).trim().to_string()).collect();
        assert_eq!(block.len(), 7, "§七 字段块应是开场＋5 个字段＋收尾共 7 行");
        assert_eq!(block[0], "卡牌 = {", "§七 字段块开场行不是「卡牌 = 花括号开」，实测 {:?}", block[0]);
        assert_eq!(block[6], "}", "§七 字段块收尾行不是单独一个花括号，实测 {:?}", block[6]);
        let doc_fields: Vec<String> = block[1..6].iter().map(|l| l.trim_end_matches(',').to_string()).collect();

        let th = (head + 2..end)
            .find(|&n| at(n).trim() == "术语 定义")
            .expect("§七 找不到术语表列头");
        let mut terms: Vec<(String, String)> = Vec::new();
        let mut n = th + 1;
        while n <= end && !at(n).trim().is_empty() {
            let tk: Vec<&str> = at(n).split_whitespace().collect();
            assert!(tk.len() >= 2, "{} 术语表这一行只有术语没有定义（{} 列）", tag(n), tk.len());
            // 定义列**允许内部有空格**（md:266「卡牌的生命值 = 数值」是 4 个 token），所以按"行首是术语、其余全归定义"切；
            // 对照表那张不用这条宽容——它三列每列都是短串， arity 一变就该红（见下面）。
            terms.push((tk[0].to_string(), tk[1..].join(" ")));
            n += 1;
        }
        assert_eq!(terms.len(), 9, "§七 术语表推导到 {} 行，与文档行 263–271 的 9 行不符", terms.len());
        let mut seen: Vec<&str> = terms.iter().map(|(k, _)| k.as_str()).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "§七 术语表有重复术语 ⇒ 两行同名，锚点会指错对象");
        for (k, v) in &terms {
            assert!(!v.is_empty(), "§七 术语「{k}」的定义列是空的");
        }

        let ch = (head + 2..end)
            .find(|&n| at(n).trim() == "项目 特性 技能")
            .expect("§七 找不到「特性 vs 技能」表的列头");
        let mut cmp: Vec<(String, String, String)> = Vec::new();
        let mut n = ch + 1;
        while n <= end && !at(n).trim().is_empty() {
            let tk: Vec<&str> = at(n).split_whitespace().collect();
            assert_eq!(tk.len(), 3, "{} 对照表出现 {} 列的行 ⇒ 表形变了，解析器要跟着改", tag(n), tk.len());
            cmp.push((tk[0].to_string(), tk[1].to_string(), tk[2].to_string()));
            n += 1;
        }
        let cmp_names: Vec<&str> = cmp.iter().map(|(k, _, _)| k.as_str()).collect();
        assert_eq!(cmp_names, vec!["来源", "数量", "作用", "融合", "示例"], "§七 对照表行首名单变了：{cmp_names:?}");

        // —— ①② 字段名逐字节 ＋ 宿主＝措辞 ——
        let def_fields = s7_fields(&model, "pub struct CardDef {");
        let inst_fields = s7_fields(&model, "pub struct CardInst {");
        let is_name_comment = |c: &str| !c.is_empty() && !c.contains('§') && !c.contains("md:");
        let mut named: Vec<(String, String)> = Vec::new(); // (字段名, 宿主)
        let mut plain: Vec<String> = Vec::new(); // 没挂名字注释的字段
        for (host, fs) in [("CardDef", &def_fields), ("CardInst", &inst_fields)] {
            for (name, _, comment) in fs {
                if is_name_comment(comment) {
                    named.push((comment.clone(), host.to_string()));
                } else {
                    plain.push(name.clone());
                }
            }
        }
        let named_comments: Vec<&str> = named.iter().map(|(c, _)| c.as_str()).collect();
        let doc_field_refs: Vec<&str> = doc_fields.iter().map(String::as_str).collect();
        if named_comments != doc_field_refs {
            bad.push(format!(
                "① 字段名不是逐字节对上：文档块 {doc_field_refs:?} ≠ 代码行尾注释 {named_comments:?}"
            ));
        }
        assert_eq!(
            plain,
            vec!["name", "faction", "id", "def", "hp", "flame", "deaths", "upgrades", "seq", "placed_turn", "triggered_turn", "crafted"],
            "① 另一半全消费：没挂文档名字注释的字段名单变了 ⇒ 新字段要么补上文档措辞、要么在这里登记它是引擎内部记账，别让它静默隐身"
        );
        for (idx, (_, host)) in named.iter().enumerate() {
            let doc_line = &doc_fields[idx];
            let fixed = doc_line.contains("自带") || doc_line.contains("固定");
            let random = doc_line.contains("随机附加");
            if fixed && host != "CardDef" {
                bad.push(format!("②「{doc_line}」写着自带／固定，字段却落在 `{host}`（定义层＝`CardDef`）"));
            }
            if random && host != "CardInst" {
                bad.push(format!("②「{doc_line}」写着随机附加，字段却落在 `{host}`（实例层＝`CardInst`）"));
            }
        }

        // —— ③ 数量行 ↔ 类型形状 ——
        let ty_of = |name: &str| -> String {
            for fs in [&def_fields, &inst_fields] {
                for (n, t, _) in fs {
                    if n == name {
                        return t.clone();
                    }
                }
            }
            panic!("③ 字段 `{name}` 在 CardDef／CardInst 里找不到了");
        };
        for (item, tr_cell, sk_cell) in &cmp {
            if item != "数量" {
                continue;
            }
            let digits: String = tr_cell.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits != "1" {
                bad.push(format!("③ 特性列「{tr_cell}」里量不出「恰好 1 个」的措辞 ⇒ 文档换了说法，判据要跟着改"));
            } else if ty_of("tr").contains("Vec") {
                bad.push(format!("③ 文档说特性「{tr_cell}」，而 `tr` 的类型是 `{}`（容器）", ty_of("tr")));
            }
            if !sk_cell.contains('~') {
                bad.push(format!("③ 技能列「{sk_cell}」里没有区间记号『~』⇒ 0~N 的措辞变了，判据要跟着改"));
            } else if !ty_of("skills").starts_with("Vec<") {
                bad.push(format!("③ 文档说技能「{sk_cell}」，而 `skills` 的类型是 `{}`（不是容器）", ty_of("skills")));
            }
        }

        // —— ④ 示例行 ↔ 跨章等值（特性 ↔ TraitKind::label，技能 ↔ §八 表体「技能」列）——
        let trait_labels = s7_labels(&model, "TraitKind");
        let s8_rows = table_body(&lines, "八、技能池", "编号 技能 效果");
        let s8_skills: Vec<String> = s8_rows
            .iter()
            .map(|&n| at(n).split_whitespace().collect::<Vec<&str>>())
            .filter(|tk| tk.len() == 3 && !tk[0].is_empty() && tk[0].bytes().all(|b| b.is_ascii_digit()))
            .map(|tk| tk[1].to_string())
            .collect();
        assert_eq!(s8_skills.len(), 12, "④ §八 技能列读出 {} 行，不是 12 ⇒ §七 的跨章等值失去基准", s8_skills.len());
        for (item, tr_cell, sk_cell) in &cmp {
            if item != "示例" {
                continue;
            }
            let strip = |s: &str| s.trim_matches('“').trim_matches('”').trim_matches('"').to_string();
            let tr_ex = strip(tr_cell);
            let sk_ex = strip(sk_cell);
            if !trait_labels.contains(&tr_ex) {
                bad.push(format!("④ 特性示例「{tr_ex}」不在 `TraitKind::label` 的 {} 个返回值里 ⇒ §七:280 与代码里的特性名单不同步", trait_labels.len()));
            }
            if !s8_skills.contains(&sk_ex) {
                bad.push(format!("④ 技能示例「{sk_ex}」不在 §八 表体「技能」列的 12 个原文里 ⇒ §七:280 与 §八 表体不同步"));
            }
        }

        // —— ⑤ 融合行＋设计意图 ↔ `fuse_cards` 函数体形状 ——
        let body = s7_fn_body(&prog, "pub fn fuse_cards(");
        let intent = at(282);
        for (item, tr_cell, sk_cell) in &cmp {
            if item != "融合" {
                continue;
            }
            if !tr_cell.contains("不参与融合") {
                bad.push(format!("⑤ 特性列「{tr_cell}」不含「不参与融合」⇒ 判据的措辞前提没了"));
            } else if body.contains(".tr") {
                bad.push("⑤ 文档说特性不参与融合，而 `fuse_cards` 函数体里出现了 `.tr`（剔注释后仍出现＝真动了特性）".to_string());
            }
            if !sk_cell.contains("融合的核心") {
                bad.push(format!("⑤ 技能列「{sk_cell}」不含「融合的核心」⇒ 判据的措辞前提没了"));
            } else if !body.contains("skills") {
                bad.push("⑤ 文档说技能是融合的核心，而 `fuse_cards` 函数体里没读 `skills`".to_string());
            }
        }
        assert!(
            intent.contains("融合只融合技能") && intent.contains("不融合特性"),
            "⑤ md:282 设计意图的措辞变了（实测「{intent}」）⇒ 它就不再与 279 行同义，等值层的前提要重写"
        );

        assert!(bad.is_empty(), "{} 处 §七 卡牌模型与代码形状不符：\n{}", bad.len(), bad.join("\n"));
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
        let debited = debt_claimed_lines();
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

    /// §廿三 规则总览速查：53 行里每一行都必须落到**四条认领路**之一——
    /// ① **锚点指回**＝实现了；② **挂债**＝`NOT_IMPLEMENTED` 的主债行或其复述行；
    /// ③ **§廿二 同 key 复述**；④ **否定行**＝文档断言"没有这东西"。
    /// 速查表是全文档最容易假绿的一张：它把别处已经实现的规则重打一遍，"看着没人锚它"常常只是因为正主在另一章。
    /// 但 ③ 不能开成自由裁量：只认**行首 key 逐字相同**，且那一行在 §廿二 自己必须已经落到 ① 或 ②——
    /// 复述的上游若是排除表，就串成"排除表洗白 §廿二 → §廿三 再复述回来"的两级洗白。
    /// ④ 也不是空口：只有**内容列整列是「无」**或**内容列含反问「哪来的」**才判为否定行（判据从文档措辞推导，
    /// 不收裸的「无」字——「无卡→打中线」里的「无」是条件不是断言），并要求该 key 在**生产码面** 0 命中；
    /// 真长出这套东西，本测当场红。排除表（`NOT_A_RULE`）在本章不作数：同一条洗白路是上一帧在 §十二 量出来的
    /// （删锚＋塞排除表，硬化前全量 136 测一路绿）。
    #[test]
    fn every_row_of_section23_quick_reference_is_anchored_debited_or_restates_section22() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let toks = |n: usize| -> Vec<String> {
            at(n).split_whitespace().map(str::to_string).collect()
        };
        let key_of = |n: usize| -> String { toks(n).first().cloned().unwrap_or_default() };
        let content_of = |n: usize| -> String { toks(n).iter().skip(1).cloned().collect::<Vec<_>>().join(" ") };

        let rows = table_body(&lines, "二十三、规则总览速查", "规则 内容");
        assert_eq!(rows.len(), 53, "§廿三 速查表按结构推导应得 53 行，实测 {} 行 ⇒ 文档加了规则条目，或推导口径失效", rows.len());

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        // ③ 的上游：只收**已被锚点或债认领**的 §廿二 行的 key（排除表里的行不作上游，见上面的两级洗白）。
        let s22_claimed_keys: Vec<String> = table_body(&lines, "二十二、边界情况处理", "情况 处理")
            .iter()
            .filter(|&&n| referenced.contains(&(n as u32)) || debited.contains(&n))
            .map(|&n| key_of(n))
            .collect();

        let (mut anchored, mut on_debt, mut restated, mut absent) = (0usize, 0usize, 0usize, 0usize);
        let mut missing: Vec<String> = Vec::new();
        for &n in &rows {
            let content = content_of(n);
            if referenced.contains(&(n as u32)) {
                anchored += 1;
            } else if debited.contains(&n) {
                on_debt += 1;
            } else if s22_claimed_keys.contains(&key_of(n)) {
                restated += 1;
            } else if is_absence_row(&content) {
                assert_eq!(
                    code_occurrences(&key_of(n)),
                    0,
                    "md:{n} 是 §廿三 的否定行（内容列「{content}」断言没有这个东西），但生产码面有 {} 处「{}」⇒ 文档过期，要么删掉这行断言、要么把锚点挂上",
                    code_occurrences(&key_of(n)),
                    key_of(n)
                );
                absent += 1;
            } else {
                missing.push(format!("  md:{n} ← {}", at(n)));
            }
        }
        assert!(
            missing.is_empty(),
            "§廿三 有 {} 行速查规则既无锚点指回、也不在债表里、也不是 §廿二 已落地之行的同 key 复述、内容列也没断言「无」（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );
        // 四条路各自钉死：总数对上只说明"没漏"，对上**分档**才说明没有行从一条路悄悄挪到另一条
        // （把已实现的改成挂债＝把牙拔掉，把该锚定的塞进否定行＝把缺口说成"文档本来就说没有"）。
        assert_eq!(
            (anchored, on_debt, restated, absent),
            (39, 2, 10, 2),
            "§廿三 四条认领路应为 锚点39／挂债2／§廿二复述10／否定2，实测 ({anchored},{on_debt},{restated},{absent})"
        );
        // 成员级反证：每条路各钉一行，路走错了当场红在"哪一行走错"上，而不只是红在一个总数上。
        let claims = |n: usize| -> (&'static str, bool) {
            let content = content_of(n);
            if referenced.contains(&(n as u32)) {
                ("锚点", true)
            } else if debited.contains(&n) {
                ("挂债", true)
            } else if s22_claimed_keys.contains(&key_of(n)) {
                ("§廿二复述", true)
            } else if is_absence_row(&content) {
                ("否定行", true)
            } else {
                ("无人认领", false)
            }
        };
        for (n, want) in [
            (978usize, "锚点"),
            (985, "锚点"),
            (1029, "挂债"),
            (980, "挂债"),
            (1011, "§廿二复述"),
            (995, "§廿二复述"),
            (1028, "否定行"),
            (1030, "否定行"),
        ] {
            let (got, ok) = claims(n);
            assert!(ok && got == want, "md:{n} 走的是「{got}」路，登记的期望是「{want}」⇒ 认领路挪动了（这一行的处置变了）");
        }
        // 上游必须是"落地过的"§廿二 行：970 在排除表里，它的 key 不许成为复述上游。
        assert!(!s22_claimed_keys.iter().any(|k| k == "商业模式"), "§廿二 排除表里的行成了 §廿三 的复述上游＝两级洗白");
    }

    /// §廿一 单机模式：**一张章里有三张子表**（模式表 3 行／每日挑战规则围栏 6 步／Boss 表 5 行＝14 行），
    /// 每一行只许两条认领路——**锚点指回**或**挂债**；排除表在本围栏不作数（同 §十二，那条洗白路是量出来的）。
    /// 必检集由结构推导，判据只有三条：① 围栏内必须逐行匹配 `^数字. `（围栏里冒出不像流程步的行就红）；
    /// ② **紧跟空行的第一非空行是「块首」**，块首里单 token 的是小节头（`每日挑战规则`／`Boss设计`）、
    /// 多 token 的是列头（`模式 说明`／`章节 Boss 特征`）；③ 其余非空行进必检集。
    /// 三条判据谁都不许偷偷吞行：列头与小节头两个集合按**行号逐个钉死**，必检集钉 14，成员级反证再从
    /// 三张子表各钉一行——把「回滚次数」这类真规则误当块首吃掉、或把小节头当规则行放进来，都当场红在名字上。
    #[test]
    fn every_row_of_section21_single_player_mode_is_anchored_back_or_debited() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "二十一、单机模式")
            .expect("§廿一 标题必须存在（文档结构变了就要同步改本检查）");

        let mut rows: Vec<usize> = Vec::new();
        let mut headers: Vec<usize> = Vec::new();
        let mut headings: Vec<usize> = Vec::new();
        let mut fence = false;
        let mut block_start = false;
        for (off, l) in lines[head + 1..].iter().enumerate() {
            let n = head + 2 + off;
            let t = l.trim();
            if !fence && t == "---" {
                break;
            }
            if t == "```" {
                fence = !fence;
                block_start = false;
                continue;
            }
            if t.is_empty() {
                block_start = true;
                continue;
            }
            if fence {
                let numbered = t.chars().next().is_some_and(|c| c.is_ascii_digit());
                assert!(
                    numbered && t[1..].starts_with('.'),
                    "md:{n} 落在 §廿一 的围栏里却不是编号步（「{t}」）⇒ 推导口径失效，请同步改本检查而不是放宽它"
                );
                rows.push(n);
                block_start = false;
                continue;
            }
            if block_start {
                if t.split_whitespace().count() == 1 {
                    headings.push(n);
                } else {
                    headers.push(n);
                }
            } else {
                rows.push(n);
            }
            block_start = false;
        }
        assert_eq!(rows.len(), 14, "§廿一 三张子表按结构推导应得 14 行，实测 {} 行 ⇒ 文档加了模式/规则/Boss 条目，或推导口径失效", rows.len());
        assert_eq!(headers, vec![913, 931], "§廿一 的列头应恰好是 913「模式 说明」与 931「章节 Boss 特征」，实测 {headers:?}");
        assert_eq!(headings, vec![918, 929], "§廿一 的小节头应恰好是 918「每日挑战规则」与 929「Boss设计」，实测 {headings:?}");
        // 成员级反证：三张子表各钉一行必须在必检集里，两个块首必须不在——只钉总数会让"吞掉一行、凭空加一行"蒙混过关。
        for n in [914usize, 916, 921, 924, 926, 932, 936] {
            assert!(rows.contains(&n), "md:{n} 被剔出 §廿一 必检集 ⇒ 推导器漏了这种形态（{}）", at(n));
        }
        for n in [913usize, 918, 929, 931] {
            assert!(!rows.contains(&n), "md:{n}（{}）进了必检集 ⇒ 块首判据失效", at(n));
        }

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        // 排除表在本围栏不作数（同 §十二 的裁定26 修法）。
        for (n, why) in NOT_A_RULE {
            assert!(
                !rows.contains(n),
                "md:{n} 落在 §廿一 围栏内却被 `NOT_A_RULE` 认领＝排除表能吞掉真规则。要说它不算规则，请挂债并写去处；登记的排除理由：{why}"
            );
        }
        let (mut anchored, mut on_debt) = (0usize, 0usize);
        let mut missing: Vec<String> = Vec::new();
        for &n in &rows {
            if referenced.contains(&(n as u32)) {
                anchored += 1;
            } else if debited.contains(&n) {
                on_debt += 1;
            } else {
                missing.push(format!("  md:{n} ← {}", at(n)));
            }
        }
        assert!(
            missing.is_empty(),
            "§廿一 有 {} 行既无锚点指回、也不在债表里（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );
        assert_eq!(
            (anchored, on_debt),
            (11, 3),
            "§廿一 两条认领路应为 锚点11／挂债3，实测 ({anchored},{on_debt}) ⇒ 有行从一条路悄悄挪到另一条（把已实现的改成挂债＝拔掉牙）"
        );
        // 逐档再钉成员：路走错了要红在"哪一行走错"上，而不只是红在一个总数上。
        for n in [914usize, 916, 921, 924, 925, 926, 932, 936] {
            assert!(referenced.contains(&(n as u32)), "md:{n}（{}）登记的处置是「锚点指回」，现在没有锚点＝债已偿或锚被删", at(n));
        }
        for n in [915usize, 922, 923] {
            assert!(debited.contains(&n), "md:{n}（{}）登记的处置是「挂债」，现在不在债表里＝这条账被删了或换了对象", at(n));
        }
    }

    /// §十五 伤害回滚：**围栏式散文章，不是表**，所以形态判据换一套（三条）：
    /// ① 章内 trim 后**单 token 且以「：」收尾**的行是标签——含围栏外的 606/615/633/641/662 与**围栏内**的 636「触发后：」，
    ///    标签不入必检集；② 其余非空行（围栏内外都算）全入必检集，实测 34 行；
    /// ③ 「示例」标签后那道围栏里的行**只许走第三条认领路：用文档自己的数字驱动引擎跑一遍**
    ///    （`battle::parse_section15_examples` 解析 ＋ `section15_worked_examples_reproduce_on_the_engine` 实测）。
    /// 为什么示例不吃挂锚：644–659 是 618–630 那批规则的重说一遍，锚点只能证明"有代码行指过来"，
    /// 证明不了"引擎算出的数＝文档写的那串数"；实测才是两头都拦——改文档示例的数会红，改引擎的算法也会红。
    /// 为什么示例也不许同时走锚点／挂债：那等于把"必须跑一遍"降级成"有人指过来就行"。
    /// 排除表（`NOT_A_RULE`）在本章同样不作数（同 §十二／§廿一 的裁定26 修法）。
    #[test]
    fn every_row_of_section15_damage_rollback_is_anchored_debited_or_reproduced_by_the_engine() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "十五、伤害回滚（仅玩家拥有）")
            .expect("§十五 标题必须存在（文档结构变了就要同步改本检查）");

        let mut rows: Vec<usize> = Vec::new();
        let mut labels: Vec<usize> = Vec::new();
        let mut fence = false;
        // 标签的两条形态判据（各自只在一侧生效，合起来才覆盖本章两种标签写法）：
        // ① 围栏**内**：单 token 且以「：」收尾 —— 636「触发后：」；
        // ② 围栏**外**：单 token 且下一个非空行就是 ``` —— 606/615/633/641/662 这种不带冒号的块首标签。
        // ② 只在围栏外生效是必须的：示例最后一行 659「→ 下回合业力-1」也是单 token，而它的下一个非空行
        // 恰好是**闭**围栏 ⇒ 若 ② 不分内外就会把这条真示例行吞成标签，而吞掉一行正好能让总数从 39 落回 34，
        // 只钉总数拦不住（本帧先实测到 39≠34，才把这条危害写成实证的）。
        let next_non_blank = |n: usize| -> String {
            let mut k = n + 1;
            while k <= lines.len() && at(k).trim().is_empty() {
                k += 1;
            }
            at(k).trim().to_string()
        };
        for n in (head + 2)..=lines.len() {
            let t = at(n).trim();
            if !fence && t == "---" {
                break;
            }
            if t == "```" {
                fence = !fence;
                continue;
            }
            if t.is_empty() {
                continue;
            }
            let one_word = t.split_whitespace().count() == 1;
            let is_label = one_word && ((fence && t.ends_with('：')) || (!fence && next_non_blank(n) == "```"));
            if is_label {
                labels.push(n);
                continue;
            }
            rows.push(n);
        }
        assert_eq!(rows.len(), 34, "§十五 按形态推导应得 34 行（触发条件4＋效果12＋回滚代价2＋示例14＋视觉2），实测 {} 行 ⇒ 文档加了规则或推导口径失效", rows.len());
        assert_eq!(
            labels,
            vec![606, 615, 633, 636, 641, 662],
            "§十五 的标签应恰好是 606 触发条件／615 效果／633 回滚代价／636 触发后：／641 示例／662 视觉，实测 {labels:?} ⇒ 有真规则行被当成标签吞掉，或标签判据失效"
        );
        // 成员级反证：各形态各钉一行必须在必检集里（只钉总数会让"吞一行、加一行"蒙混过关）。
        for n in [609usize, 612, 618, 622, 628, 630, 637, 638, 644, 657, 665] {
            assert!(rows.contains(&n), "md:{n} 被剔出 §十五 必检集 ⇒ 推导器漏了这种形态（「{}」）", at(n));
        }
        for n in [604usize, 606, 633, 636, 641, 662] {
            assert!(!rows.contains(&n), "md:{n}（「{}」）进了必检集 ⇒ 标签／章标题判据失效", at(n));
        }

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        for (n, why) in NOT_A_RULE {
            assert!(
                !rows.contains(n),
                "md:{n} 落在 §十五 内却被 `NOT_A_RULE` 认领＝排除表能吞掉真规则。要说它不算规则，请挂债并写去处；登记的排除理由：{why}"
            );
        }
        // 第三条路：示例行必须正好是那个解析器认下来的行，两套口径不许分叉。
        let verified: Vec<usize> = crate::battle::parse_section15_examples(&lines).iter().map(|c| c.line).collect();
        assert_eq!(verified.len(), 14, "§十五 示例围栏应解析出 14 行，实测 {} 行 ⇒ 解析器与本推导器的口径分叉了", verified.len());
        for n in &verified {
            assert!(rows.contains(n), "md:{n} 被示例解析器收了却不在必检集 ⇒ 推导器与解析器对同一段围栏读法不同");
            assert!(
                !referenced.contains(&(*n as u32)) && !debited.contains(n),
                "md:{n}（「{}」）是 §十五 示例行，却走了锚点／挂债路＝把「必须跑一遍」降级成「有人指过来就行」",
                at(*n)
            );
        }

        let (mut anchored, mut on_debt, mut reproduced) = (0usize, 0usize, 0usize);
        let mut missing: Vec<String> = Vec::new();
        for &n in &rows {
            if verified.contains(&n) {
                reproduced += 1;
            } else if referenced.contains(&(n as u32)) {
                anchored += 1;
            } else if debited.contains(&n) {
                on_debt += 1;
            } else {
                missing.push(format!("  md:{n} ← {}", at(n)));
            }
        }
        assert!(
            missing.is_empty(),
            "§十五 有 {} 行既无锚点指回、也不在债表里、又不是示例围栏里被引擎实测复现的行（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );
        assert_eq!(
            (anchored, on_debt, reproduced),
            (18, 2, 14),
            "§十五 三条认领路应为 锚点18／挂债2／示例实测14，实测 ({anchored},{on_debt},{reproduced}) ⇒ 有行从一条路悄悄挪到另一条"
        );
        for n in [609usize, 610, 612, 618, 622, 625, 628, 630, 637, 638] {
            assert!(referenced.contains(&(n as u32)), "md:{n}（「{}」）登记的处置是「锚点指回」，现在没有锚点＝实现被删或锚被摘", at(n));
        }
        for n in [665usize, 666] {
            assert!(debited.contains(&n), "md:{n}（「{}」）登记的处置是「挂债」，现在不在债表里＝这条账被删了或换了对象", at(n));
        }
        // §十四 帧补的尺：实测路那 14 行靠这条复现测真的在跑，删掉它行数照对、却没人再跑（见 engine_repro_test_exists）。
        engine_repro_test_exists("section15_worked_examples_reproduce_on_the_engine");
    }

    /// §十七 挤压与推进——**第二张非表的章**，也是第一条"整章都是围栏流程步"的章（没有示例数字，
    /// 所以 §十五 那第三条认领路在这里开不出来：只能走锚点／挂债两条）。
    /// 标签判据在本章多出一档：761「中线绝对规则」是围栏外的块首，但它引的不是 ``` 而是一段「·」散列行，
    /// 判据 ② 认不下它。第三档只放宽**围栏外**（围栏内仍是「：」收尾那一档），且靠成员级双向钉拦住
    /// "把真规则行当标签吞掉"——与 §十五 同一套防"吞一行、加一行"的做法。
    #[test]
    fn every_row_of_section17_squeeze_and_advance_is_anchored_back_or_debited() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "十七、挤压与推进")
            .expect("§十七 标题必须存在（文档结构变了就要同步改本检查）");

        let mut rows: Vec<usize> = Vec::new();
        let mut labels: Vec<usize> = Vec::new();
        let mut fence = false;
        let next_non_blank = |n: usize| -> String {
            let mut k = n + 1;
            while k <= lines.len() && at(k).trim().is_empty() {
                k += 1;
            }
            at(k).trim().to_string()
        };
        for n in (head + 2)..=lines.len() {
            let t = at(n).trim();
            if !fence && t == "---" {
                break;
            }
            if t == "```" {
                fence = !fence;
                continue;
            }
            if t.is_empty() {
                continue;
            }
            let one_word = t.split_whitespace().count() == 1;
            // 三档标签形态：① 围栏内单 token 且「：」收尾；② 围栏外单 token 且下一非空行是 ```；
            // ③ 围栏外单 token 且下一非空行以「·」开头（本章独有的散列式块）。
            let is_label = one_word
                && ((fence && t.ends_with('：'))
                    || (!fence && (next_non_blank(n) == "```" || next_non_blank(n).starts_with('·'))));
            if is_label {
                labels.push(n);
                continue;
            }
            rows.push(n);
        }
        assert_eq!(
            rows.len(),
            19,
            "§十七 按形态推导应得 19 行（我方挤压2＋敌方推进10＋对称规则4＋中线绝对规则3），实测 {} 行 ⇒ 文档加了规则或推导口径失效",
            rows.len()
        );
        assert_eq!(
            labels,
            vec![728, 731, 736, 739, 752, 761],
            "§十七 的标签应恰好是 728 我方挤压／731 放置新卡到P格：／736 敌方挤压／739 敌方回合结束推进：／752 双方规则对称／761 中线绝对规则，实测 {labels:?} ⇒ 有真规则行被当成标签吞掉，或标签判据失效"
        );
        // 成员级双向钉：四个块各钉一行必须在必检集里，三档标签各钉一行必须不在。
        for n in [732usize, 733, 742, 746, 749, 755, 758, 763, 765] {
            assert!(rows.contains(&n), "md:{n} 被剔出 §十七 必检集 ⇒ 推导器漏了这种形态（「{}」）", at(n));
        }
        for n in [728usize, 731, 739, 761] {
            assert!(!rows.contains(&n), "md:{n}（「{}」）进了必检集 ⇒ 标签判据失效", at(n));
        }

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        for (n, why) in NOT_A_RULE {
            assert!(
                !rows.contains(n),
                "md:{n} 落在 §十七 内却被 `NOT_A_RULE` 认领＝排除表能吞掉真规则。要说它不算规则，请挂债并写去处；登记的排除理由：{why}"
            );
        }

        let (mut anchored, mut on_debt) = (0usize, 0usize);
        let mut missing: Vec<String> = Vec::new();
        for &n in &rows {
            if referenced.contains(&(n as u32)) {
                anchored += 1;
            } else if debited.contains(&n) {
                on_debt += 1;
            } else {
                missing.push(format!("  md:{n} ← {}", at(n)));
            }
        }
        assert!(
            missing.is_empty(),
            "§十七 有 {} 行既无锚点指回、也不在债表里（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );
        assert_eq!(
            (anchored, on_debt),
            (19, 0),
            "§十七 认领路应为 锚点19／挂债0（本章没有示例围栏，开不出第三条实测路），实测 ({anchored},{on_debt}) ⇒ 有行从一条路悄悄挪到另一条，或实现被删"
        );
        // 逐行钉处置：这 10 行登记的处置是「锚点指回」，摘掉任何一条锚点都要当场红。
        for n in [732usize, 733, 740, 742, 743, 746, 748, 756, 758, 764, 765] {
            assert!(referenced.contains(&(n as u32)), "md:{n}（「{}」）登记的处置是「锚点指回」，现在没有锚点＝实现被删或锚被摘", at(n));
        }
        // 否定式那两行（763「不能越过中线」／747「多张只推最靠近的」）只锚不测，盲区照 §十五:621 登记；
        // 763 的可测那半已由 `a_squeezed_card_dies_at_its_own_line_and_never_reaches_the_other_side` 钉住。
        for n in [747usize, 763] {
            assert!(
                referenced.contains(&(n as u32)),
                "md:{n}（「{}」）是否定式条款，本轮登记的处置是锚点指回；改成没锚＝把这条盲区抹进静默里",
                at(n)
            );
        }
    }

    /// §十六 业火条——**第一条同时走满三条认领路的章**：基本规则表走锚点、「触发示例」围栏走引擎复现、
    /// 外观与色档走挂债。形态判据在本章多出两档，都是被这张章的长相逼出来的：
    /// ④ 围栏外单 token 且下一非空行是**等元数表体**的表头 ⇒ 它是块首（673「基本规则」）——
    ///    本章的表不跟围栏而跟散行，§十七 那三档在这里认不下它；
    /// ⑤ 制表符框线不算规则（696/699 只有 `┌──┐`/`└──┘`，说的是"有个框"而不是可核对的行为），
    ///    但同一道围栏里**带文字的注释行照算**（697「← 业火值：5/6」、698「← 进度条」）。
    /// 列头（675「项目 说明」／704「状态 表现」）走**双条件**：既要满足结构（同元数 run 的首行），
    /// 也要字面命中这两串。少任何一边都不排除 ⇒ 文档改了列头措辞、或加了一张没有列头的表，
    /// 那一行都会落进必检集当场红（失败方向是响的，不是静默吞行）。
    #[test]
    fn every_row_of_section16_karma_bar_is_anchored_debited_or_reproduced_by_the_engine() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "十六、业火条")
            .expect("§十六 标题必须存在（文档结构变了就要同步改本检查）");
        let toks = |n: usize| -> usize { at(n).split_whitespace().count() };
        let next_non_blank = |n: usize| -> usize {
            let mut k = n + 1;
            while k <= lines.len() && at(k).trim().is_empty() {
                k += 1;
            }
            k
        };
        let arity = |n: usize| -> usize { toks(n) };
        // 从 n 起的"等元数（≥2）连续非空 run"长度——表体识别只看得出形状，不认字面。
        let run_len = |n: usize| -> usize {
            let c = arity(n);
            if c < 2 || at(n).trim().is_empty() {
                return 0;
            }
            let mut k = n;
            let mut cnt = 0usize;
            while k <= lines.len() && !at(k).trim().is_empty() && arity(k) == c {
                cnt += 1;
                k += 1;
            }
            cnt
        };
        // 框线判据：抹掉制表符与空白后什么都不剩。
        let is_frame = |t: &str| -> bool {
            !t.is_empty()
                && t.chars().all(|c| {
                    c.is_whitespace()
                        || matches!(
                            c,
                            '┌' | '┐' | '└' | '┘' | '├' | '┤' | '─' | '│' | '━' | '┃' | '┏' | '┓' | '┗' | '┛' | '╞' | '╡' | '╪'
                        )
                })
        };

        let mut rows: Vec<usize> = Vec::new();
        let mut labels: Vec<usize> = Vec::new();
        let mut headers: Vec<usize> = Vec::new();
        let mut frames: Vec<usize> = Vec::new();
        let mut fence = false;
        for n in (head + 2)..=lines.len() {
            let t = at(n).trim();
            if !fence && t == "---" {
                break;
            }
            if t == "```" {
                fence = !fence;
                continue;
            }
            if t.is_empty() {
                continue;
            }
            let nb = next_non_blank(n);
            let one_word = toks(n) == 1;
            let is_label = one_word
                && ((fence && t.ends_with('：'))
                    || (!fence
                        && (at(nb).trim() == "```"
                            || at(nb).trim().starts_with('·')
                            || run_len(nb) >= 3)));
            if is_label {
                labels.push(n);
                continue;
            }
            let first_of_run = {
                let p = n - 1;
                p < head + 2 || at(p).trim().is_empty() || arity(p) != arity(n)
            };
            if !fence && first_of_run && run_len(n) >= 3 && (t == "项目 说明" || t == "状态 表现") {
                headers.push(n);
                continue;
            }
            if is_frame(t) {
                frames.push(n);
                continue;
            }
            rows.push(n);
        }
        assert_eq!(
            rows.len(),
            25,
            "§十六 按形态推导应得 25 行（基本规则7＋示例3＋外观图注2＋百分比1＋色档5＋爆发步骤7），实测 {} 行 ⇒ 文档加了规则或推导口径失效",
            rows.len()
        );
        assert_eq!(
            labels,
            vec![673, 684, 692, 695, 711, 714],
            "§十六 的标签应恰好是 673 基本规则／684 触发示例／692 业火条外观／695 卡牌下方：／711 业火爆发外观／714 触发时：，实测 {labels:?} ⇒ 有真规则行被当成标签吞掉，或标签判据失效"
        );
        assert_eq!(
            headers,
            vec![675, 704],
            "§十六 的列头应恰好是 675「项目 说明」与 704「状态 表现」，实测 {headers:?} ⇒ 列头判据失效（结构＋字面两个条件缺一不可）"
        );
        assert_eq!(
            frames,
            vec![696, 699],
            "§十六 的纯框线应恰好是 696/699，实测 {frames:?} ⇒ 框线判据吞掉了带文字的图注行（697/698 必须算规则）"
        );
        // 成员级双向钉：五种形态各钉一行必须在必检集，各类排除行各钉一行必须不在。
        for n in [676usize, 679, 687, 697, 698, 702, 705, 709, 715, 720, 721] {
            assert!(rows.contains(&n), "md:{n} 被剔出 §十六 必检集 ⇒ 推导器漏了这种形态（「{}」）", at(n));
        }
        for n in [673usize, 675, 692, 695, 696, 699, 704, 714] {
            assert!(!rows.contains(&n), "md:{n}（「{}」）进了必检集 ⇒ 标签／列头／框线判据失效", at(n));
        }

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        for (n, why) in NOT_A_RULE {
            assert!(
                !rows.contains(n),
                "md:{n} 落在 §十六 内却被 `NOT_A_RULE` 认领＝排除表能吞掉真规则。要说它不算规则，请挂债并写去处；登记的排除理由：{why}"
            );
        }
        // 第三条路：示例行必须正好是那个解析器认下来的行，两套口径不许分叉。
        let verified: Vec<usize> = crate::battle::parse_section16_examples(&lines).iter().map(|c| c.line).collect();
        assert_eq!(
            verified.len(),
            3,
            "§十六 示例围栏应解析出 3 行，实测 {} 行 ⇒ 解析器与本推导器的口径分叉了",
            verified.len()
        );
        for n in &verified {
            assert!(rows.contains(n), "md:{n} 被示例解析器收了却不在必检集 ⇒ 推导器与解析器对同一段围栏读法不同");
            assert!(
                !referenced.contains(&(*n as u32)) && !debited.contains(n),
                "md:{n}（「{}」）是 §十六 示例行，却走了锚点／挂债路＝把「必须跑一遍」降级成「有人指过来就行」",
                at(*n)
            );
        }

        let (mut anchored, mut on_debt, mut reproduced) = (0usize, 0usize, 0usize);
        let mut missing: Vec<String> = Vec::new();
        for &n in &rows {
            if verified.contains(&n) {
                reproduced += 1;
            } else if referenced.contains(&(n as u32)) {
                anchored += 1;
            } else if debited.contains(&n) {
                on_debt += 1;
            } else {
                missing.push(format!("  md:{n} ← {}", at(n)));
            }
        }
        assert!(
            missing.is_empty(),
            "§十六 有 {} 行既无锚点指回、也不在债表里、又不是示例围栏里被引擎实测复现的行（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );
        assert_eq!(
            (anchored, on_debt, reproduced),
            (9, 13, 3),
            "§十六 三条认领路应为 锚点9／挂债13／示例实测3，实测 ({anchored},{on_debt},{reproduced}) ⇒ 有行从一条路悄悄挪到另一条",
        );
        for n in [676usize, 677, 678, 679, 680, 681, 682, 719, 720] {
            assert!(referenced.contains(&(n as u32)), "md:{n}（「{}」）登记的处置是「锚点指回」，现在没有锚点＝实现被删或锚被摘", at(n));
        }
        for n in [697usize, 698, 702, 705, 706, 707, 708, 709, 715, 716, 717, 718, 721] {
            assert!(debited.contains(&n), "md:{n}（「{}」）登记的处置是「挂债」，现在不在债表里＝这条账被删了或换了对象", at(n));
        }
        // 同上：§十六 那 3 行示例实测也要求复现测还在（§十四 帧补的尺，三章共用）。
        engine_repro_test_exists("section16_worked_examples_reproduce_on_the_engine");
    }

    /// §十四 持业者·蜡烛反向覆盖：12 条必检行，**本仓第一次在同一章并用两条路、且让九行双记账**。
    ///
    /// 形状：这一章同时有一张三行小表（582–585）与一段围栏（589–600），所以 §十五/§十六 那套"标签／列头"
    /// 形状路和 §十六 的"文档数字驱动引擎"实测路在同一章都用上。围栏内的行**一律由解析器给**
    /// （`battle::parse_section14_candle_rules`），推导器不在围栏里再判一次标签——否则同一串字两套口径，
    /// 会朝不同方向错（同 §十五 那条"两处各写一遍"的理由）。顺带一个后果：本章的「我方持业者：／敌方持业者：」
    /// 在 §十五 会被 ① 档判成**标签**吞掉，这里它们是规则行（点名哪一侧）——所以 walk 一进围栏就放弃标签判据。
    ///
    /// **双记账**（本帧新东西）：围栏那 9 行既有锚点指回、又被引擎实测。§十五/§十六 明令示例行**不许**走锚点路，
    /// 怕的是"用锚点顶掉实测"；这里顶不掉——实测路的行集合由解析器强制（少一行当场红），锚点只是**额外**一层。
    /// 两层的牙不一样，这才是双写的理由：
    /// 摘掉 md:592 的锚 ⇒ 只有"12 行全有锚"那条红（分区计数仍是 锚点3／挂债0／实测9，看不见）；
    /// 把 md:592 的减短量改成 2 ⇒ 只有引擎实测那条红（行数没变，形状尺量不到数字）。
    ///
    /// 五层等值，每层各钉一件事：
    /// ① 认领路：12 行全有锚点、挂债 0；分区计数 (锚点3／挂债0／实测9)。
    /// ② md:583「正常燃烧…**不减短**」＝否定式条款 → 蜡烛写点必须是那份**封闭名单**（见 `s14_candle_writes`）。
    /// ③ md:591／md:596 两处「初始长度20单位」→ `CANDLE_HP` 必须等于文档那个数，且两侧构造期同读这一个常量。
    /// ④ 数字→引擎：`section14_candle_numbers_reproduce_on_the_engine` 必须真的还在（`engine_repro_test_exists`）。
    /// ⑤ md:599「视觉与玩家持业者对称」→ 棋盘上两侧共用同一个 `candle_bar`：1 处定义＋2 处调用，多一处＝另起炉灶。
    ///
    /// 边界如实登记（不由本章声称覆盖）：Boss 双烛（`e_candle2`／`both_holders_out`，见本文件裁定19）与
    /// Boss profile 覆写初始 20，都超出 §十四 的措辞——文档 md:598 只写单烛判死。这两处由 ② 的名单
    /// （列出了 `e_candle2` 的写点）与 `battle.rs`／`boss.rs` 的 Boss 测各自守着，不在本章配比里主张"已覆盖"。
    #[test]
    fn every_row_of_section14_holder_candle_is_anchored_back_and_its_numbers_are_reproduced() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "十四、持业者 · 蜡烛")
            .expect("§十四 标题必须存在（文档结构变了就要同步改本检查）");
        let arity = |n: usize| -> usize { at(n).split_whitespace().count() };
        let next_non_blank = |n: usize| -> usize {
            let mut k = n + 1;
            while k <= lines.len() && at(k).trim().is_empty() {
                k += 1;
            }
            k
        };
        // 从 n 起「连续非空、且不是 `---`／``` 」的 run 长度。
        // **这里故意不用 §十六/§十五 的「等元数」run**：本章表体三列，可空格切出来的 token 数是 4/4/3
        // （`❌ 不减短` 占了两段），等元数判据会把这张表的列头与表体切成两个 run，从 582 起只数到 1 行，
        // 于是 580 与 582 都不再被认成标签／列头，580 反倒伪装成必检行（本帧实测到的：先红在 labels 那条）。
        // 改判「连续非空 run ≥ 2」＋字面命中词表，结构条件照样在（词表吃不进的新列头仍旧掉进必检行），
        // 只是不再假设列与列之间空格数一致。
        let block_len = |n: usize| -> usize {
            let mut k = n;
            let mut cnt = 0usize;
            while k <= lines.len() && !at(k).trim().is_empty() && at(k).trim() != "---" && at(k).trim() != "```" {
                cnt += 1;
                k += 1;
            }
            cnt
        };

        // 围栏外的形状路（标签／列头）＋围栏内的解析器路，两条拼成必检集。
        const S14_HEADERS: [&str; 1] = ["状态 表现 是否减短"];
        let mut rows: Vec<usize> = Vec::new();
        let mut labels: Vec<usize> = Vec::new();
        let mut headers: Vec<usize> = Vec::new();
        let mut fence_ticks = 0usize;
        let mut fence = false;
        for n in (head + 2)..=lines.len() {
            let t = at(n).trim();
            if !fence && t == "---" {
                break;
            }
            if t == "```" {
                fence = !fence;
                fence_ticks += 1;
                continue;
            }
            if t.is_empty() || fence {
                continue;
            }
            let nb = next_non_blank(n);
            let nb_text = at(nb).trim();
            if arity(n) == 1 && (nb_text == "```" || S14_HEADERS.contains(&nb_text)) {
                labels.push(n);
                continue;
            }
            if S14_HEADERS.contains(&t) {
                assert!(block_len(n) >= 2, "md:{n}「{t}」在列头词表里却不在任何表格块的开头 ⇒ 文档改了表形，词表得跟着改（结构与字面两个方向都要对得上）");
                headers.push(n);
                continue;
            }
            rows.push(n);
        }
        assert_eq!(fence_ticks, 2, "§十四 应只有一道 ``` 围栏（蜡烛视觉），实测 {fence_ticks} 个围栏符 ⇒ 文档加了第二段围栏，而解析器只读第一段，那里的新行会从两把尺外面一起漏过去");
        assert!(!fence, "§十四 的围栏没有闭合（数到奇数个 ```）");
        assert_eq!(
            labels,
            vec![580, 587],
            "§十四 的标签应恰好是 580 蜡烛机制／587 蜡烛视觉，实测 {labels:?} ⇒ 有真规则行被当成标签吞掉，或标签判据失效"
        );
        assert_eq!(headers, vec![582], "§十四 的列头应恰好是 582「状态 表现 是否减短」，实测 {headers:?} ⇒ 列头判据失效（结构＋字面两个条件缺一不可）");
        assert_eq!(rows, vec![583, 584, 585], "§十四 围栏外应得表体 583/584/585 三行，实测 {rows:?}");

        let parsed = crate::battle::parse_section14_candle_rules(&lines);
        let verified: Vec<usize> = parsed.iter().map(|c| c.line).collect();
        assert_eq!(
            verified,
            vec![590, 591, 592, 593, 595, 596, 597, 598, 599],
            "§十四 围栏行的名单由解析器给，应为 590–593＋595–599，实测 {verified:?} ⇒ 解析器与本推导器对同一段围栏读法不同"
        );
        rows.extend(verified.iter().copied());
        rows.sort_unstable();
        assert_eq!(
            rows.len(),
            12,
            "§十四 必检行应恰好 12 行（表体3＋围栏9），实测 {} 行 ⇒ 文档加了规则或推导口径失效",
            rows.len()
        );
        // 成员级双向钉：两种形态各钉一行必须在必检集，各类排除行各钉一行必须不在。
        for n in [583usize, 585, 591, 593, 599] {
            assert!(rows.contains(&n), "md:{n} 被剔出 §十四 必检集 ⇒ 推导器漏了这种形态（「{}」）", at(n));
        }
        for n in [578usize, 580, 582, 587, 589, 600, 602] {
            assert!(!rows.contains(&n), "md:{n}（「{}」）进了必检集 ⇒ 章标题／标签／列头／围栏符判据失效", at(n));
        }

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        for (n, why) in NOT_A_RULE {
            assert!(
                !rows.contains(n),
                "md:{n} 落在 §十四 内却被 `NOT_A_RULE` 认领＝排除表能吞掉真规则。要说它不算规则，请挂债并写去处；登记的排除理由：{why}"
            );
        }

        let (mut anchored, mut on_debt, mut reproduced) = (0usize, 0usize, 0usize);
        let mut missing: Vec<String> = Vec::new();
        for &n in &rows {
            if verified.contains(&n) {
                reproduced += 1;
            } else if referenced.contains(&(n as u32)) {
                anchored += 1;
            } else if debited.contains(&n) {
                on_debt += 1;
            } else {
                missing.push(format!("  md:{n} ← {}", at(n)));
            }
        }
        assert!(
            missing.is_empty(),
            "§十四 有 {} 行既无锚点指回、也不在债表里、又不是围栏里被引擎实测复现的行（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );
        assert_eq!(
            (anchored, on_debt, reproduced),
            (3, 0, 9),
            "§十四 三条认领路的分区计数应为 锚点3／挂债0／实测9（围栏行按实测优先计），实测 ({anchored},{on_debt},{reproduced}) ⇒ 有行从一条路悄悄挪到另一条"
        );
        for n in [583usize, 584, 585] {
            assert!(referenced.contains(&(n as u32)), "md:{n}（「{}」）是表体行，登记的处置是「锚点指回」，现在没有锚点＝实现被删或锚被摘", at(n));
        }
        // 双记账的另一层：本章纪律是 **12 行全有锚**，实测路不豁免（分区计数看不见这一层，摘掉围栏行的锚它照样绿）。
        for &n in &rows {
            assert!(referenced.contains(&(n as u32)), "md:{n}（「{}」）没有锚点指回 ⇒ §十四 的纪律是 12 行**全**指回，走了实测路也不豁免锚点", at(n));
        }
        assert!(
            rows.iter().all(|n| !debited.contains(n)),
            "§十四 不该有挂债行：本章 12 行全部已实现并已指回，任何一行躺进债表都说明实现被撤"
        );

        // ② 否定式条款的形状：不减短＝写点名单里没有"回合推进"这一类。名单是封闭集，逐条点名。
        let writes = s14_candle_writes();
        assert_eq!(
            writes,
            vec![
                "b.e_candle = p.holder_hp;",
                "b.e_candle2 = p.holder_hp2;",
                "e_candle2: None,",
                "e_candle: CANDLE_HP,",
                "p_candle: CANDLE_HP,",
                "self.e_candle -= dmg;",
                "self.e_candle -= if hit_first { dmg } else { mirror };",
                "self.e_candle2 = Some(c2 - if hit_first { mirror } else { dmg });",
                "self.p_candle -= d;",
                "self.p_candle -= remain;",
            ],
            "§十四:583「正常燃烧…**不减短」**靠这份**封闭的**蜡烛写点名单兑现：构造 3 ＋ Boss 档案覆写 2 ＋ 伤害 5。多一条＝文档没写过的东西在减蜡烛（例如「每回合自动衰减」），少一条＝已登记的减短路被摘 ⇒ 名单实测 {} 条：{writes:?}",
            writes.len()
        );

        // ③ 文档给的那个数，必须就是引擎用的那个常量，而且两侧同读它。
        let mut inits: Vec<i32> = Vec::new();
        let mut per_hits: Vec<(i32, i32)> = Vec::new();
        let mut deaths: Vec<i32> = Vec::new();
        let mut symmetries = 0usize;
        for c in &parsed {
            match c.claim {
                crate::battle::S14Claim::Init(v) => inits.push(v),
                crate::battle::S14Claim::PerHit { dmg, shrink } => per_hits.push((dmg, shrink)),
                crate::battle::S14Claim::DeathAt(v) => deaths.push(v),
                crate::battle::S14Claim::Symmetry => symmetries += 1,
                crate::battle::S14Claim::Side(_) => {}
            }
        }
        assert_eq!((inits.len(), per_hits.len(), deaths.len(), symmetries), (2, 2, 2, 1), "§十四 围栏应给两侧各一行初始长度／每受伤减短／判死上界，加一行对称，实测 初始{}／每受{}／判死{}／对称{}", inits.len(), per_hits.len(), deaths.len(), symmetries);
        assert!(inits.iter().all(|v| *v == inits[0]), "§十四 两侧写的初始长度不一致 {inits:?} ⇒ md:599「对称」在数字层面已经不成立");
        assert_eq!(crate::battle::CANDLE_HP, inits[0], "引擎常量 CANDLE_HP＝{} ≠ 文档写的初始长度 {}（md:591／md:596 两处）⇒ 改任何一侧都要撞这里，挂锚证不了\"数相等\"", crate::battle::CANDLE_HP, inits[0]);
        assert!(
            writes.contains(&"p_candle: CANDLE_HP,".to_string()) && writes.contains(&"e_candle: CANDLE_HP,".to_string()),
            "md:591／md:596 两侧「初始长度」必须由**同一个**常量在构造期写入；写点名单里缺一侧＝那一侧改成了别的来源（字面量或档案），md:599 的对称破了"
        );

        // ④ 那些数字真的被引擎跑过——不是"有个测试文件提到过"，而是那条复现测还在。
        engine_repro_test_exists("section14_candle_numbers_reproduce_on_the_engine");

        // ⑤ md:599「对称」＝棋盘上只有一处画蜡烛的函数，敌我各调一次。
        assert_eq!(
            code_occurrences("candle_bar("),
            3,
            "§十四:599 要求棋盘的蜡烛条只有 1 处定义＋敌我各 1 处调用（`candle_bar(` 合计 3 次），实测 {} 次 ⇒ 有人另起炉灶画蜡烛，或摘掉了一侧的调用（`boss::dossier` 那种纯数字串不是画条，不在此数）",
            code_occurrences("candle_bar(")
        );
    }

    /// §九 融合系统反向覆盖：27 条必检行——**第一条把四条认领路在同一章用满的章**。
    ///
    /// 形状：五道围栏（定义 311／流程 324–335／示例 341–347／叠加等式 357／开端 363）＝10 个 ``` 围栏符，
    /// 外加一张三行「融合时机」表。与 §十四 那台模板有两处口径差别，都不是随手改的：
    /// ① **标签判据多一条否决：不以全角冒号收尾。** md:354「若技能效果冲突（如两个"攻击后+1累积"），则叠加：」
    ///    单 token、下一非空行正是 ```，按 §十四 那条会被吞成标签——可它是**条件句**，它的取值在下一道围栏里
    ///    （357）。实测：否决前 labels 多出 354、围栏外只剩 5 行；否决后 352 与 354 双双落进必检集（两行都已有锚）。
    /// ② **分区计数把「挂债」排在「实测」之前**（§十四/§十五/§十六 都是实测优先）。本章唯一那条债 md:335
    ///    就躺在流程围栏里，解析器数它只为让它**可见**（它写「继承堆总数不变」，引擎 2 张进 1 张出，正好相反），
    ///    并不复现它。按 §十四 的顺序它会被记进"实测 21"，那条与文档相反的账就此隐身。
    ///
    /// 配比钉死 **锚点6／挂债1／实测20**，另四层各钉一件事：
    /// ② 示例那 6 行**不许有锚**（分界由解析器变体 `is_example` 判，这里不写行号清单）；
    /// ③ 围栏那 14 条**规则**行一行都不许缺锚（双记账，同 §十四：摘锚只红这一层，配比看不见）；
    /// ④ **文档自己两处对撞**——第 1 步「保留其特性+数值+阈值+费用」↔ 第 4 步那四条 `- X = 主牌X`；
    ///    示例的价 3-1=2 ↔ 流程的「副牌费用-1，最低0」；等式左边合计 ↔ 右边；
    /// ⑤ 复现测还在 ＋ 代码形状（时机表那三行的 ❌ 落成"战斗词表里没有 `\"fuse\"`"；
    ///    `fuse_cards` 体内 `is_starter()` 恰两次＝两侧都查；`.remove(` 恰一次＝副牌只被摘走一次）。
    ///
    /// 边界如实登记：⑤ 只核到"命令词在不在词表里"这个形状，**不核执行序**——准备／结算阶段那条闸属 §十二:415/500，
    /// 由 §十二 的推导器负责；融合把新牌标成自造牌（`m.crafted = true`）之后"离场永久消失"那半截属 §十:372，
    /// 不在本章配比里。
    /// 语义曲解始终不可机检：把「叠加」实现成「取最大」，④ 那层文档内部对撞量不到，靠 ⑤ 之前的引擎实测复现测
    /// （`progress.rs::section9_fuse_rows_reproduce_on_the_engine` 真的走一遍攻击阶段）与 §八 的效果尺兜。
    #[test]
    fn every_row_of_section9_fusion_is_anchored_debited_or_reproduced_and_examples_never_take_anchors() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "九、融合系统")
            .expect("§九 标题必须存在（文档结构变了就要同步改本检查）");
        let arity = |n: usize| -> usize { at(n).split_whitespace().count() };
        let next_non_blank = |n: usize| -> usize {
            let mut k = n + 1;
            while k <= lines.len() && at(k).trim().is_empty() {
                k += 1;
            }
            k
        };
        let block_len = |n: usize| -> usize {
            let mut k = n;
            let mut cnt = 0usize;
            while k <= lines.len() && !at(k).trim().is_empty() && at(k).trim() != "---" && at(k).trim() != "```" {
                cnt += 1;
                k += 1;
            }
            cnt
        };

        // 围栏外的形状路（标签／列头）＋围栏内的解析器路，两条拼成必检集。
        const S9_HEADERS: [&str; 1] = ["时机 是否可融合"];
        let mut rows: Vec<usize> = Vec::new();
        let mut labels: Vec<usize> = Vec::new();
        let mut headers: Vec<usize> = Vec::new();
        let mut fence_ticks = 0usize;
        let mut fence = false;
        for n in (head + 2)..=lines.len() {
            let t = at(n).trim();
            if !fence && t == "---" {
                break;
            }
            if t == "```" {
                fence = !fence;
                fence_ticks += 1;
                continue;
            }
            if t.is_empty() || fence {
                continue;
            }
            let nb_text = at(next_non_blank(n)).trim();
            if arity(n) == 1 && !t.ends_with('：') && (nb_text == "```" || S9_HEADERS.contains(&nb_text)) {
                labels.push(n);
                continue;
            }
            if S9_HEADERS.contains(&t) {
                assert!(block_len(n) >= 2, "md:{n}「{t}」在列头词表里却不在任何表格块的开头 ⇒ 文档改了表形，词表得跟着改（结构与字面两个方向都要对得上）");
                headers.push(n);
                continue;
            }
            rows.push(n);
        }
        assert_eq!(fence_ticks, 10, "§九 应有五道围栏＝10 个 ``` 围栏符（定义／流程／示例／叠加等式／开端），实测 {fence_ticks} 个 ⇒ 文档加了或拆了一道，而解析器读的正是这五道，那道里的行会从两把尺外面一起漏过去");
        assert!(!fence, "§九 的围栏没有闭合（数到奇数个 ```）");
        assert_eq!(
            labels,
            vec![308, 314, 321, 338, 360],
            "§九 的标签应恰好是 308 基本规则／314 融合时机／321 融合流程／338 融合示例／360 开端不可融合，实测 {labels:?} ⇒ 有真规则行被当成标签吞掉，或标签判据（含那条「不以全角冒号收尾」的否决）失效"
        );
        assert_eq!(headers, vec![316], "§九 的列头应恰好是 316「时机 是否可融合」，实测 {headers:?} ⇒ 列头判据失效（结构＋字面两个条件缺一不可）");
        assert_eq!(
            rows,
            vec![317, 318, 319, 350, 352, 354],
            "§九 围栏外应得时机表体 3 行＋设计意图 350＋冲突处理 352／354，实测 {rows:?} ⇒ 多半是 354 又被当成标签吞了（它以全角冒号收尾，见本测 ①）"
        );

        let parsed = crate::progress::parse_section9_fuse_rules(&lines);
        let verified: Vec<usize> = parsed.iter().map(|c| c.line).collect();
        assert_eq!(
            verified,
            vec![311, 324, 325, 326, 327, 328, 329, 330, 331, 332, 333, 334, 335, 341, 342, 344, 345, 346, 347, 357, 363],
            "§九 围栏行由解析器给，应为 定义1＋流程12＋示例6＋等式1＋开端1＝21 行，实测 {verified:?} ⇒ 解析器与本推导器对同一段围栏读法不同"
        );
        rows.extend(verified.iter().copied());
        rows.sort_unstable();
        assert_eq!(rows.len(), 27, "§九 必检行应恰好 27 行（围栏外 6＋围栏内 21），实测 {} 行 ⇒ 文档加了规则或推导口径失效", rows.len());
        // 成员级双向钉：四种形态各钉一行必须在必检集，各类排除行各钉一行必须不在。
        for n in [311usize, 317, 326, 335, 341, 346, 350, 354, 357, 363] {
            assert!(rows.contains(&n), "md:{n} 被剔出 §九 必检集 ⇒ 推导器漏了这种形态（「{}」）", at(n));
        }
        for n in [306usize, 308, 310, 314, 316, 321, 323, 338, 356, 360, 362, 366] {
            assert!(!rows.contains(&n), "md:{n}（「{}」）进了必检集 ⇒ 章标题／标签／列头／围栏符判据失效", at(n));
        }

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        for (n, why) in NOT_A_RULE {
            assert!(
                !rows.contains(n),
                "md:{n} 落在 §九 内却被 `NOT_A_RULE` 认领＝排除表能吞掉真规则。要说它不算规则，请挂债并写去处；登记的排除理由：{why}"
            );
        }

        // 示例／规则的分界由**解析器的变体**判（`S9Claim::is_example`）。这里不写行号清单：写了＝推导器再认一次形状，
        // 两处各判就会朝不同方向错，而「这行算不算示例」恰恰只在这种偏移上才静默绿。
        let examples: Vec<usize> = parsed.iter().filter(|c| c.claim.is_example()).map(|c| c.line).collect();
        let rule_lines: Vec<usize> = parsed.iter().filter(|c| !c.claim.is_example()).map(|c| c.line).collect();
        assert_eq!(
            examples,
            vec![341, 342, 344, 345, 346, 347],
            "§九 的示例行应恰好是「文档点名的一对牌（焚稿人＋燎原）」所在的 6 行，实测 {examples:?} ⇒ 有围栏行被重新归类，而两条路的牙不一样（示例禁锚、规则双记账）"
        );
        assert_eq!(rule_lines.len(), 15, "§九 围栏规则行应恰好 15 条（21 行减 6 条示例），实测 {} 条", rule_lines.len());
        for &n in &examples {
            assert!(
                !referenced.contains(&(n as u32)) && !debited.contains(&n),
                "md:{n}（「{}」）是 §九 示例行，却走了锚点／挂债路＝把「必须跑一遍」降级成「有人指过来就行」",
                at(n)
            );
        }

        // ① 三条认领路＋配比。这里的顺序与 §十四 相反：**先债再实测**（见文档注释 ②）。
        let (mut anchored, mut on_debt, mut reproduced) = (0usize, 0usize, 0usize);
        let mut missing: Vec<String> = Vec::new();
        for &n in &rows {
            if debited.contains(&n) {
                on_debt += 1;
            } else if verified.contains(&n) {
                reproduced += 1;
            } else if referenced.contains(&(n as u32)) {
                anchored += 1;
            } else {
                missing.push(format!("  md:{n} ← {}", at(n)));
            }
        }
        assert!(
            missing.is_empty(),
            "§九 有 {} 行既无锚点指回、也不在债表里、又不是围栏里被引擎实测复现的行（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );
        assert_eq!(
            (anchored, on_debt, reproduced),
            (6, 1, 20),
            "§九 三条认领路应为 锚点6／挂债1／实测20，实测 ({anchored},{on_debt},{reproduced}) ⇒ 有行从一条路悄悄挪到另一条"
        );
        assert!(
            debited.contains(&335),
            "md:335「{}」必须仍躺在债表里：它写「继承堆总数不变」，引擎 2 张进 1 张出，正好相反。把它摘进实现＝替文档把这笔反账做平",
            at(335)
        );

        // ③ 双记账那层：分区计数把围栏行按实测优先记，摘掉它们的锚它**看不见**——§十四 M39 实测过这条路。
        for n in [317usize, 318, 319, 350, 352, 354] {
            assert!(referenced.contains(&(n as u32)), "md:{n}（「{}」）是围栏外必检行，登记的处置是「锚点指回」，现在没有锚点＝实现被删或锚被摘", at(n));
        }
        for &n in &rule_lines {
            if debited.contains(&n) {
                continue;
            }
            assert!(
                referenced.contains(&(n as u32)),
                "md:{n}（「{}」）是 §九 围栏里的**规则**行，没有锚点指回 ⇒ 本章纪律是规则行既走实测又全有锚：摘锚红这里，改行为红复现测，两层的牙不一样",
                at(n)
            );
        }

        // ④ 文档自己两处对撞（不碰引擎，纯"文档内部两处说法必须一致"）。
        let mut keeps: Vec<String> = Vec::new();
        let mut fields: Vec<String> = Vec::new();
        let mut price: Option<(i32, i32)> = None;
        let mut sub_cost: Option<i32> = None;
        let mut eq: Option<(i32, i32, i32)> = None;
        let mut stack: Option<(usize, Vec<i32>, i32)> = None;
        for c in &parsed {
            match &c.claim {
                crate::progress::S9Claim::MainKeeps(f) => keeps = f.clone(),
                crate::progress::S9Claim::Field(name) => fields.push(name.clone()),
                crate::progress::S9Claim::Price { minus, floor } => price = Some((*minus, *floor)),
                crate::progress::S9Claim::ExampleCard { which, cost, .. } if *which == "副牌" => sub_cost = Some(*cost),
                crate::progress::S9Claim::ExamplePrice { a, b, out } => eq = Some((*a, *b, *out)),
                crate::progress::S9Claim::StackEquation { terms, lhs, rhs } => stack = Some((terms.len(), lhs.clone(), *rhs)),
                _ => {}
            }
        }
        assert_eq!(
            fields, keeps,
            "§九 流程第 1 步（md:324）的保留清单 {keeps:?} 与第 4 步下面逐行（md:328–331）的 {fields:?} 对不上 ⇒ 文档两处不同名或不同序，融合到底保留什么读不出唯一答案"
        );
        let (minus, floor) = price.expect("md:326 必须给出「副牌费用-N，最低M」两个数");
        assert_eq!((minus, floor), (1, 0), "§九:326 的价现在是 副牌费用-{minus}、最低 {floor}，而引擎两侧逐字同用的那条算式钉死是 -1 与 max(0)（见下面第 ⑤ 层的 `code_occurrences`）⇒ 文档改了价、代码没跟着改");
        let (a, b, out) = eq.expect("md:344 必须给出 A-B=C 三个数");
        assert_eq!((a, b, out), (3, 1, 2), "§九:344 的示例算式应恰好是 3-1=2，实测 {a}-{b}={out}");
        assert_eq!(out, a - b, "§九:344「融合消耗：{a}-{b}={out}业力」自己的算式不平行");
        assert_eq!(sub_cost, Some(a), "§九:344 被减的那个数（{a}）应等于 md:342 副牌的费用（实测 {sub_cost:?}）⇒ 价按哪张牌算，文档两处读法不同");
        assert_eq!(b, minus, "§九:344 减去的 {b} 与 md:326 登记的「减 {minus}」不同 ⇒ 示例围栏与规则围栏各说一套");
        let (n_terms, lhs, rhs) = stack.expect("md:357 必须给出那条叠加等式");
        assert_eq!(lhs.len(), n_terms, "§九:357 左边 {n_terms} 项却数出 {} 个增量 ⇒ 有一项没带数，叠加读不出来", lhs.len());
        assert_eq!(rhs, lhs.iter().sum::<i32>(), "§九:357 左边合计 {}、右边写 {rhs} ⇒ 文档自己的叠加口径不守恒（引擎是逐条 append，见 `progress.rs:51`）", lhs.iter().sum::<i32>());

        // ⑤ 代码形状。
        assert_eq!(
            code_occurrences("\"fuse\""),
            1,
            "§九:317／318／319 那张时机表：准备 ✅／战斗 ❌／结算 ✅。全仓生产码面上命令词 `\"fuse\"` 只能有 1 处（结算词表 `command.rs:217`）——多出一次＝战斗内词表也收了 fuse，md:318 那个 ❌ 就成了假账"
        );
        assert_eq!(
            code_occurrences("def.cost - 1).max(0)"),
            2,
            "§九:326 那条价在引擎面只能有两处**逐字同式**：`progress.rs::fuse_cards` 与 `ai.rs` 专家档（AI 不得自算一套价）。实测 {} 次 ⇒ 有人只改了其中一处 ⇒ 文档那一条与两侧结算不同步",
            code_occurrences("def.cost - 1).max(0)")
        );
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let prog = std::fs::read_to_string(manifest.join("src/progress.rs")).unwrap();
        let body = s7_fn_body(&prog, "pub fn fuse_cards(");
        assert_eq!(
            body.matches("is_starter()").count(),
            2,
            "§九:363「开端不可融合，无论在手牌还是继承堆」要求**主、副两侧都查**：`fuse_cards` 体内 `is_starter()` 应恰 2 次，实测 {} 次 ⇒ 只剩一侧（副牌是开端也照样融）",
            body.matches("is_starter()").count()
        );
        assert_eq!(
            body.matches(".remove(").count(),
            1,
            "§九:325／333 副牌的作用域：体内只该有一次摘除，实测 {} 次 ⇒ 有人另摘一张牌，或把摘除改成置空占位（那是拿实现去还 md:335 那笔反账，债表会跟着红）",
            body.matches(".remove(").count()
        );
        engine_repro_test_exists("section9_fuse_rows_reproduce_on_the_engine");
    }

    /// §五 开端·核心起始牌反向覆盖：19 条必检行——**这一章第一次把文档那张卡逐字段撞回 `STARTER`**。
    ///
    /// 形状：列头「项目 内容」＋七行表体（179–185）＋一道开局围栏（189–208）。表体走"锚点指回＋逐字段等值"，
    /// 围栏走"锚点指回＋文档数字驱动引擎"（`battle::parse_section5_opening` ＋ `s5_fold` ＋
    /// `section5_opening_fence_reproduces_on_the_engine`）。与 §十四 那台模板的差别只有一处：**标签分两档给**——
    /// 围栏外的 `开端`／`开局选择` 由形状路判（arity 1 且下一非空行是列头或 ```），围栏内的 4 条标签
    /// （190/195/197/203）由**解析器**给，推导器不在围栏里再判一次（同 §十四 那条理由：两处各判会朝不同方向错）。
    ///
    /// **双记账**同 §十四——19 行**全**要求锚点指回，走了实测路也不豁免。两层的牙不一样：
    /// 摘掉 md:199 的锚 ⇒ 只有"19 行全有锚"那条红（配比 锚点8／挂债0／实测11 看不见）；
    /// 把文档副本 md:200 的「每关最多2次」改成 3 ⇒ 只有引擎实测那条红（行数没变，形状尺量不到数字）。
    ///
    /// 六层各钉一件事：
    /// ① 认领路：配比 锚点8／挂债0／实测11；19 行全有锚；`NOT_A_RULE` 不吃本章任何一行。
    /// ② 表体逐字段等值 vs `STARTER`：键是**封闭词表**（7 个），费用→`cost`／数值→`power`（那一格里两个数
    ///    必须彼此相等且等于 `power`，文档"血量=伤害=1"就是这一个字段）／阈值→`threshold`／特性→`TraitKind::Starter`，
    ///    外加 md:176 那两个字 ↔ `STARTER.name` 逐字节。
    /// ③ **文档两处对撞**（表体 ↔ 围栏，不碰引擎）：182 第一子句那个数 ↔ 204 ↔ 207；182 第三子句 (+N／上限M) ↔ 200 ↔ 201；
    ///    179 费用 ↔ 198 那个费；185「每关固定发放」↔ 191 手牌行含开端。
    /// ④ 上限那道闸在码面只认一个数，而且那个数由文档给（③ 已把它与 182/200 撞平）。
    /// ⑤ 三条**否定式** cell 各落成一份**封闭的**码面形状——"没有这东西"只能靠点名实现处来证。
    /// ⑥ 复现测还在（`engine_repro_test_exists`）。
    ///
    /// 边界如实登记：md:191 在复现测里只复现成**前缀**（开局手牌的头几张），因为 `Battle::new` 末尾调
    /// `player_turn_start()`——§十二:426 那张"回合开始自动抽 1 张"归 §十二，把它算进 §五 的张数就是替别的章作证。
    /// md:210「设计意图」只拿到一个锚（`ai.rs` 献祭分支）：它写的是"取舍"，机器侧唯一可读的形状就是 AI 真选了哪一侧，
    /// 语义本身仍不可机检。把「长期收益」实现成一次性 +1，③ 量不到（数都对），靠 `flags.starter_gains` 那个计数器
    /// 与复现测里逐回合递增的那条序列兜。
    #[test]
    fn every_row_of_section5_starter_is_anchored_back_and_its_opening_fence_is_reproduced() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let head = lines
            .iter()
            .position(|l| l.trim() == "五、开端 · 核心起始牌")
            .expect("§五 标题必须存在（文档结构变了就要同步改本检查）");
        let arity = |n: usize| -> usize { at(n).split_whitespace().count() };
        let next_non_blank = |n: usize| -> usize {
            let mut k = n + 1;
            while k <= lines.len() && at(k).trim().is_empty() {
                k += 1;
            }
            k
        };
        let block_len = |n: usize| -> usize {
            let mut k = n;
            let mut cnt = 0usize;
            while k <= lines.len() && !at(k).trim().is_empty() && at(k).trim() != "---" && at(k).trim() != "```" {
                cnt += 1;
                k += 1;
            }
            cnt
        };

        // 围栏外的形状路（标签／列头）＋围栏内的解析器路，两条拼成必检集。
        const S5_HEADERS: [&str; 1] = ["项目 内容"];
        let mut rows: Vec<usize> = Vec::new();
        let mut labels: Vec<usize> = Vec::new();
        let mut headers: Vec<usize> = Vec::new();
        let mut fence_ticks = 0usize;
        let mut fence = false;
        for n in (head + 2)..=lines.len() {
            let t = at(n).trim();
            if !fence && t == "---" {
                break;
            }
            if t == "```" {
                fence = !fence;
                fence_ticks += 1;
                continue;
            }
            if t.is_empty() || fence {
                continue;
            }
            let nb_text = at(next_non_blank(n)).trim();
            if arity(n) == 1 && (nb_text == "```" || S5_HEADERS.contains(&nb_text)) {
                labels.push(n);
                continue;
            }
            if S5_HEADERS.contains(&t) {
                assert!(block_len(n) >= 2, "md:{n}「{t}」在列头词表里却不在任何表格块的开头 ⇒ 文档改了表形，词表得跟着改（结构与字面两个方向都要对得上）");
                headers.push(n);
                continue;
            }
            rows.push(n);
        }
        assert_eq!(fence_ticks, 2, "§五 应只有一道 ``` 围栏（开局选择那一段），实测 {fence_ticks} 个围栏符 ⇒ 文档加了第二段围栏，而解析器只读第一道，那里的新行会从两把尺外面一起漏过去");
        assert!(!fence, "§五 的围栏没有闭合（数到奇数个 ```）");
        assert_eq!(
            labels,
            vec![176, 187],
            "§五 围栏外的标签应恰好是 176 开端／187 开局选择，实测 {labels:?} ⇒ 有真规则行被当成标签吞掉，或标签判据失效"
        );
        assert_eq!(headers, vec![178], "§五 的列头应恰好是 178「项目 内容」，实测 {headers:?} ⇒ 列头判据失效（结构＋字面两个条件缺一不可）");
        assert_eq!(
            rows,
            vec![179, 180, 181, 182, 183, 184, 185, 210],
            "§五 围栏外应得表体 7 行＋设计意图 210，实测 {rows:?} ⇒ 表体行数或 210 的形状判据变了"
        );

        let parsed = crate::battle::parse_section5_opening(&lines);
        let fence_labels: Vec<usize> = parsed.iter().filter(|c| c.claim == crate::battle::S5Claim::Label).map(|c| c.line).collect();
        let verified: Vec<usize> = parsed.iter().filter(|c| c.claim != crate::battle::S5Claim::Label).map(|c| c.line).collect();
        assert_eq!(
            fence_labels,
            vec![190, 195, 197, 203],
            "§五 围栏内的标签由解析器给，应恰好是 190 战斗开始／195 我方回合1／197 选择A／203 选择B，实测 {fence_labels:?} ⇒ 有围栏行被当成标签吞掉（那一条就此从实测与配比里一起隐身）"
        );
        assert_eq!(
            verified,
            vec![191, 192, 193, 198, 199, 200, 201, 204, 205, 206, 207],
            "§五 围栏实测行应为 开局3＋选择A的4＋选择B的4＝11 行，实测 {verified:?} ⇒ 解析器与本推导器对同一段围栏读法不同"
        );
        rows.extend(verified.iter().copied());
        rows.sort_unstable();
        assert_eq!(rows.len(), 19, "§五 必检行应恰好 19 行（围栏外 8＋围栏内 11），实测 {} 行 ⇒ 文档加了规则或推导口径失效", rows.len());
        // 成员级双向钉：三种形态各钉一行必须在必检集，各类排除行各钉一行必须不在。
        for n in [179usize, 182, 185, 191, 199, 207, 210] {
            assert!(rows.contains(&n), "md:{n} 被剔出 §五 必检集 ⇒ 推导器漏了这种形态（「{}」）", at(n));
        }
        for n in [174usize, 176, 178, 187, 189, 190, 197, 208, 212] {
            assert!(!rows.contains(&n), "md:{n}（「{}」）进了必检集 ⇒ 章标题／标签／列头／围栏符判据失效", at(n));
        }

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        for (n, why) in NOT_A_RULE {
            assert!(
                !rows.contains(n),
                "md:{n} 落在 §五 内却被 `NOT_A_RULE` 认领＝排除表能吞掉真规则。要说它不算规则，请挂债并写去处；登记的排除理由：{why}"
            );
        }

        // ① 三条认领路＋配比。
        let (mut anchored, mut on_debt, mut reproduced) = (0usize, 0usize, 0usize);
        let mut missing: Vec<String> = Vec::new();
        for &n in &rows {
            if verified.contains(&n) {
                reproduced += 1;
            } else if referenced.contains(&(n as u32)) {
                anchored += 1;
            } else if debited.contains(&n) {
                on_debt += 1;
            } else {
                missing.push(format!("  md:{n} ← {}", at(n)));
            }
        }
        assert!(
            missing.is_empty(),
            "§五 有 {} 行既无锚点指回、也不在债表里、又不是围栏里被引擎实测复现的行（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );
        assert_eq!(
            (anchored, on_debt, reproduced),
            (8, 0, 11),
            "§五 三条认领路应为 锚点8／挂债0／实测11（围栏行按实测优先计），实测 ({anchored},{on_debt},{reproduced}) ⇒ 有行从一条路悄悄挪到另一条"
        );
        for n in [179usize, 180, 181, 182, 183, 184, 185, 210] {
            assert!(referenced.contains(&(n as u32)), "md:{n}（「{}」）是围栏外必检行，登记的处置是「锚点指回」，现在没有锚点＝实现被删或锚被摘", at(n));
        }
        // 双记账那层：分区计数把围栏行按实测优先记，摘掉它们的锚它**看不见**（同 §十四 的 M39）。
        for &n in &rows {
            assert!(referenced.contains(&(n as u32)), "md:{n}（「{}」）没有锚点指回 ⇒ §五 的纪律是 19 行**全**指回，走了实测路也不豁免锚点", at(n));
        }
        assert!(
            rows.iter().all(|n| !debited.contains(n)),
            "§五 不该有挂债行：本章 19 行全部已实现并已指回，任何一行躺进债表都说明实现被撤"
        );

        // ② 表体逐字段等值。列头之后那 7 行是「键 值」，键是**封闭词表**，值里的数就是文档给的数。
        const S5_KEYS: [&str; 7] = ["费用", "数值", "阈值", "特性", "技能", "可融合", "入继承堆"];
        let mut cells: Vec<(usize, String, String)> = Vec::new();
        for n in (headers[0] + 1)..=lines.len() {
            let t = at(n).trim();
            if t.is_empty() {
                break;
            }
            let mut it = t.split_whitespace();
            let k = it.next().unwrap_or("").to_string();
            let rest: Vec<&str> = it.collect();
            assert!(
                S5_KEYS.contains(&k.as_str()) && rest.len() == 1,
                "md:{n}「{t}」不是 §五 表体那 7 个键之一的「键 单值」形态（键表 {S5_KEYS:?}，实测键「{k}」／值段 {} 段）⇒ 文档改了这张表的形（加列、或把一个值拆成两段），逐字段等值就没法成立",
                rest.len()
            );
            cells.push((n, k, rest[0].to_string()));
        }
        assert_eq!(
            cells.iter().map(|(_, k, _)| k.as_str()).collect::<Vec<&str>>(),
            S5_KEYS.to_vec(),
            "§五 表体的键与顺序应恰好是 {S5_KEYS:?}，实测 {:?}（行号 {:?}）⇒ 加行、删行或换序都会让「这张表描述的就是引擎那张卡」失去唯一读法",
            cells.iter().map(|(_, k, _)| k.as_str()).collect::<Vec<&str>>(),
            cells.iter().map(|(n, _, _)| *n).collect::<Vec<usize>>()
        );
        let cell = |key: &str| -> &str {
            cells.iter().find(|(_, k, _)| k == key).map(|(_, _, v)| v.as_str()).unwrap_or_else(|| panic!("§五 表体没有「{key}」这一格"))
        };
        assert_eq!(crate::battle::s15_digits(cell("费用")), vec![super::STARTER.cost], "md:179「费用 {}」与 `STARTER.cost`={} 不等 ⇒ 文档改了价、代码没跟着改（围栏里那个「0费」由 ③ 撞回来）", cell("费用"), super::STARTER.cost);
        let powers = crate::battle::s15_digits(cell("数值"));
        assert_eq!(powers.len(), 2, "md:180「数值 {}」应给两个数（数值本身＋「血量=伤害=1」那两个里的一个），实测 {powers:?} ⇒ 这一格换了形制，\"一个字段两处用\"那条就不成立了", cell("数值"));
        assert_eq!(powers[0], powers[1], "md:180「数值 {}」里那两个数不等 {powers:?} ⇒ 文档这张卡自己血量与伤害分家，而引擎只有 `power` 一个字段（§七:265／§七:266），按哪头都不服另一头", cell("数值"));
        assert_eq!(super::STARTER.power, powers[0], "md:180 的数值 {} ≠ `STARTER.power`={} ⇒ 改任何一侧都要撞这里，挂锚证不了\"数相等\"", powers[0], super::STARTER.power);
        assert_eq!(crate::battle::s15_digits(cell("阈值")), vec![super::STARTER.threshold], "md:181「阈值 {}」≠ `STARTER.threshold`={}（触发特性所需业火值，比较点在 `battle.rs` 的触发闸）", cell("阈值"), super::STARTER.threshold);
        assert!(matches!(super::STARTER.tr, super::TraitKind::Starter), "md:182 那一整行说这张卡自带特性 ⇒ 码面类型标签必须是 `TraitKind::Starter`；实测不是，则 ④⑤ 点名的那些靠 `is_starter()` 的开端闸会集体失效（免费放置、回合末 +1、献祭定额、不入堆）");

        // ③ 文档两处对撞：特性行的三个子句 ↔ 围栏里选择A／选择B 那几条。按位置取，所以先钉顺序。
        let clauses: Vec<&str> = cell("特性").split('；').collect();
        assert_eq!(clauses.len(), 3, "md:182「特性 {}」按全角分号切出 {} 子句，应为 3（死亡获业力／放置不消耗业力／在场回合末 +N 上限M）⇒ 文档加了第四子句，得同步扩这里与它的落点", cell("特性"), clauses.len());
        assert!(
            clauses[0].contains("死亡") && clauses[1].contains("不消耗业力") && clauses[2].contains("每回合结束"),
            "md:182 三个子句的**顺序**变了（0「{}」1「{}」2「{}」）⇒ 这里是按位置取数的，换序等于把数挂到别的条款上",
            clauses[0], clauses[1], clauses[2]
        );
        let death = crate::battle::s15_digits(clauses[0]);
        assert_eq!(death.len(), 1, "md:182 第一子句「{}」应恰给一个数（死亡获N业力），实测 {death:?}", clauses[0]);
        assert_eq!(crate::battle::s15_digits(clauses[1]), Vec::<i32>::new(), "md:182 第二子句「{}」里出现了数字 ⇒ 「放置不消耗业力」被写成了别的东西（那个 0 费在 md:198，别在这里加第二处真值）", clauses[1]);
        let turn = crate::battle::s15_digits(clauses[2]);
        assert_eq!(turn.len(), 2, "md:182 第三子句「{}」应给两个数（每次 +N／每关上限M），实测 {turn:?}", clauses[2]);
        // 围栏折叠出的那几个数**由文档给**（`s5_fold` 少一行当场 panic）：③ 拿它们撞表体，复现测拿它们撞引擎。
        let f = crate::battle::s5_fold(&parsed);
        let f_starter_in_hand = f.hand.expect("折叠后必有手牌行：缺它 `s5_fold` 已 panic").0;
        let f_place_cost = f.place_free.expect("折叠后必有放置行").1;
        let (fe_gain, fe_cap) = f.turn_end.expect("折叠后必有回合末行");
        let f_accumulate = f.accumulate.expect("折叠后必有长期收益行");
        let f_sac_gain = f.sac_gain.expect("折叠后必有献祭行");
        let f_burst_gain = f.burst.expect("折叠后必有短期爆发行").0;

        assert_eq!(
            (turn[0], turn[1]),
            (fe_gain, fe_cap),
            "md:182 特性行给的 (+{}／上限{}) 与 md:200 围栏里的 (+{}／最多{}) 不等 ⇒ 同一条规则在文档两处各写一套，引擎按哪边都不服另一边",
            turn[0], turn[1], fe_gain, fe_cap
        );
        assert_eq!(
            turn[0], f_accumulate,
            "md:182 那份每回合 +{} 与 md:201「长期收益：每回合+{}业力」不等 ⇒ 长期收益那条与特性行分叉",
            turn[0], f_accumulate
        );
        assert_eq!(
            death[0], f_sac_gain,
            "md:182 第一子句「死亡获{}业力」与 md:204「献祭开端 → 获得{}业力」不等 ⇒ md:204 明写「来源：特性」，那两个数必须就是同一个数",
            death[0], f_sac_gain
        );
        assert_eq!(
            f_sac_gain, f_burst_gain,
            "md:204 那个 {} 与 md:207「短期爆发：立即获得{}业力」不等 ⇒ 选择B 的价在围栏里两处各写一套",
            f_sac_gain, f_burst_gain
        );
        assert_eq!(
            super::STARTER.cost, f_place_cost,
            "md:179 表体那个费用 {} 与 md:198「开端放到P1，{}费」不等 ⇒ 卡表费用与放置费分叉（引擎只按 `is_starter()` 硬编 0，文档任一处改了都得撞这里）",
            super::STARTER.cost, f_place_cost
        );
        assert!(f_starter_in_hand, "md:191 的手牌行不含「开端」⇒ md:185「{}」那条在本章实测里隐身了（开局第一张必须就是开端，不是从堆里抽来的）", cell("入继承堆"));
        assert_eq!(cell("技能"), "无", "md:183「技能 {}」不是那个否定字面 ⇒ 文档开始给开端写技能了，⑤ 那四处发牌口闸和 §八 的技能尺都得跟着改", cell("技能"));
        assert!(cell("可融合").starts_with('❌'), "md:184「可融合 {}」不以 ❌ 开头 ⇒ 这一格从否定翻成肯定，⑤ 那条融合闸就成了在实现文档没写的东西", cell("可融合"));
        assert!(cell("入继承堆").starts_with('❌'), "md:185「入继承堆 {}」不以 ❌ 开头 ⇒ 同上，那一格是本章唯一说\"开端不进堆\"的字面", cell("入继承堆"));
        assert!(cell("入继承堆").contains("每关固定发放"), "md:185「入继承堆 {}」丢了括号里那句「每关固定发放」⇒ 它与 md:191 手牌行的「开端」和 §十二:421 各说一套（③ 与 §十二 的锚靠这句连起来）", cell("入继承堆"));

        // ④ 上限那道闸在码面只认一个数，而且那个数由文档给（③ 已把 fe_cap 与 turn[1] 撞平）。
        let gate = format!("starter_gains >= {fe_cap}");
        assert_eq!(
            code_occurrences(&gate),
            1,
            "md:200／md:182 说每关上限 {fe_cap} 次 ⇒ 生产码面上那道闸 `{gate}` 必须恰有 1 处，实测 {} 处 ⇒ 0 处＝闸被摘（回合末无限 +1，复现测那条序列会跟着红）；≥2 处＝有人另起一份计数器，文档没写过第二个上限",
            code_occurrences(&gate)
        );

        // ⑤ 三条否定式 cell 各落成一份**封闭的**码面形状："没有这东西"只能靠点名实现处来证。
        assert_eq!(
            code_occurrences("&& !c.is_starter()"),
            4,
            "md:183「技能 无」＝四处发牌口（`Battle::new` 的 `mk`／`make_card`／`player_turn_start` 自动抽牌／`action_draw` 手动抽牌）**全部**把开端排除在技能之外，实测 {} 次 ⇒ 少一次＝有一口会给开端发技能（文档那格「无」成假账）；多一次＝文档没登记的地方在按这个形状闸，先把它挂进本章口径再改这里",
            code_occurrences("&& !c.is_starter()")
        );
        assert_eq!(
            code_occurrences("inherit[main].is_starter() || inherit[sub].is_starter()"),
            1,
            "md:184「可融合 ❌」＝融合闸两侧各查一次、且只在这一处（`progress::fuse_cards`），实测 {} 次 ⇒ 0 次＝开端可融；2 次＝另起炉灶又闸一遍（§九:363 那条同一规则的措辞会跟着分叉）",
            code_occurrences("inherit[main].is_starter() || inherit[sub].is_starter()")
        );
        assert_eq!(
            code_occurrences("v.retain(|c| !c.is_starter())"),
            1,
            "md:185「入继承堆 ❌」＝继承堆构造时剔开端、且只在这一处（`battle.rs` 的继承堆构造），实测 {} 次 ⇒ 0 次＝开端会跨关继承（与 md:191 的「固定发放」和 md:185 自己同时相反）；2 次＝有人多剔一遍，那第二处的口径文档里找不到",
            code_occurrences("v.retain(|c| !c.is_starter())")
        );

        // ⑥ 文档唯一的卡名 ↔ 引擎那张卡；那些数字真的被引擎跑过。
        assert_eq!(at(176).trim(), super::STARTER.name, "md:176「{}」与 `STARTER.name`=「{}」不等 ⇒ 这张表描述的不是引擎里那张开端（逐字段等值全部作废）", at(176).trim(), super::STARTER.name);
        engine_repro_test_exists("section5_opening_fence_reproduces_on_the_engine");
    }

    /// §六 开局手牌与双牌堆反向覆盖：12 条必检行——**这一章是纳入尺子的十四章里第一条 12 行全走实测的**
    /// （配比 锚点0／挂债0／实测12；§五 是 8／0／11、§十四 是 3／0／9，那两章的围栏外表体行只挂锚）。
    ///
    /// 形状：两张表（218 表头＋219/220 表体；231 表头＋232/233 表体）＋两道围栏（223–226／238–243）。
    /// 四块的行集由 `battle::parse_section6_dealing` 按状态**显式**分派，本推导器再把围栏外那两块**独立走一遍**
    /// （标签＝arity 1 且下一非空行是表头或 ```；表头＝封闭词表；其余＝表体），两边各数一次、不等就红——
    /// 同一串字让两处各判一次才会朝不同方向错，而「这行算不算本章一条规则」恰恰只在这种偏移上静默绿。
    ///
    /// **双记账**同 §五／§十四：本章 12 行**全**要求锚点指回，走了实测路也不豁免。两层的牙不一样：
    /// 摘掉 md:226 的锚 ⇒ 只有"12 行全有锚"那条红（配比 0／0／12 看不见它）；
    /// 把文档 md:220 的「3张」改成「4张」⇒ 只有引擎实测那条红（行数没变，形状尺量不到数字）。
    ///
    /// 五层各钉一件事：
    /// ① 认领路：配比 锚点0／挂债0／实测12；12 行全有锚；`NOT_A_RULE` 不吃本章任何一行。
    /// ② 两张表的形制：arity 恰为 3、键名走**封闭名单**且顺序与行序一致、每行该带几个数（1／3／3／1）。
    /// ③ 文档那几个数送进码面 grep：`for _ in 0..{n}` 恰 2 处（我发＋敌发的开局循环）、补齐线恰 1 处、
    ///    每回合额度重置那两行恰 1 处、`HAND_LIMIT` 那个常量恰 1 处、自动抽牌落点恰 1 处。
    /// ④ §六:226 那条否定式（「开局手牌**不计入**每回合抽牌次数」）落成一份**封闭的写点名单**——
    ///    "这里没有扣减"只能靠点名全部扣减处来证（§十四 蜡烛写点名单的同族）。
    /// ⑤ 复现测还在（`engine_repro_test_exists`）。
    ///
    /// 边界如实登记：md:219 那半句「固定发放」在复现测里只复现成**前缀**（`Battle::new` 末尾会走 §十二:426
    /// 的回合开始自动抽，把那张算进 §六 的张数就是替别的章作证）；md:232 那句「上一关剩余＋新造牌」的
    /// "装什么"两半里，只有「不包含开端」有本章的实测（收尸那处），另一半归 §廿二:947／§十二:503 那两把尺。
    #[test]
    fn every_row_of_section6_dealing_is_anchored_back_and_reproduced_on_the_engine() {
        let Some(lines) = doc_or_skip() else { return };
        let at = |n: usize| lines.get(n - 1).map(String::as_str).unwrap_or("");
        let arity = |n: usize| -> usize { at(n).split_whitespace().count() };
        let head = lines
            .iter()
            .position(|l| l.trim() == "六、开局手牌与双牌堆")
            .expect("§六 标题必须存在（文档结构变了就要同步改本检查）");
        let next_non_blank = |n: usize| -> usize {
            let mut k = n + 1;
            while k <= lines.len() && at(k).trim().is_empty() {
                k += 1;
            }
            k
        };
        const S6_HEADERS: [&str; 2] = ["手牌 数量 说明", "牌堆 内容 抽取规则"];

        // 围栏外的形状路，独立再走一遍（围栏内的行集来自解析器，本推导器不在围栏里重判标签）。
        let mut rows_outside: Vec<usize> = Vec::new();
        let mut labels: Vec<usize> = Vec::new();
        let mut headers: Vec<usize> = Vec::new();
        let mut fence_ticks = 0usize;
        let mut fence = false;
        for n in (head + 2)..=lines.len() {
            let t = at(n).trim();
            if !fence && t == "---" {
                break;
            }
            if t == "```" {
                fence = !fence;
                fence_ticks += 1;
                continue;
            }
            if t.is_empty() || fence {
                continue;
            }
            let nb = at(next_non_blank(n)).trim();
            if arity(n) == 1 && (nb == "```" || S6_HEADERS.contains(&nb)) {
                labels.push(n);
                continue;
            }
            if S6_HEADERS.contains(&t) {
                headers.push(n);
                continue;
            }
            rows_outside.push(n);
        }
        assert_eq!(fence_ticks, 4, "§六 应恰有两道 ``` 围栏（开局手牌／抽牌规则）＝4 个围栏符，实测 {fence_ticks} ⇒ 文档加了第三道，而本章两把尺都只登记两道，那里的新行会一起漏过去");
        assert!(!fence, "§六 的围栏没有闭合（数到奇数个 ```）⇒ 章界 `---` 落在围栏里，本走法会把下一章的表读成 §六 的");
        assert_eq!(labels, vec![216, 229, 235], "§六 围栏外的标签应恰好是 216 开局手牌／229 双牌堆／235 抽牌规则，实测 {labels:?} ⇒ 有真规则行被当成标签吞掉，或标签判据失效");
        assert_eq!(headers, vec![218, 231], "§六 的表头应恰好是那两张（顺序也要对），实测 {headers:?} ⇒ 表头判据失效（结构＋字面两个条件缺一不可）");
        assert_eq!(rows_outside, vec![219, 220, 232, 233], "§六 围栏外的表体应得 4 行（两张表各两行），实测 {rows_outside:?} ⇒ 有表加了行");

        // 解析器那一路的行集，与上面逐块对账。
        let parsed = crate::battle::parse_section6_dealing(&lines);
        let claimed = |v: &[crate::battle::S6Line]| -> Vec<usize> {
            v.iter().filter(|r| r.claim != crate::battle::S6Claim::Label).map(|r| r.line).collect()
        };
        let mut verified: Vec<usize> = Vec::new();
        verified.extend(claimed(&parsed.table1));
        verified.extend(claimed(&parsed.table2));
        verified.extend(claimed(&parsed.fence1));
        verified.extend(claimed(&parsed.fence2));
        verified.sort_unstable();
        assert_eq!(parsed.labels, labels, "解析器与本推导器对「围栏外标签」读法不同（解析器 {:?}／本尺 {labels:?}）⇒ 两处必有一处漂", parsed.labels);
        assert_eq!(parsed.headers, headers, "解析器与本推导器对「表头行」读法不同（解析器 {:?}／本尺 {headers:?}）⇒ 同上", parsed.headers);
        assert_eq!(claimed(&parsed.table1), vec![219, 220], "解析器给表1 的行集与本尺的 {rows_outside:?} 前半不符 ⇒ 两块表被读成一快，`s6_fold` 那边少一行只会 panic、这里却可能静默");
        assert_eq!(claimed(&parsed.table2), vec![232, 233], "解析器给表2 的行集与本尺的 {rows_outside:?} 后半不符 ⇒ 同上");
        assert_eq!(claimed(&parsed.fence1), vec![223, 224, 225, 226], "解析器给围栏1（开局手牌）的行集应为 两关来源＋补齐＋不计入，实测 {:?}", claimed(&parsed.fence1));
        assert_eq!(claimed(&parsed.fence2), vec![239, 240, 242, 243], "解析器给围栏2（抽牌规则）的行集应为 自动＋主动＋合计＋满额弃牌，实测 {:?}", claimed(&parsed.fence2));
        assert_eq!(verified.len(), 12, "§六 必检行应恰好 12 行（表体 4＋围栏规则行 8），实测 {} 行 ⇒ 文档加了规则或推导口径失效", verified.len());
        let rows = verified.clone();
        for n in [219usize, 220, 223, 226, 232, 233, 239, 243] {
            assert!(rows.contains(&n), "md:{n} 被剔出 §六 必检集 ⇒ 推导器漏了这种形态（「{}」）", at(n));
        }
        for n in [214usize, 216, 218, 222, 229, 231, 235, 237, 238, 241, 246] {
            assert!(!rows.contains(&n), "md:{n}（「{}」）进了必检集 ⇒ 章标题／标签／表头／围栏符判据失效", at(n));
        }

        let referenced = referenced_doc_lines();
        let debited = debt_claimed_lines();
        for (n, why) in NOT_A_RULE {
            assert!(
                !rows.contains(n),
                "md:{n} 落在 §六 内却被 `NOT_A_RULE` 认领＝排除表能吞掉真规则。要说它不算规则，请挂债并写去处；登记的排除理由：{why}"
            );
        }

        // ① 三条认领路＋配比＋双记账。
        let (mut anchored, mut on_debt, mut reproduced) = (0usize, 0usize, 0usize);
        let mut missing: Vec<String> = Vec::new();
        for &n in &rows {
            if verified.contains(&n) {
                reproduced += 1;
            } else if referenced.contains(&(n as u32)) {
                anchored += 1;
            } else if debited.contains(&n) {
                on_debt += 1;
            } else {
                missing.push(format!("  md:{n} ← {}", at(n)));
            }
        }
        assert!(
            missing.is_empty(),
            "§六 有 {} 行既无锚点指回、也不在债表里、又不是被引擎实测复现的行（漏登记）：\n{}",
            missing.len(),
            missing.join("\n")
        );
        assert_eq!(
            (anchored, on_debt, reproduced),
            (0, 0, 12),
            "§六 三条认领路应为 锚点0／挂债0／实测12——本章 12 行**全**走实测（表体那四行也是 `s6_fold` 折叠出来再打进引擎的），实测 ({anchored},{on_debt},{reproduced}) ⇒ 有行从实测路悄悄挪走"
        );
        for &n in &rows {
            assert!(referenced.contains(&(n as u32)), "md:{n}（「{}」）没有锚点指回 ⇒ §六 的纪律与 §五／§十四 相同：12 行**全**指回，走了实测路也不豁免锚点", at(n));
        }
        assert!(
            rows.iter().all(|n| !debited.contains(n)),
            "§六 不该有挂债行：本章 12 行全部已实现并已指回，任何一行躺进债表都说明实现被撤"
        );

        let f = crate::battle::s6_fold(&parsed);
        let (starter_each, starter_note) = f.starter_row.unwrap();
        let (draw_each, t1_base, t1_inherit) = f.inherit_draw_row.unwrap();
        let (s1_stage, s1_n) = f.stage1.unwrap();
        let (s2_stage, s2_n) = f.stage2.unwrap();
        let topup = f.topup.unwrap();
        let (pile_total, pile_auto, pile_manual, no_starter, holds_both) = f.inherit_pile.unwrap();
        let (starter_turns, all_starters) = f.starter_pile.unwrap();
        let auto_n = f.auto_draw.unwrap();
        let (manual_n, mixable) = f.manual_draw.unwrap();
        let per_turn_max = f.per_turn_max.unwrap();
        let hand_cap = f.hand_cap.unwrap();

        // ② 两张表的形制：键名走封闭名单、顺序与行序一致、每行该带的数字个数钉死。
        const S6_KEYS: [(&str, u8, usize); 4] = [("开端", 1, 1), ("继承堆抽牌", 1, 3), ("继承堆", 2, 3), ("开端堆", 2, 1)];
        for (i, &n) in rows_outside.iter().enumerate() {
            let t = at(n).trim();
            let cols: Vec<&str> = t.split_whitespace().collect();
            assert_eq!(cols.len(), 3, "md:{n}「{t}」不是「键 数量 说明」三段式（切成 {} 段）⇒ 这张表加列或并列了，逐格等值没法成立", cols.len());
            let table = if i < 2 { 1u8 } else { 2 };
            let idx = S6_KEYS
                .iter()
                .position(|(k, tb, _)| *k == cols[0] && *tb == table)
                .unwrap_or_else(|| panic!("md:{n}「{t}」的键「{}」不在表{table} 那两行的封闭名单里（{S6_KEYS:?}）⇒ 表换脸了，而实测还在按旧名读数", cols[0]));
            assert_eq!(idx, i, "md:{n} 的键在名单里的位置 {idx} 与它在表里的行序 {i} 不符 ⇒ 两行换了个位置，`s6_fold` 按形状取数就会挂到别的条款上");
            let nums = crate::battle::s15_digits(t);
            assert_eq!(nums.len(), S6_KEYS[idx].2, "md:{n}「{t}」应恰给 {} 个数字（{S6_KEYS:?} 那一格登记的口径），实测 {nums:?} ⇒ 这一格换了形制，折叠出来的数会挂错条款", S6_KEYS[idx].2);
        }
        // 说明列里那几处措辞是本章实测的**前提**，本尺独立再查一遍（`s6_table_row` 那边也查一次，两处读法不齐就红）。
        let note = |n: usize| -> String { at(n).split_whitespace().nth(2).unwrap_or("").to_string() };
        assert!(starter_note && note(219).contains("不入继承堆"), "md:219 的说明列丢了「不入继承堆」（折叠读成 {starter_note}）⇒ ④ 那份写点名单与 §六:232 那半句「不包含开端」就少了一处文档依据");
        assert!(t1_base && t1_inherit, "md:220 那格读不出「第1关从基础牌堆」与「第2关起从继承堆」两半（{t1_base}/{t1_inherit}）⇒ 表1 与围栏1 的说法分家，实测按关号分派来源就没了依据");
        assert!(no_starter && holds_both, "md:232 的内容列读不出「不包含开端」与「上一关剩余＋新造牌」（{no_starter}/{holds_both}）⇒ 表2 那格换了措辞，③ 点名的收尸处与 §廿二:947 都对不上号了");
        assert!(all_starters, "md:233 的内容列读不出「全是开端」⇒ 复现测里那句「开端堆抽出的必须是开端」失去文档依据");
        assert!(mixable, "md:240 读不出「可混合」⇒ 复现测那句「同一回合两种来源各抽一次」失去文档依据");

        // ③ 文档数字送进码面 grep：等的是「那一行字面里写的那个数」，不是"看起来像"。
        assert_eq!((s1_stage, s2_stage, s1_n, s2_n, draw_each), (1, 2, draw_each, draw_each, draw_each), "md:220／md:223／md:224 三处的张数或关号不齐（表1 {draw_each}／围栏 {s1_stage}→{s1_n}、{s2_stage}→{s2_n}）⇒ 同一件「开局抽几张」在文档里写了三套");
        assert_eq!(topup, draw_each, "md:225 的补齐线 {topup} 与 md:220 的张数 {draw_each} 不是同一个数 ⇒ 「不足」读不出触发线");
        assert_eq!((pile_auto, pile_manual), (auto_n, manual_n), "md:232 那份（{pile_auto}自动+{pile_manual}可选）与 md:239／md:240 那两条（{auto_n}+{manual_n}）不齐 ⇒ 每回合那份预算在文档两处各写一套");
        assert_eq!(pile_auto + pile_manual, pile_total, "md:232 自己不合账：{pile_auto}+{pile_manual} ≠ {pile_total}");
        assert_eq!(auto_n + manual_n, per_turn_max, "md:242 的「每回合最多 {per_turn_max} 张」落不到 {auto_n}+{manual_n} 上");
        assert_eq!(pile_total, per_turn_max, "md:232 说每回合可抽 {pile_total} 次，md:242 说的上限却是 {per_turn_max} 张");
        assert_eq!(code_occurrences(&format!("for _ in 0..{draw_each} {{")), 2, "md:220／md:223／md:224 那个开局张数 {draw_each} 在码面只该出现在**开局发牌那两个循环**（我方＋敌方各一处），实测 {} 处 ⇒ 0 处＝那张表写的数在码面根本没有对应字面（实测会红，但这里的数也就成了空头支票）；≥3 处＝别处又硬编了一个同样的数，文档没登记过第三个吃这个数的地方", code_occurrences(&format!("for _ in 0..{draw_each} {{")));
        assert_eq!(code_occurrences(&format!("if draw_pile.len() < {topup}")), 1, "md:225 那条补齐线在码面恰有 1 处，实测 {} 处 ⇒ 0 处＝不足时不再补齐（复现测① 的第三段会跟着红）；≥2 处＝有人另起一份补齐口径，文档只写了一次", code_occurrences(&format!("if draw_pile.len() < {topup}")));
        assert_eq!(code_occurrences(&format!("self.pf.manual_draws = {pile_manual};")), 1, "md:232 那份「{pile_manual}可选」＝每回合开始重置的主动额度，码面恰有 1 处，实测 {} 处 ⇒ 0 处＝额度不再每回合重置（§六:226 那句「不计入」连带失效，复现测② 会红）", code_occurrences(&format!("self.pf.manual_draws = {pile_manual};")));
        assert_eq!(code_occurrences(&format!("self.pf.starter_draws = {starter_turns};")), 1, "md:233 那句「每回合可抽{starter_turns}次」＝每回合重置的开端堆计数，码面恰有 1 处，实测 {} 处", code_occurrences(&format!("self.pf.starter_draws = {starter_turns};")));
        assert_eq!(code_occurrences(&format!("pub const HAND_LIMIT: usize = {hand_cap};")), 1, "md:243 写的上限 {hand_cap} 张必须就是 `HAND_LIMIT` 那个常量，实测 {} 处命中 ⇒ 0 处＝那个数在码面换了写法或换了值，而文档那句「满8张」还照旧读着", code_occurrences(&format!("pub const HAND_LIMIT: usize = {hand_cap};")));
        assert_eq!(code_occurrences("·自动抽牌："), 1, "md:239 那句「每回合开始自动抽」在码面只有一个落点（`player_turn_start` 那条日志），实测 {} 处 ⇒ 2 处＝有人另起一次自动抽，那 §六:242 那个上限就不是文档写的那个数了", code_occurrences("·自动抽牌："));
        assert_eq!(code_occurrences("starter_draws <= 0"), 1, "md:233 那道「每回合只 {starter_turns} 次」的闸恰有 1 处，实测 {} 处 ⇒ 0 处＝开端堆可连抽（复现测⑤ 那条 Err 断言会红）", code_occurrences("starter_draws <= 0"));
        assert_eq!(code_occurrences("manual_draws <= 0"), 1, "md:242 那个「最多 {per_turn_max} 张」的主动侧闸恰有 1 处，实测 {} 处 ⇒ 0 处＝每回合抽牌不设上限（复现测⑥ 会红）", code_occurrences("manual_draws <= 0"));
        assert_eq!(starter_each, 1, "md:219 写的开端张数是 {starter_each}，而码面 `Battle::new` 那句手牌字面量只有一项（复现测① 拿这个数撞引擎）⇒ 文档若改成 2 张，得先给那处字面量加一项，否则这里红");
        assert_eq!(auto_n, 1, "md:239 说每回合自动抽 {auto_n} 张，可码面那一支是**写死的一次** `remove(0)`＋一次 `push_hand` ⇒ 文档改成 2 张时本章没有「第二处字面」可撞，只能靠复现测④ 那条手牌增量红");

        // ④ §六:226 那条否定式的封闭名单：生产码面上**所有写 `manual_draws` 的行**。
        let mut want_writes: Vec<String> = vec![
            "pub manual_draws: i32,".to_string(),
            format!("pf: SideFlags {{ manual_draws: {pile_manual}, starter_draws: {starter_turns}, ..Default::default() }},"),
            format!("ef: SideFlags {{ manual_draws: {pile_manual}, starter_draws: {starter_turns}, ..Default::default() }},"),
            format!("self.pf.manual_draws = {pile_manual};"),
            "self.pf.manual_draws -= 1;".to_string(),
            "self.pf.manual_draws -= 1;".to_string(),
        ];
        want_writes.sort();
        assert_eq!(
            s6_manual_draw_writes(),
            want_writes,
            "md:226「开局手牌不计入每回合抽牌次数」在码面的形式＝这份**封闭的写点名单**：两处结构体字面量初始化、一处每回合重置、两处 `-= 1`（都在 `action_draw` 的两个来源分支里）。实测多一条＝有人新起了一个扣主动额度的地方，而 §六 只写了「主动抽 {manual_n} 次」这一种扣法（开局那 {draw_each} 张走 `hand.push`，一次都不该碰它）；少一条＝那条路被摘了（连「每回合重置」一起被摘时，§六:226 就没人兑现了）"
        );

        // ⑤ 那些数字真的被引擎跑过、而且那条测还挂在 `#[test]` 上。
        engine_repro_test_exists("section6_dealing_fence_reproduces_on_the_engine");
    }

    /// 债的**分档**——混档就是改写缺口的性质：呈现层欠的是设施（画不出颜色、没有音频），
    /// 规则层欠的是校验（引擎收了它不该收的走法）。后者会让同一局打出不同结果，前者不会。
    /// 第三档 `Mode` 是本帧被 §廿一 逼出来的：整块模式没做（Roguelike／金币／每日固定卡组）既不是
    /// "画不出来"也不是"引擎放过了不该放的走法"，塞进前两档都是改写缺口性质。
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Tier {
        Presentation,
        Rule,
        /// 曾经躺在 `NOT_A_RULE` 里当"规则层范围外"，按裁定26 改挂成有去处的债。
        Washed,
        /// 整块模式／字段级别的功能没有对应物，且文档没给可实现口径。
        Mode,
    }

    impl Tier {
        fn label(self) -> &'static str {
            match self {
                Tier::Presentation => "呈现层·设施缺失",
                Tier::Rule => "规则层·真校验缺失",
                Tier::Washed => "豁免改登记",
                Tier::Mode => "模式层·整块设施未做",
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
    /// 读数：呈现层 17 行（§十五 665/666 ＋ §十六 697/698/702/705/706/707/708/709/715/716/717/718/721
    /// ＋ §十二 479 ＋ §廿三 980；704 是列头「状态 表现」，属非规则行不入债表）、规则层 5 行（§二 47/48 ＋
    /// §九 335 ＋ §十二 416/456）、豁免改登记 1 行（§一 17）、模式层 3 行（§廿一 915/922/923）＝**26 条**。
    /// 提案原文写"18 行／呈现层 15 行"，与它自己逐行列举的 14 差一，本表以文档实测为准。§十二 那三条、
    /// §廿三 980、以及本帧 §廿一 那三条都不是手挑：由各自章的推导器从文档结构里列出"无人认领"的行，
    /// 再逐行判定挂锚还是挂债。模式层这一档是 §廿一 逼出来的——"整块模式没做"塞进前两档都是改写缺口性质。
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
            doc: 335,
            section: 9,
            row: "7. 继承堆总数不变（副牌消失空出1位，新牌占用1位）",
            tier: Tier::Rule,
            evidence: "引擎是 **2 张进 1 张出**：`progress.rs::fuse_cards` 里 `inherit.remove(sub)` 摘走副牌，新牌**就地改写主牌**（不新增元素），所以融合一次继承堆净减一张——实测 `pile.len()==1`。文档这一句「总数不变」与它自己上一句「副牌消失（不进入弃牌堆）」也打架：既消失又占位，那空出来的那位由谁填没写",
            dest: "两条路只能择一：改语义（`progress.rs::fuse_cards` 保留副牌那个空位、新牌 append 进去 ⇒ 堆数不变，但「空位」是什么牌要先定义，且撞 §十「继承堆上限10张」的计数口径），或改文档措辞（把 335 降为「副牌消失⇒堆减一」）。择哪条属设计侧，本帧只登记，实测不复现它（复现＝替文档把反账做平）",
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
            doc: 697,
            section: 16,
            row: "│  🔥🔥🔥🔥🔥      │  ← 业火值：5/6",
            tier: Tier::Presentation,
            evidence: "render.rs 的 `face()` 打的是「值/阈值」再跟一段「焰N」，两个数不同框、也没有按图标个数表示业火值",
            dest: "render.rs：`Board` 已带 flame 与 threshold，图标串可按同章「百分比＝业火值/阈值」那条补齐；纯字符数 CLI 就能偿",
        },
        Debt {
            doc: 698,
            section: 16,
            row: "│  ████████░░     │  ← 进度条（5/6=83%）",
            tier: Tier::Presentation,
            evidence: "全仓唯一一条进度条是 `candle_bar`，它画的是**持业者**的血量（§廿三:1013），卡牌下方的业火进度条没有对应物",
            dest: "render.rs：按 `candle_bar` 同款算法给每张卡补一条（filled = flame*10/阈值）；实心/空心块字符文档已给",
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
        Debt {
            doc: 915,
            section: 21,
            row: "Roguelike 随机卡组+遗物，死亡重来",
            tier: Tier::Mode,
            evidence: "由 §廿一 推导器列出：src/ 全文「遗物」0 命中，`rogue`／随机卡组／死亡重来 三路 grep 皆 0 命中；meta.rs 只有两个入口——mainline_run（60 关推进）与 ladder（play 与 daily 共用的无上限爬梯），没有第三种模式；save.rs 的 Progress 也没有遗物位",
            dest: "文档未给随机卡组的抽取口径、遗物清单与效果、死亡重来的保留范围三件事 ⇒ 先要设计给数再动；开工点 meta.rs 加一个 run 入口 + save.rs::Progress 扩遗物位（CLI 命令面一动，main.rs 词表与裁定27/28 的存档语义要同步）",
        },
        Debt {
            doc: 922,
            section: 21,
            row: "2. 固定卡组+特殊规则",
            tier: Tier::Mode,
            evidence: "由 §廿一 推导器列出：meta.rs::daily_run 只是把 ladder 的种子换成 daily_seed()，起手仍是 §六 的常规开局（开端＋继承堆顶 3 张），没有按日固定的卡组表，也没有任何按日生效的规则开关；文档对「固定卡组」是哪副、「特殊规则」是什么一字未定义 ⇒ 没有可实现口径",
            dest: "与 Boss 数值同一条「文档没真值」的账（裁定20 ④），待人给数；开工点 meta.rs::daily_run 在进 ladder 前装配一次卡组，外加一条按日种子取值的规则开关（落点 save.rs 的日种子比对）",
        },
        Debt {
            doc: 923,
            section: 21,
            row: "3. 完成获得金币，用于解锁新卡",
            tier: Tier::Mode,
            evidence: "由 §廿一 推导器列出：src/ 全文「金币」仅 1 命中，且那一条正是 save.rs 顶部说明「不建金币/成就/收集字段」的注释本身；文档给了金币的获取路径却没给数额、卡池与解锁价格 ⇒ 建了就是没有真值源的第二套壳",
            dest: "先要设计给金币数额与解锁价目，再在 save.rs::Progress 扩金币位并在 meta.rs 通关处发放；解锁表另需文档给卡池",
        },
        Debt {
            doc: 980,
            section: 23,
            row: "屏幕 横屏",
            tier: Tier::Presentation,
            evidence: "由 §廿三 推导器列出：src/ 全文无方向/屏幕尺寸概念（横屏、orientation、COLUMNS、stty 全 0 命中），render.rs 只按 §二 的横屏示意图逐行重打字符帧，帧本身没有「朝向」这件事",
            dest: "TUI/2D/3D 壳的项目设置里定死横屏（三步走里的第二、三步）；CLI 无对应物，不重复计账",
        },
    ];

    /// 未实现债表的机检（裁定26 的同族要求：缺口不许靠排除表洗白，必须挂成**有去处的显式债**）。四条：
    /// ① 每条目指向的文档行**逐字**仍是登记时那一行、且归属同一章——行号漂移当场红，债条不会悄悄换了对象；
    /// ② 机检可见面上**不许有任何锚点指回**这些行：谁实现并挂上锚点，本测就当场红"债已偿，请删条目"，
    ///    这是"债只减不增"唯一能被机器看见的形式；
    /// ③ 每条必须写去处，且去处要点名文件（`.rs`）或点名壳——"以后再说"不是去处；
    /// ④ 同一条文档行不许既躺在 `NOT_A_RULE`（不算规则）又躺在债表（算债），两头下注等于两头都不负责；
    /// ⑤ 复述行（同一缺口在别处又打了一遍）必须与主债同 key 前缀、主债必须还在表里、且自己也不许有锚点。
    /// 条数与分档各钉一个死数：加一条债必须同时改这两个数，等于每次加债都被迫看一眼它属于哪一档。
    /// **诚实盲区（部分已解，剩余如实登记）**：这张表本身仍是**手录**的——机器能证它没过期、不自我洗白，
    /// 证不了它**完整**。§廿二／§十二／§十八／§廿三／§廿一／§十五／§十七／§十六／§八／§七／§十四／§九／§五／§六 十四张表／章现在各自带推导器
    /// （见上面的反向覆盖测试），它们的"无人认领"清单就是这些章债的来源，所以**这十四章的完整性由推导器负责**；
    /// 其余各章（§一/§二/§三/§四/§十/§十一/§十三/§十九/§廿）那九章仍是散文行、未反向纳入，那里的漏记只能靠人 review 发现。
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
        // ⑤ 复述行：同一个缺口在多章各打一遍时，债只记一次、其余登记成 `DEBT_RESTATEMENTS`。
        //    这不是"多一个抵账口"，它比"每条各自挂债"更严：主债必须仍在表里（偿了 ⇒ 复述自动失去认领资格）、
        //    复述行必须非空、**行首 key 必须与主债行同前缀**（不相干的行抵不进来）、且复述行自己也不许有锚点。
        for (main, rs, why) in DEBT_RESTATEMENTS {
            let Some(d) = NOT_IMPLEMENTED.iter().find(|x| x.doc == *main) else {
                bad.push(format!("复述登记：主债 md:{main} 不在 NOT_IMPLEMENTED 里（{why}）⇒ 债已偿，复述跟着删"));
                continue;
            };
            let main_key = d.row.split_whitespace().next().unwrap_or("");
            for &r in *rs {
                let tag = format!("债复述 md:{r} → 主债 md:{main}");
                match lines.get(r - 1) {
                    None => bad.push(format!("{tag}：越界，全文仅 {} 行", lines.len())),
                    Some(l) if l.trim().is_empty() => bad.push(format!("{tag}：指向空行（{why}）")),
                    Some(l) => {
                        let key = l.split_whitespace().next().unwrap_or("");
                        if !(key.starts_with(main_key) || main_key.starts_with(key)) {
                            bad.push(format!(
                                "{tag}：复述行行首 key「{key}」与主债行 key「{main_key}」不同前缀＝拿不相干的行抵债"
                            ));
                        }
                    }
                }
                if referenced.contains(&(r as u32)) {
                    bad.push(format!("{tag}：已有锚点指回 ⇒ 这一处偿了；主债要么整条删掉，要么把复述从这里摘走（{why}）"));
                }
                if NOT_A_RULE.iter().any(|(n, _)| *n == r) {
                    bad.push(format!("{tag}：同时躺在 NOT_A_RULE 里＝既「不算规则」又「算债」"));
                }
            }
        }
        assert!(bad.is_empty(), "未实现债表有 {} 处失效：\n{}", bad.len(), bad.join("\n"));
        assert_eq!(
            NOT_IMPLEMENTED.len(),
            26,
            "债表条数变了。偿了债 ⇒ 删条目并把本数字与下面的分档数一起改小；真要新增债 ⇒ 连同文档出处、证据、去处一起写"
        );
        let pres = NOT_IMPLEMENTED.iter().filter(|d| d.tier == Tier::Presentation).count();
        let rule = NOT_IMPLEMENTED.iter().filter(|d| d.tier == Tier::Rule).count();
        let washed = NOT_IMPLEMENTED.iter().filter(|d| d.tier == Tier::Washed).count();
        let mode = NOT_IMPLEMENTED.iter().filter(|d| d.tier == Tier::Mode).count();
        // 分档不许互相挪：把规则层挪进呈现层＝把"引擎收了它不该收的走法"说成"只是没画出来"，缺口的性质就变了。
        assert_eq!(
            (pres, rule, washed, mode),
            (17, 5, 1, 3),
            "债表应为 呈现层17／规则层5／豁免改登记1／模式层3，实测 ({pres},{rule},{washed},{mode})"
        );
        assert_eq!(
            pres + rule + washed + mode,
            NOT_IMPLEMENTED.len(),
            "分档之和 ({}) ≠ 债表条数 ({}) ⇒ 有新档没被上面的计数覆盖（加了 Tier 变体却忘了 here）",
            pres + rule + washed + mode,
            NOT_IMPLEMENTED.len()
        );
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

    /// **隐形锚点机检**：形似锚点、`anchor_at` 却不认的写法一律报红。写了等于没写，还比没写更坏——
    /// 注释里摆着一个"锚"，读的人（和我自己）都会以为那一行文档已经被指回了，而反向覆盖根本没收到。
    /// 立这条的经过（本帧实测）：§五 首次登记锚点那一轮写出 8 处"章号后直接跟行号"（少冒号），
    /// 当时正向尺把那种写法当合法锚点验一遍判绿、`anchor_numbers` 一个字没看见 ⇒ 全仓 151 条测试一路绿。
    /// 语法收成一处之后那种"绿"没了，但**不报错 ≠ 被看见**：所以这里主动找那种形状。
    ///
    /// 只报两种，判据是**零误报**而非全覆盖（今天实测各 0 处）：
    /// ① `§<中文章号>` 紧跟阿拉伯数字——章号与行号之间缺冒号。
    /// ② `§<阿拉伯数字>:NNN`——章号写成阿拉伯数字，`cn2int` 不认，两把尺都看不见。
    ///
    /// 明知不管的三类，写清楚免得后人以为是漏了：
    /// ㊀ `§` 后面直接跟阿拉伯数字而**没有**冒号＋行号（`boss.rs` 的 `§10-C1`、`meta.rs` 的 `§2`）——
    ///    那是指**别份文档**的条款号，本仓语法只描述 中线.MD，不该替别份文档判形状。
    /// ㊁ `md` 后面紧跟数字（`md5`）——散列名与"md 缺冒号"字面同形，报它就是把这把尺变成天天误报的噪声。
    /// ㊂ 章号与行号之间隔一个空格（"§十五 620"）——两把尺都不认、也不会替它作证，而 `§十五 立的那条`
    ///    这类散文引用在注释里极多，误报代价大于漏报代价。这一类靠作者自律：**要么写冒号，要么别写成锚点的样子**。
    ///
    /// 本注释不举①②的连排实例——那条判据会打到自己的文件（这正是"隐形"反过来咬一口的形态，值得记着）。
    /// 反向覆盖那条尺因此也**不需要**文档在：它扫的是本仓源码，判的是形状，不是行号对不对。
    #[test]
    fn no_doc_anchor_is_written_without_its_colon() {
        let digit = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_digit());
        let mut bad: Vec<String> = Vec::new();
        for fp in src_rs_files() {
            let name = fp.file_name().unwrap().to_string_lossy().into_owned();
            let src = std::fs::read_to_string(&fp).unwrap();
            for (idx, line) in src.lines().enumerate() {
                let cs: Vec<char> = line.chars().collect();
                let digits_run = |from: usize| {
                    let mut k = from;
                    while k < cs.len() && cs[k].is_ascii_digit() {
                        k += 1;
                    }
                    k
                };
                let mut i = 0;
                while i < cs.len() {
                    if cs[i] == '§' {
                        let mut j = i + 1;
                        while j < cs.len() && NUM.contains(&cs[j]) {
                            j += 1;
                        }
                        if j > i + 1 && digit(cs.get(j)) {
                            let k = digits_run(j);
                            bad.push(format!(
                                "  {name}:{} ①章号后直接跟行号，缺冒号 ⇒ 机检看不见这枚锚点：`{}`",
                                idx + 1,
                                cs[i..k].iter().collect::<String>()
                            ));
                            i = k;
                            continue;
                        }
                        if j == i + 1 && digit(cs.get(i + 1)) {
                            let k = digits_run(i + 1);
                            if matches!(cs.get(k), Some(&':' | &'：')) && digit(cs.get(k + 1)) {
                                bad.push(format!(
                                    "  {name}:{} ②章号写成阿拉伯数字，`cn2int` 不认 ⇒ 机检看不见这枚锚点：`{}`",
                                    idx + 1,
                                    cs[i..k].iter().collect::<String>()
                                ));
                            }
                            i = k;
                            continue;
                        }
                    }
                    i += 1;
                }
            }
        }
        assert!(bad.is_empty(), "{} 处隐形锚点（写法形似锚点、语法不认）：\n{}", bad.len(), bad.join("\n"));
    }

    /// **锚点语法的验收单**：`anchor_at` 认什么、不认什么，逐形状钉死。
    /// 为什么单独一条（同 §九 那章"配比钉住"的做法）：`anchor_at` 现在是两把尺共用的唯一定义，
    /// 改它一个字，正向尺与反向覆盖**一起**变。共用最省事的失效方式不是写错，是**悄悄放宽**——
    /// 把冒号改成可选、把 `.` 前缀放过，当场没有任何一条测会红（少冒号的形状由上一条测管，
    /// `.md:NNN` 今天全仓 0 个样本），而反向覆盖从此开始拿文件名替规则行作证：那是**虚覆盖**。
    /// 所以这里两列都要验：正例必须全认出（漏认＝假缺口），反例必须一个都不认（误认＝虚覆盖）。
    /// 电池 M59／M60 一边一条地撞这张表，撞不红就是表没钉住。
    #[test]
    fn anchor_grammar_accepts_only_these_shapes() {
        let yes: &[(&str, &[u32])] = &[
            ("// §五:180 数值 1", &[180]),
            ("// §廿三:990 速查表本体", &[990]),
            ("// md:452 纯行号锚点", &[452]),
            ("// 速查:995 固定指 §廿三", &[995]),
            ("// §十五：620 全角冒号两把尺都认", &[620]),
            ("// §十五:620 与 md:452 同行", &[620, 452]),
            ("//见md:416 中文紧贴也算边界", &[416]),
            ("`§九:335` 反引号包住", &[335]),
        ];
        // 三条 § 形状用 `format!` 拼出来：源码文本里不出现"章号紧跟行号"那种连排，
        // 否则上面那条隐形锚点测会打到自己的夹具。这不是绕规则——夹具拼完仍然是那个形状，
        // 语法要是认了它，下面 `bad` 一样装进去。
        let no: &[(&str, String)] = &[
            ("少冒号（形状由上一条测管）", format!("// §{}185 少冒号", "五")),
            ("少冒号，另一种章号", format!("// §{}354 少冒号", "九")),
            ("章号写成阿拉伯数字", format!("// §{}22:990 阿拉伯章号", "")),
            ("别份文档的条款号，本语法不管", "// §10-C1 别份文档".into()),
            ("章号与行号隔一个空格（明知不管）", "// §十五 620 隔空格".into()),
            ("散列名 md5 与「md 缺冒号」同形，不许报", "// 散列名 md5 比较".into()),
            ("文件名＋行号，不是 中线 的行", "// README.md:12 是文件坐标".into()),
            ("同上", "// chapter-strengthening.md:2 也是文件坐标".into()),
            ("紧贴字母前缀，prev_ok 挡住", "// 命令行 cmd:452".into()),
            ("带点的也一样挡", "// 变量 self.md:452".into()),
            ("只写 § 不写章号", "// 只写 § 没下文".into()),
            ("有冒号没行号", "// §五: 后面是空格".into()),
        ];
        let mut bad: Vec<String> = Vec::new();
        assert!(yes.len() >= 8 && no.len() >= 12, "验收单自己空了或被裁短（正例 {}／反例 {}）⇒ 本测等于没在验", yes.len(), no.len());
        for (s, want) in yes {
            let got = anchor_numbers(s);
            if got.as_slice() != *want {
                bad.push(format!("  正例没认全：`{s}` ⇒ 认到 {got:?}，应为 {want:?}"));
            }
        }
        for (why, s) in no {
            let got = anchor_numbers(s);
            if !got.is_empty() {
                bad.push(format!("  反例被当成锚点（{why}）：`{s}` ⇒ 认到 {got:?}，应为空"));
            }
        }
        assert!(bad.is_empty(), "锚点语法与验收单不符：\n{}", bad.join("\n"));
    }
}
