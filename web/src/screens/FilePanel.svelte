<script lang="ts">
  import { FILE } from '../app/copy.ts';
  import type { ReleaseFile } from '../app/operate.svelte.ts';

  interface Props {
    file: ReleaseFile;
    /** Nodes the update runs on. */
    targets: number;
  }

  let { file, targets }: Props = $props();

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
  {#if file.verdict === 'pass'}
    <p class="pair"><span class="label">{FILE.targets}</span> {targets}</p>
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
  .pair {
    margin-top: var(--s-5);
    font-variant-numeric: tabular-nums;
  }
</style>
