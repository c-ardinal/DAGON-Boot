<script lang="ts">
  import type { Snippet } from 'svelte';
  import { NOTICE, WRITE } from '../app/copy.ts';
  import Modal from './Modal.svelte';

  interface Props {
    /** The node the write stopped at: `[n] name`. */
    target: string;
    /** The outcome's own word beside the title, for one that is neither went back nor failed. */
    label?: string;
    reason: string;
    fix: string;
    onclose: () => void;
    onmore: () => void;
    /** The band's actions, primary last. */
    buttons: Snippet;
  }

  let { target, label, reason, fix, onclose, onmore, buttons }: Props = $props();
</script>

<!-- A write that could not be done and needs the user. One that went back is told by StopNotice, beside the graph. -->
<Modal title={WRITE.run} chip={{ text: label ?? WRITE.failed, kind: label ? 'info' : 'error' }} {target} {onclose} {onmore}>
  <h3 class="modal-h">{NOTICE.reason}</h3>
  <p>{reason}</p>
  <h3 class="modal-h">{NOTICE.fix}</h3>
  <p>{fix}</p>
  {#snippet actions()}
    {@render buttons()}
  {/snippet}
</Modal>
