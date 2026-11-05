<script lang="ts">
  import { DRAWER, LOCK, NODE, STATUS } from '../app/copy.ts';

  interface Props {
    /** The connection: 「○ 未接続」 or 「● 接続済 [1] メイン（親機）」. */
    hub: string;
    /** The text button at the right end: 接続する / 切断する. */
    hubAction: string;
    onhub?: () => void;
    /** Why connecting or disconnecting is held now (the button stays focusable and says it). */
    hubLocked?: string | null;
    /** The lock table's state now (待機, 書き込み中 …). */
    state: string;
    cli: string;
    /** The CLI label opens the CLI sheet (its link, features and key). */
    oncli?: () => void;
    key: string | null;
    problems: number;
    /** Nodes writing cannot reach (◇ 書き込めない n), absent when none. */
    unreachable: number;
    /** 「書き込み 1/3 · 53%」 while writing runs, else null. */
    progress: string | null;
    mode: string;
    saved: string;
    /** What to do next, for screen readers and the focus after an action (the notices show it on screen). */
    sentence: string;
    status: string;
    alert: string;
    nextEl?: HTMLElement;
  }

  let { hub, hubAction, onhub, hubLocked = null, state, cli, oncli, key, problems, unreachable, progress, mode, saved, sentence, status, alert, nextEl = $bindable() }: Props = $props();
</script>

<!-- StatusRow: information left, the connection's action right. -->
<footer id="band" class="status" aria-label={STATUS.label}>
  <span class="on">{hub}</span>
  <span class="on" title={LOCK.stateTitle}><span class="visually-hidden">{LOCK.stateLabel} </span>{state}</span>
  {#if oncli}<button class="item" onclick={oncli}>{cli}</button>{:else}<span>{cli}</span>{/if}
  <span title={STATUS.keyTitle}>{STATUS.key} {key ?? '—'}</span>
  <span class:err={problems > 0}>{DRAWER.problems} {problems}</span>
  {#if unreachable}<span>{NODE.notYet} {unreachable}</span>{/if}
  {#if progress}<span class="progress">{progress}</span>{/if}
  <span class="grow" tabindex="-1" bind:this={nextEl}><span class="visually-hidden">{sentence}</span></span>
  <span>{mode}</span>
  {#if saved}<span>{saved}</span>{/if}
  {#if onhub}
    <button class="text" data-control="connect" aria-disabled={hubLocked !== null} aria-describedby={hubLocked ? 'hub-why' : undefined} onclick={onhub}>{hubAction}</button>
    {#if hubLocked}<span id="hub-why" class="visually-hidden">{hubLocked}</span>{/if}
  {/if}
  <div class="visually-hidden" role="status">{status}</div>
  <div class="visually-hidden" role="alert">{alert}</div>
</footer>

<style>
  .status {
    grid-area: band;
    display: flex;
    align-items: center;
    gap: var(--s-5);
    min-width: 0;
    height: var(--h-status);
    padding: 0 var(--m-edge);
    border-top: 1px solid var(--c-layer-2);
    background: var(--c-ink);
    color: var(--c-muted);
    font: 400 var(--t-caption) / var(--h-status) var(--f-ja);
    white-space: nowrap;
  }
  .on {
    color: var(--c-paper);
  }
  .err {
    color: var(--c-signal);
  }
  .progress {
    color: var(--c-progress);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .grow:focus {
    outline: none;
  }
  button {
    height: var(--h-text-button);
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    line-height: 1;
    cursor: pointer;
  }
  .item {
    color: inherit;
  }
  /* Button/Text: the connection's action, accent, no border and no fill. */
  .text {
    color: var(--c-accent);
    font: 700 var(--t-small) / 1 var(--f-ja);
  }
  .text[aria-disabled='true'] {
    color: var(--c-muted);
    cursor: not-allowed;
  }
  button:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
</style>
