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
  releaseAgentDigest,
  releaseAppDigest,
  releaseCoreVersions,
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
  type VersionResp,
  type UpdateEvent,
  trialPercent,
} from '../wasm/dagon.ts';
import type { RollbackRecord } from './agent.svelte.ts';
import { FAILURE_BAND } from './band.ts';
import { ConnectError, wasmReady } from './connect.ts';
import { ANNOUNCE, FILE, WRITE } from './copy.ts';
import { HUB_LOCKED, NODE_WRITE, WATCHED, next, type ConnectFailure, type Ctx, type Ev, type L } from './machine.ts';
import { SAMPLE_CONFIG, targetKey, type Config, type Node } from './sample.ts';

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
  /** VERSION; null when the node's agent predates it (or it was not readable), undefined until read. */
  version?: VersionResp | null;
}

const UNKNOWN: Live = { responding: null, rttMs: null, lastSeen: null, hello: null, status: null };

/** Where one node is in an update (the node chip and the update-order row). */
export interface Progress {
  stage: 'waiting' | 'writing' | 'lastResort' | 'rebooting' | 'done' | 'silent' | 'refused' | 'previous' | 'coreBack' | 'unwritten' | 'cannotFix';
  percent: number;
  /** dagon-host's reason when the node refused (`E_FLASH`, `E_SIGNATURE`, ...). */
  reason?: string;
  /** The boot core version being written, while it is; the app's write has none. */
  core?: number;
}

/** The recovery agent's slot in STATUS. */
const FACTORY_SLOT = 2;

const sameBytes = (a: Uint8Array | null, b: Uint8Array): boolean => a !== null && a.length === b.length && a.every((x, i) => x === b[i]);

/** The package's app for a node is what it runs, off trial: dagon-host's normal write leaves it out. */
function appUnchanged(bytes: Uint8Array, node: number, st: { hello: HelloResp; status: StatusResp }): boolean {
  const running = st.status.slots[st.status.active]?.digest16 ?? null;
  const app = releaseAppDigest(bytes, node, st.status.active);
  return !st.hello.trial && app !== null && sameBytes(running, app);
}

/** On a node's card while it is written: what is being written now (its boot core comes first when that differs), or null when not known. */
export function writingTo(p: Progress, app: number | undefined): string | null {
  if (p.core !== undefined) return WRITE.toCore(versionText(p.core));
  return app === undefined ? null : WRITE.to(versionText(app));
}

/** Stages in which a node's water rises with its progress; a finished or stopped node drains. */
const FILLING: ReadonlySet<Progress['stage']> = new Set(['writing', 'lastResort', 'rebooting']);

/** While a write or a recovery runs, the card checks stay as they are. */
const UPDATE_LOCKED: ReadonlySet<L> = new Set(['writing', 'hubRestarting', 'reconnectNeeded', 'stopped', 'recovering']);

/** How full each node being written is, 0–100 (the node's background on the canvas). */
export function waterOf(progress: Record<string, Progress>): Record<string, number> {
  return Object.fromEntries(
    Object.entries(progress)
      .filter(([, p]) => FILLING.has(p.stage))
      .map(([id, p]) => [id, Math.min(100, Math.max(0, p.percent))]),
  );
}

/**
 * What the last run left (done's one sentence says only this): an update with
 * `left` nodes of the 構成 not on the new version (they did not answer), a
 * node rolled back to its previous version, or a node recovered.
 */
export type Finish =
  | { kind: 'updated'; left: number; leftOut?: number }
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
  /** What dagon-host last said for the node. */
  shown: number;
  /** The transfer took this long (it is verified); null while it runs. */
  xferMs: number | null;
  t0: number | null;
  rebootAt: number | null;
  done: boolean;
}

/**
 * A node's one % at `now`, the same number its display is told: dagon-host's
 * calculation in every part. During a transfer it is the last number dagon-host
 * reported; after it, the trial reports nothing, so its time is read here
 * against the MCU definition's `[watchdog] trial_ms`. Only the confirmation
 * makes 100.
 */
export function writePercent(k: WriteTrack, now: number, trialMs: number | null): number {
  if (k.done) return 100;
  // Nothing transferred before the reboot (a rollback): the trial is all there is to count.
  if (k.rebootAt === null ? k.xferMs === null : k.xferMs === null && trialMs === null) return k.shown;
  return trialPercent(k.xferMs ?? 0, k.rebootAt === null ? 0 : Math.max(0, now - k.rebootAt), trialMs, k.shown);
}

/** States in which a node's write or recovery really goes on, so its % may rise with the clock. */
const WORKING: ReadonlySet<L> = new Set(['writing', 'hubRestarting', 'recovering']);

const NO_TRACK: WriteTrack = { shown: 0, xferMs: null, t0: null, rebootAt: null, done: false };

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
  if (reason === 'BOOT CORE ROLLED BACK') return { stage: 'coreBack' };
  // A refused release (source, key, topology, format) keeps its kind: the chip says which.
  if (REFUSALS.includes(reason) || reason.startsWith('E_') || reason === 'REJECTED') return { stage: 'refused', reason };
  return { stage: 'cannotFix' };
}

/**
 * Where a hub chosen again by hand stands: on the app just `sent`; else, when
 * only a boot core was sent, its running boot core `core` tells whether that
 * core went back or took over: with the app still to write, or with none to write when the package's is what it runs
 * (`appSame`).
 */
export function hubBack(r: ConfirmResult, sent: number | undefined, coreSent: number | undefined, core: number | null, appSame = false): 'done' | 'previous' | 'coreBack' | 'unwritten' {
  if (r.outcome === 'confirmed' || (r.outcome === 'notOnTrial' && r.version === sent)) return 'done';
  if (sent === undefined && coreSent !== undefined) return core !== coreSent ? 'coreBack' : appSame ? 'done' : 'unwritten';
  return 'previous';
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
  state = $state<L>('idle');
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
  /** The node being recovered or rolled back (and tried again from recoverFailed). */
  recovering = $state<string | null>(null);
  /** How the last recovery got through: the ROM path shows its figures in the detail. */
  recovered = $state<RecoverResult | null>(null);
  /** The update after a recovery covers the nodes not written yet ("残りのノードへ書き込む"). */
  rest = $state(false);
  /**
   * The 構成 a release made for another 構成 was made for (otherTopology): shown
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
  /** Why the page could not get the rebooted hub back by itself (reconnectNeeded's diagnostic log). */
  reconnectNotes = $state<ReconnectNote[]>([]);
  /** Why the device the CLI gave back was not taken (lost shows it); null otherwise. */
  returnRefused = $state<ReconnectNote | null>(null);
  /** The hub's HELLO when the user connected: what "the same 親機" means after its reboot. */
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
  private backRecord = $state.raw<RollbackRecord | null>(null);
  private backRead: Promise<void> = Promise.resolve();
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
  private updated = new Set<string>();
  /** The run hubRestarting / reconnectNeeded belong to, so done says what it did. */
  private running: 'update' | 'recover' | 'rollback' = 'update';

  private link: SwapLink | null = null;
  /** Version each node was given in this run. */
  private sentVersion: Record<string, number> = {};
  /** The boot core version sent to each node in this run. */
  private coreSent: Record<string, number> = {};

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

  /**
   * What stands in the way of writing `bytes` to the checked nodes, before anything is sent: none checked, or every
   * checked node already on its app, its boot core and its recovery agent, compared as dagon-host's update_app,
   * update_core and update_agent do, against what the node answers now; what cannot be read is not called the same,
   * so the write goes and says why.
   */
  async noWrite(bytes: Uint8Array): Promise<'noneChecked' | { same: number; coreToo: boolean } | null> {
    const nodes = this.answering();
    if (nodes.length === 0) return 'noneChecked';
    const given = releaseVersions(bytes);
    const cores = releaseCoreVersions(bytes);
    const reach = this.reach;
    const at = (id: string) => reach.findIndex((n) => n.id === id);
    const first = given.get(at(nodes[0]));
    if (first === undefined) return null;
    let coreToo = false;
    for (const id of nodes) {
      const st = await this.statusOf(id);
      if (!st || !appUnchanged(bytes, at(id), st)) return null;
      const agent = releaseAgentDigest(bytes, at(id));
      if (agent && !sameBytes(st.status.slots[FACTORY_SLOT]?.digest16 ?? null, agent)) return null;
      const held = cores.get(at(id));
      if (!held) continue;
      if (!held.includes((await this.runningCore(id)) ?? NaN)) return null;
      coreToo = true;
    }
    return { same: first, coreToo };
  }

  /** What node `id` answers now; null when it cannot be read. */
  private async statusOf(id: string): Promise<{ hello: HelloResp; status: StatusResp } | null> {
    const n = this.reach.find((x) => x.id === id);
    if (!n) return null;
    return this.call((h) => h.status(n.target.kind, n.target.addr)).catch(() => null);
  }

  /** The version of the boot core node `id` runs now; null when it cannot be read. */
  private async runningCore(id: string): Promise<number | null> {
    const n = this.reach.find((x) => x.id === id);
    if (!n) return null;
    const core = await this.call((h) => h.bootCoreStatus(n.target.kind, n.target.addr)).catch(() => null);
    return core?.slots[core.active]?.version ?? null;
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
    this.stillSince = WORKING.has(to) ? Infinity : Math.min(this.stillSince, this.now());
    this.latestMissing = false;
    // Out of the hub's reboot (confirmed, stopped, lost, ...): no version to keep a reopened device by any more.
    if (to !== 'hubRestarting' && to !== 'reconnectNeeded') this.transport?.expectHub?.(null);
    if (focus) this.focusRequest++;
  }

  /** `dagon serve` needs the hub's port (device.need): close Web Serial and show cliTurn. */
  async handOver(): Promise<void> {
    this.session++;
    await this.drop();
    this.go('cliClaim', false);
  }

  /**
   * The header's 切断: the port is closed but its permission is kept (no
   * forget()), so the CLI or a later つなぐ can open it; the session waits at idle.
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
   * it had again by itself and goes on watching (ready, M67); when that fails
   * the user connects again (lost, M34). It must be the same 親機 as before
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
    this.config = SAMPLE_CONFIG;
    this.live = Object.fromEntries(SAMPLE_CONFIG.nodes.map((n) => [n.id, UNKNOWN]));
    this.go('placeSample', false);
  }

  /**
   * "パソコンとつなぐ" / "つないで、自動で見つける" / "選び直す" / "もう一度つなぐ".
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
    this.go(this.state === 'scanned' ? 'scanAgain' : 'helloDiscover');
    this.stopWatching();
    this.found = 1;
    if (!this.config) this.placeSample();
    try {
      const d = await this.call((h) => h.discover());
      if (my !== this.session) return;
      const seen = new Map(d.nodes.map((n) => [targetKey(n), n.hello]));
      seen.set('hub', d.hub);
      const now = Date.now();
      // places the sample and marks which of its nodes answered; a
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
    this.go(this.state === 'sendFailed' ? 'pushAgain' : 'push');
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
    if (this.state === 'hubRestarting' || this.state === 'reconnectNeeded') return;
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
    // ready and done move to nodesSilent by themselves while a node does not answer: it stays visible and recoverable
    // (after "応答する n つだけ先に書き込む" the ones left out are named there).
    if ((this.state === 'ready' || this.state === 'done') && this.stuck) this.go('watchAlarm', false);
    else if (this.state === 'nodesSilent' && !this.stuck) this.go('watchOk', false);
  }

  private async refreshStatus(id: string): Promise<void> {
    const n = this.reach.find((x) => x.id === id);
    if (!n || this.live[id]?.responding === false) return;
    try {
      const s = await this.call((h) => h.status(n.target.kind, n.target.addr));
      // A node without boot core slots refuses this STATUS: no row then.
      const core = await this.call((h) => h.bootCoreStatus(n.target.kind, n.target.addr)).catch(() => null);
      // An agent that predates VERSION answers null: its versions show as unknown.
      const version = await this.call((h) => h.version(n.target.kind, n.target.addr)).catch(() => null);
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
    const ev = this.state === 'ready' ? 'chooseFile' : this.state === 'nodesSilent' ? 'updateResponding' : 'otherFile';
    this.go(ev);
    if (this.state !== 'reading') return;
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
    // topology node index = position in reach, as in update().
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

  /** Node `id`'s one % for its write or recovery, at the clock's `now`; time stops counting when the work stops. */
  percentOf(id: string, now = this.now()): number {
    const k = this.track[id];
    return k ? writePercent(k, Math.min(now, this.stillSince), this.deps.trialMs?.(id) ?? null) : 0;
  }

  /** The write's check of node `id` once a file is chosen (fileReady): an unchecked node is not written. Targets keep update order. */
  choose(id: string, on: boolean): void {
    if (this.state !== 'fileReady' || !this.reach.some((n) => n.id === id)) return;
    const chosen = new Set(this.targets);
    if (on) chosen.add(id);
    else chosen.delete(id);
    this.targets = this.reach.map((n) => n.id).filter((x) => chosen.has(x));
  }

  /** "n ノードへ書き込む": dagon-host's apply on the checked nodes, children first and the hub last. */
  async update(): Promise<void> {
    const c = this.config;
    const f = this.file;
    if (!c || !f || f.verdict !== 'pass' || this.state !== 'fileReady' || this.targets.length === 0) return;
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
    this.coreSent = {};
    this.progress = Object.fromEntries(this.targets.map((id) => [id, { stage: 'waiting', percent: 0 }]));
    this.track = Object.fromEntries(this.targets.map((id) => [id, { ...NO_TRACK }]));
    // topology node index = position in reach (true for the sample, which follows its YAML).
    const reach = this.reach;
    const nodes = this.targets.map((id) => reach.findIndex((n) => n.id === id));
    let results: NodeResult[];
    try {
      results = await this.call((h) => (this.tellTrials(h), h.apply(f.bytes, topo_compile(c.yaml), nodes, (e) => void (my === this.session && this.onEvent(e)))));
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
    if (e.phase === PHASE.transfer && e.bootCore) this.coreSent[n.id] = e.version;
    const was = this.progress[n.id] ?? { stage: 'waiting', percent: 0 };
    if (e.phase === PHASE.transfer || e.phase === PHASE.verify) {
      if (was.stage !== 'writing') this.status = ANNOUNCE.A11(n.name);
      this.progress[n.id] = { stage: 'writing', percent: e.phase === PHASE.verify ? 100 : e.percent, ...(e.bootCore ? { core: e.version } : {}) };
    } else if (e.phase === PHASE.reboot && n.role === 'hub') {
      this.progress[n.id] = { stage: 'rebooting', percent: 100, ...(e.bootCore ? { core: e.version } : {}) };
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
    const now = e.atMs;
    const k = this.track[id] ?? { ...NO_TRACK };
    if (e.phase === PHASE.transfer || e.phase === PHASE.rom) {
      // A new image (after a boot core's reboot) starts its own transfer.
      const fresh = k.t0 === null || k.xferMs !== null;
      this.track[id] = { ...k, shown: e.shown, ...(fresh ? { t0: now, xferMs: null, rebootAt: null, done: false } : {}) };
    } else if (e.phase === PHASE.verify) {
      this.track[id] = { ...k, shown: e.shown, xferMs: Math.max(1, now - (k.t0 ?? now)) };
    } else if (e.phase === PHASE.reboot) {
      // A recovery may reboot without a VERIFY: its transfer ends here.
      this.track[id] = { ...k, shown: e.shown, xferMs: k.xferMs ?? (k.t0 === null ? null : Math.max(1, now - k.t0)), rebootAt: now };
    } else if (e.phase === PHASE.confirm && e.bootCore) {
      // dagon-host counted the boot core's trial up to this moment, from the same moments, so its number is not under
      // what was shown; the clock stops here, or the page would run ahead of the number the next image starts from.
      this.track[id] = { ...k, shown: e.shown, t0: null, xferMs: null, rebootAt: null };
    } else if (e.phase === PHASE.confirm) {
      this.track[id] = { ...k, done: true };
    }
  }

  /** Tells dagon-host each node's trial period, for the node's one %. */
  private tellTrials(h: Host): void {
    this.reach.forEach((n, i) => {
      const ms = this.deps.trialMs?.(n.id) ?? null;
      if (ms !== null) h.setTrialMs(i, ms);
    });
  }

  /** The hub reboots onto `version`: dagon-host reopens it for 45 s (hubRestarting), keeping only that hub. */
  private hubAway(version: number | undefined): void {
    if (version !== undefined) this.transport?.expectHub?.(version, this.hubHello);
    this.reconnectNotes = [];
    this.reconnectUntil = this.now() + HUB_RECONNECT_MS;
    this.status = ANNOUNCE.A12;
    this.go('hubTurn');
  }

  /** dagon-host gave up reopening the hub (reconnectNeeded): the dialog opens, with why. */
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
    if (n.role === 'hub' && this.state === 'hubRestarting' && stop.failure?.reason === 'NO ANSWER') return this.autoFailed();
    const why = stop.failure;
    const f = why?.running != null ? { stage: 'previous' as const } : failureStage(why?.reason ?? 'NO ANSWER');
    // A stopped update has no "cannot fix" yet: an unnamed failure is a refusal of the update.
    this.stopAt(n.id, f.stage === 'cannotFix' ? 'refused' : f.stage, f.reason);
    if (why?.running != null) this.backRead = this.recordBack(n.id, why.running, why.reason);
  }

  /** Keeps what a write that went back left: the versions and reason at once, then what the node reports now. */
  private async recordBack(id: string, returned: number, reason: string): Promise<void> {
    const sent = this.sentVersion[id] ?? this.fileVersions[id];
    const n = this.reach.find((x) => x.id === id);
    if (sent === undefined || !n) return;
    const record: RollbackRecord = { node: id, failed_version: versionText(sent), returned_version: versionText(returned), reason, node_state: null };
    this.backRecord = record;
    try {
      const { hello, status } = await this.call((h) => h.status(n.target.kind, n.target.addr));
      if (this.backRecord === record) this.backRecord = { ...record, node_state: { trial: hello.trial, last_reason: status.lastReason } };
    } catch {
      // A node that does not answer STATUS: the record says its state is not known.
    }
  }

  private stopAt(id: string, stage: Progress['stage'], reason?: string): void {
    const n = this.config!.nodes.find((x) => x.id === id)!;
    this.failed = id;
    this.progress[id] = { stage, percent: this.progress[id]?.percent ?? 0, reason };
    this.status = stage === 'previous' ? ANNOUNCE.A13previous(n.name) : stage === 'coreBack' ? ANNOUNCE.A13core(n.name) : ANNOUNCE.A13(n.name);
    this.go(this.state === 'reconnectNeeded' ? 'revertedOnReconnect' : 'nodeStuck');
    this.startWatching();
  }

  private done(): void {
    // An app left unchanged is on the package's version too.
    const v = Object.values(this.sentVersion)[0] ?? Object.values(this.fileVersions)[0];
    this.status = ANNOUNCE.A14(v === undefined ? '' : versionText(v));
    this.finished = { kind: 'updated', left: this.reach.filter((n) => !this.updated.has(n.id) && !this.skip.has(n.id)).length, ...(this.skip.size ? { leftOut: this.skip.size } : {}) };
    this.go(this.state === 'hubRestarting' ? 'confirmed' : this.state === 'reconnectNeeded' ? 'reconnected' : 'doneNoHub');
    this.startWatching();
  }

  /**
   * "もう一度つなぐ" while the hub reboots: the user chooses it again. While
   * dagon-host still reopens (hubRestarting) the chosen device goes under its next
   * reopen; after that (reconnectNeeded) dagon-host applies the confirm rule to it.
   * `onlyMine` (a device the browser reported connected, nobody chose it):
   * taken only when it is the hub on trial with the version just sent.
   * Resolves whether the device was taken.
   */
  async reconnectHub(pick = this.deps.reselect, onlyMine = false): Promise<boolean> {
    if (!pick || !this.link || (this.state !== 'hubRestarting' && this.state !== 'reconnectNeeded')) return false;
    let t: Transport;
    try {
      t = await pick();
    } catch {
      return false;
    }
    // hubRestarting: dagon-host is still reopening (its apply holds the queue), so the device goes under its next reopen now.
    if (this.state === 'hubRestarting') {
      this.swap(t);
      return true;
    }
    // reconnectNeeded: swapping, checking and confirming run as one queued step, so two devices reported at once cannot interleave.
    const link = this.link;
    const r = await this.call(async (h) => {
      const old = this.transport;
      link.use(t);
      try {
        const why = this.notThisHub(await h.hello('hub', 0), onlyMine);
        if (why) throw why;
        const confirmed = await h.confirm('hub', 0, false);
        const core = await h.bootCoreStatus('hub', 0).catch(() => null);
        const st = await h.status('hub', 0).catch(() => null);
        const hubAt = this.reach.findIndex((n) => n.role === 'hub');
        const appSame = !!st && !!this.file && appUnchanged(this.file.bytes, hubAt, st);
        this.swap(t, old);
        return { confirmed, core: core ? core.slots[core.active].version : null, appSame };
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
    this.confirmed(r.confirmed, r.core, r.appSame);
    return true;
  }

  /**
   * Why the device under the link is not the hub the update rebooted, or null.
   * HELLO carries no serial number, so "the same 親機" is the role, MCU and
   * release key of its first HELLO; `onTrial` (a device nobody chose) also
   * needs it on trial with the version just sent. One the user chose may be
   * back on its previous version: CONFIRM then stops there (stopped).
   */
  private notThisHub(h: HelloResp, onTrial: boolean): ReconnectNote | null {
    // -1 matches no version, so a device nobody chose is never kept when nothing was sent.
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

  /** reconnectNeeded: the hub confirmed on the version just sent goes on to done; on its previous version it stops there (stopped). */
  private confirmed(r: ConfirmResult, core: number | null, appSame: boolean): void {
    const hub = this.reach.find((n) => n.role === 'hub')!;
    const stage = hubBack(r, this.sentVersion[hub.id], this.coreSent[hub.id], core, appSame);
    this.alert = '';
    if (stage === 'coreBack' || stage === 'unwritten') return this.stopAt(hub.id, stage);
    if (stage === 'previous') {
      this.stopAt(hub.id, 'previous');
      this.backRead = this.recordBack(hub.id, r.version, 'ROLLED BACK');
      return;
    }
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

  /** The node a write names without an id: the stopped one (stopped, recoverFailed) or the first silent one (nodesSilent). */
  get recoverTarget(): Node | undefined {
    const id = this.state === 'recoverFailed' ? this.recovering : this.state === 'stopped' ? this.failed : this.stuck?.id;
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
   * 「書き込む」 on one node (`id`, else the stuck one): the normal transfer
   * when it answers; silent, `direct` without a ROM entry and, with one,
   * `confirm` until `confirmed`, nothing touched before that. Takes the
   * release on the screen (else the project's latest) and, for the ROM path,
   * the project's boot core. Without any release it asks for one
   * (`needFile`); `chosen` is that file.
   */
  async writeNode(o: { id?: string; chosen?: { name: string; bytes: Uint8Array }; confirmed?: boolean } = {}): Promise<'needFile' | 'direct' | 'confirm' | void> {
    const c = this.config;
    const chosen = o.chosen;
    const n = o.id ? this.reach.find((x) => x.id === o.id) : this.recoverTarget;
    if (!c || !n || !NODE_WRITE.has(this.state)) return;
    const topo = topo_compile(c.yaml);
    const index = this.reach.indexOf(n);
    // A link that fails here fails the write itself, with its reason.
    const way = await this.call((h) => h.writePath(index, topo)).catch(() => 'transfer' as const);
    if (way === 'direct') return 'direct';
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
    if (way === 'rom' && !o.confirmed) {
      // A release the host would refuse is not worth the question: the refusal comes with its reason, nothing touched.
      try {
        check_release_for(bytes, topo);
        return 'confirm';
      } catch {
        // Below.
      }
    }
    const bootcore = (await this.deps.bootcore?.(n.id).catch(() => null)) ?? null;
    const stage0 = (await this.deps.stage0?.(n.id).catch(() => null)) ?? null;
    await this.runRecovery(n, 'recover', (h, on) => h.recover(index, bytes, topo, bootcore, stage0, !!o.confirmed, on));
  }

  /** "もう一度試す" (recoverFailed): the operation that failed, again (a rollback stays a rollback). */
  async retry(): Promise<'needFile' | 'direct' | 'confirm' | void> {
    if (this.state !== 'recoverFailed') return;
    if (this.lastKind === 'rollback' && this.recovering) return this.rollback(this.recovering);
    return this.writeNode();
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

  /** "このバージョンで確定する": keeps the version a node runs on trial. */
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

  /** What ran last in recovering, so "もう一度試す" repeats it. */
  private lastKind: 'recover' | 'rollback' = 'recover';

  private async runRecovery(
    n: Node,
    kind: 'recover' | 'rollback',
    run: (h: Host, on: (e: UpdateEvent) => void) => Promise<RecoverResult>,
  ): Promise<void> {
    const my = this.session;
    this.go(this.state === 'recoverFailed' ? 'recoverAgain' : kind);
    if (this.state !== 'recovering') return;
    this.stopWatching();
    this.lastKind = kind;
    this.recovering = n.id;
    this.recovered = null;
    this.running = kind;
    this.startedAt = Date.now();
    this.progress = { ...this.progress, [n.id]: { stage: 'writing', percent: 0 } };
    this.track = { ...this.track, [n.id]: { ...NO_TRACK } };
    const on = (e: UpdateEvent) => {
      if (my !== this.session || this.reach[e.node]?.id !== n.id) return;
      this.trackEvent(n.id, e);
      if (e.phase === PHASE.rom) this.progress[n.id] = { stage: 'lastResort', percent: e.percent };
      else if (e.phase === PHASE.transfer) this.progress[n.id] = { stage: 'writing', percent: e.percent };
      // TRANSFER and REBOOT carry the version the node is given (FAILED carries the one it runs).
      if ((e.phase === PHASE.transfer || e.phase === PHASE.reboot) && !e.bootCore) this.sentVersion[n.id] = e.version;
      // The hub reboots onto it: the same reconnect and confirm rule as an update (hubRestarting / reconnectNeeded).
      if (e.phase === PHASE.reboot && n.role === 'hub') {
        this.progress[n.id] = { stage: 'rebooting', percent: 100 };
        this.hubAway(this.sentVersion[n.id]);
      }
    };
    let r: RecoverResult;
    try {
      r = await this.call((h) => (this.tellTrials(h), run(h, on)));
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
    this.track[n.id] = { ...(this.track[n.id] ?? NO_TRACK), done: true };
    this.finishRun(n, r.version);
    this.live[n.id] = { ...(this.live[n.id] ?? UNKNOWN), responding: true, lastSeen: Date.now() };
    const rest = this.file?.verdict === 'pass' ? Object.entries(this.progress).filter(([, p]) => p.stage === 'unwritten').map(([id]) => id) : [];
    this.status = kind === 'rollback' ? ANNOUNCE.A15(n.name, versionText(r.version)) : ANNOUNCE.A16(n.name, rest.length);
    // The hub's REBOOT event moved the state on while dagon-host ran.
    if ((this.state as L) === 'hubRestarting') {
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

  /** A recovery or rollback got through on `version`: what done says. */
  private finishRun(n: Node, version: number): void {
    if (this.running === 'recover' && this.file?.verdict === 'pass') this.updated.add(n.id);
    const v = versionText(version);
    this.finished = this.running === 'rollback' ? { kind: 'rolledBack', name: n.name, version: v } : { kind: 'recovered', name: n.name, version: v };
  }

  /** A recovery or rollback that did not get through, shown with dagon-host's reason. */
  private recoveryFailed(n: Node, reason: string): void {
    // The rebooted hub was not reopened in time: the user chooses it again (reconnectNeeded).
    if (this.state === 'hubRestarting' && reason === 'NO ANSWER') return this.autoFailed();
    const percent = this.progress[n.id]?.percent ?? 0;
    this.progress[n.id] = { ...failureStage(reason), percent };
    this.status = REFUSALS.includes(reason) ? refusalAnnounce(reason as Refusal) : ANNOUNCE.A13(n.name);
    this.go(this.state === 'hubRestarting' ? 'nodeStuck' : 'gaveUp');
  }

  /**
   * "最初からやり直す": the page's records go (connection, file,
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
