<script lang="ts" module>
  /** A drawer tab's toggle on the status row. */
  export interface StatusToggle {
    id: string;
    label: string;
    count?: number;
    /** The count is a failure count (問題): red when above 0. */
    alarm?: boolean;
  }
</script>

<script lang="ts">
  import { LOCK, STATUS } from '../app/copy.ts';

  interface Props {
    /** 親機: connected (and how many answer), or not. */
    hub: { on: boolean; text: string };
    /** The 親機 item connects and disconnects (the lock table's つなぐ・切断). */
    onhub?: () => void;
    /** The lock table's state now (待機, 更新中 …). */
    state: string;
    cli: string;
    /** The CLI label opens the CLI sheet (its link, features and key). */
    oncli?: () => void;
    key: string | null;
    toggles: StatusToggle[];
    /** The open drawer tab, or null. */
    open: string | null;
    ontoggle: (id: string) => void;
    /** Update progress 0–100, or null when no update runs. */
    progress: number | null;
    /** What to do next, in one sentence (the band's sentence). */
    sentence: string;
    /** A failure cause before the sentence (× …), red. */
    cause?: string;
    mode: string;
    saved: string;
    status: string;
    alert: string;
    nextEl?: HTMLElement;
  }

  let { hub, onhub, state, cli, oncli, key, toggles, open, ontoggle, progress, sentence, cause, mode, saved, status, alert, nextEl = $bindable() }: Props = $props();
</script>

<!-- one 24 px line under everything: 親機, the state, CLI, key, the drawer's toggles, progress and the next step. -->
<footer id="band" class="status" tabindex="-1" aria-label={STATUS.label}>
  {#if onhub}
    <button class="hub" class:on={hub.on} data-control="connect" onclick={onhub}><span class="dot" class:full={hub.on}></span>{hub.text}</button>
  {:else}
    <span class="hub" class:on={hub.on}><span class="dot" class:full={hub.on}></span>{hub.text}</span>
  {/if}
  <span class="state" title={LOCK.stateTitle}><span class="visually-hidden">{LOCK.stateLabel} </span>{state}</span>
  {#if oncli}<button class="opt" onclick={oncli}>{cli}</button>{:else}<span class="opt">{cli}</span>{/if}
  {#if key}<span class="opt" title={STATUS.keyTitle}>{STATUS.key} {key}</span>{/if}
  {#each toggles as t (t.id)}
    <button aria-pressed={open === t.id} aria-controls="drawer-body" class:err={t.alarm && (t.count ?? 0) > 0} onclick={() => ontoggle(t.id)}
      >{t.label}{#if t.count !== undefined}&ensp;{t.count}{/if}</button
    >
  {/each}
  {#if progress !== null}
    <span class="prog"
      ><span class="bar" role="progressbar" aria-valuenow={Math.round(progress)} aria-valuemin="0" aria-valuemax="100" aria-label={STATUS.progress}
        ><i style:transform="scaleX({(progress / 100).toFixed(3)})"></i></span
      ><span class="on mono">{Math.round(progress)}%</span></span
    >
  {/if}
  <span class="next" tabindex="-1" bind:this={nextEl} title={sentence}>{#if cause}<b class="cause" class:fail={cause.startsWith('×')}>{cause}</b>{/if}{sentence}</span>
  <span class="opt">{mode}</span>
  <span class="opt">{saved}</span>
  <div class="visually-hidden" role="status">{status}</div>
  <div class="visually-hidden" role="alert">{alert}</div>
</footer>

<style>
  .status {
    grid-area: band;
    display: flex;
    align-items: stretch;
    min-width: 0;
    height: var(--h-band);
    padding: 0 var(--s-1);
    border-top: 1px solid var(--c-hair);
    background: var(--c-ink);
    font: 400 var(--t-12) / 1 var(--f-ja);
  }
  .status:focus {
    outline: none;
  }
  .status > * {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0 var(--s-2);
    border: 0;
    background: none;
    color: var(--c-muted);
    white-space: nowrap;
  }
  .status button {
    cursor: pointer;
    font: inherit;
  }
  @media (hover: hover) and (pointer: fine) {
    .status button:hover {
      background: var(--c-layer-2);
      color: var(--c-paper);
    }
  }
  .status button[aria-pressed='true'],
  .on,
  .state {
    color: var(--c-paper);
  }
  .status .err {
    color: var(--c-signal);
  }
  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border: 1.5px solid currentColor;
    border-radius: 50%;
  }
  .dot.full {
    background: currentColor;
  }
  .bar {
    width: 5.625rem;
    height: 0.25rem;
    overflow: hidden;
    border-radius: 2px;
    background: var(--c-layer-3);
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--c-accent);
    transform-origin: left center;
    transition: transform 200ms linear;
  }
  .mono {
    font-family: var(--f-mono);
  }
  .next {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--c-paper);
    text-overflow: ellipsis;
  }
  .next:focus {
    outline: none;
  }
  .cause {
    margin-right: 0.375rem;
  }
  .cause.fail {
    color: var(--c-signal);
  }
  @media (max-width: 1099px) {
    .opt {
      display: none;
    }
  }
  /* Phones: the sentence is in the bar's action and the sheet; the row keeps the 親機, state and toggles. */
  @media (max-width: 767px) {
    .next {
      display: none;
    }
  }
  @media (max-width: 767px), (pointer: coarse) {
    .status button {
      min-height: 44px;
      min-width: 44px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .bar i {
      transition: none;
    }
  }
</style>
