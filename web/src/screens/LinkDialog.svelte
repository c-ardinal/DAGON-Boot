<script lang="ts">
  import type { AgentChoice, AgentStatus } from '../app/agent.svelte.ts';
  import { AGENT } from '../app/copy.ts';
  import { untrack } from 'svelte';

  interface Props {
    status: AgentStatus;
    /** 『同意して連携』: records the consent for the chosen agent and way through serve. */
    onagree: (choice: AgentChoice) => void;
    /** 『やめる』 or Escape: nothing is recorded. */
    oncancel: () => void;
  }

  let { status, onagree, oncancel }: Props = $props();
  let dialog: HTMLDialogElement;
  let cancel: HTMLButtonElement;

  $effect(() => {
    dialog.showModal();
    // Sending leaves the page: the safe choice takes focus.
    cancel.focus();
  });
  const L = AGENT.link;
  // The linked choice when there is one, else serve's default (Claude, Claude Code).
  let agentId = $state(untrack(() => status.consent?.agent ?? status.default.agent));
  const agentDef = $derived(status.agents.find((a) => a.id === agentId) ?? status.agents[0]);
  let wayId = $state(untrack(() => status.consent?.way ?? status.default.way));
  const way = $derived(agentDef?.ways.find((w) => w.id === wayId) ?? agentDef?.ways[0]);
  $effect(() => {
    if (agentDef && !agentDef.ways.some((w) => w.id === wayId)) wayId = agentDef.ways[0]?.id ?? '';
  });
</script>

<!-- DES-UX3-188: the one consent, when linking. The provider's name appears here and nowhere else in the page. -->
<dialog bind:this={dialog} aria-labelledby="link-h" aria-describedby="link-s" onclose={oncancel}>
  <img class="mark" src="./icons/dagon-256.png" alt="" width="64" height="64" />
  <h2 id="link-h">{L.heading}</h2>
  <p id="link-s">{L.intro}</p>
  <dl>
    <dt><label for="link-agent">{L.agent}</label></dt>
    <dd>
      {#if status.agents.length > 1}
        <select id="link-agent" bind:value={agentId}>{#each status.agents as a (a.id)}<option value={a.id}>{a.name}（{a.provider}）</option>{/each}</select>
      {:else}
        <strong id="link-agent">{agentDef?.name}（{agentDef?.provider}）</strong>
      {/if}
      <span class="note">{L.agentNote}</span>
    </dd>
    <dt>{L.way}</dt>
    <dd>
      <div class="ways" role="radiogroup" aria-label={L.way}>
        {#each agentDef?.ways ?? [] as w (w.id)}
          <label class="way"><input type="radio" name="link-way" value={w.id} bind:group={wayId} />{w.name}</label>
        {/each}
      </div>
    </dd>
    <dt>{L.ready}</dt>
    <dd><span>{L.wayNote[way?.kind ?? ''] ?? ''}</span>{#if way && !way.available}<span class="note">{L.wayMissing}</span>{/if}</dd>
    <dt>{L.sends}</dt>
    <dd><ul>{#each status.sends as s (s)}<li>{s}</li>{/each}</ul></dd>
    <dt>{L.neverSends}</dt>
    <dd><ul>{#each status.never_sends as s (s)}<li>{s}</li>{/each}</ul></dd>
  </dl>
  <p class="undo">{L.undo}</p>
  <div class="actions">
    <button class="secondary" bind:this={cancel} onclick={oncancel}>{L.cancel}</button>
    <button class="primary" disabled={!agentDef || !way} onclick={() => agentDef && way && onagree({ agent: agentDef.id, way: way.id })}>{L.agree}</button>
  </div>
</dialog>

<style>
  dialog {
    width: min(36rem, calc(100vw - 2 * var(--s-4)));
    max-height: calc(100vh - 2 * var(--s-4));
    overflow: auto;
    padding: var(--s-6);
    border: 1px solid var(--c-line);
    border-radius: var(--r-l);
    background: var(--c-layer-1);
    color: var(--c-paper);
  }
  dialog::backdrop {
    background: var(--c-scrim);
  }
  .mark {
    float: right;
    width: 4rem;
    height: 4rem;
    margin: 0 0 var(--s-3) var(--s-4);
  }
  h2 {
    margin: 0 0 var(--s-4);
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
    letter-spacing: 0.02em;
  }
  p {
    margin: 0 0 var(--s-4);
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--s-3) var(--s-4);
    margin: 0 0 var(--s-4);
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  dt {
    color: var(--c-muted);
  }
  dd {
    display: grid;
    gap: var(--s-1);
    margin: 0;
  }
  .note {
    color: var(--c-muted);
  }
  .ways {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-1) var(--s-4);
  }
  .way {
    display: inline-flex;
    align-items: center;
    gap: var(--s-1);
    cursor: pointer;
  }
  select {
    height: 1.5rem;
    border: 1px solid var(--c-hair);
    border-radius: var(--r-s);
    background: var(--c-layer-1);
    color: var(--c-paper);
  }
  ul {
    margin: 0;
    padding-left: 1.2em;
  }
  .undo {
    color: var(--c-muted);
    font-size: var(--t-small-size);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--s-4);
  }
  @media (max-width: 767px) {
    dialog {
      padding: var(--s-5);
    }
    dl {
      grid-template-columns: 1fr;
      gap: var(--s-1);
    }
    dd {
      margin-bottom: var(--s-2);
    }
    .actions button {
      min-height: 2.75rem;
    }
  }
</style>
