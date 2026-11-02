<script lang="ts">
  import { tick } from 'svelte';
  import { BAND, BUTTON, CLI_LINK, START } from '../app/copy.ts';
  import { inside, type Start, type StartChoice } from '../app/start.svelte.ts';

  interface Props {
    s: Start;
    /** dagon serve answers (K2); otherwise C11 says how to start it. */
    served: boolean;
  }

  let { s, served }: Props = $props();

  const CHOICES: [StartChoice, string, string][] = [
    ['new', '01', START.new],
    ['sample', '02', START.sample],
    ['open', '03', START.open],
  ];
  const making = $derived(s.choice === 'new' || s.choice === 'sample');
  /** A folder that cannot be written or made is M35; the others are their own (案). */
  const fail = $derived.by(() => {
    const f = s.failed;
    if (!f) return null;
    if (f === 'failed') return s.making ? BAND.M35 : START.missing;
    return f === 'refused' && s.making ? { chip: START.cantMake, sentence: START.refused.sentence } : START[f];
  });

  const buttons = $state<HTMLButtonElement[]>([]);
  let listEl = $state<HTMLElement>();
  // §7.2b: focus starts on 『新しく作る』.
  $effect(() => {
    void tick().then(() => buttons[0]?.focus());
  });

  async function into(path: string) {
    await s.browse(path);
    await tick();
    listEl?.querySelector<HTMLButtonElement>('button')?.focus();
  }

  /** Backspace goes up a folder (§7.2b a11y). */
  function listKeys(e: KeyboardEvent) {
    if (e.key !== 'Backspace' || !s.list?.parent || (e.target as HTMLElement).tagName === 'INPUT') return;
    e.preventDefault();
    void into(s.list.parent);
  }
</script>

<main id="canvas" class="start" tabindex="-1" aria-labelledby="start-h">
  <h1 id="start-h" class="visually-hidden">{START.heading}</h1>
  <img class="mark" src="./icons/favicon-180.png" alt="" width="72" height="72" />
  <div class="choices">
    {#each CHOICES as [c, no, label], i (c)}
      <button class="choice" bind:this={buttons[i]} aria-pressed={s.choice === c} aria-disabled={!s.usable(c)} onclick={() => void s.choose(c)}
        ><span class="no">{no}</span>{label}</button
      >
    {/each}
  </div>

  {#if s.choice && s.online}
    <section class="where" aria-labelledby="where-h">
      <h2 id="where-h">{START.place}</h2>
      {#if s.loading}
        <span class="skeleton crumb-sk"></span>
        <ul class="fs" aria-busy="true">
          {#each [0, 1, 2] as k (k)}<li><span class="skeleton row-sk"></span></li>{/each}
        </ul>
      {:else if s.list}
        {@const list = s.list}
        <div class="crumb">
          <button class="secondary small" aria-disabled={!list.parent} onclick={() => list.parent && void into(list.parent)}>{START.up}</button>
          <span class="path" translate="no">{list.path}</span>
        </div>
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <ul class="fs" bind:this={listEl} onkeydown={listKeys}>
          {#each list.folders as f (f.name)}
            {@const opens = s.choice === 'open' && f.project}
            <li>
              <button class="entry" class:opens onclick={() => void (opens ? s.open(inside(list.path, f.name)) : into(inside(list.path, f.name)))}
                ><span class="kind" aria-hidden="true">{opens ? '·' : '▸'}</span><span translate="no">{f.name}</span>{#if f.project}<span class="tag">{START.project}</span>{/if}</button
              >
            </li>
          {/each}
        </ul>
        {#if making}
          <label class="name">{START.name}<input bind:value={s.name} translate="no" autocomplete="off" spellcheck="false" /></label>
        {/if}
      {/if}
    </section>
  {/if}
</main>

<aside class="right" aria-labelledby="recent-h">
  <h2 id="recent-h">{START.recent}</h2>
  {#if s.recent.length === 0}
    <p class="muted">{START.recentNone}</p>
  {:else}
    <ul class="recent">
      {#each s.recent as r (r.path)}
        <li>
          <button class="entry" aria-disabled={!s.online} onclick={() => s.online && void s.open(r.path)}>
            <strong>{r.name}</strong><span class="path" translate="no">{r.path}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<footer id="band" class="band" tabindex="-1">
  <div class="next">
    {#if !served && !s.usable('new')}
      <p>{CLI_LINK.needCli}</p>
    {:else if fail}
      <span class="cause">{fail.chip}</span>
      <p>{fail.sentence}</p>
    {/if}
  </div>
  {#if making && s.online}
    <button class="primary" aria-disabled={!s.list || s.loading || !s.name.trim()} onclick={() => void s.make()}
      >{s.failed && s.making ? BUTTON.retrySample : s.choice === 'sample' ? START.sample : START.new}<span class="arrow" aria-hidden="true">→</span></button
    >
  {:else if s.choice !== 'open'}
    <button class="primary" aria-disabled={!s.usable('new')} onclick={() => void s.choose('new')}>{START.new}<span class="arrow" aria-hidden="true">→</span></button>
  {/if}
</footer>

<style>
  .start {
    grid-area: canvas;
    display: grid;
    align-content: start;
    gap: var(--s-6);
    min-height: 0;
    padding: var(--s-7) var(--s-6);
    overflow: auto;
    background: var(--c-layer-0);
  }
  .start:focus {
    outline: none;
  }
  .mark {
    border-radius: 16px;
  }
  .choices {
    display: grid;
    gap: var(--s-3);
    max-width: 36rem;
  }
  .choice {
    display: flex;
    align-items: baseline;
    gap: var(--s-5);
    padding: var(--s-5) var(--s-6);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-layer-1);
    text-align: left;
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
    cursor: pointer;
    outline: 3px solid transparent;
    outline-offset: -3px;
    transition: background var(--dur-quick) var(--ease-out);
  }
  .no {
    color: var(--c-muted);
    font: 400 var(--t-version-small-size) / 1 var(--f-version);
  }
  .choice[aria-pressed='true'] {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .choice[aria-pressed='true'] .no {
    color: var(--c-ink);
  }
  .choice:focus-visible {
    border-color: var(--c-paper);
    box-shadow: inset 0 0 0 2px var(--c-paper);
  }
  .choice[aria-pressed='true']:focus-visible {
    box-shadow: inset 0 0 0 2px var(--c-ink);
  }
  .choice[aria-disabled='true'] {
    background: var(--c-layer-2);
    color: var(--c-muted);
    cursor: default;
  }
  @media (hover: hover) and (pointer: fine) {
    .choice:not([aria-pressed='true'], [aria-disabled='true']):hover {
      background: var(--c-layer-2);
    }
  }
  .where {
    display: grid;
    gap: var(--s-3);
    max-width: 36rem;
  }
  h2 {
    margin: 0;
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
  }
  .crumb {
    display: flex;
    align-items: center;
    gap: var(--s-4);
  }
  .small {
    height: var(--h-chip);
    padding: 0 var(--s-3);
    font-size: var(--t-small-size);
  }
  .path {
    color: var(--c-muted);
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
    overflow-wrap: anywhere;
  }
  ul {
    display: grid;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .fs {
    border-top: 1px solid var(--c-line);
  }
  .fs li {
    border-bottom: 1px solid var(--c-line);
  }
  .entry {
    display: flex;
    align-items: baseline;
    gap: var(--s-3);
    width: 100%;
    min-height: var(--h-secondary);
    padding: var(--s-2) var(--s-3);
    border: 0;
    background: none;
    text-align: left;
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
    cursor: pointer;
    outline: 3px solid transparent;
    outline-offset: -3px;
  }
  .entry:focus-visible {
    box-shadow: inset 0 0 0 3px var(--c-paper);
  }
  .entry[aria-disabled='true'] {
    color: var(--c-muted);
    cursor: default;
  }
  @media (hover: hover) and (pointer: fine) {
    .entry:not([aria-disabled='true']):hover {
      background: var(--c-layer-2);
    }
  }
  .kind {
    color: var(--c-muted);
  }
  .tag {
    margin-left: auto;
    color: var(--c-muted);
    font: 700 var(--t-caption-size) / 1 var(--f-ja);
  }
  .entry.opens {
    font-weight: 700;
  }
  .crumb-sk {
    width: 60%;
    height: var(--h-chip);
  }
  .row-sk {
    height: var(--h-secondary);
    margin: 2px 0;
  }
  .name {
    display: grid;
    gap: var(--s-2);
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .name input {
    min-height: var(--h-secondary);
    padding: 0 var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
  }
  .name input:focus-visible {
    outline: none;
    border-color: var(--c-paper);
    box-shadow: inset 0 0 0 2px var(--c-paper);
  }
  .right {
    grid-area: right;
    display: grid;
    align-content: start;
    gap: var(--s-4);
    min-height: 0;
    padding: var(--s-5);
    overflow: auto;
    border-left: 1px solid var(--c-line);
    background: var(--c-ink);
  }
  .recent .entry {
    flex-direction: column;
    gap: 0;
    padding: var(--s-3);
    border-radius: var(--r-m);
    font-family: var(--f-ja);
  }
  .muted {
    margin: 0;
    color: var(--c-muted);
  }
  .band {
    grid-area: band;
    display: flex;
    align-items: center;
    gap: var(--s-4);
    min-height: var(--h-band);
    padding: 0 var(--s-5);
    border-top: 1px solid var(--c-line);
  }
  .next {
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    margin-right: auto;
  }
  .next p {
    margin: 0;
  }
  .cause {
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
  }
  @media (prefers-reduced-motion: reduce) {
    .choice {
      transition: none;
    }
  }
  @media (max-width: 767px) {
    .start {
      padding: var(--s-5) var(--s-4);
    }
    .right {
      border-left: 0;
      border-top: 1px solid var(--c-line);
    }
  }
</style>
