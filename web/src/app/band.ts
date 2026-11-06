// What the bottom band shows in each state: cause chip, one sentence, and the
// named buttons the machine allows there.
import { BAND, BAND_CHIP, BAND_FOR, BUTTON, BUTTON_FOR, SIM, WRITE, type BandCopy } from './copy.ts';
import { view, type Action, type Ctx, type L } from './machine.ts';
import type { Finish, Progress } from './operate.svelte.ts';
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
  /** lost after the CLI gave back a device that is not this 親機: why. */
  refused?: ReconnectNote['kind'] | null;
  stuck?: string;
  responding?: number;
  /** How the node the write stopped at ended (stopped). */
  stopped?: Progress['stage'];
  /** Nodes after the stopped one that were not written (stopped). */
  unwritten?: number;
  /** The update covers the nodes left after a recovery. */
  rest?: boolean;
  /** dagon demo: the 親機 is simulated, so no device list appears. */
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

const REFUSAL_BAND = { source: BAND.M18, key: BAND.M18b, format: BAND.M19, topology: BAND.M20 } as const;

export const FAILURE_BAND = {
  'not-found': BAND.M04,
  busy: BAND.M05,
  denied: BAND.M06,
  'no-response': BAND.M08,
  unavailable: BAND.M57,
} as const;

/**
 * How a failed connection is told. What the user must fix before anything
 * works opens a dialog (reason and next step); choosing no device in the
 * browser's list may be what they meant, so it is a toast.
 */
export function connectNotice(s: L): 'modal' | 'toast' | null {
  if (s === 'portFailed' || s === 'silentHub' || s === 'sendFailed') return 'modal';
  return s === 'notChosen' ? 'toast' : null;
}

/** One short sentence in every operating state; unsupported has no band. */
function words(s: L, c: Ctx, n: Names): BandCopy {
  switch (s) {
    case 'idle':
    case 'choosing':
      return n.sim ? SIM.connect : BAND.M03;
    case 'notChosen':
    case 'portFailed':
    case 'silentHub':
      return c.failure ? FAILURE_BAND[c.failure] : n.sim ? SIM.connect : BAND.M03;
    case 'checking':
      return BAND.M59;
    case 'scanning':
      return BAND.M60;
    case 'scanned':
      return BAND.M10;
    case 'sending':
      return BAND.M61;
    case 'sendFailed':
      return BAND.M12;
    case 'ready':
      return BAND.M13;
    case 'nodesSilent':
      // Nothing left to update first: recovery is the main action.
      return c.answering === false ? BAND.M62 : BAND.M62b;
    case 'reading':
    case 'verifying':
      return BAND.M63;
    case 'hubRestarting':
      return BAND.M64;
    case 'reconnectNeeded':
      return BAND.M26;
    case 'stopped':
      if (n.stopped === 'coreBack') return { sentence: WRITE.coreBack.next };
      if (n.stopped === 'previous') return { sentence: WRITE.rolled.next };
      return n.unwritten && n.stuck ? BAND_FOR.M29(n.stuck) : BAND.M65;
    case 'done':
      return done(n.finish);
    case 'cliTurn':
      return BAND.M67;
    case 'fileRefused':
    case 'otherTopology':
      return REFUSAL_BAND[c.refusal ?? 'source'];
    case 'fileReady':
    case 'writing':
    case 'recovering':
      return BAND.M22;
    case 'recoverFailed':
      return BAND.M33;
    case 'lost':
      return n.refused ? { chip: REFUSED_CHIP[n.refused] ?? REFUSED_CHIP.otherHub, sentence: BAND.M34.sentence } : BAND.M34;
    default:
      return {};
  }
}

/** done says only what is true: all updated, the checked ones updated, some not (they did not answer), or one node rolled back or recovered. */
function done(f: Finish | null | undefined): BandCopy {
  if (f?.kind === 'rolledBack') return BAND_FOR.M66back(f.name, f.version);
  if (f?.kind === 'recovered') return BAND_FOR.M66fixed(f.name, f.version);
  if (f?.kind === 'updated' && f.left > 0) return BAND_FOR.M66some(f.left);
  if (f?.kind === 'updated' && f.leftOut) return BAND.M66checked;
  return BAND.M66;
}

function label(a: Action, n: Names): string {
  if (a === 'recover') return BUTTON_FOR.recover(n.stuck ?? '');
  if (a === 'updateResponding') return BUTTON_FOR.updateResponding(n.responding ?? 0);
  if (a === 'update' && n.rest) return BUTTON_FOR.updateRest;
  return BUTTON[a];
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
