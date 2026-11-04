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

  interface Props {
    tabs: DrawerTab[];
    /** The open tab; the drawer shows only while one is open (its toggles are on the status row). */
    open: string;
    onpick: (id: string) => void;
    onclose: () => void;
    body: Snippet<[string]>;
  }

  let { tabs, open, onpick, onclose, body }: Props = $props();
</script>

<!-- DES-UX3-015: closed by default; while open the canvas above shrinks by its height (no overlap). -->
<section class="drawer" aria-label={DRAWER.label}>
  <div class="tabs" role="tablist" aria-label={DRAWER.label}>
    {#each tabs as t (t.id)}
      <button role="tab" id="drawer-tab-{t.id}" aria-selected={open === t.id} aria-controls="drawer-body" onclick={() => onpick(t.id)}>
        {t.label}{#if t.count !== undefined}<span class="count" class:alarm={t.alarm && t.count > 0}>{t.count}</span>{/if}
      </button>
    {/each}
    <span class="grow"></span>
    <button class="x" title="{DRAWER.close} (Ctrl+J)" aria-label={DRAWER.close} onclick={onclose}>×</button>
  </div>
  <div class="body" id="drawer-body" role="tabpanel" aria-labelledby="drawer-tab-{open}" tabindex="-1">
    {@render body(open)}
  </div>
</section>

<style>
  .drawer {
    grid-area: drawer;
    display: flex;
    flex-direction: column;
    height: 10.5rem;
    border-top: 1px solid var(--c-line);
    background: var(--c-ink);
  }
  @media (max-height: 520px) {
    .drawer {
      height: min(10.5rem, 22vh);
    }
  }
  .tabs {
    display: flex;
    flex: none;
    align-items: stretch;
    height: 1.75rem;
    padding: 0 var(--s-1);
    border-bottom: 1px solid var(--c-hair);
  }
  .tabs button {
    padding: 0 var(--s-3);
    border: 0;
    background: none;
    color: var(--c-muted);
    font: 400 var(--t-12) / 1 var(--f-ja);
    cursor: pointer;
  }
  .tabs button[aria-selected='true'] {
    color: var(--c-paper);
    box-shadow: inset 0 -2px 0 var(--c-paper);
  }
  .tabs button:focus-visible {
    outline: 2px solid var(--c-paper);
    outline-offset: -2px;
  }
  .count {
    margin-left: 0.25rem;
    padding: 0 0.375rem;
    border-radius: var(--r-full);
    background: var(--c-layer-3);
    font: 500 var(--t-11) / 1rem var(--f-mono);
  }
  .count.alarm {
    background: var(--c-signal);
    color: var(--c-ink);
  }
  .grow {
    flex: 1;
  }
  .x {
    width: 1.75rem;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .body:focus {
    outline: none;
  }
  @media (max-width: 767px), (pointer: coarse) {
    .tabs {
      height: auto;
    }
    .tabs button {
      min-height: 44px;
      min-width: 44px;
    }
  }
</style>
