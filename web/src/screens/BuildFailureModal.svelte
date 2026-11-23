<script lang="ts">
  // A build that failed: the node and file it stopped at, why, and what to do next.
  import { BUILD, NOTICE, SKELETON, fill } from '../app/copy.ts';
  import type { BuildFailure } from '../app/jobFlow.ts';
  import Modal from './Modal.svelte';

  let {
    failure: f,
    target,
    startFailed,
    linked,
    onclose,
    onmore,
    onskeleton,
    onask,
    onretry,
  }: {
    failure: BuildFailure;
    /** The node it stopped at, as `[n] name · nodes/<id>/`. */
    target?: string;
    /** Serve would not take one of the build's jobs. */
    startFailed: boolean;
    /** DAGON's agent is linked: it can be asked why. */
    linked: boolean;
    onclose: () => void;
    onmore: () => void;
    onskeleton: () => void;
    onask: (at: Element) => void;
    onretry: () => void;
  } = $props();
</script>

<Modal title={BUILD.title} chip={{ text: BUILD.state.failed, kind: 'error' }} {target} {onclose} {onmore}>
  <h3 class="modal-h">{NOTICE.reason}</h3>
  {#if f.skeleton}
    <p>{f.skeleton === 'none' ? BUILD.noSkeleton.reason : BUILD.noSkeleton.stale}</p>
    <h3 class="modal-h">{NOTICE.fix}</h3>
    <p>{BUILD.noSkeleton.fix}</p>
  {:else}
    <p class="modal-mono">{startFailed ? BUILD.startFailed : f.reason}</p>
    <h3 class="modal-h">{NOTICE.fix}</h3>
    <p>{f.file ? fill(BUILD.fail.fix, { file: f.file, line: f.line ?? 0 }) : BUILD.fail.plain}{f.others ? fill(BUILD.fail.others, { n: f.others }) : ''}</p>
  {/if}
  {#snippet actions()}
    {#if f.skeleton}
      <button class="button-primary" data-control="gen" onclick={onskeleton}>{SKELETON.make}</button>
    {:else if linked}
      <button class="button-secondary" data-control="ask" onclick={(e) => onask(e.currentTarget)}>{BUILD.askAgent}</button>
    {/if}
    {#if !f.skeleton}<button class="button-primary" data-control="build" onclick={onretry}>{BUILD.retry}</button>{/if}
  {/snippet}
</Modal>

<style>
  .modal-mono {
    margin: 0;
    font: 400 var(--t-mono) / 24px var(--f-mono);
    overflow-wrap: anywhere;
  }
</style>
