<script lang="ts" module>
  export interface DrawerTab {
    id: string;
    label: string;
    /** A count after the label ("問題 2"); `alarm` marks it as a failure (red), never for 0. */
    count?: number;
    alarm?: boolean;
  }

  /** Where the drawer's height is kept across reloads. */
  export const DRAWER_HEIGHT_KEY = 'dagon-drawer-height';
</script>

<script lang="ts">
  import { untrack, type Snippet } from 'svelte';
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

  /** The user's height (px), kept across reloads; null until dragged, when the token's default holds. */
  let height = $state<number | null>(read());
  // A kept value is limited as a dragged one is (a stored 1 would leave no body).
  $effect(() => {
    if (height !== null) untrack(() => (height = limit(height!)));
  });
  /** The stage above must keep room for its tools: at most this share of the window. */
  const MAX_SHARE = 0.6;
  const STEP = 4;

  function read(): number | null {
    try {
      const v = Number(localStorage.getItem(DRAWER_HEIGHT_KEY));
      return Number.isFinite(v) && v > 0 ? v : null;
    } catch {
      return null;
    }
  }
  function keep(h: number) {
    try {
      localStorage.setItem(DRAWER_HEIGHT_KEY, String(h));
    } catch {
      // Storage may be off: the height then lasts for this page only.
    }
  }
  const limit = (h: number) => {
    const min = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--h-drawer-min')) || 120;
    return Math.round(Math.min(Math.max(h, min), innerHeight * MAX_SHARE) / STEP) * STEP;
  };

  /** The top edge drags: the body follows the pointer 1:1, within the limits. */
  let section = $state<HTMLElement>();
  function grab(e: PointerEvent) {
    if (e.button !== 0 || !section) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const h0 = section.getBoundingClientRect().height;
    const y0 = e.clientY;
    const move = (ev: PointerEvent) => (height = limit(h0 + (y0 - ev.clientY)));
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      el.removeEventListener('pointercancel', up);
      if (height !== null) keep(height);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
    el.addEventListener('pointercancel', up);
  }
  /** The keyboard moves the edge in steps. */
  function keys(e: KeyboardEvent) {
    const d = e.key === 'ArrowUp' ? 8 * STEP : e.key === 'ArrowDown' ? -8 * STEP : 0;
    if (!d || !section) return;
    e.preventDefault();
    height = limit(section.getBoundingClientRect().height + d);
    keep(height);
  }
</script>

<!-- The tab strip always stays; the body opens under it, over the canvas, whose box does not change. -->
<section in:fade bind:this={section} class="drawer" class:open={open !== null} style:height={open !== null && height !== null ? `${height}px` : undefined} aria-label={DRAWER.label}>
  {#if open}
    <!-- The handle: the open drawer's top edge, dragged to change its height. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex a11y_no_noninteractive_element_interactions -->
    <div class="handle" role="separator" aria-orientation="horizontal" aria-valuenow={Math.round(section?.getBoundingClientRect().height ?? height ?? 0)} aria-valuemin={120} aria-valuemax={Math.round((globalThis.innerHeight ?? 0) * MAX_SHARE)} aria-label={DRAWER.resize} title={DRAWER.resize} tabindex="0" onpointerdown={grab} onkeydown={keys}></div>
  {/if}
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
    {#key open}
      <div transition:fade={{ dur: '--dur-quick' }} class="body" id="drawer-body" role="tabpanel" aria-labelledby="drawer-tab-{open}" tabindex="-1">
        {@render body(open)}
      </div>
    {/key}
  {/if}
</section>

<style>
  .drawer {
    grid-area: drawer;
    position: relative;
    display: flex;
    flex: 0 1 auto;
    flex-direction: column;
    min-height: var(--h-row);
    border-top: 1px solid var(--c-line);
    background: var(--c-layer-0);
    user-select: none;
  }
  /* In a low window the drawer gives way (its body scrolls; the tabs stay), so the focal number and the zoom keep their room above it. */
  .drawer.open {
    height: var(--h-drawer);
    max-height: 60vh;
  }
  .handle {
    position: absolute;
    top: -4px;
    left: 0;
    right: 0;
    height: var(--s-2);
    cursor: row-resize;
    touch-action: none;
  }
  .handle:hover,
  .handle:focus-visible {
    background: var(--c-accent);
    outline: none;
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
    transition:
      color var(--dur-quick) var(--ease-out),
      box-shadow var(--dur-quick) var(--ease-out);
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
  /* The body: one size of text for its content; long content scrolls inside. */
  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    user-select: text;
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  .body:focus {
    outline: none;
  }
</style>
