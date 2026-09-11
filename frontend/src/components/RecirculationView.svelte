<script>
  // 再循环产品链:每次回到平衡罐形成新的通过尝试;
  // 最终去向保留链上全部热暴露 —— 不得只显示最后一次合格段。
  export let data; // { batch, chain, findings }

  const fmt = (iso) => (iso ? new Date(iso).toLocaleTimeString() : '—');
  const L = (v) => `${Number(v).toFixed(1)} L`;
  const SEV_LABEL = { info: '提示', warning: '警告', critical: '严重' };
  const KIND_LABEL = {
    recirculation_unmetered: '回流量未计',
    balance_tank_mixing: '平衡罐两批混合',
    recirculation_across_cleaning: '回流跨清洗边界',
    first_passage_temp_missing: '首次通过缺温度',
    partial_recirculation_drawoff: '部分回流料未取用',
  };
</script>

{#if data && data.chain}
  <div class="balance">
    <div class="stat">
      <h3>前向去灌装机</h3>
      <b>{L(data.chain.forward_volume_l)}</b>
    </div>
    <div class="stat">
      <h3>分流回平衡罐</h3>
      <b>{L(data.chain.returned_volume_l)}</b>
    </div>
    <div class="stat" class:warn={data.chain.pending_return_l > 0.5}>
      <h3>罐内未走完回流料</h3>
      <b>{L(data.chain.pending_return_l)}</b>
    </div>
  </div>

  <h3>通过尝试(每次回到平衡罐 → 新的通过尝试)</h3>
  <table>
    <thead>
      <tr><th>次数</th><th>窗口</th><th>前向</th><th>回流</th><th>热暴露(温度证据)</th></tr>
    </thead>
    <tbody>
      {#each data.chain.attempts as a}
        <tr class:missing={!a.exposure}>
          <td class="seq">第 {a.seq} 次</td>
          <td class="win">{fmt(a.window[0])} – {fmt(a.window[1])}</td>
          <td class="num">{L(a.forward_l)}</td>
          <td class="num">{a.returned_l > 0.05 ? L(a.returned_l) : '—'}</td>
          <td>
            {#if a.exposure}
              最低 {a.exposure.min_c.toFixed(1)}°C · 平均 {a.exposure.avg_c.toFixed(1)}°C
              <span class="samples">({a.exposure.samples} 样本)</span>
            {:else}
              <span class="bad">缺温度记录 —— 该段热暴露无法验证</span>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>

  <div class="final">
    <h3>最终去向:保留全部 {data.chain.attempts.length} 次通过的热暴露</h3>
    <p class="rule">
      不得只显示最后一次合格段 —— 回流料再进入后与新产品混合,
      最终产品可能经历过 1–{data.chain.attempts.length} 次通过;任一通过缺证据都必须显式标注。
    </p>
    <ol>
      {#each data.chain.attempts as a}
        <li class:missing={!a.exposure}>
          第 {a.seq} 次通过({fmt(a.window[0])} – {fmt(a.window[1])}):
          {#if a.exposure}
            最低 {a.exposure.min_c.toFixed(1)}°C / 平均 {a.exposure.avg_c.toFixed(1)}°C
          {:else}
            <span class="bad">无温度记录,热暴露无法验证</span>
          {/if}
        </li>
      {/each}
    </ol>
  </div>

  {#if data.findings.length}
    <h3>链级发现({data.findings.length})</h3>
    <ul class="findings">
      {#each data.findings as f}
        <li class={f.severity}>
          <span class="badge {f.severity}">{SEV_LABEL[f.severity] || f.severity}</span>
          <b>{KIND_LABEL[f.kind] || f.kind}</b> — {f.message}
        </li>
      {/each}
    </ul>
  {/if}
{:else}
  <p class="none">加载中…</p>
{/if}

<style>
  .balance { display: flex; gap: 12px; margin-bottom: 10px; }
  .stat { flex: 1; border: 1px solid #d0d7de; border-radius: 8px; padding: 8px 12px; }
  .stat h3 { margin: 0 0 4px; font-size: 12px; color: #57606a; font-weight: normal; }
  .stat b { font-size: 16px; }
  .stat.warn { border-color: #d4a72c; background: #fff8c5; }
  h3 { font-size: 13px; margin: 12px 0 6px; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th, td { text-align: left; padding: 5px 8px; border-bottom: 1px solid #eaeef2; }
  .seq { white-space: nowrap; }
  .win { white-space: nowrap; color: #57606a; font-size: 12px; }
  .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .samples { color: #57606a; font-size: 12px; }
  tr.missing { background: #fff8f8; }
  .bad { color: #cf222e; }
  .final { border: 1px solid #54aeff66; background: #ddf4ff33; border-radius: 8px; padding: 10px 14px; margin-top: 12px; }
  .final h3 { margin-top: 0; }
  .rule { font-size: 12px; color: #57606a; margin: 0 0 8px; }
  .final ol { margin: 0; padding-left: 20px; font-size: 13px; }
  .final li.missing { color: #cf222e; }
  .findings { list-style: none; padding: 0; margin: 0; font-size: 13px; }
  .findings li { padding: 4px 0; border-bottom: 1px solid #eaeef2; }
  .badge { border-radius: 10px; padding: 1px 8px; font-size: 12px; }
  .badge.critical { background: #ffebe9; color: #cf222e; }
  .badge.warning { background: #fff8c5; color: #9a6700; }
  .badge.info { background: #ddf4ff; color: #0969da; }
  .none { color: #57606a; font-size: 13px; }
</style>
