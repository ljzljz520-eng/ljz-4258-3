<script>
  import { api } from '../api.js';
  // 审核证据清单:逐条核对"这条温度/流量/分流样本属于哪个产品批、哪个安装位置"。
  // 证据边界 = 批次窗口;每行都显式标注批次归属与传感器安装位置。
  export let batch;

  const CHANNELS = [
    ['all', '全部'],
    ['temperature', '温度'],
    ['flow', '流量'],
    ['divert', '分流'],
  ];
  const CH_LABEL = { temperature: '温度', flow: '流量', divert: '分流' };

  let channel = 'all';
  let rows = [];
  let total = 0;
  let truncated = false;
  let windowRange = null;
  let loading = false;
  let loaded = false;
  let loadedFor = null;
  let error = '';

  async function load() {
    if (!batch) return;
    loading = true;
    try {
      const res = await api.evidence(batch.id, channel);
      rows = res.rows;
      total = res.total;
      truncated = res.truncated;
      windowRange = [res.window_start, res.window_end];
      loadedFor = batch.id;
      loaded = true;
      error = '';
    } catch (e) {
      error = e.message;
    }
    loading = false;
  }
  function pick(ch) {
    channel = ch;
    load();
  }

  $: stale = loaded && batch && loadedFor !== batch.id;
  const fmt = (iso) => (iso ? new Date(iso).toLocaleTimeString() : '—');
  const fmtVal = (r) =>
    r.channel === 'divert' ? (r.value === 'divert' ? '分流' : '前向') : Number(r.value).toFixed(1);
</script>

<div class="evidence">
  <div class="toolbar">
    <span class="hint">通道:</span>
    {#each CHANNELS as [ch, label]}
      <button class:sel={channel === ch} on:click={() => pick(ch)}>{label}</button>
    {/each}
    <button class="primary" on:click={load} disabled={loading}>
      {loading ? '加载中…' : loaded ? '刷新证据清单' : '加载证据清单'}
    </button>
    {#if error}<span class="err">{error}</span>{/if}
  </div>

  {#if stale}
    <p class="warn">批次已切换 —— 当前清单属于批次 #{loadedFor},请重新加载。</p>
  {/if}

  {#if loaded && windowRange}
    <p class="boundary">
      证据边界:批次 <b>#{batch.id}</b>「{batch.product_name}」 · 窗口 {fmt(windowRange[0])} – {fmt(windowRange[1])}
      · 共 {total} 条{#if truncated}(仅显示前 {rows.length} 条){/if}
    </p>
    {#if rows.length === 0}
      <p class="none">该通道在批次窗口内没有样本。</p>
    {:else}
      <div class="scroll">
        <table>
          <thead>
            <tr>
              <th>#</th><th>设备时间</th><th>通道</th><th>位号</th><th>安装位置</th>
              <th>数值</th><th>时钟差 s</th><th>所属批次</th>
            </tr>
          </thead>
          <tbody>
            {#each rows as r, i}
              <tr>
                <td class="idx">{i + 1}</td>
                <td class="t">{fmt(r.device_time)}</td>
                <td>{CH_LABEL[r.channel] || r.channel}</td>
                <td class="mono">{r.sensor_id}</td>
                <td class:unregistered={r.position === '未登记位置'}>{r.position}</td>
                <td class="num">{fmtVal(r)}{r.unit !== '—' ? ' ' + r.unit : ''}</td>
                <td class="num">{r.skew_s.toFixed(1)}</td>
                <td class="batch">#{r.batch_id} {r.product_name}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {:else if !loading}
    <p class="none">点击"加载证据清单",逐条核对本批温度/流量/分流样本的批次归属与安装位置。</p>
  {/if}
</div>

<style>
  .toolbar { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; margin-bottom: 8px; }
  .toolbar button.sel { background: #ddf4ff; border-color: #54aeff; }
  .hint { font-size: 12px; color: #57606a; }
  .boundary { font-size: 12px; color: #1f2328; background: #ddf4ff; border: 1px solid #54aeff66; border-radius: 6px; padding: 6px 10px; }
  .warn { font-size: 12px; color: #9a6700; background: #fff8c5; border-radius: 6px; padding: 6px 10px; }
  .err { color: #cf222e; font-size: 12px; }
  .none { color: #57606a; font-size: 13px; }
  .scroll { max-height: 320px; overflow: auto; border: 1px solid #eaeef2; border-radius: 6px; }
  table { width: 100%; border-collapse: collapse; font-size: 12px; }
  th, td { text-align: left; padding: 4px 8px; border-bottom: 1px solid #eaeef2; white-space: nowrap; }
  thead th { position: sticky; top: 0; background: #f6f8fa; }
  .idx { color: #57606a; }
  .t { font-variant-numeric: tabular-nums; }
  .mono { font-family: ui-monospace, monospace; }
  .num { text-align: right; font-variant-numeric: tabular-nums; }
  .batch { color: #57606a; }
  .unregistered { color: #cf222e; font-weight: 600; }
</style>
