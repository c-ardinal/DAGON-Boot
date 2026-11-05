<script lang="ts">
  import { FILE, PLAN } from '../app/copy.ts';
  import type { ReleaseFile } from '../app/operate.svelte.ts';

  interface Props {
    file: ReleaseFile;
    /** About how long writing the checked nodes takes (目安). */
    minutes?: number;
    /** The sheet's one primary (while the sheet is open it holds the emphasis): writing, or the next step. */
    go?: Act | null;
    /** The sheet's other actions (another file), outlined, before the primary. */
    others?: Act[];
    /** Each node the package can be written to: its check, 今 → 新 and where it is (進み). */
    nodes?: SheetNode[];
    /** A node checked or unchecked; unchecked nodes are not written. */
    onchoose?: (id: string, on: boolean) => void;
    /** Writing has started: the checks stay as they are. */
    locked?: boolean;
  }
  interface SheetNode {
    id: string;
    name: string;
    from: string;
    to: string;
    checked: boolean;
    stage: string;
  }
  interface Act {
    label: string;
    off: boolean;
    control?: string;
    onclick: () => void;
  }

  let { file, minutes, go = null, others = [], nodes = [], onchoose, locked = false }: Props = $props();
</script>

<!-- A refused package is told in a modal (its reason and next step), never as rows here. -->
<section class="file" aria-labelledby="file-h">
  <h2 id="file-h" class="title" tabindex="-1">{FILE.heading}</h2>
  <p class="name"><span translate="no">{file.name}</span><span class="size">{FILE.size(file.size)}</span></p>
  {#if file.verdict === 'checking'}
    <p class="muted">{FILE.checking}</p>
  {/if}
  {#if file.verdict === 'pass' && minutes}
    <p class="pair"><span class="label">{PLAN.estimateLabel}</span> {PLAN.minutes(minutes)}</p>
  {/if}
  {#if file.verdict === 'pass' && nodes.length}
    <table class="nodes">
      <thead><tr>{#each [FILE.nodes.node, FILE.nodes.now, FILE.nodes.next, FILE.nodes.progress] as c (c)}<th scope="col">{c}</th>{/each}</tr></thead>
      <tbody>
        {#each nodes as n (n.id)}
          <tr data-node={n.id}>
            <th scope="row">
              <label><input type="checkbox" aria-label={n.name} checked={n.checked} disabled={locked} onchange={(e) => onchoose?.(n.id, e.currentTarget.checked)} /><span>{n.name}</span></label>
            </th>
            <td translate="no">{n.from}</td>
            <td translate="no">{n.to}</td>
            <td>{n.stage}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
  {#if go || others.length}
    <div class="acts">
      {#each others as o (o.label)}
        <button class="other" data-control={o.control} aria-disabled={o.off} onclick={() => !o.off && o.onclick()}>{o.label}</button>
      {/each}
      {#if go}<button class="go" data-control={go.control} aria-disabled={go.off} onclick={() => !go.off && go.onclick()}>{go.label}</button>{/if}
    </div>
  {/if}
</section>

<style>
  .file {
    padding: var(--s-5);
  }
  .title {
    margin: 0 0 var(--s-4);
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
    letter-spacing: 0.02em;
  }
  .title:focus {
    outline: none;
  }
  .name {
    display: flex;
    justify-content: space-between;
    gap: var(--s-3);
    margin: 0 0 var(--s-5);
    padding: var(--s-3) var(--s-4);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
    overflow-wrap: anywhere;
  }
  .size,
  .label,
  .muted {
    color: var(--c-muted);
  }
  .size {
    white-space: nowrap;
  }
  p {
    margin: 0;
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  /* Per Node (the design's sheet table): the check sits with the node's name. */
  .nodes {
    width: 100%;
    margin-top: var(--s-5);
    border-collapse: collapse;
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  .nodes th,
  .nodes td {
    padding: var(--s-2) var(--s-3) var(--s-2) 0;
    text-align: left;
    font-weight: 400;
  }
  .nodes thead th {
    color: var(--c-muted);
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .nodes td[translate] {
    font-family: var(--f-mono);
    font-variant-numeric: tabular-nums;
  }
  .nodes label {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    cursor: pointer;
  }
  .nodes input {
    margin: 0;
    accent-color: var(--c-accent);
  }
  /* Button/Primary: the sheet's one emphasised action, full width. */
  .acts {
    display: grid;
    gap: var(--s-2);
    margin-top: var(--s-5);
  }
  .go,
  .other {
    width: 100%;
    height: var(--h-button);
    border: 0;
    border-radius: var(--r-m);
    background: var(--c-accent);
    color: var(--c-ink);
    font: 700 var(--t-body) / 1 var(--f-ja);
    cursor: pointer;
  }
  /* Button/Secondary: the same shape, outlined. */
  .other {
    border: 1px solid var(--c-line);
    background: none;
    color: var(--c-paper);
  }
  .go[aria-disabled='true'],
  .other[aria-disabled='true'] {
    background: var(--c-layer-2);
    color: var(--c-muted);
    cursor: default;
  }
  .go:focus-visible,
  .other:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
  .pair {
    margin-top: var(--s-5);
    font-variant-numeric: tabular-nums;
  }
</style>
