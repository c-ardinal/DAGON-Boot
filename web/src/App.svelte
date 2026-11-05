<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { bandOf } from './app/band.ts';
  import { AGENT, ANNOUNCE, BAND, BUILD, BUTTON, CANVAS, DEFINE, DIFF, DRAWER, JOBS, SKELETON, WRITE, HUB, LOCK, SHEET, STATUS, ZOOM, HUB_TOGGLE, MODE, NODE, ORDER, PRODUCT, RECONNECT, SAVE, SKIP, STEP, TIME } from './app/copy.ts';
  import { webSerial } from './app/connect.ts';
  import { HUB_LOCKED, hubOf, view, type Action } from './app/machine.ts';
  import { Operate, waterOf, type Progress, type ReleaseFile } from './app/operate.svelte.ts';
  import { keptChoices, type Kept } from './app/kept.ts';
  import Graph, { type Chip, type Dot } from './graph/Graph.svelte';
  import Status from './screens/Status.svelte';
  import Drawer, { type DrawerTab } from './screens/Drawer.svelte';
  import Timeline, { type StageEvent } from './screens/Timeline.svelte';
  import { PARTS } from './app/copy.ts';
  import { activeStates, lockOf, stateName, type Control } from './app/locks.ts';
  import { Agent, consentProblem, type AgentChoice, type AskKind, type Selection } from './app/agent.svelte.ts';
  import { agentMock } from './app/agentMock.ts';
  import Dagon from './screens/Dagon.svelte';
  import LinkDialog from './screens/LinkDialog.svelte';
  import Detail from './screens/Detail.svelte';
  import { Define, type Operable } from './app/define/define.svelte.ts';
  import { topoDiff } from './app/define/diff.ts';
  import { fromYaml, type Topo } from './app/define/model.ts';
  import { problemChip } from './app/define/problems.ts';
  import { SAMPLE_CONFIG, SAMPLE_YAML } from './app/sample.ts';
  import { Start, type StartChoice } from './app/start.svelte.ts';
  import Menu from './screens/Menu.svelte';
  import Restart from './screens/Restart.svelte';
  import Unsaved from './screens/Unsaved.svelte';
  import { Autosave } from './app/define/autosave.svelte.ts';
  import StartScreen from './screens/Start.svelte';
  import Inspector from './screens/define/Inspector.svelte';
  import NodeList, { type Part } from './screens/define/NodeList.svelte';
  import Problems from './screens/define/Problems.svelte';
  import Table from './screens/define/Table.svelte';
  import CliPanel from './screens/define/CliPanel.svelte';
  import { Cli, jobReason, type Layout } from './app/define/cli.svelte.ts';
  import { CLI_LINK, CLI_STATE } from './app/copy.ts';
  import Reconnect from './screens/Reconnect.svelte';
  import Header from './screens/Header.svelte';
  import ZoomPill from './screens/ZoomPill.svelte';
  import SkeletonModal from './screens/SkeletonModal.svelte';
  import Modal from './screens/Modal.svelte';
  import FactoryModal from './screens/FactoryModal.svelte';
  import { answerConsent } from './app/factory.ts';
  import Toast, { type Notice } from './screens/Toast.svelte';
  import { CONN, FILE, NOTICE, SIM, fill as fillIn } from './app/copy.ts';
  import OrderList from './screens/OrderList.svelte';
  import { devHub } from './serial/devHub.ts';
  import { DEMO_SITE } from './app/site.ts';
  import { serveSim, type SimStand } from './serial/serveSim.ts';
  import { MockTransport } from './serial/mock.ts';
  import { WebSerialTransport, type Transport } from './serial/transport.ts';
  import Unsupported from './Unsupported.svelte';
  import { dagonReady, mcuConnection, releaseVersions, require_release_key, set_release_key, trialMs, useDefinitions, versionText } from './wasm/dagon.ts';

  const params = new URLSearchParams(location.search);
  // The stand-in hubs are for development builds only; the public demo page always drives the simulator.
  const stand = import.meta.env.DEV ? devHub(params) : DEMO_SITE ? devHub(new URLSearchParams('sim')) : null;
  const dev = stand !== null;
  const serial = navigator.serial;
  // Fetch the .wasm alongside the first paint; connecting awaits it and reports a failure.
  if (serial || dev) dagonReady().catch(() => undefined);

  let opened: Transport | null = null;
  /** `dagon demo`'s simulator through serve, made once serve says it has one; null for a real hub. */
  let simStand: SimStand | null = null;
  function simHub(): SimStand | null {
    const s = cli.sim;
    if (!s) return null;
    simStand ??= serveSim(cli, s.speed);
    return simStand;
  }
  const op = new Operate({
    open: async () => {
      const t = stand?.transport ?? simHub()?.transport ?? null;
      if (t) await t.open();
      opened = t ?? (await ports.chooseAndOpen());
      return opened;
    },
    // The stand-in hub comes back by itself; a real one is chosen again in the browser.
    reselect: async () => {
      if ((dev || simHub()) && opened) {
        if (opened instanceof MockTransport) opened.replug();
        await opened.open();
        return opened;
      }
      // `opened` follows only once Operate took it as this hub (reconnectChosen).
      return ports.chooseAndOpen();
    },
    timer: true,
    // The simulator's clock runs at the demo's speed; a real hub keeps the default.
    get clock() {
      return stand?.clock ?? simHub()?.clock;
    },
    // The CLI gave the hub back: the device the page had, opened again without the chooser.
    again: async () => {
      if (!opened) throw new Error('no device');
      await opened.open();
      return opened;
    },
    // The project's files through dagon serve only (stand-ins in development); nothing public.
    latest: async () => (cli.state === 'ready' ? await cli.latestRelease() : null) ?? (await stand?.latest?.()) ?? null,
    bootcore: async (id) => (cli.state === 'ready' ? await cli.bootcore(id) : null) ?? (await stand?.bootcore?.(id)) ?? null,
    stage0: async (id) => (cli.state === 'ready' ? await cli.stage0(id) : null),
    // Every package written through DAGON is kept in the project (serve); without serve there is nowhere to keep it.
    keep: async (bytes) => (cli.state === 'ready' ? (await cli.keep(bytes)) !== null : true),
    // A node's % counts its trial against its MCU definition's [watchdog] trial_ms.
    trialMs: (id) => trialMs(d.doc.topo.nodes.find((n) => n.id === id)?.mcu ?? ''),
  });

  /** `dagon serve`: the page is always its page; K0/K3 when it cannot be reached. */
  const cli = new Cli();
  /** The 親機 and 子機 are simulated: `dagon demo` through serve, or the demo page's own simulator. */
  const simulated = $derived(cli.sim !== null || DEMO_SITE);
  /** The 定義 document: the project's 構成 as edited (its YAML is the one source of truth). */
  const d = new Define();
  /** DAGON's link to its agent through serve (development: `?agent` stands in for serve's routes). */
  const agentStandIn = dev && !DEMO_SITE && params.has('agent');
  const agent = new Agent(agentStandIn ? agentMock(params, () => d.config.yaml) : (m, p, b) => cli.api(m, p, b));
  if (agentStandIn) void agent.refresh();
  /** Web Serial, which the page never uses while it drives the simulator. */
  const ports = webSerial(serial, () => cli.sim !== null);
  cli.onDevice = (need) => (need ? op.handOver() : op.cliReturned());

  /** The start screen until a project is open; then 定義 and 運用. */
  let screen = $state<'start' | 'work'>('start');
  const start = new Start({
    cli,
    // serve switched to the project's keys (Cli refreshed hello): the page verifies with that key from now on.
    opened: async () => {
      useReleaseKey();
      useServedDefinitions(await cli.definitions());
      await openWork();
    },
    // Development stand-ins (?mock, ?sim) have no serve: an unsaved 構成 on the page.
    local: dev ? (c) => startLocal(c) : null,
  });
  /** `#mode=operate` / `#mode=define`: the mode a reload or a shared URL comes back to. */
  const wantMode = /(?:^|[#&])mode=(operate|define)\b/.exec(globalThis.location?.hash ?? '')?.[1] as 'operate' | 'define' | undefined;

  /**
   * The release key: dagon serve writes the developer's key into the page (meta) and /v1/hello;
   * a page it serves verifies with that key only, and with none until it is there.
   */
  // The public demo page has no serve behind it and never calls one.
  void (DEMO_SITE ? Promise.resolve() : cli.connect())
    .then(async () => {
      await dagonReady();
      if (cli.state === 'none') return;
      require_release_key();
      useReleaseKey(document.querySelector<HTMLMetaElement>('meta[name="dagon-release-key"]')?.content);
      if (cli.state !== 'ready') return;
      void agent.refresh();
      // The project's MCU and board definitions; the builtin set when serve has none (or no serve).
      useServedDefinitions(await cli.definitions());
      // A project open in serve opens at once; without one the page stays on the start screen.
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

  /** The open project's 構成 (its YAML through serve): 運用 when it has no problems, else 定義. */
  // The start screen is reached only through `leave` (menu, 最初からやり直す), so no unsaved edit is here when a project opens.
  async function openWork(want?: 'operate' | 'define') {
    const yaml = await cli.loadTopology().catch(() => null);
    // 呼び名 and positions live beside the YAML (GET /v1/layout).
    projectLayout = (await cli.loadLayout().catch(() => null)) ?? {};
    local = false;
    op.reset();
    d.reset();
    if (yaml) {
      try {
        d.load(fromYaml(yaml, projectLayout.names ?? {}), yaml);
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
    // #mode=operate on a 構成 that cannot be operated: 定義 with the reason.
    if (want === 'operate' && m === 'define') lock(d.operable);
  }

  /** Without serve (development stand-ins): an empty 構成 or the sample, not saved. */
  function startLocal(c: 'new' | 'sample') {
    local = true;
    op.reset();
    d.reset();
    if (c === 'sample') d.load(fromYaml(SAMPLE_YAML, Object.fromEntries(SAMPLE_CONFIG.nodes.map((n) => [n.id, n.name]))), SAMPLE_YAML);
    enterWork('define');
  }

  /** The menu under the header, and its one confirmation. */
  let menuOpen = $state(false);
  let helpOpen = $state(false);
  let restartOpen = $state(false);
  let restartButton = $state<HTMLButtonElement>();
  /** Menu 新しく作る / 見本から新しく作る / 開く: the 構成 is saved first (or the dialog asks). */
  function toStart(c: StartChoice) {
    if (stopped(DEMO_SITE && c === 'open' ? 'open' : 'project')) return;
    menuOpen = false;
    void autosave.leave(() => goStart(c));
  }

  function goStart(c: StartChoice) {
    cliPanel = null;
    screen = 'start';
    void start.refresh();
    void start.choose(c);
  }

  /** "最初からやり直す": the page's records go and the start screen opens; the project's files stay. */
  function restart() {
    if (stopped('project')) return void (restartOpen = false);
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

  /** 運用 pressed while it cannot be entered: M69 (or 未保存) in the band until the 構成 changes. */
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
    if (stopped('file')) return;
    if (!(await autosave.ready())) return lock('unsaved');
    switchMode('operate');
    op.takeFile(name, bytes);
  }

  /** 運用 / 定義. The define document goes to the operate canvas when leaving define mode. */
  let mode = $state<'operate' | 'define'>('define');
  const busyStates = ['writing', 'hubRestarting', 'reconnectNeeded', 'stopped', 'recovering'];
  const modeLocked = $derived(busyStates.includes(op.state));

  function switchMode(m: 'operate' | 'define') {
    // 運用 only with a 構成 that has no problems (parts not updatable yet are skipped there); the YAML decides, never the 親機.
    if (m === 'operate' && d.operable !== 'ok') return lock(d.operable);
    // ... and only the saved one, the 構成 build, sign and release use too.
    if (m === 'operate' && autosave.pending) {
      void autosave.ready().then((ok) => (ok ? enterMode(m) : lock('unsaved')));
      return;
    }
    enterMode(m);
  }

  function enterMode(m: 'operate' | 'define') {
    // A file checked against another 構成 (fileRefused, otherTopology or fileReady, e.g. after "ファイルの構成を取り込む") is checked again.
    const recheck = m === 'operate' && op.file !== null && ['fileRefused', 'otherTopology', 'fileReady'].includes(op.state) && op.config?.yaml !== d.config.yaml;
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
    if (stopped('connect')) return;
    // The 親機 gets the saved 構成 only.
    if (!(await autosave.ready())) return lock('unsaved');
    op.useConfig(d.config);
    if (!connected) await op.connect();
    if (connected && op.config) await op.push();
  }

  /** Development stand-ins (?mock, ?sim without serve): an unsaved 構成 on the page, nothing to save to. */
  let local = $state(false);
  /** Edits go to the project's YAML by themselves; 保存 saves at once. */
  const autosave = new Autosave(d, {
    put: (yaml) => cli.saveTopology(yaml),
    enabled: () => screen === 'work' && !local,
    blocked: () => cli.defsFailed,
  });
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

  /** The project's definitions, or none at all with × 定義を読めない when they fail (never the builtin set instead). */
  function useServedDefinitions(json: string | null) {
    if (useDefinitions(json, cli.defsFailed) === 'blocked') cli.defsFailed = true;
    // Blocked: a save waiting for its quiet time is dropped (none is sent while blocked).
    if (cli.defsFailed) autosave.cancel();
  }

  /** "ファイルの構成を取り込む": the release's 構成 replaces this one (undoable) and goes to the YAML. */
  async function importFileTopology() {
    const t = op.fileTopology;
    // Editing stopped (the lock table): nothing is replaced and nothing is saved.
    if (!t || !d.importRelease(t as unknown as Topo)) return;
    op.fileTopology = null;
    await save();
  }
  const fileDiff = $derived(op.state === 'otherTopology' && op.fileTopology ? topoDiff(d.doc.topo, op.fileTopology as unknown as Topo, d.doc.names) : []);

  /** Define mode's drawing scale: up to 32 ノード fit by zooming out; the canvas scrolls to pan. */
  let zoom = $state(1);

  /** The linking dialog, and requests to open DAGON's bubble. */
  let linkOpen = $state(false);
  let dagonOpen = $state(0);
  async function openLink(at?: Element | null) {
    if (cli.sim) return showWhy(AGENT.demoLink, at);
    if (stopped('link', at)) return;
    if (!agent.status) await agent.refresh();
    if (!agent.status) return showWhy(AGENT.errors.OLD_CLI.text, at);
    menuOpen = false;
    linkOpen = true;
  }
  async function agree(choice: AgentChoice) {
    linkOpen = false;
    const p = await agent.setLink(choice);
    // An empty text: the lock table stopped it and has already said why.
    if (p) return void (p.text && showWhy(p.text, null, p.failed));
    const c = agent.status?.consent;
    note(AGENT.linked(agent.status?.provider.name ?? '', c?.at ?? ''));
    dagonOpen++;
  }
  async function unlink() {
    menuOpen = false;
    const p = await agent.setLink(null);
    if (p) return void (p.text && showWhy(p.text, null, p.failed));
    note(AGENT.unlinked);
    if (drawerTab === 'talk') drawerTab = null;
  }
  /** What is selected, as the agent's question names it. */
  function selection(): Selection | undefined {
    const k = mode === 'define' ? d.selected : op.selected;
    if (!k) return undefined;
    if (k.startsWith('node:')) return { node: k.slice(5) };
    const to = k.slice(5);
    const from = d.config.links.find((l) => l.to === to)?.from;
    return from ? { link: { from, to } } : undefined;
  }
  function askDagon(text: string, kind: AskKind, at: Element) {
    if (stopped('ask', at)) return;
    void agent.ask(text, kind, selection());
  }
  /** 『反映する』: the proposal goes in as one 構成 edit (undoable) and is saved like any edit. */
  function applyProposal(i: number, at: Element) {
    if (stopped('apply', at)) return;
    const p = agent.messages[i]?.proposal;
    if (!p) return;
    try {
      if (!d.importRelease(fromYaml(p.topology, d.doc.names).topo)) return;
    } catch {
      return showWhy(AGENT.errors.AGENT_FAILED.text, at, true);
    }
    agent.applied(i);
    note(AGENT.applied);
  }

  /** The bottom drawer's open tab, null while closed. */
  let drawerTab = $state<string | null>(null);
  const pickTab = (id: string) => (drawerTab = drawerTab === id ? null : id);
  /** 記録: what the page did and why something was stopped, newest last (the drawer's log). */
  let notes = $state<{ at: string; text: string }[]>([]);
  function note(text: string) {
    notes = [...notes, { at: new Date().toTimeString().slice(0, 8), text }].slice(-200);
  }
  const problemCount = $derived(d.problems.filter((p) => p.severity === 'error').length);
  const drawerTabs = $derived<DrawerTab[]>([
    { id: 'problems', label: DRAWER.problems, count: problemCount + d.notYet, alarm: problemCount > 0 },
    { id: 'jobs', label: DRAWER.jobs, ...(runningJobs ? { count: runningJobs } : {}) },
    { id: 'output', label: DRAWER.output },
    { id: 'timeline', label: DRAWER.timeline },
    { id: 'talk', label: DRAWER.talk },
    { id: 'log', label: DRAWER.log },
  ]);
  /** 定義's next step in one sentence (the status row), and its failure cause. */
  const defineNext = $derived.by((): { cause?: string; sentence: string } => {
    const firstError = d.problems.find((p) => p.severity === 'error');
    if (d.saveFailure) return { cause: SAVE[d.saveFailure].chip, sentence: SAVE[d.saveFailure].sentence };
    if (lockShown === 'unsaved') return { cause: SAVE.unsaved, sentence: MODE.lockedUnsaved.sentence };
    if (lockShown) return { sentence: MODE.lockedProblems.sentence };
    if (cli.state !== 'ready' && !local) return { sentence: CLI_LINK.needCli };
    if (cli.defsFailed) return { cause: DEFINE.defsChip, sentence: DEFINE.defsSentence };
    if (d.unreadable) return { cause: DEFINE.unreadableChip, sentence: DEFINE.unreadableSentence };
    if (firstError) return { sentence: DEFINE.fixSentence };
    if (d.notYet) return { sentence: DEFINE.notYetSentence(d.notYet) };
    if (d.doc.topo.nodes.length === 0) return { sentence: CANVAS.empty };
    return { sentence: '' };
  });
  const savedLabel = $derived(local ? '' : d.unsaved ? SAVE.unsaved : autosave.pending ? STATUS.saving : STATUS.saved);
  /** Where each flow step leads (02–04 open their CLI sheet in 定義; 05 is 運用). */
  function goStep(step: 'define' | 'project' | 'build' | 'flash' | 'update') {
    if (step === 'update') return switchMode('operate');
    if (mode !== 'define') switchMode('define');
    d.selected = null;
    cliPanel = step === 'define' ? null : step === 'build' ? 'release' : step;
    if (step === 'define' && problemCount > 0) drawerTab = 'problems';
  }

  let graph = $state<ReturnType<typeof Graph>>();
  /** The node a dragged part would go under (highlighted on the canvas). */
  let dropOn = $state<string | null>(null);

  /** A part dragged from the parts window to the canvas: a ghost follows the pointer. */
  let ghost = $state<{ part: Part; x: number; y: number; shown: boolean } | null>(null);
  const DRAG_PX = 4;
  function startPart(part: Part, e: PointerEvent) {
    const x0 = e.clientX;
    const y0 = e.clientY;
    ghost = { part, x: x0, y: y0, shown: false };
    const move = (ev: PointerEvent) => {
      if (!ghost) return;
      const shown = ghost.shown || Math.hypot(ev.clientX - x0, ev.clientY - y0) > DRAG_PX;
      ghost = { ...ghost, x: ev.clientX, y: ev.clientY, shown };
      dropOn = shown ? (graph?.dropTarget(ev.clientX, ev.clientY)?.parent ?? null) : null;
    };
    const up = (ev: PointerEvent) => {
      removeEventListener('pointermove', move);
      removeEventListener('pointerup', up);
      const g = ghost;
      ghost = null;
      dropOn = null;
      if (!g?.shown) return;
      const t = graph?.dropTarget(ev.clientX, ev.clientY);
      if (!t || stopped('edit')) return;
      d.placeAt(g.part.key, g.part.name, t.parent);
    };
    addEventListener('pointermove', move);
    addEventListener('pointerup', up);
  }
  /** A refused port drag: the reason in red (it is a refusal). */
  let refuseAt = 0;
  function refuse(text: string, x: number, y: number) {
    const now = Date.now();
    if (why?.text === text && now - refuseAt < 600) return;
    refuseAt = now;
    showWhy(text, null, true);
    why = why && { ...why, x, y };
  }

  /** Widths of the floating windows, so the trees centre between them. */
  let partsW = $state(0);
  let sheetW = $state(0);
  let sheetH = $state(0);

  /** Ctrl+Z / Ctrl+Shift+Z anywhere but in a field; Delete and L on the canvas; Ctrl+J the drawer. */
  function defineKeys(e: KeyboardEvent) {
    if (screen === 'work' && e.key.toLowerCase() === 'j' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      return pickTab(drawerTab ?? 'problems');
    }
    const t = e.target as HTMLElement;
    if (screen === 'work' && agent.linked && e.key === '/' && !/^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName)) {
      e.preventDefault();
      return void dagonOpen++;
    }
    if (mode !== 'define') return;
    const el = e.target as HTMLElement;
    const field = /^(INPUT|SELECT|TEXTAREA)$/.test(el.tagName);
    const z = e.key.toLowerCase() === 'z' && (e.ctrlKey || e.metaKey);
    if (z && !field) {
      e.preventDefault();
      if (stopped('edit')) return;
      return e.shiftKey ? d.redo() : d.undo();
    }
    if (field || !el.closest('#canvas')) return;
    if (e.key === 'Delete') {
      e.preventDefault();
      if (stopped('edit')) return;
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
  /** 定義's focal number: 問題 n, red only above 0; ノード, リンク and ◇ under it. */
  const defineDot = $derived<Dot>({
    label: DEFINE.dotProblems,
    value: problemCount,
    alarm: true,
    sub: [DEFINE.dotNodes, d.doc.topo.nodes.length] as const,
    more: [[DEFINE.dotLinks, String(d.doc.topo.links?.length ?? 0)], ...(d.notYet ? [[DEFINE.dotNotYet, String(d.notYet)] as const] : [])],
    name: DEFINE.dotProblemsName(problemCount, d.doc.topo.nodes.length, d.doc.topo.links?.length ?? 0),
  });

  /** Chrome's SerialConnectionEvent names the port; the spec's event targets it. */
  const portOf = (e: Event) => ((e as Event & { port?: SerialPort }).port ?? e.target) as SerialPort;
  ports.on('disconnect', (e) => {
    if (opened instanceof WebSerialTransport && portOf(e) === opened.port) op.lose();
  });
  /**
   * An allowed device came (back): while dagon-host reopens the rebooted hub
   * (hubRestarting) it is a candidate; once that is over (reconnectNeeded) the page takes it by
   * itself when it is the hub on trial with the version just sent.
   */
  ports.on('connect', (e) => {
    const port = portOf(e);
    // One device at a time: candidates reported together are judged in turn.
    connecting = connecting.then(() => hubConnected(port)).catch(() => undefined);
  });
  let connecting: Promise<void> = Promise.resolve();
  /** "もう一度つなぐ": the device the user chose, kept only when Operate took it as this hub. */
  async function reconnectChosen() {
    if (await op.reconnectHub()) opened = op.device;
  }
  async function hubConnected(port: SerialPort) {
    if (opened instanceof WebSerialTransport) opened.arrived(port);
    if (op.state !== 'reconnectNeeded') return;
    const t = new WebSerialTransport(port);
    const took = await op.reconnectHub(async () => {
      await t.open();
      return t;
    }, true);
    if (took) opened = t;
  }

  let primaryEl = $state<HTMLButtonElement>();
  // Phones start with the canvas: the parts window opens from "＋ 部品".
  let partsOpen = $state(globalThis.innerWidth === undefined || innerWidth >= 768);
  /** 運用's update order: the top-left stays clear there; it opens from its button. */
  let orderOpen = $state(false);
  let nextEl = $state<HTMLElement>();
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

  /** Buttons whose procedures this page runs; the others show as unavailable. */
  const RUNS: ReadonlySet<Action> = new Set([
    'connectDiscover', 'connect', 'reselect', 'reconnect', 'push', 'pushAgain', 'scanAgain', 'reload',
    'chooseFile', 'updateResponding', 'otherFile', 'update', 'useLatest', 'recover', 'recoverAgain', 'defineFix',
  ]);

  /** States that show the update's progress (node chips, percent, the dot number). */
  const UPDATING: ReadonlySet<string> = new Set(['writing', 'hubRestarting', 'reconnectNeeded', 'stopped', 'done', 'recovering', 'recoverFailed']);
  /** States whose dot number and update-order rows show percent. */
  const PERCENT: ReadonlySet<string> = new Set(['writing', 'hubRestarting', 'reconnectNeeded', 'stopped', 'recovering']);
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
  /** Stages in which a card shows → the new version and its one %. */
  const FILLING_STAGES: ReadonlySet<Progress['stage']> = new Set(['writing', 'lastResort', 'rebooting']);
  const updating = $derived(UPDATING.has(op.state));
  const diffChips = $derived(Object.fromEntries(fileDiff.flatMap((r) => r.nodes).map((id) => [id, { chip: DIFF.chip, spoken: DIFF.spoken, silent: true }])));
  /** a flat minute per node, the copy's 目安; measure per link and image size if it misleads. */
  const MINUTES_PER_NODE = 1;
  /** On the cards while and after writing: → the new version, then 前のバージョンに戻った / 書き込めなかった; nothing once confirmed. */
  const RESULT: Partial<Record<Progress['stage'], { word: string; silent: boolean }>> = {
    previous: { word: WRITE.previous, silent: false },
    refused: { word: WRITE.failed, silent: true },
    silent: { word: WRITE.failed, silent: true },
    cannotFix: { word: WRITE.failed, silent: true },
    unwritten: { word: WRITE.failed, silent: true },
  };
  const cardChipOf = (id: string, p: Progress): Chip | null => {
    const to = op.fileVersions[id];
    if (FILLING_STAGES.has(p.stage)) return to === undefined ? null : { chip: WRITE.to(versionText(to)), spoken: WRITE.to(versionText(to)), silent: false };
    const r = RESULT[p.stage];
    return r ? { chip: r.word, spoken: r.word.slice(2), silent: r.silent } : null;
  };
  const progressChips = $derived(
    Object.fromEntries(
      Object.entries(op.progress).flatMap(([id, p]): [string, Chip][] => {
        const c = cardChipOf(id, p);
        if (!c) return [];
        // M27 beside the rebooting hub while the page reconnects by itself.
        return [[id, p.stage === 'rebooting' && reconnectLeft !== null ? { ...c, note: `${RECONNECT.auto} ${RECONNECT.left(reconnectLeft)}` } : c]];
      }),
    ),
  );
  /** ビルドする in 準備: build every node (cargo skips unchanged ones), sign with the project's key, then make the release 書込 offers first; the jobs are serve's. */
  let buildIds = $state<{ build: number | null; sign: number | null; release: number | null }>({ build: null, sign: null, release: null });
  let buildFailureOpen = $state(false);
  const jobOf = (id: number | null) => (id === null ? null : (cli.jobs.find((j) => j.id === id) ?? null));
  const buildJob = $derived(jobOf(buildIds.build));
  const signJob = $derived(jobOf(buildIds.sign));
  const releaseJob = $derived(jobOf(buildIds.release));
  /** Where the build stands: null before any, else running, done or failed (signing counts as building). */
  const buildState = $derived.by(() => {
    const jobs = [buildJob, signJob, releaseJob].filter((j) => j !== null);
    if (jobs.length === 0) return null;
    if (jobs.some((j) => j.state === 'failed')) return 'failed';
    if (releaseJob?.state === 'succeeded' || (buildJob?.state === 'succeeded' && !cli.hello?.key)) return 'done';
    return 'running';
  });
  async function build() {
    if (stopped('build') || !cli.has('build')) return;
    buildFailureOpen = false;
    buildIds = { build: null, sign: null, release: null };
    await autosave.whenSaved(async () => {
      const j = await cli.build(d.config.reach);
      buildIds = { build: j?.id ?? null, sign: null, release: null };
    });
  }
  $effect(() => {
    const s = buildState;
    untrack(() => {
      if (buildJob?.state === 'succeeded' && buildIds.sign === null && cli.hello?.key) {
        buildIds = { ...buildIds, sign: -1 };
        void cli.sign(d.config.reach).then((j) => (buildIds = { ...buildIds, sign: j?.id ?? null }));
      }
      if (signJob?.state === 'succeeded' && buildIds.release === null) {
        buildIds = { ...buildIds, release: -1 };
        void cli.release().then((j) => (buildIds = { ...buildIds, release: j?.id ?? null }));
      }
      if (s === 'done') {
        notice = { word: fillIn(BUILD.toast.done, { n: d.config.reach.length }) };
        void loadVersions();
      }
      if (s === 'failed') buildFailureOpen = true;
    });
  });
  /** While building, only the card of the node cargo builds now says so; nothing stays once it is done. */
  const buildingChips = $derived.by((): Record<string, Chip> => {
    if (buildJob?.state !== 'running') return {};
    const line = [...buildJob.lines].reverse().find((l) => /^\S+: building /.test(l));
    const id = line?.slice(0, line.indexOf(':'));
    return id && d.config.reach.includes(id) ? { [id]: { chip: BUILD.state.running, spoken: BUILD.state.running, silent: false } } : {};
  });
  const chips = $derived(updating ? progressChips : diffChips);
  /** The water in each node while it is written (writing, hubRestarting, recovering). */
  const fill = $derived(PERCENT.has(op.state) ? Object.fromEntries(Object.keys(waterOf(op.progress)).map((id) => [id, op.percentOf(id, nowMs)])) : {});
  const percent = $derived(
    PERCENT.has(op.state) ? Object.fromEntries(Object.keys(op.progress).map((id) => [id, op.percentOf(id, nowMs)])) : {},
  );
  // The comparison stays while the update runs (P14: never silently replaced).
  /** The refusal modal: why the package cannot be written and what to do; closing leaves the band's buttons. */
  let refusalClosed = $state<ReleaseFile | null>(null);
  const refusalOpen = $derived(op.file !== null && (op.state === 'fileRefused' || op.state === 'otherTopology') && refusalClosed !== op.file);
  /** A write that stopped at a node (back on its previous version, or not written): what happened and what to do, until closed. */
  let stopClosed = $state<string | null>(null);
  const stopNode = $derived(op.state === 'stopped' && op.failed ? (op.config?.nodes.find((n) => n.id === op.failed) ?? null) : null);
  const stopOpen = $derived(stopNode !== null && stopClosed !== op.failed);
  $effect(() => {
    if (op.state !== 'stopped') stopClosed = null;
  });
  const refusalWord = (v: ReleaseFile['verdict']) => (v === 'topology' ? FILE.topologyBad : v === 'format' ? FILE.notRelease : FILE.sourceBad);

  let fileInput = $state<HTMLInputElement>();
  async function picked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const f = input.files?.[0];
    input.value = '';
    if (f && stopped(pickForRecovery ? 'recover' : 'file')) return void (pickForRecovery = false);
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
    if (stopped('recover')) return;
    if ((await (again ? op.retry() : op.recover())) === 'needFile') {
      pickForRecovery = true;
      fileInput?.click();
    }
  }
  /** The sheet shows only when it holds something. */
  const sheetShown = $derived(
    mode === 'define'
      ? cliPanel !== null || d.selected !== null
      : !op.config || op.selected !== null || (!!op.recovering && !!op.recovered),
  );
  $effect(() => {
    if (!sheetShown || layout === 'narrow') sheetW = 0;
  });
  $effect(() => {
    if (layout === 'narrow' || !(mode === 'define' ? partsOpen : orderOpen)) partsW = 0;
  });
  /** Node actions in the detail: while watching, with nothing running. */
  const nodeActions = $derived(['ready', 'nodesSilent', 'done'].includes(op.state));

  /** The reconnect dialog only after the automatic reconnect failed (reconnectNeeded); Escape leaves the same button in the band. */
  let dialogDismissed = $state(false);
  const dialogOpen = $derived(op.state === 'reconnectNeeded' && !dialogDismissed);
  $effect(() => {
    if (op.state !== 'reconnectNeeded') dialogDismissed = false;
  });
  /** Clock for M27 (dagon-host's clock) and the elapsed time (wall clock), once a second while they show. */
  let nowMs = $state(0);
  let wallMs = $state(Date.now());
  $effect(() => {
    if (!PERCENT.has(op.state)) return;
    const tick = () => {
      nowMs = op.now();
      wallMs = Date.now();
    };
    tick();
    const t = setInterval(tick, 1000);
    return () => clearInterval(t);
  });
  /** The timeline: each node's stage changes since the update began; it opens with the update. */
  let tlEvents = $state<StageEvent[]>([]);
  let tlStart = 0;
  let tlLast: Record<string, string> = {};
  $effect(() => {
    const start = op.startedAt;
    const prog = op.progress;
    if (start === null) return;
    untrack(() => {
      if (start !== tlStart) {
        tlStart = start;
        tlEvents = [];
        tlLast = {};
        if (drawerTab === null) drawerTab = 'timeline';
      }
      const t = Date.now() - start;
      const add = Object.entries(prog)
        .filter(([id, p]) => tlLast[id] !== p.stage)
        .map(([id, p]) => ((tlLast[id] = p.stage), { t, id, stage: p.stage }));
      if (add.length) tlEvents = [...tlEvents, ...add];
    });
  });
  const tlNow = $derived(tlStart && PERCENT.has(op.state) ? Math.max(0, wallMs - tlStart) : Math.max(0, ...tlEvents.map((e) => e.t)));
  /** 書込's version picker: the latest build (the project's newest release) first, then the kept packages; writing an older one is going back. */
  let latest = $state<{ bytes: Uint8Array; label: string } | null>(null);
  let kept = $state<Kept[]>([]);
  let chosen = $state('latest');
  /** One version for all of a package's nodes names it; otherwise it is 最新のビルド. */
  function labelOf(bytes: Uint8Array): string {
    const one = [...new Set([...releaseVersions(bytes).values()].map(versionText))];
    return one.length === 1 ? one[0] : WRITE.latest;
  }
  async function loadVersions() {
    const served = cli.state === 'ready';
    // Without serve the stand-in's release is the latest build (development, the demo page); nothing is kept.
    const fetchLatest = served ? () => cli.latestRelease() : stand?.latest;
    if (!fetchLatest) return;
    const bytes = await fetchLatest().catch(() => null);
    latest = bytes ? { bytes, label: labelOf(bytes) } : null;
    kept = served ? await cli.keptList().catch(() => []) : [];
    if (chosen === 'latest' ? !latest : !kept.some((k) => k.id === chosen)) chosen = latest ? 'latest' : (kept[0]?.id ?? 'latest');
  }
  const versionChoices = $derived([
    ...(latest ? [{ id: 'latest', label: latest.label }] : []),
    ...keptChoices(kept).filter((c) => c.label !== latest?.label),
  ]);
  $effect(() => {
    // Again on entering 書込 and after each write (the written package was kept).
    if (mode === 'operate' && (op.state === 'ready' || op.state === 'done')) untrack(() => void loadVersions());
  });
  /** 書き込む while watching: the chosen version, checked against the 構成, to the checked cards. */
  async function writeChosen() {
    if (stopped('update')) return;
    const pkg = chosen === 'latest' ? (latest ? { name: `${latest.label}.dgb`, bytes: latest.bytes } : null) : await cli.keptBytes(chosen).then((b) => (b ? { name: `${chosen}.dgb`, bytes: b } : null));
    if (!pkg) {
      notice = { word: BAND.M58.chip, error: true, detail: BAND.M58.sentence };
      return;
    }
    op.takeFile(pkg.name, pkg.bytes);
    if (op.state === 'fileReady') await op.update();
  }
  $effect(() => {
    if (op.keepFailed) untrack(() => (notice = { word: WRITE.keepFailed.label, error: true, detail: WRITE.keepFailed.detail }));
  });
  const stageWord = (s: string) => (CHIP[s as Progress['stage']] ?? s).replace(/^\S+\s/, '');
  /** 出力: the CLI jobs' output lines, oldest job first (the latest 300). */
  const outputLines = $derived(
    [...cli.jobs]
      .reverse()
      .flatMap((j) => j.lines.map((line) => ({ kind: j.kind, line })))
      .slice(-300),
  );

  /** M27: seconds left of the automatic reconnect, in 5 s steps (hubRestarting). */
  const RECONNECT_STEP_S = 5;
  const reconnectLeft = $derived(
    op.reconnectUntil === null || op.state !== 'hubRestarting'
      ? null
      : Math.ceil(Math.max(0, (op.reconnectUntil - nowMs) / 1000) / RECONNECT_STEP_S) * RECONNECT_STEP_S,
  );

  const hub = $derived(hubOf(op.state));
  /** The 親機 this page talks to (the one writing reaches). */
  const hubNode = $derived(op.config?.nodes.find((n) => n.role === 'hub' && op.config?.reach.includes(n.id)));
  const connected = $derived(!['idle', 'lost'].includes(hub));
  /** DAGON waits for its agent's answer (the lock table's 返事待ち). */
  const thinking = $derived(agent.thinking);
  const BUILD_KINDS = new Set(['build', 'sign', 'release', 'gen', 'conform']);
  const running = (kinds: (k: string) => boolean) => cli.jobs.some((j) => (j.state === 'running' || j.state === 'queued') && kinds(j.kind));
  /** The lock table's states now, leftmost first. */
  const lockStates = $derived(
    activeStates({
      // the page has no demo playback of its own yet; `dagon demo`'s simulator is driven by the user like a real one.
      demo: false,
      updating: ['writing', 'hubRestarting', 'reconnectNeeded'].includes(op.state),
      recovering: op.state === 'recovering',
      flashing: running((k) => k === 'flash'),
      building: running((k) => BUILD_KINDS.has(k)),
      unsaved: autosave.pending && !local,
      connecting: ['choosing', 'checking', 'scanning'].includes(op.state),
      disconnected: !connected,
      thinking,
      // The development stand-in answers for serve's agent routes, so DAGON is not stopped by its absence.
      site: DEMO_SITE,
      cliNone: cli.state !== 'ready' && !agentStandIn,
    }),
  );
  const lockedBy = (c: Control) => lockOf(c, lockStates);
  /** Operations stopped now: dimmed through `data-locks` on the frame (base.css). */
  const locksNow = $derived((Object.keys(LOCK.controls) as Control[]).filter((c) => lockedBy(c) !== null).join(' '));
  /** The reason of a stopped operation, shown where it was pressed (information style, role=status). */
  let why = $state<{ text: string; x: number; y: number; failed: boolean } | null>(null);
  let whyTimer: ReturnType<typeof setTimeout> | undefined;
  const WHY_MS = 4000;
  /** `failed`: a refused operation (red, role=alert); otherwise information. */
  function showWhy(text: string, at?: Element | null, failed = false) {
    const r = at?.getBoundingClientRect();
    why = { text, x: r ? r.left : innerWidth / 2, y: r ? r.bottom + 6 : innerHeight / 2, failed };
    note(text);
    clearTimeout(whyTimer);
    whyTimer = setTimeout(() => (why = null), WHY_MS);
  }
  /** True (and the reason shown) when `c` is stopped now: the one check buttons, menus, keys and commands go through. */
  function stopped(c: Control, at?: Element | null): boolean {
    const r = lockedBy(c);
    if (r) showWhy(r, at);
    return r !== null;
  }
  /** Every control marked data-control: pressing a stopped one shows why and does nothing else. */
  function lockClick(e: MouseEvent) {
    const el = (e.target as Element | null)?.closest?.<HTMLElement>('[data-control]');
    if (!el || !stopped(el.dataset.control as Control, el)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
  }
  /** Typing into a field of a stopped part (the inspector, the table) is stopped the same way. */
  function lockKeys(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (!/^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName) || ['Tab', 'Escape', 'Shift'].includes(e.key)) return;
    const el = t.closest<HTMLElement>('[data-control]');
    if (!el || !stopped(el.dataset.control as Control, t)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
  }
  /**
   * The public demo page keeps controls stopped for good (files, devices, keys), so their reason is also the
   * hover text and the accessible description, not only the note shown on press. The control's own title comes back once it is usable.
   */
  function lockHint(e: Event) {
    const el = (e.target as Element | null)?.closest?.<HTMLElement>('[data-control]');
    if (!el) return;
    const r = lockedBy(el.dataset.control as Control);
    if (r) {
      el.dataset.title ??= el.title;
      el.title = r;
    } else if (el.dataset.title !== undefined) {
      el.title = el.dataset.title;
      delete el.dataset.title;
    }
  }
  // On the window in the capture phase: ahead of every handler on the page (Svelte delegates clicks to its root).
  $effect(() => {
    addEventListener('click', lockClick, true);
    addEventListener('keydown', lockKeys, true);
    if (DEMO_SITE) {
      addEventListener('pointerover', lockHint, true);
      addEventListener('focusin', lockHint, true);
    }
    return () => {
      removeEventListener('click', lockClick, true);
      removeEventListener('keydown', lockKeys, true);
      removeEventListener('pointerover', lockHint, true);
      removeEventListener('focusin', lockHint, true);
    };
  });
  // The models check the lock table inside their actions: any path to them (key, double-click, drag) is stopped.
  d.guard = (k) => stopped(k);
  cli.guard = (k) => stopped(k);
  agent.guard = (k) => stopped(k);
  /** Band actions as lock-table operations. */
  const CONTROL_OF: Partial<Record<Action, Control>> = {
    chooseFile: 'file', updateResponding: 'file', otherFile: 'file', useLatest: 'update', update: 'update',
    recover: 'recover', recoverAgain: 'recover', connect: 'connect', connectDiscover: 'connect', reselect: 'connect', reconnect: 'connect',
  };
  /** Why the header's つなぐ / 切断 cannot be used now. */
  const hubLocked = $derived(HUB_LOCKED.has(op.state) ? (HUB_TOGGLE.locked[op.state] ?? null) : null);
  /** The locked toggle was pressed: its reason stands in the band until the state changes. */
  let hubNotice = $state<{ at: string; why: string } | null>(null);
  const hubNoticeShown = $derived(hubNotice && hubNotice.at === op.state ? hubNotice.why : null);
  /** The header's つなぐ / 切断: 切断 closes the port and keeps its permission. */
  function toggleHub() {
    if (stopped('connect')) return;
    if (hubLocked) {
      hubNotice = { at: op.state, why: hubLocked };
      if (mode === 'operate') void tick().then(() => nextEl?.focus());
      return;
    }
    if (connected) return void op.disconnect();
    if (op.state === 'portFailed' && op.failure === 'unavailable') return location.reload();
    if (mode === 'define' && d.operable === 'ok') op.useConfig(d.config);
    void op.connect();
  }
  const step = $derived(view(op.state, op.ctx).step);
  const unwritten = $derived(Object.values(op.progress).filter((p) => p.stage === 'unwritten').length);
  const band = $derived.by(() => {
    const b = bandOf(op.state, op.ctx, {
      stuck: op.recoverTarget?.name,
      responding: op.responding,
      unwritten,
      rest: op.rest && op.state === 'fileReady',
      finish: op.finished,
      refused: op.returnRefused?.kind,
      sim: simulated,
    });
    if (dialogOpen) return { ...b, primary: null };
    // The demo page reads no files: its prepared package is what 書き込む writes.
    if (DEMO_SITE && op.state === 'ready') return { ...b, sentence: SIM.siteWrite };
    if (op.latestMissing) return { ...b, ...BAND.M58 };
    if (hubNoticeShown) return { ...b, chip: undefined, pair: undefined, sentence: hubNoticeShown };
    return b;
  });
  /** The status row's sentence in either mode. */
  const next = $derived(
    mode === 'define'
      ? defineNext
      : { cause: band.chip, sentence: [band.pair ? `${band.pair[0]} ${band.pair[1]}` : '', band.sentence ?? ''].filter(Boolean).join('　') },
  );
  const dot = $derived.by((): Dot | null => {
    const c = op.config;
    if (!c || !connected || op.state === 'checking') return null;
    if (PERCENT.has(op.state)) {
      const stopped = op.state === 'stopped';
      const progress = { label: CANVAS.dotProgress, value: op.overall, unit: stopped ? CANVAS.dotStopped : CANVAS.dotPercent, name: CANVAS.dotProgressName(op.overall, stopped) };
      if (op.startedAt === null) return progress;
      // 経過 under the number, 残り beside it (the estimate less the time gone).
      const gone = Math.max(0, wallMs - op.startedAt);
      const nodes = op.state === 'recovering' ? 1 : Math.max(1, op.targets.length);
      const left = nodes * MINUTES_PER_NODE * 60_000 - gone;
      return { ...progress, sub: [TIME.elapsed, TIME.clock(gone)] as const, more: [[TIME.left, TIME.about(left)]] as const };
    }
    if (op.state === 'scanning' || op.state === 'scanned') return { label: CANVAS.dotFound, value: op.found, name: CANVAS.dotFoundName(op.found) };
    // Out of the nodes updating reaches: the others are never watched.
    const n = op.reach.length;
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

  /** 雛形を作る: the modal is open while `skeleton` is set; `id` is serve's job once it has taken it. */
  let skeleton = $state<{ id: number | null } | null>(null);
  const skeletonJob = $derived(skeleton?.id == null ? null : (cli.jobs.find((j) => j.id === skeleton?.id) ?? null));
  // 中止: the modal goes with the job.
  $effect(() => {
    if (skeletonJob?.state === 'cancelled') untrack(() => (skeleton = null));
  });
  async function makeSkeleton() {
    skeleton = { id: null };
    let job: Awaited<ReturnType<typeof cli.gen>> = null;
    // Made from the saved YAML: after the saves queued before it.
    await autosave.whenSaved(async () => (job = await cli.gen(d.config.reach)));
    skeleton = job ? { id: (job as { id: number }).id } : null;
  }

  /** The non-blocking notice on screen (bottom-right), or null. */
  let notice = $state<Notice | null>(null);
  const hubName = $derived(hubNode ? `[${hubNode.no}] ${hubNode.name}` : '');
  let wasConnected = false;
  $effect(() => {
    const c = connected;
    untrack(() => {
      if (c && !wasConnected && hubName) notice = { word: fillIn(CONN.toast.connected, { node: hubName }) };
      if (!c && wasConnected && hub === 'lost') notice = { word: fillIn(CONN.toast.lost, { node: hubName }), error: true };
      wasConnected = c;
    });
  });
  $effect(() => {
    if (op.state === 'done') untrack(() => (notice = { word: fillIn(WRITE.toast.done, { n: op.targets.length }) }));
  });
  // A consent of a version this dagon does not read: a failure the user must notice.
  $effect(() => {
    const p = consentProblem(agent.status);
    const why = agent.status?.consent_error ?? '';
    if (p) untrack(() => (notice = { word: p.label ?? p.text, error: true, detail: `${p.text}（${why}）` }));
  });
  // A failure or warning the user must notice: its short word on a toast, the sentence behind it.
  $effect(() => {
    const cause = next.cause;
    // A stop or refusal that needs the user opens its modal instead.
    if (cause && !untrack(() => refusalOpen || stopNode !== null)) untrack(() => (notice = { word: cause, error: cause.startsWith('×'), detail: next.sentence }));
  });

  /** 書き込む: the 親機 answered nothing (silentHub) → writing directly over USB; otherwise the next step of writing through DAGON. */
  const writeAction = $derived(
    op.state === 'silentHub' ? null : op.state === 'ready' && !op.latestMissing && band.secondary.some((b) => b.action === 'useLatest') ? 'useLatest' : (band.primary?.action ?? null),
  );
  const writeControl = $derived(writeAction ? CONTROL_OF[writeAction] : 'flash');
  /** States 書き込む writes in directly: the hub answers and nothing runs. */
  const WRITABLE: ReadonlySet<string> = new Set(['ready', 'nodesSilent', 'done']);
  const writeOff = $derived(op.state === 'choosing' || (op.state !== 'silentHub' && !WRITABLE.has(op.state) && (writeAction === null || !RUNS.has(writeAction))));
  function write() {
    if (writeOff) return;
    if (writeAction === null) return void (factory = { id: null });
    if (WRITABLE.has(op.state)) return void writeChosen();
    act(writeAction);
  }

  /** Writing directly over USB to the 親機 whose DAGON answered nothing (the hub first: it reaches the rest). */
  let factory = $state<{ id: number | null } | null>(null);
  const factoryNode = $derived(hubNode ?? op.config?.nodes.find((n) => n.role === 'hub') ?? null);
  const factoryMcu = $derived(d.doc.topo.nodes.find((n) => n.id === factoryNode?.id)?.mcu ?? '');
  const factoryJob = $derived(factory?.id == null ? null : (cli.jobs.find((j) => j.id === factory?.id) ?? null));
  async function factoryWrite() {
    if (!factoryNode || stopped('flash')) return;
    const j = await cli.flash(factoryNode.id);
    factory = { id: j?.id ?? null };
  }
  // Pressing 書き込む in the modal is the consent serve's write asks for.
  const consentAnswered = new Set<string>();
  $effect(() => {
    const j = factoryJob;
    void j?.state;
    untrack(() => answerConsent(j, (job, text) => cli.answer(job, text), consentAnswered));
  });
  $effect(() => {
    if (factoryJob?.state === 'succeeded') untrack(() => (notice = { word: WRITE.state.done }));
    if (factoryJob?.state === 'cancelled') untrack(() => (factory = null));
  });

  function act(a: Action) {
    const c = CONTROL_OF[a];
    if (c && stopped(c)) return;
    switch (a) {
      case 'connectDiscover':
      case 'connect':
      case 'reselect':
        return void op.connect();
      case 'reconnect':
        return void (op.state === 'hubRestarting' || op.state === 'reconnectNeeded' ? reconnectChosen() : op.connect());
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
  <NodeList {d} ondrag={startPart} onclose={() => (partsOpen = false)} />
{/snippet}

{#snippet zoomBar(scale: number)}
  {#if mode === 'define'}
    <button class="wide-only" data-control="edit" aria-disabled={!d.canUndo} title="{DEFINE.undo} (Ctrl+Z)" onclick={() => d.undo()}>{DEFINE.undo}</button>
    <button class="wide-only" data-control="edit" aria-disabled={!d.canRedo} title="{DEFINE.redo} (Ctrl+Shift+Z)" onclick={() => d.redo()}>{DEFINE.redo}</button>
    <span class="seg" role="radiogroup" aria-label="{DEFINE.graph}・{DEFINE.table}">
      <button role="radio" aria-checked={d.view === 'graph'} onclick={() => (d.view = 'graph')}>{DEFINE.graph}</button>
      <button role="radio" aria-checked={d.view === 'table'} onclick={() => (d.view = 'table')}>{DEFINE.table}</button>
    </span>
  {/if}
  {#if mode === 'operate' || d.view === 'graph'}
    <ZoomPill {scale} canIn={zoom < 2} canOut={zoom > 0.5} onin={() => (zoom = Math.min(2, zoom * 1.2))} onout={() => (zoom = Math.max(0.5, zoom / 1.2))} onfit={() => (zoom = 1)} />
  {/if}
{/snippet}

{#snippet defineCanvas()}
  <main id="canvas" tabindex="-1" aria-describedby="canvas-help" class="define">
    <p id="canvas-help" class="visually-hidden">{CANVAS.describe}</p>
    {#if d.view === 'graph'}
      <Graph
        bind:this={graph}
        config={d.doc.topo.nodes.length ? d.config : null}
        live={{}}
        connected={false}
        sending={false}
        selected={d.selected}
        onselect={(k) => (d.selected = d.selected === k ? null : k)}
        dot={defineDot}
        chips={{ ...defineChips, ...buildingChips }}
        {zoom}
        inset={layout === 'narrow' ? { left: 0, right: 0, bottom: sheetShown ? sheetH : 0 } : { left: partsW, right: sheetW }}
        controls={zoomBar}
        dropOn={dropOn}
        canLink={(from, to) => d.linkRefusal(from, to)}
        onlink={(from, to) => !stopped('edit') && d.link(to, from)}
        onrefuse={refuse}
      />
    {:else}
      <div class="edit-area" data-control="edit"><Table {d} onaddnode={() => document.querySelector<HTMLElement>('#list-h')?.focus()} /></div>
      <div class="zoom">{@render zoomBar(1)}</div>
    {/if}
  </main>
{/snippet}

{#snippet defineRight()}
  {#if cliPanel}
    <CliPanel {cli} {d} {autosave} {agent} panel={cliPanel} onuse={useRelease} />
  {:else if d.selected}
    <div class="edit-area" data-control="edit"><Inspector {d} /></div>
  {/if}
{/snippet}

{#snippet actions()}
  {#if mode === 'define'}
    {@const blocked = problemCount > 0 || d.doc.topo.nodes.length === 0}
    <button class="button-primary-header" data-control="gen" bind:this={primaryEl} aria-disabled={blocked} title={blocked ? SKELETON.disabled : undefined} onclick={() => !blocked && void makeSkeleton()}><span>{SKELETON.make}</span></button>
    <button class="button-secondary-header" data-control="build" aria-disabled={buildState === 'running'} onclick={() => buildState !== 'running' && void build()}><span>{BUILD.run}</span></button>
  {:else}
    {#if versionChoices.length}
      <select class="button-secondary-header version" aria-label={WRITE.version} data-control="update" disabled={updating} onchange={(e) => (chosen = e.currentTarget.value)}>
        {#each versionChoices as c (c.id)}<option value={c.id} selected={c.id === chosen} translate="no">{c.label}</option>{/each}
      </select>
    {/if}
    <button class="button-primary-header" data-control={writeControl} bind:this={primaryEl} aria-disabled={writeOff} onclick={write}><span>{WRITE.run}</span></button>
  {/if}
{/snippet}

{#snippet drawerBody(tab: string)}
  {#if tab === 'problems'}
    <Problems {d} />
  {:else if tab === 'talk'}
    {#if agent.messages.length === 0}
      <p class="log-none">{AGENT.talkNone}</p>
    {:else}
      <ol class="talk">
        {#each agent.messages as m, i (i)}
          <li class={m.role}>
            <span class="who">{m.role === 'dagon' ? AGENT.name : AGENT.you}</span>
            <p class:failed={m.failed}>{m.text}</p>
            {#if m.more}<p class="more">{m.more}</p>{/if}
            {#if m.proposal}
              <figure>
                <figcaption>{AGENT.diffCaption}</figcaption>
                <pre>{#each m.proposal.yaml_diff.split('\n') as line, k (k)}<span class:add={line.startsWith('+')} class:del={line.startsWith('-')}>{line}</span>{'\n'}{/each}</pre>
                {#if !m.applied}<button class="primary" data-control="apply" onclick={(e) => applyProposal(i, e.currentTarget)}>{AGENT.apply}</button>{/if}
              </figure>
            {/if}
          </li>
        {/each}
      </ol>
    {/if}
  {:else if tab === 'output'}
    {#if outputLines.length === 0}
      <p class="log-none">{DRAWER.outputNone}</p>
    {:else}
      <ol class="log out">
        {#each outputLines as o, i (i)}<li><span class="k">{JOBS.kinds[o.kind] ?? o.kind}</span> {o.line}</li>{/each}
      </ol>
    {/if}
  {:else if tab === 'timeline'}
    <Timeline rows={(op.config?.nodes ?? []).filter((n) => op.targets.includes(n.id)).map((n) => ({ id: n.id, name: `[${n.no}] ${n.name}` }))} events={tlEvents} now={tlNow} label={stageWord} />
  {:else if tab === 'jobs'}
    <CliPanel {cli} {d} {autosave} panel="jobs" onuse={useRelease} />
  {:else if tab === 'log'}
    {#if notes.length === 0}
      <p class="log-none">{DRAWER.logNone}</p>
    {:else}
      <ol class="log">
        {#each notes as n, i (i)}<li><time>{n.at}</time> {n.text}</li>{/each}
      </ol>
    {/if}
  {/if}
{/snippet}

{#snippet left()}
  {#if mode === 'define'}{@render defineLeft()}{:else}{@render operateLeft()}{/if}
{/snippet}

{#snippet operateLeft()}
  <button class="x close-order" aria-label={PARTS.closeOrder} title={PARTS.closeOrder} onclick={() => (orderOpen = false)}>×</button>
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
      sending={op.state === 'sending'}
      selected={op.selected}
      onselect={(k) => op.select(op.selected === k ? null : k)}
      {dot}
      {chips}
      {fill}
      {zoom}
      controls={zoomBar}
      inset={{ left: partsW, right: sheetW }}
      checks={Object.fromEntries(op.reach.map((n) => [n.id, !op.skip.has(n.id)]))}
      oncheck={(id) => op.toggleSkip(id)}
      checksLocked={updating && !['done', 'recoverFailed'].includes(op.state)}
    />
  </main>
{/snippet}

{#snippet right()}
  {#if mode === 'define'}{@render defineRight()}{:else}{@render operateRight()}{/if}
{/snippet}

{#snippet operateRight()}
      <Detail
        config={op.config}
        live={op.live}
        selected={op.selected}
        {connected}
        act={nodeActions}
        onkeep={(id) => !stopped('rollback') && void op.keepTrial(id)}
        onback={(id) => !stopped('rollback') && void op.rollback(id)}
        recovery={op.recovering && op.recovered ? { id: op.recovering, result: op.recovered } : null}
      />
{/snippet}

{#if !serial && !dev && !cli.sim}
  <Unsupported />
{:else}
  <a class="skip" href="#canvas">{SKIP.canvas}</a>
  <a class="skip" href="#band">{SKIP.next}</a>
  <!-- The DOM follows the visual order of each layout, so Tab order matches what is seen. -->
  <div class="frame {layout}" data-locks={locksNow} class:work={screen === 'work'} class:open={drawerOpen} class:modal={restartOpen || autosave.asking !== null}>
    <Header
      sim={simulated}
      {mode}
      onmode={screen === 'start' ? undefined : switchMode}
      {modeLocked}
      operateLocked={d.operable !== 'ok'}
      onlocked={() => lock(d.operable)}
      {menuOpen}
      onmenu={() => (menuOpen = !menuOpen)}
      actions={screen === 'work' ? actions : undefined}
      bind:helpOpen
      title={screen === 'work' ? (d.doc.topo.name ?? '') : ''}
    />
    {#if menuOpen}
      <Menu
        onstart={toStart}
        onrestart={() => (restartOpen = true)}
        locked={modeLocked}
        bind:restartButton
        agentLinked={agent.linked}
        onlink={openLink}
        ontalk={() => ((menuOpen = false), dagonOpen++)}
        onunlink={() => void unlink()}
        onhelp={() => ((menuOpen = false), (helpOpen = true))}
      />
    {/if}
    {#if screen === 'start'}
      <aside class="left"></aside>
      <StartScreen s={start} served={cli.state === 'ready'} />
    {:else}
      <!-- The graph fills the stage; the parts window (top-left) and the sheet (right) float over it only when they hold something. -->
      <div class="stage">
        {@render canvas()}
        {#if mode === 'define' ? partsOpen : orderOpen}
          <aside class="float parts" data-avoid="" aria-label={mode === 'define' ? PARTS.heading : ORDER.heading} bind:offsetWidth={partsW}>{@render left()}</aside>
        {:else}
          <button class="secondary tray-open" onclick={() => (mode === 'define' ? (partsOpen = true) : (orderOpen = true))}>{mode === 'define' ? PARTS.open : ORDER.heading}</button>
        {/if}
        {#if agent.linked}
          <Dagon {agent} place={cli.hello?.workspace ?? d.doc.topo.name ?? 'local'} onask={askDagon} onapply={applyProposal} ontalk={() => (drawerTab = 'talk')} openRequest={dagonOpen} />
        {/if}
        {#if notice}<Toast {notice} onmore={() => (drawerTab = 'log')} ondone={() => (notice = null)} />{/if}
        {#if sheetShown}
          <aside class="float sheet right" aria-label={SHEET} data-avoid="" bind:offsetWidth={sheetW} bind:offsetHeight={sheetH}>{@render right()}</aside>
        {/if}
      </div>
      <Drawer tabs={drawerTabs} open={drawerTab} onpick={(id) => (drawerTab = id)} onclose={() => (drawerTab = null)} body={drawerBody} />
      <Status
        hub={connected && hubNode ? `${HUB[hub]} [${hubNode.no}] ${hubNode.name}（${NODE.roleHub}）` : HUB[hub]}
        hubAction={connected ? HUB_TOGGLE.disconnect : HUB_TOGGLE.connect}
        onhub={toggleHub}
        {hubLocked}
        state={stateName(lockStates)}
        cli={cliLabel}
        oncli={() => {
          if (mode !== 'define') switchMode('define');
          d.selected = null;
          cliPanel = 'link';
        }}
        key={cli.hello?.key_id?.slice(0, 8) ?? null}
        problems={problemCount}
        unreachable={d.notYet}
        progress={PERCENT.has(op.state) ? `${LOCK.controls.update} ${op.overall}%` : null}
        sentence={[next.cause, next.sentence].filter(Boolean).join(' ')}
        mode={mode === 'define' ? MODE.define : MODE.operate}
        saved={savedLabel}
        status={d.status || op.status}
        alert={op.alert || (cli.defsFailed ? DEFINE.defsSentence : '')}
        bind:nextEl
      />
    {/if}
  </div>
  <!-- Fixed, so the hidden chooser never adds a line under the full-height frame. -->
  <input
    class="visually-hidden"
    style:position="fixed"
    type="file"
    accept=".dgb"
    tabindex="-1"
    aria-hidden="true"
    bind:this={fileInput}
    onchange={picked}
    oncancel={() => (pickForRecovery = false)}
  />
  {#if ghost?.shown}
    <div class="ghost" aria-hidden="true" style:left="{ghost.x + 8}px" style:top="{ghost.y + 8}px">{ghost.part.name}</div>
  {/if}
  {#if why}
    <!-- a stopped operation's reason is information (blue edge), never red. -->
    <div class="why" class:failed={why.failed} role={why.failed ? 'alert' : 'status'} style:left="{Math.max(8, Math.min(why.x, innerWidth - 336))}px" style:top="{Math.min(why.y, innerHeight - 96)}px">{why.text}</div>
  {/if}
  {#if linkOpen && agent.status}
    <LinkDialog status={agent.status} onagree={(c) => void agree(c)} oncancel={() => (linkOpen = false)} />
  {/if}
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
  {#if factory && factoryNode}
    <FactoryModal
      node={{ label: `[${factoryNode.no}] ${factoryNode.name}`, target: `[${factoryNode.no}] ${factoryNode.name}（${NODE.roleHub}）· ${factoryNode.product}` }}
      how={mcuConnection(factoryMcu)}
      canWrite={cli.canFlash(factoryMcu)}
      job={factoryJob}
      onwrite={() => void factoryWrite()}
      oncancel={() => (factoryJob ? void cli.cancel(factoryJob) : (factory = null))}
      onclose={() => (factory = null)}
      onmore={() => ((drawerTab = 'jobs'), (factory = null))}
    />
  {/if}
  {#if refusalOpen && op.file}
    <Modal title={FILE.heading} chip={{ text: refusalWord(op.file.verdict), kind: 'error' }} target={op.file.name} onclose={() => (refusalClosed = op.file)}>
      {#if fileDiff.length}
        <table class="modal-diff">
          <caption>{DIFF.heading}</caption>
          <thead><tr>{#each DIFF.cols as c (c)}<th scope="col">{c}</th>{/each}</tr></thead>
          <tbody>
            {#each fileDiff as r, i (i)}
              <tr><th scope="row">{r.what}</th><td translate="no">{r.here}</td><td translate="no">{r.file}</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}
      <h3 class="modal-h">{NOTICE.fix}</h3>
      <p>{next.sentence}</p>
      {#snippet actions()}
        {#each band.secondary as b (b.label)}
          <button class="button-secondary" data-control={CONTROL_OF[b.action]} aria-disabled={!RUNS.has(b.action)} onclick={() => RUNS.has(b.action) && ((refusalClosed = op.file), act(b.action))}>{b.label}</button>
        {/each}
        {#if band.primary}
          {@const p = band.primary}
          <button class="button-primary" data-control={CONTROL_OF[p.action]} aria-disabled={!RUNS.has(p.action)} onclick={() => RUNS.has(p.action) && ((refusalClosed = op.file), act(p.action))}>{p.label}</button>
        {/if}
      {/snippet}
    </Modal>
  {/if}
  {#if stopOpen && stopNode}
    {@const back = op.progress[stopNode.id]?.stage === 'previous'}
    {@const running = op.live[stopNode.id]?.hello?.activeVersion}
    <Modal title={WRITE.run} chip={{ text: back ? WRITE.previous : WRITE.failed, kind: back ? 'info' : 'error' }} target={`[${stopNode.no}] ${stopNode.name}`} onclose={() => (stopClosed = op.failed)} onmore={() => ((drawerTab = 'log'), (stopClosed = op.failed))}>
      <h3 class="modal-h">{NOTICE.reason}</h3>
      <p>{back && running !== undefined ? fillIn(WRITE.rolled.reason, { version: versionText(running) }) : (next.cause ?? WRITE.failed)}</p>
      <h3 class="modal-h">{NOTICE.fix}</h3>
      <p>{back ? WRITE.rolled.next : next.sentence}</p>
      {#snippet actions()}
        {#each band.secondary as b (b.label)}
          <button class="button-secondary" data-control={CONTROL_OF[b.action]} aria-disabled={!RUNS.has(b.action)} onclick={() => RUNS.has(b.action) && ((stopClosed = op.failed), act(b.action))}>{b.label}</button>
        {/each}
        {#if band.primary}
          {@const p = band.primary}
          <button class="button-primary" data-control={CONTROL_OF[p.action]} aria-disabled={!RUNS.has(p.action)} onclick={() => RUNS.has(p.action) && ((stopClosed = op.failed), act(p.action))}>{p.label}</button>
        {/if}
      {/snippet}
    </Modal>
  {/if}
  {#if buildFailureOpen}
    {@const failed = [buildJob, signJob, releaseJob].find((j) => j?.state === 'failed')}
    <Modal title={BUILD.title} chip={{ text: BUILD.state.failed, kind: 'error' }} target={d.config.reach.map((id) => `nodes/${id}/`).join(' · ')} onclose={() => (buildFailureOpen = false)} onmore={() => ((drawerTab = 'jobs'), (buildFailureOpen = false))}>
      <h3 class="modal-h">{NOTICE.reason}</h3>
      <p class="modal-mono">{jobReason(failed)}</p>
      {#snippet actions()}
        {#if agent.linked}
          <button class="button-secondary" data-control="ask" onclick={(e) => askDagon(BUILD.askAgent, 'failure', e.currentTarget)}>{BUILD.askAgent}</button>
        {/if}
        <button class="button-primary" data-control="build" onclick={() => void build()}>{BUILD.retry}</button>
      {/snippet}
    </Modal>
  {/if}
  {#if skeleton}
    <SkeletonModal
      job={skeletonJob}
      place={cli.hello?.workspace ?? d.doc.topo.name ?? ''}
      nodes={d.config.nodes.filter((n) => d.config.reach.includes(n.id)).map((n) => [`[${n.no}] ${n.name}`, `nodes/${n.id}/`] as [string, string])}
      onclose={() => (skeleton = null)}
      oncancel={() => skeletonJob && void cli.cancel(skeletonJob)}
      onretry={() => void makeSkeleton()}
      onmore={() => ((drawerTab = 'jobs'), (skeleton = null))}
    />
  {/if}
  {#if dialogOpen}
    <Reconnect notes={op.reconnectNotes} onreconnect={() => void reconnectChosen()} onclose={() => (dialogDismissed = true)} />
  {/if}
{/if}

<svelte:window onkeydown={defineKeys} />

<style>
  .modal-h {
    margin: 0;
    font: 700 var(--t-caption) / 16px var(--f-ja);
  }
  .modal-mono {
    margin: 0;
    font: 400 var(--t-mono) / 24px var(--f-mono);
    overflow-wrap: anywhere;
  }
  /* The refusal modal's 構成 differences (the table the sheet had). */
  .modal-diff {
    width: 100%;
    border-collapse: collapse;
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .modal-diff caption {
    margin-bottom: var(--s-2);
    text-align: left;
    font-weight: 700;
  }
  .modal-diff th,
  .modal-diff td {
    padding: var(--s-2) var(--s-3) var(--s-2) 0;
    border-bottom: 1px solid var(--c-line);
    text-align: left;
    vertical-align: top;
  }
  .modal-diff td {
    font-family: var(--f-mono);
    overflow-wrap: anywhere;
  }
  main.define {
    position: relative;
  }
  .zoom {
    position: absolute;
    right: 0.75rem;
    bottom: 0.75rem;
    z-index: var(--z-chrome);
    display: flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--c-hair);
    border-radius: var(--r-full);
    background: var(--c-layer-1);
  }
  .zoom button:not(.button-icon),
  :global(.graph .zoom button:not(.button-icon)) {
    height: 1.5rem;
    min-width: 1.5rem;
    padding: 0 0.5rem;
    border: 0;
    border-radius: var(--r-full);
    background: none;
    color: var(--c-muted);
    font: 400 var(--t-12) / 1 var(--f-ja);
    cursor: pointer;
  }
  :global(.graph .zoom button:not(.button-icon)[aria-checked='true']),
  .zoom button[aria-checked='true'] {
    background: var(--c-paper);
    color: var(--c-ink);
  }
  :global(.graph .zoom .seg),
  .seg {
    display: flex;
    gap: 2px;
  }
  :global(.graph .zoom .pct),
  .pct {
    display: grid;
    place-items: center;
    padding: 0 0.5rem;
    color: var(--c-muted);
    font: 500 var(--t-12) / 1 var(--f-mono);
  }
  @media (hover: hover) and (pointer: fine) {
    :global(.graph .zoom button:not(.button-icon):hover) {
      background: var(--c-layer-3);
      color: var(--c-paper);
    }
  }
  @media (max-width: 767px), (pointer: coarse) {
    :global(.graph .zoom button:not(.button-icon)),
    .tb {
      min-height: 44px;
      min-width: 44px;
    }
  }
  .close-order {
    position: absolute;
    top: 0.375rem;
    right: 0.375rem;
    width: 1.5rem;
    height: 1.5rem;
    border: 0;
    border-radius: var(--r-full);
    background: none;
    color: var(--c-muted);
    cursor: pointer;
  }
  @media (max-width: 767px) {
    :global(.graph .zoom .wide-only),
    .zoom .wide-only {
      display: none;
    }
  }
  .tray-open {
    position: absolute;
    z-index: var(--z-chrome);
    top: 0.75rem;
    left: 0.75rem;
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
  /* The start page: one centred column under the bar, its status line at the bottom. */
  .frame:not(.work) {
    grid-template-areas: 'header' 'help' 'menu' 'canvas' 'right' 'band';
    grid-template-rows: auto auto auto auto minmax(0, 1fr) auto;
    grid-template-columns: minmax(0, 1fr);
  }
  .frame:not(.work) > .left {
    display: none;
  }
  /* Work: the graph is the whole stage; a drawer under it, the band at the bottom. */
  .frame.work {
    grid-template-areas: 'header' 'help' 'menu' 'canvas' 'drawer' 'band';
    grid-template-rows: auto auto auto minmax(0, 1fr) auto auto;
    grid-template-columns: minmax(0, 1fr);
  }
  .left {
    grid-area: left;
    min-height: 0;
    overflow: auto;
    background: var(--c-ink);
    border-right: 1px solid var(--c-line);
  }
  .stage {
    grid-area: canvas;
    position: relative;
    display: grid;
    min-height: 0;
  }
  main {
    grid-area: canvas;
    display: grid;
    min-height: 0;
    background: var(--c-layer-0);
  }
  .stage > main {
    grid-area: auto;
  }
  main:focus {
    outline: none;
  }
  /* Floating windows: over the graph, apart from its edges, never under the focal number. */
  /* PartsWindow and DetailSheet: L1 surfaces with a 1 px frame, no shadow. */
  .float {
    position: absolute;
    z-index: var(--z-chrome);
    top: var(--s-4);
    overflow: auto;
    border: 1px solid var(--c-line);
    border-radius: var(--r-l);
    background: var(--c-layer-1);
  }
  .float.parts {
    left: var(--s-4);
    width: var(--w-parts);
    /* The focal number keeps the bottom-left: the window stops above it. */
    max-height: calc(100% - 9.5rem - 1.5rem);
  }
  .float.sheet {
    right: var(--s-4);
    width: var(--w-sheet);
    max-height: calc(100% - 1.5rem);
  }
  .log-none,
  .log {
    margin: 0;
    padding: var(--s-3) var(--s-5);
    font: 400 var(--t-mono-size) / var(--t-mono-log-lh) var(--f-ja);
  }
  .log {
    list-style: none;
  }
  .log.out {
    font-family: var(--f-mono);
    white-space: pre-wrap;
  }
  .log .k {
    color: var(--c-muted);
  }
  .talk {
    display: grid;
    gap: var(--s-3);
    margin: 0;
    padding: var(--s-3) var(--s-5);
    list-style: none;
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
  }
  .talk p {
    margin: 0;
  }
  .talk .who {
    color: var(--c-muted);
    font-size: var(--t-caption-size);
  }
  .talk .user p {
    color: var(--c-muted);
  }
  .talk .failed {
    color: var(--c-signal);
  }
  .talk .more {
    white-space: pre-wrap;
  }
  .talk figure {
    margin: var(--s-2) 0 0;
  }
  .talk pre {
    margin: var(--s-1) 0 var(--s-2);
    padding: var(--s-2) var(--s-3);
    border: 1px solid var(--c-line);
    border-radius: var(--r-s);
    font: 400 var(--t-mono-size) / var(--t-mono-lh) var(--f-mono);
    overflow-x: auto;
  }
  /* A proposal's diff: added lines in blue, removed ones muted and struck (not red: nothing failed). */
  .talk .add {
    color: var(--c-accent);
  }
  .talk .del {
    color: var(--c-muted);
    text-decoration: line-through;
  }
  .log time {
    color: var(--c-muted);
    font-family: var(--f-mono);
  }
  /* The start screen on phones: one column, top to bottom. */
  .frame.narrow:not(.work) {
    grid-template-areas: 'header' 'help' 'menu' 'band' 'canvas' 'right' 'left' 'drawer';
    grid-template-rows: auto;
    grid-template-columns: minmax(0, 1fr);
    height: auto;
  }
  /* Phones: the same one window; the parts drop from the bar, the sheet rises from the status row. */
  .narrow .float.parts {
    top: 0.5rem;
    left: 0.5rem;
    right: 0.5rem;
    width: auto;
    max-height: min(50vh, calc(100% - 7rem));
  }
  .narrow .float.sheet {
    top: auto;
    left: 0.5rem;
    right: 0.5rem;
    bottom: 0.5rem;
    width: auto;
    max-height: 45vh;
  }
  .why {
    position: fixed;
    z-index: var(--z-announce);
    max-width: min(20rem, calc(100vw - 2 * var(--s-3)));
    padding: var(--s-2) var(--s-3);
    border: 1px solid var(--c-accent);
    border-radius: var(--r-s);
    background: var(--c-layer-2);
    color: var(--c-paper);
    font: 400 var(--t-small-size) / var(--t-small-lh) var(--f-ja);
    box-shadow: 0 0.25rem 1rem rgb(0 0 0 / 0.5);
    pointer-events: none;
    animation: why-in 120ms var(--ease-out);
  }
  .ghost {
    position: fixed;
    z-index: var(--z-announce);
    padding: 0.375rem 0.625rem;
    border: 1px solid var(--c-paper);
    border-radius: var(--r-m);
    background: var(--c-ink);
    font: 400 var(--t-12) / 1.3 var(--f-ja);
    opacity: 0.9;
    pointer-events: none;
  }
  .why.failed {
    border-color: var(--c-signal);
  }
  @keyframes why-in {
    from {
      opacity: 0;
      transform: translateY(-2px) scale(0.97);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .why {
      animation: none;
    }
  }
  /* Q2: while a dialog is open its own button is the one main action. */
  .frame.modal :global(footer .primary) {
    visibility: hidden;
  }
</style>
