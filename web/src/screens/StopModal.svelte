<script lang="ts">
  import type { Snippet } from 'svelte';
  import { BUILD, NOTICE, WRITE } from '../app/copy.ts';
  import Modal from './Modal.svelte';

  interface Props {
    /** The node the write stopped at: `[n] name`. */
    target: string;
    /** It went back to its previous version (else it could not be written). */
    back: boolean;
    /** The outcome's own word beside the title, for one that is neither. */
    label?: string;
    reason: string;
    fix: string;
    /** An agent is linked: asking why is offered for a write that went back. */
    linked: boolean;
    onask: (at: Element) => void;
    onclose: () => void;
    onmore: () => void;
    /** The band's actions, primary last. */
    buttons: Snippet;
  }

  let { target, back, label, reason, fix, linked, onask, onclose, onmore, buttons }: Props = $props();
</script>

<Modal title={WRITE.run} chip={{ text: label ?? (back ? WRITE.previous : WRITE.failed), kind: back || label ? 'info' : 'error' }} {target} {onclose} {onmore}>
  <h3 class="modal-h">{NOTICE.reason}</h3>
  <p>{reason}</p>
  <h3 class="modal-h">{NOTICE.fix}</h3>
  <p>{fix}</p>
  {#snippet actions()}
    {#if back && linked}
      <button class="button-secondary" data-control="ask" onclick={(e) => onask(e.currentTarget)}>{BUILD.askAgent}</button>
    {/if}
    {@render buttons()}
  {/snippet}
</Modal>
