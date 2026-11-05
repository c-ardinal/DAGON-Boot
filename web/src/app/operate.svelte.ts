// Operating-mode session: drives the state machine with what the user does
// and what dagon-host (through dagon-wasm) reports. The page only chooses and
// opens the device; every request, timeout and retry is dagon-host's.
import { hostLink, type LinkClock, type SwapLink } from '../serial/hostLink.ts';
import type { HelloResp } from '../serial/hello.ts';
import { isNote, notThisHub, type ReconnectNote, type Transport } from '../serial/transport.ts';
import {
  PHASE,
  check_release_for,
  newHost,
  releaseVersions,
  release_topology,
  topo_compile,
  versionText,
  watch_interval_ms,
  type Host,
  type NodeResult,
  type ConfirmResult,
  type RecoverResult,
  type Refusal,
  type StatusResp,
  type UpdateEvent,
} from '../wasm/dagon.ts';
import { FAILURE_BAND } from './band.ts';
import { ConnectError, wasmReady } from './connect.ts';
import { ANNOUNCE, FILE } from './copy.ts';
import { HUB_LOCKED, WATCHED, next, type ConnectFailure, type Ctx, type Ev, type L } from './machine.ts';
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
  /** STATUS of the boot core slots; null when the node keeps none or it is not read yet. */
  core?: StatusResp | null;
}

const UNKNOWN: Live = { responding: null, rttMs: null, lastSeen: null, hello: null, status: null };

/** Where one node is in an update (the node chip and the update-order row). */
export interface Progress {
  stage: 'waiting' | 'writing' | 'lastResort' | 'rebooting' | 'done' | 'silent' | 'refused' | 'previous' | 'unwritten' | 'cannotFix';
  percent: number;
  /** dagon-host's reason when the node refused (`E_FLASH`, `E_SIGNATURE`, ...). */
  reason?: string;
}

/** Stages in which a node's water rises with its progress (SC-O5); a finished or stopped node drains. */
const FILLING: ReadonlySet<Progress['stage']> = new Set(['writing', 'lastResort', 'rebooting']);

/** While a write or a recovery runs, the card checks stay as they are. */
const UPDATE_LOCKED: ReadonlySet<L> = new Set(['L19', 'L20', 'L21', 'L22', 'L24']);

/** How full each node being written is, 0–100 (the node's background on the canvas). */
export function waterOf(progress: Record<string, Progress>): Record<string, number> {
  return Object.fromEntries(
    Object.entries(progress)
      .filter(([, p]) => FILLING.has(p.stage))
      .map(([id, p]) => [id, Math.min(100, Math.max(0, p.percent))]),
  );
}

/**
 * What the last run left (L23's one sentence says only this): an update with
 * `left` nodes of the 構成 not on the new version (they did not answer), a
 * node rolled back to its previous version, or a node recovered.
 */
export type Finish =
  | { kind: 'updated'; left: number }
  | { kind: 'rolledBack'; name: string; version: string }
  | { kind: 'recovered'; name: string; version: string };

/**
 * Where one node's write is, for its one %: when the transfer of the
 * current image started (`t0`), how many of its bytes are sent, how long
 * the transfer took once verified, when the node was rebooted onto it, and
 * whether it confirmed. `floor` keeps the % from moving back when a
 * second image (boot core, then app) starts.
 */
export interface WriteTrack {
  t0: number | null;
  xferPct: number;
  xferMs: number | null;
  rebootAt: number | null;
  done: boolean;
  floor: number;
}

/**
 * One % over a node's whole write: the transfer by its bytes, then the
 * trial (start-up and self-check until CONFIRM) by time against the MCU
 * definition's `[watchdog] trial_ms`; the two parts weigh what each takes
 * (the transfer's time projected from its byte rate until it is verified).
 * Only the confirmation makes 100. Without a trial deadline only the bytes
 * count.
 */
export function writePercent(k: WriteTrack, now: number, trialMs: number | null): number {
  if (k.done) return 100;
  let p: number;
  if (k.xferMs === null) {
    if (k.t0 === null || k.xferPct <= 0) p = 0;
    else if (trialMs === null) p = k.xferPct;
    else {
      const elapsed = Math.max(0, now - k.t0);
      p = (100 * elapsed) / ((elapsed * 100) / k.xferPct + trialMs);
    }
  } else if (trialMs === null) p = 99;
  else {
    const onTrial = k.rebootAt === null ? 0 : Math.min(trialMs, Math.max(0, now - k.rebootAt));
    p = (100 * (k.xferMs + onTrial)) / (k.xferMs + trialMs);
  }
  return Math.max(k.floor, Math.min(99, p));
}

const NO_TRACK: WriteTrack = { t0: null, xferPct: 0, xferMs: null, rebootAt: null, done: false, floor: 0 };

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

/** dagon-host's release check (check_release_for, apply, recover) threw this: which refusal it names. */
export function refusalOf(e: unknown): Refusal {
  const m = e instanceof Error ? e.message : String(e);
  return (REFUSALS.includes(m) ? m : 'format') as Refusal;
}

/** The announcement of a refused release, the same wherever the check ran (file, recovery). */
export function refusalAnnounce(k: Refusal): string {
  return k === 'topology' ? ANNOUNCE.A10topology : k === 'format' ? ANNOUNCE.A10format : ANNOUNCE.A10source;
}

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
  /**
   * The project's latest release through dagon serve (GET /v1/release/latest),
   * which "最新のリリースを使う" and a recovery without a file take; null when
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

/** The file name "最新のリリースを使う" shows. */
export const LATEST_NAME = 'latest.dgb';

/**
 * dagon-host's reason for a node that did not get through, as its chip:
 * no answer (`NO ANSWER`: timeout, RELAY timeout, lost link), back on the
 * previous version (`ROLLED BACK`), an explicit refusal (`E_FLASH`, ...),
 * or a failure the page has no word for (cannot fix).
 */
export function failureStage(reason: string): Pick<Progress, 'stage' | 'reason'> {
  if (reason === 'NO ANSWER') return { stage: 'silent' };
  if (reason === 'ROLLED BACK') return { stage: 'previous' };
  // A refused release (source, key, topology, format) keeps its kind: the chip says which.
  if (REFUSALS.includes(reason) || reason.startsWith('E_') || reason === 'REJECTED') return { stage: 'refused', reason };
  return { stage: 'cannotFix' };
}

/** The 構成 in a release's topology item (dagon-wasm reads it), or null when it has none or cannot be read. */
function fileTopologyOf(bytes: Uint8Array, yaml: string): Record<string, unknown> | null {
  try {
    return (release_topology(bytes, yaml) as Record<string, unknown> | null) ?? null;
  } catch {
    return null;
  }
}

export class Operate {
  state = $state<L>('L02');
  failure = $state<ConnectFailure | undefined>();
  config = $state<Config | null>(null);
  live = $state<Record<string, Live>>({});
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
  /** The node being recovered or rolled back (and tried again from L25). */
  recovering = $state<string | null>(null);
  /** How the last recovery got through: the ROM path shows its figures in the detail. */
  recovered = $state<RecoverResult | null>(null);
  /** The update after a recovery covers the nodes not written yet ("残りのノードへ書き込む"). */
  rest = $state(false);
  /**
   * The 構成 a release made for another 構成 was made for (L17, §7.6): shown
   * as differences, and taken in 定義 by "ファイルの構成を取り込む". Null when
   * the file carries none.
   */
  fileTopology = $state<Record<string, unknown> | null>(null);
  /** "最新のリリースを使う" found no release (M58 in the band until the next step). */
  latestMissing = $state(false);
  /** The version the chosen release gives each node (shown as "↑ 1.0.0 → 1.1.0" before the update). */
  fileVersions = $state<Record<string, number>>({});
  /** Date.now() when the running update or recovery started (elapsed time); null when none runs. */
  startedAt = $state<number | null>(null);
  /** Why the page could not get the rebooted hub back by itself (L21's diagnostic log). */
  reconnectNotes = $state<ReconnectNote[]>([]);
  /** Why the device the CLI gave back was not taken (L26 shows it, C-389); null otherwise. */
  returnRefused = $state<ReconnectNote | null>(null);
  /** The hub's HELLO when the user connected: what "the same 親機" means after its reboot. */
  private hubHello: HelloResp | null = null;
  /** Nodes whose card check is off: the next write leaves them out. */
  skip = $state(new Set<string>());
  /** Each node's write so far, for its one % (see writePercent). */
  track = $state<Record<string, WriteTrack>>({});
  /** The package being written could not be kept in the project (it is written all the same). */
  keepFailed = $state(false);
  /** What the last update, recovery or rollback left (L23). */
  finished = $state<Finish | null>(null);
  /** Nodes on the chosen release's version since it was chosen (an update, its rest, a recovery). */
  private updated = new Set<string>();
  /** The run L20 / L21 belong to, so L23 says what it did. */
  private running: 'update' | 'recover' | 'rollback' = 'update';

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
    return { hasConfig: this.config !== null, failure: this.failure, refusal: this.refusal, answering: this.needUpdate > 0 };
  }

  /**
   * The nodes updating reaches, as the checked cut lists them (config.reach):
   * the compiled topology's nodes, so a node's topology index is its place
   * here. The others (V-23) are drawn but never watched, updated or recovered.
   */
  get reach(): Node[] {
    const c = this.config;
    return c ? c.reach.flatMap((id) => c.nodes.filter((n) => n.id === id)) : [];
  }

  /** Answering nodes not yet on the chosen release's version (all answering ones before an update ran). */
  get needUpdate(): number {
    return this.reach.filter((n) => this.live[n.id]?.responding === true && !this.updated.has(n.id)).length;
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

  private go(ev: Ev, focus = true): void {
    const to = next(this.state, ev);
    if (to === this.state && ev !== 'placeSample') return;
    this.state = to;
    this.latestMissing = false;
    // Out of the hub's reboot (confirmed, stopped, lost, ...): no version to keep a reopened device by any more.
    if (to !== 'L20' && to !== 'L21') this.transport?.expectHub?.(null);
    if (focus) this.focusRequest++;
  }

  /** `dagon serve` needs the hub's port (device.need): close Web Serial and show L28. */
  async handOver(): Promise<void> {
    this.session++;
    await this.drop();
    this.go('cliClaim', false);
  }

  /**
   * The header's 切断: the port is closed but its permission is kept (no
   * forget()), so the CLI or a later つなぐ can open it; the session waits at L02.
   */
  async disconnect(): Promise<void> {
    if (HUB_LOCKED.has(this.state) || next(this.state, 'disconnect') === this.state) return;
    this.session++;
    await this.drop();
    this.alert = '';
    this.status = ANNOUNCE.A21lost;
    this.go('disconnect', false);
  }

  /**
   * The CLI gave the port back (device.returned): the page opens the device
   * it had again by itself and goes on watching (L12, M67); when that fails
   * the user connects again (L26, M34). It must be the same 親機 as before
   * (notThisHub against the first HELLO); otherwise it is closed and the
   * reason goes to the log.
   */
  async cliReturned(): Promise<void> {
    if (this.state !== 'L28') return;
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
      const h = await this.call((x) => x.hello('hub', 0));
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
      this.alert = ANNOUNCE.A21lost;
      return this.go('lost', false);
    }
    if (my !== this.session) return;
    this.status = ANNOUNCE.A05;
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
    this.config = DESK_DEMO;
    this.live = Object.fromEntries(DESK_DEMO.nodes.map((n) => [n.id, UNKNOWN]));
    this.go('placeSample', false);
  }

  /**
   * "パソコンとつなぐ" / "つないで、自動で見つける" / "選び直す" / "もう一度つなぐ".
   * With nothing on the canvas, connecting goes on to find the nodes.
   */
  async connect(): Promise<void> {
    this.returnRefused = null;
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
      this.hubHello = await this.call((h) => h.hello('hub', 0));
    } catch {
      await this.drop();
      return this.fail(my, 'noAnswer', 'no-response');
    }
    if (my !== this.session) return;
    this.status = ANNOUNCE.A05;
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
      for (const n of this.reach) {
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
    if (this.responding !== before) this.status = ANNOUNCE.A08(this.responding, this.reach.length);
    // L12 and L23 move to L13 by themselves while a node does not answer: it stays visible and recoverable
    // (after "応答する n つだけ先に書き込む" the ones left out are named there).
    if ((this.state === 'L12' || this.state === 'L23') && this.stuck) this.go('watchAlarm', false);
    else if (this.state === 'L13' && !this.stuck) this.go('watchOk', false);
  }

  private async refreshStatus(id: string): Promise<void> {
    const n = this.reach.find((x) => x.id === id);
    if (!n || this.live[id]?.responding === false) return;
    try {
      const s = await this.call((h) => h.status(n.target.kind, n.target.addr));
      // A node without boot core slots refuses this STATUS: no row then.
      const core = await this.call((h) => h.bootCoreStatus(n.target.kind, n.target.addr)).catch(() => null);
      this.live[id] = { ...(this.live[id] ?? UNKNOWN), hello: s.hello, status: s.status, core };
    } catch {
      // A missed STATUS shows the last values; the next watch round decides answering or not.
    }
  }

  /** Update progress over the target nodes, 0–100. */
  get overall(): number {
    const p = this.targets.map((id) => this.progress[id]?.percent ?? 0);
    return p.length ? Math.floor(p.reduce((a, b) => a + b, 0) / p.length) : 0;
  }

  /** The nodes an update runs on: those answering now and checked on their cards (the hub alone when only it answers). */
  private answering(): string[] {
    return this.reach.filter((n) => this.live[n.id]?.responding === true && !this.skip.has(n.id)).map((n) => n.id);
  }

  /**
   * A release file was chosen ("パッケージファイルを選ぶ", "応答する n つだけ先に
   * 書き込む", "別のファイルを選ぶ"): dagon-host's release check against the
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
    this.fileTopology = null;
    this.status = ANNOUNCE.A09(name, FILE.size(bytes.length));
    this.go('read', false);
    try {
      check_release_for(bytes, topo_compile(this.config.yaml));
    } catch (e) {
      const k = refusalOf(e);
      this.file = { ...file, verdict: k };
      this.refusal = k;
      if (k === 'topology') this.fileTopology = fileTopologyOf(bytes, this.config.yaml);
      this.status = refusalAnnounce(k);
      return this.go(k === 'topology' ? 'topoDiff' : 'badSource');
    }
    this.file = { ...file, verdict: 'pass' };
    const given = releaseVersions(bytes);
    // ponytail: topology node index = position in reach, as in update().
    this.updated.clear();
    this.fileVersions = Object.fromEntries(this.reach.flatMap((n, i) => (given.has(i) ? [[n.id, given.get(i)!]] : [])));
    this.rest = false;
    this.targets = this.answering();
    this.status = ANNOUNCE.A10ok(this.targets.length, this.config.nodes.length - this.reach.length);
    this.go('pass');
  }

  /** A node card's check: off leaves the node out of the next write (not while one runs). */
  toggleSkip(id: string): void {
    if (UPDATE_LOCKED.has(this.state)) return;
    const next = new Set(this.skip);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    this.skip = next;
  }

  /** Node `id`'s one % for its write, at the clock's `now`. */
  percentOf(id: string, now = this.now()): number {
    const k = this.track[id];
    return k ? writePercent(k, now, this.deps.trialMs?.(id) ?? null) : 0;
  }

  /** The write's check of node `id` once a file is chosen (L18): an unchecked node is not written. Targets keep update order. */
  choose(id: string, on: boolean): void {
    if (this.state !== 'L18' || !this.reach.some((n) => n.id === id)) return;
    const chosen = new Set(this.targets);
    if (on) chosen.add(id);
    else chosen.delete(id);
    this.targets = this.reach.map((n) => n.id).filter((x) => chosen.has(x));
  }

  /** "n ノードへ書き込む": dagon-host's apply on the checked nodes, children first and the hub last. */
  async update(): Promise<void> {
    const c = this.config;
    const f = this.file;
    if (!c || !f || f.verdict !== 'pass' || this.state !== 'L18' || this.targets.length === 0) return;
    const my = this.session;
    this.go('update');
    this.stopWatching();
    // Every package written is kept first, so it can be written back later.
    this.keepFailed = this.deps.keep ? !(await this.deps.keep(f.bytes).catch(() => false)) : false;
    if (my !== this.session) return;
    this.running = 'update';
    this.startedAt = Date.now();
    this.failed = null;
    this.sentVersion = {};
    this.progress = Object.fromEntries(this.targets.map((id) => [id, { stage: 'waiting', percent: 0 }]));
    this.track = Object.fromEntries(this.targets.map((id) => [id, { ...NO_TRACK }]));
    // ponytail: topology node index = position in reach (true for the sample, which follows its YAML).
    const reach = this.reach;
    const nodes = this.targets.map((id) => reach.findIndex((n) => n.id === id));
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
    this.startedAt = null;
    this.finish(results);
  }

  private onEvent(e: UpdateEvent): void {
    const n = this.reach[e.node];
    if (!n) return;
    this.trackEvent(n.id, e);
    // Only the app's transfer carries the release's version; FAILED / REJECTED carry the version the node still runs.
    if (e.phase === PHASE.transfer && !e.bootCore) this.sentVersion[n.id] = e.version;
    const was = this.progress[n.id] ?? { stage: 'waiting', percent: 0 };
    if (e.phase === PHASE.transfer || e.phase === PHASE.verify) {
      if (was.stage !== 'writing') this.status = ANNOUNCE.A11(n.name);
      this.progress[n.id] = { stage: 'writing', percent: e.phase === PHASE.verify ? 100 : e.percent };
    } else if (e.phase === PHASE.reboot && n.role === 'hub') {
      this.progress[n.id] = { stage: 'rebooting', percent: 100 };
      this.hubAway(this.sentVersion[n.id]);
    } else if (e.phase === PHASE.confirm && !e.bootCore) {
      // Only the app's confirmation finishes the node; its boot core's comes before it.
      this.progress[n.id] = { stage: 'done', percent: 100 };
      this.updated.add(n.id);
      // The card shows the version the node now runs.
      const was = this.live[n.id];
      if (was?.hello) this.live[n.id] = { ...was, hello: { ...was.hello, activeVersion: e.version, trial: false } };
    }
  }

  /** Feeds node `id`'s write track (its one %) with one of dagon-host's events. */
  private trackEvent(id: string, e: UpdateEvent): void {
    const now = this.now();
    const k = this.track[id] ?? { ...NO_TRACK };
    if (e.phase === PHASE.transfer) {
      // A new image (after a boot core's reboot) starts its own transfer.
      const fresh = k.t0 === null || k.xferMs !== null;
      const base = fresh ? { ...k, floor: this.percentOf(id, now), t0: now, xferMs: null, rebootAt: null, done: false } : k;
      this.track[id] = { ...base, xferPct: e.percent };
    } else if (e.phase === PHASE.verify) {
      this.track[id] = { ...k, xferPct: 100, xferMs: Math.max(1, now - (k.t0 ?? now)) };
    } else if (e.phase === PHASE.reboot) {
      this.track[id] = { ...k, rebootAt: now };
    } else if (e.phase === PHASE.confirm && !e.bootCore) {
      this.track[id] = { ...k, done: true };
    }
  }

  /** The hub reboots onto `version`: dagon-host reopens it for 45 s (L20), keeping only that hub. */
  private hubAway(version: number | undefined): void {
    if (version !== undefined) this.transport?.expectHub?.(version, this.hubHello);
    this.reconnectNotes = [];
    this.reconnectUntil = this.now() + HUB_RECONNECT_MS;
    this.status = ANNOUNCE.A12;
    this.go('hubTurn');
  }

  /** dagon-host gave up reopening the hub (L21): the dialog opens, with why. */
  private autoFailed(): void {
    this.reconnectNotes = this.transport?.notes?.() ?? [];
    this.alert = ANNOUNCE.A21timeout;
    this.go('autoFailed');
  }

  /** The clock dagon-host's reconnect window runs on. */
  now(): number {
    return (this.deps.clock ?? performance).now();
  }

  /** Where the run ended: all done, a node that stopped, or the hub to choose again. */
  private finish(results: NodeResult[]): void {
    const reach = this.reach;
    for (const r of results) {
      const id = reach[r.node]?.id;
      if (id && r.outcome === 'notStarted') this.progress[id] = { stage: 'unwritten', percent: 0 };
      // A confirmed node runs the new version now, also when a later node stops the run.
      const hello = id ? this.live[id]?.hello : null;
      if (id && hello && r.outcome === 'done' && r.version !== null) {
        this.live[id] = { ...this.live[id]!, hello: { ...hello, activeVersion: r.version, trial: false } };
      }
    }
    const stop = results.find((r) => r.outcome === 'failed');
    if (!stop) return this.done();
    const n = reach[stop.node];
    if (n.role === 'hub' && this.state === 'L20' && stop.failure?.reason === 'NO ANSWER') return this.autoFailed();
    const why = stop.failure;
    const f = why?.running != null ? { stage: 'previous' as const } : failureStage(why?.reason ?? 'NO ANSWER');
    // A stopped update has no "cannot fix" yet: an unnamed failure is a refusal of the update.
    this.stopAt(n.id, f.stage === 'cannotFix' ? 'refused' : f.stage, f.reason);
  }

  private stopAt(id: string, stage: Progress['stage'], reason?: string): void {
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
    this.finished = { kind: 'updated', left: this.reach.filter((n) => !this.updated.has(n.id)).length };
    this.go(this.state === 'L20' ? 'confirmed' : this.state === 'L21' ? 'reconnected' : 'doneNoHub');
    this.startWatching();
  }

  /**
   * "もう一度つなぐ" while the hub reboots: the user chooses it again. While
   * dagon-host still reopens (L20) the chosen device goes under its next
   * reopen; after that (L21) dagon-host applies the confirm rule to it.
   * `onlyMine` (a device the browser reported connected, nobody chose it):
   * taken only when it is the hub on trial with the version just sent.
   * Resolves whether the device was taken.
   */
  async reconnectHub(pick = this.deps.reselect, onlyMine = false): Promise<boolean> {
    if (!pick || !this.link || (this.state !== 'L20' && this.state !== 'L21')) return false;
    let t: Transport;
    try {
      t = await pick();
    } catch {
      return false;
    }
    // L20: dagon-host is still reopening (its apply holds the queue), so the device goes under its next reopen now.
    if (this.state === 'L20') {
      this.swap(t);
      return true;
    }
    // L21: swapping, checking and confirming run as one queued step, so two devices reported at once cannot interleave.
    const link = this.link;
    const r = await this.call(async (h) => {
      const old = this.transport;
      link.use(t);
      try {
        const why = this.notThisHub(await h.hello('hub', 0), onlyMine);
        if (why) throw why;
        const confirmed = await h.confirm('hub', 0, false);
        this.swap(t, old);
        return confirmed;
      } catch (e) {
        this.noteWhy(isNote(e) ? e : { kind: 'silent' });
        if (old) link.use(old);
        return null;
      }
    }).catch(() => null);
    if (!r) {
      // Never left open while 切断 is held: the dialog stays, with the reason under 詳しく見る.
      void t.close().catch(() => undefined);
      this.alert = ANNOUNCE.A21timeout;
      return false;
    }
    this.confirmed(r);
    return true;
  }

  /**
   * Why the device under the link is not the hub the update rebooted, or null.
   * HELLO carries no serial number, so "the same 親機" is the role, MCU and
   * release key of its first HELLO; `onTrial` (a device nobody chose) also
   * needs it on trial with the version just sent. One the user chose may be
   * back on its previous version: CONFIRM then stops there (L22).
   */
  private notThisHub(h: HelloResp, onTrial: boolean): ReconnectNote | null {
    // ponytail: -1 matches no version, so a device nobody chose is never kept when nothing was sent.
    return notThisHub(h, this.hubHello, onTrial ? (this.hubSent() ?? -1) : null);
  }

  /** `t` replaces the device under the link; the one it replaces is closed. */
  private swap(t: Transport, old = this.transport): void {
    this.link?.use(t);
    this.transport = t;
    if (old && old !== t) void old.close().catch(() => undefined);
  }

  /** The version just sent to the hub, if any. */
  private hubSent(): number | null {
    const hub = this.reach.find((n) => n.role === 'hub');
    return hub ? (this.sentVersion[hub.id] ?? null) : null;
  }

  private noteWhy(why: ReconnectNote): void {
    if (!this.reconnectNotes.some((x) => JSON.stringify(x) === JSON.stringify(why))) this.reconnectNotes = [...this.reconnectNotes, why];
  }

  /** L21: the hub confirmed on the version just sent goes on to L23; on its previous version it stops there (L22). */
  private confirmed(r: ConfirmResult): void {
    const hub = this.reach.find((n) => n.role === 'hub')!;
    const mine = r.outcome === 'confirmed' || (r.outcome === 'notOnTrial' && r.version === this.sentVersion[hub.id]);
    this.alert = '';
    if (!mine) return this.stopAt(hub.id, 'previous');
    this.progress[hub.id] = { stage: 'done', percent: 100 };
    if (this.running === 'update') {
      this.updated.add(hub.id);
      return this.done();
    }
    this.finishRun(hub, r.version);
    this.status = ANNOUNCE.A14(versionText(r.version));
    this.go('reconnected');
    this.startWatching();
  }

  /** The node "（ノード）を復旧する" names: the stopped one (L22, L25) or the first silent one (L13). */
  get recoverTarget(): Node | undefined {
    const id = this.state === 'L25' ? this.recovering : this.state === 'L22' ? this.failed : this.stuck?.id;
    return this.reach.find((n) => n.id === id);
  }

  private async latest(): Promise<Uint8Array | null> {
    return (await this.deps.latest?.().catch(() => null)) ?? null;
  }

  /** "最新のリリースを使う": the project's latest release, checked like a chosen file. */
  async useLatest(): Promise<boolean> {
    const bytes = await this.latest();
    if (!bytes) {
      this.alert = ANNOUNCE.A22latest;
      this.latestMissing = true;
      return false;
    }
    this.takeFile(LATEST_NAME, bytes);
    return this.file?.verdict === 'pass';
  }

  /**
   * "（ノード）を復旧する": dagon-host's recover with the release on the
   * screen (else the project's latest) and, for the ROM path, the project's
   * boot core. Without any release it asks for one (`needFile`); `chosen`
   * is that file.
   */
  async recover(chosen?: { name: string; bytes: Uint8Array }): Promise<'needFile' | void> {
    const c = this.config;
    const n = this.recoverTarget;
    if (!c || !n || !['L13', 'L22', 'L25'].includes(this.state)) return;
    const topo = topo_compile(c.yaml);
    let release = chosen?.bytes ?? (this.file?.verdict === 'pass' ? this.file.bytes : null);
    if (chosen) {
      try {
        check_release_for(chosen.bytes, topo);
      } catch (e) {
        this.alert = refusalAnnounce(refusalOf(e));
        return;
      }
      this.file = { name: chosen.name, size: chosen.bytes.length, bytes: chosen.bytes, verdict: 'pass' };
    }
    release ??= await this.latest();
    if (!release) return 'needFile';
    const bytes = release;
    const bootcore = (await this.deps.bootcore?.(n.id).catch(() => null)) ?? null;
    const index = this.reach.indexOf(n);
    const stage0 = (await this.deps.stage0?.(n.id).catch(() => null)) ?? null;
    await this.runRecovery(n, 'recover', (h, on) => h.recover(index, bytes, topo, bootcore, stage0, on));
  }

  /** "もう一度直す" (L25): the operation that failed, again (a rollback stays a rollback). */
  async retry(): Promise<'needFile' | void> {
    if (this.state !== 'L25') return;
    if (this.lastKind === 'rollback' && this.recovering) return this.rollback(this.recovering);
    return this.recover();
  }

  /** "前のバージョンに戻す" (the detail of a node, UC-O10 / M38): the version in the other slot. */
  async rollback(id: string): Promise<void> {
    const reach = this.reach;
    const n = reach.find((x) => x.id === id);
    if (!n) return;
    const index = reach.indexOf(n);
    await this.runRecovery(n, 'rollback', async (h, on) => {
      const v = await h.rollback(n.target.kind, n.target.addr, index, on);
      return { path: 'retransfer', version: v, rom: null };
    });
  }

  /** "このバージョンで確定する" (M38): keeps the version a node runs on trial. */
  async keepTrial(id: string): Promise<void> {
    const n = this.reach.find((x) => x.id === id);
    if (!n || !this.host) return;
    try {
      await this.call((h) => h.confirm(n.target.kind, n.target.addr, true));
    } catch {
      // The next watch round shows whether it still answers.
    }
    await this.refreshStatus(id);
  }

  /** What ran last in L24, so "もう一度直す" repeats it. */
  private lastKind: 'recover' | 'rollback' = 'recover';

  private async runRecovery(
    n: Node,
    kind: 'recover' | 'rollback',
    run: (h: Host, on: (e: UpdateEvent) => void) => Promise<RecoverResult>,
  ): Promise<void> {
    const my = this.session;
    this.go(this.state === 'L25' ? 'recoverAgain' : kind);
    if (this.state !== 'L24') return;
    this.stopWatching();
    this.lastKind = kind;
    this.recovering = n.id;
    this.recovered = null;
    this.running = kind;
    this.startedAt = Date.now();
    this.progress = { ...this.progress, [n.id]: { stage: 'writing', percent: 0 } };
    const on = (e: UpdateEvent) => {
      if (my !== this.session || this.reach[e.node]?.id !== n.id) return;
      if (e.phase === PHASE.rom) this.progress[n.id] = { stage: 'lastResort', percent: e.percent };
      else if (e.phase === PHASE.transfer) this.progress[n.id] = { stage: 'writing', percent: e.percent };
      // TRANSFER and REBOOT carry the version the node is given (FAILED carries the one it runs).
      if ((e.phase === PHASE.transfer || e.phase === PHASE.reboot) && !e.bootCore) this.sentVersion[n.id] = e.version;
      // The hub reboots onto it: the same reconnect and confirm rule as an update (L20 / L21).
      if (e.phase === PHASE.reboot && n.role === 'hub') {
        this.progress[n.id] = { stage: 'rebooting', percent: 100 };
        this.hubAway(this.sentVersion[n.id]);
      }
    };
    let r: RecoverResult;
    try {
      r = await this.call((h) => run(h, on));
    } catch (e) {
      if (my !== this.session) return;
      this.reconnectUntil = null;
      this.startedAt = null;
      return this.recoveryFailed(n, e instanceof Error ? e.message : '');
    }
    if (my !== this.session) return;
    this.reconnectUntil = null;
    this.startedAt = null;
    this.recovered = r;
    this.progress[n.id] = { stage: 'done', percent: 100 };
    this.finishRun(n, r.version);
    this.live[n.id] = { ...(this.live[n.id] ?? UNKNOWN), responding: true, lastSeen: Date.now() };
    const rest = this.file?.verdict === 'pass' ? Object.entries(this.progress).filter(([, p]) => p.stage === 'unwritten').map(([id]) => id) : [];
    this.status = kind === 'rollback' ? ANNOUNCE.A15(n.name, versionText(r.version)) : ANNOUNCE.A16(n.name, rest.length);
    // The hub's REBOOT event moved the state on while dagon-host ran.
    if ((this.state as L) === 'L20') {
      this.go('confirmed');
    } else if (rest.length) {
      this.targets = rest;
      this.rest = true;
      this.go('fixedRest');
    } else {
      this.go('fixedNoRest');
    }
    this.startWatching();
    await this.refreshStatus(n.id);
  }

  /** A recovery or rollback got through on `version`: what L23 says. */
  private finishRun(n: Node, version: number): void {
    if (this.running === 'recover' && this.file?.verdict === 'pass') this.updated.add(n.id);
    const v = versionText(version);
    this.finished = this.running === 'rollback' ? { kind: 'rolledBack', name: n.name, version: v } : { kind: 'recovered', name: n.name, version: v };
  }

  /** A recovery or rollback that did not get through, shown with dagon-host's reason. */
  private recoveryFailed(n: Node, reason: string): void {
    // The rebooted hub was not reopened in time: the user chooses it again (L21).
    if (this.state === 'L20' && reason === 'NO ANSWER') return this.autoFailed();
    const percent = this.progress[n.id]?.percent ?? 0;
    this.progress[n.id] = { ...failureStage(reason), percent };
    this.status = REFUSALS.includes(reason) ? refusalAnnounce(reason as Refusal) : ANNOUNCE.A13(n.name);
    this.go(this.state === 'L20' ? 'nodeStuck' : 'gaveUp');
  }

  /**
   * "最初からやり直す" (SC-M): the page's records go (connection, file,
   * progress) and the session waits at L02; the project's files are untouched.
   */
  reset(): void {
    this.returnRefused = null;
    this.session++;
    void this.drop();
    this.state = 'L02';
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
