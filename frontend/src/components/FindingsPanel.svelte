<script>
  export let findings = [];
  const KIND_LABEL = {
    low_temperature: '低温事件',
    clock_drift: '时钟差',
    evidence_gap: '证据缺口',
    flow_drop: '流量突降',
    divert_feedback_late: '分流反馈迟到',
    instrument_label_stale: '校准标签未更新',
    holding_volume_stale: '保持管容积滞后',
    product_held: '停机滞留',
    passage_over_divert: '通过窗跨分流',
    config_missing: '配置缺失',
    recirculation_unmetered: '回流量未计',
    balance_tank_mixing: '平衡罐两批混合',
    recirculation_across_cleaning: '回流跨清洗边界',
    first_passage_temp_missing: '首次通过缺温度',
    partial_recirculation_drawoff: '部分回流料未取用',
  };
  const SEV_LABEL = { info: '提示', warning: '警告', critical: '严重' };
  const fmt = (iso) => (iso ? new Date(iso).toLocaleTimeString() : '');
  // 证据来源:优先取 detail.evidence(通道/位号/安装位置),其次 detail.position
  const srcOf = (f) => {
    const d = f.detail || {};
    if (d.evidence && d.evidence.positions) {
      const positions = d.evidence.positions.join('、');
      const sensors = (d.evidence.sensors || []).join(', ');
      return `${positions}(${sensors})`;
    }
    if (d.position) return d.position;
    return '—';
  };
</script>

{#if findings.length === 0}
  <p class="none">暂无发现。运行"验证检查"后在此展示。</p>
{:else}
  <table>
    <thead>
      <tr><th>级别</th><th>类型</th><th>窗口</th><th>说明</th><th>证据来源</th></tr>
    </thead>
    <tbody>
      {#each findings as f}
        <tr class={f.severity}>
          <td><span class="badge {f.severity}">{SEV_LABEL[f.severity] || f.severity}</span></td>
          <td>{KIND_LABEL[f.kind] || f.kind}</td>
          <td class="win">{#if f.window}{fmt(f.window[0])} – {fmt(f.window[1])}{:else}—{/if}</td>
          <td>{f.message}</td>
          <td class="src">{srcOf(f)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th, td { text-align: left; padding: 6px 8px; border-bottom: 1px solid #eaeef2; vertical-align: top; }
  .badge { border-radius: 10px; padding: 1px 8px; font-size: 12px; }
  .badge.critical { background: #ffebe9; color: #cf222e; }
  .badge.warning { background: #fff8c5; color: #9a6700; }
  .badge.info { background: #ddf4ff; color: #0969da; }
  tr.critical { background: #fff8f8; }
  .win { white-space: nowrap; color: #57606a; font-size: 12px; }
  .src { font-size: 12px; color: #57606a; }
  .none { color: #57606a; font-size: 13px; }
</style>
