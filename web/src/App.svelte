<script lang="ts">
  import { tick } from 'svelte';
  import { bandOf } from './app/band.ts';
  import { CANVAS, PRODUCT, SKIP, STEP } from './app/copy.ts';
  import { chooseAndOpen } from './app/connect.ts';
  import { hubOf, view, type Action } from './app/machine.ts';
  import { Operate } from './app/operate.svelte.ts';
  import Graph, { type Dot } from './graph/Graph.svelte';
  import Band from './screens/Band.svelte';
  import Detail from './screens/Detail.svelte';
  import Header from './screens/Header.svelte';
  import OrderList from './screens/OrderList.svelte';
  import { devTransport } from './serial/devHub.ts';
  import { WebSerialTransport, type Transport } from './serial/transport.ts';
  import Unsupported from './Unsupported.svelte';
  import { dagonReady } from './wasm/dagon.ts';

  const params = new URLSearchParams(location.search);
  const dev = params.has('mock');
  const serial = navigator.serial;
  // Fetch the .wasm alongside the first paint; connecting awaits it and reports a failure.
  if (serial || dev) dagonReady().catch(() => undefined);

  let opened: Transport | null = null;
  const op = new Operate({
    open: async () => {
      const t = devTransport(params);
      if (t) await t.open();
      opened = t ?? (await chooseAndOpen(serial!));
      return opened;
    },
    timer: true,
  });

  serial?.addEventListener('disconnect', (e) => {
    if (opened instanceof WebSerialTransport && e.target === opened.port) op.lose();
  });

  let primaryEl = $state<HTMLButtonElement>();
  let nextEl = $state<HTMLDivElement>();
  let narrow = $state(false);
  $effect(() => {
    const mq = matchMedia('(max-width: 767px)');
    narrow = mq.matches;
    const on = () => (narrow = mq.matches);
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  });

  const hub = $derived(hubOf(op.state));
  const connected = $derived(!['idle', 'lost'].includes(hub));
  const step = $derived(view(op.state, op.ctx).step);
  const band = $derived(bandOf(op.state, op.ctx, { stuck: op.stuck?.name, responding: op.responding }));
  const dot = $derived.by((): Dot | null => {
    const c = op.config;
    if (!c || !connected || op.state === 'L06') return null;
    if (op.state === 'L08' || op.state === 'L09') return { label: CANVAS.dotFound, value: op.found, name: CANVAS.dotFoundName(op.found) };
    const n = c.nodes.length;
    return { label: CANVAS.dotResponding, value: op.responding, of: n, name: CANVAS.dotRespondingName(op.responding, n) };
  });

  $effect(() => {
    const s = step;
    document.title = s === null || s === 'recover' ? PRODUCT : `${String(s).padStart(2, '0')} ${STEP.of} ${STEP.names[s]}｜${PRODUCT}`;
  });

  // Focus follows the band only after a user action changed it, never on load.
  $effect(() => {
    if (op.focusRequest === 0) return;
    void tick().then(() => (band.primary ? primaryEl : nextEl)?.focus());
  });

  function act(a: Action) {
    switch (a) {
      case 'connectDiscover':
      case 'connect':
      case 'reselect':
      case 'reconnect':
        return void op.connect();
      case 'placeSample':
        return op.placeSample();
      case 'push':
      case 'pushAgain':
        return void op.push();
      case 'scanAgain':
        return void op.discover();
      case 'reload':
        return location.reload();
      default:
        // Update, recovery and file checks come with their procedures.
        return;
    }
  }
</script>

{#if !serial && !dev}
  <Unsupported />
{:else}
  <a class="skip" href="#canvas">{SKIP.canvas}</a>
  <a class="skip" href="#band">{SKIP.next}</a>
  <div class="frame">
    <Header {step} {hub} />
    <aside class="left">
      <OrderList config={op.config} selected={op.selected} onselect={(k) => op.select(k)} fold={narrow} />
    </aside>
    <main id="canvas" tabindex="-1" aria-describedby="canvas-help">
      <p id="canvas-help" class="visually-hidden">{CANVAS.describe}</p>
      <Graph
        config={op.config}
        live={op.live}
        {connected}
        sending={op.state === 'L10'}
        selected={op.selected}
        onselect={(k) => op.select(op.selected === k ? null : k)}
        {dot}
      />
    </main>
    <aside class="right">
      <Detail config={op.config} live={op.live} selected={op.selected} {connected} />
    </aside>
    <Band {band} busy={op.state === 'L03'} status={op.status} alert={op.alert} onaction={act} bind:primaryEl bind:nextEl />
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
    grid-template-areas:
      'header header header'
      'help help help'
      'left canvas right'
      'band band band';
    grid-template-rows: auto auto minmax(0, 1fr) auto;
    grid-template-columns: var(--w-left) minmax(0, 1fr) var(--w-right);
    height: 100vh;
  }
  .left,
  .right {
    min-height: 0;
    overflow: auto;
    background: var(--c-ink);
  }
  .left {
    grid-area: left;
    border-right: 1px solid var(--c-line);
  }
  .right {
    grid-area: right;
    border-left: 1px solid var(--c-line);
  }
  main {
    grid-area: canvas;
    display: grid;
    min-height: 0;
    background: var(--c-layer-0);
  }
  main:focus {
    outline: none;
  }
  @media (max-width: 1439px) {
    .frame {
      grid-template-columns: 17.5rem minmax(0, 1fr) 20rem;
    }
  }
  /* One column, top to bottom: next step, canvas, detail, the folded update order. */
  @media (max-width: 767px) {
    .frame {
      grid-template-areas: 'header' 'help' 'band' 'canvas' 'right' 'left';
      grid-template-rows: auto;
      grid-template-columns: minmax(0, 1fr);
      height: auto;
    }
    .left,
    .right {
      overflow: visible;
      border: 0;
      border-top: 1px solid var(--c-line);
    }
  }
</style>
