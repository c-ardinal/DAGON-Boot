<script lang="ts">
  import { DEFINE, INSPECT } from '../../app/copy.ts';
  import type { Define } from '../../app/define/define.svelte.ts';
  import { canPlace, mcus } from '../../app/define/model.ts';
  import { supportedBoards } from '../../wasm/dagon.ts';

  interface Props {
    d: Define;
    onconnect: () => void;
  }

  let { d, onconnect }: Props = $props();
  const boards = supportedBoards();
  const mcuList = mcus();
  let by = $state<'product' | 'mcu'>('product');
  const full = $derived(d.doc.topo.nodes.length >= 8);
  const placed = (id: string) => d.doc.topo.nodes.some((n) => n.board === id);
</script>

<section class="list" aria-labelledby="list-h">
  <h2 id="list-h">{DEFINE.listHeading}</h2>
  <p class="help">{DEFINE.listHelp}</p>
  <div class="by" role="radiogroup" aria-label={DEFINE.chooseBy}>
    <button role="radio" aria-checked={by === 'product'} onclick={() => (by = 'product')}>{DEFINE.byProduct}</button>
    <button role="radio" aria-checked={by === 'mcu'} onclick={() => (by = 'mcu')}>{DEFINE.byMcu}</button>
  </div>
  {#if by === 'mcu'}
    <ul>
      {#each mcuList as m (m.id)}
        <li>
          <span class="what"><span translate="no">{m.name}</span><span class="mcu"><span class="k">{INSPECT.board}</span> {INSPECT.boardOwn}</span></span>
          <button class="small" aria-disabled={full} aria-label="{m.name} {DEFINE.place}" onclick={() => !full && d.placeMcu(m.id, m.name)}>{DEFINE.place}</button>
        </li>
      {/each}
    </ul>
  {:else}
  <ul>
    {#each boards as b (b.id)}
      {@const ok = canPlace(d.doc, b.id)}
      <li>
        <span class="what"><span translate="no">{b.product}</span><span class="mcu" translate="no">{b.mcuName}</span></span>
        {#if placed(b.id)}
          <span class="placed">{DEFINE.placed}</span>
        {:else}
          <button class="small" aria-disabled={!ok} aria-label="{b.product} {DEFINE.place}" onclick={() => ok && d.place(b.id, b.product)}>{DEFINE.place}</button>
        {/if}
      </li>
    {/each}
  </ul>
  {/if}
  <h3>{DEFINE.firstHeading}</h3>
  <div class="first">
    <button class="secondary" onclick={() => d.placeSample()}>{DEFINE.placeSample}</button>
    <button class="secondary" onclick={onconnect}>{DEFINE.connectDiscover}</button>
  </div>
</section>

<style>
  .list {
    padding: var(--s-5);
  }
  h2 {
    margin: 0;
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
  }
  h3 {
    margin: var(--s-5) 0 var(--s-3);
    color: var(--c-muted);
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  .help {
    margin: var(--s-2) 0 var(--s-4);
    color: var(--c-muted);
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  ul {
    display: grid;
    gap: var(--s-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-3);
    padding: var(--s-3) var(--s-4);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
  }
  .what {
    display: grid;
    font: 500 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
    overflow-wrap: anywhere;
  }
  .mcu {
    color: var(--c-muted);
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
  }
  .placed {
    flex: none;
    padding: var(--s-1) var(--s-3);
    border-radius: var(--r-s);
    background: var(--c-layer-2);
    color: var(--c-muted);
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .small {
    flex: none;
    min-height: 2rem;
    padding: 0 var(--s-3);
    border: 1px solid var(--c-paper);
    border-radius: var(--r-s);
    background: none;
    color: var(--c-paper);
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
    cursor: pointer;
  }
  .small[aria-disabled='true'] {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .by {
    display: flex;
    margin-bottom: var(--s-4);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
  }
  .by button {
    flex: 1;
    padding: var(--s-2);
    border: 0;
    background: none;
    color: var(--c-paper);
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
    cursor: pointer;
  }
  .by button[aria-checked='true'] {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .k {
    color: var(--c-muted);
  }
  .first {
    display: grid;
    gap: var(--s-3);
  }
</style>
