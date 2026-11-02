// Operating-mode session: drives the state machine with what the user does
// and what dagon-host (through dagon-wasm) reports. The page only chooses and
// opens the device; every request, timeout and retry is dagon-host's.
import { hostLink } from '../serial/hostLink.ts';
import type { HelloResp } from '../serial/hello.ts';
import type { Transport } from '../serial/transport.ts';
import { newHost, topo_compile, watch_interval_ms, type Host, type StatusResp } from '../wasm/dagon.ts';
import { FAILURE_BAND } from './band.ts';
import { ConnectError, wasmReady } from './connect.ts';
import { ANNOUNCE } from './copy.ts';
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

export interface Deps {
  /** Opens the device: the browser chooser, or a stand-in hub. Throws ConnectError. */
  open: () => Promise<Transport>;
  ready?: () => Promise<unknown>;
  /** Starts the 5 s monitoring timer (off in tests, which call `round`). */
  timer?: boolean;
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

  private transport: Transport | null = null;
  private host: Host | null = null;
  private chain: Promise<unknown> = Promise.resolve();
  private watching = false;
  private timer: ReturnType<typeof setInterval> | undefined;
  /** Bumped per connection; results of an older one are dropped. */
  private session = 0;

  constructor(private readonly deps: Deps) {}

  get ctx(): Ctx {
    return { hasConfig: this.config !== null, failure: this.failure };
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
    this.host = newHost(hostLink(t));
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

  /** The device went away (USB unplugged). */
  lose(): void {
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
    await t?.close().catch(() => undefined);
  }
}
