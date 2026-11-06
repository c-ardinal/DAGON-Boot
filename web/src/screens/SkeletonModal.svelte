<script lang="ts">
  import { NOTICE, SKELETON } from '../app/copy.ts';
  import { jobReason, type Job } from '../app/define/cli.svelte.ts';
  import Modal from './Modal.svelte';

  interface Props {
    /** The gen job; null until serve has taken it. */
    job: Job | null;
    /** Where the project is (the modal's target line). */
    place: string;
    /** The nodes the skeleton is made for: [label, folder]. */
    nodes: [string, string][];
    onclose: () => void;
    oncancel: () => void;
    onretry: () => void;
    onmore: () => void;
  }

  let { job, place, nodes, onclose, oncancel, onretry, onmore }: Props = $props();

  const state = $derived(job?.state === 'succeeded' ? 'done' : job?.state === 'failed' || job?.state === 'cancelled' ? 'failed' : 'running');
  const percent = $derived(Math.max(0, Math.min(100, job?.percent ?? 0)));
  const made = $derived(state === 'done' ? nodes.length : Math.floor((percent / 100) * nodes.length));
  /** The job's last line is its reason (serve writes why it stopped last). */
  const reason = $derived(jobReason(job));
  const chip = $derived(
    state === 'done'
      ? ({ text: SKELETON.state.done, kind: 'ok' } as const)
      : state === 'failed'
        ? ({ text: SKELETON.state.failed, kind: 'error' } as const)
        : ({ text: SKELETON.state.running, kind: 'active' } as const),
  );
</script>

<!-- 雛形を作る: progress (中止 only), success (where it was made, what to do next), failure (reason, もう一度作る). -->
<Modal title={SKELETON.title} {chip} target={place} onclose={state === 'running' ? undefined : onclose} {onmore}>
  {#if state === 'running'}
    <div class="bar" role="progressbar" aria-valuenow={percent} aria-valuemin="0" aria-valuemax="100" aria-label={SKELETON.title}><i style:width="{percent}%"></i></div>
    <p class="mono">{made} / {nodes.length} · {percent}%</p>
  {:else if state === 'done'}
    <h3>{SKELETON.where}</h3>
    <ul>
      {#each nodes as [label, folder] (folder)}
        <li><span>{label}</span><span class="chip ok">{SKELETON.state.done}</span><span class="mono folder" translate="no">{folder}</span></li>
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
  {/if}
  {#snippet actions()}
    {#if state === 'running'}
      <button class="button-secondary" onclick={oncancel}>{SKELETON.cancel}</button>
    {:else if state === 'done'}
      <button class="button-secondary" onclick={() => void navigator.clipboard?.writeText(place)}>{SKELETON.copyPath}</button>
    {:else}
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
  .bar {
    height: 8px;
    border-radius: var(--r-full);
    background: var(--c-layer-2);
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--c-water);
    transition: width var(--dur-rise) var(--ease-steady);
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
  .chip {
    display: inline-flex;
    align-items: center;
    height: var(--h-chip);
    padding: 0 var(--s-2);
    border: 1px solid var(--c-paper);
    border-radius: var(--r-full);
    font: 700 var(--t-caption) / 1 var(--f-ja);
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
    font: 400 var(--t-version) / 24px var(--f-version);
  }
  @media (prefers-reduced-motion: reduce) {
    .bar i {
      transition: none;
    }
  }
</style>
