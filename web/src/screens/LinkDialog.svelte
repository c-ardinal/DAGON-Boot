<script lang="ts">
  import type { AgentStatus } from '../app/agent.svelte.ts';
  import { AGENT } from '../app/copy.ts';

  interface Props {
    status: AgentStatus;
    /** 『同意して連携』: records the consent through serve. */
    onagree: () => void;
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
</script>

<!-- DES-UX3-188: the one consent, when linking. The provider's name appears here and nowhere else in the page. -->
<dialog bind:this={dialog} aria-labelledby="link-h" aria-describedby="link-s" onclose={oncancel}>
  <img class="mark" src="./icons/dagon-256.png" alt="" width="64" height="64" />
  <h2 id="link-h">{L.heading}</h2>
  <p id="link-s">{L.intro}</p>
  <dl>
    <dt>{L.provider}</dt>
    <dd><strong>{status.provider.name}</strong><span class="note">{L.providerNote}</span></dd>
    <dt>{L.key}</dt>
    <dd><span>{L.keyNote}</span>{#if !status.available}<span class="note">{L.keyMissing}</span>{/if}</dd>
    <dt>{L.sends}</dt>
    <dd><ul>{#each status.sends as s (s)}<li>{s}</li>{/each}</ul></dd>
    <dt>{L.neverSends}</dt>
    <dd><ul>{#each status.never_sends as s (s)}<li>{s}</li>{/each}</ul></dd>
  </dl>
  <p class="undo">{L.undo}</p>
  <div class="actions">
    <button class="secondary" bind:this={cancel} onclick={oncancel}>{L.cancel}</button>
    <button class="primary" onclick={onagree}>{L.agree}</button>
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
