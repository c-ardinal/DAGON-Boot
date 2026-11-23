<script lang="ts">
  // A package that cannot be written: why (the 構成 differences when it is built for another
  // 構成) and what to do, with the band's buttons.
  import type { Button } from '../app/band.ts';
  import { DIFF, FILE, NOTICE } from '../app/copy.ts';
  import type { DiffRow } from '../app/define/diff.ts';
  import type { Action } from '../app/machine.ts';
  import type { ReleaseFile } from '../app/writeProgress.ts';
  import BandButtons from './BandButtons.svelte';
  import Modal from './Modal.svelte';

  let {
    file,
    diff,
    fix,
    secondary,
    primary,
    onclose,
    onpress,
  }: {
    file: ReleaseFile;
    diff: DiffRow[];
    /** What to do, in one sentence. */
    fix: string;
    secondary: Button[];
    primary: Button | null;
    onclose: () => void;
    onpress: (a: Action) => void;
  } = $props();

  const word = $derived(file.verdict === 'topology' ? FILE.topologyBad : file.verdict === 'format' ? FILE.notRelease : FILE.sourceBad);
</script>

<Modal title={FILE.heading} chip={{ text: word, kind: 'error' }} target={file.name} {onclose}>
  {#if diff.length}
    <table class="modal-diff">
      <caption>{DIFF.heading}</caption>
      <thead><tr>{#each DIFF.cols as c (c)}<th scope="col">{c}</th>{/each}</tr></thead>
      <tbody>
        {#each diff as r, i (i)}
          <tr><th scope="row">{r.what}</th><td translate="no">{r.here}</td><td translate="no">{r.file}</td></tr>
        {/each}
      </tbody>
    </table>
  {/if}
  <h3 class="modal-h">{NOTICE.fix}</h3>
  <p>{fix}</p>
  {#snippet actions()}
    <BandButtons {secondary} {primary} {onpress} />
  {/snippet}
</Modal>

<style>
  /* The 構成 differences (the table the sheet had). */
  .modal-diff {
    width: 100%;
    border-collapse: collapse;
    font: 400 var(--t-caption) / 16px var(--f-ja);
  }
  .modal-diff caption {
    margin-bottom: var(--s-2);
    text-align: left;
    font-weight: 700;
  }
  .modal-diff th,
  .modal-diff td {
    padding: var(--s-2) var(--s-3) var(--s-2) 0;
    border-bottom: 1px solid var(--c-line);
    text-align: left;
    vertical-align: top;
  }
  .modal-diff td {
    font-family: var(--f-mono);
    overflow-wrap: anywhere;
  }
</style>
