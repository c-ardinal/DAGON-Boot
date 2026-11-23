// The open project on the page: the start screen until one opens, then 定義 and 運用 over its
// 構成, saved by itself to the project's YAML, with its names and card positions in the layout file.
import { tick, untrack } from 'svelte';
import { newView, type View } from '../../graph/Graph.svelte';
import { dagonReady, require_release_key, set_release_key, useDefinitions } from '../../wasm/dagon.ts';
import type { Agent } from '../agent.svelte.ts';
import { ANNOUNCE, LAYOUT, TOPOLOGY_READ } from '../copy.ts';
import { Autosave } from '../define/autosave.svelte.ts';
import type { Cli } from '../define/cli.svelte.ts';
import type { Define, Operable } from '../define/define.svelte.ts';
import { LayoutFile } from '../define/layoutFile.ts';
import { fromYaml } from '../define/model.ts';
import type { Control } from '../locks.ts';
import type { Operate } from '../operate.svelte.ts';
import { SAMPLE_CONFIG, SAMPLE_YAML } from '../sample.ts';
import { Start, type StartChoice } from '../start.svelte.ts';
import type { Feedback } from './feedback.svelte.ts';

export type Mode = 'operate' | 'define';

export interface WorkspaceDeps {
  cli: Cli;
  d: Define;
  op: Operate;
  agent: Agent;
  feedback: Feedback;
  /** Development stand-ins (?mock, ?sim) have no serve: the start screen starts an unsaved 構成 on the page. */
  dev: boolean;
  /** The public demo page: it has no serve behind it and never calls one. */
  demoSite: boolean;
  stopped: (c: Control) => boolean;
}

/** The recent projects the start screen lists. */
const RECENT_SHOWN = 5;

export class Workspace {
  /** The start screen until a project is open; then 定義 and 運用. */
  screen = $state<'start' | 'work'>('start');
  /** 運用 / 定義. The define document goes to the operate canvas when leaving define mode. */
  mode = $state<Mode>('define');
  /** Development stand-ins (?mock, ?sim without serve): an unsaved 構成 on the page, nothing to save to. */
  local = $state(false);
  /** The menu's popover, and リスタート's confirmation. */
  menuOpen = $state(false);
  restartOpen = $state(false);
  /** The sheet's CLI panel (定義), null while it shows the selection. */
  cliPanel = $state<'link' | null>(null);
  /** The canvas's zoom and pan, the user's: kept across modes and views, new for each project. */
  canvasView = $state<View>(newView());
  /** Where the user put cards, kept in the project's layout file. */
  positions = $state.raw<Record<string, { x: number; y: number }>>({});
  private locked = $state<{ why: Operable | 'unsaved'; at: number } | null>(null);
  /** 運用 pressed while it cannot be entered: MODE.lockedProblems (or 未保存) in the band until the 構成 changes. */
  readonly lockShown = $derived(this.locked && this.locked.at === this.deps.d.revision ? this.locked.why : null);

  readonly start: Start;
  /** Edits go to the project's YAML by themselves; 保存 saves at once. */
  readonly autosave: Autosave;
  private readonly layoutFile: LayoutFile;
  private layoutNames = '';

  /** Call it where an effect may be made (a component's script); it connects to serve at once. */
  constructor(private readonly deps: WorkspaceDeps) {
    const { cli, d } = deps;
    this.start = new Start({
      cli,
      // serve switched to the project's keys (Cli refreshed hello): the page verifies with that key from now on.
      opened: async () => {
        this.useReleaseKey();
        this.useServedDefinitions(await cli.definitions());
        await this.openWork();
      },
      local: deps.dev ? (c) => this.startLocal(c) : null,
    });
    this.autosave = new Autosave(d, {
      put: (yaml) => cli.saveTopology(yaml),
      enabled: () => this.screen === 'work' && !this.local,
      blocked: () => cli.defsFailed || d.notRead,
    });
    // The project's layout file; names follow the edits.
    this.layoutFile = new LayoutFile(cli, (read) => {
      d.adoptNames(read.names ?? {});
      this.layoutNames = JSON.stringify(d.doc.names);
      this.positions = { ...read.positions, ...this.positions };
    });
    $effect(() => {
      const names = JSON.stringify(d.doc.names);
      // With no 構成 read, the editor knows none of the file's nodes: its names say nothing about them.
      if (this.screen !== 'work' || this.local || d.notRead) return;
      untrack(() => {
        if (names === this.layoutNames) return;
        this.layoutNames = names;
        void this.layoutFile.change(() => ({ names: { ...d.doc.names } }));
      });
    });
    $effect(() => {
      if (d.selected) this.cliPanel = null;
    });
    // `#mode=operate` / `#mode=define`: the mode a reload or a shared URL comes back to.
    $effect(() => {
      if (this.screen === 'work') history.replaceState(history.state, '', `${location.pathname}${location.search}#mode=${this.mode}`);
    });
    $effect(() => {
      void d.revision;
      this.autosave.edited();
    });
    $effect(() => {
      const warn = (e: BeforeUnloadEvent) => {
        if (!this.autosave.pending) return;
        void this.autosave.now();
        e.preventDefault();
      };
      addEventListener('beforeunload', warn);
      return () => removeEventListener('beforeunload', warn);
    });
    void this.connect();
  }

  /**
   * The release key: dagon serve writes the developer's key into the page (meta) and /v1/hello;
   * a page it serves verifies with that key only, and with none until it is there. A project
   * open in serve opens at once; without one the page stays on the start screen.
   */
  private async connect(): Promise<void> {
    const { cli, agent } = this.deps;
    const want = /(?:^|[#&])mode=(operate|define)\b/.exec(globalThis.location?.hash ?? '')?.[1] as Mode | undefined;
    try {
      if (!this.deps.demoSite) await cli.connect();
      await dagonReady();
      if (cli.state === 'none') return;
      require_release_key();
      this.useReleaseKey(document.querySelector<HTMLMetaElement>('meta[name="dagon-release-key"]')?.content);
      if (cli.state !== 'ready') return;
      void agent.refresh();
      // The project's MCU and board definitions; the builtin set when serve has none (or no serve).
      this.useServedDefinitions(await cli.definitions());
      if (cli.has('project')) {
        const p = await cli.project();
        this.start.recent = (p.recent ?? []).slice(0, RECENT_SHOWN);
        if (p.open) await this.openWork(want);
      } else if (await cli.loadTopology()) {
        await this.openWork(want);
      }
    } catch {
      // No serve, or it went away: the start screen says so.
    } finally {
      this.start.loading = false;
    }
  }

  /** The key releases are verified with: serve's (hello, or the page's meta at first); never a secret. */
  private useReleaseKey(meta?: string): void {
    const key = this.deps.cli.hello?.release_key ?? meta;
    if (key) set_release_key(key);
  }

  /** The project's definitions, or none at all with × 定義エラー when they fail (never the builtin set instead). */
  private useServedDefinitions(json: string | null): void {
    const { cli } = this.deps;
    if (useDefinitions(json, cli.defsFailed) === 'blocked') cli.defsFailed = true;
    // Blocked: a save waiting for its quiet time is dropped (none is sent while blocked).
    if (cli.defsFailed) this.autosave.cancel();
  }

  /** The open project's 構成 (its YAML through serve): 運用 when it has no problems, else 定義. */
  // The start screen is reached only through `leave` (menu, リスタート), so no unsaved edit is here when a project opens.
  private async openWork(want?: Mode): Promise<void> {
    const { cli, d, op, feedback } = this.deps;
    const yaml = await cli.loadTopology().catch(() => undefined);
    // 表示名 and positions live beside the YAML (GET /v1/layout).
    if (!(await this.layoutFile.open())) feedback.notice = { word: LAYOUT.refused, error: true, detail: `${LAYOUT.refusedDetail}（${cli.layoutRefused}）` };
    this.local = false;
    op.reset();
    d.reset();
    if (yaml) {
      try {
        d.load(fromYaml(yaml, this.layoutFile.current.names ?? {}), yaml);
      } catch {
        d.unreadable = true;
        d.notRead = true;
      }
    } else if (yaml === undefined) {
      d.notRead = true;
      feedback.notice = { word: TOPOLOGY_READ.label, error: true, detail: TOPOLOGY_READ.detail };
    }
    this.layoutNames = JSON.stringify(d.doc.names);
    this.positions = this.layoutFile.current.positions ?? {};
    this.canvasView = newView();
    this.enterWork(want);
  }

  private enterWork(want?: Mode): void {
    const { d } = this.deps;
    this.start.clear();
    this.screen = 'work';
    this.menuOpen = false;
    const m = want !== 'define' && d.operable === 'ok' ? 'operate' : 'define';
    this.mode = 'define';
    this.switchMode(m);
    // #mode=operate on a 構成 that cannot be operated: 定義 with the reason.
    if (want === 'operate' && m === 'define') this.lock(d.operable);
  }

  /** Without serve (development stand-ins): an empty 構成 or the sample, not saved. */
  private startLocal(c: 'new' | 'sample'): void {
    const { d, op } = this.deps;
    this.local = true;
    op.reset();
    d.reset();
    this.positions = {};
    this.canvasView = newView();
    if (c === 'sample') d.load(fromYaml(SAMPLE_YAML, Object.fromEntries(SAMPLE_CONFIG.nodes.map((n) => [n.id, n.name]))), SAMPLE_YAML);
    this.enterWork('define');
  }

  /** Menu プロジェクト作成 (空白から作成 / サンプルから作成) / プロジェクトのオープン: the 構成 is saved first (or the dialog asks). */
  toStart(c: StartChoice): void {
    if (this.deps.stopped(this.deps.demoSite && c === 'open' ? 'open' : 'project')) return;
    this.menuOpen = false;
    void this.autosave.leave(() => {
      this.cliPanel = null;
      this.screen = 'start';
      void this.start.refresh();
      void this.start.choose(c);
    });
  }

  /** "リスタート": the page's records go and the start screen opens; the project's files stay. */
  restart(): void {
    if (this.deps.stopped('project')) return void (this.restartOpen = false);
    this.restartOpen = false;
    this.menuOpen = false;
    void this.autosave.leave(() => {
      const { cli, d, op } = this.deps;
      this.local = false;
      op.reset();
      d.reset();
      cli.clearJobs();
      this.cliPanel = null;
      this.start.clear();
      this.mode = 'define';
      this.screen = 'start';
      void this.start.refresh();
    });
  }

  cancelRestart(): void {
    this.restartOpen = false;
    void tick().then(() => document.querySelector<HTMLElement>('[aria-controls="menu"]')?.focus());
  }

  lock(why: Operable | 'unsaved'): void {
    this.locked = { why, at: this.deps.d.revision };
  }

  switchMode(m: Mode): void {
    const { d } = this.deps;
    // 運用 only with a 構成 that has no problems (parts updating skips do not stop it); the YAML decides, never the node connected to the PC.
    if (m === 'operate' && d.operable !== 'ok') return this.lock(d.operable);
    // ... and only the saved one, the 構成 build, sign and release use too.
    if (m === 'operate' && this.autosave.pending) {
      void this.autosave.ready().then((ok) => (ok ? this.enterMode(m) : this.lock('unsaved')));
      return;
    }
    this.enterMode(m);
  }

  private enterMode(m: Mode): void {
    const { d, op } = this.deps;
    // A file checked against another 構成 (fileRefused, otherTopology or fileReady, e.g. after "ファイルの構成の取込") is checked again.
    const recheck = m === 'operate' && op.file !== null && ['fileRefused', 'otherTopology', 'fileReady'].includes(op.state) && op.config?.yaml !== d.config.yaml;
    if (m === 'operate') op.useConfig(d.config);
    this.mode = m;
    d.status = ANNOUNCE.mode(m === 'define');
    if (recheck && op.file) op.takeFile(op.file.name, op.file.bytes);
  }

  /** "保存": the 構成 into the project's YAML now. */
  async save(): Promise<void> {
    await this.autosave.now();
  }

  savePositions(): void {
    if (this.local) return;
    void this.layoutFile.change(() => ({ positions: this.positions }));
  }

  /** 整列: the cards go back to their computed places. */
  arrange(): void {
    this.positions = {};
    this.savePositions();
  }
}
