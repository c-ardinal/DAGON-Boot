<script lang="ts">
  import type { Snippet } from 'svelte';
  import { NOTICE } from '../app/copy.ts';
  import { rise } from '../app/motion.ts';
  import Icon from './Icon.svelte';

  interface Props {
    word: string;
    /** The node it is about: `node` is its card, which this notice must not cover. */
    target: string;
    card: () => Element | null | undefined;
    reason: string;
    fix: string;
    onclose: () => void;
    onmore: () => void;
    actions?: Snippet;
  }

  let { word, target, card, reason, fix, onclose, onmore, actions }: Props = $props();
  let el = $state<HTMLElement>();
  /** It rests low beside the zoom; over the node's card it stands at the top instead. */
  let high = $state(false);
  $effect(() => {
    const c = card()?.getBoundingClientRect();
    const n = el?.getBoundingClientRect();
    if (c && n && c.left < n.right && n.left < c.right && c.top < n.bottom && n.top < c.bottom) high = !high;
  });
</script>

<!-- A write that went back: told beside the graph, with no scrim, so the card's change is seen as it happens. -->
<div transition:rise bind:this={el} class="notice" class:high role="alert" data-avoid="" data-stop-notice="">
  <header>
    <Icon name="undo-2" />
    <strong>{word}</strong>
    <span translate="no">{target}</span>
    <button class="button-icon" aria-label={NOTICE.close} title={NOTICE.close} onclick={onclose}><span class="layer"><Icon name="x" /></span></button>
  </header>
  <p>{reason}</p>
  <p>{fix}</p>
  <footer>
    <button class="more" onclick={onmore}>{NOTICE.more}</button>
    <span class="grow"></span>
    {@render actions?.()}
  </footer>
</div>

<style>
  .notice {
    display: grid;
    flex: none;
    gap: var(--s-2);
    align-self: flex-end;
    width: min(var(--w-modal), 100%);
    padding: var(--s-3) var(--s-4);
    border: var(--st-w-strong) solid var(--st-attention);
    border-radius: var(--r-m);
    background: var(--c-layer-1);
    color: var(--c-paper);
    pointer-events: auto;
  }
  .notice.high {
    margin-bottom: auto;
  }
  header,
  footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s-3);
  }
  header :global(.icon:first-child) {
    color: var(--st-attention);
  }
  strong {
    font: 700 var(--t-body) / 24px var(--f-ja);
  }
  header span {
    flex: 1;
    min-width: 0;
    color: var(--c-muted);
    font: 500 var(--t-small) / 24px var(--f-ja);
  }
  p {
    margin: 0;
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  .grow {
    flex: 1;
  }
  .more {
    height: var(--h-text-button);
    padding: 0;
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
