<script lang="ts">
  import { UNSUPPORTED } from './app/copy.ts';

  const url = location.href;
  let urlEl: HTMLElement;

  async function copyUrl() {
    try {
      await navigator.clipboard.writeText(url);
    } catch {
      const range = document.createRange();
      range.selectNodeContents(urlEl);
      const sel = getSelection();
      sel?.removeAllRanges();
      sel?.addRange(range);
    }
  }
</script>

<main class="unsupported">
  <h1>{UNSUPPORTED.heading}</h1>
  <dl>
    <dt>{UNSUPPORTED.supportLabel}</dt>
    <dd>{UNSUPPORTED.supportValue}</dd>
  </dl>
  <p>{UNSUPPORTED.sentence}</p>
  <code bind:this={urlEl} translate="no">{url}</code>
  <button onclick={copyUrl}>{UNSUPPORTED.button}</button>
</main>

<style>
  .unsupported {
    display: grid;
    gap: var(--s-4);
    justify-items: start;
    max-width: 40rem;
    margin: var(--s-9) auto;
    padding: 0 var(--s-5);
  }
  h1 {
    margin: 0;
    font: 700 var(--t-heading) / 28px var(--f-ja);
    letter-spacing: 0.02em;
    font-feature-settings: 'palt';
  }
  dl {
    display: flex;
    gap: var(--s-4);
    margin: 0;
  }
  dt {
    color: var(--c-muted);
  }
  dd {
    margin: 0;
  }
  p {
    margin: 0;
  }
  code {
    font: 500 var(--t-mono) / 20px var(--f-mono);
    overflow-wrap: anywhere;
  }
  button {
    height: var(--h-secondary);
    padding: 0 var(--s-5);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    color: var(--c-paper);
    font: 700 var(--t-small) / 1 var(--f-ja);
    cursor: pointer;
  }
  button:focus-visible {
    border: 3px solid var(--c-paper);
    padding: 0 var(--s-5);
    outline: 3px solid transparent;
    outline-offset: -3px;
  }
</style>
