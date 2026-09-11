<script>
  import { onMount, onDestroy } from 'svelte';
  import { api, setAuth } from './api.js';
  import Timeline from './components/Timeline.svelte';
  import FindingsPanel from './components/FindingsPanel.svelte';
  import PassageView from './components/PassageView.svelte';
  import ConfigPanel from './components/ConfigPanel.svelte';
  import EventBar from './components/EventBar.svelte';

  let role = 'operator';
  let user = 'op-1';
  $: setAuth(role, user);

  let batches = [];
  let selected = null;
  let timeline = null;
  let error = '';
  let productName = '全脂牛奶 3.2%';
  let timer = null;

  async function refreshBatches() {
    try { batches = await api.batches(); } catch (e) { error = e.message; }
  }
  async function refreshTimeline() {
    if (selected == null) return;
    try { timeline = await api.timeline(selected); error = ''; }
    catch (e) { error = e.message; }
  }
  async function selectBatch(id) { selected = id; await refreshTimeline(); }
  async function createBatch() {
    try {
      const b = await api.createBatch(productName);
      await refreshBatches();
      await selectBatch(b.id);
    } catch (e) { error = e.message; }
  }
  async function closeBatch() {
    try { await api.closeBatch(selected); await refreshBatches(); await refreshTimeline(); }
    catch (e) { error = e.message; }
  }
  async function analyze() {
    try { await api.analyze(selected); error = ''; }
    catch (e) { error = e.message; }
    await refreshTimeline();
  }

  onMount(async () => {
    await refreshBatches();
    timer = setInterval(() => { refreshBatches(); refreshTimeline(); }, 5000);
  });
  onDestroy(() => clearInterval(timer));
</script>

<header>
  <h1>巴氏杀菌验证平台 <small>只读镜像 · 不输出设定值 · 不控制阀门</small></h1>
  <div class="rolebar">
    <label>角色
      <select bind:value={role}>
        <option value="operator">操作员</option>
        <option value="engineer">验证工程师</option>
      </select>
    </label>
    <label>工号 <input bind:value={user} size="8" /></label>
  </div>
</header>

{#if error}<div class="error">{error}</div>{/if}

<div class="layout">
  <aside>
    <h2>产品批</h2>
    <div class="newbatch">
      <input bind:value={productName} placeholder="产品名称" />
      <button on:click={createBatch}>开批</button>
    </div>
    <ul class="batchlist">
      {#each batches as b}
        <li>
          <button class:sel={selected === b.id} on:click={() => selectBatch(b.id)}>
            #{b.id} {b.product_name}
            <small>{b.status === 'active' ? '进行中' : '已关闭'} · {new Date(b.started_at).toLocaleTimeString()}</small>
          </button>
        </li>
      {/each}
    </ul>
    <ConfigPanel {role} />
  </aside>

  <main>
    {#if timeline}
      <section class="bar">
        <EventBar batch={timeline.batch} on:marked={refreshTimeline} />
        <div class="actions">
          {#if timeline.batch.status === 'active'}
            <button on:click={closeBatch}>关批</button>
          {/if}
          <button class="primary" on:click={analyze}>运行验证检查</button>
        </div>
      </section>

      <section>
        <h2>热历程时间线 <small>温度(绿)/ 流量(蓝)/ 分流(红底)/ 事件(紫虚线)/ 发现(橙底)</small></h2>
        <Timeline
          batch={timeline.batch}
          temps={timeline.temps}
          flows={timeline.flows}
          diverts={timeline.diverts}
          events={timeline.events}
          findings={timeline.findings}
          spec={timeline.spec}
        />
        {#if timeline.tube_config}
          <p class="meta">冻结容积:{timeline.tube_config.volume_l} L(#{timeline.tube_config.id},{timeline.tube_config.frozen_by} 冻结)
            {#if timeline.spec}· 冻结限值:{timeline.spec.min_hold_temp_c} °C{/if}</p>
        {:else}
          <p class="meta warn">尚无冻结的保持管容积 —— 通过窗无法重建</p>
        {/if}
      </section>

      <section>
        <h2>保持段通过窗重建</h2>
        <PassageView batch={timeline.batch} />
      </section>

      <section>
        <h2>验证发现({timeline.findings.length})</h2>
        <FindingsPanel findings={timeline.findings} />
      </section>
    {:else}
      <p class="empty">选择或新建一个产品批。</p>
    {/if}
  </main>
</div>

<style>
  :global(body) { font-family: system-ui, "PingFang SC", "Microsoft YaHei", sans-serif; margin: 0; background: #f6f8fa; color: #1f2328; }
  header { display: flex; justify-content: space-between; align-items: center; padding: 10px 18px; background: #0d3b66; color: #fff; }
  header h1 { font-size: 18px; margin: 0; }
  header small { font-weight: normal; font-size: 12px; opacity: .8; margin-left: 8px; }
  .rolebar { display: flex; gap: 12px; align-items: center; font-size: 13px; }
  .error { background: #ffebe9; color: #cf222e; padding: 8px 16px; border-bottom: 1px solid #ff818266; }
  .layout { display: grid; grid-template-columns: 300px 1fr; gap: 14px; padding: 14px; }
  aside { background: #fff; border: 1px solid #d0d7de; border-radius: 8px; padding: 12px; height: fit-content; }
  h2 { font-size: 14px; margin: 10px 0 8px; }
  h2 small { font-weight: normal; color: #57606a; }
  .newbatch { display: flex; gap: 6px; }
  .newbatch input { flex: 1; }
  .batchlist { list-style: none; padding: 0; margin: 10px 0; max-height: 260px; overflow: auto; }
  .batchlist button { width: 100%; text-align: left; background: none; border: 1px solid transparent; border-radius: 6px; padding: 6px 8px; cursor: pointer; display: block; }
  .batchlist button.sel { background: #ddf4ff; border-color: #54aeff; }
  .batchlist small { display: block; color: #57606a; }
  main section { background: #fff; border: 1px solid #d0d7de; border-radius: 8px; padding: 12px; margin-bottom: 14px; }
  .bar { display: flex; justify-content: space-between; align-items: center; gap: 10px; flex-wrap: wrap; }
  .actions { display: flex; gap: 8px; }
  button { border: 1px solid #d0d7de; background: #f6f8fa; border-radius: 6px; padding: 6px 12px; cursor: pointer; font-size: 13px; }
  button.primary { background: #1f6feb; border-color: #1f6feb; color: #fff; }
  .meta { font-size: 12px; color: #57606a; }
  .meta.warn { color: #cf222e; }
  .empty { color: #57606a; padding: 40px; text-align: center; }
</style>
