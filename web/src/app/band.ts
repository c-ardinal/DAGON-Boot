// What the bottom band shows in each state: cause chip, one sentence, and the
// named buttons the machine allows there.
import { BAND, BAND_FOR, BUTTON, BUTTON_FOR, type BandCopy } from './copy.ts';
import { view, type Action, type Ctx, type L } from './machine.ts';

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
  stuck?: string;
  responding?: number;
  /** Nodes after the stopped one that were not written (L22). */
  unwritten?: number;
}

const REFUSAL_BAND = { source: BAND.M18, key: BAND.M18b, format: BAND.M19, topology: BAND.M20 } as const;

export const FAILURE_BAND = {
  'not-found': BAND.M04,
  busy: BAND.M05,
  denied: BAND.M06,
  'no-response': BAND.M08,
  unavailable: BAND.M57,
} as const;

function words(s: L, c: Ctx, n: Names): BandCopy {
  switch (s) {
    case 'L02':
    case 'L03':
      return BAND.M03;
    case 'L04':
    case 'L05':
    case 'L07':
      return c.failure ? FAILURE_BAND[c.failure] : BAND.M03;
    case 'L09':
      return BAND.M10;
    case 'L11':
      return BAND.M12;
    case 'L12':
      return BAND.M13;
    case 'L13':
      return BAND.M14;
    case 'L22':
      return n.unwritten && n.stuck ? BAND_FOR.M29(n.stuck) : BAND.M14;
    case 'L16':
    case 'L17':
      return REFUSAL_BAND[c.refusal ?? 'source'];
    case 'L18':
    case 'L19':
    case 'L24':
      return BAND.M22;
    case 'L26':
      return BAND.M34;
    case 'L27':
      return BAND.M35;
    default:
      return {};
  }
}

function label(a: Action, n: Names): string {
  if (a === 'recover') return BUTTON_FOR.recover(n.stuck ?? '');
  if (a === 'updateResponding') return BUTTON_FOR.updateResponding(n.responding ?? 0);
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
