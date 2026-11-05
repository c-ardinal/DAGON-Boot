// What the bottom band shows in each state: cause chip, one sentence, and the
// named buttons the machine allows there.
import { BAND, BAND_CHIP, BAND_FOR, BUTTON, BUTTON_FOR, SIM, type BandCopy } from './copy.ts';
import { view, type Action, type Ctx, type L } from './machine.ts';
import type { Finish } from './operate.svelte.ts';
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
  /** L26 after the CLI gave back a device that is not this 親機 (C-389): why. */
  refused?: ReconnectNote['kind'] | null;
  stuck?: string;
  responding?: number;
  /** Nodes after the stopped one that were not written (L22). */
  unwritten?: number;
  /** The update covers the nodes left after a recovery (M50). */
  rest?: boolean;
  /** dagon demo: the 親機 is simulated, so no device list appears. */
  sim?: boolean;
  /** What the last run left (L23): the band says only that. */
  finish?: Finish | null;
}

/** 案: the cause chip on L26 when the device the CLI gave back was not taken. */
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

/** One short sentence in every operating state (W9); L01 has no band (SC-00). */
function words(s: L, c: Ctx, n: Names): BandCopy {
  switch (s) {
    case 'L02':
    case 'L03':
      return n.sim ? SIM.connect : BAND.M03;
    case 'L04':
    case 'L05':
    case 'L07':
      return c.failure ? FAILURE_BAND[c.failure] : n.sim ? SIM.connect : BAND.M03;
    case 'L06':
      return BAND.M59;
    case 'L08':
      return BAND.M60;
    case 'L09':
      return BAND.M10;
    case 'L10':
      return BAND.M61;
    case 'L11':
      return BAND.M12;
    case 'L12':
      return BAND.M13;
    case 'L13':
      // Nothing left to update first: recovery is the main action (M62).
      return c.answering === false ? BAND.M62 : BAND.M62b;
    case 'L14':
    case 'L15':
      return BAND.M63;
    case 'L20':
      return BAND.M64;
    case 'L21':
      return BAND.M26;
    case 'L22':
      return n.unwritten && n.stuck ? BAND_FOR.M29(n.stuck) : BAND.M65;
    case 'L23':
      return done(n.finish);
    case 'L28':
      return BAND.M67;
    case 'L16':
    case 'L17':
      return REFUSAL_BAND[c.refusal ?? 'source'];
    case 'L18':
    case 'L19':
    case 'L24':
      return BAND.M22;
    case 'L25':
      return BAND.M33;
    case 'L26':
      return n.refused ? { chip: REFUSED_CHIP[n.refused] ?? REFUSED_CHIP.otherHub, sentence: BAND.M34.sentence } : BAND.M34;
    case 'L27':
      return BAND.M35;
    default:
      return {};
  }
}

/** L23 says only what is true: all updated, some not (they did not answer), or one node rolled back or recovered. */
function done(f: Finish | null | undefined): BandCopy {
  if (f?.kind === 'rolledBack') return BAND_FOR.M66back(f.name, f.version);
  if (f?.kind === 'recovered') return BAND_FOR.M66fixed(f.name, f.version);
  if (f?.kind === 'updated' && f.left > 0) return BAND_FOR.M66some(f.left);
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
