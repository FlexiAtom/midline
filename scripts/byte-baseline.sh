#!/usr/bin/env bash
# midline - 《中线》核心规则引擎 CLI
# Copyright (C) 2026 FlexiAtom
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU Affero General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU Affero General Public License for more details.
#
# You should have received a copy of the GNU Affero General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.
# 逐字节对账基线（验收尺＝纯重构＋同一族场景在两个二进制之间逐字节相同）。
# 为什么这份文件要进版本库：既往各帧只把 md5 抄进状态文档、脚本留在 /tmp 里被清掉，
# 结果归档的 10 条读数里只有 5 条还能被后人复跑对撞。md5 有据、脚本无档＝不可复现的验收。
# 用法：bash scripts/byte-baseline.sh <二进制> <输出目录>
set -u
B="${1:?用法: $0 <midline 二进制> <输出目录>}"
O="${2:?用法: $0 <midline 二进制> <输出目录>}"
mkdir -p "$O"
# p1 纯 go（准备阶段 EOF 视作 go）｜p2 战斗内基础流＋日志｜p3/p4 Boss 遭遇（脚本输入被忽略，只钉规则形状）
p1() { printf 'go\n'            | "$B" play --seed 7 > "$O/p1.txt" 2>&1; }
p2() { printf 'go\np 0 P1\ne\ne\nl\nq\n' | "$B" play --seed 7 > "$O/p2.txt" 2>&1; }
p3() { printf 'go\ne\ne\ne\nq\n' | "$B" play --boss 4 --seed 2026 > "$O/p3.txt" 2>&1; }
p4() { printf 'go\ne\ne\ne\nq\n' | "$B" boss 5 --seed 2026 > "$O/p4.txt" 2>&1; }
p5() { "$B" auto 3 --seed 7 > "$O/p5.txt" 2>&1; }
p6() { "$B" auto 1 --boss all --seed 7 > "$O/p6.txt" 2>&1; }
# p7 结算词表全动线（fuse/drop/move/up 超限）｜p8 战斗内词表含非法输入｜p10 战斗内 EOF（区别于结算 EOF）
p7() { printf 'fuse 1 2\ndrop 0\nmove 0 1\nup 1 power\nup 2 thr\ngo\nq\n' | "$B" play --seed 7 > "$O/p7.txt" 2>&1; }
p8() { printf 'go\ns P1\nsh 0\ndi\nds\np 0 P1\nx\n\nl\nb\ne\nq\n' | "$B" play --seed 7 > "$O/p8.txt" 2>&1; }
p9() { "$B" daily --save "$O/daily.save" > "$O/p9.txt" 2>&1; }
p10(){ printf 'go\np 0 P1\n' | "$B" play --seed 7 > "$O/p10.txt" 2>&1; }
# p11 影长 遭遇全程。p3/p4/p6 三条 Boss 场景都不落进 影长 的棋盘（p6 的 `--boss all` 只出一行摘要、
# p3/p4 各钉 炎与冰／终影），改脚本在这三条上量不出来——本条是给 Boss 脚本层装的牙：
# 开场脚本、暗影核心的随机附加技能、第一次暗渡挤压全在这条的字节里。
p11(){ printf 'go\ne\ne\ne\nq\n' | "$B" play --boss 3 --seed 2026 > "$O/p11.txt" 2>&1; }
# p12..p14 是每日挑战的三条出口。p9 只有 EOF 一条路（它连一场仗都没打开），所以"场次形状／额度闸／日界重置"
# 三件事在 p9 上**全都量不出来**——改这三处而只有 p9 在跑，就等于把"10/11 条 SAME"当成绿灯。
# 手工造的档面（`daily_kv`）不是测试夹具的作弊：它就是本程序自己的 to_kv 形状，只有日戳与额度两个数被人按过。
# **日戳不许在 shell 里再算一遍哈希**：种子算法（帧 D2＝位置加权求和＋机器本地零点）住在 `meta.rs::daily_seed`，
# 脚本里另写一份就成了第二套真值——它会"永远正确"，因为它从不与程序对撞。现取：先让二进制把今天的种子**印出来**
# （prep 阶段 `q` 弃局，一行输出都不落盘），再拿那个数去造"今天已用完"的档面。
today_seed() { printf 'q\n' | "$B" daily --save "$O/.seed.probe" 2>&1 | sed -n 's/.*seed=\([0-9]\{1,\}\).*/\1/p' | head -1; }
daily_kv() { # $1=日戳 $2=已用额度 $3=落盘路径
    printf 'schema=4\nfaction=1\ndifficulty=normal\nlevel=1\ncarry_karma=0\ncompleted=0\ndaily_done=%s\ndaily_attempts=%s\ndaily_draw_state=%s\ndaily_rule_last=1\ndaily_rule_run=2\ntokens=0\n' "$1" "$2" "$1" > "$3"; }
# p12 一天里的第一场（空档起手）：入口那行报"今日第 1/3 次 ＋ 本场规则"，打三回合后 `q` 弃局——弃局不落盘，
# 所以这条同时钉住"额度只在通关那刻扣"。
p12(){ printf 'go\ne\ne\ne\nq\n' | "$B" daily --save "$O/d12.save" > "$O/p12.txt" 2>&1; }
# p13 今日 3 次已用完 ⇒ 第二次进来该被挡住，且**一场都不许开**。日戳取二进制自己印出的今日种子（见 `today_seed`），
# 所以这条的输出**随日历换 md5**（与 p9 同一口径，那是日历在走、不是行为漂）。
p13(){ local s; s="$(today_seed)"; [ -n "$s" ] || { echo "p13：二进制没报出今日种子 ⇒ 造不出「今天已用完」的档面（别拿一份读不懂的档冒充额度闸）" >&2; exit 1; }
       daily_kv "$s" 3 "$O/d13.save"; "$B" daily --save "$O/d13.save" > "$O/p13.txt" 2>&1; }
# p14 昨天用满、今天该重开：日戳与今天不符 ⇒ 额度作废归零，照常开局。这条**不随日期漂**（日戳钉死为 1），
# 是 p13 的反面——只钉"用完挡得住"不钉"跨日放得开"，把重置判据写成 `==` 也照样全绿。
p14(){ daily_kv 1 3 "$O/d14.save"; printf 'go\nq\n' | "$B" daily --save "$O/d14.save" > "$O/p14.txt" 2>&1; }
# p15 帧 D2 的兼容面：一份**旧档**（schema=2，抽取序列只有 state、没有 pity 那两键，也没有代币）交给新二进制读。
# 报告行里那几个数是这条的全部断言——`上一条 0（连中 0 次）`与`代币=0`＝缺的键取了默认值而不是拒载，
# `schema=4（接受 1..=4）`＝读宽写窄（读进来不就地改写文件，`progress` 从不落盘）。
p15(){ printf 'schema=2\nfaction=1\ndifficulty=normal\nlevel=1\ncarry_karma=0\ncompleted=0\ndaily_done=1\ndaily_attempts=2\ndaily_draw_state=777\n' > "$O/v2.save";
       "$B" progress --save "$O/v2.save" > "$O/p15.txt" 2>&1; }
# p16 帧 D4 装的那条**整场**：p9/p12/p13/p14 四条每日场景全都在**开局前后**就收住了（EOF／三回合弃局／额度挡住），
# 所以"存活回合数→代币→落盘"这条乘法在字节面上一个 witness 都没有——改坏了照样 15 条 SAME。
# 这条只按 `e`（放弃行动、把回合交出去），让战斗自己打到"我方烛尽"或 30 回合上限，于是结语那行的
# `+N 代币（余额 M）` 用的是**战斗自己数出来的回合数**（不是测试手工设的数），随后 `progress` 把同一个档读回来：
# 两个读数同场出现＝发放、落盘、报告三段咬上了；对不上就是断在中间某处。
# 与 p9/p12/p13 同一口径：md5 **随日历换**（今日种子 ⇒ 今日规则 ⇒ 今日这场的打法），那是日历在走。
p16(){ { printf 'go\n'; printf 'e\n%.0s' $(seq 1 60); } | "$B" daily --save "$O/d16.save" > "$O/p16.txt" 2>&1;
       "$B" progress --save "$O/d16.save" >> "$O/p16.txt" 2>&1; }
# p17 消费面（升级收费）装的那条**花钱**的整场。p1–p16 里没有一条能在字节面上看见"扣了 10 枚"：
# 唯一进结算环的场景全走 `play`，而 play 没有存档槽 ⇒ 余额恒为 0（`PlayHost::tokens`），升级一律 ✖ 代币不足；
# 每日那四条又永远进不了结算环（`has_next: false` ⇒ 提前 return）。所以"改了价目而 16 条全 SAME"这种假绿此刻无牙。
# 这条的形状是"先让二进制自己写出第 1 关入口快照，再拿同一份档续打"：
# ① `go` ⇒ 准备阶段出口那次 `snapshot_entry` 落盘，战斗内 EOF 收手（一行输出都不留）；
# ② 把档上那个 `tokens=0` 手改成 25 —— **不是**夹具作弊：主线不发币，钱包只能从每日挣，
#    而"先跑一场 daily"会让这条的 md5 随日历漂（p13/p16 已经各占一条日历依赖）。本条要钉的是扣款这笔算术，不是发放。
# ③ 同一份档 `--resume` 真打赢第 1 关（这套脚本 seed=1 在第 11 回合把对面烛打掉，换成别的 seed 会输 ⇒ 换不得）；
# ④ 结算阶段 `up 0 power` 花掉 10，`go` 提交 —— 钱与牌走同一个提交点＝「下一关入口快照」；第 2 关准备阶段 `q` 弃局，
#    此后一个字节都不动（若第 2 关也被打赢，档面会再被下一关快照盖一次，那条 ✖ 就不是本条要钉的东西了）；
# ⑤ 最后 `progress` 把档读回来：「代币=15」与「第 2 关」同场出现 ⇒ 扣款、提交、报告三段咬上，对不上就是断在中间。
# 与 p13/p16 不同：**这条不随日历换 md5**（seed 钉死 1，不碰每日那三键）。
p17(){ printf 'go\n' | "$B" mainline --seed 1 --save "$O/m17.kv" >/dev/null 2>&1;
       sed -i 's/^tokens=0$/tokens=25/' "$O/m17.kv";
       grep -q '^tokens=25$' "$O/m17.kv" || { echo "p17：档面上那个 tokens 键没改成 25 ⇒ 二进制没写出第 1 关入口快照（别拿一份读不懂的档冒充钱包）" >&2; exit 1; }
       { printf 'go\n'; for _ in $(seq 1 11); do printf 'p 0 P1\np 0 P2\np 0 P3\np 0 P4\nsh 0\nsh 0\ne\n'; done;
         printf 'up 0 power\ngo\nq\n'; } | "$B" mainline --seed 1 --resume --save "$O/m17.kv" > "$O/p17.txt" 2>&1;
       "$B" progress --save "$O/m17.kv" >> "$O/p17.txt" 2>&1; }
for f in p1 p2 p3 p4 p5 p6 p7 p8 p9 p10 p11 p12 p13 p14 p15 p16 p17; do $f; done
# 取 md5 前先把**输出目录**从文件内容里抠掉：p15/p16/p17 三条会把「存档路径：<目录>」原样印进输出里，
# 而双臂必然跑在两个不同目录下 ⇒ 不归一就换 md5，量出来的是"脚本换了个临时目录"，不是行为漂移。
# 这一步此前只写在人话注释里、没进代码，于是同一挂陷阱能被同一个人踩两次（第十六步·其九 实测两次假 DIFF）。
for f in "$O"/p*.txt; do printf '%s  %s\n' "$(sed "s#$O#<OUT>#g" "$f" | md5sum | cut -d' ' -f1)" "${f#"$O"/}"; done
