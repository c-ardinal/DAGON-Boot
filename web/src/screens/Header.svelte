<script lang="ts">
  import { HELP, MENU, MODE, PRODUCT, SIM } from '../app/copy.ts';
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
    /** 困ったとき is open (the menu opens it). */
    helpOpen?: boolean;
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
    helpOpen = $bindable(false),
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
      <button class="button-icon" aria-label={MENU.button} title={MENU.button} aria-expanded={menuOpen} aria-controls="menu" onclick={onmenu}
        ><span class="layer"><Icon name="menu" /></span></button
      >
    {/if}
  </div>
</header>

<section id="help" class="help" hidden={!helpOpen} aria-label={HELP.heading}>
  <dl class="rows">
    {#each HELP.rows as [when, what] (when)}
      <dt>{when}</dt>
      <dd>{what}</dd>
    {/each}
  </dl>
  <details>
    <summary>{HELP.keysHeading}</summary>
    <dl class="keys">
      {#each HELP.keys as [key, does] (key)}
        <dt translate="no">{key}</dt>
        <dd>{does}</dd>
      {/each}
    </dl>
  </details>
</section>

<style>
  /* TopBar: brand left, mode toggle centre, actions right. */
  header {
    grid-area: header;
    display: flex;
    align-items: center;
    gap: var(--p-bar-gap);
    min-width: 0;
    height: var(--h-bar);
    padding: 0 var(--m-edge);
    border-bottom: 1px solid var(--c-layer-2);
    background: var(--c-ink);
    z-index: var(--z-chrome);
    white-space: nowrap;
  }
  .left,
  .right {
    display: flex;
    flex: 1 1 0;
    align-items: center;
    min-width: 0;
  }
  .left {
    gap: var(--s-5);
    padding-left: var(--p-hit);
  }
  .right {
    justify-content: flex-end;
    gap: var(--p-icon-gap);
  }
  .brand {
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
    height: calc(var(--h-control) - 2 * var(--p-toggle));
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
    padding: var(--s-1) var(--s-3);
    border: 1px dashed var(--c-ink);
    border-radius: var(--r-full);
    background: var(--c-paper);
    color: var(--c-ink);
    font: 700 var(--t-caption) / 1 var(--f-ja);
    letter-spacing: 0.08em;
  }
  .help {
    grid-area: help;
    display: grid;
    gap: var(--s-4);
    padding: var(--s-5);
    border-bottom: 1px solid var(--c-line);
    background: var(--c-layer-1);
  }
  .help[hidden] {
    display: none;
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
