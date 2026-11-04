<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { bandOf } from './app/band.ts';
  import { AGENT, ANNOUNCE, BAND, BUTTON, CANVAS, DEFINE, DIFF, DRAWER, FLOW, HUB, LOCK, SHEET, STATUS, ZOOM, HUB_TOGGLE, MODE, NODE, ORDER, PLAN, PRODUCT, RECONNECT, SAVE, SKIP, STEP, TIME } from './app/copy.ts';
  import { webSerial } from './app/connect.ts';
  import { HUB_LOCKED, hubOf, view, type Action } from './app/machine.ts';
  import { Operate, waterOf, type Progress } from './app/operate.svelte.ts';
  import Graph, { type Chip, type Dot } from './graph/Graph.svelte';
  import Status from './screens/Status.svelte';
  import Drawer, { type DrawerTab } from './screens/Drawer.svelte';
  import { PARTS } from './app/copy.ts';
  import { activeStates, lockOf, stateName, type Control } from './app/locks.ts';
  import { Agent, type AskKind, type Selection } from './app/agent.svelte.ts';
  import { agentMock } from './app/agentMock.ts';
  import Dagon from './screens/Dagon.svelte';
  import LinkDialog from './screens/LinkDialog.svelte';
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
  import NodeList, { type Part } from './screens/define/NodeList.svelte';
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
  import { serveSim, type SimStand } from './serial/serveSim.ts';
  import { MockTransport } from './serial/mock.ts';
  import { WebSerialTransport, type Transport } from './serial/transport.ts';
  import Unsupported from './Unsupported.svelte';
  import { dagonReady, require_release_key, set_release_key, useDefinitions, versionText } from './wasm/dagon.ts';

  const params = new URLSearchParams(location.search);
  // The stand-in hubs are for development builds only.
  const stand = import.meta.env.DEV ? devHub(params) : null;
  const dev = stand !== null;
  const serial = navigator.serial;
  // Fetch the .wasm alongside the first paint; connecting awaits it and reports a failure.
  if (serial || dev) dagonReady().catch(() => undefined);

  let opened: Transport | null = null;
  /** `dagon demo`'s simulated desk through serve, made once serve says it has one; null for a real hub. */
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
    // The simulated desk's clock runs at the demo's speed; a real hub keeps the default.
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
  });

  /** `dagon serve` (§8): the page is always its page; K0/K3 when it cannot be reached. */
  const cli = new Cli();
  /** The 定義 document: the project's 構成 as edited (§5.2: its YAML is the one source of truth). */
  const d = new Define();
  /** DAGON's link to its agent through serve (development: `?agent` stands in for serve's routes). */
  const agentStandIn = dev && params.has('agent');
  const agent = new Agent(agentStandIn ? agentMock(params, () => d.config.yaml) : (m, p, b) => cli.api(m, p, b));
  if (agentStandIn) void agent.refresh();
  /** Web Serial, which the page never uses while it drives the simulated desk. */
  const ports = webSerial(serial, () => cli.sim !== null);
  cli.onDevice = (need) => (need ? op.handOver() : op.cliReturned());

  /** SC-S0 until a project is open (§7.2b); then 定義 and 運用. */
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
      void agent.refresh();
      // The project's MCU and board definitions; the builtin set when serve has none (or no serve).
      useServedDefinitions(await cli.definitions());
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

  /** The open project's 構成 (its YAML through serve): 運用 when it has no problems, else 定義 (§7.2b). */
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
    // #mode=operate on a 構成 that cannot be operated: 定義 with the reason (§5.2).
    if (want === 'operate' && m === 'define') lock(d.operable);
  }

  /** Without serve (development stand-ins): an empty 構成 or the sample, not saved. */
  function startLocal(c: 'new' | 'sample') {
    local = true;
    op.reset();
    d.reset();
    if (c === 'sample') d.load(fromYaml(DESK_DEMO_YAML, Object.fromEntries(DESK_DEMO.nodes.map((n) => [n.id, n.name]))), DESK_DEMO_YAML);
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
    if (!(await autosave.ready())) return lock('unsaved');
    switchMode('operate');
    op.takeFile(name, bytes);
  }

  /** 運用 / 定義 (§5). The define document goes to the operate canvas when leaving define mode. */
  let mode = $state<'operate' | 'define'>('define');
  const busyStates = ['L19', 'L20', 'L21', 'L22', 'L24'];
  const modeLocked = $derived(busyStates.includes(op.state));

  function switchMode(m: 'operate' | 'define') {
    // 運用 only with a 構成 that has no problems (§5.2; parts not updatable yet are skipped there); the YAML decides, never the 親機.
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

  /** The linking dialog (DES-UX3-188), and requests to open DAGON's bubble. */
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
  async function agree() {
    linkOpen = false;
    const p = await agent.setLink(true);
    if (p) return showWhy(p.text, null, p.failed);
    const c = agent.status?.consent;
    note(AGENT.linked(agent.status?.provider.name ?? '', c?.at ?? ''));
    dagonOpen++;
  }
  async function unlink() {
    menuOpen = false;
    const p = await agent.setLink(false);
    if (p) return showWhy(p.text, null, p.failed);
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
      d.importRelease(fromYaml(p.topology, d.doc.names).topo);
    } catch {
      return showWhy(AGENT.errors.AGENT_FAILED.text, at, true);
    }
    agent.applied(i);
    note(AGENT.applied);
  }

  /** The bottom drawer's open tab (DES-UX3-015), null while closed. */
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
    { id: 'log', label: DRAWER.log },
    ...(cli.state === 'ready' ? [{ id: 'jobs', label: DRAWER.jobs, count: runningJobs }] : []),
    ...(agent.linked ? [{ id: 'talk', label: DRAWER.talk }] : []),
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

  /** A part dragged from the parts window to the canvas (DES-UX3-030): a ghost follows the pointer. */
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
  /** A refused port drag: the reason in red (it is a refusal, round8d). */
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

  /** Ctrl+Z / Ctrl+Shift+Z anywhere but in a field; Delete and L on the canvas (§12.2); Ctrl+J the drawer. */
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
  /** 定義's focal number (Q12 A+C): 問題 n, red only above 0; ノード, リンク and ◇ under it. */
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
   * (L20) it is a candidate; once that is over (L21) the page takes it by
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
    if (op.state !== 'L21') return;
    const t = new WebSerialTransport(port);
    const took = await op.reconnectHub(async () => {
      await t.open();
      return t;
    }, true);
    if (took) opened = t;
  }

  let primaryEl = $state<HTMLButtonElement>();
  // Phones start with the canvas: the parts window opens from "＋ 部品" (the mock's narrow tray).
  let partsOpen = $state(globalThis.innerWidth === undefined || innerWidth >= 768);
  /** 運用's update order: the mock keeps the top-left clear there; it opens from its button. */
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
  /** 案 (owner 11/2): ponytail: a flat minute per node, the copy's 目安; measure per link and image size if it misleads. */
  const MINUTES_PER_NODE = 1;
  /** L18: what each target node gets ("↑ 1.0.0 → 1.1.0") and about how long it takes. */
  const planChips = $derived(
    Object.fromEntries(
      op.targets.flatMap((id): [string, Chip][] => {
        const to = op.fileVersions[id];
        if (to === undefined) return [];
        const from = op.live[id]?.hello?.activeVersion;
        const [a, b] = [from === undefined ? PLAN.unknown : versionText(from), versionText(to)];
        const same = from === to;
        return [[id, { chip: same ? PLAN.same(b) : PLAN.up(a, b), spoken: same ? PLAN.sameName(b) : PLAN.upName(a, b), silent: false, note: same ? undefined : PLAN.estimate(MINUTES_PER_NODE) }]];
      }),
    ),
  );
  const progressChips = $derived(
    Object.fromEntries(
      Object.entries(op.progress).map(([id, p]) => {
        const c = chipOf(p);
        // M27 beside the rebooting hub while the page reconnects by itself.
        return [id, p.stage === 'rebooting' && reconnectLeft !== null ? { ...c, note: `${RECONNECT.auto} ${RECONNECT.left(reconnectLeft)}` } : c];
      }),
    ),
  );
  const chips = $derived(updating ? progressChips : op.state === 'L18' ? planChips : diffChips);
  /** The water in each node while it is written (L19, L20, L24). */
  const fill = $derived(PERCENT.has(op.state) ? waterOf(op.progress) : {});
  const percent = $derived(
    PERCENT.has(op.state) ? Object.fromEntries(Object.entries(op.progress).map(([id, p]) => [id, p.percent])) : {},
  );
  // The comparison stays while the update runs (P14: never silently replaced).
  const showFile = $derived(op.file !== null && op.selected === null && ['L14', 'L15', 'L16', 'L17', 'L18', 'L19', 'L20', 'L21', 'L22', 'L23'].includes(op.state));

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
  /** The sheet shows only when it holds something (DES-UX3-014). */
  const sheetShown = $derived(
    mode === 'define'
      ? cliPanel !== null || d.selected !== null
      : !op.config || op.selected !== null || (showFile && op.file !== null) || (!!op.recovering && !!op.recovered),
  );
  $effect(() => {
    if (!sheetShown || layout === 'narrow') sheetW = 0;
  });
  $effect(() => {
    if (layout === 'narrow' || (layout === 'drawer' && !drawerOpen)) partsW = 0;
  });
  /** Node actions in the detail: while watching, with nothing running. */
  const nodeActions = $derived(['L12', 'L13', 'L23'].includes(op.state));

  /** SC-O7 only after the automatic reconnect failed (L21, owner 11/2); Escape leaves the same button in the band. */
  let dialogDismissed = $state(false);
  const dialogOpen = $derived(op.state === 'L21' && !dialogDismissed);
  $effect(() => {
    if (op.state !== 'L21') dialogDismissed = false;
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
  /** M27: seconds left of the automatic reconnect, in 5 s steps (L20). */
  const RECONNECT_STEP_S = 5;
  const reconnectLeft = $derived(
    op.reconnectUntil === null || op.state !== 'L20'
      ? null
      : Math.ceil(Math.max(0, (op.reconnectUntil - nowMs) / 1000) / RECONNECT_STEP_S) * RECONNECT_STEP_S,
  );

  const hub = $derived(hubOf(op.state));
  const connected = $derived(!['idle', 'lost'].includes(hub));
  /** DAGON waits for its agent's answer (the lock table's 返事待ち). */
  const thinking = $derived(agent.thinking);
  const BUILD_KINDS = new Set(['build', 'sign', 'release', 'gen', 'conform']);
  const running = (kinds: (k: string) => boolean) => cli.jobs.some((j) => (j.state === 'running' || j.state === 'queued') && kinds(j.kind));
  /** The lock table's states now (DES-UX3-182), leftmost first. */
  const lockStates = $derived(
    activeStates({
      // ponytail: the page has no demo playback of its own yet; `dagon demo`'s desk is driven by the user like a real one.
      demo: false,
      updating: ['L19', 'L20', 'L21'].includes(op.state),
      recovering: op.state === 'L24',
      flashing: running((k) => k === 'flash'),
      building: running((k) => BUILD_KINDS.has(k)),
      unsaved: autosave.pending && !local,
      connecting: ['L03', 'L06', 'L08'].includes(op.state),
      disconnected: !connected,
      thinking,
      // The development stand-in answers for serve's agent routes, so DAGON is not stopped by its absence.
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
  // On the window in the capture phase: ahead of every handler on the page (Svelte delegates clicks to its root).
  $effect(() => {
    addEventListener('click', lockClick, true);
    addEventListener('keydown', lockKeys, true);
    return () => {
      removeEventListener('click', lockClick, true);
      removeEventListener('keydown', lockKeys, true);
    };
  });
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
  /** The header's つなぐ / 切断 (owner 11/2): 切断 closes the port and keeps its permission. */
  function toggleHub() {
    if (hubLocked) {
      hubNotice = { at: op.state, why: hubLocked };
      if (mode === 'operate') void tick().then(() => nextEl?.focus());
      return;
    }
    if (connected) return void op.disconnect();
    if (op.state === 'L05' && op.failure === 'unavailable') return location.reload();
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
      rest: op.rest && op.state === 'L18',
      finish: op.finished,
      refused: op.returnRefused?.kind,
      sim: cli.sim !== null,
    });
    if (dialogOpen) return { ...b, primary: null };
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
    if (!c || !connected || op.state === 'L06') return null;
    if (PERCENT.has(op.state)) {
      const stopped = op.state === 'L22';
      const progress = { label: CANVAS.dotProgress, value: op.overall, unit: stopped ? CANVAS.dotStopped : CANVAS.dotPercent, name: CANVAS.dotProgressName(op.overall, stopped) };
      if (op.startedAt === null) return progress;
      // S28 and SC-O5: 経過 under the number, 残り beside it (the estimate less the time gone).
      const gone = Math.max(0, wallMs - op.startedAt);
      const nodes = op.state === 'L24' ? 1 : Math.max(1, op.targets.length);
      const left = nodes * MINUTES_PER_NODE * 60_000 - gone;
      return { ...progress, sub: [TIME.elapsed, TIME.clock(gone)] as const, more: [[TIME.left, TIME.about(left)]] as const };
    }
    if (op.state === 'L08' || op.state === 'L09') return { label: CANVAS.dotFound, value: op.found, name: CANVAS.dotFoundName(op.found) };
    // Out of the nodes updating reaches: the others are never watched (V-23).
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

  function act(a: Action) {
    switch (a) {
      case 'connectDiscover':
      case 'connect':
      case 'reselect':
        return void op.connect();
      case 'reconnect':
        return void (op.state === 'L20' || op.state === 'L21' ? reconnectChosen() : op.connect());
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
    <button aria-disabled={zoom <= 0.5} title="{ZOOM.out} (Ctrl+-)" aria-label={ZOOM.out} onclick={() => (zoom = Math.max(0.5, zoom / 1.2))}>−</button>
    <button title="{ZOOM.fit} (F)" onclick={() => (zoom = 1)}>{ZOOM.fit}</button>
    <button aria-disabled={zoom >= 2} title="{ZOOM.in} (Ctrl++)" aria-label={ZOOM.in} onclick={() => (zoom = Math.min(2, zoom * 1.2))}>＋</button>
    <span class="pct wide-only">{Math.round(scale * 100)}%</span>
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
        chips={defineChips}
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
    <CliPanel {cli} {d} {autosave} panel={cliPanel} onuse={useRelease} />
  {:else if d.selected}
    <div class="edit-area" data-control="edit"><Inspector {d} /></div>
  {/if}
{/snippet}

{#snippet flow()}
  {@const errs = problemCount}
  {@const step = (id: 'define' | 'project' | 'build' | 'flash' | 'update') => id}
  <div class="toolbar" role="toolbar" aria-label={FLOW.label}>
    <div class="flow">
      {#each [['define', '01'], ['project', '02'], ['build', '03'], ['flash', '04'], ['update', '05']] as const as [id, no], i (id)}
        {@const done = id === 'define' ? d.doc.topo.nodes.length > 0 && errs === 0 : id === 'update' && op.state === 'L23'}
        {@const here = id === 'update' ? mode === 'operate' : mode === 'define' && (id === 'define' ? cliPanel === null : cliPanel === (id === 'build' ? 'release' : id))}
        {#if i}<span class="dash" aria-hidden="true"></span>{/if}
        <button
          class="tb"
          class:done
          class:here
          aria-current={here ? 'step' : undefined}
          data-control={id === 'project' ? 'gen' : id === 'build' ? 'build' : id === 'flash' ? 'flash' : undefined}
          onclick={() => goStep(step(id))}
        >
          <span class="no">{done ? '✓' : no}</span><span class="lbl">{FLOW.steps[id]}</span>
          {#if id === 'define' && errs}<span class="st err">{FLOW.problems(errs)}</span>{:else if id === 'define' && d.notYet}<span class="st">{FLOW.unsupported(d.notYet)}</span>{/if}
          {#if id === 'update' && PERCENT.has(op.state)}<span class="st">{op.overall}%</span>{/if}
        </button>
      {/each}
    </div>
    {#if mode === 'define'}
      {@const empty = d.doc.topo.nodes.length === 0}
      {@const firstError = d.problems.find((p) => p.severity === 'error')}
      {#if op.fileTopology && op.state === 'L17'}
        <button class="tb side" onclick={() => void importFileTopology()}>{DEFINE.importFileTopology}</button>
      {/if}
      {#if empty}
        <button class="tb primary" data-control="connect" bind:this={primaryEl} onclick={() => void op.connect()}>{BUTTON.connectDiscover}</button>
      {:else if firstError}
        <button class="tb primary" data-control="edit" bind:this={primaryEl} onclick={() => d.fix(firstError)}>{DEFINE.fix}</button>
      {:else if d.operable === 'ok'}
        <!-- Optional: updating sends the 構成 before it updates (owner 11/3). -->
        <button class="tb side" data-control="connect" onclick={() => void sendDefined()}>{connected ? DEFINE.send : DEFINE.connectSend}</button>
        <button class="tb primary" bind:this={primaryEl} onclick={() => switchMode('operate')}>{DEFINE.toOperate}</button>
      {/if}
    {:else}
      {#each band.secondary as b (b.action)}
        {@const off = !RUNS.has(b.action)}
        <button class="tb side" data-control={CONTROL_OF[b.action]} aria-disabled={off} onclick={() => !off && act(b.action)}>{b.label}</button>
      {/each}
      {#if band.primary}
        {@const p = band.primary}
        {@const off = op.state === 'L03' || !RUNS.has(p.action)}
        <!-- Busy and unavailable stay focusable (aria-disabled) so focus never drops to the page. -->
        <button class="tb primary" data-control={CONTROL_OF[p.action]} bind:this={primaryEl} aria-disabled={off} onclick={() => !off && act(p.action)}>
          {op.state === 'L03' ? `${p.label}…` : p.label}
        </button>
      {/if}
    {/if}
    <button
      class="tb conn"
      class:on={connected}
      data-control="connect"
      aria-disabled={hubLocked !== null}
      title={connected ? HUB_TOGGLE.disconnect : HUB_TOGGLE.connect}
      aria-label={connected ? `${FLOW.responding(op.responding, op.reach.length)}、${HUB_TOGGLE.disconnect}` : HUB_TOGGLE.connect}
      onclick={toggleHub}
    >
      <span class="dot" aria-hidden="true"></span><span class="lbl">{connected ? FLOW.responding(op.responding, op.reach.length) : HUB_TOGGLE.connect}</span>
    </button>
  </div>
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
      sending={op.state === 'L10'}
      selected={op.selected}
      onselect={(k) => op.select(op.selected === k ? null : k)}
      {dot}
      {chips}
      {fill}
      inset={{ left: partsW, right: sheetW }}
    />
  </main>
{/snippet}

{#snippet right()}
  {#if mode === 'define'}{@render defineRight()}{:else}{@render operateRight()}{/if}
{/snippet}

{#snippet operateRight()}
  {#if showFile && op.file}
      <FilePanel file={op.file} targets={op.targets.length} minutes={op.targets.length * MINUTES_PER_NODE} diff={fileDiff} />
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
{/snippet}

{#if !serial && !dev && !cli.sim}
  <Unsupported />
{:else}
  <a class="skip" href="#canvas">{SKIP.canvas}</a>
  <a class="skip" href="#band">{SKIP.next}</a>
  <!-- The DOM follows the visual order of each layout, so Tab order matches what is seen. -->
  <div class="frame {layout}" data-locks={locksNow} class:work={screen === 'work'} class:open={drawerOpen} class:modal={restartOpen || autosave.asking !== null}>
    <Header
      sim={cli.sim !== null}
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
      done={op.state === 'L23'}
      onhub={screen === 'start' ? undefined : toggleHub}
      {hubLocked}
      flow={screen === 'work' ? flow : undefined}
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
      />
    {/if}
    {#if screen === 'start'}
      <aside class="left"></aside>
      <StartScreen s={start} served={cli.state === 'ready'} />
    {:else}
      <!-- The graph fills the stage; the parts window (top-left) and the sheet (right) float over it only when they hold something. -->
      <div class="stage">
        {@render canvas()}
        {#if (layout === 'wide' || layout === 'narrow') && (mode === 'define' ? partsOpen : orderOpen)}
          <aside class="float parts" data-avoid="" aria-label={mode === 'define' ? PARTS.heading : ORDER.heading} bind:offsetWidth={partsW}>{@render left()}</aside>
        {:else if layout !== 'drawer'}
          <button class="secondary tray-open" onclick={() => (mode === 'define' ? (partsOpen = true) : (orderOpen = true))}>{mode === 'define' ? PARTS.open : ORDER.heading}</button>
        {:else if drawerOpen}
          <div class="float parts" data-avoid="" id="drawer" role="dialog" aria-modal="true" aria-label={ORDER.heading} tabindex="-1" bind:this={drawerEl} bind:offsetWidth={partsW} onkeydown={drawerKeys}>
            {@render left()}
          </div>
        {/if}
        {#if agent.linked}
          <Dagon {agent} place={cli.hello?.workspace ?? d.doc.topo.name ?? 'local'} onask={askDagon} onapply={applyProposal} ontalk={() => (drawerTab = 'talk')} openRequest={dagonOpen} />
        {/if}
        {#if sheetShown}
          <aside class="float sheet right" aria-label={SHEET} data-avoid="" bind:offsetWidth={sheetW} bind:offsetHeight={sheetH}>{@render right()}</aside>
        {/if}
      </div>
      {#if drawerTab}
        <Drawer tabs={drawerTabs} open={drawerTab} onpick={(id) => (drawerTab = id)} onclose={() => (drawerTab = null)} body={drawerBody} />
      {/if}
      <Status
        hub={{ on: connected, text: HUB[hub].replace(/^\S+\s/, '') }}
        onhub={toggleHub}
        state={stateName(lockStates)}
        cli={cliLabel}
        oncli={() => {
          if (mode !== 'define') switchMode('define');
          d.selected = null;
          cliPanel = 'link';
        }}
        key={cli.hello?.key_id?.slice(0, 8) ?? null}
        toggles={drawerTabs}
        open={drawerTab}
        ontoggle={pickTab}
        progress={PERCENT.has(op.state) ? op.overall : null}
        sentence={next.sentence}
        cause={next.cause}
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
    <!-- Round8d: a stopped operation's reason is information (blue edge), never red. -->
    <div class="why" class:failed={why.failed} role={why.failed ? 'alert' : 'status'} style:left="{Math.max(8, Math.min(why.x, innerWidth - 336))}px" style:top="{Math.min(why.y, innerHeight - 96)}px">{why.text}</div>
  {/if}
  {#if linkOpen && agent.status}
    <LinkDialog status={agent.status} onagree={() => void agree()} oncancel={() => (linkOpen = false)} />
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
  {#if dialogOpen}
    <Reconnect notes={op.reconnectNotes} onreconnect={() => void reconnectChosen()} onclose={() => (dialogDismissed = true)} />
  {/if}
{/if}

<svelte:window onkeydown={defineKeys} />

<style>
  main.define {
    position: relative;
  }
  /* The flow (DES-UX3-020): 構成 › プロジェクト › ビルド › 書き込み › 更新, the next action, and the 親機. */
  .toolbar {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: center;
    gap: var(--s-2);
    min-width: 0;
  }
  .flow {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .dash {
    width: 0.875rem;
    height: 1px;
    background: var(--c-line);
  }
  .tb {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    height: 1.75rem;
    padding: 0 0.625rem;
    border: 1px solid transparent;
    border-radius: var(--r-full);
    background: none;
    color: var(--c-paper);
    font: 400 var(--t-13) / 1 var(--f-ja);
    white-space: nowrap;
    cursor: pointer;
    transition: transform var(--dur-press) var(--ease-out), background-color var(--dur-press) ease;
  }
  .tb:active {
    transform: scale(0.97);
  }
  @media (hover: hover) and (pointer: fine) {
    .tb:hover {
      background: var(--c-layer-2);
    }
  }
  .tb:focus-visible {
    outline: 2px solid var(--c-paper);
    outline-offset: -2px;
  }
  .tb .no {
    color: var(--c-muted);
    font: 400 var(--t-13) / 1 var(--f-version);
  }
  .tb.done .no,
  .tb.here .no {
    color: var(--c-paper);
  }
  .tb.here {
    border-color: var(--c-line);
  }
  .tb .st {
    color: var(--c-muted);
    font-size: var(--t-12);
  }
  .tb .st.err {
    color: var(--c-signal);
  }
  .tb.primary {
    background: var(--c-accent);
    color: var(--c-ink);
    font-weight: 700;
  }
  .tb.side {
    height: 1.5rem;
    padding: 0 0.5rem;
    border-color: var(--c-hair);
    color: var(--c-muted);
    font-size: var(--t-12);
  }
  .tb[aria-disabled='true'] {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .tb.conn {
    border-color: var(--c-line);
  }
  .tb.conn .dot {
    width: 0.5rem;
    height: 0.5rem;
    border: 1.5px solid currentColor;
    border-radius: 50%;
  }
  .tb.conn.on .dot {
    background: currentColor;
  }
  /* Narrower bars drop words before controls: the step names, then the side buttons' words. */
  @media (max-width: 1365px) {
    .flow .tb:not(.here) .lbl {
      display: none;
    }
  }
  @media (max-width: 1099px) {
    .flow .tb .st {
      display: none;
    }
  }
  /* Phones: the bar keeps the one next action; the 親機 is on the status row, the rest in the sheet and menu. */
  @media (max-width: 767px) {
    .flow,
    .tb.conn {
      display: none;
    }
    /* The band's other actions stay reachable: they wrap onto a second row of the bar. */
    .toolbar {
      flex-wrap: wrap;
      justify-content: flex-end;
      padding: 0.25rem 0;
    }
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
  .zoom button,
  :global(.graph .zoom button) {
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
  :global(.graph .zoom button[aria-checked='true']),
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
    :global(.graph .zoom button:hover) {
      background: var(--c-layer-3);
      color: var(--c-paper);
    }
  }
  @media (max-width: 767px), (pointer: coarse) {
    :global(.graph .zoom button),
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
  /* Floating windows (DES-UX3-014): over the graph, apart from its edges, never under the focal number. */
  .float {
    position: absolute;
    z-index: var(--z-chrome);
    top: 0.75rem;
    overflow: auto;
    border: 1px solid var(--c-hair);
    border-radius: var(--r-m);
    background: rgb(15 15 14 / 0.96);
    box-shadow: 0 0.75rem 2rem rgb(0 0 0 / 0.5);
  }
  .float.parts {
    left: 0.75rem;
    width: var(--w-left);
    /* The focal number keeps the bottom-left: the window stops above it. */
    max-height: calc(100% - 9.5rem - 1.5rem);
  }
  .float.sheet {
    right: 0.75rem;
    width: var(--w-right);
    max-height: calc(100% - 1.5rem);
  }
  #drawer:focus {
    outline: none;
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
  /* Phones (the mock's narrow window): the same one window; the parts drop from the bar, the sheet rises from the status row. */
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
