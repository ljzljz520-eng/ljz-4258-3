<script>
  import { api } from '../api.js';
  // 通过窗重建:拖动"进入时刻",平台按冻结容积对实际流量积分,给出出口时刻
  export let batch;
  let offsetS = 0;
  let result = null;
  let error = '';

  $: t0 = batch ? new Date(batch.started_at).getTime() : 0;
  $: t1 = batch ? new Date(batch.ended_at || Date.now()).getTime() : 0;
  $: spanS = Math.max(Math.floor((t1 - t0) / 1000), 1);
  $: entryIso = new Date(t0 + offsetS * 1000).toISOString();

  let deb;
  $: if (batch && batch.id) scheduleQuery(offsetS, batch.id);
  function scheduleQuery(off, id) {
    clearTimeout(deb);
    deb = setTimeout(async () => {
      try {
        result = await api.passage(id, new Date(t0 + off * 1000).toISOString());
        error = '';
      } catch (e) { error = e.message; result = null; }
    }, 150);
  }
  const fmt = (iso) => (iso ? new Date(iso).toLocaleTimeString() : '—');
</script>

<div class="passage">
  <label>
    进入保持管时刻:{new Date(entryIso).toLocaleTimeString()
    }<input type="range" min="0" max={spanS} bind:value={offsetS} />
  </label>
  {#if error}<p class="err">{error}</p>{/if}
  {#if result}
    <div class="cards">
      <div class="card">
        <h3>重建结果</h3>
        {#if result.result.status === 'exited'}
          <p>出口时刻:<b>{fmt(result.result.exit)}</b></p>
          <p>停留时间:<b>{result.result.residence_s.toFixed(1)} s</b></p>
        {:else if result.result.status === 'held_in_tube'}
          <p class="bad">产品滞留管内(已累计 {result.result.accumulated_l.toFixed(1)} L)</p>
        {:else}
          <p class="bad">数据不足(已累计 {result.result.accumulated_l.toFixed(1)} L)</p>
        {/if}
      </div>
      <div class="card">
        <h3>可解释性</h3>
        {#if result.interpretable}
          <p class="ok">✓ 通过窗有效,热历程可解释</p>
        {:else}
          <p class="bad">✗ 不可解释:{result.obstruction || '未知原因'}</p>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .passage label { display: block; font-size: 13px; }
  .passage input[type='range'] { width: 100%; margin-top: 6px; }
  .cards { display: flex; gap: 12px; margin-top: 10px; }
  .card { flex: 1; border: 1px solid #d0d7de; border-radius: 8px; padding: 10px 14px; }
  .card h3 { margin: 0 0 6px; font-size: 13px; color: #57606a; }
  .ok { color: #1a7f37; } .bad { color: #cf222e; } .err { color: #cf222e; font-size: 13px; }
</style>
