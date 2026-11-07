<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { bandOf, connectNotice, prepareSentence } from './app/band.ts';
  import { AGENT, ANNOUNCE, BAND, BUILD, BUTTON, CANVAS, DEFINE, DIFF, DRAWER, JOBS, SKELETON, WRITE, HUB, LOCK, SHEET, STATUS, ZOOM, HUB_TOGGLE, MODE, NODE, PRODUCT, RECONNECT, SAVE, SKIP, STEP, TIME } from './app/copy.ts';
  import { webSerial } from './app/connect.ts';
  import { fade, rise, tokenMs } from './app/motion.ts';
  import { resultChip } from './app/nodeState.ts';
  import { HUB_LOCKED, hubOf, view, type Action } from './app/machine.ts';
  import { Operate, waterOf, writingTo, type Progress, type ReleaseFile } from './app/operate.svelte.ts';
  import { keptChoices, type Kept } from './app/kept.ts';
  import { previousVersion } from './app/slots.ts';
  import { buildFailure, buildingNow, buildStateOf, firstWriteNodes, type BuildingMemo, type BuildState } from './app/jobFlow.ts';
  import { logStages } from './app/stageLog.svelte.ts';
  import Graph, { COMPACT_FROM, newView, ZOOM_LIMITS, type Chip, type View } from './graph/Graph.svelte';
  import Focal, { type Dot } from './screens/Focal.svelte';
  import Icon from './screens/Icon.svelte';
  import SplitButton from './screens/SplitButton.svelte';
  import StopNotice from './screens/StopNotice.svelte';
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
  import Menu, { type MenuGroup, type MenuItem } from './screens/Menu.svelte';
  import Restart from './screens/Restart.svelte';
  import Unsaved from './screens/Unsaved.svelte';
  import { Autosave } from './app/define/autosave.svelte.ts';
  import StartScreen from './screens/Start.svelte';
  import Inspector from './screens/define/Inspector.svelte';
  import NodeList, { type Part } from './screens/define/NodeList.svelte';
  import Problems from './screens/define/Problems.svelte';
  import Table from './screens/define/Table.svelte';
  import CliPanel from './screens/define/CliPanel.svelte';
  import { Cli, jobReason } from './app/define/cli.svelte.ts';
  import { LayoutFile } from './app/define/layoutFile.ts';
  import { CLI_LINK, CLI_STATE } from './app/copy.ts';
  import Reconnect from './screens/Reconnect.svelte';
  import Header from './screens/Header.svelte';
  import ZoomPill from './screens/ZoomPill.svelte';
  import SkeletonModal from './screens/SkeletonModal.svelte';
  import Modal from './screens/Modal.svelte';
  import StopModal from './screens/StopModal.svelte';
  import FactoryModal from './screens/FactoryModal.svelte';
  import RomConfirm from './screens/RomConfirm.svelte';
  import { answerConsent } from './app/factory.ts';
  import Toast, { type Notice } from './screens/Toast.svelte';
  import { CONFIRM, CONN, FILE, HELP, LAYOUT, MENU, NOTICE, SHEET_CLOSE, SIM, START, TOPOLOGY_READ, fill as fillIn } from './app/copy.ts';
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
  const demoStandIn = import.meta.env.DEV && params.has('demo');
  /** The 親機 and 子機 are simulated: `dagon demo` through serve, or the demo page's own simulator. */
  const simulated = $derived(cli.sim !== null || DEMO_SITE || demoStandIn);
  /** One condition closes every path to linking, talking, keys and the CLI sheet: linked is never true in a demo. */
  const agentLinked = $derived(!simulated && agent.linked);
  /** The 定義 document: the project's 構成 as edited (its YAML is the one source of truth). */
  const d = new Define();
  /** DAGON's link to its agent through serve (development: `?agent` stands in for serve's routes). */
  // Development builds only, written so every other build (the served page, the demo page) leaves agentMock out.
  const agentStandIn = import.meta.env.DEV && dev && params.has('agent');
  const agent = new Agent(import.meta.env.DEV && agentStandIn ? agentMock(params, () => d.config.yaml) : (m, p, b) => cli.api(m, p, b));
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
    const yaml = await cli.loadTopology().catch(() => undefined);
    // 呼び名 and positions live beside the YAML (GET /v1/layout).
    if (!(await layoutFile.open())) notice = { word: LAYOUT.refused, error: true, detail: `${LAYOUT.refusedDetail}（${cli.layoutRefused}）` };
    local = false;
    op.reset();
    d.reset();
    if (yaml) {
      try {
        d.load(fromYaml(yaml, layoutFile.current.names ?? {}), yaml);
      } catch {
        d.unreadable = true;
        d.notRead = true;
      }
    } else if (yaml === undefined) {
      d.notRead = true;
      notice = { word: TOPOLOGY_READ.label, error: true, detail: TOPOLOGY_READ.detail };
    }
    layoutNames = JSON.stringify(d.doc.names);
    positions = layoutFile.current.positions ?? {};
    canvasView = newView();
    enterWork(want);
  }

  /** The project's layout file; names follow the edits. */
  const layoutFile = new LayoutFile(cli, (read) => {
    d.adoptNames(read.names ?? {});
    layoutNames = JSON.stringify(d.doc.names);
    positions = { ...read.positions, ...positions };
  });
  let layoutNames = '';
  $effect(() => {
    const names = JSON.stringify(d.doc.names);
    // With no 構成 read, the editor knows none of the file's nodes: its names say nothing about them.
    if (screen !== 'work' || local || d.notRead) return;
    untrack(() => {
      if (names === layoutNames) return;
      layoutNames = names;
      void layoutFile.change(() => ({ names: { ...d.doc.names } }));
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
    positions = {};
    canvasView = newView();
    if (c === 'sample') d.load(fromYaml(SAMPLE_YAML, Object.fromEntries(SAMPLE_CONFIG.nodes.map((n) => [n.id, n.name]))), SAMPLE_YAML);
    enterWork('define');
  }

  /** The menu's popover, and its one confirmation. */
  let menuOpen = $state(false);
  let helpOpen = $state(false);
  let helpKeys = $state(false);
  let restartOpen = $state(false);
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
    void tick().then(() => document.querySelector<HTMLElement>('[aria-controls="menu"]')?.focus());
  }

  /** 運用 pressed while it cannot be entered: M69 (or 未保存) in the band until the 構成 changes. */
  let locked = $state<{ why: Operable | 'unsaved'; at: number } | null>(null);
  const lockShown = $derived(locked && locked.at === d.revision ? locked.why : null);
  function lock(why: Operable | 'unsaved') {
    locked = { why, at: d.revision };
  }
  const cliLabel = $derived(cli.state === 'ready' ? CLI_STATE.ready(cli.hello?.cli ?? '') : CLI_STATE[cli.state]);
  let cliPanel = $state<'link' | null>(null);
  const runningJobs = $derived(cli.jobs.filter((j) => j.state === 'running' || j.state === 'queued' || j.state === 'question').length);
  $effect(() => {
    if (d.selected) cliPanel = null;
  });

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

  /** Development stand-ins (?mock, ?sim without serve): an unsaved 構成 on the page, nothing to save to. */
  let local = $state(false);
  /** Edits go to the project's YAML by themselves; 保存 saves at once. */
  const autosave = new Autosave(d, {
    put: (yaml) => cli.saveTopology(yaml),
    enabled: () => screen === 'work' && !local,
    blocked: () => cli.defsFailed || d.notRead,
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

  const fileDiff = $derived(op.state === 'otherTopology' && op.fileTopology ? topoDiff(d.doc.topo, op.fileTopology as unknown as Topo, d.doc.names) : []);

  /** The canvas's zoom and pan, the user's: kept across modes and views, new for each project. */
  let canvasView = $state<View>(newView());
  /** Where the user put cards, kept in the project's layout file. */
  let positions = $state.raw<Record<string, { x: number; y: number }>>({});
  function savePositions() {
    if (local) return;
    void layoutFile.change(() => ({ positions }));
  }
  let tableTab = $state(0);
  /** The free part of the canvas (between the floating columns): what a fit fills. */
  let midEl = $state<HTMLElement>();

  /** The linking dialog, and requests to open DAGON's bubble. */
  let linkOpen = $state(false);
  let dagonOpen = $state(0);
  async function openLink(at?: Element | null) {
    if (simulated) return;
    if (stopped('link', at)) return;
    if (!agent.status) await agent.refresh();
    if (!agent.status) return showWhy(AGENT.errors.OLD_CLI.text, at);
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
  function askWhy(at: Element, close: () => void) {
    void agent.askWhy(() => stopped('ask', at), close, selection());
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
    ...(simulated ? [] : [{ id: 'talk', label: DRAWER.talk }]),
    { id: 'log', label: DRAWER.log },
  ]);
  /** serve names the nodes that can be built; a node without one has no skeleton yet. */
  const needsSkeleton = $derived(!!cli.hello?.build_nodes && d.config.reach.some((id) => !cli.hello?.build_nodes?.includes(id)));
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
    return { sentence: prepareSentence({ needsSkeleton, building: buildState === 'running', built: builtRevision === d.revision }) };
  });
  const savedLabel = $derived(local ? '' : d.unsaved ? SAVE.unsaved : autosave.pending ? STATUS.saving : STATUS.saved);
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

  /** Ctrl+Z / Ctrl+Shift+Z anywhere but in a field; Delete and L on the canvas; Ctrl+J the drawer. */
  function defineKeys(e: KeyboardEvent) {
    if (screen === 'work' && e.key.toLowerCase() === 'j' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      return pickTab(drawerTab ?? 'problems');
    }
    const t = e.target as HTMLElement;
    if (screen === 'work' && (agentLinked || simulated) && e.key === '/' && !/^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName)) {
      e.preventDefault();
      return void dagonOpen++;
    }
    // Escape closes the top thing only: a dialog, the version list or a notice take it first; then the bubble, the menu, the help, the sheet.
    if (screen === 'work' && e.key === 'Escape' && bubbleOpen && !document.querySelector('dialog[open], .split .list, [data-stop-notice]')) {
      e.preventDefault();
      return void (bubbleOpen = false);
    }
    if (screen === 'work' && e.key === 'Escape' && !menuOpen && !helpOpen && !document.querySelector('dialog[open], .split .list, [data-stop-notice]')) {
      if (why) why = null;
      return closeSheet();
    }
    if (screen === 'work' && !/^(INPUT|SELECT|TEXTAREA)$/.test(t.tagName)) {
      if (e.key.toLowerCase() === 'f' && !e.ctrlKey && !e.metaKey) return graph?.fit();
      if ((e.ctrlKey || e.metaKey) && (e.key === '+' || e.key === '=' || e.key === '-')) {
        e.preventDefault();
        return graph?.zoomBy(e.key === '-' ? 1 / ZOOM_LIMITS.step : ZOOM_LIMITS.step);
      }
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
      d.requestRemove();
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

  let genEl = $state<HTMLButtonElement>();
  let writeEl = $state<HTMLButtonElement>();
  /** The mode's main action, the one the band's 次へ focuses. */
  const primaryEl = $derived(mode === 'define' ? genEl : writeEl);
  let partsOpen = $state(true);
  let nextEl = $state<HTMLElement>();

  /** Buttons whose procedures this page runs; the others show as unavailable. */
  const RUNS: ReadonlySet<Action> = new Set([
    'connectDiscover', 'connect', 'reselect', 'reconnect', 'push', 'pushAgain', 'scanAgain', 'reload',
    'chooseFile', 'updateResponding', 'otherFile', 'update', 'useLatest', 'recover', 'recoverAgain', 'defineFix',
  ]);

  /** States 書き込む writes in directly: the hub answers and nothing runs. */
  const WRITABLE: ReadonlySet<string> = new Set(['ready', 'nodesSilent', 'done']);
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
    coreBack: WRITE.coreBack.label,
    unwritten: NODE.unwritten,
    unchanged: NODE.unchanged,
  };
  /** Stages in which a card shows → the new version and its one %. */
  const FILLING_STAGES: ReadonlySet<Progress['stage']> = new Set(['writing', 'lastResort', 'rebooting']);
  const updating = $derived(UPDATING.has(op.state));
  const diffChips = $derived(Object.fromEntries(fileDiff.flatMap((r) => r.nodes).map((id) => [id, { chip: DIFF.chip, spoken: DIFF.spoken, silent: true }])));
  /** On the cards while writing: → the new version; after it, what the write left (resultChip). */
  const cardChipOf = (id: string, p: Progress): Chip | null => {
    const to = op.fileVersions[id];
    if (FILLING_STAGES.has(p.stage)) {
      const said = writingTo(p, to);
      return said === null ? null : { chip: said, spoken: said, silent: false };
    }
    return resultChip(p.stage);
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
  /** Serve would not take one of the build's jobs. */
  let buildStartFailed = $state(false);
  const buildJobs = $derived({ build: buildJob, sign: signJob, release: releaseJob });
  const buildState = $derived(buildStateOf(buildJobs, buildStartFailed));
  async function build() {
    if (stopped('build') || !cli.has('build')) return;
    buildFailureOpen = false;
    buildStartFailed = false;
    buildIds = { build: null, sign: null, release: null };
    try {
      await autosave.whenSaved(async () => {
        const j = await cli.build(d.config.reach);
        buildIds = { build: j?.id ?? null, sign: null, release: null };
      });
    } catch {
      buildStartFailed = true;
    }
  }
  /** Starts the next step of the build; a step serve would not take ends it as a failure. */
  function buildStep(step: 'sign' | 'release', start: () => Promise<{ id: number } | null>) {
    buildIds = { ...buildIds, [step]: -1 };
    void start()
      .then((j) => {
        if (!j) buildStartFailed = true;
        buildIds = { ...buildIds, [step]: j?.id ?? -1 };
      })
      .catch(() => (buildStartFailed = true));
  }
  let buildAnnounced: BuildState = null;
  $effect(() => {
    const s = buildState;
    // Each step's end starts the next while the state stays "running".
    const [built, signed] = [buildJob?.state, signJob?.state];
    untrack(() => {
      if (built === 'succeeded' && buildIds.sign === null) buildStep('sign', () => cli.sign(d.config.reach));
      if (signed === 'succeeded' && buildIds.release === null) buildStep('release', () => cli.release());
      if (s === buildAnnounced) return;
      buildAnnounced = s;
      if (s === 'done') {
        notice = { word: fillIn(BUILD.toast.done, { n: d.config.reach.length }) };
        builtRevision = d.revision;
        void loadVersions();
      }
      if (s === 'failed') buildFailureOpen = true;
    });
  });
  /** While building, only the node cargo builds now carries the edge light; it goes when the next is named or the job ends. */
  let buildingMemo: BuildingMemo = { job: null, node: null };
  const building = $derived.by((): readonly string[] => {
    buildingMemo = buildingNow(buildJobs, d.config.reach, buildingMemo);
    return buildingMemo.node ? [buildingMemo.node] : [];
  });
  /** The 構成 as it was when a build last finished: while it stays so, the next step is to write. */
  let builtRevision = $state<number | null>(null);
  const chips = $derived(updating ? progressChips : diffChips);
  /** The water in each node while it is written (writing, hubRestarting, recovering). */
  const fill = $derived(PERCENT.has(op.state) ? Object.fromEntries(Object.keys(waterOf(op.progress)).map((id) => [id, op.percentOf(id, nowMs)])) : {});
  // The comparison stays while the update runs (P14: never silently replaced).
  /** The refusal modal: why the package cannot be written and what to do; closing leaves the band's buttons. */
  let refusalClosed = $state<ReleaseFile | null>(null);
  const refusalOpen = $derived(op.file !== null && (op.state === 'fileRefused' || op.state === 'otherTopology') && refusalClosed !== op.file);
  /** A connection the user must fix: its next step, until closed (or the state moves on). */
  let connectClosed = $state(false);
  const connectModal = $derived(connectNotice(op.state) === 'modal' && !connectClosed);
  $effect(() => {
    void op.state;
    connectClosed = false;
  });
  /** A write that stopped at a node (back on its previous version, or not written): what happened and what to do, until closed. */
  let stopClosed = $state<string | null>(null);
  const stopNode = $derived(op.state === 'stopped' && op.failed ? (op.config?.nodes.find((n) => n.id === op.failed) ?? null) : null);
  const stopOpen = $derived(stopNode !== null && stopClosed !== op.failed);
  /** What the node or the link said, in the user's words: the cause, then what to do (the band's sentence when nothing more is known). */
  const failWhy = $derived.by(() => {
    const p = op.failed ? op.progress[op.failed] : undefined;
    const w = (p?.reason && WRITE.why[p.reason]) || (p?.stage && WRITE.why[p.stage]);
    return w ?? { reason: next.cause ?? WRITE.failed, fix: next.sentence };
  });
  $effect(() => {
    if (op.state !== 'stopped') stopClosed = null;
  });
  const refusalWord = (v: ReleaseFile['verdict']) => (v === 'topology' ? FILE.topologyBad : v === 'format' ? FILE.notRelease : FILE.sourceBad);

  let fileInput = $state<HTMLInputElement>();
  async function picked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const f = input.files?.[0];
    input.value = '';
    if (f && stopped(pickForRecovery ? 'recover' : 'file')) return void (pickForRecovery = null);
    const forRecovery = pickForRecovery;
    pickForRecovery = null;
    if (!f) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    if (forRecovery) void writeOne(forRecovery.id, { name: f.name, bytes });
    else op.takeFile(f.name, bytes);
  }

  /** The file chooser is open for a one-node write that found no release. */
  let pickForRecovery: { id?: string } | null = null;
  /** The silent node whose ROM write waits for the user's word. */
  let romAsk = $state<string | null>(null);
  /** 「書き込む」 on one node: the way it answers decides what comes next. */
  async function writeOne(id?: string, chosen?: { name: string; bytes: Uint8Array }, again = false) {
    if (stopped('recover')) return;
    const r = await (again ? op.retry() : op.writeNode({ id, chosen }));
    const node = id ?? op.recoverTarget?.id ?? null;
    if (r === 'needFile') {
      pickForRecovery = { id };
      fileInput?.click();
    } else if (r === 'direct' && node) factory = { node, id: null };
    else if (r === 'confirm') romAsk = node;
  }
  function writeRom() {
    const id = romAsk;
    romAsk = null;
    if (id && !stopped('recover')) void op.writeNode({ id, confirmed: true });
  }
  const romNode = $derived(op.config?.nodes.find((n) => n.id === romAsk) ?? null);
  /** The bar's width: under this the edit tools would meet the actions, so they live in ☰ alone (its items are always there). */
  const BAR_TOOLS_MIN = 1100;
  let barNarrow = $state(false);
  $effect(() => {
    const bar = document.querySelector('.frame > header');
    if (!bar) return;
    const look = () => (barNarrow = bar.getBoundingClientRect().width < BAR_TOOLS_MIN);
    look();
    const ro = new ResizeObserver(look);
    ro.observe(bar);
    return () => ro.disconnect();
  });
  /** ×, Escape or the canvas background: the sheet closes (the selection goes). */
  function closeSheet() {
    if (mode === 'define') {
      cliPanel = null;
      d.selected = null;
    } else op.select(null);
  }
  /** A press on the canvas's empty part that does not move (a pan is not a click) closes the sheet. */
  let pressAt: { x: number; y: number } | null = null;
  function canvasDown(e: PointerEvent) {
    pressAt = { x: e.clientX, y: e.clientY };
  }
  function canvasPress(e: MouseEvent) {
    const el = e.target as HTMLElement;
    const moved = !pressAt || Math.hypot(e.clientX - pressAt.x, e.clientY - pressAt.y) > DRAG_PX;
    pressAt = null;
    if (moved || el.closest('button, .port, .link-chip, a, input')) return;
    if (el.closest('.box') && !el.closest('.node')) closeSheet();
  }
  /** Every destructive action asks through this one modal: what, why, then the red button. */
  let ask = $state<{ title: string; sentence: string; label: string; run: () => void } | null>(null);
  /** The DAGON bubble is open: Escape closes it first, wherever focus is. */
  let bubbleOpen = $state(false);
  const confirmThen = (c: { title: string; sentence: string; label: string }, run: () => void) => (ask = { ...c, run });
  /** The free part of the canvas a fit fills: the middle column less the room a toast stands in, so a notice never covers a card of the fit view. */
  const TOAST_ROOM = 64;
  const fitArea = () => {
    const r = midEl?.getBoundingClientRect();
    return r && new DOMRect(r.left, r.top, r.width, Math.max(0, r.height - TOAST_ROOM));
  };
  /** The whole middle column: the fit falls back to it when the toast's room alone would make the cards compact. */
  const wholeArea = () => midEl?.getBoundingClientRect();
  /** The sheet shows only when it holds something. */
  const sheetShown = $derived(
    mode === 'define'
      ? cliPanel !== null || d.selected !== null
      : !op.config || op.selected !== null || (!!op.recovering && !!op.recovered),
  );
  /** Node actions in the detail: while watching, with nothing running. */
  const nodeActions = $derived(['ready', 'nodesSilent', 'done'].includes(op.state));

  /** The reconnect dialog only after the automatic reconnect failed (reconnectNeeded); Escape leaves the same button in the band. */
  let dialogDismissed = $state(false);
  const dialogOpen = $derived(op.state === 'reconnectNeeded' && !dialogDismissed);
  $effect(() => {
    if (op.state !== 'reconnectNeeded') dialogDismissed = false;
  });
  /** Clock for the %, M27 (dagon-host's clock) and the elapsed time (wall clock), read while they show; the % rises as often. */
  let nowMs = $state(0);
  let wallMs = $state(Date.now());
  $effect(() => {
    if (!PERCENT.has(op.state)) return;
    const tick = () => {
      nowMs = op.now();
      wallMs = Date.now();
    };
    tick();
    const t = setInterval(tick, tokenMs('--dur-tick'));
    return () => clearInterval(t);
  });
  /** The timeline: each node's stage changes since the update began; it opens with the update. */
  let tlEvents = $state<StageEvent[]>([]);
  let tlStart = 0;
  logStages(op, {
    begin: (start) => {
      tlStart = start;
      tlEvents = [];
    },
    add: (events) => (tlEvents = [...tlEvents, ...events]),
  });
  const tlNow = $derived(tlStart && PERCENT.has(op.state) ? Math.max(0, wallMs - tlStart) : Math.max(0, ...tlEvents.map((e) => e.t)));
  /** 書込's version picker: the latest build (the project's newest release) first, then the kept packages; writing an older one is going back. */
  let latest = $state<{ bytes: Uint8Array; label: string } | null>(null);
  let kept = $state<Kept[]>([]);
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
  }
  const versionChoices = $derived([
    ...(latest ? [{ id: 'latest', label: latest.label }] : []),
    ...keptChoices(kept).filter((c) => c.label !== latest?.label),
  ]);
  $effect(() => {
    // Again on entering 書込 and after each write (the written package was kept).
    if (mode === 'operate' && (op.state === 'ready' || op.state === 'done')) untrack(() => void loadVersions());
  });
  /** 書き込む while watching: the version (the latest build unless one was chosen), checked against the 構成, to the checked cards. */
  async function writeVersion(id = latest ? 'latest' : (kept[0]?.id ?? 'latest')) {
    if (stopped('update')) return;
    const pkg = id === 'latest' ? (latest ? { name: `${latest.label}.dgb`, bytes: latest.bytes } : null) : await cli.keptBytes(id).then((b) => (b ? { name: `${id}.dgb`, bytes: b } : null));
    if (!pkg) {
      notice = { word: BAND.M58.chip, error: true, detail: BAND.M58.sentence };
      return;
    }
    const no = await op.noWrite(pkg.bytes);
    if (no === 'noneChecked') return void (notice = { word: WRITE.noneChecked.label, detail: WRITE.noneChecked.detail });
    if (no) {
      const said = WRITE.alreadyOn(versionText(no.same), no.coreToo);
      return void (notice = { word: said.label, detail: said.detail });
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
    addEventListener('pointerover', lockHint, true);
    addEventListener('focusin', lockHint, true);
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
  agent.record = () => op.settledRollbackRecord();
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
      // Seen where the button is, in either mode; the status row's sentence is for screen readers.
      showWhy(hubLocked, document.querySelector('header [data-control="connect"]'));
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
      stopped: op.failed ? op.progress[op.failed]?.stage : undefined,
      rest: op.rest && op.state === 'fileReady',
      finish: op.finished,
      refused: op.returnRefused?.kind,
      sim: simulated,
      built: latest !== null || kept.length > 0,
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
      ? // Before a project opens there is no 構成 to read, and dagon-wasm may still be loading.
        screen === 'work'
        ? defineNext
        : { sentence: '' }
      : { cause: band.chip, sentence: band.sentence ?? '' },
  );
  const dot = $derived.by((): Dot | null => {
    const c = op.config;
    if (!c || !connected || op.state === 'checking') return null;
    if (PERCENT.has(op.state)) {
      const halted = op.state === 'stopped';
      const overall = op.overallAt(nowMs);
      const progress = { label: CANVAS.dotProgress, value: overall, unit: halted ? CANVAS.dotStopped : CANVAS.dotPercent, name: CANVAS.dotProgressName(overall, halted) };
      if (op.startedAt === null) return progress;
      // 経過 under the number: the one time that is measured, not guessed.
      const gone = Math.max(0, wallMs - op.startedAt);
      return { ...progress, sub: [TIME.elapsed, TIME.clock(gone)] as const };
    }
    if (op.state === 'scanning' || op.state === 'scanned') return { label: CANVAS.dotFound, value: op.found, name: CANVAS.dotFoundName(op.found) };
    // Out of the nodes updating reaches: the others are never watched.
    const n = op.reach.length;
    return { label: CANVAS.dotResponding, value: op.responding, of: n, name: CANVAS.dotRespondingName(op.responding, n) };
  });

  /** What the one graph shows in each mode. */
  const graphProps = $derived(
    mode === 'define'
      ? {
          config: d.doc.topo.nodes.length ? d.config : null,
          live: {},
          connected: false,
          sending: false,
          selected: d.selected,
          onselect: (k: string) => (d.selected = d.selected === k ? null : k),
          chips: defineChips,
          dropOn,
          canLink: (from: string, to: string) => d.linkRefusal(from, to),
          onlink: (from: string, to: string) => !stopped('edit') && d.link(to, from),
          onrefuse: refuse,
        }
      : {
          config: op.config,
          live: op.live,
          connected,
          sending: op.state === 'sending',
          selected: op.selected,
          onselect: (k: string) => op.select(op.selected === k ? null : k),
          chips,
          fill,
          checks: Object.fromEntries(op.reach.map((n) => [n.id, !op.skip.has(n.id)])),
          oncheck: (id: string) => op.toggleSkip(id),
          checksLocked: updating && !['done', 'recoverFailed'].includes(op.state),
          confirmedOf: (id: string) => op.progress[id]?.stage === 'done',
        },
  );

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

  /** 記録 and the page's address, to paste elsewhere; a refused clipboard says so. */
  async function copyText(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      notice = { word: MENU.copied };
    } catch {
      notice = { word: MENU.copied.replace('した', 'できない'), error: true };
    }
  }
  const selectedNode = $derived(op.selected?.startsWith('node:') ? op.selected.slice(5) : null);
  const lastProposal = $derived(agent.messages.map((m) => !!m.proposal && !m.applied).lastIndexOf(true));
  const idle = $derived(WRITABLE.has(op.state));
  /** The ☰ menu: only operations that run here; a group's items by how often they are used. */
  const menuGroups = $derived<MenuGroup[]>([
    {
      label: MENU.groups.project,
      items: [
        { label: START.new, hint: MENU.hint.new, control: 'project', off: modeLocked ? MENU.off.notIdle : null, run: () => toStart('new') },
        { label: START.open, hint: MENU.hint.open, control: DEMO_SITE ? 'open' : 'project', off: modeLocked ? MENU.off.notIdle : null, run: () => toStart('open') },
        { label: MENU.save, hint: MENU.hint.save, off: local || screen !== 'work' ? MENU.off.noProject : null, run: () => void save() },
        { label: MENU.restart, hint: MENU.hint.restart, control: 'project', danger: true, off: modeLocked ? MENU.off.notIdle : null, run: () => (restartOpen = true) },
      ],
    },
    {
      label: MENU.groups.define,
      items: [
        { label: DEFINE.undo, hint: MENU.hint.undo, key: 'Ctrl+Z', control: 'edit', off: mode !== 'define' ? MENU.off.notDefine : d.canUndo ? null : MENU.off.noUndo, run: () => d.undo() },
        { label: DEFINE.redo, hint: MENU.hint.redo, key: 'Ctrl+Shift+Z', control: 'edit', off: mode !== 'define' ? MENU.off.notDefine : d.canRedo ? null : MENU.off.noRedo, run: () => d.redo() },
        { label: PARTS.open, hint: MENU.hint.parts, off: mode !== 'define' ? MENU.off.notDefine : partsOpen ? MENU.off.partsOpen : null, run: () => (partsOpen = true) },
        { label: MENU.arrange, hint: MENU.hint.arrange, off: Object.keys(positions).length ? null : MENU.off.arranged, run: () => ((positions = {}), savePositions()) },
        // The view toggle leaves the menu while the header holds it.
        ...(barNarrow ? [{ label: d.view === 'graph' ? MENU.viewTable : MENU.viewGraph, hint: d.view === 'graph' ? MENU.hint.viewTable : MENU.hint.viewGraph, off: mode !== 'define' ? MENU.off.notDefine : null, run: () => (d.view = d.view === 'graph' ? 'table' : 'graph') }] : []),
        { label: ZOOM.fit, hint: MENU.hint.fit, key: 'F', off: mode === 'define' && d.view !== 'graph' ? MENU.off.notGraph : null, run: () => graph?.fit() },
      ],
    },
    {
      label: MENU.groups.connect,
      items: [
        { label: connected ? HUB_TOGGLE.disconnect : HUB_TOGGLE.connect, hint: connected ? MENU.hint.disconnect : MENU.hint.connect, control: 'connect', off: hubLocked, run: toggleHub },
        { label: MENU.place, hint: MENU.hint.place, control: 'file', off: !connected ? MENU.off.notConnected : idle ? null : MENU.off.notIdle, run: () => fileInput?.click() },
        { label: MENU.rollback, hint: MENU.hint.rollback, control: 'rollback', off: !connected ? MENU.off.notConnected : !idle ? MENU.off.notIdle : selectedNode ? null : MENU.off.noNode, run: () => selectedNode && askRollback(selectedNode) },
      ],
    },
    {
      label: MENU.groups.dagon,
      items: [
        ...(agentLinked
          ? [
              { label: AGENT.menuTalk, hint: MENU.hint.talk, control: 'ask', run: () => void dagonOpen++ },
              { label: MENU.applyProposal, hint: MENU.hint.apply, control: 'apply', off: lastProposal < 0 ? MENU.off.noProposal : null, run: () => applyProposal(lastProposal, document.body) },
            ]
          : [{ label: AGENT.menuLink, hint: MENU.hint.link, control: 'link', off: simulated ? MENU.off.demo : null, run: () => void openLink(document.querySelector('[aria-controls="menu"]')) }]),
        { label: MENU.cli, hint: MENU.hint.cli, off: simulated ? MENU.off.demo : null, run: openCli },
        ...(agentLinked ? [{ label: AGENT.menuUnlink, hint: MENU.hint.unlink, danger: true, run: () => confirmThen(CONFIRM.unlink, () => void unlink()) }] : []),
      ],
    },
    {
      label: MENU.groups.help,
      items: [
        { label: HELP.heading, hint: MENU.hint.help, run: () => ((helpKeys = false), (helpOpen = true)) },
        { label: MENU.keys, hint: MENU.hint.keys, run: () => ((helpKeys = true), (helpOpen = true)) },
        { label: MENU.copyLog, hint: MENU.hint.copyLog, off: notes.length ? null : MENU.off.noLog, run: () => void copyText(notes.map((n) => `${n.at} ${n.text}`).join('\n')) },
        { label: MENU.copyUrl, hint: MENU.hint.copyUrl, run: () => void copyText(location.href) },
      ],
    },
  ]);
  /** 前のバージョンに戻す, from the menu or the sheet: asks first, like every change that cannot be undone. */
  function askRollback(id: string) {
    const n = op.config?.nodes.find((x) => x.id === id);
    const v = previousVersion(op.live[id]?.status);
    confirmThen(CONFIRM.rollback(n ? `[${n.no}] ${n.name}` : id, v ?? ''), () => void op.rollback(id));
  }
  function openCli() {
    if (simulated) return;
    if (mode !== 'define') switchMode('define');
    d.selected = null;
    cliPanel = 'link';
  }

  /** 雛形を作る: the modal is open while `skeleton` is set; `id` is serve's job once it has taken it. */
  let skeleton = $state<{ id: number | null; failure?: string } | null>(null);
  const skeletonJob = $derived(skeleton?.id == null ? null : (cli.jobs.find((j) => j.id === skeleton?.id) ?? null));
  // 中止: the modal goes with the job.
  $effect(() => {
    if (skeletonJob?.state === 'cancelled') untrack(() => (skeleton = null));
    if (skeletonJob?.state === 'succeeded') untrack(() => void cli.refreshHello());
  });
  async function makeSkeleton() {
    skeleton = { id: null };
    let job: Awaited<ReturnType<typeof cli.gen>> = null;
    try {
      // Made from the saved YAML: after the saves queued before it.
      await autosave.whenSaved(async () => (job = await cli.gen(d.config.reach)));
      skeleton = job ? { id: (job as { id: number }).id } : null;
    } catch {
      skeleton = { id: null, failure: SKELETON.startFailed };
    }
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
    if (op.state !== 'done') return;
    untrack(() => {
      const f = op.finished;
      const wrote = f?.kind === 'updated' ? (f.wrote ?? op.targets.length) : op.targets.length;
      notice = { word: wrote ? fillIn(WRITE.toast.done, { n: wrote }) : WRITE.toast.none };
    });
  });
  // A consent of a version this dagon does not read: a failure the user must notice.
  $effect(() => {
    const p = consentProblem(agent.status);
    const why = agent.status?.consent_error ?? '';
    if (p) untrack(() => (notice = { word: p.label ?? p.text, error: true, detail: `${p.text}（${why}）` }));
  });
  // A failure or warning the user must notice: its short word on a toast, the sentence behind it.
  $effect(() => {
    // 準備 has its own sentence, but a connection that failed there is told all the same.
    const bad = connectNotice(op.state);
    const cause = bad === 'toast' ? band.chip : next.cause;
    const detail = bad === 'toast' ? band.sentence : next.sentence;
    // What needs the user opens its modal instead.
    if (cause && bad !== 'modal' && !untrack(() => refusalOpen || stopNode !== null)) untrack(() => (notice = { word: cause, error: cause.startsWith('×'), detail }));
  });

  /** 書き込む: the 親機 answered nothing (silentHub) → writing directly over USB; otherwise the next step of writing through DAGON. */
  const writeAction = $derived(
    op.state === 'silentHub' ? null : op.state === 'ready' && !op.latestMissing && band.secondary.some((b) => b.action === 'useLatest') ? 'useLatest' : (band.primary?.action ?? null),
  );
  const writeControl = $derived(WRITABLE.has(op.state) ? 'update' : writeAction ? CONTROL_OF[writeAction] : 'flash');
  const writeOff = $derived(op.state === 'choosing' || (op.state !== 'silentHub' && !WRITABLE.has(op.state) && (writeAction === null || !RUNS.has(writeAction))));
  function write() {
    if (writeOff) return;
    if (WRITABLE.has(op.state)) return void writeVersion();
    if (writeAction === null) return void (factory = hubId ? { node: hubId, id: null } : null);
    act(writeAction);
  }

  /** Writing directly over USB to the 親機 whose DAGON answered nothing (the hub first: it reaches the rest). */
  let factory = $state<{ node: string; id: number | null } | null>(null);
  const hubId = $derived((hubNode ?? op.config?.nodes.find((n) => n.role === 'hub'))?.id ?? null);
  const factoryNode = $derived(op.config?.nodes.find((n) => n.id === factory?.node) ?? null);
  /** Nodes whose first write can start from their card: not in dagon demo, whose nodes are simulated. */
  const firstNodes = $derived(mode === 'operate' && !cli.sim && cli.has('flash') ? firstWriteNodes(op.config?.nodes ?? [], op.live, connected, op.state) : []);
  const factoryMcu = $derived(d.doc.topo.nodes.find((n) => n.id === factoryNode?.id)?.mcu ?? '');
  const factoryJob = $derived(factory?.id == null ? null : (cli.jobs.find((j) => j.id === factory?.id) ?? null));
  async function factoryWrite() {
    if (!factoryNode || stopped('flash')) return;
    const node = factoryNode.id;
    const j = await cli.flash(node).catch(() => {
      notice = { word: WRITE.failed, error: true, detail: BUILD.startFailed };
      return null;
    });
    factory = { node, id: j?.id ?? null };
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
        return void writeOne();
      case 'recoverAgain':
        return void writeOne(undefined, undefined, true);
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

{#snippet stopActions()}
  {#each band.secondary as b (b.label)}
    <button class="button-secondary" data-control={CONTROL_OF[b.action]} aria-disabled={!RUNS.has(b.action)} onclick={() => RUNS.has(b.action) && ((stopClosed = op.failed), act(b.action))}>{b.label}</button>
  {/each}
  {#if band.primary}
    {@const p = band.primary}
    <button class="button-primary" data-control={CONTROL_OF[p.action]} aria-disabled={!RUNS.has(p.action)} onclick={() => RUNS.has(p.action) && ((stopClosed = op.failed), act(p.action))}>{p.label}</button>
  {/if}
{/snippet}

{#snippet defineRight()}
  {#if cliPanel}
    <CliPanel {cli} {agent} panel={cliPanel} onconfirm={(run) => confirmThen(CONFIRM.key, run)} />
  {:else if d.selected}
    <div class="edit-area" data-control="edit"><Inspector {d} /></div>
  {/if}
{/snippet}

{#snippet actions(m: 'define' | 'operate')}
  {#if m === 'define'}
    {@const blocked = problemCount > 0 || d.doc.topo.nodes.length === 0}
    <button class="button-primary-header" data-control="gen" bind:this={genEl} aria-disabled={blocked} title={blocked ? SKELETON.makeWhy : undefined} onclick={() => !blocked && void makeSkeleton()}><span>{SKELETON.make}</span></button>
    <button class="button-secondary-header" data-control="build" aria-disabled={buildState === 'running'} title={buildState === 'running' ? BUILD.runningWhy : undefined} onclick={() => buildState !== 'running' && void build()}><span>{BUILD.run}</span></button>
  {:else}
    <SplitButton
      label={WRITE.run}
      onmain={write}
      off={writeOff}
      offWhy={op.state === 'choosing' ? HUB_TOGGLE.locked.choosing : !connected ? WRITE.offWhy.notConnected : WRITE.offWhy.busy}
      pickOffWhy={WRITE.pickOffWhy}
      control={writeControl}
      pickName={WRITE.pickVersion}
      options={versionChoices.map((c) => (c.id === 'latest' && c.label !== WRITE.latest ? { ...c, note: WRITE.latest } : c))}
      pickOff={!WRITABLE.has(op.state)}
      onpick={(id) => void writeVersion(id)}
      bind:mainEl={writeEl}
    />
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
    <CliPanel {cli} panel="jobs" />
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
        onback={(id) => !stopped('rollback') && askRollback(id)}
        first={firstNodes}
        onfirst={(id) => (connected ? void writeOne(id) : (factory = { node: id, id: null }))}
        recovery={op.recovering && op.recovered ? { id: op.recovering, result: op.recovered } : null}
      />
{/snippet}

{#if !serial && !dev && !cli.sim}
  <Unsupported />
{:else}
  <a class="skip" href="#canvas">{SKIP.canvas}</a>
  <a class="skip" href="#band">{SKIP.next}</a>
  <!-- The DOM follows the visual order of each layout, so Tab order matches what is seen. -->
  <div class="frame" data-locks={locksNow} class:work={screen === 'work'} class:modal={restartOpen || autosave.asking !== null}>
    <Header
      sim={simulated}
      {mode}
      onmode={screen === 'start' ? undefined : switchMode}
      {modeLocked}
      operateLocked={d.operable !== 'ok'}
      onlocked={() => lock(d.operable)}
      {menuOpen}
      onmenu={() => ((menuOpen = !menuOpen), menuOpen && (bubbleOpen = false))}
      actions={screen === 'work' ? actions : undefined}
      bind:helpOpen
      {helpKeys}
      title={screen === 'work' ? (d.doc.topo.name ?? '') : ''}
      edit={screen === 'work' && !barNarrow ? { canUndo: d.canUndo, canRedo: d.canRedo, onundo: () => d.undo(), onredo: () => d.redo(), view: d.view, onview: (v) => (d.view = v) } : undefined}
      hub={screen === 'work' ? { connected, locked: hubLocked, hint: op.state === 'idle' || op.state === 'choosing' ? band.sentence : undefined, onpress: toggleHub } : undefined}
    >
      {#snippet menu()}
        {#if menuOpen}<Menu groups={menuGroups} onclose={() => ((menuOpen = false), document.querySelector<HTMLElement>('[aria-controls="menu"]')?.focus())} />{/if}
      {/snippet}
    </Header>
    {#if screen === 'start'}
      <StartScreen s={start} served={cli.state === 'ready'} />
    {:else}
      <!-- The canvas's box is the whole stage, whatever floats over it. Three columns float over the free part: each column stacks what shares a side, so they cannot overlap. -->
      <div in:fade class="stage">
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
        <main id="canvas" tabindex="-1" aria-describedby="canvas-help" onpointerdown={canvasDown} onclick={canvasPress}>
          <p id="canvas-help" class="visually-hidden">{CANVAS.describe}</p>
          {#if mode === 'operate' || d.view === 'graph'}
            <!-- One graph for both modes: changing mode moves the same cards to their places. -->
            <Graph bind:this={graph} bind:view={canvasView} bind:positions onmoved={savePositions} freeArea={fitArea} {wholeArea} {building} {...graphProps} />
          {/if}
        </main>
        <div class="overlay">
          <div class="col">
            {#if mode === 'define'}
              {#if !partsOpen}
                <!-- The parts opener: where the window it opens stands. -->
                <div class="tools" data-avoid="">
                  <button transition:fade class="button-icon" aria-label={PARTS.open} title={PARTS.open} onclick={() => (partsOpen = true)}><span class="layer"><Icon name="plus" /></span></button>
                </div>
              {/if}
              {#if partsOpen}
                <aside transition:rise class="float parts" data-avoid="" aria-label={PARTS.heading}><NodeList {d} ondrag={startPart} onclose={() => (partsOpen = false)} /></aside>
              {/if}
            {/if}
            <span class="grow"></span>
            {#if mode === 'define' ? d.view === 'graph' : dot}
              <Focal dot={mode === 'define' ? defineDot : dot!} small={(graphProps.config?.nodes.length ?? 0) >= COMPACT_FROM} />
            {/if}
          </div>
          <div class="col mid" bind:this={midEl}>
            {#if mode === 'define' && d.view === 'table'}
              <div transition:fade class="edit-area table" data-control="edit"><Table {d} bind:tab={tableTab} onaddnode={() => document.querySelector<HTMLElement>('#list-h')?.focus()} /></div>
            {/if}
            {#if stopOpen && stopNode && op.wentBack}
              {@const running = op.live[stopNode.id]?.hello?.activeVersion}
              <StopNotice
                word={WRITE.previous}
                target={`[${stopNode.no}] ${stopNode.name}`}
                card={() => document.querySelector(`[data-key="node:${stopNode.id}"]`)}
                reason={running !== undefined ? fillIn(WRITE.rolled.reason, { version: versionText(running) }) : WRITE.previous}
                fix={WRITE.rolled.next}
                onclose={() => (stopClosed = op.failed)}
                onmore={() => ((drawerTab = 'log'), (stopClosed = op.failed))}
                linked={agentLinked}
                onask={(at) => askWhy(at, () => (stopClosed = op.failed))}
                actions={stopActions}
              />
            {/if}
            {#if notice}<Toast {notice} onmore={() => (drawerTab = 'log')} ondone={() => (notice = null)} />{/if}
          </div>
          <div class="col end">
            {#if sheetShown}
              <!-- The sheet: its × at the top right; Escape and the canvas background close it too. -->
              <aside transition:rise class="float sheet" aria-label={SHEET} data-avoid="">
                <div class="sheet-bar"><button class="button-icon sheet-close" aria-label={SHEET_CLOSE} title="{SHEET_CLOSE} (Esc)" onclick={closeSheet}><span class="layer"><Icon name="x" /></span></button></div>
                {#key d.selected ?? op.selected ?? cliPanel}<div in:fade={{ dur: '--dur-quick' }}>{@render right()}</div>{/key}
              </aside>
            {/if}
            <span class="grow"></span>
            {#if mode === 'operate' || d.view === 'graph'}
              <ZoomPill scale={canvasView.zoom} canIn={canvasView.zoom < ZOOM_LIMITS.max} canOut={canvasView.zoom > ZOOM_LIMITS.min} onin={() => graph?.zoomBy(ZOOM_LIMITS.step)} onout={() => graph?.zoomBy(1 / ZOOM_LIMITS.step)} onfit={() => graph?.fit()} />
            {:else if agentLinked || simulated}
              <!-- No zoom in the table: the sheet still stops above the mark, which keeps its place. -->
              <span class="mark-room"></span>
            {/if}
          </div>
        </div>
        {#if agentLinked || simulated}
          <!-- On the stage, outside every column: no sheet, drawer, table or toast moves it, only the user's drag. -->
          <Dagon {agent} demo={simulated} place={cli.hello?.workspace ?? d.doc.topo.name ?? 'local'} bind:open={bubbleOpen} onask={askDagon} onapply={applyProposal} ontalk={() => (drawerTab = 'talk')} openRequest={dagonOpen} />
        {/if}
        <div class="dock" data-avoid="">
          <Drawer tabs={drawerTabs} open={drawerTab} onpick={(id) => (drawerTab = id)} onclose={() => (drawerTab = null)} body={drawerBody} />
          <Status
            hub={connected && hubNode ? `[${hubNode.no}] ${hubNode.name}` : stageWord(HUB[hub])}
            {connected}
            state={stateName(lockStates)}
            cli={cli.hello?.key_id ? `${cliLabel}　${STATUS.key} ${cli.hello.key_id.slice(0, 8)}` : cliLabel}
            cliReady={cli.state === 'ready'}
            oncli={simulated ? undefined : openCli}
            saved={savedLabel}
            savedOk={!d.unsaved && !autosave.pending}
            sentence={[next.cause, next.sentence].filter(Boolean).join(' ')}
            status={d.status || op.status}
            alert={op.alert || (cli.defsFailed ? DEFINE.defsSentence : '')}
            bind:nextEl
          />
        </div>
      </div>
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
    oncancel={() => (pickForRecovery = null)}
  />
  {#if ghost?.shown}
    <div class="ghost" aria-hidden="true" style:left="{ghost.x + 8}px" style:top="{ghost.y + 8}px">{ghost.part.name}</div>
  {/if}
  {#if why}
    <!-- a stopped operation's reason is information (blue edge), never red. -->
    <div transition:rise={{ dur: '--dur-quick' }} class="why" class:failed={why.failed} role={why.failed ? 'alert' : 'status'} style:left="{Math.max(8, Math.min(why.x, innerWidth - 336))}px" style:top="{Math.min(why.y, innerHeight - 96)}px">{why.text}</div>
  {/if}
  {#if linkOpen && agent.status}
    <LinkDialog status={agent.status} onagree={(c) => void agree(c)} oncancel={() => (linkOpen = false)} />
  {/if}
  {#if restartOpen}
    <Restart onconfirm={restart} oncancel={cancelRestart} />
  {/if}
  {#if ask || d.askRemove}
    {@const key = d.askRemove ?? ''}
    {@const isNode = key.startsWith('node:')}
    {@const l = isNode ? null : d.doc.topo.links?.find((x) => `link:${x.to}` === key)}
    {@const nameOf = (id: string) => d.doc.names[id] ?? id}
    {@const c = ask ?? { title: isNode ? CONFIRM.node(nameOf(key.slice(5))) : CONFIRM.link(nameOf(l?.from ?? ''), nameOf(l?.to ?? '')), sentence: isNode ? CONFIRM.nodeSentence : CONFIRM.linkSentence, label: CONFIRM.remove, run: () => d.answerRemove(true) }}
    {@const no = () => (ask ? (ask = null) : d.answerRemove(false))}
    <Modal title={c.title} chip={{ text: c.label, kind: 'error' }} onclose={no}>
      <p>{c.sentence}</p>
      {#snippet actions()}
        <button class="button-secondary" onclick={no}>{CONFIRM.cancel}</button>
        <button class="button-destructive" onclick={() => { const { run } = c; ask = null; run(); }}>{c.label}</button>
      {/snippet}
    </Modal>
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
      node={{ label: `[${factoryNode.no}] ${factoryNode.name}`, target: `[${factoryNode.no}] ${factoryNode.name}（${factoryNode.role === 'hub' ? NODE.roleHub : NODE.roleNode}）· ${factoryNode.product}` }}
      child={factoryNode.role !== 'hub'}
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
  {#if stopOpen && stopNode && !op.wentBack}
    <StopModal
      target={`[${stopNode.no}] ${stopNode.name}`}
      label={op.coreWentBack ? WRITE.coreBack.label : undefined}
      reason={op.coreWentBack ? WRITE.coreBack.detail : failWhy.reason}
      fix={op.coreWentBack ? WRITE.coreBack.next : failWhy.fix}
      onclose={() => (stopClosed = op.failed)}
      onmore={() => ((drawerTab = 'log'), (stopClosed = op.failed))}
    >
      {#snippet buttons()}
        {@render stopActions()}
      {/snippet}
    </StopModal>
  {/if}
{#if romNode}
    <RomConfirm node={romNode.name} onwrite={writeRom} oncancel={() => (romAsk = null)} />
  {/if}
  {#if buildFailureOpen}
    {@const f = buildFailure(buildJobs, d.config.reach)}
    {@const at = d.config.nodes.find((n) => n.id === f.node)}
    <Modal title={BUILD.title} chip={{ text: BUILD.state.failed, kind: 'error' }} target={at ? `[${at.no}] ${at.name} · nodes/${at.id}/` : undefined} onclose={() => (buildFailureOpen = false)} onmore={() => ((drawerTab = 'jobs'), (buildFailureOpen = false))}>
      <h3 class="modal-h">{NOTICE.reason}</h3>
      {#if f.skeleton}
        <p>{f.skeleton === 'none' ? BUILD.noSkeleton.reason : BUILD.noSkeleton.stale}</p>
        <h3 class="modal-h">{NOTICE.fix}</h3>
        <p>{BUILD.noSkeleton.fix}</p>
      {:else}
        <p class="modal-mono">{buildStartFailed ? BUILD.startFailed : f.reason}</p>
        <h3 class="modal-h">{NOTICE.fix}</h3>
        <p>{f.file ? fillIn(BUILD.fail.fix, { file: f.file, line: f.line ?? 0 }) : BUILD.fail.plain}{f.others ? fillIn(BUILD.fail.others, { n: f.others }) : ''}</p>
      {/if}
      {#snippet actions()}
        {#if f.skeleton}
          <button class="button-primary" data-control="gen" onclick={() => ((buildFailureOpen = false), void makeSkeleton())}>{SKELETON.make}</button>
        {:else if agentLinked}
          <button class="button-secondary" data-control="ask" onclick={(e) => askWhy(e.currentTarget, () => (buildFailureOpen = false))}>{BUILD.askAgent}</button>
        {/if}
        {#if !f.skeleton}<button class="button-primary" data-control="build" onclick={() => void build()}>{BUILD.retry}</button>{/if}
      {/snippet}
    </Modal>
  {/if}
  {#if connectModal}
    <Modal title={HUB_TOGGLE.connect} chip={{ text: band.chip ?? '', kind: 'error' }} onclose={() => (connectClosed = true)}>
      <h3 class="modal-h">{NOTICE.fix}</h3>
      <p>{band.sentence}</p>
      {#snippet actions()}
        {#if band.primary}
          {@const p = band.primary}
          <button class="button-primary" data-control={CONTROL_OF[p.action]} onclick={() => ((connectClosed = true), act(p.action))}>{p.label}</button>
        {/if}
      {/snippet}
    </Modal>
  {/if}
  {#if skeleton}
    <SkeletonModal
      job={skeletonJob}
      failure={skeleton.failure ?? null}
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
  .modal-mono {
    margin: 0;
    font: 400 var(--t-mono) / 24px var(--f-mono);
    overflow-wrap: anywhere;
  }
  /* The refusal modal's 構成 differences (the table the sheet had). */
  .modal-diff {
    width: 100%;
    border-collapse: collapse;
    font: 400 var(--t-caption) / 16px var(--f-ja);
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
    grid-template-areas: 'header' 'canvas' 'band';
    grid-template-rows: auto minmax(0, 1fr) auto;
    grid-template-columns: minmax(0, 1fr);
    height: 100vh;
    overflow: hidden;
  }
  /* Work: the stage holds everything under the bar; the start page's band, on its way out, takes no room from it. */
  .frame.work {
    grid-template-rows: auto minmax(0, 1fr) 0;
  }
  /* The canvas spans both rows, so its box does not depend on the dock; the overlay is what is left above the dock. */
  .stage {
    grid-area: canvas;
    position: relative;
    display: grid;
    /* The overlay's least: a window's worth of tools over the zoom pill or the focal number; the dock gives way below that. */
    grid-template-rows: minmax(calc(4 * var(--h-button) + var(--s-7) + var(--s-6)), 1fr) minmax(0, auto);
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
    overflow: clip;
  }
  main {
    grid-area: 1 / 1 / 3 / 2;
    display: grid;
    min-height: 0;
    background: var(--c-layer-0);
  }
  main:focus {
    outline: none;
  }
  .overlay {
    grid-area: 1 / 1 / 2 / 2;
    z-index: var(--z-chrome);
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    gap: var(--s-4);
    min-height: 0;
    padding: var(--s-4) var(--m-edge) var(--m-edge);
    pointer-events: none;
  }
  .dock {
    grid-area: 2 / 1 / 3 / 2;
    z-index: var(--z-chrome);
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  /* A column: what shares a side, top to bottom; it gives way by scrolling its window, never by covering the next. */
  .col {
    display: flex;
    flex-direction: column;
    gap: var(--s-3);
    min-width: 0;
    min-height: 0;
  }
  .col.mid {
    justify-content: flex-end;
  }
  .col.end {
    align-items: flex-end;
  }
  .grow {
    flex: 1;
    min-height: 0;
  }
  .tools {
    display: flex;
    flex: none;
    gap: var(--s-1);
    pointer-events: auto;
  }
  .table {
    flex: 1;
    min-height: 0;
    overflow: auto;
    pointer-events: auto;
  }
  /* Floating windows: L1 surfaces with a 1 px frame, no shadow. */
  .float {
    flex: 0 1 auto;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--c-line);
    border-radius: var(--r-l);
    background: var(--c-layer-1);
    pointer-events: auto;
  }
  .float.parts {
    display: flex;
    width: var(--w-parts);
  }
  .float.sheet {
    width: var(--w-sheet);
  }
  /* The × rides a zero-height sticky strip at the sheet's top, so it stays while the body scrolls. */
  .sheet-bar {
    position: sticky;
    z-index: 2;
    top: 0;
    height: 0;
  }
  /* On the head's line: the × stands level with the sheet's own top-right action (the head's trash). */
  .sheet-close {
    position: absolute;
    top: var(--s-4);
    right: var(--s-2);
  }
  /* The mark's default place, bottom right beside the zoom (Dagon.svelte), stays free: the table ends above it, a toast stands left of it. */
  .mark-room {
    flex: none;
    height: var(--w-mark);
  }
  .frame:has(:global(.dagon)) .edit-area.table {
    margin-bottom: calc(var(--w-mark) + var(--s-3));
  }
  .frame:has(:global(.dagon)) .col.mid > :global(.toast) {
    max-width: calc(100% - var(--w-mark) - var(--s-3));
    margin-right: calc(var(--w-mark) + var(--s-3));
  }
  .log-none,
  .log {
    margin: 0;
    padding: var(--s-3) var(--s-5);
    font: 400 var(--t-small) / 20px var(--f-ja);
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
    font: 400 var(--t-small) / 20px var(--f-ja);
  }
  .talk p {
    margin: 0;
  }
  .talk .who {
    color: var(--c-muted);
    font-size: var(--t-caption);
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
    font: 400 var(--t-mono) / 20px var(--f-mono);
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
  .why {
    position: fixed;
    z-index: var(--z-announce);
    max-width: min(20rem, calc(100vw - 2 * var(--s-3)));
    padding: var(--s-2) var(--s-3);
    border: 1px solid var(--c-accent);
    border-radius: var(--r-s);
    background: var(--c-layer-2);
    color: var(--c-paper);
    font: 400 var(--t-caption) / 16px var(--f-ja);
    box-shadow: 0 0.25rem 1rem rgb(0 0 0 / 0.5);
    pointer-events: none;
  }
  .ghost {
    position: fixed;
    z-index: var(--z-announce);
    padding: var(--s-1) var(--s-2);
    border: 1px solid var(--c-paper);
    border-radius: var(--r-m);
    background: var(--c-ink);
    font: 400 var(--t-caption) / 20px var(--f-ja);
    opacity: 0.9;
    pointer-events: none;
  }
  .why.failed {
    border-color: var(--c-signal);
  }
  /* Q2: while a dialog is open its own button is the one main action. */
  .frame.modal :global(footer .primary) {
    visibility: hidden;
  }
</style>
