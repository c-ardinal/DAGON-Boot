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
  import { canPlace, MAX_NODES, mcus } from '../../app/define/model.ts';
  import { dagonReady, supportedBoards } from '../../wasm/dagon.ts';

  interface Props {
    d: Define;
    /** A drag from a row started (DES-UX3-030): the page follows the pointer and places it on release. */
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
  const match = (...t: string[]) => !q.trim() || t.some((s) => s.toLowerCase().includes(q.trim().toLowerCase()));
  const full = $derived(d.doc.topo.nodes.length >= MAX_NODES);
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
      <span class="n"><span translate="no">{p.name}</span><small translate="no">{sub}</small></span>
      <span class="used">{n ? DEFINE.count(n) : ''}</span>
    </div>
  </li>
{/snippet}

<section class="list" aria-labelledby="list-h">
  <div class="float-h">
    <h2 id="list-h" tabindex="-1">{PARTS.heading}</h2>
    <span class="hint">{PARTS.hint}</span>
    {#if onclose}<button class="x" title={PARTS.close} aria-label={PARTS.close} onclick={onclose}>×</button>{/if}
  </div>
  <input class="search" bind:value={q} placeholder={DEFINE.search} aria-label={DEFINE.search} />
  <div class="rows">
    <h3>{DEFINE.byProduct}</h3>
    <ul>
      {#each boards.filter((b) => match(b.product, b.mcuName)) as b (b.id)}
        {@render row({ key: { board: b.id }, name: b.product }, b.mcuName, canPlace(d.doc, b.id))}
      {/each}
    </ul>
    <h3>{DEFINE.byMcuOwn}</h3>
    <ul>
      {#each mcuList.filter((m) => match(m.name)) as m (m.id)}
        {@render row({ key: { mcu: m.id }, name: DEFINE.ownBoard(m.name) }, m.name, !full)}
      {/each}
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
  .float-h {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--s-2);
    height: 1.75rem;
    padding: 0 0.375rem 0 var(--s-3);
    border-bottom: 1px solid var(--c-hair);
    color: var(--c-muted);
    font: 700 var(--t-12) / 1 var(--f-ja);
  }
  h2 {
    margin: 0;
    font: inherit;
  }
  h2:focus {
    outline: none;
  }
  .hint {
    font-weight: 400;
  }
  .x {
    width: 1.5rem;
    height: 1.5rem;
    margin-left: auto;
    border: 0;
    border-radius: var(--r-full);
    background: none;
    color: var(--c-muted);
    cursor: pointer;
  }
  .search {
    flex: none;
    height: 1.5rem;
    margin: var(--s-2) var(--s-2) var(--s-1);
    padding: 0 var(--s-2);
    border: 1px solid var(--c-hair);
    border-radius: var(--r-full);
    background: var(--c-layer-1);
    color: var(--c-paper);
    font: 400 var(--t-12) / 1 var(--f-ja);
  }
  .rows {
    min-height: 0;
    overflow: auto;
    padding-bottom: var(--s-2);
  }
  h3 {
    margin: 0;
    padding: 0.375rem var(--s-3) 2px;
    color: var(--c-muted);
    font: 400 var(--t-11) / 1.3 var(--f-ja);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .part {
    display: grid;
    grid-template-columns: 1fr auto;
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
    font: 400 var(--t-13) / 1.3 var(--f-ja);
  }
  .n > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  small {
    color: var(--c-muted);
    font: 500 var(--t-11) / 1.2 var(--f-mono);
  }
  .used {
    color: var(--c-muted);
    font-size: var(--t-11);
  }
  @media (max-width: 767px), (pointer: coarse) {
    .part,
    .x {
      min-height: 44px;
      min-width: 44px;
    }
  }
</style>
