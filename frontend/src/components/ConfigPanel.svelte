<script>
  import { onMount } from 'svelte';
  import { api } from '../api.js';
  // 验证工程师冻结配置:保持管容积 / 验证规格 / 仪表版本;并录入工程通知
  export let role;
  $: isEngineer = role === 'engineer';

  let volume = 120;
  let minTemp = 72.0;
  let skew = 1.0, gap = 10.0, divertDelay = 2.0, dropRatio = 0.6;
  let instrId = 'TT-101', instrLabel = 'TT-101/A', instrCal = new Date().toISOString().slice(0, 16);
  let configs = [], instruments = [], spec = null, notices = [];
  let msg = '';

  async function refresh() {
    try {
      configs = await api.tubeConfigs();
      instruments = await api.instruments();
      spec = await api.activeSpec();
      notices = await api.notices();
    } catch { /* 后端未就绪时忽略 */ }
  }
  onMount(refresh);

  async function guard(fn) {
    try { await fn(); msg = ''; await refresh(); }
    catch (e) { msg = e.message; }
  }
  const freezeTube = () => guard(() => api.freezeTube(Number(volume), null));
  const freezeSpec = () => guard(() => api.freezeSpec({
    min_hold_temp_c: Number(minTemp),
    max_clock_skew_s: Number(skew),
    max_gap_s: Number(gap),
    max_divert_feedback_s: Number(divertDelay),
    flow_drop_ratio: Number(dropRatio),
  }));
  const freezeInstr = () => guard(() => api.freezeInstrument({
    instrument_id: instrId, label: instrLabel,
    calibrated_at: new Date(instrCal).toISOString(),
  }));
  const notice = (kind) => guard(() => api.addNotice({
    kind, instrument_id: kind === 'probe_recalibrated' ? instrId : null, message: '',
  }));
</script>

<div class="cfg">
  <h2>冻结配置 {#if !isEngineer}<small>(工程师角色可写)</small>{/if}</h2>
  {#if msg}<p class="err">{msg}</p>{/if}

  <fieldset disabled={!isEngineer}>
    <legend>保持管容积(L)</legend>
    <input type="number" bind:value={volume} step="0.1" min="0" />
    <button on:click={freezeTube}>冻结</button>
  </fieldset>

  <fieldset disabled={!isEngineer}>
    <legend>验证规格(限值/阈值,平台不生成)</legend>
    <label>低温限值 °C <input type="number" bind:value={minTemp} step="0.1" size="6" /></label>
    <label>时钟差 s <input type="number" bind:value={skew} step="0.1" size="4" /></label>
    <label>缺口 s <input type="number" bind:value={gap} step="1" size="4" /></label>
    <label>分流反馈 s <input type="number" bind:value={divertDelay} step="0.1" size="4" /></label>
    <label>流量比 <input type="number" bind:value={dropRatio} step="0.05" size="4" /></label>
    <button on:click={freezeSpec}>冻结</button>
  </fieldset>

  <fieldset disabled={!isEngineer}>
    <legend>仪表版本</legend>
    <label>位号 <input bind:value={instrId} size="8" /></label>
    <label>标签 <input bind:value={instrLabel} size="10" /></label>
    <label>校准时间 <input type="datetime-local" bind:value={instrCal} /></label>
    <button on:click={freezeInstr}>冻结</button>
  </fieldset>

  <fieldset>
    <legend>工程通知(维护事实录入)</legend>
    <button on:click={() => notice('probe_recalibrated')}>探头已校准</button>
    <button on:click={() => notice('tube_modified')}>保持管已改造</button>
  </fieldset>

  <div class="state">
    <p><b>当前冻结容积:</b>{configs.length ? configs[configs.length - 1].volume_l + ' L' : '—'}</p>
    <p><b>当前规格:</b>{spec ? spec.min_hold_temp_c + ' °C' : '未冻结'}</p>
    <p><b>仪表版本:</b>{instruments.length} 条 · <b>通知:</b>{notices.length} 条</p>
  </div>
</div>

<style>
  .cfg { border-top: 1px solid #eaeef2; margin-top: 12px; padding-top: 8px; }
  fieldset { border: 1px solid #d0d7de; border-radius: 6px; margin-bottom: 8px; font-size: 12px; }
  fieldset label { display: inline-flex; gap: 4px; align-items: center; margin: 2px 6px 2px 0; }
  fieldset input { max-width: 130px; }
  fieldset:disabled { opacity: 0.55; }
  legend { font-size: 12px; color: #57606a; }
  .state { font-size: 12px; color: #57606a; }
  .state p { margin: 3px 0; }
  .err { color: #cf222e; font-size: 12px; }
</style>
