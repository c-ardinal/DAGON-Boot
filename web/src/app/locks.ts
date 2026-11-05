// The state-lock table: one place that says which
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
  'site',
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
  /** The public demo page: the simulated desk only, no serve, files, devices or keys. */
  site: boolean;
  /** No `dagon serve` behind the page. */
  cliNone: boolean;
}

export function activeStates(i: LockInput): LockState[] {
  return LOCK_ORDER.filter((s) => i[s]);
}

/** A control that is also a case of a broader one: whatever stops the broader one stops it too. */
const PART_OF: Partial<Record<Control, Control>> = { open: 'project' };

/** Why `control` is stopped now, or null when it can be used. */
export function lockOf(control: Control, states: readonly LockState[]): string | null {
  const broader = PART_OF[control];
  for (const s of states) {
    const why = LOCK.states[s].why as Partial<Record<Control, string>>;
    const r = why[control] ?? (broader && why[broader]);
    if (r) return r;
  }
  return null;
}

/** States that hold for the whole page, never named in the status line. */
const STANDING: readonly LockState[] = ['site', 'cliNone'];

/** The state's short name for the status line; 待機 or 未接続 when nothing runs. */
export function stateName(states: readonly LockState[]): string {
  const s = states.find((x) => !STANDING.includes(x));
  return s ? LOCK.states[s].name : LOCK.idle;
}
