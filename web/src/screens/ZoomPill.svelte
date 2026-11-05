<script lang="ts">
  import { ZOOM } from '../app/copy.ts';
  import Icon from './Icon.svelte';

  interface Props {
    /** The zoom shown, 1 = 100 %. */
    scale: number;
    canIn: boolean;
    canOut: boolean;
    onin: () => void;
    onout: () => void;
    onfit: () => void;
  }

  let { scale, canIn, canOut, onin, onout, onfit }: Props = $props();
</script>

<!-- ZoomPill: a vertical stack, + on top; ghost icon buttons in a pill (0S-4). -->
<div class="pill" role="toolbar" aria-label={ZOOM.label} aria-orientation="vertical">
  <button class="button-icon" aria-label={ZOOM.in} title="{ZOOM.in} (Ctrl++)" aria-disabled={!canIn} onclick={() => canIn && onin()}><span class="layer"><Icon name="plus" /></span></button>
  <button class="button-icon" aria-label={ZOOM.out} title="{ZOOM.out} (Ctrl+-)" aria-disabled={!canOut} onclick={() => canOut && onout()}><span class="layer"><Icon name="minus" /></span></button>
  <button class="button-icon" aria-label={ZOOM.fit} title="{ZOOM.fit} (F)" onclick={onfit}><span class="layer"><Icon name="maximize" /></span></button>
  <span class="pct">{Math.round(scale * 100)}%</span>
</div>

<style>
  .pill {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: var(--h-button);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: var(--c-ink);
  }
  .pct {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    color: var(--c-muted);
    font: 400 var(--t-mono) / 1 var(--f-mono);
  }
</style>
