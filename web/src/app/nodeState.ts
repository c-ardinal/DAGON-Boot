import { NODE, STATE, WRITE } from './copy.ts';
import type { Progress } from './operate.svelte.ts';

/** The six node states, each with its own mark (shape + colour) at the card's top-right. */
export type NodeState = keyof typeof STATE;

/** The Lucide shape of each state's mark: a different shape for each, so colour is never the only cue. */
export const STATE_ICON = {
  idle: 'unplug',
  alive: 'activity',
  silent: 'circle-dashed',
  busy: 'loader',
  attention: 'triangle-alert',
  error: 'octagon-alert',
} as const satisfies Record<NodeState, string>;

export interface StateInput {
  /** The page is connected to the 親機. */
  connected: boolean;
  /** Last watch result: null before the first answer or miss. */
  responding?: boolean | null;
  /** The node runs a new version on trial (waits for its check). */
  trial?: boolean;
  /** The chip the page set for the node (writing, refused, went back …), if any. */
  chip?: string;
  /** The node is being written (its water level shows). */
  writing?: boolean;
  /** The node is outside what writing reaches (書き込めない). */
  unreachable?: boolean;
}

/** One state per node, strongest first: error, attention, busy, then what the watch saw. */
export function nodeState(s: StateInput): NodeState {
  if (s.chip?.startsWith('×') || s.chip?.startsWith('!')) return 'error';
  if (s.chip?.startsWith('▲') || s.chip?.startsWith('↺') || s.trial) return 'attention';
  if (s.writing) return 'busy';
  if (!s.connected || s.unreachable || s.responding == null) return 'idle';
  return s.responding ? 'alive' : 'silent';
}

/**
 * What a write left on a card: red only for a node that failed. A node the
 * stopped write never reached did not fail, so its state stays the watch's.
 * Nothing once confirmed or while it runs.
 */
const RESULT: Partial<Record<Progress['stage'], { word: string; silent: boolean }>> = {
  previous: { word: WRITE.previous, silent: false },
  coreBack: { word: WRITE.coreBack.label, silent: false },
  refused: { word: WRITE.failed, silent: true },
  silent: { word: WRITE.failed, silent: true },
  cannotFix: { word: WRITE.failed, silent: true },
  unwritten: { word: NODE.unwritten, silent: false },
  unchanged: { word: NODE.unchanged, silent: false },
};

export function resultChip(stage: Progress['stage']): { chip: string; spoken: string; silent: boolean } | null {
  const r = RESULT[stage];
  return r ? { chip: r.word, spoken: r.word.slice(2), silent: r.silent } : null;
}

/** While any node needs attention or failed, the other busy lights stand still (one moving thing at a time). */
export const lightsStill = (states: readonly NodeState[]): boolean => states.some((s) => s === 'error' || s === 'attention');
