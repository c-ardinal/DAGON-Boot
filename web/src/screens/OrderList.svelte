<script lang="ts">
  import { ORDER } from '../app/copy.ts';
  import type { Config } from '../app/sample.ts';

  interface Props {
    config: Config | null;
    selected: string | null;
    onselect: (key: string) => void;
    /** One-column layout: the list folds under its heading. */
    fold: boolean;
    /** During an update: each target node's percent (the one place for it). */
    percent?: Record<string, number>;
  }

  let { config, selected, onselect, fold, percent = {} }: Props = $props();

  const rows = $derived(config ? config.order.map((id) => config.nodes.find((n) => n.id === id)!) : []);
</script>

{#snippet list()}
  {#if rows.length}
    <ol>
      {#each rows as n (n.id)}
        {@const key = `node:${n.id}`}
        {@const p = percent[n.id]}
        <li>
          <button aria-pressed={selected === key} onclick={() => onselect(key)}>
            <span class="row"><span>[{n.no}] {n.name}</span>{#if p !== undefined}<span class="pct">{p}%</span>{/if}</span>
            {#if p !== undefined}<progress max="100" value={p} aria-label="{n.name} {p}%"></progress>{/if}
          </button>
        </li>
      {/each}
    </ol>
  {:else}
    <p class="none">{ORDER.none}</p>
  {/if}
{/snippet}

{#if fold}
  <details class="wrap">
    <summary><h2>{ORDER.heading}</h2></summary>
    {@render list()}
  </details>
{:else}
  <section class="wrap" aria-labelledby="order-h">
    <h2 id="order-h">{ORDER.heading}</h2>
    {@render list()}
  </section>
{/if}

<style>
  .wrap {
    padding: var(--s-5) 0;
  }
  summary,
  section > h2 {
    padding: 0 var(--s-5);
  }
  summary {
    cursor: pointer;
  }
  summary h2 {
    display: inline;
  }
  h2 {
    margin: 0;
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
    letter-spacing: 0.02em;
    font-feature-settings: 'palt';
  }
  ol {
    margin: var(--s-3) 0 0;
    padding: 0;
    list-style: none;
  }
  li + li {
    border-top: 1px solid var(--c-line);
  }
  li button {
    display: block;
    width: 100%;
    padding: var(--s-3) var(--s-5);
    border: 0;
    background: none;
    text-align: left;
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
    cursor: pointer;
    outline: 3px solid transparent;
    outline-offset: -3px;
  }
  li button[aria-pressed='true'] {
    background: var(--c-layer-2);
    box-shadow: inset 4px 0 0 var(--c-paper);
  }
  li button:focus-visible {
    outline-color: var(--c-paper);
  }
  @media (hover: hover) and (pointer: fine) {
    li button:not([aria-pressed='true']):hover {
      background: var(--c-hover);
    }
  }
  .row {
    display: flex;
    justify-content: space-between;
    gap: var(--s-3);
  }
  .pct {
    font: 400 var(--t-mono-size) / var(--t-body-lh) var(--f-mono);
    font-variant-numeric: tabular-nums;
  }
  progress {
    display: block;
    width: 100%;
    height: 4px;
    margin-top: var(--s-2);
    border: 0;
    appearance: none;
    background: var(--c-layer-2);
  }
  progress::-webkit-progress-bar {
    background: var(--c-layer-2);
  }
  progress::-webkit-progress-value {
    background: var(--c-paper);
  }
  .none {
    margin: var(--s-3) 0 0;
    padding: 0 var(--s-5);
    color: var(--c-muted);
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
</style>
