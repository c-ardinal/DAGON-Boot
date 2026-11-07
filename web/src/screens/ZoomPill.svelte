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

<!-- ZoomPill: a vertical stack, + on top; the readout between + and −, on the straight part of the pill where the round ends cannot touch it. -->
<div class="pill" data-avoid="" role="toolbar" aria-label={ZOOM.label} aria-orientation="vertical">
  <button class="button-icon" aria-label={ZOOM.in} title={canIn ? `${ZOOM.in} (Ctrl++)` : ZOOM.maxWhy} aria-disabled={!canIn} onclick={() => canIn && onin()}><span class="layer"><Icon name="plus" /></span></button>
  <span class="pct">{Math.round(scale * 100)}%</span>
  <button class="button-icon" aria-label={ZOOM.out} title={canOut ? `${ZOOM.out} (Ctrl+-)` : ZOOM.minWhy} aria-disabled={!canOut} onclick={() => canOut && onout()}><span class="layer"><Icon name="minus" /></span></button>
  <button class="button-icon" aria-label={ZOOM.fit} title="{ZOOM.fit} (F)" onclick={onfit}><span class="layer"><Icon name="maximize" /></span></button>
</div>

<style>
  .pill {
    display: flex;
    flex: none;
    flex-direction: column;
    align-items: center;
    align-self: flex-end;
    width: var(--h-button);
    pointer-events: auto;
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: var(--c-ink);
  }
  .pct {
    width: 100%;
    color: var(--c-paper);
    font: 500 var(--t-caption) / 16px var(--f-mono);
    text-align: center;
  }
</style>
