<script lang="ts">
  import { DIFF, FILE, PLAN } from '../app/copy.ts';
  import type { DiffRow } from '../app/define/diff.ts';
  import type { ReleaseFile } from '../app/operate.svelte.ts';

  interface Props {
    file: ReleaseFile;
    /** Nodes the update runs on. */
    targets: number;
    /** About how long the update takes (案, M21's 目安). */
    minutes?: number;
    /** §7.6: how the file's 構成 differs (L17); empty otherwise. */
    diff?: DiffRow[];
    /** The sheet's one primary (while the sheet is open it holds the emphasis): writing, or the next step. */
    go?: Act | null;
    /** The sheet's other actions (another file, fixing the 構成 in 定義), outlined, before the primary. */
    others?: Act[];
  }
  interface Act {
    label: string;
    off: boolean;
    control?: string;
    onclick: () => void;
  }

  let { file, targets, minutes, diff = [], go = null, others = [] }: Props = $props();

  /** dagon-host checks the source first, then the configuration: a source refusal leaves the configuration unchecked. */
  const rows = $derived.by((): [string, string, boolean][] => {
    switch (file.verdict) {
      case 'checking':
      case 'format':
        return [];
      case 'source':
      case 'key':
        return [[FILE.source, FILE.sourceBad, true]];
      case 'topology':
        return [
          [FILE.source, FILE.sourceOk, false],
          [FILE.topology, FILE.topologyBad, true],
        ];
      case 'pass':
        return [
          [FILE.source, FILE.sourceOk, false],
          [FILE.topology, FILE.topologyOk, false],
        ];
    }
  });
</script>

<section class="file" aria-labelledby="file-h">
  <h2 id="file-h" class="title" tabindex="-1">{FILE.heading}</h2>
  <p class="name"><span translate="no">{file.name}</span><span class="size">{FILE.size(file.size)}</span></p>
  {#if file.verdict === 'checking'}
    <p class="muted">{FILE.checking}</p>
  {:else if file.verdict === 'format'}
    <p class="bad">{FILE.notRelease}</p>
  {:else}
    <dl>
      {#each rows as [label, value, bad] (label)}
        <dt>{label}</dt>
        <dd class:bad>{value}</dd>
      {/each}
    </dl>
  {/if}
  {#if diff.length}
    <table class="diff">
      <caption>{DIFF.heading}</caption>
      <thead><tr>{#each DIFF.cols as c (c)}<th scope="col">{c}</th>{/each}</tr></thead>
      <tbody>
        {#each diff as r, i (i)}
          <tr><th scope="row">{r.what}</th><td translate="no">{r.here}</td><td translate="no">{r.file}</td></tr>
        {/each}
      </tbody>
    </table>
  {/if}
  {#if file.verdict === 'pass'}
    <p class="pair">
      <span class="label">{FILE.targets}</span> {targets}{#if minutes}&emsp;<span class="label">{PLAN.estimateLabel}</span> {PLAN.minutes(minutes)}{/if}
    </p>
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
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2) var(--s-6);
    margin: 0;
    padding-bottom: var(--s-5);
    border-bottom: 1px solid var(--c-line);
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  dt {
    color: var(--c-muted);
  }
  dd {
    margin: 0;
  }
  .bad {
    color: var(--c-signal);
    font-weight: 700;
  }
  p {
    margin: 0;
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  .diff {
    width: 100%;
    margin-top: var(--s-5);
    border-collapse: collapse;
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .diff caption {
    margin-bottom: var(--s-3);
    text-align: left;
    font: 700 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  .diff th,
  .diff td {
    padding: var(--s-2) var(--s-3) var(--s-2) 0;
    border-bottom: 1px solid var(--c-line);
    text-align: left;
    vertical-align: top;
  }
  .diff thead th {
    color: var(--c-muted);
    font-weight: 400;
    white-space: nowrap;
  }
  .diff tbody th {
    min-width: 5em;
  }
  .diff td {
    font-family: var(--f-mono);
    overflow-wrap: anywhere;
  }
  .diff td:last-child {
    color: var(--c-signal);
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
