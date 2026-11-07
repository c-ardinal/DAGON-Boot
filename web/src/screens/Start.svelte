<script lang="ts">
  import { tick } from 'svelte';
  import { BAND, CLI_LINK, MENU, PRODUCT, START } from '../app/copy.ts';
  import { inside, type Start } from '../app/start.svelte.ts';
  import { DEMO_SITE } from '../app/site.ts';
  import { fade } from '../app/motion.ts';
  import Icon from './Icon.svelte';
  import Modal from './Modal.svelte';

  interface Props {
    s: Start;
    /** dagon serve answers; otherwise the page says how to start it. */
    served: boolean;
  }

  let { s, served }: Props = $props();

  /** The open modal: making (how, then where) or opening. */
  let modal = $state<'how' | 'where' | 'open' | null>(null);
  let how = $state<'new' | 'sample'>('new');

  /** A folder that cannot be written or made is M35; the others are their own. */
  const fail = $derived.by(() => {
    const f = s.failed;
    if (!f) return null;
    if (f === 'failed') return s.making ? BAND.M35 : START.missing;
    return f === 'refused' && s.making ? { chip: START.cantMake, sentence: START.refused.sentence } : START[f];
  });

  let first = $state<HTMLButtonElement>();
  $effect(() => {
    void tick().then(() => first?.focus());
  });

  function close() {
    modal = null;
    s.clear();
  }
  async function toWhere() {
    await s.choose(how);
    // Without serve the page starts an unsaved 構成 at once (development stand-ins).
    modal = s.online ? 'where' : null;
  }
  async function toOpen() {
    modal = 'open';
    await s.choose('open');
  }
  /** Backspace goes up a folder. */
  function listKeys(e: KeyboardEvent) {
    if (e.key !== 'Backspace' || !s.list?.parent || (e.target as HTMLElement).tagName === 'INPUT') return;
    e.preventDefault();
    void s.browse(s.list.parent);
  }
</script>

{#snippet folders(opening: boolean)}
  {#if s.loading}
    <ul class="fs" aria-busy="true">
      {#each [0, 1, 2] as k (k)}<li><span class="skeleton row-sk"></span></li>{/each}
    </ul>
  {:else if s.list}
    {@const list = s.list}
    <div class="crumb">
      <button class="button-secondary small" aria-disabled={!list.parent} title={list.parent ? undefined : START.upWhy} onclick={() => list.parent && void s.browse(list.parent)}>{START.up}</button>
      <span class="mono" translate="no">{list.path}</span>
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <ul class="fs" onkeydown={listKeys}>
      {#each list.folders as f (f.name)}
        {@const opens = opening && (f.project || f.yaml === true)}
        <li>
          <button class="row" class:opens onclick={() => void (opens ? s.open(inside(list.path, f.name)) : s.browse(inside(list.path, f.name)))}>
            <Icon name="folder" size="var(--i-s)" /><span translate="no">{f.name}</span>{#if f.project}<span class="tag">{START.project}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
{/snippet}

{#snippet failure()}
  {#if fail}<p class="fail"><b>{fail.chip}</b> {fail.sentence}</p>{/if}
{/snippet}

<main id="canvas" transition:fade class="start" tabindex="-1" aria-labelledby="start-h">
  <h1 id="start-h" class="title" translate="no">{PRODUCT}</h1>
  <p class="sub">{START.sub}</p>
  <div class="choices">
    <button class="button-primary" bind:this={first} aria-disabled={!s.usable('new')} title={s.usable('new') || DEMO_SITE ? undefined : CLI_LINK.needCli} onclick={() => s.usable('new') && (modal = 'how')}>{START.new}</button>
    <button class="button-secondary" data-control={DEMO_SITE ? 'open' : undefined} aria-disabled={!s.usable('open')} title={s.usable('open') || DEMO_SITE ? undefined : CLI_LINK.needCli} onclick={() => s.usable('open') && void toOpen()}>{START.open}</button>
  </div>
  {#if !served && !s.usable('new')}<p class="why">{CLI_LINK.needCli}</p>{/if}
  <section class="recent" aria-labelledby="recent-h">
    <h2 id="recent-h">{START.recent}</h2>
    {#if s.recent.length === 0}
      <p class="muted">{START.recentNone}</p>
    {:else}
      <ul>
        {#each s.recent as r (r.path)}
          <li>
            <button class="row" aria-disabled={!s.online} title={s.online ? undefined : CLI_LINK.needCli} onclick={() => s.online && void s.open(r.path)}>
              <Icon name="folder" size="var(--i-s)" /><span class="name">{r.name}</span><span class="mono path" translate="no">{r.path}</span><Icon name="chevron-right" size="var(--i-s)" />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</main>

<footer id="band" class="band" tabindex="-1">
  {#if fail && !modal}{@render failure()}{/if}
</footer>

{#if modal === 'how'}
  <Modal title={START.create.title} onclose={close}>
    <h3>{START.create.how}</h3>
    <div class="options" role="radiogroup" aria-label={START.create.how}>
      {#each [['new', START.create.empty], ['sample', START.create.sample]] as const as [c, label] (c)}
        <button class="option" role="radio" aria-checked={how === c} onclick={() => (how = c)}><span class="radio" aria-hidden="true"></span>{label}</button>
      {/each}
    </div>
    {#snippet actions()}
      <button class="button-secondary" onclick={close}>{START.create.cancel}</button>
      <button class="button-primary" onclick={() => void toWhere()}>{START.create.next}</button>
    {/snippet}
  </Modal>
{:else if modal === 'where'}
  <Modal title={START.create.title} onclose={close}>
    {#if s.choice === 'sample'}<p class="muted">{START.create.sample}</p>{/if}
    <label class="field">{START.name}<input bind:value={s.name} translate="no" autocomplete="off" spellcheck="false" /></label>
    <div class="field">{START.place}{@render folders(false)}</div>
    {#if s.list && s.name.trim()}
      <h3>{START.create.where}</h3>
      <p class="mono" translate="no">{inside(s.list.path, s.name.trim())}</p>
    {/if}
    {@render failure()}
    {#snippet actions()}
      <button class="button-secondary" onclick={() => (modal = 'how')}>{START.create.back}</button>
      <button class="button-primary" aria-disabled={!s.list || s.loading || !s.name.trim()} title={!s.list || s.loading || !s.name.trim() ? START.goWhy : undefined} onclick={() => void s.make()}>{START.create.go}</button>
    {/snippet}
  </Modal>
{:else if modal === 'open'}
  <Modal title={START.open} onclose={close}>
    {#if s.asking}
      <p>{START.initAsk}</p>
    {:else}
      {@render folders(true)}
    {/if}
    {@render failure()}
    {#snippet actions()}
      {#if s.asking}
        <button class="button-secondary" onclick={() => void s.init(false)}>{MENU.cancel}</button>
        <button class="button-primary" onclick={() => void s.init(true)}>{START.open}</button>
      {/if}
    {/snippet}
  </Modal>
{/if}

<style>
  /* Start: one centred column (w-modal), title → words → actions → recent. */
  .start {
    grid-area: canvas;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: var(--s-4);
    width: min(var(--w-modal), calc(100% - 2 * var(--s-4)));
    min-height: 0;
    margin: 0 auto;
    padding: var(--s-6) 0 var(--s-4);
    overflow: auto;
  }
  .start:focus {
    outline: none;
  }
  .title {
    margin: 0;
    font: 700 var(--t-title) / 40px var(--f-ja);
  }
  .sub {
    margin: 0;
    color: var(--c-muted);
    font: 400 var(--t-small) / 24px var(--f-ja);
  }
  .choices {
    display: flex;
    gap: var(--s-2);
  }
  .why {
    margin: 0;
    color: var(--c-paper);
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  h2,
  h3 {
    margin: 0;
    color: var(--c-muted);
    font: 700 var(--t-caption) / 16px var(--f-ja);
    letter-spacing: 0.06em;
  }
  ul {
    display: grid;
    margin: var(--s-3) 0 0;
    padding: 0;
    list-style: none;
  }
  li {
    border-bottom: 1px solid var(--c-layer-2);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    width: 100%;
    min-height: 40px;
    padding: 0 var(--s-3);
    border: 0;
    background: none;
    color: var(--c-paper);
    text-align: left;
    font: 400 var(--t-small) / 24px var(--f-ja);
    cursor: pointer;
  }
  .row :global(.icon) {
    color: var(--c-muted);
  }
  .row .path {
    margin-left: auto;
    color: var(--c-muted);
  }
  .row:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: calc(-1 * var(--st-w-focus));
  }
  .row[aria-disabled='true'] {
    color: var(--c-muted);
    cursor: default;
  }
  @media (hover: hover) and (pointer: fine) {
    .row:not([aria-disabled='true']):hover {
      background: var(--c-layer-2);
    }
  }
  .row.opens {
    font-weight: 700;
  }
  .tag {
    margin-left: auto;
    color: var(--c-muted);
    font: 700 var(--t-caption) / 1 var(--f-ja);
  }
  .mono {
    font: 400 var(--t-mono) / 24px var(--f-mono);
    overflow-wrap: anywhere;
  }
  .muted {
    margin: 0;
    color: var(--c-muted);
  }
  .crumb {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    margin-top: var(--s-2);
  }
  .small {
    height: var(--h-button-small);
    padding: 0 var(--s-3);
    font-size: var(--t-small);
  }
  .fs {
    max-height: 200px;
    overflow: auto;
  }
  .row-sk {
    display: block;
    height: 40px;
    margin: var(--s-1) 0;
  }
  .options {
    display: grid;
    gap: var(--s-3);
  }
  .option {
    display: flex;
    align-items: center;
    gap: var(--s-3);
    height: var(--h-button);
    padding: 0 var(--s-4);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 700 var(--t-body) / 1 var(--f-ja);
    cursor: pointer;
  }
  .option[aria-checked='true'] {
    border-color: var(--c-paper);
    background: var(--c-layer-2);
  }
  .option:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
  .radio {
    width: var(--i-m);
    height: var(--i-m);
    border: 2px solid currentColor;
    border-radius: var(--r-full);
  }
  .option[aria-checked='true'] .radio {
    background: radial-gradient(currentColor 0 3px, transparent 3.5px);
  }
  .field {
    display: grid;
    gap: var(--s-2);
  }
  .field input {
    height: var(--h-button);
    padding: 0 var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 400 var(--t-mono) / 24px var(--f-mono);
  }
  .field input:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
  }
  .fail {
    margin: 0;
  }
  .fail b {
    color: var(--c-signal);
  }
  .band {
    grid-area: band;
    display: flex;
    align-items: center;
    gap: var(--s-4);
    min-height: var(--h-status);
    padding: 0 var(--m-edge);
    border-top: 1px solid var(--c-layer-2);
    color: var(--c-paper);
    font: 500 var(--t-small) / 20px var(--f-ja);
  }
  .band p {
    margin: 0;
  }
</style>
