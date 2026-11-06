<script lang="ts">
  import { rise } from '../app/motion.ts';
  import { dismiss } from './dismiss.ts';
  import Icon from './Icon.svelte';

  interface Props {
    label: string;
    /** The button's own action: the default choice. */
    onmain: () => void;
    off?: boolean;
    /** Names the dropdown part and what it does. */
    pickName: string;
    /** The choices; choosing one runs it at once. */
    options: readonly { id: string; label: string; note?: string }[];
    onpick: (id: string) => void;
    /** The dropdown part cannot be used now. */
    pickOff?: boolean;
    /** The operation in the lock table. */
    control?: string;
    mainEl?: HTMLButtonElement;
  }

  let { label, onmain, off = false, pickName, options, onpick, pickOff = false, control, mainEl = $bindable() }: Props = $props();
  let open = $state(false);
  const none = $derived(pickOff || options.length === 0);
  let caret = $state<HTMLButtonElement>();
  /** Escape gives the focus back to what opened the list. */
  function close() {
    if (open && document.activeElement?.closest('.list')) caret?.focus();
    open = false;
  }
</script>

<!-- One decision, one control: the button writes the default, its dropdown part writes the chosen one. They never part or wrap. -->
<span class="split" use:dismiss={close}>
  <button class="button-primary-header main" data-control={control} aria-disabled={off} bind:this={mainEl} onclick={onmain}><span>{label}</span></button>
  <button
    class="button-primary-header caret"
    bind:this={caret}
    data-control={control}
    aria-label={pickName}
    title={pickName}
    aria-haspopup="true"
    aria-expanded={open}
    aria-disabled={none}
    onclick={() => {
      if (!none) open = !open;
    }}><span><Icon name="chevron-down" size="var(--i-s)" /></span></button
  >
  {#if open}
    <div transition:rise={{ dur: '--dur-quick' }} class="list" role="group" aria-label={pickName}>
      {#each options as o (o.id)}
        <button
          class="row"
          translate="no"
          onclick={() => {
            open = false;
            mainEl?.focus();
            onpick(o.id);
          }}>{o.label}{#if o.note}<small>{o.note}</small>{/if}</button
        >
      {/each}
    </div>
  {/if}
</span>

<style>
  .split {
    position: relative;
    display: inline-flex;
    flex: none;
  }
  .main :global(> span) {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .caret {
    margin-left: 0;
  }
  .caret :global(> span) {
    padding: 0 var(--s-2);
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }
  .list {
    position: absolute;
    z-index: var(--z-announce);
    top: 100%;
    right: 0;
    display: grid;
    min-width: 100%;
    max-height: calc(100vh - var(--h-bar) - var(--s-4));
    padding: var(--s-2) 0;
    overflow: auto;
    border: 1px solid var(--c-paper);
    border-radius: var(--r-m);
    background: var(--c-layer-1);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s-5);
    height: var(--h-row);
    padding: 0 var(--s-4);
    border: 0;
    background: none;
    color: var(--c-paper);
    font: 400 var(--t-version) / 1 var(--f-mono);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .row small {
    color: var(--c-muted);
    font: 400 var(--t-caption) / 1 var(--f-ja);
  }
  @media (hover: hover) and (pointer: fine) {
    .row:hover {
      background: var(--c-layer-2);
    }
  }
  .row:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: calc(-1 * var(--st-w-focus));
  }
</style>
