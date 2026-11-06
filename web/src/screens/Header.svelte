<script lang="ts">
  import { DEFINE, HELP, HUB_TOGGLE, MENU, MODE, PRODUCT, SIM } from '../app/copy.ts';
  import { rise } from '../app/motion.ts';
  import { dismiss } from './dismiss.ts';
  import Icon from './Icon.svelte';
  import type { Snippet } from 'svelte';

  type Mode = 'operate' | 'define';

  interface Props {
    mode?: Mode;
    /** Absent on the start screen: no mode toggle there. */
    onmode?: (m: Mode) => void;
    /** Switching is held while writing or recovery runs. */
    modeLocked?: boolean;
    /** 運用 cannot be entered: no 構成 or problems. */
    operateLocked?: boolean;
    /** 運用 was pressed while locked: the page says why. */
    onlocked?: () => void;
    menuOpen?: boolean;
    onmenu?: () => void;
    /** `dagon demo`: the hub is simulated (模擬, always shown). */
    sim?: boolean;
    /** The open project's name. */
    title?: string;
    /** The mode's main actions (header-size buttons), right of the toggle. */
    actions?: Snippet;
    /** The menu's popover, anchored to ☰. */
    menu?: Snippet;
    /** 困ったとき is open (the menu opens it). */
    helpOpen?: boolean;
    /** It opened on the keys: their own popover. */
    helpKeys?: boolean;
    /** 準備's edit tools: undo, redo and the graph／表 toggle (absent in 書込 and on the start screen). */
    edit?: { canUndo: boolean; canRedo: boolean; onundo: () => void; onredo: () => void; view: 'graph' | 'table'; onview: (v: 'graph' | 'table') => void };
    /** 接続する／切断する: the one connection action, beside the main actions (absent on the start screen). */
    hub?: { connected: boolean; locked: string | null; hint?: string; onpress: () => void };
  }

  let {
    mode = 'operate',
    onmode,
    modeLocked = false,
    operateLocked = false,
    onlocked,
    menuOpen = false,
    onmenu,
    sim = false,
    title = '',
    actions,
    menu,
    helpOpen = $bindable(false),
    helpKeys = false,
    edit,
    hub,
  }: Props = $props();

  /** 定義 → 運用: the order the user works in. */
  const MODES = [
    ['define', MODE.define],
    ['operate', MODE.operate],
  ] as const;
  const off = (m: Mode) => (modeLocked && mode !== m) || (m === 'operate' && operateLocked && mode !== m);
  function pick(m: Mode) {
    if (mode === m) return;
    if (m === 'operate' && operateLocked && !modeLocked) return onlocked?.();
    if (!off(m)) onmode?.(m);
  }
</script>

<!-- TopBar: brand left, mode toggle centre, then what every sequence needs: edit tools (準備), main actions, the connection, ☰. -->
<header>
  <div class="left">
    <span class="brand" translate="no"><img class="mark" src="./icons/dagon-256.png" alt="" width="24" height="24" /><span class="name">{PRODUCT}</span></span>
    {#if title}<span class="project" translate="no">{title}</span>{/if}
    {#if sim}<span class="sim" role="note" aria-label={SIM.label} title={SIM.label}>{SIM.badge}</span>{/if}
  </div>
  {#if onmode}
    <div class="modes" role="radiogroup" aria-label={MODE.label}>
      {#each MODES as [m, label] (m)}
        <button role="radio" aria-checked={mode === m} aria-disabled={off(m)} onclick={() => pick(m)}>{label}</button>
      {/each}
    </div>
  {/if}
  <div class="right">
    {#if edit}
      <!-- Undo, redo and the view toggle: where editors keep them (Jakob); when the bar is too narrow for them they live in ☰ alone. -->
      <div class="edit">
        <button class="button-icon" data-control="edit" aria-disabled={!edit.canUndo} aria-label={DEFINE.undo} title="{DEFINE.undo} (Ctrl+Z)" onclick={() => edit?.canUndo && edit.onundo()}><span class="layer"><Icon name="undo-2" /></span></button>
        <button class="button-icon" data-control="edit" aria-disabled={!edit.canRedo} aria-label={DEFINE.redo} title="{DEFINE.redo} (Ctrl+Shift+Z)" onclick={() => edit?.canRedo && edit.onredo()}><span class="layer"><Icon name="redo-2" /></span></button>
        <div class="seg" role="radiogroup" aria-label="{DEFINE.graph}・{DEFINE.table}">
          <button class="button-icon" role="radio" aria-checked={edit.view === 'graph'} aria-label={DEFINE.graph} title={DEFINE.graph} onclick={() => edit?.onview('graph')}><span class="layer"><Icon name="network" /></span></button>
          <button class="button-icon" role="radio" aria-checked={edit.view === 'table'} aria-label={DEFINE.table} title={DEFINE.table} onclick={() => edit?.onview('table')}><span class="layer"><Icon name="table-2" /></span></button>
        </div>
      </div>
    {/if}
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
    {#if hub}
      <!-- Button/Text: the connection's state and its one action, next to what needs it (書き込む). -->
      <button class="hub" class:on={hub.connected} data-control="connect" title={hub.locked ?? hub.hint} aria-disabled={hub.locked !== null} onclick={hub.onpress}>
        <Icon name={hub.connected ? 'plug' : 'unplug'} size="var(--i-s)" /><span>{hub.connected ? HUB_TOGGLE.disconnect : HUB_TOGGLE.connect}</span>
      </button>
    {/if}
    {#if onmenu}
      <!-- Pressing ☰ again closes it: the press is inside the anchor. -->
      <span class="anchor" use:dismiss={() => menuOpen && onmenu()}>
        <button class="button-icon" aria-label={MENU.button} title={MENU.button} aria-haspopup="true" aria-expanded={menuOpen} aria-controls="menu" onclick={onmenu}
          ><span class="layer"><Icon name="menu" /></span></button
        >
        {@render menu?.()}
      </span>
    {/if}
  </div>
</header>

{#if helpOpen}
  <!-- 困ったとき and キー操作: two popovers under ☰ with their own content, so each menu item opens what it names. -->
  <section id="help" transition:rise={{ dur: '--dur-quick' }} class="help" aria-label={helpKeys ? HELP.keysHeading : HELP.heading} use:dismiss={() => (helpOpen = false)}>
    <h2>{helpKeys ? HELP.keysHeading : HELP.heading}</h2>
    <button class="button-icon close" aria-label={helpKeys ? HELP.keysClose : HELP.close} title={helpKeys ? HELP.keysClose : HELP.close} onclick={() => (helpOpen = false)}><span class="layer"><Icon name="x" /></span></button>
    {#if helpKeys}
      <dl class="keys">
        {#each HELP.keys as [key, does] (key)}
          <dt translate="no"><kbd>{key}</kbd></dt>
          <dd>{does}</dd>
        {/each}
      </dl>
    {:else}
      <dl class="rows">
        {#each HELP.rows as [when, what] (when)}
          <dt>{when}</dt>
          <dd>{what}</dd>
        {/each}
      </dl>
    {/if}
  </section>
{/if}

<style>
  /* The bar: brand left, mode toggle centre, tools and actions right. The two sides stand equal so the toggle is centred; the name gives way first, the actions never do. */
  header {
    grid-area: header;
    position: relative;
    container-type: inline-size;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(max-content, 1fr);
    align-items: center;
    gap: var(--p-bar-gap);
    min-width: 0;
    height: var(--h-bar);
    padding: 0 var(--m-edge);
    /* Inside the bar's height, so the controls keep their full clearance. */
    box-shadow: inset 0 -1px 0 var(--c-layer-2);
    background: var(--c-ink);
    /* Its popovers open over the canvas's columns. */
    z-index: calc(var(--z-chrome) + 1);
    white-space: nowrap;
  }
  .left,
  .right {
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .left {
    gap: var(--s-5);
    padding-left: var(--p-hit);
  }
  .right {
    grid-column: 3;
    justify-content: flex-end;
    gap: var(--s-4);
  }
  .anchor {
    position: relative;
    display: inline-flex;
  }
  .brand {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--s-2);
  }
  .mark {
    width: var(--i-l);
    height: var(--i-l);
  }
  .name {
    color: var(--c-paper);
    font: 700 var(--t-mono) / 24px var(--f-mono);
  }
  .project {
    min-width: 0;
    overflow: hidden;
    color: var(--c-paper);
    font: 500 var(--t-small) / 24px var(--f-ja);
    text-overflow: ellipsis;
  }
  /* ModeToggle: a pill, segments are pills too (radius = height / 2). */
  .modes {
    display: flex;
    flex: none;
    padding: var(--p-toggle);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
  }
  .modes button {
    height: calc(var(--h-control) - 2 * var(--p-toggle) - 2px);
    padding: 0 var(--s-4);
    border: 0;
    border-radius: var(--r-full);
    background: none;
    color: var(--c-muted);
    font: 700 var(--t-small) / 1 var(--f-ja);
    cursor: pointer;
    transition:
      background-color var(--dur-quick) var(--ease-out),
      color var(--dur-quick) var(--ease-out);
  }
  .modes button[aria-checked='true'] {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .modes button[aria-disabled='true'] {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .modes button:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
  .edit {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--s-1);
  }
  /* Below this bar width the edit group would meet the actions: it gives way to ☰, where the same items always are. */
  @container (max-width: 1100px) {
    .edit {
      display: none;
    }
  }
  /* The graph／表 segment: a pill of two icons, the current one filled. */
  .seg {
    display: flex;
    gap: 0;
    margin-left: var(--s-2);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: var(--c-ink);
  }
  .seg [aria-checked='true'] :global(.layer) {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .actions {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--s-3);
  }
  /* 接続する／切断する: accent text in a 44 px hit area; connected shows the plug on the paper. */
  .hub {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: var(--s-2);
    height: var(--h-row);
    padding: 0 var(--s-2);
    border: 0;
    background: none;
    color: var(--c-accent);
    font: 700 var(--t-small) / 1 var(--f-ja);
    white-space: nowrap;
    cursor: pointer;
    transition: color var(--dur-quick) var(--ease-out);
  }
  .hub.on {
    color: var(--c-paper);
  }
  .hub[aria-disabled='true'] {
    color: var(--c-muted);
    cursor: not-allowed;
  }
  .hub:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: -2px;
    border-radius: var(--r-s);
  }
  /* 模擬: on the paper, not the signal colour (it is no fault), and never animated. */
  .sim {
    flex: none;
    padding: var(--s-1) var(--s-3);
    border: 1px dashed var(--c-ink);
    border-radius: var(--r-full);
    background: var(--c-paper);
    color: var(--c-ink);
    font: 700 var(--t-caption) / 1 var(--f-ja);
    letter-spacing: 0.08em;
  }
  /* 困ったとき: a popover under ☰, over the page; nothing under it moves. */
  .help {
    position: absolute;
    z-index: var(--z-announce);
    top: var(--h-bar);
    right: var(--m-edge);
    display: grid;
    gap: var(--s-4);
    width: min(var(--w-modal), calc(100% - 2 * var(--m-edge)));
    max-height: calc(100vh - var(--h-bar) - var(--s-4));
    padding: var(--s-5);
    overflow: auto;
    border: 1px solid var(--c-paper);
    border-radius: var(--r-m);
    background: var(--c-layer-1);
    white-space: normal;
  }
  .help h2 {
    margin: 0;
    font: 700 var(--t-heading) / 28px var(--f-ja);
  }
  .close {
    position: absolute;
    top: var(--s-2);
    right: var(--s-2);
  }
  dl {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 2fr);
    gap: var(--s-3) var(--s-4);
    margin: 0;
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  .keys {
    grid-template-columns: max-content 1fr;
  }
  dt {
    color: var(--c-paper);
    font-weight: 700;
  }
  dd {
    margin: 0;
    color: var(--c-muted);
  }
  kbd {
    display: inline-block;
    padding: 0 var(--s-2);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    font: 400 var(--t-caption) / 20px var(--f-mono);
  }
</style>
