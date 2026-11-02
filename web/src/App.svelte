<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { bandOf } from './app/band.ts';
  import { ANNOUNCE, BAND, BUTTON, CANVAS, DEFINE, DIFF, MODE, NODE, ORDER, PRODUCT, SAVE, SKIP, STEP } from './app/copy.ts';
  import { chooseAndOpen } from './app/connect.ts';
  import { hubOf, view, type Action } from './app/machine.ts';
  import { Operate, type Progress } from './app/operate.svelte.ts';
  import Graph, { type Chip, type Dot } from './graph/Graph.svelte';
  import Band from './screens/Band.svelte';
  import Detail from './screens/Detail.svelte';
  import { Define, type Operable } from './app/define/define.svelte.ts';
  import { topoDiff } from './app/define/diff.ts';
  import { fromYaml, type Topo } from './app/define/model.ts';
  import { problemChip } from './app/define/problems.ts';
  import { DESK_DEMO, DESK_DEMO_YAML } from './app/sample.ts';
  import { Start, type StartChoice } from './app/start.svelte.ts';
  import Menu from './screens/Menu.svelte';
  import Restart from './screens/Restart.svelte';
  import Unsaved from './screens/Unsaved.svelte';
  import { Autosave } from './app/define/autosave.svelte.ts';
  import StartScreen from './screens/Start.svelte';
  import Inspector from './screens/define/Inspector.svelte';
  import NodeList from './screens/define/NodeList.svelte';
  import Problems from './screens/define/Problems.svelte';
  import Table from './screens/define/Table.svelte';
  import CliPanel from './screens/define/CliPanel.svelte';
  import { Cli, type Layout } from './app/define/cli.svelte.ts';
  import { CLI_LINK, CLI_STATE } from './app/copy.ts';
  import FilePanel from './screens/FilePanel.svelte';
  import Reconnect from './screens/Reconnect.svelte';
  import Header from './screens/Header.svelte';
  import OrderList from './screens/OrderList.svelte';
  import { devHub } from './serial/devHub.ts';
  import { MockTransport } from './serial/mock.ts';
  import { WebSerialTransport, type Transport } from './serial/transport.ts';
  import Unsupported from './Unsupported.svelte';
  import { dagonReady, require_release_key, set_release_key } from './wasm/dagon.ts';

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
    // The project's files through dagon serve only (stand-ins in development); nothing public.
    latest: async () => (cli.state === 'ready' ? await cli.latestRelease() : null) ?? (await stand?.latest?.()) ?? null,
    bootcore: async (id) => (cli.state === 'ready' ? await cli.bootcore(id) : null) ?? (await stand?.bootcore?.(id)) ?? null,
  });

  /** `dagon serve` (§8): the page is always its page; K0/K3 when it cannot be reached. */
  const cli = new Cli();
  /** The 定義 document: the project's 構成 as edited (§5.2: its YAML is the one source of truth). */
  const d = new Define();
  cli.onDevice = (need) => (need ? op.handOver() : op.cliReturned());

  /** SC-S0 until a project is open (§7.2b); then 定義 and 運用. */
  let screen = $state<'start' | 'work'>('start');
  const start = new Start({
    cli,
    // serve switched to the project's keys (Cli refreshed hello): the page verifies with that key from now on.
    opened: async () => {
      useReleaseKey();
      await openWork();
    },
    // Development stand-ins (?mock, ?sim) have no serve: an unsaved 構成 on the page.
    local: dev ? (c) => startLocal(c) : null,
  });
  /** `#mode=operate` / `#mode=define` (§5.2): the mode a reload or a shared URL comes back to. */
  const wantMode = /(?:^|[#&])mode=(operate|define)\b/.exec(globalThis.location?.hash ?? '')?.[1] as 'operate' | 'define' | undefined;

  /**
   * The release key: dagon serve writes the developer's key into the page (meta) and /v1/hello;
   * a page it serves verifies with that key only, and with none until it is there.
   */
  void cli
    .connect()
    .then(async () => {
      await dagonReady();
      if (cli.state === 'none') return;
      require_release_key();
      useReleaseKey(document.querySelector<HTMLMetaElement>('meta[name="dagon-release-key"]')?.content);
      if (cli.state !== 'ready') return;
      // A project open in serve opens at once; without one the page stays on SC-S0.
      if (cli.has('project')) {
        const p = await cli.project();
        start.recent = (p.recent ?? []).slice(0, 5);
        if (p.open) await openWork(wantMode);
      } else if (await cli.loadTopology()) {
        await openWork(wantMode);
      }
    })
    .catch(() => undefined)
    .finally(() => (start.loading = false));

  /** The key releases are verified with: serve's (hello, or the page's meta at first); never a secret. */
  function useReleaseKey(meta?: string) {
    const key = cli.hello?.release_key ?? meta;
    if (key) set_release_key(key);
  }

  /** The open project's 構成 (its YAML through serve): 運用 when it is updatable as it is, else 定義 (§7.2b). */
  // SC-S0 is reached only through `leave` (menu, 最初からやり直す), so no unsaved edit is here when a project opens.
  async function openWork(want?: 'operate' | 'define') {
    const yaml = await cli.loadTopology().catch(() => null);
    // 呼び名 and positions live beside the YAML (GET /v1/layout).
    projectLayout = (await cli.loadLayout().catch(() => null)) ?? {};
    local = false;
    op.reset();
    d.reset();
    if (yaml) {
      try {
        d.load(fromYaml(yaml, projectLayout.names ?? {}));
      } catch {
        d.unreadable = true;
      }
    }
    layoutNames = JSON.stringify(d.doc.names);
    enterWork(want);
  }

  /** The project's layout file as last read or written; names follow the edits. */
  let projectLayout: Layout = {};
  let layoutNames = '';
  $effect(() => {
    const names = JSON.stringify(d.doc.names);
    if (screen !== 'work' || local) return;
    untrack(() => {
      if (names === layoutNames) return;
      layoutNames = names;
      projectLayout = { ...projectLayout, names: { ...d.doc.names } };
      void cli.saveLayout(projectLayout);
    });
  });

  function enterWork(want?: 'operate' | 'define') {
    start.clear();
    screen = 'work';
    menuOpen = false;
    const m = want !== 'define' && d.operable === 'ok' ? 'operate' : 'define';
    mode = 'define';
    switchMode(m);
    // #mode=operate on a 構成 that cannot be operated: 定義 with the reason (§5.2).
    if (want === 'operate' && m === 'define') lock(d.operable);
  }

  /** Without serve (development stand-ins): an empty 構成 or the sample, not saved. */
  function startLocal(c: 'new' | 'sample') {
    local = true;
    op.reset();
    d.reset();
    if (c === 'sample') d.load(fromYaml(DESK_DEMO_YAML, Object.fromEntries(DESK_DEMO.nodes.map((n) => [n.id, n.name]))));
    enterWork('define');
  }

  /** SC-M: the menu under the header, and its one confirmation (M53). */
  let menuOpen = $state(false);
  let restartOpen = $state(false);
  let restartButton = $state<HTMLButtonElement>();
  /** Menu 新しく作る / 見本から新しく作る / 開く: the 構成 is saved first (or the dialog asks). */
  function toStart(c: StartChoice) {
    menuOpen = false;
    void autosave.leave(() => goStart(c));
  }

  function goStart(c: StartChoice) {
    cliPanel = null;
    screen = 'start';
    void start.refresh();
    void start.choose(c);
  }

  /** "最初からやり直す": the page's records go and SC-S0 opens; the project's files stay (§7.20). */
  function restart() {
    restartOpen = false;
    menuOpen = false;
    void autosave.leave(restartNow);
  }

  function restartNow() {
    local = false;
    op.reset();
    d.reset();
    cli.clearJobs();
    cliPanel = null;
    start.clear();
    mode = 'define';
    screen = 'start';
    void start.refresh();
  }

  function cancelRestart() {
    restartOpen = false;
    void tick().then(() => restartButton?.focus());
  }

  /** 運用 pressed while it cannot be entered: M68 / M69 (or 未保存) in the band until the 構成 changes. */
  let locked = $state<{ why: Operable | 'unsaved'; at: number } | null>(null);
  const lockShown = $derived(locked && locked.at === d.revision ? locked.why : null);
  function lock(why: Operable | 'unsaved') {
    locked = { why, at: d.revision };
  }
  const cliLabel = $derived(cli.state === 'ready' ? CLI_STATE.ready(cli.hello?.cli ?? '') : CLI_STATE[cli.state]);
  let cliPanel = $state<'link' | 'jobs' | 'release' | 'project' | 'flash' | null>(null);
  const runningJobs = $derived(cli.jobs.filter((j) => j.state === 'running' || j.state === 'queued' || j.state === 'question').length);
  $effect(() => {
    if (d.selected) cliPanel = null;
  });

  /** "このリリースを運用で使う" (X03): the release goes to the operate mode's file check. */
  async function useRelease(name: string, bytes: Uint8Array) {
    if (!(await autosave.ready())) return lock('unsaved');
    switchMode('operate');
    op.takeFile(name, bytes);
  }

  /** 運用 / 定義 (§5). The define document goes to the operate canvas when leaving define mode. */
  let mode = $state<'operate' | 'define'>('define');
  const busyStates = ['L19', 'L20', 'L21', 'L22', 'L24'];
  const modeLocked = $derived(busyStates.includes(op.state));

  function switchMode(m: 'operate' | 'define') {
    // 運用 only with a 構成 that has no problems and is updatable (§5.2); the YAML decides, never the 親機.
    if (m === 'operate' && d.operable !== 'ok') return lock(d.operable);
    // ... and only the saved one, the 構成 build, sign and release use too.
    if (m === 'operate' && autosave.pending) {
      void autosave.ready().then((ok) => (ok ? enterMode(m) : lock('unsaved')));
      return;
    }
    enterMode(m);
  }

  function enterMode(m: 'operate' | 'define') {
    // A file checked against another 構成 (L16–L18, e.g. after "ファイルの構成を取り込む") is checked again.
    const recheck = m === 'operate' && op.file !== null && ['L16', 'L17', 'L18'].includes(op.state) && op.config?.yaml !== d.config.yaml;
    if (m === 'operate') op.useConfig(d.config);
    mode = m;
    d.status = ANNOUNCE.A22(m === 'define');
    if (recheck && op.file) op.takeFile(op.file.name, op.file.bytes);
  }

  $effect(() => {
    if (screen === 'work') history.replaceState(history.state, '', `${location.pathname}${location.search}#mode=${mode}`);
  });

  /** "親機へ送る" / "つないで、親機へ送る" from define mode: the operate session sends the document. */
  async function sendDefined() {
    // The 親機 gets the saved 構成 only.
    if (!(await autosave.ready())) return lock('unsaved');
    op.useConfig(d.config);
    if (!connected) await op.connect();
    if (connected && op.config) await op.push();
  }

  /** Development stand-ins (?mock, ?sim without serve): an unsaved 構成 on the page, nothing to save to. */
  let local = $state(false);
  /** Edits go to the project's YAML by themselves (UC-D04); 保存 saves at once. */
  const autosave = new Autosave(d, { put: (yaml) => cli.saveTopology(yaml), enabled: () => screen === 'work' && !local });
  $effect(() => {
    void d.revision;
    autosave.edited();
  });
  $effect(() => {
    const warn = (e: BeforeUnloadEvent) => {
      if (!autosave.pending) return;
      void autosave.now();
      e.preventDefault();
    };
    addEventListener('beforeunload', warn);
    return () => removeEventListener('beforeunload', warn);
  });

  /** "保存": the 構成 into the project's YAML now. */
  async function save() {
    await autosave.now();
  }

  /** "ファイルの構成を取り込む" (§7.6): the release's 構成 replaces this one (undoable) and goes to the YAML. */
  async function importFileTopology() {
    const t = op.fileTopology;
    if (!t) return;
    d.importRelease(t as unknown as Topo);
    op.fileTopology = null;
    await save();
  }
  const fileDiff = $derived(op.state === 'L17' && op.fileTopology ? topoDiff(d.doc.topo, op.fileTopology as unknown as Topo, d.doc.names) : []);

  /** Define mode's drawing scale: up to 32 ノード fit by zooming out; the canvas scrolls to pan. */
  let zoom = $state(1);

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
        .map((p) => [p.select!.slice(5), { chip: `${problemChip(p).mark} ${p.short}`, spoken: p.short, silent: problemChip(p).kind === 'error' }]),
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
    'connectDiscover', 'connect', 'reselect', 'reconnect', 'push', 'pushAgain', 'scanAgain', 'reload',
    'chooseFile', 'updateResponding', 'otherFile', 'update', 'useLatest', 'recover', 'recoverAgain', 'defineFix',
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
  const diffChips = $derived(Object.fromEntries(fileDiff.flatMap((r) => r.nodes).map((id) => [id, { chip: DIFF.chip, spoken: DIFF.spoken, silent: true }])));
  const chips = $derived(updating ? Object.fromEntries(Object.entries(op.progress).map(([id, p]) => [id, chipOf(p)])) : diffChips);
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
    if (op.latestMissing) return { ...b, ...BAND.M58 };
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
    const s3 = s === null ? null : STEP.of3(s);
    document.title = s3 === null || s3 === 'recover' ? PRODUCT : `${String(s3).padStart(2, '0')} ${STEP.of} ${STEP.names[s3]}｜${PRODUCT}`;
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
      case 'defineFix':
        return switchMode('define');
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
  <NodeList
    {d}
    onconnect={() => void op.connect()}
    panel={cliPanel}
    onpanel={(p) => {
      d.selected = null;
      cliPanel = p as typeof cliPanel;
    }}
    running={runningJobs}
    cliReady={cli.state === 'ready'}
  />
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
      <button class="secondary" aria-disabled={zoom <= 0.5} onclick={() => (zoom = Math.max(0.5, zoom - 0.25))}>{DEFINE.zoomOut}</button>
      <button class="secondary" aria-disabled={zoom >= 1.5} onclick={() => (zoom = Math.min(1.5, zoom + 0.25))}>{DEFINE.zoomIn}</button>
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
        {zoom}
      />
    {:else}
      <Table {d} onaddnode={() => document.querySelector<HTMLElement>('#list-h')?.focus()} />
    {/if}
  </main>
{/snippet}

{#snippet defineRight()}
  <aside class="right">
    {#if cliPanel}
      <CliPanel {cli} {d} panel={cliPanel} onuse={useRelease} />
    {:else if d.selected}
      <Inspector {d} />
    {:else}
      <Problems {d} />
    {/if}
  </aside>
{/snippet}

{#snippet defineBottom()}
  {@const empty = d.doc.topo.nodes.length === 0}
  {@const firstError = d.problems.find((p) => p.severity === 'error' && p.rule !== 'V-23')}
  <footer id="band" class="dband" tabindex="-1">
    <div class="next">
      {#if d.saveFailure}
        <span class="cause">{SAVE[d.saveFailure].chip}</span>
        <p>{SAVE[d.saveFailure].sentence}</p>
      {:else if lockShown === 'unsaved'}
        <span class="cause">{SAVE.unsaved}</span>
        <p>{MODE.lockedUnsaved.sentence}</p>
      {:else if lockShown === 'unsupported'}
        <span class="cause">{MODE.lockedUnsupported.chip}</span>
        <p>{MODE.lockedUnsupported.sentence}</p>
      {:else if lockShown}
        <p>{MODE.lockedProblems.sentence}</p>
      {:else if cli.state !== 'ready'}
        <p>{CLI_LINK.needCli}</p>
      {:else if d.unreadable}
        <span class="cause">{DEFINE.unreadableChip}</span>
        <p>{DEFINE.unreadableSentence}</p>
      {:else if firstError}
        <p>{DEFINE.fixSentence}</p>
      {:else if d.operable === 'unsupported'}
        <p>{DEFINE.unsupportedSentence}</p>
      {/if}
    </div>
    {#if op.fileTopology && op.state === 'L17'}
      <button class="secondary" onclick={() => void importFileTopology()}>{DEFINE.importFileTopology}</button>
    {/if}
    {#if empty}
      <!-- E1: C02 on the canvas says what this does. -->
      <button class="primary" onclick={() => void op.connect()}>{BUTTON.connectDiscover}<span class="arrow" aria-hidden="true">→</span></button>
    {:else}
      {#if d.unsaved && !local}<span class="unsaved">{SAVE.unsaved}</span>{/if}
      <button class="secondary" aria-disabled={cli.state !== 'ready'} onclick={() => void save()}>{DEFINE.save}</button>
      {#if firstError}
        <button class="primary" onclick={() => d.fix(firstError)}>{DEFINE.fix}<span class="arrow" aria-hidden="true">→</span></button>
      {:else if d.operable === 'ok'}
        <!-- Optional: updating sends the 構成 before it updates (owner 11/3). -->
        <button class="secondary" onclick={() => void sendDefined()}>{connected ? DEFINE.send : DEFINE.connectSend}</button>
        <button class="primary" onclick={() => switchMode('operate')}>{DEFINE.toOperate}<span class="arrow" aria-hidden="true">→</span></button>
      {/if}
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
      <FilePanel file={op.file} targets={op.targets.length} diff={fileDiff} />
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
  <div class="frame {layout}" class:open={drawerOpen} class:modal={restartOpen || autosave.asking !== null}>
    <Header
      step={screen === 'start' ? null : step}
      {hub}
      drawer={layout === 'drawer' && screen === 'work'}
      {drawerOpen}
      ondrawer={() => (drawerOpen ? closeDrawer() : openDrawer())}
      bind:drawerButton
      {mode}
      onmode={screen === 'start' ? undefined : switchMode}
      {modeLocked}
      {cliLabel}
      operateLocked={d.operable !== 'ok'}
      onlocked={() => lock(d.operable)}
      {menuOpen}
      onmenu={() => (menuOpen = !menuOpen)}
    />
    {#if menuOpen}
      <Menu onstart={toStart} onrestart={() => (restartOpen = true)} locked={modeLocked} bind:restartButton />
    {/if}
    {#if screen === 'start'}
      <aside class="left"></aside>
      <StartScreen s={start} served={cli.state === 'ready'} />
    {:else if layout === 'narrow'}
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
  {#if restartOpen}
    <Restart onconfirm={restart} oncancel={cancelRestart} />
  {/if}
  {#if autosave.asking}
    <Unsaved
      reason={SAVE[d.saveFailure ?? 'failed'].chip}
      onsave={() => void autosave.saveAgain()}
      ondiscard={() => autosave.discard()}
      onstay={() => autosave.stay()}
    />
  {/if}
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
  .dband .unsaved {
    color: var(--c-muted);
    font: 700 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
    white-space: nowrap;
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
      'menu menu menu'
      'left canvas right'
      'band band band';
    grid-template-rows: auto auto auto minmax(0, 1fr) auto;
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
    grid-template-areas: 'header' 'help' 'menu' 'band' 'canvas' 'right' 'left';
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
  /* Q2: while a dialog is open its own button is the one main action. */
  .frame.modal :global(footer .primary) {
    visibility: hidden;
  }
</style>
