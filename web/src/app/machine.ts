// Operating-mode states (L01–L28), their main and side actions and every
// transition. Pure data plus two lookups: the page asks `next` where an event
// leads and `view` what the bottom band offers; nothing here touches the DOM.

export type L =
  | 'L01' | 'L02' | 'L03' | 'L04' | 'L05' | 'L06' | 'L07' | 'L08' | 'L09' | 'L10'
  | 'L11' | 'L12' | 'L13' | 'L14' | 'L15' | 'L16' | 'L17' | 'L18' | 'L19' | 'L20'
  | 'L21' | 'L22' | 'L23' | 'L24' | 'L25' | 'L26' | 'L27' | 'L28';

/** What a bottom-band button does. */
export type Action =
  | 'copyUrl' | 'connectDiscover' | 'connect' | 'placeSample' | 'reselect' | 'reload'
  | 'push' | 'pushAgain' | 'scanAgain' | 'defineFix' | 'chooseFile' | 'useLatest'
  | 'recover' | 'updateResponding' | 'otherFile' | 'update'
  | 'reconnect' | 'recoverAgain' | 'copyLog' | 'retrySample' | 'placeInDefine' | 'showJobs';

/** Actions that leave this page or mode instead of moving the machine. */
export const LEAVING: ReadonlySet<Action> = new Set(['copyUrl', 'reload', 'defineFix', 'copyLog', 'placeInDefine', 'showJobs']);

/** Things that happen: button presses (same names as their action) and results. */
export type Ev =
  | Exclude<Action, 'copyUrl' | 'reload' | 'defineFix' | 'copyLog' | 'placeInDefine' | 'showJobs'>
  | 'dismissed' | 'openFailed' | 'wasmFailed' | 'chosen' | 'noAnswer'
  | 'helloDiscover' | 'helloConfigured' | 'scanned' | 'pushed' | 'pushFailed'
  | 'watchAlarm' | 'watchOk' | 'read' | 'badSource' | 'topoDiff' | 'pass'
  | 'hubTurn' | 'nodeStuck' | 'doneNoHub' | 'confirmed' | 'autoFailed' | 'reconnected' | 'revertedOnReconnect'
  | 'fixedNoRest' | 'fixedRest' | 'gaveUp' | 'rollback' | 'sampleFailed' | 'cliClaim' | 'cliReturn' | 'lost'
  /** The header's 切断: back to L02, not L26. */
  | 'disconnect';

/** Why connecting stopped (L04, L05, L07). */
export type ConnectFailure = 'not-found' | 'busy' | 'denied' | 'no-response' | 'unavailable';

export interface Ctx {
  /** A configuration is on the canvas. */
  hasConfig: boolean;
  failure?: ConnectFailure;
  /** Why the chosen file was refused (L16, L17). */
  refusal?: 'source' | 'key' | 'topology' | 'format';
  /** Some node answers (L13: updating those comes first); absent counts as yes. */
  answering?: boolean;
}

/** Header step: 01–06, or recovery. */
export type Step = 1 | 2 | 3 | 4 | 5 | 6 | 'recover';

export interface Spec {
  step: Step | ((c: Ctx) => Step) | null;
  primary?: Action | ((c: Ctx) => Action);
  secondary?: Action[] | ((c: Ctx) => Action[]);
  on: Partial<Record<Ev, L>>;
}

const toLost = { lost: 'L26' } as const;
/** Connected and idle: the header's 切断 releases the hub (ui-design §6.2). */
const leave = { lost: 'L26', disconnect: 'L02' } as const;

export const MACHINE: Record<L, Spec> = {
  L01: { step: null, primary: 'copyUrl', on: {} },
  L02: {
    step: (c) => (c.hasConfig ? 1 : 2),
    primary: (c) => (c.hasConfig ? 'connect' : 'connectDiscover'),
    // The sample comes from the start screen (SC-S0), not from here.
    on: { connect: 'L03', connectDiscover: 'L03', placeSample: 'L02', sampleFailed: 'L27' },
  },
  // The chooser is open: the button stays (busy) and a second press changes nothing.
  L03: { step: 1, primary: 'connect', on: { connect: 'L03', dismissed: 'L04', openFailed: 'L05', wasmFailed: 'L05', chosen: 'L06' } },
  L04: { step: 1, primary: 'connect', on: { connect: 'L03' } },
  L05: { step: 1, primary: (c) => (c.failure === 'unavailable' ? 'reload' : 'connect'), on: { connect: 'L03' } },
  L06: {
    step: 1,
    // The 構成 is the project's YAML: connecting never asks whether the 親機 holds it.
    on: { noAnswer: 'L07', helloDiscover: 'L08', helloConfigured: 'L12', ...leave },
  },
  L07: { step: 1, primary: 'reselect', on: { reselect: 'L03' } },
  L08: { step: 2, on: { scanned: 'L09', ...toLost } },
  L09: { step: 3, primary: 'push', secondary: ['scanAgain'], on: { push: 'L10', scanAgain: 'L08', ...leave } },
  L10: { step: 3, on: { pushed: 'L12', pushFailed: 'L11', ...toLost } },
  L11: { step: 3, primary: 'pushAgain', on: { pushAgain: 'L10', ...leave } },
  L12: {
    step: 4,
    primary: 'chooseFile',
    secondary: ['useLatest'],
    // rollback: M38's "前のバージョンに戻す" for a node left on trial.
    on: { push: 'L10', watchAlarm: 'L13', chooseFile: 'L14', useLatest: 'L14', cliClaim: 'L28', rollback: 'L24', ...leave },
  },
  L13: {
    step: 4,
    // Updating the nodes that answer comes first; recovery is the side action.
    primary: (c) => (c.answering === false ? 'recover' : 'updateResponding'),
    secondary: (c) => (c.answering === false ? [] : ['recover']),
    on: { recover: 'L24', watchOk: 'L12', updateResponding: 'L14', rollback: 'L24', ...leave },
  },
  L14: { step: 4, on: { read: 'L15', ...leave } },
  L15: { step: 4, on: { badSource: 'L16', topoDiff: 'L17', pass: 'L18', ...leave } },
  L16: { step: 4, primary: 'otherFile', secondary: ['useLatest'], on: { otherFile: 'L14', useLatest: 'L14', ...leave } },
  L17: {
    step: 4,
    primary: 'otherFile',
    // The 構成 is replaced in 定義 (ファイルの構成を取り込む), never from here.
    secondary: ['defineFix'],
    on: { otherFile: 'L14', ...leave },
  },
  L18: { step: 5, primary: 'update', secondary: ['otherFile'], on: { update: 'L19', otherFile: 'L14', watchAlarm: 'L13', ...leave } },
  L19: { step: 5, on: { hubTurn: 'L20', nodeStuck: 'L22', doneNoHub: 'L23', ...toLost } },
  // nodeStuck: the hub came back in time but on its previous version.
  L20: { step: 5, on: { confirmed: 'L23', autoFailed: 'L21', nodeStuck: 'L22' } },
  L21: { step: 5, primary: 'reconnect', on: { reconnect: 'L21', reconnected: 'L23', revertedOnReconnect: 'L22' } },
  L22: { step: 5, primary: 'recover', on: { recover: 'L24', ...leave } },
  L23: { step: 6, secondary: ['otherFile'], on: { watchAlarm: 'L13', rollback: 'L24', otherFile: 'L14', ...leave } },
  // hubTurn: recovering or rolling back the hub reboots it; reconnecting is L20 / L21's.
  L24: { step: 'recover', on: { fixedNoRest: 'L23', fixedRest: 'L18', gaveUp: 'L25', hubTurn: 'L20', ...toLost } },
  L25: { step: 'recover', primary: 'recoverAgain', secondary: ['copyLog'], on: { recoverAgain: 'L24', ...leave } },
  L26: { step: 1, primary: 'reconnect', on: { reconnect: 'L03' } },
  L27: { step: 2, primary: 'retrySample', secondary: ['placeInDefine'], on: { retrySample: 'L02' } },
  L28: { step: 4, secondary: ['showJobs'], on: { cliReturn: 'L12', ...toLost } },
};

/** Where `ev` leads from `s`; the same state when `ev` does not apply there. */
export function next(s: L, ev: Ev): L {
  return MACHINE[s].on[ev] ?? s;
}

export interface View {
  step: Step | null;
  primary: Action | null;
  secondary: Action[];
}

export function view(s: L, c: Ctx): View {
  const m = MACHINE[s];
  const pick = <T>(v: T | ((c: Ctx) => T) | undefined): T | undefined =>
    typeof v === 'function' ? (v as (c: Ctx) => T)(c) : v;
  return { step: pick(m.step) ?? null, primary: pick(m.primary) ?? null, secondary: pick(m.secondary) ?? [] };
}

/** Connection shown in the header for a state. */
export type HubState = 'idle' | 'checking' | 'connected' | 'reconnecting' | 'lost' | 'cli';

export function hubOf(s: L): HubState {
  switch (s) {
    case 'L01': case 'L02': case 'L03': case 'L04': case 'L05': case 'L07': case 'L27':
      return 'idle';
    case 'L06':
      return 'checking';
    case 'L20': case 'L21':
      return 'reconnecting';
    case 'L26':
      return 'lost';
    case 'L28':
      return 'cli';
    default:
      return 'connected';
  }
}

/**
 * States in which the header's つなぐ / 切断 cannot be used (ui-design §7.1):
 * choosing or scanning, sending, updating, reconnecting, recovering, and the
 * CLI holding the hub.
 */
export const HUB_LOCKED: ReadonlySet<L> = new Set(['L03', 'L08', 'L10', 'L19', 'L20', 'L21', 'L24', 'L28']);

/** States in which nodes are watched every round (no transfer running). */
export const WATCHED: ReadonlySet<L> = new Set(['L09', 'L12', 'L13', 'L18', 'L23']);
