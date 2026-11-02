// What the page shows for each connect state: bottom band, header state,
// status announcement and where focus goes after a user-started change.
import type { HelloResp } from '../serial/hello.ts';
import { ANNOUNCE, BAND, type BandCopy } from './copy.ts';
import type { ConnectFailure } from './connect.ts';

export type View =
  | { kind: 'idle' }
  | { kind: 'choosing' }
  | { kind: 'checking' }
  | { kind: 'connected'; hello: HelloResp }
  | { kind: 'failed'; failure: ConnectFailure }
  | { kind: 'lost' }
  /** dagon-wasm did not load; the only way on is reloading the page. */
  | { kind: 'unavailable' };

const BAND_OF: Record<ConnectFailure, BandCopy> = {
  'not-found': BAND.M04,
  busy: BAND.M05,
  denied: BAND.M06,
  'no-response': BAND.M08,
};

export function bandOf(v: View): BandCopy | null {
  switch (v.kind) {
    case 'idle':
    case 'choosing':
      return BAND.M03;
    case 'failed':
      return BAND_OF[v.failure];
    case 'lost':
      return BAND.M34;
    case 'unavailable':
      return BAND.LOAD;
    default:
      return null;
  }
}

export type HubState = 'idle' | 'checking' | 'connected' | 'lost';

export function hubOf(v: View): HubState {
  switch (v.kind) {
    case 'checking':
    case 'connected':
    case 'lost':
      return v.kind;
    default:
      return 'idle';
  }
}

/** Text of the single role="status" region. */
export function statusOf(v: View): string {
  if (v.kind === 'connected') return ANNOUNCE.A05;
  if (v.kind === 'failed') return bandOf(v)?.chip ?? '';
  return '';
}

/** Focus target after the band changed: its main button, else the next-step area. */
export function focusOf(v: View): 'button' | 'next' {
  return bandOf(v) ? 'button' : 'next';
}
