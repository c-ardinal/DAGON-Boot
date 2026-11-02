<script lang="ts">
  import { tick } from 'svelte';
  import { bandOf } from './app/band.ts';
  import { ANNOUNCE, BUTTON, CANVAS, DEFINE, NODE, ORDER, PRODUCT, SKIP, STEP } from './app/copy.ts';
  import { chooseAndOpen } from './app/connect.ts';
  import { hubOf, view, type Action } from './app/machine.ts';
  import { Operate, type Progress } from './app/operate.svelte.ts';
  import Graph, { type Chip, type Dot } from './graph/Graph.svelte';
  import Band from './screens/Band.svelte';
  import Detail from './screens/Detail.svelte';
  import { Define } from './app/define/define.svelte.ts';
  import { fromYaml } from './app/define/model.ts';
  import Inspector from './screens/define/Inspector.svelte';
  import NodeList from './screens/define/NodeList.svelte';
  import Problems from './screens/define/Problems.svelte';
  import Table from './screens/define/Table.svelte';
  import FilePanel from './screens/FilePanel.svelte';
  import Reconnect from './screens/Reconnect.svelte';
  import Header from './screens/Header.svelte';
  import OrderList from './screens/OrderList.svelte';
  import { devHub } from './serial/devHub.ts';
  import { MockTransport } from './serial/mock.ts';
  import { WebSerialTransport, type Transport } from './serial/transport.ts';
  import Unsupported from './Unsupported.svelte';
  import { dagonReady } from './wasm/dagon.ts';

  const params = new URLSearchParams(location.search);
  // The stand-in hubs are for development builds only.
  const stand = import.meta.env.DEV ? devHub(params) : null;
  const dev = stand !== null;
  const serial = navigator.serial;
  // Fetch the .wasm alongside the first paint; connecting awaits it and reports a failure.
  if (serial || dev) dagonReady().catch(() => undefined);

  let opened: Transport | null = null;
  const op = new Operate({
    open: async () => {
      const t = stand?.transport ?? null;
      if (t) await t.open();
      opened = t ?? (await chooseAndOpen(serial!));
      return opened;
    },
    // The stand-in hub comes back by itself; a real one is chosen again in the browser.
    reselect: async () => {
      if (dev && opened) {
        if (opened instanceof MockTransport) opened.replug();
        await opened.open();
        return opened;
      }
      opened = await chooseAndOpen(serial!);
      return opened;
    },
    timer: true,
    clock: stand?.clock,
    fetchAsset: stand?.fetchAsset,
  });

  /** 運用 / 定義 (§5). The define document goes to the operate canvas when leaving define mode. */
  let mode = $state<'operate' | 'define'>('operate');
  const d = new Define();
  const busyStates = ['L19', 'L20', 'L21', 'L22', 'L24'];
  const modeLocked = $derived(busyStates.includes(op.state));

  function switchMode(m: 'operate' | 'define') {
    if (m === 'define' && d.doc.topo.nodes.length === 0 && op.config) {
      d.apply(fromYaml(op.config.yaml, Object.fromEntries(op.config.nodes.map((n) => [n.id, n.name]))));
    }
    if (m === 'operate' && d.doc.topo.nodes.length) op.useConfig(d.config);
    mode = m;
    d.status = ANNOUNCE.A22(m === 'define');
  }

  /** "親機へ送る" / "つないで、親機へ送る" from define mode: the operate session sends the document. */
  async function sendDefined() {
    op.useConfig(d.config);
    if (!connected) await op.connect();
    if (connected && op.config) await op.push();
  }

  function download(name: string, text: string) {
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([text], { type: 'application/yaml' }));
    a.download = name;
    a.click();
    URL.revokeObjectURL(a.href);
  }

  let yamlInput = $state<HTMLInputElement>();
  async function pickedYaml(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const f = input.files?.[0];
    input.value = '';
    if (f) d.importYaml(await f.text());
  }

  /** Ctrl+Z / Ctrl+Shift+Z anywhere but in a field; Delete and L on the canvas (§12.2). */
  function defineKeys(e: KeyboardEvent) {
    if (mode !== 'define') return;
    const el = e.target as HTMLElement;
    const field = /^(INPUT|SELECT|TEXTAREA)$/.test(el.tagName);
    const z = e.key.toLowerCase() === 'z' && (e.ctrlKey || e.metaKey);
    if (z && !field) {
      e.preventDefault();
      return e.shiftKey ? d.redo() : d.undo();
    }
    if (field || !el.closest('#canvas')) return;
    if (e.key === 'Delete') {
      e.preventDefault();
      d.removeSelected();
    } else if (e.key === 'l' || e.key === 'L') {
      d.focusField = 'connect';
      d.focusRequest++;
    }
  }

  const defineChips = $derived(
    Object.fromEntries(
      d.problems
        .filter((p) => p.select?.startsWith('node:'))
        .map((p) => [p.select!.slice(5), { chip: `${p.severity === 'warning' ? '▲' : '!'} ${p.short}`, spoken: p.short, silent: p.severity === 'error' }]),
    ),
  );
  const defineDot = $derived({
    label: DEFINE.dotNodes,
    value: d.doc.topo.nodes.length,
    sub: [DEFINE.dotLinks, d.doc.topo.links?.length ?? 0] as const,
    name: DEFINE.dotNodesName(d.doc.topo.nodes.length, d.doc.topo.links?.length ?? 0),
  });

  serial?.addEventListener('disconnect', (e) => {
    // Chrome's SerialConnectionEvent names the port; the spec's event targets it.
    const port = (e as Event & { port?: SerialPort }).port ?? e.target;
    if (opened instanceof WebSerialTransport && port === opened.port) op.lose();
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
  const RUNS: ReadonlySet<Action> = new Set([
    'connectDiscover', 'connect', 'reselect', 'reconnect', 'placeSample', 'push', 'pushAgain', 'scanAgain', 'reload',
    'chooseFile', 'updateResponding', 'otherFile', 'update', 'useLatest', 'recover', 'recoverAgain',
  ]);

  /** States that show the update's progress (node chips, percent, the dot number). */
  const UPDATING: ReadonlySet<string> = new Set(['L19', 'L20', 'L21', 'L22', 'L23', 'L24', 'L25']);
  /** States whose dot number and update-order rows show percent. */
  const PERCENT: ReadonlySet<string> = new Set(['L19', 'L20', 'L21', 'L22', 'L24']);
  const CHIP: Record<Progress['stage'], string> = {
    waiting: NODE.waiting,
    writing: NODE.writing,
    lastResort: NODE.lastResort,
    cannotFix: NODE.cannotFix,
    rebooting: NODE.rebooting,
    done: NODE.done,
    silent: NODE.silent,
    refused: NODE.refusedOther,
    previous: NODE.previous,
    unwritten: NODE.unwritten,
  };
  const chipOf = ({ stage, reason }: Progress): Chip => {
    const chip = (stage === 'refused' && reason && NODE.refused[reason]) || CHIP[stage];
    return { chip, spoken: chip.slice(chip.indexOf(' ') + 1), silent: ['silent', 'refused', 'previous', 'cannotFix'].includes(stage) };
  };
  const updating = $derived(UPDATING.has(op.state));
  const chips = $derived(updating ? Object.fromEntries(Object.entries(op.progress).map(([id, p]) => [id, chipOf(p)])) : {});
  const percent = $derived(
    PERCENT.has(op.state) ? Object.fromEntries(Object.entries(op.progress).map(([id, p]) => [id, p.percent])) : {},
  );
  const showFile = $derived(op.file !== null && op.selected === null && ['L14', 'L15', 'L16', 'L17', 'L18'].includes(op.state));

  let fileInput = $state<HTMLInputElement>();
  async function picked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const f = input.files?.[0];
    input.value = '';
    const forRecovery = pickForRecovery;
    pickForRecovery = false;
    if (!f) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    if (forRecovery) void op.recover({ name: f.name, bytes });
    else op.takeFile(f.name, bytes);
  }

  /** The file chooser is open for a recovery that found no release. */
  let pickForRecovery = false;
  async function recover(again = false) {
    if ((await (again ? op.retry() : op.recover())) === 'needFile') {
      pickForRecovery = true;
      fileInput?.click();
    }
  }
  /** Node actions in the detail: while watching, with nothing running. */
  const nodeActions = $derived(['L12', 'L13', 'L23'].includes(op.state));

  /** SC-O7 while the hub reboots; Escape leaves the same button in the band. */
  let dialogDismissed = $state(false);
  const dialogOpen = $derived((op.state === 'L20' || op.state === 'L21') && !dialogDismissed);
  $effect(() => {
    if (op.state !== 'L20' && op.state !== 'L21') dialogDismissed = false;
  });
  let nowMs = $state(0);
  $effect(() => {
    if (!dialogOpen) return;
    const tick = () => (nowMs = op.now());
    tick();
    const t = setInterval(tick, 1000);
    return () => clearInterval(t);
  });
  const reconnectLeft = $derived(op.reconnectUntil === null || op.state === 'L21' ? null : Math.max(0, Math.ceil((op.reconnectUntil - nowMs) / 1000)));

  const hub = $derived(hubOf(op.state));
  const connected = $derived(!['idle', 'lost'].includes(hub));
  const step = $derived(view(op.state, op.ctx).step);
  const unwritten = $derived(Object.values(op.progress).filter((p) => p.stage === 'unwritten').length);
  const band = $derived.by(() => {
    const b = bandOf(op.state, op.ctx, {
      stuck: op.recoverTarget?.name,
      responding: op.responding,
      unwritten,
      rest: op.rest && op.state === 'L18',
    });
    if (dialogOpen) return { ...b, primary: null };
    if (op.state === 'L20') return { ...b, primary: { action: 'reconnect' as const, label: BUTTON.reconnect } };
    return b;
  });
  const dot = $derived.by((): Dot | null => {
    const c = op.config;
    if (!c || !connected || op.state === 'L06') return null;
    if (PERCENT.has(op.state)) {
      const stopped = op.state === 'L22';
      return { label: CANVAS.dotProgress, value: op.overall, unit: stopped ? CANVAS.dotStopped : CANVAS.dotPercent, name: CANVAS.dotProgressName(op.overall, stopped) };
    }
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
        return void op.connect();
      case 'reconnect':
        return void (op.state === 'L20' || op.state === 'L21' ? op.reconnectHub() : op.connect());
      case 'chooseFile':
      case 'updateResponding':
      case 'otherFile':
        return fileInput?.click();
      case 'update':
        return void op.update();
      case 'useLatest':
        return void op.useLatest();
      case 'recover':
        return void recover();
      case 'recoverAgain':
        return void recover(true);
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

{#snippet defineLeft()}
  <NodeList {d} onconnect={() => void op.connect()} />
{/snippet}

{#snippet defineCanvas()}
  <main id="canvas" tabindex="-1" aria-describedby="canvas-help" class="define">
    <p id="canvas-help" class="visually-hidden">{CANVAS.describe}</p>
    <div class="toolbar">
      <button class="secondary" aria-disabled={!d.canUndo} onclick={() => d.undo()}>{DEFINE.undo}</button>
      <button class="secondary" aria-disabled={!d.canRedo} onclick={() => d.redo()}>{DEFINE.redo}</button>
      <div class="seg" role="radiogroup" aria-label="{DEFINE.graph}・{DEFINE.table}">
        <button role="radio" aria-checked={d.view === 'graph'} onclick={() => (d.view = 'graph')}>{DEFINE.graph}</button>
        <button role="radio" aria-checked={d.view === 'table'} onclick={() => (d.view = 'table')}>{DEFINE.table}</button>
      </div>
      <button class="secondary" onclick={() => yamlInput?.click()}>{DEFINE.importYaml}</button>
    </div>
    {#if d.view === 'graph'}
      <Graph
        config={d.doc.topo.nodes.length ? d.config : null}
        live={{}}
        connected={false}
        sending={false}
        selected={d.selected}
        onselect={(k) => (d.selected = d.selected === k ? null : k)}
        dot={defineDot}
        chips={defineChips}
      />
    {:else}
      <Table {d} onaddnode={() => document.querySelector<HTMLElement>('#list-h')?.focus()} />
    {/if}
  </main>
{/snippet}

{#snippet defineRight()}
  <aside class="right">
    {#if d.selected}
      <Inspector {d} />
    {:else}
      <Problems {d} />
    {/if}
  </aside>
{/snippet}

{#snippet defineBottom()}
  {@const empty = d.doc.topo.nodes.length === 0}
  {@const firstError = d.problems.find((p) => p.severity === 'error')}
  <footer id="band" class="dband" tabindex="-1">
    <div class="next">
      {#if d.unreadable}
        <span class="cause">{DEFINE.unreadableChip}</span>
        <p>{DEFINE.unreadableSentence}</p>
      {:else if empty}
        <p>{DEFINE.emptySentence}</p>
      {:else if firstError}
        <p>{DEFINE.fixSentence}</p>
      {:else if !op.sent}
        <p>{DEFINE.sendSentence}</p>
      {/if}
    </div>
    {#if !empty}
      <button class="secondary" onclick={() => { const f = d.exportYaml(); download(f.name, f.text); }}>{DEFINE.exportYaml}</button>
    {/if}
    {#if empty}
      <button class="primary" onclick={() => void op.connect()}>{DEFINE.connectDiscover}<span class="arrow" aria-hidden="true">→</span></button>
    {:else if firstError}
      <button class="primary" onclick={() => d.fix(firstError)}>{DEFINE.fix}<span class="arrow" aria-hidden="true">→</span></button>
    {:else if !op.sent}
      <button class="primary" onclick={() => void sendDefined()}>{connected ? DEFINE.send : DEFINE.connectSend}<span class="arrow" aria-hidden="true">→</span></button>
    {/if}
    <div class="visually-hidden" role="status">{d.status || op.status}</div>
    <div class="visually-hidden" role="alert">{op.alert}</div>
  </footer>
{/snippet}

{#snippet left()}
  {#if mode === 'define'}{@render defineLeft()}{:else}{@render operateLeft()}{/if}
{/snippet}

{#snippet operateLeft()}
  <OrderList config={op.config} selected={op.selected} onselect={(k) => op.select(k)} fold={layout === 'narrow'} {percent} />
{/snippet}

{#snippet canvas()}
  {#if mode === 'define'}{@render defineCanvas()}{:else}{@render operateCanvas()}{/if}
{/snippet}

{#snippet operateCanvas()}
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
      {chips}
    />
  </main>
{/snippet}

{#snippet right()}
  {#if mode === 'define'}{@render defineRight()}{:else}{@render operateRight()}{/if}
{/snippet}

{#snippet operateRight()}
  <aside class="right">
    {#if showFile && op.file}
      <FilePanel file={op.file} targets={op.targets.length} />
    {:else}
      <Detail
        config={op.config}
        live={op.live}
        selected={op.selected}
        {connected}
        act={nodeActions}
        onkeep={(id) => void op.keepTrial(id)}
        onback={(id) => void op.rollback(id)}
        recovery={op.recovering && op.recovered ? { id: op.recovering, result: op.recovered } : null}
      />
    {/if}
  </aside>
{/snippet}

{#snippet bottom()}
  {#if mode === 'define'}{@render defineBottom()}{:else}{@render operateBottom()}{/if}
{/snippet}

{#snippet operateBottom()}
  <Band {band} busy={op.state === 'L03'} available={(a) => RUNS.has(a)} status={op.status} alert={op.alert} onaction={act} bind:primaryEl bind:nextEl />
{/snippet}

{#if !serial && !dev}
  <Unsupported />
{:else}
  <a class="skip" href="#canvas">{SKIP.canvas}</a>
  <a class="skip" href="#band">{SKIP.next}</a>
  <!-- The DOM follows the visual order of each layout, so Tab order matches what is seen. -->
  <div class="frame {layout}" class:open={drawerOpen}>
    <Header
      {step}
      {hub}
      drawer={layout === 'drawer'}
      {drawerOpen}
      ondrawer={() => (drawerOpen ? closeDrawer() : openDrawer())}
      bind:drawerButton
      {mode}
      onmode={switchMode}
      {modeLocked}
      sent={op.sent && op.config?.yaml === d.config.yaml}
    />
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
  <input class="visually-hidden" type="file" accept=".yaml,.yml" tabindex="-1" aria-hidden="true" bind:this={yamlInput} onchange={pickedYaml} />
  <input
    class="visually-hidden"
    type="file"
    accept=".dgb"
    tabindex="-1"
    aria-hidden="true"
    bind:this={fileInput}
    onchange={picked}
    oncancel={() => (pickForRecovery = false)}
  />
  {#if dialogOpen}
    <Reconnect left={reconnectLeft} onreconnect={() => void op.reconnectHub()} onclose={() => (dialogDismissed = true)} />
  {/if}
{/if}

<svelte:window onkeydown={defineKeys} />

<style>
  main.define {
    grid-template-rows: auto minmax(0, 1fr);
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s-3);
    padding: var(--s-3) var(--s-5);
    border-bottom: 1px solid var(--c-line);
  }
  .seg {
    display: flex;
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
  }
  .seg button {
    padding: 0 var(--s-4);
    border: 0;
    background: none;
    color: var(--c-paper);
    font: 700 var(--t-small-size) / 2.5rem var(--f-ja);
    cursor: pointer;
  }
  .seg button[aria-checked='true'] {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  .dband {
    grid-area: band;
    display: flex;
    align-items: center;
    gap: var(--s-4);
    min-height: var(--h-band);
    padding: 0 var(--s-5);
    border-top: 1px solid var(--c-line);
  }
  .dband .next {
    display: flex;
    align-items: baseline;
    gap: var(--s-2);
    margin-right: auto;
  }
  .dband .next p {
    margin: 0;
  }
  .dband .cause {
    font: 700 var(--t-heading-size) / var(--t-heading-lh) var(--f-ja);
  }
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
