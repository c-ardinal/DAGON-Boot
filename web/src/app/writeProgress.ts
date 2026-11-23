// What the page knows about each node in an update: its last answers, where its write is and its one %,
// the chosen release's verdict, and how dagon-host's results read as the node's chip.
import type { HelloResp } from '../serial/hello.ts';
import { release_topology, releaseAppDigest, trialPercent, versionText, type ConfirmResult, type Refusal, type StatusResp, type VersionResp } from '../wasm/dagon.ts';
import { ANNOUNCE, WRITE } from './copy.ts';
import { topoFromNames, type Topo } from './define/model.ts';

/** What the page knows about one node from the node connected to the PC. */
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

export const UNKNOWN: Live = { responding: null, rttMs: null, lastSeen: null, hello: null, status: null };

/** Where one node is in an update (the node chip and the update-order row). */
export interface Progress {
  stage: 'waiting' | 'writing' | 'lastResort' | 'rebooting' | 'done' | 'unchanged' | 'silent' | 'refused' | 'previous' | 'coreBack' | 'unwritten' | 'cannotFix';
  percent: number;
  /** dagon-host's reason when the node refused (`E_FLASH`, `E_SIGNATURE`, ...). */
  reason?: string;
  /** The boot core version being written, while it is; the app's write has none. */
  core?: number;
}

/** The recovery agent's slot in STATUS. */
export const FACTORY_SLOT = 2;

export const sameBytes = (a: Uint8Array | null, b: Uint8Array): boolean => a !== null && a.length === b.length && a.every((x, i) => x === b[i]);

/** STATUS slot states (dagon_proto::payload::SlotState) that decide what the next reset runs. */
const SLOT = { pending: 2, trying: 3, confirmed: 4 } as const;

/** The running slot is the confirmed one and no slot waits for the next boot or is on trial (dagon-host's slots_settled). */
export function slotsSettled(st: StatusResp): boolean {
  return st.active === st.confirmed && st.slots[st.active]?.state === SLOT.confirmed && !st.slots.some((s) => s.state === SLOT.pending || s.state === SLOT.trying);
}

/** The package's app for a node is what it runs, and nothing else is about to: dagon-host's normal write leaves it out. */
export function appUnchanged(bytes: Uint8Array, node: number, st: { hello: HelloResp; status: StatusResp }): boolean {
  const running = st.status.slots[st.status.active]?.digest16 ?? null;
  const app = releaseAppDigest(bytes, node, st.status.active);
  return !st.hello.trial && slotsSettled(st.status) && app !== null && sameBytes(running, app);
}

/** On a node's card while it is written: what is being written now (its boot core comes first when that differs), or null when not known. */
export function writingTo(p: Progress, app: number | undefined): string | null {
  if (p.core !== undefined) return WRITE.toCore(versionText(p.core));
  return app === undefined ? null : WRITE.to(versionText(app));
}

/** Stages in which a node's water rises with its progress; a finished or stopped node drains. */
const FILLING: ReadonlySet<Progress['stage']> = new Set(['writing', 'lastResort', 'rebooting']);

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
  | { kind: 'updated'; left: number; leftOut?: number; wrote?: number; same?: number; firstWrite?: number }
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

export const NO_TRACK: WriteTrack = { shown: 0, xferMs: null, t0: null, rebootAt: null, done: false };

/** The chosen release and dagon-host's verdict on it. */
export interface ReleaseFile {
  name: string;
  size: number;
  bytes: Uint8Array;
  verdict: 'checking' | 'pass' | Refusal;
}

export const REFUSALS: readonly string[] = ['source', 'key', 'topology', 'format'];

/** dagon-host's release check (check_release_for, apply, recover) threw this: which refusal it names. */
export function refusalOf(e: unknown): Refusal {
  const m = e instanceof Error ? e.message : String(e);
  return (REFUSALS.includes(m) ? m : 'format') as Refusal;
}

/** The announcement of a refused release, the same wherever the check ran (file, recovery). */
export function refusalAnnounce(k: Refusal): string {
  return k === 'topology' ? ANNOUNCE.fileTopology : k === 'format' ? ANNOUNCE.fileFormat : ANNOUNCE.fileSource;
}

/** The file name "最新リリースの使用" shows. */
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
 * Where a node connected to the PC chosen again by hand stands: on the app just `sent`; else, when
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
export function fileTopologyOf(bytes: Uint8Array, yaml: string): Topo | null {
  try {
    const t = release_topology(bytes, yaml) as Topo | null;
    return t ? topoFromNames(t) : null;
  } catch {
    return null;
  }
}

