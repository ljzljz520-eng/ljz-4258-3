<script>
  // 热历程时间线:温度/流量曲线 + 分流带 + 事件标记 + 发现窗口
  export let batch;
  export let temps = [];
  export let flows = [];
  export let diverts = [];
  export let events = [];
  export let findings = [];
  export let spec = null;

  const W = 940, H = 300, PL = 46, PR = 52, PT = 16, PB = 26;
  const KIND_LABEL = { changeover: '换料', cycle: '循环', shutdown: '停机', sample: '取样' };

  $: t0 = batch ? new Date(batch.started_at).getTime() : 0;
  $: t1 = batch ? new Date(batch.ended_at || Date.now()).getTime() : 1;
  $: span = Math.max(t1 - t0, 1);
  $: x = (iso) => PL + ((new Date(iso).getTime() - t0) / span) * (W - PL - PR);

  $: tempVals = temps.map((s) => s.value);
  $: tempLo = Math.min(...tempVals, spec ? spec.min_hold_temp_c : 75) - 0.5;
  $: tempHi = Math.max(...tempVals, 76) + 0.5;
  $: yT = (v) => PT + (1 - (v - tempLo) / Math.max(tempHi - tempLo, 0.1)) * (H - PT - PB);

  $: flowHi = Math.max(...flows.map((s) => s.value), 1) * 1.1;
  $: yF = (v) => PT + (1 - v / flowHi) * (H - PT - PB);

  $: tempPath = temps.map((s, i) => `${i ? 'L' : 'M'}${x(s.device_time).toFixed(1)},${yT(s.value).toFixed(1)}`).join(' ');
  $: flowPath = flows.map((s, i) => `${i ? 'L' : 'M'}${x(s.device_time).toFixed(1)},${yF(s.value).toFixed(1)}`).join(' ');

  // 分流区间(阶梯信号 → 红底带)
  $: divertBands = (() => {
    const bands = [];
    let cur = null;
    for (const d of diverts) {
      if (d.value === 'divert' && !cur) cur = d.device_time;
      if (d.value === 'forward' && cur) { bands.push([cur, d.device_time]); cur = null; }
    }
    if (cur) bands.push([cur, new Date(t1).toISOString()]);
    return bands;
  })();

  $: ticks = (() => {
    const out = [];
    const n = 6;
    for (let i = 0; i <= n; i++) out.push(new Date(t0 + (span * i) / n));
    return out;
  })();
</script>

<svg viewBox="0 0 {W} {H}" width="100%" role="img" aria-label="热历程时间线">
  <rect x={PL} y={PT} width={W - PL - PR} height={H - PT - PB} fill="#fbfcfd" stroke="#d0d7de" />

  {#each divertBands as [a, b]}
    <rect x={x(a)} y={PT} width={Math.max(x(b) - x(a), 1.5)} height={H - PT - PB} fill="#ffe1e1" />
  {/each}

  {#each findings as f}
    {#if f.window}
      <rect
        x={x(f.window[0])} y={PT}
        width={Math.max(x(f.window[1]) - x(f.window[0]), 2)} height={H - PT - PB}
        fill={f.severity === 'critical' ? '#ff7b7240' : '#fff8c5'}
        stroke={f.severity === 'critical' ? '#cf222e' : '#d4a72c'}
        stroke-width="0.6"
      />
    {/if}
  {/each}

  {#if spec}
    <line x1={PL} x2={W - PR} y1={yT(spec.min_hold_temp_c)} y2={yT(spec.min_hold_temp_c)} stroke="#cf222e" stroke-dasharray="5 3" stroke-width="1" />
    <text x={PL + 4} y={yT(spec.min_hold_temp_c) - 3} font-size="10" fill="#cf222e">限值 {spec.min_hold_temp_c}°C</text>
  {/if}

  {#if tempPath}<path d={tempPath} fill="none" stroke="#1a7f37" stroke-width="1.6" />{/if}
  {#if flowPath}<path d={flowPath} fill="none" stroke="#0969da" stroke-width="1.4" />{/if}

  {#each events as ev}
    <line x1={x(ev.at)} x2={x(ev.at)} y1={PT} y2={H - PB} stroke="#8250df" stroke-dasharray="3 3" />
    <text x={x(ev.at) + 3} y={PT + 11} font-size="10" fill="#8250df">{KIND_LABEL[ev.kind] || ev.kind}</text>
  {/each}

  {#each ticks as tk}
    <text x={PL + ((tk.getTime() - t0) / span) * (W - PL - PR)} y={H - 8} font-size="10" fill="#57606a" text-anchor="middle">
      {tk.toLocaleTimeString()}
    </text>
  {/each}
  <text x="6" y={PT + 10} font-size="10" fill="#1a7f37">°C</text>
  <text x={W - PR + 6} y={PT + 10} font-size="10" fill="#0969da">L/h</text>
</svg>
