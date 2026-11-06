<script lang="ts">
  import { FLASH, NOTICE, WRITE, fill } from '../app/copy.ts';
  import { jobReason, type Job } from '../app/define/cli.svelte.ts';
  import Modal from './Modal.svelte';

  interface Props {
    /** The node written directly: its label ([1] メイン) and what it is. */
    node: { label: string; target: string };
    /** How this MCU is connected for writing, from its definition; null when the definitions say nothing. */
    how: { label: string; hint: string } | null;
    /** serve can write this MCU itself. */
    canWrite: boolean;
    /** A 子機: connected directly because the 親機 does not reach it. */
    child?: boolean;
    /** The flash job once started. */
    job: Job | null;
    onwrite: () => void;
    oncancel: () => void;
    onclose: () => void;
    onmore: () => void;
  }

  let { node, how, canWrite, child = false, job, onwrite, oncancel, onclose, onmore }: Props = $props();

  /** Writing has started once serve runs the job: from then on it cannot be stopped. */
  const writing = $derived(job?.state === 'running');
  const done = $derived(job?.state === 'succeeded');
  const failed = $derived(job?.state === 'failed');
  const chip = $derived(
    done
      ? ({ text: WRITE.state.done, kind: 'ok' } as const)
      : failed
        ? ({ text: WRITE.failed, kind: 'error' } as const)
        : writing
          ? ({ text: WRITE.state.writing, kind: 'active' } as const)
          : job
            ? ({ text: WRITE.factory.waiting, kind: 'active' } as const)
            : ({ text: WRITE.factory.first, kind: 'info' } as const),
  );
</script>

<!-- First (or factory) writing over USB: only for a node whose DAGON does not answer. -->
<Modal title={WRITE.run} {chip} target={node.target} onclose={writing ? undefined : onclose} {onmore}>
  <p>{fill(child ? WRITE.factory.childIntro : WRITE.factory.intro, { node: node.label })}</p>
  <h3>{child ? WRITE.factory.howToConnect : WRITE.factory.steps}</h3>
  {#if how}
    <p><b>{how.label}</b>{canWrite ? `　${how.hint}` : ''}</p>
  {/if}
  {#if !canWrite}<p>{FLASH.otherTools}</p>{/if}
  {#if failed}
    <h3>{NOTICE.reason}</h3>
    <p class="mono">{jobReason(job)}</p>
  {/if}
  <h3>{NOTICE.after}</h3>
  <p>{WRITE.factory.after}</p>
  {#snippet actions()}
    {#if canWrite && !done}
      <!-- 中止 only before writing starts: stopping half way can leave the node unable to start. -->
      <button class="button-secondary" aria-disabled={writing} title={writing ? WRITE.factory.cancelLocked : undefined} onclick={() => !writing && oncancel()}>{WRITE.factory.cancel}</button>
      <button class="button-primary" data-control="flash" aria-disabled={!!job && !failed} onclick={() => (!job || failed) && onwrite()}>{failed ? WRITE.retry : WRITE.run}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  p {
    margin: 0;
  }
  h3 {
    margin: 0;
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  .mono {
    font: 400 var(--t-mono) / 24px var(--f-mono);
    overflow-wrap: anywhere;
  }
</style>
