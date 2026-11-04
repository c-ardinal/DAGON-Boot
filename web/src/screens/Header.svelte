<script lang="ts">
  import { CLI_NONE, HELP, HUB, HUB_NAME, HUB_TOGGLE, MENU, MODE, ORDER, PRODUCT, SIM, STEP } from '../app/copy.ts';
  import type { HubState, Step } from '../app/machine.ts';

  interface Props {
    step: Step | null;
    hub: HubState;
    /** Show the button that opens the update-order drawer (768–1279 px). */
    drawer: boolean;
    drawerOpen: boolean;
    ondrawer: () => void;
    drawerButton?: HTMLButtonElement;
    mode?: 'operate' | 'define';
    onmode?: (m: 'operate' | 'define') => void;
    /** Switching is held while an update or recovery runs (C10). */
    modeLocked?: boolean;
    /** The CLI link (K0–K3). */
    cliLabel?: string;
    /** 運用 cannot be entered: no 構成 or problems (§5.2). */
    operateLocked?: boolean;
    /** 運用 was pressed while locked: the band says why (M69). */
    onlocked?: () => void;
    /** SC-M: the menu under the header. */
    menuOpen?: boolean;
    onmenu?: () => void;
    /** The update ran to the end (L23): all three steps are done. */
    done?: boolean;
    /** The 親機 つなぐ / 切断 toggle; absent on the start screen. */
    onhub?: () => void;
    /** Why the toggle cannot be used now (it stays focusable; pressing it shows this in the band). */
    hubLocked?: string | null;
    /** `dagon demo`: the hub is simulated (模擬, always shown). */
    sim?: boolean;
  }

  let {
    step,
    hub,
    drawer,
    drawerOpen,
    ondrawer,
    drawerButton = $bindable(),
    mode = 'operate',
    onmode,
    modeLocked = false,
    cliLabel = CLI_NONE,
    operateLocked = false,
    onlocked,
    menuOpen = false,
    onmenu,
    done = false,
    onhub,
    hubLocked = null,
    sim = false,
  }: Props = $props();
  const now = $derived(step === null ? null : STEP.of3(step));
  /** Position of the current step; recovery happens in the third (更新). */
  const at = $derived(now === 'recover' ? 3 : now);
  /** Connected (or checking): the toggle releases the hub; otherwise it connects. */
  const linked = $derived(!['idle', 'lost'].includes(hub));
  const off = (m: 'operate' | 'define') => (modeLocked && mode !== m) || (m === 'operate' && operateLocked && mode !== m);
  function pick(m: 'operate' | 'define') {
    if (mode === m) return;
    if (m === 'operate' && operateLocked && !modeLocked) return onlocked?.();
    if (!off(m)) onmode?.(m);
  }
  let helpOpen = $state(false);
</script>

<header>
  <span class="product" translate="no"><img class="mark" src="./icons/favicon-48.png" alt="" width="24" height="24" />{PRODUCT}</span>
  {#if sim}<span class="sim" role="note" aria-label={SIM.label} title={SIM.label}>{SIM.badge}</span>{/if}
  {#if onmode}
    <div class="modes" role="radiogroup" aria-label={MODE.label}>
      {#each [['operate', MODE.operate], ['define', MODE.define]] as const as [m, label] (m)}
        <button
          role="radio"
          aria-checked={mode === m}
          aria-disabled={off(m)}
          onclick={() => pick(m)}>{label}</button
        >
      {/each}
    </div>
  {/if}
  {#if drawer}
    <button class="secondary" aria-expanded={drawerOpen} aria-controls="drawer" bind:this={drawerButton} onclick={ondrawer}>{ORDER.heading}</button>
  {/if}
  <!-- 定義 has no steps; the 構成 file is the source of truth (owner 11/3). In 運用 all three always show. -->
  <ol class="steps" aria-label={STEP.label}>
    {#if mode !== 'define' && at !== null}
      {#each [1, 2, 3] as const as n (n)}
        {@const past = done || n < at}
        <li class:now={!done && n === at} class:past aria-current={!done && n === at ? 'step' : undefined}>
          <span class="step-n">{past ? '✓' : String(n).padStart(2, '0')}</span>
          {n === 3 && now === 'recover' ? STEP.names.recover : STEP.names[n]}
        </li>
      {/each}
    {/if}
  </ol>
  <span class="hub" aria-label={HUB_NAME[hub]}>{HUB[hub]}</span>
  {#if onhub}
    <button class="secondary toggle" data-control="connect" aria-disabled={hubLocked !== null} aria-describedby={hubLocked ? 'hub-why' : undefined} onclick={onhub}
      >{linked ? HUB_TOGGLE.disconnect : HUB_TOGGLE.connect}</button
    >
    {#if hubLocked}<span id="hub-why" class="visually-hidden">{hubLocked}</span>{/if}
  {/if}
  <span class="cli">{cliLabel}</span>
  <button class="secondary" aria-expanded={helpOpen} aria-controls="help" onclick={() => (helpOpen = !helpOpen)}>{HELP.heading}</button>
  {#if onmenu}
    <button class="secondary" aria-expanded={menuOpen} aria-controls="menu" onclick={onmenu}>{MENU.button}</button>
  {/if}
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
  .modes {
    display: flex;
    padding: 3px;
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
  }
  .modes button {
    padding: var(--s-1) var(--s-4);
    border: 0;
    border-radius: var(--r-full);
    background: none;
    color: var(--c-paper);
    font: 700 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
    cursor: pointer;
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
    outline: 3px solid var(--c-paper);
    outline-offset: 2px;
  }
  header {
    grid-area: header;
    display: flex;
    align-items: center;
    gap: var(--s-5);
    min-height: var(--h-header);
    padding: 0 var(--s-5);
    border-bottom: 1px solid var(--c-line);
    z-index: var(--z-chrome);
    font: 400 var(--t-small-size) / 1 var(--f-ja);
  }
  header > * {
    white-space: nowrap;
  }
  .product {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    font: 500 var(--t-mono-size) / 1 var(--f-mono);
    letter-spacing: 0.12em;
  }
  .mark {
    border-radius: 5px;
  }
  .steps {
    display: flex;
    align-items: center;
    gap: var(--s-1);
    margin: 0 auto 0 0;
    padding: 0;
    list-style: none;
  }
  .steps li {
    position: relative;
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    padding: var(--s-1) var(--s-3);
    border-radius: var(--r-full);
    color: var(--c-muted);
  }
  .steps li + li {
    margin-left: var(--s-4);
  }
  /* Steps are joined by a short rule: the order reads without words. */
  .steps li + li::before {
    content: '';
    position: absolute;
    right: 100%;
    top: 50%;
    width: var(--s-4);
    height: 1px;
    background: var(--c-line);
  }
  .steps li.past {
    color: var(--c-paper);
  }
  .steps li.now {
    background: var(--c-paper);
    color: var(--c-ink);
    font-weight: 700;
  }
  .toggle[aria-disabled='true'] {
    color: var(--c-muted);
    cursor: not-allowed;
  }
  .step-n {
    font-family: var(--f-version);
    font-variant-numeric: tabular-nums;
  }
  .cli {
    color: var(--c-muted);
  }
  /* 模擬: on the paper, not the signal colour (it is no fault), and never
     animated: it is on screen the whole demo. */
  .sim {
    padding: var(--s-1) var(--s-3);
    border: 1px dashed var(--c-ink);
    border-radius: var(--r-full);
    background: var(--c-paper);
    color: var(--c-ink);
    font: 700 var(--t-small-size) / 1 var(--f-ja);
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
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  dt {
    color: var(--c-muted);
  }
  dd {
    margin: 0;
  }
  summary {
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
    cursor: pointer;
  }
  .keys {
    margin-top: var(--s-2);
  }
  @media (max-width: 1279px) {
    header {
      gap: var(--s-4);
      padding: 0 var(--s-4);
    }
    header .secondary {
      padding: 0 var(--s-4);
    }
  }
  /*
   * 767 px and below (C-390): rows on purpose, in the DOM (and Tab) order —
   * the product and the modes; the three steps (運用); the 親機 state, its
   * toggle and the CLI state; then 困ったとき and メニュー side by side.
   */
  @media (max-width: 767px) {
    header {
      display: grid;
      grid-template-columns: auto 1fr 1fr;
      align-items: center;
      gap: var(--s-2);
      padding: var(--s-2) var(--s-3) var(--s-3);
    }
    header .secondary,
    .modes button {
      padding: 0 var(--s-3);
    }
    .product {
      grid-column: 1 / 3;
    }
    .modes {
      justify-self: end;
    }
    .steps {
      grid-column: 1 / -1;
      margin: 0;
    }
    .steps:empty {
      display: none;
    }
    .hub,
    .cli {
      grid-column: 1;
    }
    .toggle {
      grid-column: 2 / -1;
      justify-self: start;
    }
  }
</style>
