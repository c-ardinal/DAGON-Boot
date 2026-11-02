<script lang="ts">
  import { DEFINE } from '../../app/copy.ts';
  import type { Define } from '../../app/define/define.svelte.ts';
  import { canPlace } from '../../app/define/model.ts';
  import { supportedBoards } from '../../wasm/dagon.ts';

  interface Props {
    d: Define;
    onconnect: () => void;
  }

  let { d, onconnect }: Props = $props();
  const boards = supportedBoards();
  const placed = (id: string) => d.doc.topo.nodes.some((n) => n.board === id);
</script>

<section class="list" aria-labelledby="list-h">
  <h2 id="list-h">{DEFINE.listHeading}</h2>
  <p class="help">{DEFINE.listHelp}</p>
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
  .first {
    display: grid;
    gap: var(--s-3);
  }
</style>
