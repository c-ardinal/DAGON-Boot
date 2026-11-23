// The operate states and actions the Console's screen groups by what it shows and allows.
import type { Chip } from '../../graph/Graph.svelte';
import { NODE, WRITE } from '../copy.ts';
import type { Control } from '../locks.ts';
import type { Action } from '../machine.ts';
import { resultChip } from '../nodeState.ts';
import { writingTo, type Progress } from '../operate.svelte.ts';

/** Buttons whose procedures this page runs; the others show as unavailable. */
export const RUNS: ReadonlySet<Action> = new Set([
  'connectDiscover', 'connect', 'reselect', 'reconnect', 'reload',
  'chooseFile', 'updateResponding', 'otherFile', 'update', 'useLatest', 'recover', 'recoverAgain', 'defineFix',
]);

/** States 書込 writes in directly: the node connected to the PC answers and nothing runs. */
export const WRITABLE: ReadonlySet<string> = new Set(['ready', 'nodesSilent', 'done']);
/** States that show the update's progress (node chips, percent, the dot number). */
export const UPDATING: ReadonlySet<string> = new Set(['writing', 'hubRestarting', 'reconnectNeeded', 'stopped', 'done', 'recovering', 'recoverFailed']);
/** States whose dot number and update-order rows show percent. */
export const PERCENT: ReadonlySet<string> = new Set(['writing', 'hubRestarting', 'reconnectNeeded', 'stopped', 'recovering']);
/** States in which the mode cannot change: a write or its recovery is under way. */
export const BUSY: readonly string[] = ['writing', 'hubRestarting', 'reconnectNeeded', 'stopped', 'recovering'];

/** Band actions as lock-table operations. */
export const CONTROL_OF: Partial<Record<Action, Control>> = {
  chooseFile: 'file', updateResponding: 'file', otherFile: 'file', useLatest: 'update', update: 'update',
  recover: 'recover', recoverAgain: 'recover', connect: 'connect', connectDiscover: 'connect', reselect: 'connect', reconnect: 'connect',
};

const CHIP: Record<Progress['stage'], string> = {
  waiting: NODE.waiting,
  writing: NODE.writing,
  lastResort: NODE.lastResort,
  cannotFix: NODE.cannotFix,
  rebooting: NODE.rebooting,
  done: NODE.done,
  silent: NODE.silent,
  refused: NODE.refusedOther,
  previous: NODE.previous,
  coreBack: WRITE.coreBack.label,
  unwritten: NODE.unwritten,
  unchanged: NODE.unchanged,
};

/** A stage (or a connection state of the same words) without its mark: the timeline's and the status row's word. */
export const stageWord = (s: string) => (CHIP[s as Progress['stage']] ?? s).replace(/^\S+\s/, '');

/** Stages in which a card shows → the new version and its one %. */
const FILLING_STAGES: ReadonlySet<Progress['stage']> = new Set(['writing', 'lastResort', 'rebooting']);

/** On the cards while writing: → the new version (`to`); after it, what the write left (resultChip). */
export function cardChipOf(p: Progress, to: number | undefined): Chip | null {
  if (FILLING_STAGES.has(p.stage)) {
    const said = writingTo(p, to);
    return said === null ? null : { chip: said, spoken: said, silent: false };
  }
  return resultChip(p.stage);
}
