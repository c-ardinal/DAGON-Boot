<script lang="ts">
  import { LOCK, STATUS } from '../app/copy.ts';
  import Icon from './Icon.svelte';

  interface Props {
    /** The 親機 this page is connected to, or the words for none. */
    hub: string;
    connected: boolean;
    /** The text button at the right end: 接続する / 切断する. */
    hubAction: string;
    onhub?: () => void;
    /** Why connecting or disconnecting is held now (the button stays focusable and says it). */
    hubLocked?: string | null;
    /** The lock table's state now (待機, 書き込み中 …), for screen readers. */
    state: string;
    /** How dagon serve stands; the icon opens its sheet. */
    cli: string;
    cliReady: boolean;
    oncli?: () => void;
    /** The saving state of the 構成 ('' without a project file). */
    saved: string;
    savedOk: boolean;
    /** The one sentence: what is going on and what to do next. */
    sentence: string;
    status: string;
    alert: string;
    nextEl?: HTMLElement;
  }

  let { hub, connected, hubAction, onhub, hubLocked = null, state, cli, cliReady, oncli, saved, savedOk, sentence, status, alert, nextEl = $bindable() }: Props = $props();
</script>

<!-- The band: where the 親機 stands, the one sentence where the eye lands after an action, the connection's action at the end. -->
<footer id="band" class="status" aria-label={STATUS.label}>
  <span class="hub" class:on={connected} title={hub}><Icon name={connected ? 'plug' : 'unplug'} size="var(--i-s)" /><span class="name">{hub}</span></span>
  <p class="sentence" tabindex="-1" bind:this={nextEl}><span class="visually-hidden">{LOCK.stateLabel} {state} </span>{sentence}</p>
  {#if saved}<span class="mark" class:ok={savedOk} title={saved} role="img" aria-label={saved}><Icon name={savedOk ? 'check' : 'circle-dashed'} size="var(--i-s)" /></span>{/if}
  {#if oncli}<button class="mark" class:ok={cliReady} title={cli} aria-label={cli} onclick={oncli}><Icon name="terminal" size="var(--i-s)" /></button>{/if}
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
    flex: none;
    align-items: center;
    gap: var(--s-5);
    min-width: 0;
    min-height: var(--h-status);
    padding: var(--s-1) var(--m-edge);
    border-top: 1px solid var(--c-layer-2);
    background: var(--c-ink);
    color: var(--c-muted);
  }
  .hub {
    display: flex;
    flex: 0 1 auto;
    align-items: center;
    max-width: var(--w-parts);
    gap: var(--s-2);
    min-width: var(--i-s);
    font: 400 var(--t-caption) / 16px var(--f-ja);
  }
  .hub.on {
    color: var(--c-paper);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The sentence takes the room it needs: it wraps, and the band grows over the canvas, never pushing it. */
  .sentence {
    flex: 1 1 0;
    min-width: 0;
    margin: 0;
    color: var(--c-paper);
    font: 500 var(--t-small) / 20px var(--f-ja);
  }
  .sentence:focus {
    outline: none;
  }
  .mark {
    display: inline-flex;
    flex: none;
    color: var(--c-muted);
  }
  .mark.ok {
    color: var(--c-paper);
  }
  button {
    flex: none;
    height: var(--h-text-button);
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    line-height: 1;
    cursor: pointer;
  }
  /* Button/Text: the connection's action, accent, no border and no fill. */
  .text {
    color: var(--c-accent);
    font: 700 var(--t-small) / 1 var(--f-ja);
    white-space: nowrap;
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
