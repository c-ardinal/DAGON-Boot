<script lang="ts">
  import { HELP, MENU, MODE, PRODUCT, SIM } from '../app/copy.ts';
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
    /** It opened on the keys. */
    helpKeys?: boolean;
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
    {#if actions}<div class="actions">{@render actions()}</div>{/if}
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
  <section id="help" transition:rise={{ dur: '--dur-quick' }} class="help" aria-label={HELP.heading} use:dismiss={() => (helpOpen = false)}>
    <button class="button-icon close" aria-label={HELP.close} title={HELP.close} onclick={() => (helpOpen = false)}><span class="layer"><Icon name="x" /></span></button>
    <dl class="rows">
      {#each HELP.rows as [when, what] (when)}
        <dt>{when}</dt>
        <dd>{what}</dd>
      {/each}
    </dl>
    <details open={helpKeys}>
      <summary>{HELP.keysHeading}</summary>
      <dl class="keys">
        {#each HELP.keys as [key, does] (key)}
          <dt translate="no">{key}</dt>
          <dd>{does}</dd>
        {/each}
      </dl>
    </details>
  </section>
{/if}

<style>
  /* TopBar: brand left, mode toggle centre, actions right. The two sides stand equal so the toggle is centred; the name gives way first, the actions never do. */
  header {
    grid-area: header;
    position: relative;
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
    gap: var(--p-icon-gap);
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
  .actions {
    display: flex;
    align-items: center;
    gap: var(--s-3);
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
  .close {
    position: absolute;
    top: var(--s-2);
    right: var(--s-2);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--s-2) var(--s-4);
    margin: 0;
    font: 400 var(--t-small) / 1.45 var(--f-ja);
  }
  dt {
    color: var(--c-muted);
  }
  dd {
    margin: 0;
  }
  summary {
    font: 700 var(--t-small) / 1.45 var(--f-ja);
    cursor: pointer;
  }
  .keys {
    margin-top: var(--s-2);
  }
</style>
