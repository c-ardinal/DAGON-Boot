// Operating-mode states, their main and side actions and every
// transition. Pure data plus two lookups: the page asks `next` where an event
// leads and `view` what the bottom band offers; nothing here touches the DOM.

export type L =
  | 'unsupported' | 'idle' | 'choosing' | 'notChosen' | 'portFailed' | 'checking' | 'silentHub' | 'scanning' | 'scanned' | 'sending'
  | 'sendFailed' | 'ready' | 'nodesSilent' | 'reading' | 'verifying' | 'fileRefused' | 'otherTopology' | 'fileReady' | 'writing' | 'hubRestarting'
  | 'reconnectNeeded' | 'stopped' | 'done' | 'recovering' | 'recoverFailed' | 'lost' | 'sampleError' | 'cliTurn';

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
  /** The header's 切断: back to idle, not lost. */
  | 'disconnect';

/** Why connecting stopped (notChosen, portFailed, silentHub). */
export type ConnectFailure = 'not-found' | 'busy' | 'denied' | 'no-response' | 'unavailable';

export interface Ctx {
  /** A configuration is on the canvas. */
  hasConfig: boolean;
  failure?: ConnectFailure;
  /** Why the chosen file was refused (fileRefused, otherTopology). */
  refusal?: 'source' | 'key' | 'topology' | 'format';
  /** Some node answers (nodesSilent: updating those comes first); absent counts as yes. */
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

const toLost = { lost: 'lost' } as const;
/** Connected and idle: the header's 切断 releases the hub. */
const leave = { lost: 'lost', disconnect: 'idle' } as const;

export const MACHINE: Record<L, Spec> = {
  unsupported: { step: null, primary: 'copyUrl', on: {} },
  idle: {
    step: (c) => (c.hasConfig ? 1 : 2),
    primary: (c) => (c.hasConfig ? 'connect' : 'connectDiscover'),
    // The sample comes from the start screen, not from here.
    on: { connect: 'choosing', connectDiscover: 'choosing', placeSample: 'idle', sampleFailed: 'sampleError' },
  },
  // The chooser is open: the button stays (busy) and a second press changes nothing.
  choosing: { step: 1, primary: 'connect', on: { connect: 'choosing', dismissed: 'notChosen', openFailed: 'portFailed', wasmFailed: 'portFailed', chosen: 'checking' } },
  notChosen: { step: 1, primary: 'connect', on: { connect: 'choosing' } },
  portFailed: { step: 1, primary: (c) => (c.failure === 'unavailable' ? 'reload' : 'connect'), on: { connect: 'choosing' } },
  checking: {
    step: 1,
    // The 構成 is the project's YAML: connecting never asks whether the 親機 holds it.
    on: { noAnswer: 'silentHub', helloDiscover: 'scanning', helloConfigured: 'ready', ...leave },
  },
  silentHub: { step: 1, primary: 'reselect', on: { reselect: 'choosing' } },
  scanning: { step: 2, on: { scanned: 'scanned', ...toLost } },
  scanned: { step: 3, primary: 'push', secondary: ['scanAgain'], on: { push: 'sending', scanAgain: 'scanning', ...leave } },
  sending: { step: 3, on: { pushed: 'ready', pushFailed: 'sendFailed', ...toLost } },
  sendFailed: { step: 3, primary: 'pushAgain', on: { pushAgain: 'sending', ...leave } },
  ready: {
    step: 4,
    primary: 'chooseFile',
    secondary: ['useLatest'],
    // rollback: M38's "前のバージョンに戻す" for a node left on trial.
    on: { push: 'sending', watchAlarm: 'nodesSilent', chooseFile: 'reading', useLatest: 'reading', cliClaim: 'cliTurn', rollback: 'recovering', ...leave },
  },
  nodesSilent: {
    step: 4,
    // Updating the nodes that answer comes first; recovery is the side action.
    primary: (c) => (c.answering === false ? 'recover' : 'updateResponding'),
    secondary: (c) => (c.answering === false ? [] : ['recover']),
    on: { recover: 'recovering', watchOk: 'ready', updateResponding: 'reading', rollback: 'recovering', ...leave },
  },
  reading: { step: 4, on: { read: 'verifying', ...leave } },
  verifying: { step: 4, on: { badSource: 'fileRefused', topoDiff: 'otherTopology', pass: 'fileReady', ...leave } },
  fileRefused: { step: 4, primary: 'otherFile', secondary: ['useLatest'], on: { otherFile: 'reading', useLatest: 'reading', ...leave } },
  otherTopology: {
    step: 4,
    primary: 'otherFile',
    // The 構成 is replaced in 定義 (ファイルの構成を取り込む), never from here.
    secondary: ['defineFix'],
    on: { otherFile: 'reading', ...leave },
  },
  fileReady: { step: 5, primary: 'update', secondary: ['otherFile'], on: { update: 'writing', otherFile: 'reading', watchAlarm: 'nodesSilent', ...leave } },
  writing: { step: 5, on: { hubTurn: 'hubRestarting', nodeStuck: 'stopped', doneNoHub: 'done', ...toLost } },
  // nodeStuck: the hub came back in time but on its previous version.
  hubRestarting: { step: 5, on: { confirmed: 'done', autoFailed: 'reconnectNeeded', nodeStuck: 'stopped' } },
  reconnectNeeded: { step: 5, primary: 'reconnect', on: { reconnect: 'reconnectNeeded', reconnected: 'done', revertedOnReconnect: 'stopped' } },
  stopped: { step: 5, primary: 'recover', on: { recover: 'recovering', ...leave } },
  done: { step: 6, secondary: ['otherFile'], on: { watchAlarm: 'nodesSilent', rollback: 'recovering', otherFile: 'reading', ...leave } },
  // hubTurn: recovering or rolling back the hub reboots it; reconnecting is hubRestarting / reconnectNeeded's.
  recovering: { step: 'recover', on: { fixedNoRest: 'done', fixedRest: 'fileReady', gaveUp: 'recoverFailed', hubTurn: 'hubRestarting', ...toLost } },
  recoverFailed: { step: 'recover', primary: 'recoverAgain', secondary: ['copyLog'], on: { recoverAgain: 'recovering', ...leave } },
  lost: { step: 1, primary: 'reconnect', on: { reconnect: 'choosing' } },
  sampleError: { step: 2, primary: 'retrySample', secondary: ['placeInDefine'], on: { retrySample: 'idle' } },
  cliTurn: { step: 4, secondary: ['showJobs'], on: { cliReturn: 'ready', ...toLost } },
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
    case 'unsupported': case 'idle': case 'choosing': case 'notChosen': case 'portFailed': case 'silentHub': case 'sampleError':
      return 'idle';
    case 'checking':
      return 'checking';
    case 'hubRestarting': case 'reconnectNeeded':
      return 'reconnecting';
    case 'lost':
      return 'lost';
    case 'cliTurn':
      return 'cli';
    default:
      return 'connected';
  }
}

/**
 * States in which the header's つなぐ / 切断 cannot be used:
 * choosing or scanning, sending, updating, reconnecting, recovering, and the
 * CLI holding the hub.
 */
export const HUB_LOCKED: ReadonlySet<L> = new Set(['choosing', 'scanning', 'sending', 'writing', 'hubRestarting', 'reconnectNeeded', 'recovering', 'cliTurn']);

/** States in which nodes are watched every round (no transfer running). */
export const WATCHED: ReadonlySet<L> = new Set(['scanned', 'ready', 'nodesSilent', 'fileReady', 'done']);
