<script lang="ts">
  import type { Snippet } from 'svelte';
  import { NOTICE } from '../app/copy.ts';
  import { modal } from '../app/motion.ts';
  import Icon from './Icon.svelte';

  interface Props {
    title: string;
    /** The state chip beside the title (… 作成中, ✓ 完了, ! 失敗). */
    chip?: { text: string; kind: 'active' | 'ok' | 'error' | 'info' } | null;
    /** What the modal acts on, in mono under the title (a project, a node). */
    target?: string;
    /** Closing is offered (the × at the top-right); absent while closing is not possible. */
    onclose?: () => void;
    /** 詳しく見る at the footer's left: the record behind the notice. */
    onmore?: () => void;
    children: Snippet;
    /** The footer's actions, primary last. */
    actions?: Snippet;
  }

  let { title, chip = null, target, onclose, onmore, children, actions }: Props = $props();
  let el: HTMLDialogElement;
  $effect(() => {
    el.showModal();
    return () => el.close();
  });
</script>

<!-- Notice/Modal: centred, w-modal wide; the scrim (::backdrop) covers the whole window. -->
<dialog bind:this={el} transition:modal|global aria-labelledby="modal-h" oncancel={(e) => (onclose ? onclose() : e.preventDefault())}>
  <header>
    <h2 id="modal-h">{title}</h2>
    {#if chip}<span class="chip {chip.kind}">{chip.text}</span>{/if}
    {#if onclose}
      <button class="button-icon close" aria-label={NOTICE.close} title={NOTICE.close} onclick={onclose}><span class="layer"><Icon name="x" /></span></button>
    {/if}
  </header>
  {#if target}<p class="target" translate="no">{target}</p>{/if}
  <div class="body">{@render children()}</div>
  {#if onmore || actions}
    <footer>
      {#if onmore}<button class="more" onclick={onmore}>{NOTICE.more}</button>{/if}
      <span class="grow"></span>
      {#if actions}{@render actions()}{/if}
    </footer>
  {/if}
</dialog>

<style>
  dialog {
    width: var(--w-modal);
    max-width: calc(100vw - 2 * var(--m-edge));
    max-height: calc(100vh - 2 * var(--m-edge));
    padding: var(--s-5);
    border: 1px solid var(--c-paper);
    border-radius: var(--r-l);
    background: var(--c-layer-1);
    color: var(--c-paper);
    overflow: auto;
    flex-direction: column;
    gap: var(--s-4);
  }
  dialog::backdrop {
    background: var(--c-scrim);
  }
  dialog[open] {
    display: flex;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--s-3);
  }
  h2 {
    margin: 0;
    font: 700 var(--t-heading) / 32px var(--f-ja);
  }
  .close {
    margin: calc(-1 * var(--p-hit)) calc(-1 * var(--p-hit)) calc(-1 * var(--p-hit)) auto;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: var(--h-chip);
    padding: 0 var(--s-2);
    border: 1px solid var(--c-paper);
    border-radius: var(--r-full);
    font: 700 var(--t-caption) / 1 var(--f-ja);
    white-space: nowrap;
  }
  .chip.active {
    border-color: var(--c-progress);
    color: var(--c-progress);
  }
  .chip.info {
    border-color: var(--c-accent);
    color: var(--c-accent);
  }
  .chip.error {
    border-color: var(--c-signal);
    background: var(--c-signal);
    color: var(--c-ink);
  }
  .target {
    margin: 0;
    color: var(--c-muted);
    font: 400 var(--t-mono) / 24px var(--f-mono);
  }
  .body {
    display: grid;
    gap: var(--s-3);
    font: 400 var(--t-small) / 24px var(--f-ja);
  }
  footer {
    display: flex;
    align-items: center;
    gap: var(--s-3);
  }
  .grow {
    flex: 1;
  }
  .more {
    height: var(--h-text-button);
    padding: 0 var(--s-2);
    border: 0;
    background: none;
    color: var(--c-accent);
    font: 700 var(--t-small) / 1 var(--f-ja);
    cursor: pointer;
  }
  .more:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
</style>
