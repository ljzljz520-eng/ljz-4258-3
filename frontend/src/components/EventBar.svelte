<script>
  import { createEventDispatcher } from 'svelte';
  import { api } from '../api.js';
  // 操作员仅能标记这四类事件(后端类型系统同样约束)
  export let batch;
  const dispatch = createEventDispatcher();
  let note = '';
  let msg = '';
  const KINDS = [
    ['changeover', '换料'],
    ['cycle', '循环'],
    ['shutdown', '停机'],
    ['sample', '取样'],
  ];
  async function mark(kind) {
    try {
      await api.addEvent(batch.id, kind, note || null);
      msg = '';
      note = '';
      dispatch('marked');
    } catch (e) { msg = e.message; }
  }
</script>

<div class="eventbar">
  <span class="label">事件标记:</span>
  {#each KINDS as [kind, label]}
    <button on:click={() => mark(kind)} disabled={batch.status !== 'active'}>{label}</button>
  {/each}
  <input bind:value={note} placeholder="备注(可选)" size="16" />
  {#if msg}<span class="err">{msg}</span>{/if}
</div>

<style>
  .eventbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .label { font-size: 13px; color: #57606a; }
  .err { color: #cf222e; font-size: 12px; }
</style>
