<script lang="ts">
  import { NOTICE, SKELETON } from '../app/copy.ts';
  import { jobReason, type Job } from '../app/define/cli.svelte.ts';
  import Modal from './Modal.svelte';

  interface Props {
    /** The gen job; null until serve has taken it. */
    job: Job | null;
    /** Serve would not take the job: why. */
    failure?: string | null;
    /** Where the project is (the modal's target line). */
    place: string;
    /** The nodes the skeleton is made for: [label, folder]. */
    nodes: [string, string][];
    onclose: () => void;
    oncancel: () => void;
    onretry: () => void;
    onmore: () => void;
  }

  let { job, failure = null, place, nodes, onclose, oncancel, onretry, onmore }: Props = $props();

  /** What dagon gen did not make (it still ends well): each line says what and why. */
  const skipped = $derived(((job?.result as { skipped?: string[] } | null)?.skipped ?? []).map((l) => l.replace(/^skipped\s+/, '')));
  const state = $derived(
    failure || skipped.length || job?.state === 'failed' || job?.state === 'cancelled' ? 'failed' : job?.state === 'succeeded' ? 'done' : 'running',
  );
  /** The job's last line is its reason (serve writes why it stopped last). */
  const reason = $derived(failure ?? (skipped.length ? skipped.join('\n') : jobReason(job)));
  const chip = $derived(
    state === 'done'
      ? ({ text: SKELETON.state.done, kind: 'ok' } as const)
      : state === 'failed'
        ? ({ text: SKELETON.state.failed, kind: 'error' } as const)
        : ({ text: SKELETON.state.running, kind: 'active' } as const),
  );
</script>

<!-- 雛形を生成する: running (what is being made, 中止 only), success (where it was made, what to do next), failure (reason, fix, もう一度生成する). -->
<Modal title={SKELETON.title} {chip} target={place} onclose={state === 'running' ? undefined : onclose} {onmore}>
  {#if state === 'running'}
    <ul>
      {#each nodes as [label, folder] (folder)}
        <li><span>{label}</span><span class="mono folder" translate="no">{folder}</span></li>
      {/each}
    </ul>
  {:else if state === 'done'}
    <h3>{SKELETON.where}</h3>
    <ul>
      {#each nodes as [label, folder] (folder)}
        <li><span>{label}</span><span class="mono folder" translate="no">{folder}</span></li>
      {/each}
    </ul>
    <h3>{SKELETON.next.heading}</h3>
    <ol>
      <li>{SKELETON.next[1]}</li>
      <li>{SKELETON.next[2]}</li>
    </ol>
  {:else}
    <h3>{NOTICE.reason}</h3>
    <p class="mono">{reason}</p>
    <h3>{NOTICE.fix}</h3>
    <p>{SKELETON.fail.fix}</p>
  {/if}
  {#snippet actions()}
    {#if state === 'running'}
      <button class="button-secondary" onclick={oncancel}>{SKELETON.cancel}</button>
    {:else if state === 'failed'}
      <button class="button-primary" onclick={onretry}>{SKELETON.retry}</button>
    {/if}
  {/snippet}
</Modal>

<style>
  h3 {
    margin: 0;
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  p {
    margin: 0;
  }
  .mono {
    color: var(--c-muted);
    font: 400 var(--t-mono) / 24px var(--f-mono);
    overflow-wrap: anywhere;
  }
  ul,
  ol {
    display: grid;
    gap: var(--s-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  ul li {
    display: flex;
    align-items: center;
    gap: var(--s-3);
  }
  .folder {
    margin-left: auto;
  }
  ol {
    counter-reset: step;
  }
  ol li {
    display: flex;
    gap: var(--s-3);
    counter-increment: step;
  }
  ol li::before {
    content: counter(step);
    color: var(--c-accent);
    font: 400 var(--t-version) / 24px var(--f-mono);
  }
</style>
