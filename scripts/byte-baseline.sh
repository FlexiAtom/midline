#!/usr/bin/env bash
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
for f in p1 p2 p3 p4 p5 p6 p7 p8 p9 p10; do $f; done
md5sum "$O"/p*.txt | sed "s#$O/##"
