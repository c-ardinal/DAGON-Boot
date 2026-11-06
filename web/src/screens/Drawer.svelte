<script lang="ts" module>
  export interface DrawerTab {
    id: string;
    label: string;
    /** A count after the label ("問題 2"); `alarm` marks it as a failure (red), never for 0. */
    count?: number;
    alarm?: boolean;
  }
</script>

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { DRAWER } from '../app/copy.ts';
  import { fade } from '../app/motion.ts';
  import Icon from './Icon.svelte';

  interface Props {
    tabs: DrawerTab[];
    /** The open tab, or null: collapsed, only the tab strip shows. */
    open: string | null;
    onpick: (id: string) => void;
    onclose: () => void;
    body: Snippet<[string]>;
  }

  let { tabs, open, onpick, onclose, body }: Props = $props();
</script>

<!-- The tab strip always stays; the body opens under it, over the canvas, whose box does not change. -->
<section in:fade class="drawer" class:open={open !== null} aria-label={DRAWER.label}>
  <div class="tabs" role="tablist" aria-label={DRAWER.label}>
    <div class="scroll">
      {#each tabs as t (t.id)}
        <button role="tab" id="drawer-tab-{t.id}" aria-selected={open === t.id} aria-controls="drawer-body" onclick={() => (open === t.id ? onclose() : onpick(t.id))}>
          {t.label}{#if t.count !== undefined}<span class="count" class:alarm={t.alarm && t.count > 0}>{t.count}</span>{/if}
        </button>
      {/each}
    </div>
    <button
      class="button-icon"
      title="{open ? DRAWER.close : DRAWER.label} (Ctrl+J)"
      aria-label={open ? DRAWER.close : DRAWER.label}
      aria-expanded={open !== null}
      onclick={() => (open ? onclose() : onpick(tabs[0].id))}><span class="layer"><Icon name={open ? 'chevron-down' : 'chevron-up'} /></span></button
    >
  </div>
  {#if open}
    <div in:fade class="body" id="drawer-body" role="tabpanel" aria-labelledby="drawer-tab-{open}" tabindex="-1">
      {@render body(open)}
    </div>
  {/if}
</section>

<style>
  .drawer {
    grid-area: drawer;
    display: flex;
    flex: 0 1 auto;
    flex-direction: column;
    min-height: var(--h-row);
    border-top: 1px solid var(--c-line);
    background: var(--c-layer-0);
  }
  /* In a low window the drawer gives way (its body scrolls; the tabs stay), so the focal number and the zoom keep their room above it. */
  .drawer.open {
    height: var(--h-drawer);
  }
  .tabs {
    display: flex;
    flex: none;
    align-items: center;
    height: var(--h-row);
    padding: 0 var(--s-2);
    border-bottom: 1px solid var(--c-layer-2);
  }
  .scroll {
    display: flex;
    flex: 1;
    align-items: center;
    min-width: 0;
    overflow-x: auto;
  }
  [role='tab'] {
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    height: var(--h-row);
    padding: 0 var(--s-4);
    border: 0;
    background: none;
    color: var(--c-muted);
    font: 700 var(--t-small) / 1 var(--f-ja);
    white-space: nowrap;
    cursor: pointer;
  }
  [role='tab'][aria-selected='true'] {
    color: var(--c-paper);
    box-shadow: inset 0 -2px 0 var(--c-paper);
  }
  [role='tab']:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: calc(-1 * var(--st-w-focus));
  }
  .count {
    padding: 0 var(--s-2);
    border-radius: var(--r-full);
    background: var(--c-layer-2);
    color: var(--c-paper);
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  .count.alarm {
    background: var(--c-signal);
    color: var(--c-ink);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .body:focus {
    outline: none;
  }
</style>
