<script lang="ts">
  import { BUTTON, RECONNECT } from '../app/copy.ts';

  interface Props {
    /** Seconds dagon-host still reopens the hub by itself; null once that is over. */
    left: number | null;
    onreconnect: () => void;
    /** Closed with Escape: the band offers the same button. */
    onclose: () => void;
  }

  let { left, onreconnect, onclose }: Props = $props();

  /** M27: shown in 5 s steps. */
  const STEP_S = 5;
  const shown = $derived(left === null ? null : Math.ceil(left / STEP_S) * STEP_S);

  let dialog: HTMLDialogElement;
  let button: HTMLButtonElement;
  const returnTo = document.activeElement as HTMLElement | null;

  $effect(() => {
    dialog.showModal();
    button.focus();
    return () => returnTo?.focus();
  });
</script>

<dialog bind:this={dialog} aria-labelledby="reconnect-h" onclose={onclose}>
  <h2 id="reconnect-h">{RECONNECT.heading}</h2>
  <p>{RECONNECT.sentence}</p>
  <p class="pair">
    <span class="label">{RECONNECT.auto}</span>
    {shown === null ? RECONNECT.over : RECONNECT.left(shown)}
  </p>
  <button class="primary" bind:this={button} onclick={onreconnect}>
    {BUTTON.reconnect}<span class="arrow" aria-hidden="true">→</span>
  </button>
</dialog>

<style>
  dialog {
    max-width: 30rem;
    padding: var(--s-6);
    border: 1px solid var(--c-line);
    border-radius: var(--r-l);
    background: var(--c-layer-1);
    color: var(--c-paper);
  }
  dialog::backdrop {
    background: var(--c-scrim);
  }
  h2 {
    margin: 0 0 var(--s-4);
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
    letter-spacing: 0.02em;
  }
  p {
    margin: 0 0 var(--s-5);
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  .pair {
    font-variant-numeric: tabular-nums;
  }
  .label {
    color: var(--c-muted);
  }
  .primary {
    display: flex;
    margin: 0 auto;
  }
</style>
