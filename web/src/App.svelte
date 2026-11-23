<script lang="ts">
  // DAGON Console: the page's parts put together. Each part lives in its own module
  // (src/app/console/); this file builds them, ties them to each other and lays out the screen.
  import { tick, untrack } from 'svelte';
  import { BUILD, CANVAS, CONFIRM, DEFINE, DRAWER, HUB, HUB_TOGGLE, MENU, PARTS, SAVE, SHEET, SHEET_CLOSE, SKELETON, SKIP, STATUS, WRITE, CLI_STATE, fill as fillIn } from './app/copy.ts';
  import { fade, rise } from './app/motion.ts';
  import type { Action } from './app/machine.ts';
  import { imageRows, previousVersion, rollbackOff } from './app/slots.ts';
  import { buildFailure, firstWriteNodes } from './app/jobFlow.ts';
  import { stateName, type Control } from './app/locks.ts';
  import { Agent } from './app/agent.svelte.ts';
  import { agentMock } from './app/agentMock.ts';
  import { Define } from './app/define/define.svelte.ts';
  import { Cli } from './app/define/cli.svelte.ts';
  import { DEMO_SITE } from './app/site.ts';
  import { AgentLink } from './app/console/agentLink.svelte.ts';
  import { BuildFlow } from './app/console/buildFlow.svelte.ts';
  import { DefineView, DRAG_PX } from './app/console/defineView.svelte.ts';
  import { Feedback } from './app/console/feedback.svelte.ts';
  import { HubPort } from './app/console/hubPort.svelte.ts';
  import { consoleKeys } from './app/console/keys.ts';
  import { LockGate } from './app/console/lockGate.svelte.ts';
  import { consoleMenu } from './app/console/menu.ts';
  import { OperateView } from './app/console/operateView.svelte.ts';
  import { MotionPref } from './app/console/motionPref.svelte.ts';
  import { BUSY, CONTROL_OF, stageWord, WRITABLE } from './app/console/states.ts';
  import { Workspace } from './app/console/workspace.svelte.ts';
  import { WriteFlow } from './app/console/writeFlow.svelte.ts';
  import Graph, { COMPACT_FROM } from './graph/Graph.svelte';
  import { ZOOM_LIMITS } from './graph/fit.ts';
  import BandButtons from './screens/BandButtons.svelte';
  import BuildFailureModal from './screens/BuildFailureModal.svelte';
  import ConfirmModal, { type Ask } from './screens/ConfirmModal.svelte';
  import Dagon from './screens/Dagon.svelte';
  import Detail from './screens/Detail.svelte';
  import Drawer, { type DrawerTab } from './screens/Drawer.svelte';
  import DrawerBody from './screens/DrawerBody.svelte';
  import Focal from './screens/Focal.svelte';
  import Header from './screens/Header.svelte';
  import Icon from './screens/Icon.svelte';
  import LinkDialog from './screens/LinkDialog.svelte';
  import Menu from './screens/Menu.svelte';
  import Restart from './screens/Restart.svelte';
  import SkeletonModal from './screens/SkeletonModal.svelte';
  import SplitButton from './screens/SplitButton.svelte';
  import StartScreen from './screens/Start.svelte';
  import Status from './screens/Status.svelte';
  import StopNotice from './screens/StopNotice.svelte';
  import Toast from './screens/Toast.svelte';
  import Pointer from './screens/Pointer.svelte';
  import WriteModals from './screens/WriteModals.svelte';
  import Unsaved from './screens/Unsaved.svelte';
  import ZoomPill from './screens/ZoomPill.svelte';
  import CliPanel from './screens/define/CliPanel.svelte';
  import Inspector from './screens/define/Inspector.svelte';
  import NodeList from './screens/define/NodeList.svelte';
  import Table from './screens/define/Table.svelte';
  import { devHub } from './serial/devHub.ts';
  import { isNarrow } from './app/narrow.ts';
  import Unsupported from './Unsupported.svelte';
  import { dagonReady, versionText } from './wasm/dagon.ts';

  const params = new URLSearchParams(location.search);
  // The stand-in nodes connected to the PC are for development builds only; the public demo page always drives the simulator.
  const stand = import.meta.env.DEV ? devHub(params) : DEMO_SITE ? devHub(new URLSearchParams('sim')) : null;
  const dev = stand !== null;
  const serial = navigator.serial;
  // Fetch the .wasm alongside the first paint; connecting awaits it and reports a failure.
  if (serial || dev) dagonReady().catch(() => undefined);

  /** `dagon serve`: the page is always its page; `none` or `stopped` when it cannot be reached. */
  const cli = new Cli();
  /** The 定義 document: the project's 構成 as edited (its YAML is the one source of truth). */
  const d = new Define();
  const mcuOf = (id: string) => d.doc.topo.nodes.find((n) => n.id === id)?.mcu ?? '';
  const port = new HubPort({ serial, stand, cli, mcuOf });
  const op = port.op;
  const demoStandIn = import.meta.env.DEV && params.has('demo');
  /** The node connected to the PC and the other nodes are simulated: the demo page's own simulator. */
  const simulated = DEMO_SITE || demoStandIn;
  /** DAGON's link to its agent through serve (development: `?agent` stands in for serve's routes). */
  // Development builds only, written so every other build (the served page, the demo page) leaves agentMock out.
  const agentStandIn = import.meta.env.DEV && dev && params.has('agent');
  const agent = new Agent(import.meta.env.DEV && agentStandIn ? agentMock(params, () => d.config.yaml) : (route, o) => cli.api(route, o));
  if (agentStandIn) void agent.refresh();
  /** One condition closes every path to linking, talking, keys and the CLI sheet: linked is never true in a demo. */
  const agentLinked = $derived(!simulated && agent.linked);
  /** The notice, 記録 and the drawer: what the page tells without stopping the user. */
  const feedback = new Feedback();
  type Next = { cause?: string; sentence: string };
  const stopped = (c: Control): boolean => gate.stopped(c);
  /** The project: the start screen, 定義 and 運用, saving, names and positions. */
  const ws = new Workspace({ cli, d, op, agent, feedback, dev, demoSite: DEMO_SITE, stopped });
  const { autosave } = ws;

  let fileInput = $state<HTMLInputElement>();
  async function picked(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const f = input.files?.[0];
    input.value = '';
    await writes.picked(f);
  }
  /** The version picker, writing while watching, to one node, and directly over USB. */
  const writes = new WriteFlow({ op, cli, standLatest: stand?.latest, stopped, tell: (n) => (feedback.notice = n), chooseFile: () => fileInput?.click() });
  /** ビルドする and テンプレート生成 in 準備: serve's jobs. */
  const build = new BuildFlow({
    cli,
    autosave,
    reach: () => d.config.reach,
    revision: () => d.revision,
    stopped,
    done: (n) => {
      feedback.notice = { word: fillIn(BUILD.toast.done, { n }) };
      void writes.loadVersions();
    },
  });
  let graph = $state<ReturnType<typeof Graph>>();
  const dv = new DefineView({ d, cli, ws, build, stopped, dropTarget: (x, y) => graph?.dropTarget(x, y) });
  const ov = new OperateView({ op, d, writes, feedback, simulated, demoSite: DEMO_SITE, next: (): Next => next });
  const band = $derived(ov.band);
  const connected = $derived(ov.connected);
  /** The status row's sentence in either mode; before a project opens there is no 構成 to read. */
  const next: Next = $derived(ws.mode === 'define' ? (ws.screen === 'work' ? dv.next : { sentence: '' }) : { cause: band.chip, sentence: band.sentence ?? '' });
  // Again on entering 書込 and after each write (the written package was kept).
  $effect(() => {
    if (ws.mode === 'operate' && (op.state === 'ready' || op.state === 'done')) untrack(() => void writes.loadVersions());
  });

  const BUILD_KINDS = new Set(['build', 'sign', 'release', 'gen', 'conform']);
  const running = (kinds: (k: string) => boolean) => cli.jobs.some((j) => (j.state === 'running' || j.state === 'queued') && kinds(j.kind));
  /** The lock table now: the one check buttons, menus, keys and commands go through. */
  const gate = new LockGate(
    () => ({
      // the page has no demo playback of its own; a simulator is driven by the user like a real PC-connected node.
      demo: false,
      updating: ['writing', 'hubRestarting', 'reconnectNeeded'].includes(op.state),
      recovering: op.state === 'recovering',
      flashing: running((k) => k === 'flash'),
      building: running((k) => BUILD_KINDS.has(k)),
      unsaved: autosave.pending && !ws.local,
      connecting: ['choosing', 'checking', 'scanning'].includes(op.state),
      disconnected: !connected,
      thinking: agent.thinking,
      // The development stand-in answers for serve's agent routes, so DAGON is not stopped by its absence.
      site: DEMO_SITE,
      cliNone: cli.state !== 'ready' && !agentStandIn,
    }),
    (text) => feedback.note(text),
  );
  gate.install();
  // The models check the lock table inside their actions: any path to them (key, double-click, drag) is stopped.
  d.guard = stopped;
  cli.guard = stopped;
  agent.guard = stopped;
  agent.record = () => op.settledRollbackRecord();
  /** DAGON's agent: linking, asking about the selection, taking in a proposal. */
  const agentLink = new AgentLink({ agent, d, gate, feedback, simulated, selected: () => (ws.mode === 'define' ? d.selected : op.selected) });

  let helpOpen = $state(false);
  let helpKeys = $state(false);
  let partsOpen = $state(true);
  let tableTab = $state(0);
  let genEl = $state<HTMLButtonElement>();
  let writeEl = $state<HTMLButtonElement>();
  let nextEl = $state<HTMLElement>();
  /** The free part of the canvas (between the floating columns): what a fit fills. */
  let midEl = $state<HTMLElement>();
  /** Every destructive action asks through this one modal: what, why, then the red button. */
  let ask = $state<Ask | null>(null);
  const confirmThen = (c: { title: string; sentence: string; label: string }, run: () => void) => (ask = { ...c, run });

  const cliLabel = $derived(cli.state === 'ready' ? CLI_STATE.ready(cli.hello?.cli ?? '') : CLI_STATE[cli.state]);
  const runningJobs = $derived(cli.jobs.filter((j) => j.state === 'running' || j.state === 'queued' || j.state === 'question').length);
  const drawerTabs = $derived<DrawerTab[]>([
    { id: 'problems', label: DRAWER.problems, count: dv.problemCount + d.skipped, alarm: dv.problemCount > 0 },
    { id: 'jobs', label: DRAWER.jobs, ...(runningJobs ? { count: runningJobs } : {}) },
    { id: 'output', label: DRAWER.output },
    { id: 'timeline', label: DRAWER.timeline },
    ...(simulated ? [] : [{ id: 'talk', label: DRAWER.talk }]),
    { id: 'log', label: DRAWER.log },
  ]);
  const modeLocked = $derived(BUSY.includes(op.state));
  const motion = new MotionPref();
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
    if (ws.mode === 'define') {
      ws.cliPanel = null;
      d.selected = null;
    } else op.select(null);
  }
  /** A press on the canvas's empty part that does not move (a pan is not a click) closes the sheet. */
  let pressAt: { x: number; y: number } | null = null;
  function canvasPress(e: MouseEvent) {
    const el = e.target as HTMLElement;
    const moved = !pressAt || Math.hypot(e.clientX - pressAt.x, e.clientY - pressAt.y) > DRAG_PX;
    pressAt = null;
    if (moved || el.closest('button, .port, .link-chip, a, input')) return;
    if (el.closest('.box') && !el.closest('.node')) closeSheet();
  }
  /** The free part of the canvas a fit fills: the middle column less the room a toast stands in, so a notice never covers a card of the fit view. */
  const TOAST_ROOM = 64;
  // The narrow tier stacks the columns under the canvas: the fit takes the canvas's own box.
  const fitArea = () => {
    if (isNarrow()) return undefined;
    const r = midEl?.getBoundingClientRect();
    return r && new DOMRect(r.left, r.top, r.width, Math.max(0, r.height - TOAST_ROOM));
  };
  /** The whole middle column: the fit falls back to it when the toast's room alone would make the cards compact. */
  const wholeArea = () => (isNarrow() ? undefined : midEl?.getBoundingClientRect());
  /** The sheet shows only when it holds something. */
  const sheetShown = $derived(ws.mode === 'define' ? ws.cliPanel !== null || d.selected !== null : !op.config || op.selected !== null || (!!op.recovering && !!op.recovered));
  const idle = $derived(WRITABLE.has(op.state));

  /** The header's 接続 / 切断: 切断 closes the port and keeps its permission. */
  function toggleHub() {
    if (stopped('connect')) return;
    if (ov.hubLocked) {
      // Seen where the button is, in either mode; the status row's sentence is for screen readers.
      gate.showWhy(ov.hubLocked, document.querySelector('header [data-control="connect"]'));
      ov.noteHubLocked(ov.hubLocked);
      if (ws.mode === 'operate') void tick().then(() => nextEl?.focus());
      return;
    }
    if (connected) return void op.disconnect();
    if (op.state === 'portFailed' && op.failure === 'unavailable') return location.reload();
    if (ws.mode === 'define' && d.operable === 'ok') op.useConfig(d.config);
    void op.connect();
  }

  /** What the one graph shows in each mode. */
  const graphProps = $derived(
    ws.mode === 'define'
      ? {
          config: d.doc.topo.nodes.length ? d.config : null,
          live: {},
          connected: false,
          selected: d.selected,
          onselect: (k: string) => (d.selected = d.selected === k ? null : k),
          chips: dv.chips,
          dropOn: dv.dropOn,
          canLink: (from: string, to: string) => d.linkRefusal(from, to),
          onlink: (from: string, to: string) => !stopped('edit') && d.link(to, from),
          onrefuse: (text: string, x: number, y: number) => gate.refuse(text, x, y),
        }
      : {
          config: op.config,
          live: op.live,
          connected,
          selected: op.selected,
          onselect: (k: string) => op.select(op.selected === k ? null : k),
          chips: ov.chips,
          fill: ov.fill,
          checks: Object.fromEntries(op.reach.map((n) => [n.id, !op.skip.has(n.id)])),
          oncheck: (id: string) => op.toggleSkip(id),
          checksLocked: ov.updating && !['done', 'recoverFailed'].includes(op.state),
          confirmedOf: (id: string) => op.progress[id]?.stage === 'done',
        },
  );

  // Focus follows the band only after a user action changed it, never on load.
  $effect(() => {
    if (op.focusRequest === 0) return;
    void tick().then(() => (band.primary ? (ws.mode === 'define' ? genEl : writeEl) : nextEl)?.focus());
  });

  const selectedNode = $derived(op.selected?.startsWith('node:') ? op.selected.slice(5) : null);
  const copied = { done: MENU.copied, failed: MENU.copied.replace('完了', '失敗') };
  const menuGroups = $derived(
    consoleMenu({
      d,
      mode: ws.mode,
      busy: modeLocked,
      noProject: ws.local || ws.screen !== 'work',
      demoSite: DEMO_SITE,
      simulated,
      agentLinked,
      barNarrow,
      partsOpen,
      arranged: Object.keys(ws.positions).length > 0,
      connected,
      hubLocked: ov.hubLocked,
      idle,
      rollbackOff: selectedNode ? rollbackOff(op.live[selectedNode]?.status, imageRows(op.config?.nodes.find((x) => x.id === selectedNode)?.mcuId)) : undefined,
      lastProposal: agentLink.lastProposal,
      hasNotes: feedback.notes.length > 0,
      motionStill: motion.still,
      run: {
        toStart: (c) => ws.toStart(c),
        save: () => void ws.save(),
        restart: () => (ws.restartOpen = true),
        openParts: () => (partsOpen = true),
        arrange: () => ws.arrange(),
        fit: () => graph?.fit(),
        toggleHub,
        chooseFile: () => fileInput?.click(),
        rollback: () => selectedNode && askRollback(selectedNode),
        talk: () => void agentLink.openRequest++,
        applyProposal: () => agentLink.applyProposal(agentLink.lastProposal, document.body),
        link: () => void agentLink.openLink(document.querySelector('[aria-controls="menu"]')),
        cli: openCli,
        unlink: () => ((ws.menuOpen = false), void agentLink.unlink()),
        help: (keys) => ((helpKeys = keys), (helpOpen = true)),
        copyLog: () => void feedback.copy(feedback.notes.map((n) => `${n.at} ${n.text}`).join('\n'), copied),
        copyUrl: () => void feedback.copy(location.href, copied),
        toggleMotion: () => (motion.still = !motion.still),
        confirm: confirmThen,
      },
    }),
  );
  function defineKeys(e: KeyboardEvent) {
    consoleKeys(e, {
      work: ws.screen === 'work',
      mode: ws.mode,
      d,
      dagon: agentLinked || simulated,
      bubbleOpen: agentLink.bubbleOpen,
      popoverOpen: ws.menuOpen || helpOpen,
      stopped,
      toggleDrawer: () => feedback.pickTab(feedback.drawerTab ?? 'problems'),
      openDagon: () => void agentLink.openRequest++,
      closeBubble: () => (agentLink.bubbleOpen = false),
      closeSheet: () => ((gate.why = null), closeSheet()),
      fit: () => graph?.fit(),
      zoomBy: (f) => graph?.zoomBy(f),
      zoomStep: ZOOM_LIMITS.step,
    });
  }
  /** 前のバージョンへの復元, from the menu or the sheet: asks first, like every change that cannot be undone. */
  function askRollback(id: string) {
    const n = op.config?.nodes.find((x) => x.id === id);
    const v = previousVersion(op.live[id]?.status, imageRows(n?.mcuId));
    // No previous version (one slot, or nothing confirmed before): nothing to go back to.
    if (!v) return;
    confirmThen(CONFIRM.rollback(n ? `[${n.no}] ${n.name}` : id, v), () => void op.rollback(id));
  }
  function openCli() {
    if (simulated) return;
    if (ws.mode !== 'define') ws.switchMode('define');
    d.selected = null;
    ws.cliPanel = 'link';
  }

  /** 書込: while watching, the chosen version; with the node connected to the PC silent, directly over USB; otherwise the band's next step. */
  function write() {
    if (ov.writeOff) return;
    if (WRITABLE.has(op.state)) return void writes.writeVersion();
    if (ov.writeAction === null) return void (writes.factory = ov.hubId ? { node: ov.hubId, id: null } : null);
    act(ov.writeAction);
  }
  /** Nodes whose first write can start from their card (serve's flash). */
  const firstNodes = $derived(ws.mode === 'operate' && cli.has('flash') ? firstWriteNodes(op.config?.nodes ?? [], op.live, connected, op.state) : []);

  function act(a: Action) {
    const c = CONTROL_OF[a];
    if (c && stopped(c)) return;
    switch (a) {
      case 'connectDiscover':
      case 'connect':
      case 'reselect':
        return void op.connect();
      case 'reconnect':
        return void (op.state === 'hubRestarting' || op.state === 'reconnectNeeded' ? port.reconnectChosen() : op.connect());
      case 'chooseFile':
      case 'updateResponding':
      case 'otherFile':
        return fileInput?.click();
      case 'update':
        return void op.update();
      case 'useLatest':
        return void op.useLatest();
      case 'recover':
        return void writes.writeOne();
      case 'recoverAgain':
        return void writes.writeOne(undefined, undefined, true);
      case 'defineFix':
        return ws.switchMode('define');
      case 'reload':
        return location.reload();
      default:
        return;
    }
  }
</script>

{#snippet stopActions()}
  <BandButtons secondary={band.secondary} primary={band.primary} onpress={(a) => ((ov.stopClosed = op.failed), act(a))} />
{/snippet}

{#snippet defineRight()}
  {#if ws.cliPanel}
    <CliPanel {cli} {agent} panel={ws.cliPanel} onconfirm={(run) => confirmThen(CONFIRM.key, run)} />
  {:else if d.selected}
    <div class="edit-area" data-control="edit"><Inspector {d} /></div>
  {/if}
{/snippet}

{#snippet actions(m: 'define' | 'operate')}
  {#if m === 'define'}
    {@const blocked = dv.problemCount > 0 || d.doc.topo.nodes.length === 0}
    <button class="button-primary-header" data-control="gen" bind:this={genEl} aria-disabled={blocked} title={blocked ? SKELETON.makeWhy : undefined} onclick={() => !blocked && void build.makeSkeleton(SKELETON.startFailed)}><span>{SKELETON.make}</span></button>
    <button class="button-secondary-header" data-control="build" aria-disabled={build.state === 'running'} title={build.state === 'running' ? BUILD.runningWhy : undefined} onclick={() => build.state !== 'running' && void build.build()}><span>{BUILD.run}</span></button>
  {:else}
    <SplitButton
      label={WRITE.run}
      onmain={write}
      off={ov.writeOff}
      offWhy={op.state === 'choosing' ? HUB_TOGGLE.locked.choosing : !connected ? WRITE.offWhy.notConnected : WRITE.offWhy.busy}
      pickOffWhy={WRITE.pickOffWhy}
      control={ov.writeControl}
      pickName={WRITE.pickVersion}
      options={writes.versionChoices.map((c) => (c.id === 'latest' && c.label !== WRITE.latest ? { ...c, note: WRITE.latest } : c))}
      pickOff={!WRITABLE.has(op.state)}
      onpick={(id) => void writes.writeVersion(id)}
      bind:mainEl={writeEl}
    />
  {/if}
{/snippet}

{#snippet drawerBody(tab: string)}
  <DrawerBody
    {tab}
    {d}
    {cli}
    {agent}
    notes={feedback.notes}
    timeline={{ rows: (op.config?.nodes ?? []).filter((n) => op.targets.includes(n.id)).map((n) => ({ id: n.id, name: `[${n.no}] ${n.name}` })), events: ov.tlEvents, now: ov.tlNow, label: stageWord }}
    onapply={(i, at) => agentLink.applyProposal(i, at)}
  />
{/snippet}

{#snippet right()}
  {#if ws.mode === 'define'}{@render defineRight()}{:else}{@render operateRight()}{/if}
{/snippet}

{#snippet operateRight()}
      <Detail
        config={op.config}
        live={op.live}
        selected={op.selected}
        {connected}
        act={idle}
        onkeep={(id) => !gate.stopped('rollback') && void op.keepTrial(id)}
        onback={(id) => !gate.stopped('rollback') && askRollback(id)}
        first={firstNodes}
        onfirst={(id) => (connected ? void writes.writeOne(id) : (writes.factory = { node: id, id: null }))}
        recovery={op.recovering && op.recovered ? { id: op.recovering, result: op.recovered } : null}
      />
{/snippet}

{#if !serial && !dev}
  <Unsupported />
{:else}
  <a class="skip" href="#canvas">{SKIP.canvas}</a>
  <a class="skip" href="#band">{SKIP.next}</a>
  <!-- The DOM follows the visual order of each layout, so Tab order matches what is seen. -->
  <div class="frame" data-locks={gate.locksNow} class:work={ws.screen === 'work'} class:modal={ws.restartOpen || ws.autosave.asking !== null}>
    <Header
      sim={simulated}
      mode={ws.mode}
      onmode={ws.screen === 'start' ? undefined : (v) => ws.switchMode(v)}
      {modeLocked}
      operateLocked={d.operable !== 'ok'}
      onlocked={() => ws.lock(d.operable)}
      menuOpen={ws.menuOpen}
      onmenu={() => ((ws.menuOpen = !ws.menuOpen), ws.menuOpen && (agentLink.bubbleOpen = false))}
      actions={ws.screen === 'work' ? actions : undefined}
      bind:helpOpen
      {helpKeys}
      title={ws.screen === 'work' ? (d.doc.topo.name ?? '') : ''}
      edit={ws.screen === 'work' && !barNarrow ? { canUndo: d.canUndo, canRedo: d.canRedo, onundo: () => d.undo(), onredo: () => d.redo(), view: d.view, onview: (v) => (d.view = v) } : undefined}
      hub={ws.screen === 'work' ? { connected, locked: ov.hubLocked, hint: op.state === 'idle' || op.state === 'choosing' ? band.sentence : undefined, onpress: toggleHub } : undefined}
    >
      {#snippet menu()}
        {#if ws.menuOpen}<Menu groups={menuGroups} onclose={() => ((ws.menuOpen = false), document.querySelector<HTMLElement>('[aria-controls="menu"]')?.focus())} />{/if}
      {/snippet}
    </Header>
    {#if ws.screen === 'start'}
      <StartScreen s={ws.start} served={cli.state === 'ready'} />
    {:else}
      <!-- The canvas's box is the whole stage, whatever floats over it. Three columns float over the free part: each column stacks what shares a side, so they cannot overlap. -->
      <div in:fade class="stage">
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
        <main id="canvas" tabindex="-1" aria-describedby="canvas-help" onpointerdown={(e) => (pressAt = { x: e.clientX, y: e.clientY })} onclick={canvasPress}>
          <p id="canvas-help" class="visually-hidden">{CANVAS.describe}</p>
          {#if ws.mode === 'operate' || d.view === 'graph'}
            <!-- One graph for both modes: changing mode moves the same cards to their places. -->
            <Graph bind:this={graph} bind:view={ws.canvasView} bind:positions={ws.positions} onmoved={() => ws.savePositions()} freeArea={fitArea} {wholeArea} building={build.building} {...graphProps} />
          {/if}
        </main>
        <div class="overlay">
          <div class="col">
            {#if ws.mode === 'define'}
              {#if !partsOpen}
                <!-- The parts opener: where the window it opens stands. -->
                <div class="tools" data-avoid="">
                  <button transition:fade class="button-icon" aria-label={PARTS.open} title={PARTS.open} onclick={() => (partsOpen = true)}><span class="layer"><Icon name="plus" /></span></button>
                </div>
              {/if}
              {#if partsOpen}
                <aside transition:rise class="float parts" data-avoid="" aria-label={PARTS.heading}><NodeList {d} ondrag={(p, e) => dv.startPart(p, e)} onclose={() => (partsOpen = false)} /></aside>
              {/if}
            {/if}
            <span class="grow"></span>
            {#if ws.mode === 'define' ? d.view === 'graph' : ov.dot}
              <Focal dot={ws.mode === 'define' ? dv.dot : ov.dot!} small={(graphProps.config?.nodes.length ?? 0) >= COMPACT_FROM} />
            {/if}
          </div>
          <div class="col mid" bind:this={midEl}>
            {#if ws.mode === 'define' && d.view === 'table'}
              <div transition:fade class="edit-area table" data-control="edit"><Table {d} bind:tab={tableTab} onaddnode={() => document.querySelector<HTMLElement>('#list-h')?.focus()} /></div>
            {/if}
            {#if ov.stopOpen && ov.stopNode && op.wentBack}
              {@const node = ov.stopNode}
              {@const running = op.live[node.id]?.hello?.activeVersion}
              <StopNotice
                word={WRITE.previous}
                target={`[${node.no}] ${node.name}`}
                card={() => document.querySelector(`[data-key="node:${node.id}"]`)}
                reason={running !== undefined ? fillIn(WRITE.rolled.reason, { version: versionText(running) }) : WRITE.previous}
                fix={WRITE.rolled.next}
                onclose={() => (ov.stopClosed = op.failed)}
                onmore={() => ((feedback.drawerTab = 'log'), (ov.stopClosed = op.failed))}
                linked={agentLinked}
                onask={(at) => agentLink.askWhy(at, () => (ov.stopClosed = op.failed))}
                actions={stopActions}
              />
            {/if}
            {#if feedback.notice}<Toast notice={feedback.notice} onmore={() => (feedback.drawerTab = 'log')} ondone={() => (feedback.notice = null)} />{/if}
          </div>
          <div class="col end">
            {#if sheetShown}
              <!-- The sheet: its × at the top right; Escape and the canvas background close it too. -->
              <aside transition:rise class="float sheet" aria-label={SHEET} data-avoid="">
                <div class="sheet-bar"><button class="button-icon sheet-close" aria-label={SHEET_CLOSE} title="{SHEET_CLOSE} (Esc)" onclick={closeSheet}><span class="layer"><Icon name="x" /></span></button></div>
                {#key d.selected ?? op.selected ?? ws.cliPanel}<div in:fade={{ dur: '--dur-quick' }}>{@render right()}</div>{/key}
              </aside>
            {/if}
            <span class="grow"></span>
            {#if ws.mode === 'operate' || d.view === 'graph'}
              <ZoomPill scale={ws.canvasView.zoom} canIn={ws.canvasView.zoom < ZOOM_LIMITS.max} canOut={ws.canvasView.zoom > ZOOM_LIMITS.min} onin={() => graph?.zoomBy(ZOOM_LIMITS.step)} onout={() => graph?.zoomBy(1 / ZOOM_LIMITS.step)} onfit={() => graph?.fit()} />
            {:else if agentLinked || simulated}
              <!-- No zoom in the table: the sheet still stops above the mark, which keeps its place. -->
              <span class="mark-room"></span>
            {/if}
          </div>
          {#if agentLinked || simulated}
            <!-- Over the columns, from the free area's bottom-right as the zoom is: no sheet, table or toast moves it; the drawer carries it up with the zoom; a drag places it. -->
            <Dagon {agent} demo={simulated} place={cli.hello?.workspace ?? d.doc.topo.name ?? 'local'} bind:open={agentLink.bubbleOpen} onask={(text, kind, at) => agentLink.ask(text, kind, at)} onapply={(i, at) => agentLink.applyProposal(i, at)} ontalk={() => (feedback.drawerTab = 'talk')} openRequest={agentLink.openRequest} />
          {/if}
        </div>
        <div class="dock" data-avoid="">
          <Drawer tabs={drawerTabs} open={feedback.drawerTab} onpick={(id) => (feedback.drawerTab = id)} onclose={() => (feedback.drawerTab = null)} body={drawerBody} />
          <Status
            hub={connected && ov.hubNode ? ov.hubName : stageWord(HUB[ov.hub])}
            {connected}
            state={stateName(gate.states)}
            cli={cli.hello?.key_id ? `${cliLabel}　${STATUS.key} ${cli.hello.key_id.slice(0, 8)}` : cliLabel}
            cliReady={cli.state === 'ready'}
            oncli={simulated ? undefined : openCli}
            saved={dv.savedLabel}
            savedOk={!d.unsaved && !ws.autosave.pending}
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
    oncancel={() => writes.pickCancelled()}
  />
  <Pointer why={gate.why} ghost={dv.ghost?.shown ? { name: dv.ghost.part.name, x: dv.ghost.x, y: dv.ghost.y } : null} />
  {#if agentLink.dialogOpen && agent.status}
    <LinkDialog status={agent.status} onagree={(c) => void agentLink.agree(c)} oncancel={() => (agentLink.dialogOpen = false)} />
  {/if}
  {#if ws.restartOpen}
    <Restart onconfirm={() => ws.restart()} oncancel={() => ws.cancelRestart()} />
  {/if}
  {#if ask || d.askRemove}
    <ConfirmModal {ask} {d} onclose={() => (ask = null)} />
  {/if}
  {#if ws.autosave.asking}
    <Unsaved
      reason={SAVE[d.saveFailure ?? 'failed'].chip}
      onsave={() => void ws.autosave.saveAgain()}
      ondiscard={() => ws.autosave.discard()}
      onstay={() => ws.autosave.stay()}
    />
  {/if}
  <WriteModals {op} {ov} {writes} {cli} fix={next.sentence} {mcuOf} {act} onmore={(tab) => (feedback.drawerTab = tab)} onreconnect={() => void port.reconnectChosen()} />
  {#if build.failureOpen}
    {@const f = buildFailure(build.jobs, d.config.reach)}
    {@const at = d.config.nodes.find((n) => n.id === f.node)}
    <BuildFailureModal
      failure={f}
      target={at ? `[${at.no}] ${at.name} · nodes/${at.id}/` : undefined}
      startFailed={build.startFailed}
      linked={agentLinked}
      onclose={() => (build.failureOpen = false)}
      onmore={() => ((feedback.drawerTab = 'jobs'), (build.failureOpen = false))}
      onskeleton={() => ((build.failureOpen = false), void build.makeSkeleton(SKELETON.startFailed))}
      onask={(at) => agentLink.askWhy(at, () => (build.failureOpen = false))}
      onretry={() => void build.build()}
    />
  {/if}
  {#if build.skeleton}
    <SkeletonModal
      job={build.skeletonJob}
      failure={build.skeleton.failure ?? null}
      place={cli.hello?.workspace ?? d.doc.topo.name ?? ''}
      nodes={d.config.nodes.filter((n) => d.config.reach.includes(n.id)).map((n) => [`[${n.no}] ${n.name}`, `nodes/${n.id}/`] as [string, string])}
      onclose={() => (build.skeleton = null)}
      oncancel={() => build.skeletonJob && void cli.cancel(build.skeletonJob)}
      onretry={() => void build.makeSkeleton(SKELETON.startFailed)}
      onmore={() => ((feedback.drawerTab = 'jobs'), (build.skeleton = null))}
    />
  {/if}
{/if}

<svelte:window onkeydown={defineKeys} />

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
    position: relative;
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
    width: var(--h-button);
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
  /* While a dialog is open its own button is the one main action. */
  .frame.modal :global(footer .primary) {
    visibility: hidden;
  }
  /* The narrow tier: the stage stacks instead of floating. The canvas keeps a window's height of its own (it pans inside, WCAG 1.4.10's two-dimensional exception); the columns follow it in the page, which scrolls down. */
  @media (max-width: 1023px) {
    .frame {
      height: auto;
      min-height: 100vh;
      overflow: visible;
    }
    .frame.work {
      grid-template-rows: auto auto 0;
    }
    .stage {
      display: flex;
      flex-direction: column;
      overflow: visible;
    }
    main {
      flex: none;
      height: var(--h-canvas-narrow);
    }
    .overlay {
      grid-template-columns: minmax(0, 1fr);
      pointer-events: auto;
    }
    .col.end {
      align-items: stretch;
    }
    .grow,
    .mark-room {
      display: none;
    }
    .float.parts,
    .float.sheet {
      width: auto;
      overflow: visible;
    }
    .dock {
      overflow: visible;
    }
  }
</style>

