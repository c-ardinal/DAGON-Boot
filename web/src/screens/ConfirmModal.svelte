<script lang="ts" module>
  export interface Ask {
    title: string;
    sentence: string;
    label: string;
    run: () => void;
  }
</script>

<script lang="ts">
  // Every destructive action asks through this one modal: what, why, then the red button.
  // The page's own questions come as `ask`; otherwise it is 定義's removal of a node or a link.
  import { CONFIRM } from '../app/copy.ts';
  import type { Define } from '../app/define/define.svelte.ts';
  import { linkKey, linkView } from '../app/define/model.ts';
  import Modal from './Modal.svelte';

  let { ask, d, onclose }: { ask: Ask | null; d: Define; onclose: () => void } = $props();

  const nameOf = (id: string) => d.doc.names[id] ?? id;
  const c = $derived.by((): Ask => {
    if (ask) return ask;
    const key = d.askRemove ?? '';
    if (key.startsWith('node:')) return { title: CONFIRM.node(nameOf(key.slice(5))), sentence: CONFIRM.nodeSentence, label: CONFIRM.remove, run: () => d.answerRemove(true) };
    const found = d.doc.topo.links?.find((x) => linkKey(d.doc.topo, x) === key);
    const l = found ? linkView(d.doc.topo, found) : null;
    return { title: CONFIRM.link(nameOf(l?.from ?? ''), nameOf(l?.to ?? '')), sentence: CONFIRM.linkSentence, label: CONFIRM.remove, run: () => d.answerRemove(true) };
  });
  const no = () => (ask ? onclose() : d.answerRemove(false));
</script>

<Modal title={c.title} chip={{ text: c.label, kind: 'error' }} onclose={no}>
  <p>{c.sentence}</p>
  {#snippet actions()}
    <button class="button-secondary" onclick={no}>{CONFIRM.cancel}</button>
    <button
      class="button-destructive"
      onclick={() => {
        const { run } = c;
        if (ask) onclose();
        run();
      }}>{c.label}</button
    >
  {/snippet}
</Modal>
