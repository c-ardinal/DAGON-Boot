<script lang="ts">
  import { tick } from 'svelte';
  import type { Agent, AskKind } from '../app/agent.svelte.ts';
  import { AGENT } from '../app/copy.ts';
  import { rise } from '../app/motion.ts';
  import Icon from './Icon.svelte';

  interface Props {
    agent: Agent;
    /** Sends a question (the lock table may stop it: `ask`). */
    onask: (text: string, kind: AskKind, at: Element) => void;
    /** Puts message `i`'s proposal into the 構成 (lock table: `apply`). */
    onapply: (i: number, at: Element) => void;
    /** Opens the drawer's 会話 tab. */
    ontalk: () => void;
    /** Bumped from outside (the menu's "DAGON と話す", the / key): opens the bubble and focuses its field. */
    openRequest: number;
    /** The bubble is open; the page closes it on Escape wherever focus is. */
    open?: boolean;
  }

  let { agent, onask, onapply, ontalk, openRequest, open = $bindable(false) }: Props = $props();

  let field = $state<HTMLInputElement>();
  let text = $state('');
  let answered = $state(false);

  const last = $derived(agent.messages.map((m) => m.role).lastIndexOf('dagon'));
  const reply = $derived(last < 0 ? null : agent.messages[last]);

  $effect(() => {
    if (openRequest === 0) return;
    open = true;
    void tick().then(() => field?.focus());
  });

  /** A reply came: a 1.2 s blue edge, then calm again. */
  const REPLIED_MS = 1200;
  let seen = 0;
  $effect(() => {
    const n = agent.messages.length;
    if (n <= seen) return void (seen = n);
    seen = n;
    if (agent.messages[n - 1].role !== 'dagon') return;
    open = true;
    answered = true;
    const t = setTimeout(() => (answered = false), REPLIED_MS);
    return () => clearTimeout(t);
  });

  /** Escape closes the bubble and gives the focus back to the mark. */
  let markEl = $state<HTMLButtonElement>();
  function keys(e: KeyboardEvent) {
    if (e.key !== 'Escape' || !open) return;
    e.stopPropagation();
    open = false;
    markEl?.focus();
  }

  function send(e: SubmitEvent) {
    e.preventDefault();
    const q = text.trim();
    if (!q) return;
    onask(q, 'free', e.submitter ?? (e.currentTarget as Element));
    text = '';
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="dagon" class:thinking={agent.thinking} class:answered data-avoid="" onkeydown={keys}>
  <button class="mark" bind:this={markEl} aria-label={AGENT.name} aria-expanded={open} title={open ? AGENT.markClose : AGENT.markOpen} onclick={() => (open = !open)}>
    <span class="shadow" aria-hidden="true"></span>
    <img src="./icons/dagon-256.png" srcset="./icons/dagon-256.png 256w, ./icons/dagon-512.png 512w" sizes="96px" alt="" draggable="false" />
  </button>

  {#if open}
    <section transition:rise={{ dur: '--dur-quick' }} class="bubble" aria-label={AGENT.name} aria-live="polite">
      {#if agent.thinking}
        <p class="muted">{AGENT.thinking}</p>
      {:else if reply}
        <p class:failed={reply.failed}>{reply.text}</p>
        {#if reply.proposal || reply.more}
          <div class="acts">
            <button class="secondary small" onclick={ontalk}>{reply.proposal ? AGENT.showDiff : AGENT.allTalk}</button>
            {#if reply.proposal && !reply.applied}
              <button class="primary small" data-control="apply" onclick={(e) => onapply(last, e.currentTarget)}>{AGENT.apply}</button>
            {/if}
          </div>
        {/if}
      {:else}
        <p class="muted">{AGENT.hello}</p>
      {/if}
      <div class="qs">
        {#each AGENT.questions as [kind, q] (kind)}
          <button class="q" data-control="ask" onclick={(e) => onask(q, kind, e.currentTarget)}>{q}</button>
        {/each}
      </div>
      <form onsubmit={send} data-control="ask">
        <input bind:this={field} bind:value={text} aria-label={AGENT.question} placeholder={AGENT.placeholder} maxlength="2000" />
        <button class="button-icon send" type="submit" aria-label={AGENT.send} title={text.trim() ? AGENT.send : AGENT.sendWhy} aria-disabled={!text.trim()}><span class="layer"><Icon name="send-horizontal" /></span></button>
      </form>
    </section>
  {/if}
</div>

<style>
  /* In the right column's flow, above the zoom: a fixed spot that no sheet, toast or table moves. */
  .dagon {
    position: relative;
    flex: none;
    position: relative;
    width: var(--w-mark);
    height: var(--w-mark);
    pointer-events: auto;
  }
  .mark {
    width: var(--w-mark);
    height: var(--w-mark);
  }
  .mark {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    background: none;
    cursor: pointer;
  }
  .mark:focus-visible {
    outline: var(--st-w-focus) solid var(--c-paper);
    outline-offset: 2px;
    border-radius: var(--r-m);
  }
  .mark img {
    position: relative;
    display: block;
    width: 100%;
    height: 100%;
    user-select: none;
  }
  /* A soft shadow on the canvas: darker and smaller as DAGON sinks, lighter and wider as it rises. */
  .shadow {
    position: absolute;
    left: 18%;
    right: 18%;
    bottom: -10%;
    height: 14%;
    border-radius: 50%;
    background: radial-gradient(closest-side, rgb(0 0 0 / 0.75), transparent);
  }
  .thinking .shadow {
    animation: breathe var(--dur-float) var(--ease-in-out) infinite alternate;
  }
  .answered .shadow {
    background: radial-gradient(closest-side, color-mix(in srgb, var(--c-accent) 55%, transparent), transparent);
  }
  .answered img {
    filter: drop-shadow(0 0 0.4rem color-mix(in srgb, var(--c-accent) 70%, transparent));
  }
  .thinking img {
    animation:
      float var(--dur-float) var(--ease-in-out) infinite alternate,
      think var(--dur-think) var(--ease-in-out) infinite alternate;
  }
  @keyframes float {
    to {
      transform: translateY(-4px);
    }
  }
  @keyframes breathe {
    to {
      transform: scaleX(1.18);
      opacity: 0.55;
    }
  }
  @keyframes think {
    to {
      opacity: 0.55;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .thinking img,
    .thinking .shadow {
      animation: none;
    }
  }
  /* The bubble opens to the left of the mark, its foot on the mark's foot (the mark stands at the right edge, low). */
  .bubble {
    position: absolute;
    top: 0;
    right: calc(100% + var(--s-3));
    width: var(--w-bubble);
    display: grid;
    gap: var(--s-2);
    padding: var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-layer-1);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.45);
    transform-origin: right top;
    pointer-events: auto;
  }
  .bubble p {
    margin: 0;
    font: 400 var(--t-caption) / 16px var(--f-ja);
  }
  .muted {
    color: var(--c-muted);
  }
  .failed {
    color: var(--c-signal);
  }
  .acts,
  .qs {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-2);
  }
  .q {
    padding: var(--s-1) var(--s-2);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    background: none;
    color: var(--c-paper);
    font: 400 var(--t-caption) / 16px var(--f-ja);
    cursor: pointer;
    transition: transform var(--dur-press) var(--ease-out);
  }
  .q:active {
    transform: scale(0.97);
  }
  form {
    display: flex;
    gap: var(--s-2);
  }
  input {
    flex: 1;
    min-width: 0;
    min-height: 2rem;
    padding: 0 var(--s-2);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 400 var(--t-caption) / 1 var(--f-ja);
  }
  .small {
    height: 2rem;
    padding: 0 var(--s-3);
    font-size: var(--t-caption);
  }
  @media (max-width: 767px) {
    .q,
    .small,
    input {
      min-height: 2.75rem;
    }
  }
</style>
