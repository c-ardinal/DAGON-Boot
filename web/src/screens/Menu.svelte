<script lang="ts" module>
  export interface MenuItem {
    label: string;
    run: () => void;
    /** The operation in the lock table, when it has one. */
    control?: string;
    /** Why it cannot be used now: it stays, dimmed, and says so. */
    off?: string | null;
    /** The key that does the same. */
    key?: string;
    /** What happens when it is pressed, under the name. */
    hint?: string;
    /** Destroys something: red, under a separator. */
    danger?: boolean;
  }

  export interface MenuGroup {
    label: string;
    items: MenuItem[];
  }
</script>

<script lang="ts">
  import { MENU } from '../app/copy.ts';
  import { rise } from '../app/motion.ts';
  import Icon from './Icon.svelte';

  interface Props {
    groups: MenuGroup[];
    /** After an item ran. */
    onclose: () => void;
  }

  let { groups, onclose }: Props = $props();
  let openGroup = $state<string | null>(null);
  const shown = $derived(groups.find((g) => g.label === openGroup));

  function press(i: MenuItem) {
    if (i.off) return;
    onclose();
    i.run();
  }
</script>

{#snippet row(i: MenuItem)}
  <li>
    <button class="row" class:danger={i.danger} data-control={i.control} aria-disabled={!!i.off} title={i.off ?? undefined} onclick={() => press(i)}>
      <span class="words"><span>{i.label}</span>{#if i.hint}<small>{i.off ?? i.hint}</small>{/if}</span>
      {#if i.key}<kbd translate="no">{i.key}</kbd>{/if}
    </button>
  </li>
{/snippet}

<!-- Anchored to ☰, over the page: nothing under it moves. Groups first, the group's items beside them on the side the window has room. -->
<nav id="menu" transition:rise={{ dur: '--dur-quick' }} class="menu" aria-label={MENU.button}>
  <ul class="panel">
    {#each groups as g (g.label)}
      <li>
        <button class="row" aria-expanded={openGroup === g.label} onpointerenter={() => (openGroup = g.label)} onclick={() => (openGroup = g.label)}>
          <Icon name="chevron-left" size="var(--i-s)" />
          <span>{g.label}</span>
        </button>
      </li>
    {/each}
  </ul>
  {#if shown}
    <ul class="panel" aria-label={shown.label}>
      {#each shown.items.filter((i) => !i.danger) as i (i.label)}
        {@render row(i)}
      {/each}
      {#if shown.items.some((i) => i.danger)}
        <li class="sep" role="separator"></li>
        {#each shown.items.filter((i) => i.danger) as i (i.label)}
          {@render row(i)}
        {/each}
      {/if}
    </ul>
  {/if}
</nav>

<style>
  .menu {
    position: absolute;
    z-index: var(--z-announce);
    top: 100%;
    right: 0;
    display: flex;
    flex-direction: row-reverse;
    align-items: flex-start;
    gap: var(--s-2);
  }
  .panel {
    width: var(--w-menu);
    max-height: calc(100vh - var(--h-bar) - var(--s-4));
    margin: 0;
    padding: var(--s-2) 0;
    overflow: auto;
    border: 1px solid var(--c-paper);
    border-radius: var(--r-m);
    background: var(--c-layer-1);
    list-style: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    width: 100%;
    min-height: var(--h-row);
    padding: var(--s-2) var(--s-4);
    transition: background-color var(--dur-quick) var(--ease-out);
    border: 0;
    background: none;
    color: var(--c-paper);
    font: 400 var(--t-small) / 1 var(--f-ja);
    text-align: left;
    cursor: pointer;
  }
  .row .words {
    flex: 1;
    display: grid;
    min-width: 0;
    gap: var(--s-1);
    white-space: normal;
  }
  .row small {
    color: var(--c-muted);
    font: 400 var(--t-caption) / 16px var(--f-ja);
  }
  .row[aria-expanded='true'] {
    background: var(--c-layer-2);
  }
  .row.danger {
    color: var(--c-signal);
  }
  .row[aria-disabled='true'] {
    color: var(--c-muted);
    cursor: default;
  }
  @media (hover: hover) and (pointer: fine) {
    .row:not([aria-disabled='true']):hover {
      background: var(--c-layer-2);
    }
  }
  .row:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: calc(-1 * var(--st-w-focus));
  }
  kbd {
    color: var(--c-muted);
    font: 400 var(--t-caption) / 1 var(--f-mono);
  }
  .sep {
    height: 1px;
    margin: var(--s-2) 0;
    background: var(--c-layer-2);
  }
</style>
