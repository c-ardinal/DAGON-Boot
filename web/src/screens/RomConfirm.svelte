<script lang="ts">
  import { WRITE, fill } from '../app/copy.ts';
  import Modal from './Modal.svelte';

  interface Props {
    node: string;
    onwrite: () => void;
    oncancel: () => void;
  }
  let { node, onwrite, oncancel }: Props = $props();
</script>

<Modal title={WRITE.run} chip={{ text: WRITE.rom.silent, kind: 'error' }} target={node} onclose={oncancel}>
  <p>{fill(WRITE.rom.body, { node })}</p>
  {#snippet actions()}
    <button class="button-secondary" data-rom="cancel" onclick={oncancel}>{WRITE.factory.cancel}</button>
    <button class="button-primary" data-rom="write" data-control="recover" onclick={onwrite}>{WRITE.run}</button>
  {/snippet}
</Modal>
