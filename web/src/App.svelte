<script lang="ts">
  import { tick } from 'svelte';
  import { bandOf } from './app/band.ts';
  import { CANVAS, ORDER, PRODUCT, SKIP, STEP } from './app/copy.ts';
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
  // The stand-in hub is for development builds only.
  const dev = import.meta.env.DEV && params.has('mock');
  const serial = navigator.serial;
  // Fetch the .wasm alongside the first paint; connecting awaits it and reports a failure.
  if (serial || dev) dagonReady().catch(() => undefined);

  let opened: Transport | null = null;
  const op = new Operate({
    open: async () => {
      const t = dev ? devTransport(params) : null;
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
  /** wide ≥ 1280 px: three columns; drawer 768–1279 px: the update order opens from the header; narrow ≤ 767 px: one column. */
  let layout = $state<'wide' | 'drawer' | 'narrow'>('wide');
  $effect(() => {
    const narrowMq = matchMedia('(max-width: 767px)');
    const drawerMq = matchMedia('(max-width: 1279px)');
    const on = () => (layout = narrowMq.matches ? 'narrow' : drawerMq.matches ? 'drawer' : 'wide');
    on();
    narrowMq.addEventListener('change', on);
    drawerMq.addEventListener('change', on);
    return () => {
      narrowMq.removeEventListener('change', on);
      drawerMq.removeEventListener('change', on);
    };
  });

  let drawerOpen = $state(false);
  let drawerEl = $state<HTMLElement>();
  let drawerButton = $state<HTMLButtonElement>();
  $effect(() => {
    if (layout !== 'drawer') drawerOpen = false;
  });

  async function openDrawer() {
    drawerOpen = true;
    await tick();
    (focusables()[0] ?? drawerEl)?.focus();
  }

  function closeDrawer() {
    drawerOpen = false;
    drawerButton?.focus();
  }

  function focusables(): HTMLElement[] {
    return [...(drawerEl?.querySelectorAll<HTMLElement>('button, [href], summary, [tabindex]:not([tabindex="-1"])') ?? [])];
  }

  /** Keeps Tab inside the open drawer; Escape closes it and returns focus to its header button. */
  function drawerKeys(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      return closeDrawer();
    }
    if (e.key !== 'Tab') return;
    const f = focusables();
    if (f.length === 0) return e.preventDefault();
    const first = f[0];
    const last = f[f.length - 1];
    if (e.shiftKey && (document.activeElement === first || document.activeElement === drawerEl)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  /** Buttons whose procedures this page runs today; the others show as unavailable. */
  const RUNS: ReadonlySet<Action> = new Set(['connectDiscover', 'connect', 'reselect', 'reconnect', 'placeSample', 'push', 'pushAgain', 'scanAgain', 'reload']);

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
        return;
    }
  }
</script>

{#snippet left()}
  <OrderList config={op.config} selected={op.selected} onselect={(k) => op.select(k)} fold={layout === 'narrow'} />
{/snippet}

{#snippet canvas()}
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
{/snippet}

{#snippet right()}
  <aside class="right">
    <Detail config={op.config} live={op.live} selected={op.selected} {connected} />
  </aside>
{/snippet}

{#snippet bottom()}
  <Band {band} busy={op.state === 'L03'} available={(a) => RUNS.has(a)} status={op.status} alert={op.alert} onaction={act} bind:primaryEl bind:nextEl />
{/snippet}

{#if !serial && !dev}
  <Unsupported />
{:else}
  <a class="skip" href="#canvas">{SKIP.canvas}</a>
  <a class="skip" href="#band">{SKIP.next}</a>
  <!-- The DOM follows the visual order of each layout, so Tab order matches what is seen. -->
  <div class="frame {layout}" class:open={drawerOpen}>
    <Header {step} {hub} drawer={layout === 'drawer'} {drawerOpen} ondrawer={() => (drawerOpen ? closeDrawer() : openDrawer())} bind:drawerButton />
    {#if layout === 'narrow'}
      {@render bottom()}
      {@render canvas()}
      {@render right()}
      <aside class="left">{@render left()}</aside>
    {:else}
      {#if layout === 'wide'}
        <aside class="left">{@render left()}</aside>
      {:else if drawerOpen}
        <div class="left" id="drawer" role="dialog" aria-modal="true" aria-label={ORDER.heading} tabindex="-1" bind:this={drawerEl} onkeydown={drawerKeys}>
          {@render left()}
        </div>
      {/if}
      {@render canvas()}
      {@render right()}
      {@render bottom()}
    {/if}
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
  /* 768–1279 px: the update order is a 300 px drawer that pushes the canvas aside. */
  .frame.drawer {
    grid-template-columns: 0 minmax(0, 1fr) 20rem;
  }
  .frame.drawer.open {
    grid-template-columns: var(--w-left) minmax(0, 1fr) 20rem;
  }
  /* One column, top to bottom: next step, canvas, detail, the folded update order. */
  .frame.narrow {
    grid-template-areas: 'header' 'help' 'band' 'canvas' 'right' 'left';
    grid-template-rows: auto;
    grid-template-columns: minmax(0, 1fr);
    height: auto;
  }
  .narrow .left,
  .narrow .right {
    overflow: visible;
    border: 0;
    border-top: 1px solid var(--c-line);
  }
  #drawer:focus {
    outline: none;
  }
</style>
