<script lang="ts">
  import { NOTICE, PORT } from '../app/copy.ts';
  import { jobReason, DESCRIPTION_MAX_BYTES, PORT_ID, type Job, type PortRequest } from '../app/define/cli.svelte.ts';
  import Modal from './Modal.svelte';

  interface Props {
    /** The port job; null before the start and until serve has taken it. */
    job: Job | null;
    /** Serve would not take the job: its reason. */
    refused?: string | null;
    /** Why the page cannot start the port itself (no such serve feature, no linked agent); null when it can. */
    off?: string | null;
    /** The request is with serve and its answer has not come. */
    sending?: boolean;
    onstart: (r: PortRequest) => void;
    oncancel: () => void;
    /** Back to the form after a failure. */
    onretry: () => void;
    onclose: () => void;
    onmore: () => void;
  }

  let { job, refused = null, off = null, sending = false, onstart, oncancel, onretry, onclose, onmore }: Props = $props();

  let mcu = $state('');
  let board = $state('');
  let kind = $state<'S' | 'V'>('S');
  let description = $state('');
  let agreed = $state(false);

  const idBad = (v: string) => v !== '' && !PORT_ID.test(v);
  const tooLong = $derived(new TextEncoder().encode(description).length > DESCRIPTION_MAX_BYTES);
  const ready = $derived(PORT_ID.test(mcu) && PORT_ID.test(board) && !tooLong && agreed && !sending);

  /** The agent's own end: its report, and whether it finished its run. */
  const result = $derived(job?.result as { ok?: boolean; text?: string; turns?: number; cost_usd?: number | null } | null);
  const phase = $derived(
    refused || job?.state === 'failed' || (job?.state === 'succeeded' && result?.ok === false)
      ? 'failed'
      : job?.state === 'succeeded'
        ? 'done'
        : job?.state === 'cancelled'
          ? 'cancelled'
          : job || sending
          ? 'running'
          : 'form',
  );
  const chip = $derived(
    phase === 'done'
      ? ({ text: PORT.state.done, kind: 'ok' } as const)
      : phase === 'failed'
        ? ({ text: PORT.state.failed, kind: 'error' } as const)
        : phase === 'cancelled'
          ? ({ text: PORT.state.cancelled, kind: 'info' } as const)
        : phase === 'running'
          ? ({ text: PORT.state.running, kind: 'active' } as const)
          : null,
  );
  /** The last steps of the agent, newest last. */
  const tail = $derived((job?.lines ?? []).slice(-12));
  const reason = $derived(refused ?? (result?.ok === false ? (result.text ?? '') : jobReason(job)));
</script>

<!-- ボード移植: the form (what to port, what the agent may do), the run (the agent's steps, 中止 only), its report, or why it did not run and the same port by hand. -->
{#snippet manual()}
  <h3>{PORT.manual.heading}</h3>
  <ol>
    {#each PORT.manual.steps as s (s)}<li>{s}</li>{/each}
  </ol>
  <p class="mono" translate="no">{PORT.manual.command(mcu, board, kind)}</p>
{/snippet}
<Modal title={PORT.title} {chip} target={job?.target} onclose={phase === 'running' ? undefined : onclose} onmore={job ? onmore : undefined}>
  {#if phase === 'form'}
    <p>{PORT.intro}</p>
    <div class="fields">
      <label
        >{PORT.mcu}<input data-field="mcu" translate="no" spellcheck="false" autocomplete="off" maxlength="32" aria-invalid={idBad(mcu)} bind:value={mcu} />
        {#if idBad(mcu)}<span class="bad">{PORT.idBad}</span>{/if}
      </label>
      <label
        >{PORT.board}<input data-field="board" translate="no" spellcheck="false" autocomplete="off" maxlength="32" aria-invalid={idBad(board)} bind:value={board} />
        {#if idBad(board)}<span class="bad">{PORT.idBad}</span>{/if}
      </label>
    </div>
    <p class="note">{PORT.idNote}</p>
    <label
      >{PORT.kind}<select data-field="kind" bind:value={kind}>
        {#each Object.entries(PORT.kinds) as [k, text] (k)}<option value={k}>{k}: {text}</option>{/each}
      </select>
    </label>
    <label
      >{PORT.description}<textarea data-field="description" rows="4" aria-invalid={tooLong} bind:value={description}></textarea>
      <span class={tooLong ? 'bad' : 'note'}>{tooLong ? PORT.descriptionBad : PORT.descriptionNote}</span>
    </label>
    {#if off}
      <p class="off">{off}</p>
      {@render manual()}
    {:else}
      <h3>{PORT.consent.heading}</h3>
      <ul class="terms">
        {#each PORT.consent.items as item (item)}<li>{item}</li>{/each}
      </ul>
      <label class="agree"><input type="checkbox" data-field="agree" bind:checked={agreed} />{PORT.consent.agree}</label>
    {/if}
  {:else if phase === 'running'}
    <h3>{PORT.log}</h3>
    {#if tail.length}
      <pre translate="no" aria-live="polite">{tail.join('\n')}</pre>
    {:else}
      <p class="note">{PORT.logNone}</p>
    {/if}
  {:else if phase === 'done'}
    <h3>{PORT.report}</h3>
    <pre translate="no">{result?.text ?? ''}</pre>
    {#if typeof result?.turns === 'number'}<p class="note">{PORT.figures(result.turns, result.cost_usd ?? null)}</p>{/if}
    <h3>{NOTICE.after}</h3>
    <p>{PORT.after}</p>
  {:else if phase === 'cancelled'}
    <p>{PORT.cancelled}</p>
  {:else}
    <h3>{NOTICE.reason}</h3>
    <pre translate="no">{reason}</pre>
    <h3>{NOTICE.fix}</h3>
    <p>{PORT.fail.fix}</p>
    {@render manual()}
  {/if}
  {#snippet actions()}
    {#if phase === 'form' && !off}
      <button class="button-primary" data-control="port-start" aria-disabled={!ready} title={ready ? undefined : PORT.startWhy} onclick={() => ready && onstart({ mcu, board, kind, description })}>{PORT.start}</button>
    {:else if phase === 'running' && job}
      <button class="button-secondary" onclick={oncancel}>{PORT.cancel}</button>
    {:else if phase === 'failed' || phase === 'cancelled'}
      <button class="button-primary" onclick={onretry}>{PORT.retry}</button>
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
  .fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--s-3);
  }
  label {
    display: grid;
    gap: var(--s-1);
    min-width: 0;
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  input:not([type='checkbox']),
  select,
  textarea {
    min-height: var(--h-secondary);
    padding: 0 var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 400 var(--t-mono) / 24px var(--f-mono);
  }
  textarea {
    padding: var(--s-2) var(--s-3);
    resize: vertical;
  }
  [aria-invalid='true'] {
    border-color: var(--c-signal);
  }
  .agree {
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  .note {
    color: var(--c-muted);
    font: 400 var(--t-caption) / 16px var(--f-ja);
  }
  .bad,
  .off {
    color: var(--c-signal);
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  .mono,
  pre {
    margin: 0;
    color: var(--c-muted);
    font: 400 var(--t-mono) / 20px var(--f-mono);
    overflow-wrap: anywhere;
  }
  pre {
    max-height: 16rem;
    overflow: auto;
    padding: var(--s-3);
    border: 1px solid var(--c-line);
    white-space: pre-wrap;
  }
  ul,
  ol {
    display: grid;
    gap: var(--s-2);
    margin: 0;
    padding: 0 0 0 var(--s-5);
  }
</style>
