<script lang="ts">
  import type { Action } from '../app/machine.ts';
  import type { Band } from '../app/band.ts';

  interface Props {
    band: Band;
    busy: boolean;
    /** False for buttons whose procedure this page does not run yet: shown unavailable. */
    available: (a: Action) => boolean;
    status: string;
    alert: string;
    onaction: (a: Action) => void;
    /** The lock-table operation an action is (DES-UX3-182), so a stopped one says why when pressed. */
    control?: (a: Action) => string | undefined;
    primaryEl?: HTMLButtonElement;
    nextEl?: HTMLDivElement;
  }

  let { band, busy, available, status, alert, onaction, control = () => undefined, primaryEl = $bindable(), nextEl = $bindable() }: Props = $props();
</script>

<footer id="band" tabindex="-1">
  <div class="next" tabindex="-1" bind:this={nextEl}>
    {#if band.chip}<span class="cause">{band.chip}</span>{/if}
    {#if band.pair}<span class="pair"><span class="label">{band.pair[0]}</span> {band.pair[1]}</span>{/if}
    {#if band.sentence}<p>{band.sentence}</p>{/if}
  </div>
  {#if band.secondary.length}
    <div class="side">
      {#each band.secondary as b (b.action)}
        {@const off = !available(b.action)}
        <button class="secondary" data-control={control(b.action)} aria-disabled={off} onclick={() => !off && onaction(b.action)}>{b.label}</button>
      {/each}
    </div>
  {/if}
  {#if band.primary}
    {@const p = band.primary}
    {@const off = busy || !available(p.action)}
    <!-- Busy and unavailable stay focusable (aria-disabled) so focus never drops to the page. -->
    <button class="primary" data-control={control(p.action)} bind:this={primaryEl} aria-disabled={off} onclick={() => !off && onaction(p.action)}>
      {busy ? `${p.label}…` : p.label}<span class="arrow" aria-hidden="true">→</span>
    </button>
  {/if}
  <div class="visually-hidden" role="status">{status}</div>
  <div class="visually-hidden" role="alert">{alert}</div>
</footer>

<style>
  footer {
    grid-area: band;
    display: flex;
    align-items: center;
    gap: var(--s-6);
    min-height: var(--h-band);
    padding: 0 var(--s-5);
    border-top: 1px solid var(--c-line);
    z-index: var(--z-chrome);
  }
  footer:focus {
    outline: none;
  }
  .next {
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    margin-right: auto;
  }
  .next:focus {
    outline: none;
  }
  .next p {
    margin: 0;
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
    text-wrap: pretty;
  }
  .cause {
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
    letter-spacing: 0.02em;
    white-space: nowrap;
  }
  .pair {
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
    font-variant-numeric: tabular-nums;
  }
  .label {
    color: var(--c-muted);
  }
  .side {
    display: flex;
    gap: var(--s-3);
  }
  @media (max-width: 767px) {
    footer {
      flex-wrap: wrap;
      gap: var(--s-4);
      padding: var(--s-4);
      border-top: 0;
      border-bottom: 1px solid var(--c-line);
    }
    .next {
      flex: 1 1 100%;
    }
  }
</style>
