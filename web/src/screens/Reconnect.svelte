<script lang="ts" module>
  import { RECONNECT } from '../app/copy.ts';
  import type { ReconnectNote } from '../serial/transport.ts';
  import { versionText } from '../wasm/dagon.ts';

  /** One line of the diagnostic log, in the page's words. */
  export function noteText(n: ReconnectNote): string {
    switch (n.kind) {
      case 'several':
        return RECONNECT.notes.several(n.count);
      case 'version':
        return RECONNECT.notes.version(versionText(n.version), n.trial);
      default:
        return RECONNECT.notes[n.kind];
    }
  }
</script>

<script lang="ts">
  import { BUTTON } from '../app/copy.ts';
  import { modal } from '../app/motion.ts';

  interface Props {
    /** Why the page could not reconnect by itself (dagon-host's 45 s are over: this dialog opens only then). */
    notes: ReconnectNote[];
    onreconnect: () => void;
    /** Closed with Escape: the band offers the same button. */
    onclose: () => void;
  }

  let { notes, onreconnect, onclose }: Props = $props();

  let dialog: HTMLDialogElement;
  let button: HTMLButtonElement;
  const returnTo = document.activeElement as HTMLElement | null;

  $effect(() => {
    dialog.showModal();
    button.focus();
    return () => returnTo?.focus();
  });
</script>

<dialog bind:this={dialog} transition:modal|global aria-labelledby="reconnect-h" {onclose}>
  <h2 id="reconnect-h">{RECONNECT.heading}</h2>
  <p>{RECONNECT.sentence}</p>
  <button class="primary" bind:this={button} onclick={onreconnect}>
    {BUTTON.reconnect}<span class="arrow" aria-hidden="true">→</span>
  </button>
  {#if notes.length}
    <details>
      <summary>{RECONNECT.why}</summary>
      <ul>
        {#each notes as n, i (i)}<li>{noteText(n)}</li>{/each}
      </ul>
    </details>
  {/if}
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
    font: 700 var(--t-heading) / 28px var(--f-ja);
    letter-spacing: 0.02em;
  }
  p {
    margin: 0 0 var(--s-5);
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  .primary {
    display: flex;
    margin: 0 auto;
  }
  details {
    margin-top: var(--s-5);
    font: 400 var(--t-caption) / 16px var(--f-ja);
  }
  summary {
    color: var(--c-muted);
    cursor: pointer;
  }
  ul {
    margin: var(--s-2) 0 0;
    padding-left: var(--s-4);
  }
</style>
