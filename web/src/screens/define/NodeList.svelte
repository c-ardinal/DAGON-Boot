<script lang="ts" module>
  /** A part of the parts window: a board (product) or an MCU on an own board. */
  export interface Part {
    key: { board: string } | { mcu: string };
    name: string;
  }
</script>

<script lang="ts">
  import { DEFINE, PARTS } from '../../app/copy.ts';
  import type { Define } from '../../app/define/define.svelte.ts';
  import { canPlace, maxNodes, mcus } from '../../app/define/model.ts';
  import { dagonReady, supportedBoards } from '../../wasm/dagon.ts';
  import Icon from '../Icon.svelte';

  interface Props {
    d: Define;
    /** A drag from a row started: the page follows the pointer and places it on release. */
    ondrag?: (part: Part, e: PointerEvent) => void;
    /** × : the window closes ("＋ 部品" opens it again). */
    onclose?: () => void;
  }

  let { d, ondrag, onclose }: Props = $props();
  // The first paint does not wait for dagon-wasm: the lists fill once it is there.
  let boards = $state<ReturnType<typeof supportedBoards>>([]);
  let mcuList = $state<ReturnType<typeof mcus>>([]);
  $effect(() => {
    void dagonReady()
      .then(() => {
        boards = supportedBoards();
        mcuList = mcus();
      })
      .catch(() => undefined);
  });
  let q = $state('');
  /** 製品 / マイコン: one list each (one long list stops scaling as boards and MCUs grow). */
  let tab = $state<'product' | 'mcu'>('product');
  const match = (...t: string[]) => !q.trim() || t.some((s) => s.toLowerCase().includes(q.trim().toLowerCase()));
  const full = $derived(d.doc.topo.nodes.length >= maxNodes());
  const used = (key: Part['key']) => d.doc.topo.nodes.filter((n) => ('board' in key ? n.board === key.board : !n.board && n.mcu === key.mcu)).length;

  /** Enter, double-click (and the 置く of the table) place without a pointer: as "置く" always did. */
  function placeNow(p: Part) {
    if ('board' in p.key ? !canPlace(d.doc, p.key.board) : full) return;
    if ('board' in p.key) d.place(p.key.board, p.name);
    else d.placeMcu(p.key.mcu, p.name);
  }
</script>

{#snippet row(p: Part, sub: string, ok: boolean)}
  {@const n = used(p.key)}
  <li>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="part"
      role="button"
      tabindex="0"
      data-control="edit"
      aria-disabled={!ok}
      aria-label="{p.name} {DEFINE.place}"
      onpointerdown={(e) => ok && e.button === 0 && ondrag?.(p, e)}
      ondblclick={() => ok && placeNow(p)}
      onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), ok && placeNow(p))}
    >
      <Icon name="grip-vertical" size="var(--i-s)" />
      <span class="n"><span translate="no" title={p.name}>{p.name}</span><small translate="no">{sub}</small></span>
      <span class="used">{n ? DEFINE.count(n) : ''}</span>
    </div>
  </li>
{/snippet}

<section class="list" aria-labelledby="list-h">
  <div class="float-h">
    <h2 id="list-h" tabindex="-1">{PARTS.heading}</h2>
    {#if onclose}<button class="button-icon" title={PARTS.close} aria-label={PARTS.close} onclick={onclose}><span class="layer"><Icon name="x" /></span></button>{/if}
  </div>
  <div class="tabs" role="tablist" aria-label={PARTS.heading}>
    {#each [['product', DEFINE.byProduct], ['mcu', DEFINE.byMcuOwn]] as const as [t, label] (t)}
      <button role="tab" aria-selected={tab === t} aria-controls="parts-rows" onclick={() => (tab = t)}>{label}</button>
    {/each}
  </div>
  <label class="search"><Icon name="search" size="var(--i-s)" /><input bind:value={q} placeholder={DEFINE.search} aria-label={DEFINE.search} /></label>
  <div class="rows" id="parts-rows" role="tabpanel">
    <ul>
      {#if tab === 'product'}
        {#each boards.filter((b) => match(b.product, b.mcuName)) as b (b.id)}
          {@render row({ key: { board: b.id }, name: b.product }, b.mcuName, canPlace(d.doc, b.id))}
        {/each}
      {:else}
        {#each mcuList.filter((m) => match(m.name)) as m (m.id)}
          {@render row({ key: { mcu: m.id }, name: DEFINE.ownBoard }, m.name, !full)}
        {/each}
      {/if}
    </ul>
  </div>
</section>

<style>
  .list {
    display: flex;
    flex-direction: column;
    max-height: inherit;
    min-height: 0;
  }
  .list {
    gap: var(--s-3);
    padding: var(--s-4);
  }
  .float-h {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    color: var(--c-paper);
    font: 700 var(--t-small) / 24px var(--f-ja);
  }
  .float-h .button-icon {
    margin: calc(-1 * var(--p-hit));
  }
  .tabs {
    display: flex;
    border-bottom: 1px solid var(--c-layer-2);
  }
  [role='tab'] {
    height: var(--h-row);
    padding: 0 var(--s-4);
    border: 0;
    background: none;
    color: var(--c-muted);
    font: 700 var(--t-small) / 1 var(--f-ja);
    cursor: pointer;
  }
  [role='tab'][aria-selected='true'] {
    color: var(--c-paper);
    box-shadow: inset 0 -2px 0 var(--c-paper);
  }
  [role='tab']:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: calc(-1 * var(--st-w-focus));
  }
  h2 {
    margin: 0;
    font: inherit;
  }
  h2:focus {
    outline: none;
  }
  .search {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--s-2);
    height: var(--h-button);
    padding: 0 var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    color: var(--c-muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    color: var(--c-paper);
    font: 400 var(--t-small) / 1 var(--f-ja);
    outline: none;
  }
  .search:focus-within {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
  .rows {
    min-height: 0;
    overflow: auto;
    padding-bottom: var(--s-2);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .part {
    display: grid;
    grid-template-columns: auto 1fr auto;
    color: var(--c-paper);
    align-items: center;
    gap: 0.375rem;
    min-height: 2rem;
    margin: 0 var(--s-1);
    padding: 0.1875rem var(--s-2);
    border-radius: var(--r-s);
    cursor: grab;
    user-select: none;
    touch-action: none;
  }
  .part[aria-disabled='true'] {
    opacity: 0.45;
    cursor: not-allowed;
  }
  @media (hover: hover) and (pointer: fine) {
    .part:hover {
      background: var(--c-layer-2);
    }
  }
  .part:focus-visible {
    outline: 2px solid var(--c-paper);
    outline-offset: -2px;
  }
  .n {
    display: grid;
    min-width: 0;
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  .n > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  small {
    color: var(--c-muted);
    font: 500 var(--t-caption) / 16px var(--f-mono);
  }
  .used {
    color: var(--c-muted);
    font-size: var(--t-11);
  }
  @media (max-width: 767px), (pointer: coarse) {
    .part {
      min-height: 44px;
      min-width: 44px;
    }
  }
</style>
