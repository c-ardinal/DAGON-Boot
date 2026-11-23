<script lang="ts">
  // What floats at the pointer: the reason a stopped operation was not done (where it was
  // pressed), and the part being dragged from the parts window.
  import type { Why } from '../app/console/lockGate.svelte.ts';
  import { rise } from '../app/motion.ts';

  let { why, ghost }: { why: Why | null; ghost: { name: string; x: number; y: number } | null } = $props();

  /** The tip's room at the window's right and bottom edges. */
  const EDGE = { right: 336, bottom: 96, gap: 8 };
</script>

{#if ghost}
  <div class="ghost" aria-hidden="true" style:left="{ghost.x + EDGE.gap}px" style:top="{ghost.y + EDGE.gap}px">{ghost.name}</div>
{/if}
{#if why}
  <!-- a stopped operation's reason is information (blue edge), never red. -->
  <div transition:rise={{ dur: '--dur-quick' }} class="why" class:failed={why.failed} role={why.failed ? 'alert' : 'status'} style:left="{Math.max(EDGE.gap, Math.min(why.x, innerWidth - EDGE.right))}px" style:top="{Math.min(why.y, innerHeight - EDGE.bottom)}px">{why.text}</div>
{/if}

<style>
  .why {
    position: fixed;
    z-index: var(--z-announce);
    max-width: min(20rem, calc(100vw - 2 * var(--s-3)));
    padding: var(--s-2) var(--s-3);
    border: 1px solid var(--c-accent);
    border-radius: var(--r-s);
    background: var(--c-layer-2);
    color: var(--c-paper);
    font: 400 var(--t-caption) / 16px var(--f-ja);
    box-shadow: 0 0.25rem 1rem rgb(0 0 0 / 0.5);
    pointer-events: none;
  }
  .why.failed {
    border-color: var(--c-signal);
  }
  .ghost {
    position: fixed;
    z-index: var(--z-announce);
    padding: var(--s-1) var(--s-2);
    border: 1px solid var(--c-paper);
    border-radius: var(--r-m);
    background: var(--c-ink);
    font: 400 var(--t-caption) / 20px var(--f-ja);
    opacity: 0.9;
    pointer-events: none;
  }
</style>
