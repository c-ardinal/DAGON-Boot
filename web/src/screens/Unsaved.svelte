<script lang="ts">
  import { modal } from '../app/motion.ts';
  import { UNSAVED } from '../app/copy.ts';

  interface Props {
    /** The save failed; why, in the dialog's words. */
    reason: string;
    onsave: () => void;
    ondiscard: () => void;
    /** "やめる" or Escape: back to the page, edits kept. */
    onstay: () => void;
  }

  let { reason, onsave, ondiscard, onstay }: Props = $props();
  let dialog: HTMLDialogElement;
  let stay: HTMLButtonElement;

  $effect(() => {
    dialog.showModal();
    // The safe choice takes focus: discarding cannot be undone.
    stay.focus();
  });
</script>

<!-- leaving with edits the project's YAML does not have. -->
<dialog bind:this={dialog} transition:modal|global aria-labelledby="unsaved-h" aria-describedby="unsaved-s" onclose={onstay}>
  <h2 id="unsaved-h">{UNSAVED.heading}</h2>
  <p class="cause">{reason}</p>
  <p id="unsaved-s">{UNSAVED.sentence}</p>
  <div class="actions">
    <button class="secondary" bind:this={stay} onclick={onstay}>{UNSAVED.stay}</button>
    <button class="secondary" onclick={ondiscard}>{UNSAVED.discard}</button>
    <button class="primary" onclick={onsave}>{UNSAVED.save}<span class="arrow" aria-hidden="true">→</span></button>
  </div>
</dialog>

<style>
  dialog {
    max-width: 34rem;
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
  .cause {
    margin-bottom: var(--s-2);
    font-weight: 700;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: var(--s-4);
  }
</style>
