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
  import { DRAWER, LOCK } from '../app/copy.ts';

  interface Props {
    tabs: DrawerTab[];
    /** The open tab, or null when the drawer is closed. */
    open: string | null;
    /** The same tab again closes the drawer (DES-UX3-015). */
    onpick: (id: string) => void;
    body: Snippet<[string]>;
    /** The state now (待機, 更新中 …), always shown (DES-UX3-182). */
    state: string;
  }

  let { tabs, open, onpick, body, state }: Props = $props();
</script>

<!-- DES-UX3-015: closed by default; while open the canvas above shrinks by its height (no overlap). -->
<section class="drawer" aria-label={DRAWER.label}>
  <div class="tabs" role="tablist" aria-label={DRAWER.label}>
    {#each tabs as t (t.id)}
      <button
        role="tab"
        id="drawer-tab-{t.id}"
        aria-selected={open === t.id}
        aria-controls="drawer-body"
        aria-expanded={open === t.id}
        onclick={() => onpick(t.id)}
      >
        {t.label}{#if t.count !== undefined}&ensp;<span class="count" class:alarm={t.alarm && t.count > 0}>{t.count}</span>{/if}
      </button>
    {/each}
    <span class="state" title={LOCK.stateTitle}><span class="visually-hidden">{LOCK.stateLabel} </span>{state}</span>
    <span class="key" aria-hidden="true">Ctrl+J</span>
  </div>
  {#if open}
    <div class="body" id="drawer-body" role="tabpanel" aria-labelledby="drawer-tab-{open}" tabindex="-1">
      {@render body(open)}
    </div>
  {/if}
</section>

<style>
  .drawer {
    grid-area: drawer;
    display: grid;
    min-height: 0;
    border-top: 1px solid var(--c-line);
    background: var(--c-ink);
  }
  .tabs {
    display: flex;
    align-items: stretch;
    gap: var(--s-1);
    padding: 0 var(--s-4);
  }
  .tabs button {
    min-height: 2rem;
    padding: 0 var(--s-3);
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--c-muted);
    font: 500 var(--t-small-size) / 1 var(--f-ja);
    cursor: pointer;
    transition: transform var(--dur-press) var(--ease-out);
  }
  .tabs button:active {
    transform: scale(0.97);
  }
  .tabs button[aria-selected='true'] {
    border-bottom-color: var(--c-accent);
    color: var(--c-paper);
  }
  .tabs button:focus-visible {
    outline: 2px solid var(--c-paper);
    outline-offset: -2px;
  }
  .count {
    font-variant-numeric: tabular-nums;
  }
  .count.alarm {
    color: var(--c-signal);
  }
  .state {
    margin-left: auto;
    align-self: center;
    color: var(--c-paper);
    font: 500 var(--t-small-size) / 1 var(--f-ja);
  }
  .key {
    margin-left: var(--s-4);
    align-self: center;
    color: var(--c-muted);
    font: 400 var(--t-caption-size) / 1 var(--f-mono);
  }
  .body {
    height: clamp(9rem, 28vh, 18rem);
    overflow: auto;
    border-top: 1px solid var(--c-line);
  }
  .body:focus {
    outline: none;
  }
  @media (max-width: 767px) {
    .tabs button {
      min-height: 2.75rem;
    }
    .key {
      display: none;
    }
  }
</style>
