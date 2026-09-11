#!/usr/bin/env bash
# 端到端演示:冻结配置 → 开批 → 事件 → 通知 → 分析 → 通过窗
set -euo pipefail
B=${BASE:-http://localhost:3000/api}
ENG=(-H 'content-type: application/json' -H 'x-role: engineer' -H 'x-user: wang')
OP=(-H 'content-type: application/json' -H 'x-role: operator' -H 'x-user: li')
JSON=(-H 'content-type: application/json')

echo "1) 工程师冻结:验证规格 / 保持管容积 / 仪表版本"
curl -s -X POST $B/configs/spec "${ENG[@]}" -d '{"min_hold_temp_c":72.0}' > /dev/null
curl -s -X POST $B/configs/holding-tube "${ENG[@]}" -d '{"volume_l":120.0}' > /dev/null
curl -s -X POST $B/configs/instruments "${ENG[@]}" \
  -d '{"instrument_id":"TT-101","label":"TT-101/A","calibrated_at":"2025-09-01T00:00:00Z"}' > /dev/null

echo "2) 开批"
BID=$(curl -s -X POST $B/batches "${JSON[@]}" -d '{"product_name":"全脂牛奶 3.2%"}' | jq -r .id)
echo "   batch #$BID"

echo "3) 操作员标记事件"
curl -s -X POST $B/batches/$BID/events "${OP[@]}" -d '{"kind":"changeover","note":"水换奶"}' > /dev/null
curl -s -X POST $B/batches/$BID/events "${OP[@]}" -d '{"kind":"sample"}' > /dev/null

echo "4) 工程通知:探头已校准(标签未更新)+ 保持管已改造(容积未更新)"
curl -s -X POST $B/notices "${JSON[@]}" -d '{"kind":"probe_recalibrated","instrument_id":"TT-101"}' > /dev/null
curl -s -X POST $B/notices "${JSON[@]}" -d '{"kind":"tube_modified"}' > /dev/null

echo "5) 等待遥测积累 30s 后运行验证检查"
sleep 30
curl -s -X POST $B/batches/$BID/analyze | jq -r '.[] | "[\(.severity)] \(.kind): \(.message)"'

echo "6) 通过窗重建(批次开始时刻进入)"
ENTRY=$(curl -s $B/batches/$BID | jq -r .started_at)
curl -s "$B/batches/$BID/passage?entry=$ENTRY" | jq .

echo "7) 再循环产品链(通过尝试 + 最终去向全部热暴露)"
curl -s $B/batches/$BID/recirculation \
  | jq '{attempts: [.chain.attempts[] | {seq, forward_l, returned_l, exposure: (.exposure != null)}],
         forward_volume_l: .chain.forward_volume_l,
         returned_volume_l: .chain.returned_volume_l,
         pending_return_l: .chain.pending_return_l,
         findings: [.findings[] | {kind, severity}]}'
