// The state-lock table (DES-UX3-182, round8d): one place that says which
// operation each state stops and why. Buttons, menus, keys and commands all
// ask lockOf(); a stopped operation stays visible (dimmed) and pressing it
// shows the reason in the information style. Red is never used for these.
import { LOCK } from './copy.ts';

export type Control = keyof typeof LOCK.controls;
export type LockState = keyof typeof LOCK.states;

/** States in the table's order: when several hold, the leftmost one's reason is shown. */
export const LOCK_ORDER: readonly LockState[] = [
  'demo',
  'updating',
  'recovering',
  'flashing',
  'building',
  'unsaved',
  'connecting',
  'disconnected',
  'thinking',
  'cliNone',
];

/** What the page knows now, reduced to the table's states. */
export interface LockInput {
  /** `dagon demo` is playing its desk (none of the page's own controls may change it). */
  demo: boolean;
  updating: boolean;
  recovering: boolean;
  flashing: boolean;
  /** A CLI job other than writing runs (project, build, sign, release, conformance). */
  building: boolean;
  /** The 構成 has an edit not saved yet (it saves by itself within about 0.5 s). */
  unsaved: boolean;
  connecting: boolean;
  /** No 親機 is connected. */
  disconnected: boolean;
  /** DAGON is waiting for its agent's answer. */
  thinking: boolean;
  /** No `dagon serve` behind the page. */
  cliNone: boolean;
}

export function activeStates(i: LockInput): LockState[] {
  return LOCK_ORDER.filter((s) => i[s]);
}

/** Why `control` is stopped now, or null when it can be used. */
export function lockOf(control: Control, states: readonly LockState[]): string | null {
  for (const s of states) {
    const why = (LOCK.states[s].why as Partial<Record<Control, string>>)[control];
    if (why) return why;
  }
  return null;
}

/** The state's short name for the status line; 待機 or 未接続 when nothing runs. */
export function stateName(states: readonly LockState[]): string {
  const s = states.find((x) => x !== 'cliNone');
  return s ? LOCK.states[s].name : LOCK.idle;
}
