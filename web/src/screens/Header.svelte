<script lang="ts">
  import { CLI_NONE, HELP, HUB, HUB_NAME, ORDER, PRODUCT, STEP } from '../app/copy.ts';
  import type { HubState, Step } from '../app/machine.ts';

  interface Props {
    step: Step | null;
    hub: HubState;
    /** Show the button that opens the update-order drawer (768–1279 px). */
    drawer: boolean;
    drawerOpen: boolean;
    ondrawer: () => void;
    drawerButton?: HTMLButtonElement;
  }

  let { step, hub, drawer, drawerOpen, ondrawer, drawerButton = $bindable() }: Props = $props();
  let helpOpen = $state(false);
</script>

<header>
  <span class="product" translate="no">{PRODUCT}</span>
  {#if drawer}
    <button class="secondary" aria-expanded={drawerOpen} aria-controls="drawer" bind:this={drawerButton} onclick={ondrawer}>{ORDER.heading}</button>
  {/if}
  <ol class="steps">
    {#if step !== null}
      <li aria-current="step">
        {#if step === 'recover'}
          {STEP.names.recover}
        {:else}
          {STEP.label} <span class="step-n">{String(step).padStart(2, '0')}{STEP.of}</span>&ensp;{STEP.names[step]}
        {/if}
      </li>
    {/if}
  </ol>
  <span class="hub" aria-label={HUB_NAME[hub]}>{HUB[hub]}</span>
  <span class="cli">{CLI_NONE}</span>
  <button class="secondary" aria-expanded={helpOpen} aria-controls="help" onclick={() => (helpOpen = !helpOpen)}>{HELP.heading}</button>
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
    font: 500 var(--t-mono-size) / 1 var(--f-mono);
    letter-spacing: 0.12em;
  }
  .steps {
    margin: 0 auto 0 0;
    padding: 0;
    list-style: none;
  }
  .step-n {
    font-family: var(--f-version);
    font-variant-numeric: tabular-nums;
  }
  .cli {
    color: var(--c-muted);
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
  @media (max-width: 767px) {
    header {
      flex-wrap: wrap;
      gap: var(--s-2) var(--s-4);
      padding: var(--s-2) var(--s-4);
    }
  }
</style>
