// Operating-mode session: drives the state machine with what the user does
// and what dagon-host (through dagon-wasm) reports. The page only chooses and
// opens the device; every request, timeout and retry is dagon-host's.
import { hostLink, type LinkClock, type SwapLink } from '../serial/hostLink.ts';
import type { HelloResp } from '../serial/hello.ts';
import type { Transport } from '../serial/transport.ts';
import {
  PHASE,
  check_release_for,
  newHost,
  topo_compile,
  versionText,
  watch_interval_ms,
  type Host,
  type NodeResult,
  type Refusal,
  type StatusResp,
  type UpdateEvent,
} from '../wasm/dagon.ts';
import { FAILURE_BAND } from './band.ts';
import { ConnectError, wasmReady } from './connect.ts';
import { ANNOUNCE, FILE } from './copy.ts';
import { WATCHED, next, type ConnectFailure, type Ctx, type Ev, type L } from './machine.ts';
import { DESK_DEMO, targetKey, type Config, type Node } from './sample.ts';

/** What the page knows about one node from the hub. */
export interface Live {
  /** null until the first answer or miss. */
  responding: boolean | null;
  rttMs: number | null;
  /** Date.now() of the last answer. */
  lastSeen: number | null;
  hello: HelloResp | null;
  status: StatusResp | null;
}

const UNKNOWN: Live = { responding: null, rttMs: null, lastSeen: null, hello: null, status: null };

/** Where one node is in an update (the node chip and the update-order row). */
export interface Progress {
  stage: 'waiting' | 'writing' | 'rebooting' | 'done' | 'silent' | 'refused' | 'previous' | 'unwritten';
  percent: number;
  /** dagon-host's reason when the node refused (`E_FLASH`, `E_SIGNATURE`, ...). */
  reason?: string;
}

/** The chosen release and dagon-host's verdict on it. */
export interface ReleaseFile {
  name: string;
  size: number;
  bytes: Uint8Array;
  verdict: 'checking' | 'pass' | Refusal;
}

/** dagon-host waits this long for the rebooted hub (HUB_RECONNECT_MS). */
export const HUB_RECONNECT_MS = 45_000;
const REFUSALS: readonly string[] = ['source', 'key', 'topology', 'format'];

export interface Deps {
  /** Opens the device: the browser chooser, or a stand-in hub. Throws ConnectError. */
  open: () => Promise<Transport>;
  ready?: () => Promise<unknown>;
  /** Starts the 5 s monitoring timer (off in tests, which call `round`). */
  timer?: boolean;
  /** The user chooses the hub again after its reboot (the browser chooser). Throws ConnectError. */
  reselect?: () => Promise<Transport>;
  /** Clock of dagon-host's timeouts (tests run it fast). */
  clock?: LinkClock;
}

export class Operate {
  state = $state<L>('L02');
  failure = $state<ConnectFailure | undefined>();
  config = $state<Config | null>(null);
  live = $state<Record<string, Live>>({});
  /** This page sent the configuration on the canvas to the hub. */
  sent = $state(false);
  /** Nodes found by the last scan (hub included). */
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
  /** Clock time when dagon-host stops reopening the rebooted hub. */
  reconnectUntil = $state<number | null>(null);

  private link: SwapLink | null = null;
  /** Version each node was given in this run. */
  private sentVersion: Record<string, number> = {};

  private transport: Transport | null = null;
  private host: Host | null = null;
  private chain: Promise<unknown> = Promise.resolve();
  private watching = false;
  private timer: ReturnType<typeof setInterval> | undefined;
  /** Bumped per connection; results of an older one are dropped. */
  private session = 0;

  constructor(private readonly deps: Deps) {}

  get ctx(): Ctx {
    return { hasConfig: this.config !== null, failure: this.failure, refusal: this.refusal };
  }

  get responding(): number {
    return this.config?.nodes.filter((n) => this.live[n.id]?.responding === true).length ?? 0;
  }

  /** The first node in update order that stopped answering. */
  get stuck(): Node | undefined {
    const c = this.config;
    return c?.order.map((id) => c.nodes.find((n) => n.id === id)!).find((n) => this.live[n.id]?.responding === false);
  }

  private go(ev: Ev, focus = true): void {
    const to = next(this.state, ev);
    if (to === this.state && ev !== 'placeSample') return;
    this.state = to;
    if (focus) this.focusRequest++;
  }

  placeSample(): void {
    this.config = DESK_DEMO;
    this.sent = false;
    this.live = Object.fromEntries(DESK_DEMO.nodes.map((n) => [n.id, UNKNOWN]));
    this.go('placeSample', false);
  }

  /**
   * "パソコンとつなぐ" / "つないで、自動で見つける" / "選び直す" / "もう一度つなぐ".
   * With nothing on the canvas, connecting goes on to find the nodes.
   */
  async connect(): Promise<void> {
    if (this.state === 'L03') return;
    const discover = !this.config;
    const my = ++this.session;
    await this.drop();
    this.alert = '';
    this.failure = undefined;
    this.go(this.state === 'L07' ? 'reselect' : this.state === 'L26' ? 'reconnect' : 'connect');
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
    this.go('chosen');
    try {
      await this.call((h) => h.hello('hub', 0));
    } catch {
      await this.drop();
      return this.fail(my, 'noAnswer', 'no-response');
    }
    if (my !== this.session) return;
    this.status = ANNOUNCE.A05;
    if (discover) return this.discover(my);
    this.go(this.sent ? 'helloConfigured' : 'helloUnsent');
    this.startWatching();
  }

  private fail(my: number, ev: Ev, f: ConnectFailure): void {
    if (my !== this.session) return;
    this.failure = f;
    this.go(ev);
    this.status = FAILURE_BAND[f].chip;
  }

  /** "もう一度探す" and the scan after "つないで、自動で見つける". */
  async discover(my = this.session): Promise<void> {
    this.go(this.state === 'L09' ? 'scanAgain' : 'helloDiscover');
    this.stopWatching();
    this.found = 1;
    if (!this.config) this.placeSample();
    try {
      const d = await this.call((h) => h.discover());
      if (my !== this.session) return;
      const seen = new Map(d.nodes.map((n) => [targetKey(n), n.hello]));
      seen.set('hub', d.hub);
      const now = Date.now();
      // ponytail: places the sample and marks which of its nodes answered; a
      // scan result that is not the sample needs the definition mode.
      this.live = Object.fromEntries(
        this.config!.nodes.map((n) => {
          const h = seen.get(targetKey(n.target)) ?? null;
          return [n.id, h ? { ...UNKNOWN, responding: true, hello: h, lastSeen: now } : UNKNOWN];
        }),
      );
      this.found = seen.size;
      this.status = ANNOUNCE.A06(this.found);
      this.go('scanned');
      this.startWatching();
    } catch {
      if (my === this.session) this.lose();
    }
  }

  /** "親機へ送る" / "もう一度送る". */
  async push(): Promise<void> {
    if (!this.config || !this.host) return;
    this.go(this.state === 'L11' ? 'pushAgain' : 'push');
    const my = this.session;
    try {
      const bin = topo_compile(this.config.yaml);
      await this.call((h) => h.pushTopology(bin));
      if (my !== this.session) return;
      this.sent = true;
      this.status = ANNOUNCE.A07;
      this.go('pushed');
    } catch {
      if (my === this.session) this.go('pushFailed');
    }
  }

  select(key: string | null): void {
    this.selected = key;
    const id = key?.startsWith('node:') ? key.slice(5) : null;
    if (id && this.host && WATCHED.has(this.state)) void this.refreshStatus(id);
  }

  /** The device went away (USB unplugged). The hub leaving for its reboot is not a loss. */
  lose(): void {
    if (this.state === 'L20' || this.state === 'L21') return;
    this.session++;
    void this.drop();
    const was = this.state;
    this.go('lost');
    if (this.state !== was) this.alert = ANNOUNCE.A21lost;
  }

  /** One monitoring round: HELLO to every node, then STATUS of the selected one. */
  async round(): Promise<void> {
    const c = this.config;
    if (!c || !this.host || this.watching || !WATCHED.has(this.state)) return;
    this.watching = true;
    const my = this.session;
    const before = this.responding;
    try {
      for (const n of c.nodes) {
        const r = await this.call((h) => h.watch(n.target.kind, n.target.addr));
        if (my !== this.session) return;
        const was = this.live[n.id] ?? UNKNOWN;
        this.live[n.id] = {
          ...was,
          responding: r.responding,
          rttMs: r.rttMs ?? was.rttMs,
          hello: r.hello ?? was.hello,
          lastSeen: r.hello ? Date.now() : was.lastSeen,
        };
        if (was.responding !== false && !r.responding) this.alert = ANNOUNCE.A21silent(n.name);
      }
      const id = this.selected?.startsWith('node:') ? this.selected.slice(5) : null;
      if (id) await this.refreshStatus(id);
    } catch {
      if (my === this.session) this.lose();
      return;
    } finally {
      this.watching = false;
    }
    if (this.responding !== before) this.status = ANNOUNCE.A08(this.responding, c.nodes.length);
    if (this.state === 'L12' && this.stuck) this.go('watchAlarm', false);
    else if (this.state === 'L13' && !this.stuck) this.go('watchOk', false);
  }

  private async refreshStatus(id: string): Promise<void> {
    const n = this.config?.nodes.find((x) => x.id === id);
    if (!n || this.live[id]?.responding === false) return;
    try {
      const s = await this.call((h) => h.status(n.target.kind, n.target.addr));
      this.live[id] = { ...(this.live[id] ?? UNKNOWN), hello: s.hello, status: s.status };
    } catch {
      // A missed STATUS shows the last values; the next watch round decides answering or not.
    }
  }

  /** Update progress over the target nodes, 0–100. */
  get overall(): number {
    const p = this.targets.map((id) => this.progress[id]?.percent ?? 0);
    return p.length ? Math.floor(p.reduce((a, b) => a + b, 0) / p.length) : 0;
  }

  /** The nodes an update runs on: those answering now (the hub alone when only it answers). */
  private answering(): string[] {
    return this.config?.nodes.filter((n) => this.live[n.id]?.responding === true).map((n) => n.id) ?? [];
  }

  /**
   * A release file was chosen ("更新ファイルを選ぶ", "応答する n つだけ先に
   * 更新", "別のファイルを選ぶ"): dagon-host's release check against the
   * configuration on the canvas, before anything is sent.
   */
  takeFile(name: string, bytes: Uint8Array): void {
    if (!this.config) return;
    const ev = this.state === 'L12' ? 'chooseFile' : this.state === 'L13' ? 'updateResponding' : 'otherFile';
    this.go(ev);
    if (this.state !== 'L14') return;
    const file: ReleaseFile = { name, size: bytes.length, bytes, verdict: 'checking' };
    this.file = file;
    this.refusal = undefined;
    this.status = ANNOUNCE.A09(name, FILE.size(bytes.length));
    this.go('read', false);
    try {
      check_release_for(bytes, topo_compile(this.config.yaml));
    } catch (e) {
      const k = (e instanceof Error && REFUSALS.includes(e.message) ? e.message : 'format') as Refusal;
      this.file = { ...file, verdict: k };
      this.refusal = k;
      this.status = k === 'topology' ? ANNOUNCE.A10topology : k === 'format' ? ANNOUNCE.A10format : ANNOUNCE.A10source;
      return this.go(k === 'topology' ? 'topoDiff' : 'badSource');
    }
    this.file = { ...file, verdict: 'pass' };
    this.targets = this.answering();
    this.status = ANNOUNCE.A10ok(this.targets.length);
    this.go('pass');
  }

  /** "更新": dagon-host's apply on the target nodes, children first and the hub last. */
  async update(): Promise<void> {
    const c = this.config;
    const f = this.file;
    if (!c || !f || f.verdict !== 'pass' || this.state !== 'L18') return;
    const my = this.session;
    this.go('update');
    this.stopWatching();
    this.failed = null;
    this.sentVersion = {};
    this.progress = Object.fromEntries(this.targets.map((id) => [id, { stage: 'waiting', percent: 0 }]));
    // ponytail: topology node index = position in config.nodes (true for the sample, which follows its YAML).
    const nodes = this.targets.map((id) => c.nodes.findIndex((n) => n.id === id));
    let results: NodeResult[];
    try {
      results = await this.call((h) => h.apply(f.bytes, topo_compile(c.yaml), nodes, (e) => void (my === this.session && this.onEvent(e))));
    } catch {
      // The release passed the same check before, so only the link can fail here.
      if (my === this.session) this.lose();
      return;
    }
    if (my !== this.session) return;
    this.reconnectUntil = null;
    this.finish(results);
  }

  private onEvent(e: UpdateEvent): void {
    const n = this.config?.nodes[e.node];
    if (!n) return;
    // Only the transfer carries the release's version; FAILED / REJECTED carry the version the node still runs.
    if (e.phase === PHASE.transfer) this.sentVersion[n.id] = e.version;
    const was = this.progress[n.id] ?? { stage: 'waiting', percent: 0 };
    if (e.phase === PHASE.transfer || e.phase === PHASE.verify) {
      if (was.stage !== 'writing') this.status = ANNOUNCE.A11(n.name);
      this.progress[n.id] = { stage: 'writing', percent: e.phase === PHASE.verify ? 100 : e.percent };
    } else if (e.phase === PHASE.reboot && n.role === 'hub') {
      this.progress[n.id] = { stage: 'rebooting', percent: 100 };
      this.reconnectUntil = this.now() + HUB_RECONNECT_MS;
      this.status = ANNOUNCE.A12;
      this.go('hubTurn');
    } else if (e.phase === PHASE.confirm) {
      this.progress[n.id] = { stage: 'done', percent: 100 };
    }
  }

  /** The clock dagon-host's reconnect window runs on. */
  now(): number {
    return (this.deps.clock ?? performance).now();
  }

  /** Where the run ended: all done, a node that stopped, or the hub to choose again. */
  private finish(results: NodeResult[]): void {
    const c = this.config!;
    for (const r of results) {
      const id = c.nodes[r.node]?.id;
      if (id && r.outcome === 'notStarted') this.progress[id] = { stage: 'unwritten', percent: 0 };
    }
    const stop = results.find((r) => r.outcome === 'failed');
    if (!stop) return this.done();
    const n = c.nodes[stop.node];
    if (n.role === 'hub' && this.state === 'L20' && stop.failure?.reason === 'NO ANSWER') {
      this.alert = ANNOUNCE.A21timeout;
      return this.go('autoFailed');
    }
    const why = stop.failure;
    if (why?.running != null) return this.stopAt(n.id, 'previous');
    // dagon-host says NO ANSWER for a timeout, a RELAY timeout or a lost link.
    if (!why || why.reason === 'NO ANSWER') return this.stopAt(n.id, 'silent');
    this.stopAt(n.id, 'refused', why.reason);
  }

  private stopAt(id: string, stage: 'previous' | 'silent' | 'refused', reason?: string): void {
    const n = this.config!.nodes.find((x) => x.id === id)!;
    this.failed = id;
    this.progress[id] = { stage, percent: this.progress[id]?.percent ?? 0, reason };
    this.status = stage === 'previous' ? ANNOUNCE.A13previous(n.name) : ANNOUNCE.A13(n.name);
    this.go(this.state === 'L21' ? 'revertedOnReconnect' : 'nodeStuck');
    this.startWatching();
  }

  private done(): void {
    const v = Object.values(this.sentVersion)[0];
    this.status = ANNOUNCE.A14(v === undefined ? '' : versionText(v));
    this.go(this.state === 'L20' ? 'confirmed' : this.state === 'L21' ? 'reconnected' : 'doneNoHub');
    this.startWatching();
  }

  /**
   * "もう一度つなぐ" while the hub reboots: the user chooses it again. While
   * dagon-host still reopens (L20) the chosen device goes under its next
   * reopen; after that (L21) dagon-host applies the confirm rule to it.
   */
  async reconnectHub(): Promise<void> {
    if (!this.deps.reselect || !this.link || (this.state !== 'L20' && this.state !== 'L21')) return;
    let t: Transport;
    try {
      t = await this.deps.reselect();
    } catch {
      return;
    }
    const old = this.transport;
    this.transport = t;
    this.link.use(t);
    if (old && old !== t) void old.close().catch(() => undefined);
    if (this.state !== 'L21') return;
    try {
      const r = await this.call((h) => h.confirm('hub', 0, false));
      const hub = this.config!.nodes.find((n) => n.role === 'hub')!;
      const mine = r.outcome === 'confirmed' || (r.outcome === 'notOnTrial' && r.version === this.sentVersion[hub.id]);
      this.alert = '';
      if (!mine) return this.stopAt(hub.id, 'previous');
      this.progress[hub.id] = { stage: 'done', percent: 100 };
      this.done();
    } catch {
      this.alert = ANNOUNCE.A21timeout;
    }
  }

  /** dagon-host runs one call at a time; calls queue here. */
  private call<T>(f: (h: Host) => Promise<T>): Promise<T> {
    const host = this.host;
    if (!host) return Promise.reject(new Error('not connected'));
    const run = this.chain.then(() => f(host));
    this.chain = run.catch(() => undefined);
    return run;
  }

  private startWatching(): void {
    if (!this.deps.timer || this.timer !== undefined) return;
    void this.round();
    this.timer = setInterval(() => void this.round(), watch_interval_ms());
  }

  private stopWatching(): void {
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
