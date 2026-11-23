<script lang="ts">
  import { MENU } from '../app/copy.ts';
  import { modal } from '../app/motion.ts';

  interface Props {
    onconfirm: () => void;
    /** "キャンセル" or Escape: back to the menu's "リスタート". */
    oncancel: () => void;
  }

  let { onconfirm, oncancel }: Props = $props();
  let dialog: HTMLDialogElement;
  let cancel: HTMLButtonElement;

  $effect(() => {
    dialog.showModal();
    // The safe choice takes focus: this one cannot be undone.
    cancel.focus();
  });
</script>

<!-- The one confirmation of the page. -->
<dialog bind:this={dialog} transition:modal|global aria-labelledby="restart-h" aria-describedby="restart-s" onclose={oncancel}>
  <h2 id="restart-h">{MENU.restartHeading}</h2>
  <p id="restart-s">{MENU.restartSentence}</p>
  <div class="actions">
    <button class="secondary" bind:this={cancel} onclick={oncancel}>{MENU.cancel}</button>
    <button class="primary" onclick={onconfirm}>{MENU.restartConfirm}<span class="arrow" aria-hidden="true">→</span></button>
  </div>
</dialog>

<style>
  dialog {
    max-width: min(32rem, calc(100vw - 2 * var(--m-edge)));
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
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: var(--s-4);
  }
</style>
