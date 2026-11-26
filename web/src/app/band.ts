// What the bottom band shows in each state: cause chip, one sentence, and the
// named buttons the machine allows there.
import { BAND, BAND_CHIP, BAND_FOR, BUTTON, BUTTON_FOR, DEFINE, SIM, WRITE, type BandCopy } from './copy.ts';
import { view, type Action, type Ctx, type L } from './machine.ts';
import type { Finish, Progress } from './writeProgress.ts';
import type { ReconnectNote } from '../serial/transport.ts';

export interface Button {
  action: Action;
  label: string;
}

export interface Band extends BandCopy {
  primary: Button | null;
  secondary: Button[];
}

/** Names a button needs beyond its action: the node to recover, how many still answer. */
export interface Names {
  /** lost after the CLI gave back a device that is not this PC-connected node: why. */
  refused?: ReconnectNote['kind'] | null;
  stuck?: string;
  responding?: number;
  /** How the node the write stopped at ended (stopped). */
  stopped?: Progress['stage'];
  /** Nodes after the stopped one that were not written (stopped). */
  unwritten?: number;
  /** Nodes the stopped run took back to the version they had, for a compatibility constraint (stopped). */
  realigned?: number;
  /** Nodes the stopped run left breaking a compatibility constraint (stopped). */
  incompatible?: number;
  /** The update covers the nodes left after a recovery. */
  rest?: boolean;
  /** There is a build to write: the main action writes it. */
  built?: boolean;
  /** The node connected to the PC is simulated, so no device list appears. */
  sim?: boolean;
  /** What the last run left (done): the band says only that. */
  finish?: Finish | null;
}

/** the cause chip on lost when the device the CLI gave back was not taken. */
const REFUSED_CHIP: Partial<Record<ReconnectNote['kind'], string>> = {
  otherHub: BAND_CHIP.otherHub,
  notHub: BAND_CHIP.notHub,
  silent: BAND_CHIP.silent,
};

const REFUSAL_BAND = { source: BAND.sourceRefused, key: BAND.keyRefused, format: BAND.formatRefused, topology: BAND.topologyRefused } as const;

export const FAILURE_BAND = {
  'not-found': BAND.notFound,
  busy: BAND.portBusy,
  denied: BAND.denied,
  'no-response': BAND.noResponse,
  wire: BAND.wireVersion,
  unavailable: BAND.pageLoadFailed,
} as const;

/**
 * How a failed connection is told. What the user must fix before anything
 * works opens a dialog (reason and next step); choosing no device in the
 * browser's list may be what they meant, so it is a toast.
 */
export function connectNotice(s: L): 'modal' | 'toast' | null {
  if (s === 'portFailed' || s === 'silentHub') return 'modal';
  return s === 'notChosen' ? 'toast' : null;
}

/** One short sentence in every operating state; unsupported has no band. */
function words(s: L, c: Ctx, n: Names): BandCopy {
  switch (s) {
    case 'idle':
    case 'choosing':
      return n.sim ? SIM.connect : BAND.choose;
    case 'notChosen':
    case 'portFailed':
    case 'silentHub':
      return c.failure ? FAILURE_BAND[c.failure] : n.sim ? SIM.connect : BAND.choose;
    case 'checking':
      return BAND.checking;
    case 'scanning':
      return BAND.scanning;
    case 'ready':
      return n.built ? BAND.readyBuilt : BAND.ready;
    case 'nodesSilent':
      // Nothing left to update first: recovery is the main action.
      return c.answering === false ? BAND.silentLeft : BAND.nodesSilent;
    case 'reading':
    case 'verifying':
      return BAND.verifying;
    case 'hubRestarting':
      return BAND.hubRestarting;
    case 'reconnectNeeded':
      return BAND.reconnectNeeded;
    case 'stopped':
      return withConstraints(stopped(n), n);
    case 'done':
      return done(n.finish);
    case 'cliTurn':
      return BAND.cliTurn;
    case 'fileRefused':
    case 'otherTopology':
      return REFUSAL_BAND[c.refusal ?? 'source'];
    case 'fileReady':
    case 'writing':
    case 'recovering':
      return BAND.writing;
    case 'recoverFailed':
      return BAND.recoverFailed;
    case 'lost':
      return n.refused ? { chip: REFUSED_CHIP[n.refused] ?? REFUSED_CHIP.otherHub, sentence: BAND.lost.sentence } : BAND.lost;
    default:
      return {};
  }
}

function stopped(n: Names): BandCopy {
  if (n.stopped === 'coreBack') return { sentence: WRITE.coreBack.next };
  if (n.stopped === 'previous') return { sentence: WRITE.rolled.next };
  return n.unwritten && n.stuck ? BAND_FOR.stoppedAt(n.stuck) : BAND.stopped;
}

/** A stopped run also says what it did for the release's compatibility constraints: nodes left breaking one first, else nodes taken back. */
function withConstraints(b: BandCopy, n: Names): BandCopy {
  const said = n.incompatible ? WRITE.constraints.incompatible(n.incompatible) : n.realigned ? WRITE.constraints.realigned(n.realigned) : null;
  return said && b.sentence ? { ...b, sentence: `${said}${b.sentence}` } : b;
}

/** done says only what is true: all updated, the checked ones updated, some not (they did not answer), some or all already on it, or one node rolled back or recovered. */
function done(f: Finish | null | undefined): BandCopy {
  if (f?.kind === 'rolledBack') return BAND_FOR.doneBack(f.name, f.version);
  if (f?.kind === 'recovered') return BAND_FOR.doneFixed(f.name, f.version);
  if (f?.kind === 'updated' && f.firstWrite) return BAND_FOR.doneFirst(f.firstWrite);
  if (f?.kind === 'updated' && f.left > 0) return BAND_FOR.doneSome(f.left);
  if (f?.kind === 'updated' && f.same && !f.wrote) return BAND.doneNone;
  if (f?.kind === 'updated' && f.same) return BAND_FOR.donePart(f.wrote ?? 0, f.same);
  if (f?.kind === 'updated' && f.leftOut) return BAND.doneChecked;
  return BAND.doneAll;
}

function label(a: Action, n: Names): string {
  if (a === 'recover') return BUTTON_FOR.recover(n.stuck ?? '');
  if (a === 'updateResponding') return BUTTON_FOR.updateResponding(n.responding ?? 0);
  if (a === 'update' && n.rest) return BUTTON_FOR.updateRest;
  return BUTTON[a];
}

/** 準備's next step with nothing wrong: what is true of the build now. `built`: this 構成 was built since it last changed; `progress`: the node the running build is at. */
export function prepareSentence(s: { needsSkeleton: boolean; building: boolean; built: boolean; progress?: { nth: number; total: number } | null }): string {
  if (s.needsSkeleton) return DEFINE.skeletonFirst;
  if (s.building) return s.progress ? DEFINE.buildingAt(s.progress.nth, s.progress.total) : DEFINE.buildingSentence;
  return s.built ? DEFINE.builtSentence : DEFINE.readySentence;
}

export function bandOf(s: L, c: Ctx, n: Names = {}): Band {
  const v = view(s, c);
  const button = (a: Action): Button => ({ action: a, label: label(a, n) });
  return {
    ...words(s, c, n),
    primary: v.primary && button(v.primary),
    secondary: v.secondary.map(button),
  };
}
