<script lang="ts">
  import { tick, untrack, type Snippet } from 'svelte';
  import { BUILD, NOTICE } from '../app/copy.ts';
  import { rise } from '../app/motion.ts';
  import Icon from './Icon.svelte';

  interface Props {
    word: string;
    /** The node it is about, `[n] name`. */
    target: string;
    /** That node's card, which this notice must not cover. */
    card: () => Element | null | undefined;
    reason: string;
    fix: string;
    onclose: () => void;
    onmore: () => void;
    /** An agent is linked: asking why is offered. */
    linked: boolean;
    onask: (at: Element) => void;
    actions?: Snippet;
  }

  let { word, target, card, reason, fix, onclose, onmore, linked, onask, actions }: Props = $props();
  let el = $state<HTMLElement>();
  /** It rests low beside the zoom; over the node's card it stands at the top instead. */
  let high = $state(false);
  /** Over the canvas nothing else takes Escape while it stands. */
  const key = (e: KeyboardEvent) => {
    if (e.key === 'Escape') onclose();
  };
  /** With little room it says only its word and its actions; the sentences stay in the record and on the title. */
  let tight = $state(false);
  const SPOTS = [[false, false], [true, false], [false, true], [true, true]];
  /** A little more than its entrance moves it by, so a card is not touched once it has landed. */
  const MARGIN = 12;
  const clear = () => {
    const c = card()?.getBoundingClientRect();
    const n = el?.getBoundingClientRect();
    return !c || !n || c.left >= n.right + MARGIN || n.left >= c.right + MARGIN || c.top >= n.bottom + MARGIN || n.top >= c.bottom + MARGIN;
  };
  async function place() {
    for (const [h, t] of SPOTS) {
      high = h;
      tight = t;
      await tick();
      if (clear()) return;
    }
  }
  $effect(() => {
    if (el) untrack(() => void place());
  });
</script>

<!-- A write that went back: told beside the graph, with no scrim, so the card's change is seen as it happens. -->
<svelte:window onkeydown={key} onresize={place} />
<div transition:rise bind:this={el} class="notice" class:high title={tight ? `${reason} ${fix}` : undefined} role="alert" data-stop-notice="">
  <header>
    <strong>{word}</strong>
    <span translate="no">{target}</span>
    <button class="button-icon" aria-label={NOTICE.close} title={NOTICE.close} onclick={onclose}><span class="layer"><Icon name="x" /></span></button>
  </header>
  {#if !tight}
    <p>{reason}</p>
    <p>{fix}</p>
  {/if}
  <footer>
    <button class="more" onclick={onmore}>{NOTICE.more}</button>
    <span class="grow"></span>
    {#if linked}<button class="button-secondary" data-control="ask" onclick={(e) => onask(e.currentTarget)}>{BUILD.askAgent}</button>{/if}
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
  strong {
    color: var(--st-attention);
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
    flex: none;
    height: var(--h-text-button);
    padding: 0;
    border: 0;
    background: none;
    color: var(--c-accent);
    font: 700 var(--t-small) / 1 var(--f-ja);
    white-space: nowrap;
    cursor: pointer;
  }
  .more:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
</style>
