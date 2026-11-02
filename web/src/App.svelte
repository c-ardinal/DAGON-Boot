<script lang="ts">
  import { tick } from 'svelte';
  import { ANNOUNCE, HUB, HUB_NAME, NODE, PRODUCT, SKIP, STEP } from './app/copy.ts';
  import { ConnectError, chooseAndOpen, hello, wasmReady } from './app/connect.ts';
  import { bandOf, focusOf, hubOf, statusOf, type View } from './app/connectView.ts';
  import { dagonReady, versionText } from './wasm/dagon.ts';
  import { MockTransport } from './serial/mock.ts';
  import { WebSerialTransport, type Transport } from './serial/transport.ts';
  import Unsupported from './Unsupported.svelte';
  import HelloDetails from './HelloDetails.svelte';

  const params = new URLSearchParams(location.search);
  const mock = params.get('mock');
  const showGraph = params.has('graph');
  const serial = navigator.serial;
  // Fetch the .wasm alongside the first paint; connect() awaits it and reports a failure.
  if (serial || mock !== null) dagonReady().catch(() => undefined);

  let view = $state<View>({ kind: 'idle' });
  let alertText = $state('');
  let transport: Transport | null = null;
  /** Bumped per attempt and on disconnect; results of an older attempt are dropped. */
  let attempt = 0;
  let primaryEl = $state<HTMLButtonElement>();
  let nextEl = $state<HTMLDivElement>();

  const band = $derived(bandOf(view));
  const hub = $derived(hubOf(view));
  const status = $derived(statusOf(view));
  const busy = $derived(view.kind === 'choosing');

  /** Moves to `v` after a user action and puts focus where the band now asks for it. */
  async function show(v: View) {
    view = v;
    await tick();
    (focusOf(v) === 'button' ? primaryEl : nextEl)?.focus();
  }

  serial?.addEventListener('disconnect', (e) => {
    if (transport instanceof WebSerialTransport && e.target === transport.port) {
      attempt++;
      transport.close().catch(() => undefined);
      transport = null;
      view = { kind: 'lost' };
      alertText = ANNOUNCE.A21lost;
    }
  });

  async function connect() {
    if (busy) return;
    if (!(await wasmReady())) {
      await show({ kind: 'unavailable' });
      return;
    }
    const my = ++attempt;
    await transport?.close().catch(() => undefined);
    transport = null;
    alertText = '';
    await show({ kind: 'choosing' });
    try {
      transport = mock !== null ? new MockTransport({ silent: mock === 'silent', delayMs: 600 }) : await chooseAndOpen(serial!);
      if (mock !== null) await transport.open();
      if (my !== attempt) return;
      await show({ kind: 'checking' });
      const h = await hello(transport);
      if (my !== attempt) return;
      await show({ kind: 'connected', hello: h });
    } catch (e) {
      if (my !== attempt) return;
      await transport?.close().catch(() => undefined);
      transport = null;
      await show({ kind: 'failed', failure: e instanceof ConnectError ? e.failure : 'no-response' });
    }
  }
</script>

{#if !serial && mock === null}
  <Unsupported />
{:else}
  <a class="skip" href="#canvas">{SKIP.canvas}</a>
  <a class="skip" href="#band">{SKIP.next}</a>
  <div class="frame">
    <header>
      <span class="product" translate="no">{PRODUCT}</span>
      <ol class="steps">
        <li aria-current="step">
          {STEP.label} <span class="step-n">{STEP.connect.n}</span>
          {STEP.connect.name}
        </li>
      </ol>
      <span class="hub" aria-label={HUB_NAME[hub]}>{HUB[hub]}</span>
    </header>

    <main id="canvas" tabindex="-1">
      {#if showGraph}
        {#await import('./graph/Graph.svelte') then { default: Graph }}
          <Graph />
        {/await}
      {:else}
        <section class="layer l0">
          <span class="layer-label">{NODE.layerL0}</span>
          <div class="pc">{NODE.pc}</div>
        </section>
        <section class="layer l1">
          <span class="layer-label">{NODE.layerL1}</span>
          {#if view.kind === 'connected'}
            <article class="node">
              <div class="node-band">
                <h2>{NODE.hub}</h2>
                <span class="version" translate="no">{versionText(view.hello.activeVersion)}</span>
              </div>
              <div class="node-body">
                <span class="chip">{view.hello.trial ? NODE.trying : NODE.responding}</span>
                <HelloDetails hello={view.hello} label={NODE.details} />
              </div>
            </article>
          {:else}
            <div class="slot" aria-hidden={view.kind === 'checking' ? 'true' : undefined}>
              {#if view.kind === 'checking'}<span class="skeleton"></span>{:else}{NODE.hubSlot}{/if}
            </div>
          {/if}
        </section>
      {/if}
    </main>

    <footer id="band" tabindex="-1">
      <div class="next" tabindex="-1" bind:this={nextEl}>
        {#if band}
          {#if band.chip}<span class="cause">{band.chip}</span>{/if}
          <p>{band.sentence}</p>
        {/if}
      </div>
      {#if band}
        <!-- Busy stays focusable (aria-disabled) so focus never drops to the page. -->
        <button class="primary" bind:this={primaryEl} onclick={view.kind === 'unavailable' ? () => location.reload() : connect} aria-disabled={busy}>
          {busy ? `${band.button}…` : band.button}<span aria-hidden="true">→</span>
        </button>
      {/if}
      <div class="visually-hidden" role="status">{status}</div>
      <div class="visually-hidden" role="alert">{alertText}</div>
    </footer>
  </div>
{/if}

<style>
  .skip {
    position: absolute;
    left: var(--s-4);
    top: -100px;
    z-index: var(--z-dialog);
    padding: var(--s-2) var(--s-3);
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .skip:focus {
    top: var(--s-2);
  }
  .frame {
    display: grid;
    grid-template-rows: var(--h-header) minmax(0, 1fr) var(--h-band);
    height: 100vh;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--s-5);
    padding: 0 var(--s-5);
    border-bottom: 1px solid var(--c-line);
    z-index: var(--z-chrome);
  }
  .product {
    font: 500 var(--t-mono-size) / 1 var(--f-mono);
    letter-spacing: 0.12em;
  }
  .steps {
    margin: 0 auto 0 0;
    padding: 0;
    list-style: none;
    font: 400 var(--t-small-size) / 1 var(--f-ja);
  }
  .step-n {
    font-family: var(--f-version);
    font-variant-numeric: tabular-nums;
  }
  .hub {
    font: 400 var(--t-small-size) / 1 var(--f-ja);
  }
  main {
    display: grid;
    grid-template-rows: auto 1fr;
    background: var(--c-layer-0);
    overflow: auto;
  }
  .layer {
    position: relative;
    display: grid;
    justify-items: center;
    align-content: start;
    gap: var(--s-4);
    padding: var(--s-6);
  }
  .l1 {
    background: var(--c-layer-1);
  }
  .layer-label {
    justify-self: start;
    font: 500 var(--t-caption-size) / var(--t-caption-lh) var(--f-ja);
    letter-spacing: 0.04em;
    color: var(--c-muted);
  }
  .pc {
    width: var(--w-node);
    padding: var(--s-3) 0;
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    text-align: center;
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .slot {
    width: var(--w-node);
    min-height: 10rem;
    display: grid;
    place-items: center;
    border: 2px dashed var(--c-line);
    border-radius: var(--r-l);
    color: var(--c-muted);
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .skeleton {
    width: 50%;
    height: var(--s-4);
    border-radius: var(--r-s);
    background: var(--c-layer-2);
  }
  .node {
    width: var(--w-node);
    border: 1px solid var(--c-line);
    border-radius: var(--r-m);
    background: var(--c-ink);
    overflow: hidden;
  }
  .node-band {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: var(--s-2) var(--s-3);
    border-bottom: 1px solid var(--c-line);
  }
  .node-band h2 {
    margin: 0;
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .version {
    font: 400 var(--t-version-small-size) / var(--t-version-small-lh) var(--f-version);
    letter-spacing: 0.02em;
    font-variant-numeric: tabular-nums;
  }
  .node-body {
    display: grid;
    gap: var(--s-2);
    justify-items: start;
    padding: var(--s-3) var(--s-4);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    height: var(--h-chip);
    padding: 0 var(--s-2);
    border: 1px solid var(--c-line);
    border-radius: var(--r-full);
    font: 700 var(--t-caption-size) / 1 var(--f-ja);
    letter-spacing: 0.04em;
  }
  footer {
    display: flex;
    align-items: center;
    gap: var(--s-6);
    padding: 0 var(--s-5);
    border-top: 1px solid var(--c-line);
    z-index: var(--z-chrome);
  }
  .next {
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    margin-right: auto;
  }
  .next p {
    margin: 0;
    font: 400 var(--t-body-size) / var(--t-body-lh) var(--f-ja);
    text-wrap: pretty;
  }
  .cause {
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
    letter-spacing: 0.02em;
    white-space: nowrap;
  }
  .primary {
    display: inline-flex;
    align-items: center;
    gap: var(--s-2);
    height: var(--h-primary);
    padding: 0 var(--s-6);
    border: 0;
    border-radius: var(--r-full);
    background: var(--c-signal);
    color: var(--c-ink);
    font: 700 var(--t-body-size) / 1 var(--f-ja);
    white-space: nowrap;
    cursor: pointer;
    outline: 3px solid transparent;
    outline-offset: 0;
    transition: transform var(--dur-press) var(--ease-out);
  }
  .primary:focus-visible {
    outline-color: var(--c-paper);
  }
  .primary:active {
    transform: scale(0.97);
  }
  .next:focus {
    outline: none;
  }
  .primary[aria-disabled='true'] {
    background: var(--c-layer-2);
    color: var(--c-muted);
    cursor: default;
  }
  .primary span {
    transition: transform var(--dur-press) var(--ease-out);
  }
  @media (hover: hover) and (pointer: fine) {
    .primary:hover span {
      transform: translateX(4px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .primary,
    .primary span {
      transition: none;
    }
    .primary:active {
      transform: none;
    }
  }
</style>
