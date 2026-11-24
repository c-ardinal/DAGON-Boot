// The operating-mode session's link: the node connected to the PC, opened by the page and driven by
// dagon-host (through dagon-wasm), what each node answers in the watch, and the state machine's moves.
// Operate (operate.svelte.ts) builds updates, reconnects and recoveries on it.
import { hostLink, type LinkClock, type SwapLink } from '../serial/hostLink.ts';
import type { HelloResp } from '../serial/hello.ts';
import { isNote, notThisHub, type ReconnectNote, type Transport } from '../serial/transport.ts';
import { LOCAL, host_reconnect_ms, newHost, watch_interval_ms, type Host, type RecoverResult, type Refusal, type StatusResp, type Target, targetKey } from '../wasm/dagon.ts';
import type { RollbackRecord } from './agent.svelte.ts';
import { FAILURE_BAND } from './band.ts';
import { ConnectError, wasmReady } from './connect.ts';
import { ANNOUNCE } from './copy.ts';
import { HUB_LOCKED, WATCHED, next, type ConnectFailure, type Ctx, type Ev, type L } from './machine.ts';
import { SAMPLE_CONFIG, type Config, type Node } from './sample.ts';
import type { Topo } from './define/model.ts';
import { UNKNOWN, slotsSettled, writePercent, type Finish, type Live, type Progress, type ReleaseFile, type WriteTrack } from './writeProgress.ts';

/** States in which a node's write or recovery really goes on, so its % may rise with the clock. */
const WORKING: ReadonlySet<L> = new Set(['writing', 'hubRestarting', 'recovering']);

export interface Deps {
  /** Opens the device: the browser chooser, or a stand-in PC-connected node. Throws ConnectError. */
  open: () => Promise<Transport>;
  ready?: () => Promise<unknown>;
  /** Starts the 5 s monitoring timer (off in tests, which call `round`). */
  timer?: boolean;
  /** The user chooses the node connected to the PC again after its reboot (the browser chooser). Throws ConnectError. */
  reselect?: () => Promise<Transport>;
  /** Clock of dagon-host's timeouts (tests run it fast). */
  clock?: LinkClock;
  /**
   * The project's latest release through dagon serve (GET /v1/release/latest),
   * which "最新リリースの使用" and a recovery without a file take; null when
   * there is none. The page fetches nothing from anywhere else.
   */
  latest?: () => Promise<Uint8Array | null>;
  /** The signed boot core of node `id` the ROM path writes, through dagon serve; null when there is none. */
  bootcore?: (id: string) => Promise<Uint8Array | null>;
  /** stage0's bytes for the ROM path of an MCU with a stage0. */
  stage0?: (id: string) => Promise<Uint8Array | null>;
  /** Keeps the package about to be written in the project (POST /v1/kept); false when it was not kept. */
  keep?: (bytes: Uint8Array) => Promise<boolean>;
  /** The node's trial deadline (its MCU definition's `[watchdog] trial_ms`), for its %; null without one. */
  trialMs?: (id: string) => number | null;
  /** Opens the device the page had again, without the chooser (the CLI gave it back). Throws when it cannot. */
  again?: () => Promise<Transport>;
}

export class OperateLink {
  state = $state<L>('idle');
  failure = $state<ConnectFailure | undefined>();
  config = $state<Config | null>(null);
  /** The configuration whose definitions dagon-host holds (`tellConfig`); null: none (a new host holds none). */
  private told: string | null = null;
  /** Each node's target, as dagon-host gave it for the configuration it holds (`tellConfig`). */
  private targetsBy: Record<string, Target> = {};
  live = $state<Record<string, Live>>({});
  /** Nodes found by the last scan (PC-connected node included). */
  found = $state(0);
  /** `node:<id>` or `link:<to>`. */
  selected = $state<string | null>(null);
  status = $state('');
  alert = $state('');
  /** Bumped when a user action changed the band; the page moves focus then. */
  focusRequest = $state(0);
  file = $state<ReleaseFile | null>(null);
  refusal = $state<Refusal | undefined>();
  /** Node ids the update runs on, in config order. */
  targets = $state<string[]>([]);
  progress = $state<Record<string, Progress>>({});
  /** The node the update stopped at. */
  failed = $state<string | null>(null);
  /** Clock time when dagon-host stops reopening the rebooted PC-connected node. */
  reconnectUntil = $state<number | null>(null);
  /** The node being recovered or rolled back (and tried again from recoverFailed). */
  recovering = $state<string | null>(null);
  /** How the last recovery got through: the ROM path shows its figures in the detail. */
  recovered = $state<RecoverResult | null>(null);
  /** The update after a recovery covers the nodes not written yet ("未書込ノードへの書込"). */
  rest = $state(false);
  /**
   * The 構成 a release made for another 構成 was made for (otherTopology): shown
   * as differences, and taken in 定義 by "ファイルの構成の取込". Null when
   * the file carries none.
   */
  fileTopology = $state<Topo | null>(null);
  /** "最新リリースの使用" found no release (BAND.noRelease until the next step). */
  latestMissing = $state(false);
  /** The version the chosen release gives each node (shown as "↑ 1.0.0 → 1.1.0" before the update). */
  fileVersions = $state<Record<string, number>>({});
  /** Date.now() when the running update or recovery started (elapsed time); null when none runs. */
  startedAt = $state<number | null>(null);
  /** Why the page could not get the rebooted PC-connected node back by itself (reconnectNeeded's diagnostic log). */
  reconnectNotes = $state<ReconnectNote[]>([]);
  /** Why the device the CLI gave back was not taken (lost shows it); null otherwise. */
  returnRefused = $state<ReconnectNote | null>(null);
  /** The PC-connected node's HELLO when the user connected: what "the same node" means after its reboot. */
  private hubHello: HelloResp | null = null;
  /** Nodes whose card check is off: the next write leaves them out. */
  skip = $state(new Set<string>());
  /** Each node's write so far, for its one % (see writePercent). */
  track = $state<Record<string, WriteTrack>>({});
  /** When the work last stopped (clock time); Infinity while it runs. */
  private stillSince = $state(Infinity);
  /** The package being written could not be kept in the project (it is written all the same). */
  keepFailed = $state(false);
  /** What the last update, recovery or rollback left (done). */
  finished = $state<Finish | null>(null);
  protected backRecord = $state.raw<RollbackRecord | null>(null);
  protected backRead: Promise<void> = Promise.resolve();
  /** The node the write stopped at is back on its previous version. */
  get wentBack(): boolean {
    return !!this.failed && this.progress[this.failed]?.stage === 'previous';
  }
  /** The node the write stopped at is back on its confirmed boot core. */
  get coreWentBack(): boolean {
    return !!this.failed && this.progress[this.failed]?.stage === 'coreBack';
  }
  /** The record of the write that went back, while the node stays stopped on it; else null. */
  get rollbackRecord(): RollbackRecord | null {
    return this.wentBack ? this.backRecord : null;
  }
  /** The record once the node's STATUS was read or missed: its node_state null then means it did not answer. */
  async settledRollbackRecord(): Promise<RollbackRecord | null> {
    await this.backRead;
    return this.rollbackRecord;
  }
  /** Nodes on the chosen release's version since it was chosen (an update, its rest, a recovery). */
  protected updated = new Set<string>();
  /** Nodes this run really wrote an image to; the others among `updated` were already on it. */
  protected wrote = new Set<string>();
  /** Nodes of the last update that answered nothing and were sent nothing (dagon-host's needsFirstWrite). */
  protected firstWrite = 0;
  /** The run hubRestarting / reconnectNeeded belong to, so done says what it did. */
  protected running: 'update' | 'recover' | 'rollback' = 'update';

  protected link: SwapLink | null = null;
  /** Version each node was given in this run. */
  protected sentVersion: Record<string, number> = {};
  /** The boot core version sent to each node in this run. */
  protected coreSent: Record<string, number> = {};

  protected transport: Transport | null = null;
  protected host: Host | null = null;
  private chain: Promise<unknown> = Promise.resolve();
  private watching = false;
  private timer: ReturnType<typeof setInterval> | undefined;
  /** Bumped per connection; results of an older one are dropped. */
  protected session = 0;

  constructor(protected readonly deps: Deps) {}

  get ctx(): Ctx {
    return { hasConfig: this.config !== null, failure: this.failure, refusal: this.refusal, answering: this.needUpdate > 0 };
  }

  /**
   * The nodes updating reaches, as the checked cut lists them (config.reach):
   * the compiled topology's nodes, so a node's topology index is its place
   * here. The others are drawn but never watched, updated or recovered.
   */
  get reach(): Node[] {
    const c = this.config;
    return c ? c.reach.flatMap((id) => c.nodes.filter((n) => n.id === id)) : [];
  }

  /** Answering nodes not yet on the chosen release's version (all answering ones before an update ran). */
  get needUpdate(): number {
    return this.reach.filter((n) => this.live[n.id]?.responding === true && !this.updated.has(n.id)).length;
  }

  /** What node `id` answers now; null when it cannot be read. */
  protected async statusOf(id: string): Promise<{ hello: HelloResp; status: StatusResp } | null> {
    const n = this.reach.find((x) => x.id === id);
    if (!n) return null;
    return this.call((h) => h.status(this.at(n))).catch(() => null);
  }

  /** The version of the boot core node `id` runs now; null when it cannot be read, or another waits for the next boot. */
  protected async runningCore(id: string): Promise<number | null> {
    const n = this.reach.find((x) => x.id === id);
    if (!n) return null;
    const core = await this.call((h) => h.bootCoreStatus(this.at(n))).catch(() => null);
    return core && slotsSettled(core) ? (core.slots[core.active]?.version ?? null) : null;
  }

  get responding(): number {
    return this.reach.filter((n) => this.live[n.id]?.responding === true).length;
  }

  /** The device the session runs on now (the page's disconnect handling compares ports with it). */
  get device(): Transport | null {
    return this.transport;
  }

  /** The first node in update order that stopped answering. */
  get stuck(): Node | undefined {
    const reach = this.reach;
    return this.config?.order.map((id) => reach.find((n) => n.id === id)).find((n) => n && this.live[n.id]?.responding === false);
  }

  protected go(ev: Ev, focus = true): void {
    const to = next(this.state, ev);
    if (to === this.state && ev !== 'placeSample') return;
    this.state = to;
    this.stillSince = WORKING.has(to) ? Infinity : Math.min(this.stillSince, this.now());
    this.latestMissing = false;
    // Out of the PC-connected node's reboot (confirmed, stopped, lost, ...): no version to keep a reopened device by any more.
    if (to !== 'hubRestarting' && to !== 'reconnectNeeded') this.transport?.expectHub?.(null);
    if (focus) this.focusRequest++;
  }

  /** `dagon serve` needs the PC-connected node's port (device.need): close Web Serial and show cliTurn. */
  async handOver(): Promise<void> {
    this.session++;
    await this.drop();
    this.go('cliClaim', false);
  }

  /**
   * The header's 切断: the port is closed but its permission is kept (no
   * forget()), so the CLI or a later 接続 can open it; the session waits at idle.
   */
  async disconnect(): Promise<void> {
    if (HUB_LOCKED.has(this.state) || next(this.state, 'disconnect') === this.state) return;
    this.session++;
    await this.drop();
    this.alert = '';
    this.status = ANNOUNCE.hubLost;
    this.go('disconnect', false);
  }

  /**
   * The CLI gave the port back (device.returned): the page opens the device
   * it had again by itself and goes on watching (ready, BAND.cliTurn); when that fails
   * the user connects again (lost, BAND.lost). It must be the same PC-connected node as before
   * (notThisHub against the first HELLO); otherwise it is closed and the
   * reason goes to the log.
   */
  async cliReturned(): Promise<void> {
    if (this.state !== 'cliTurn') return;
    const my = ++this.session;
    this.reconnectNotes = [];
    this.returnRefused = null;
    try {
      if (!this.deps.again) throw new Error('nothing to reopen');
      const t = await this.deps.again();
      if (my !== this.session) return void t.close().catch(() => undefined);
      this.transport = t;
      this.link = hostLink(t, this.deps.clock);
      this.host = newHost(this.link);
      this.told = null;
      const h = await this.call((x) => x.hello(LOCAL));
      const why = this.notThisHub(h, false);
      if (why) throw why;
      this.hubHello ??= h;
    } catch (e) {
      if (my !== this.session) return;
      if (isNote(e)) {
        this.noteWhy(e);
        this.returnRefused = e;
      }
      await this.drop();
      this.alert = ANNOUNCE.hubLost;
      return this.go('lost', false);
    }
    if (my !== this.session) return;
    this.status = ANNOUNCE.hubConnected;
    this.go('cliReturn', false);
    this.startWatching();
  }

  /** A configuration made in define mode takes the canvas (not sent yet). */
  useConfig(c: Config): void {
    if (this.config?.yaml === c.yaml) return;
    this.config = c;
    this.live = Object.fromEntries(c.nodes.map((n) => [n.id, this.live[n.id] ?? UNKNOWN]));
  }

  placeSample(): void {
    this.config = SAMPLE_CONFIG;
    this.live = Object.fromEntries(SAMPLE_CONFIG.nodes.map((n) => [n.id, UNKNOWN]));
    this.go('placeSample', false);
  }

  /**
   * "接続" / "PC接続ノードの探索" / "PC接続ノードの再選択" / "再接続".
   * With nothing on the canvas, connecting goes on to find the nodes.
   */
  async connect(): Promise<void> {
    this.returnRefused = null;
    if (this.state === 'choosing') return;
    const discover = !this.config;
    const my = ++this.session;
    await this.drop();
    this.alert = '';
    this.failure = undefined;
    this.go(this.state === 'silentHub' ? 'reselect' : this.state === 'lost' ? 'reconnect' : 'connect');
    if (!(await wasmReady(this.deps.ready))) return this.fail(my, 'wasmFailed', 'unavailable');
    let t: Transport;
    try {
      t = await this.deps.open();
    } catch (e) {
      const f = e instanceof ConnectError ? e.failure : 'busy';
      return this.fail(my, f === 'not-found' ? 'dismissed' : 'openFailed', f);
    }
    if (my !== this.session) return void t.close().catch(() => undefined);
    this.transport = t;
    this.link = hostLink(t, this.deps.clock);
    this.host = newHost(this.link);
    this.told = null;
    this.go('chosen');
    try {
      this.hubHello = await this.call((h) => h.hello(LOCAL));
    } catch (e) {
      await this.drop();
      // dagon-host's reason for an answer in another wire version (dagon_host::reason).
      const wire = e instanceof Error && e.message === 'WIRE VERSION';
      return this.fail(my, 'noAnswer', wire ? 'wire' : 'no-response');
    }
    if (my !== this.session) return;
    this.status = ANNOUNCE.hubConnected;
    if (discover) return this.discover(my);
    this.go('helloConfigured');
    this.startWatching();
  }

  private fail(my: number, ev: Ev, f: ConnectFailure): void {
    if (my !== this.session) return;
    this.failure = f;
    this.go(ev);
    this.status = FAILURE_BAND[f].chip;
  }

  /** The scan after "PC接続ノードの探索". */
  async discover(my = this.session): Promise<void> {
    this.go('helloDiscover');
    this.stopWatching();
    this.found = 1;
    if (!this.config) this.placeSample();
    try {
      const d = await this.call((h) => h.discover());
      if (my !== this.session) return;
      const seen = new Map(d.nodes.map((n) => [targetKey(n.target), n.hello]));
      seen.set(targetKey(LOCAL), d.hub);
      const now = Date.now();
      // places the sample and marks which of its nodes answered; a
      // scan result that is not the sample needs the definition mode.
      this.live = Object.fromEntries(
        this.config!.nodes.map((n) => {
          const to = this.targetsBy[n.id];
          const h = (to && seen.get(targetKey(to))) ?? null;
          return [n.id, h ? { ...UNKNOWN, responding: true, hello: h, lastSeen: now } : UNKNOWN];
        }),
      );
      this.found = seen.size;
      this.status = ANNOUNCE.found(this.found);
      this.go('scanned');
      this.startWatching();
    } catch {
      if (my === this.session) this.lose();
    }
  }

  select(key: string | null): void {
    this.selected = key;
    const id = key?.startsWith('node:') ? key.slice(5) : null;
    if (id && this.host && WATCHED.has(this.state)) void this.refreshStatus(id);
  }

  /** The device went away (USB unplugged). The node connected to the PC leaving for its reboot is not a loss. */
  lose(): void {
    if (this.state === 'hubRestarting' || this.state === 'reconnectNeeded') return;
    this.session++;
    void this.drop();
    const was = this.state;
    this.go('lost');
    if (this.state !== was) this.alert = ANNOUNCE.hubLost;
  }

  /** One monitoring round: HELLO to every node, then STATUS of the selected one. */
  async round(): Promise<void> {
    const c = this.config;
    if (!c || !this.host || this.watching || !WATCHED.has(this.state)) return;
    this.watching = true;
    const my = this.session;
    const before = this.responding;
    try {
      for (const n of this.reach) {
        const r = await this.call((h) => h.watch(this.at(n)));
        if (my !== this.session) return;
        const was = this.live[n.id] ?? UNKNOWN;
        this.live[n.id] = {
          ...was,
          responding: r.responding,
          rttMs: r.rttMs ?? was.rttMs,
          hello: r.hello ?? was.hello,
          lastSeen: r.hello ? Date.now() : was.lastSeen,
        };
        if (was.responding !== false && !r.responding) this.alert = ANNOUNCE.nodeSilent(n.name);
      }
      const id = this.selected?.startsWith('node:') ? this.selected.slice(5) : null;
      if (id) await this.refreshStatus(id);
    } catch {
      if (my === this.session) this.lose();
      return;
    } finally {
      this.watching = false;
    }
    if (this.responding !== before) this.status = ANNOUNCE.responding(this.responding, this.reach.length);
    // ready and done move to nodesSilent by themselves while a node does not answer: it stays visible and recoverable
    // (after "応答するnノードへの優先書込" the ones left out are named there).
    if ((this.state === 'ready' || this.state === 'done') && this.stuck) this.go('watchAlarm', false);
    else if (this.state === 'nodesSilent' && !this.stuck) this.go('watchOk', false);
  }

  protected async refreshStatus(id: string): Promise<void> {
    const n = this.reach.find((x) => x.id === id);
    if (!n || this.live[id]?.responding === false) return;
    try {
      const s = await this.call((h) => h.status(this.at(n)));
      // A node without boot core slots refuses this STATUS: no row then.
      const core = await this.call((h) => h.bootCoreStatus(this.at(n))).catch(() => null);
      // An agent that predates VERSION answers null: its versions show as unknown.
      const version = await this.call((h) => h.version(this.at(n))).catch(() => null);
      this.live[id] = { ...(this.live[id] ?? UNKNOWN), hello: s.hello, status: s.status, core, version };
    } catch {
      // A missed STATUS shows the last values; the next watch round decides answering or not.
    }
  }

  /**
   * Progress at `now`, 0–100: the node being recovered or rolled back, else the nodes' one % averaged over the
   * write's targets. Like theirs, it never moves back while a run lasts and reaches 100 only when all confirmed.
   */
  overallAt(now: number): number {
    const ids = this.recovering !== null && this.running !== 'update' ? [this.recovering] : this.targets;
    const p = ids.map((id) => this.percentOf(id, now));
    return p.length ? Math.floor(p.reduce((a, b) => a + b, 0) / p.length) : 0;
  }

  /** The nodes an update runs on: those answering now and checked on their cards (the node connected to the PC alone when only it answers). */
  protected answering(): string[] {
    return this.reach.filter((n) => this.live[n.id]?.responding === true && !this.skip.has(n.id)).map((n) => n.id);
  }

  /** Node `id`'s one % for its write or recovery, at the clock's `now`; time stops counting when the work stops. */
  percentOf(id: string, now = this.now()): number {
    const k = this.track[id];
    return k ? writePercent(k, Math.min(now, this.stillSince), this.deps.trialMs?.(id) ?? null) : 0;
  }

  /** Node `n` is the one the PC is connected to, as dagon-host's targets say. */
  protected isLocal(n: Node): boolean {
    return this.targetsBy[n.id]?.kind === 'local';
  }

  /** Node `n`'s target as dagon-host gave it; throws (the call fails, nothing is sent) when no target reaches it. */
  protected at(n: Node): Target {
    const to = this.targetsBy[n.id];
    if (!to) throw new Error(`no target reaches ${n.id}`);
    return to;
  }

  /** The node connected to the PC reboots onto `version`: dagon-host reopens it for host_reconnect_ms (hubRestarting), keeping only that PC-connected node. */
  protected hubAway(version: number | undefined): void {
    if (version !== undefined) this.transport?.expectHub?.(version, this.hubHello);
    this.reconnectNotes = [];
    this.reconnectUntil = this.now() + host_reconnect_ms();
    this.status = ANNOUNCE.hubRestarting;
    this.go('hubTurn');
  }

  /** dagon-host gave up reopening the node connected to the PC (reconnectNeeded): the dialog opens, with why. */
  protected autoFailed(): void {
    this.reconnectNotes = this.transport?.notes?.() ?? [];
    this.alert = ANNOUNCE.reconnectTimeout;
    this.go('autoFailed');
  }

  /** The clock dagon-host's reconnect window runs on. */
  now(): number {
    return (this.deps.clock ?? performance).now();
  }

  /**
   * Why the device under the link is not the node connected to the PC the update rebooted, or null.
   * HELLO carries no serial number, so "the same node" is the role, MCU and
   * release key of its first HELLO; `onTrial` (a device nobody chose) also
   * needs it on trial with the version just sent. One the user chose may be
   * back on its previous version: CONFIRM then stops there (stopped).
   */
  protected notThisHub(h: HelloResp, onTrial: boolean): ReconnectNote | null {
    // -1 matches no version, so a device nobody chose is never kept when nothing was sent.
    return notThisHub(h, this.hubHello, onTrial ? (this.hubSent() ?? -1) : null);
  }

  /** `t` replaces the device under the link; the one it replaces is closed. */
  protected swap(t: Transport, old = this.transport): void {
    this.link?.use(t);
    this.transport = t;
    if (old && old !== t) void old.close().catch(() => undefined);
  }

  /** The version just sent to the node connected to the PC, if any. */
  private hubSent(): number | null {
    const hub = this.reach.find((n) => this.isLocal(n));
    return hub ? (this.sentVersion[hub.id] ?? null) : null;
  }

  protected noteWhy(why: ReconnectNote): void {
    if (!this.reconnectNotes.some((x) => JSON.stringify(x) === JSON.stringify(why))) this.reconnectNotes = [...this.reconnectNotes, why];
  }

  /**
   * "リスタート": the page's records go (connection, file,
   * progress) and the session waits at idle; the project's files are untouched.
   */
  reset(): void {
    this.returnRefused = null;
    this.session++;
    void this.drop();
    this.state = 'idle';
    this.failure = undefined;
    this.config = null;
    this.live = {};
    this.found = 0;
    this.selected = null;
    this.status = '';
    this.alert = '';
    this.file = null;
    this.refusal = undefined;
    this.targets = [];
    this.progress = {};
    this.failed = null;
    this.reconnectUntil = null;
    this.recovering = null;
    this.recovered = null;
    this.rest = false;
    this.fileTopology = null;
    this.latestMissing = false;
    this.fileVersions = {};
    this.startedAt = null;
    this.reconnectNotes = [];
    this.finished = null;
    this.keepFailed = false;
    this.skip = new Set();
    this.track = {};
    this.updated.clear();
    this.sentVersion = {};
  }

  /**
   * Before a call: dagon-host gets the configuration's definitions (each node's limits, how each linked node is reached)
   * before the first call of a configuration and again when it changes. A configuration whose definitions do not load
   * throws, so the call fails with that error and sends nothing; without a configuration the host reaches only the
   * connected node.
   */
  private tellConfig(h: Host): void {
    const yaml = this.config?.yaml ?? null;
    if (this.told === yaml) return;
    if (yaml === null) h.clearTimings();
    this.targetsBy = yaml === null ? {} : h.setTimings(yaml);
    this.told = yaml;
  }

  /** dagon-host runs one call at a time; calls queue here. */
  protected call<T>(f: (h: Host) => Promise<T>): Promise<T> {
    const host = this.host;
    if (!host) return Promise.reject(new Error('not connected'));
    const run = this.chain.then(() => {
      this.tellConfig(host);
      return f(host);
    });
    this.chain = run.catch(() => undefined);
    return run;
  }

  protected startWatching(): void {
    if (!this.deps.timer || this.timer !== undefined) return;
    void this.round();
    this.timer = setInterval(() => void this.round(), watch_interval_ms());
  }

  protected stopWatching(): void {
    clearInterval(this.timer);
    this.timer = undefined;
  }

  private async drop(): Promise<void> {
    this.stopWatching();
    const t = this.transport;
    this.transport = null;
    this.host?.free();
    this.host = null;
    this.link = null;
    await t?.close().catch(() => undefined);
  }
}
