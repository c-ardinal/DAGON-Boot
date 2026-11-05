import { describe, expect, test } from 'vitest';
import { HUB_LOCKED, LEAVING, MACHINE, hubOf, next, view, type Ctx, type Ev, type L } from './machine.ts';

const STATES = Object.keys(MACHINE) as L[];
const CTXS: Ctx[] = [
  { hasConfig: false },
  { hasConfig: true },
  { hasConfig: true, failure: 'unavailable' },
  { hasConfig: false, failure: 'busy' },
];

describe('operating-mode state machine', () => {
  test('has the 28 states', () => {
    expect(STATES).toHaveLength(28);
  });

  test.each(STATES)('%s: at most one main action and two side actions, never the same one twice', (s) => {
    for (const c of CTXS) {
      const v = view(s, c);
      expect(v.secondary.length).toBeLessThanOrEqual(2);
      expect(v.primary === null || !v.secondary.includes(v.primary)).toBe(true);
    }
  });

  test.each(STATES)('%s: no dead end (a way out by the user or by a bounded result)', (s) => {
    const leaves = Object.values(MACHINE[s].on).some((to) => to !== s);
    const primaries = CTXS.map((c) => view(s, c).primary);
    expect(leaves || primaries.every((p) => p !== null && LEAVING.has(p))).toBe(true);
  });

  test.each(STATES)('%s: every button either moves the machine or leaves the page', (s) => {
    for (const c of CTXS) {
      const v = view(s, c);
      for (const a of [v.primary, ...v.secondary]) {
        if (a === null || LEAVING.has(a)) continue;
        expect(MACHINE[s].on[a as Ev], `${s} ${a}`).toBeDefined();
      }
    }
  });

  test('every target is a state and every state is reachable from the start', () => {
    const seen = new Set<L>(['unsupported', 'idle']);
    const todo: L[] = ['idle'];
    while (todo.length) {
      for (const to of Object.values(MACHINE[todo.pop()!].on)) {
        expect(STATES).toContain(to);
        if (!seen.has(to)) {
          seen.add(to);
          todo.push(to);
        }
      }
    }
    expect([...seen].sort()).toEqual([...STATES].sort());
  });

  test('a lost device ends in lost from every connected state except the hub restart (hubRestarting, reconnectNeeded)', () => {
    for (const s of STATES) {
      const connected = !['idle', 'lost'].includes(hubOf(s));
      const want = connected && s !== 'hubRestarting' && s !== 'reconnectNeeded' ? 'lost' : s;
      expect(next(s, 'lost'), s).toBe(want);
    }
  });

  test.each([
    ['idle', 'connect', 'choosing'],
    ['choosing', 'dismissed', 'notChosen'],
    ['choosing', 'openFailed', 'portFailed'],
    ['choosing', 'chosen', 'checking'],
    ['checking', 'noAnswer', 'silentHub'],
    ['checking', 'helloDiscover', 'scanning'],
    ['checking', 'helloConfigured', 'ready'],
    ['silentHub', 'reselect', 'choosing'],
    ['scanning', 'scanned', 'scanned'],
    ['scanned', 'push', 'sending'],
    ['sending', 'pushed', 'ready'],
    ['sending', 'pushFailed', 'sendFailed'],
    ['sendFailed', 'pushAgain', 'sending'],
    ['ready', 'watchAlarm', 'nodesSilent'],
    ['ready', 'chooseFile', 'reading'],
    ['nodesSilent', 'recover', 'recovering'],
    ['verifying', 'pass', 'fileReady'],
    ['fileReady', 'update', 'writing'],
    ['writing', 'hubTurn', 'hubRestarting'],
    ['hubRestarting', 'autoFailed', 'reconnectNeeded'],
    ['stopped', 'recover', 'recovering'],
    ['recovering', 'fixedRest', 'fileReady'],
    ['recovering', 'gaveUp', 'recoverFailed'],
    ['lost', 'reconnect', 'choosing'],
    ['ready', 'cliClaim', 'cliTurn'],
    ['cliTurn', 'cliReturn', 'ready'],
    ['otherTopology', 'otherFile', 'reading'],
  ] as [L, Ev, L][])('%s --%s--> %s', (from, ev, to) => {
    expect(next(from, ev)).toBe(to);
  });

  test('the header 切断 goes back to idle from every connected state it may be used in, and is held while working', () => {
    for (const s of STATES) {
      const connected = !['idle', 'lost'].includes(hubOf(s));
      const want = connected && !HUB_LOCKED.has(s) ? 'idle' : s;
      expect(next(s, 'disconnect'), s).toBe(want);
    }
    // Sending, updating, reconnecting, recovering and the CLI's turn.
    for (const s of ['sending', 'writing', 'hubRestarting', 'reconnectNeeded', 'recovering', 'cliTurn'] as L[]) expect(HUB_LOCKED.has(s), s).toBe(true);
  });

  test('nodesSilent: the answering nodes are updated first and recovery is beside it', () => {
    expect(view('nodesSilent', { hasConfig: true })).toMatchObject({ primary: 'updateResponding', secondary: ['recover'] });
    expect(view('nodesSilent', { hasConfig: true, answering: false })).toMatchObject({ primary: 'recover', secondary: [] });
  });

  test('an event that does not apply keeps the state', () => {
    expect(next('ready', 'pushed')).toBe('ready');
  });

  test('運用 never sends or replaces the 構成 from otherTopology: its side action leaves for 定義', () => {
    expect(view('otherTopology', { hasConfig: true }).secondary).toEqual(['defineFix']);
    expect(Object.values(MACHINE.otherTopology.on)).not.toContain('sending');
  });

  test('empty canvas: find automatically; with a configuration: connect', () => {
    expect(view('idle', { hasConfig: false })).toEqual({ step: 2, primary: 'connectDiscover', secondary: [] });
    expect(view('idle', { hasConfig: true })).toEqual({ step: 1, primary: 'connect', secondary: [] });
  });

  test('a page whose dagon-wasm did not load offers reloading', () => {
    expect(view('portFailed', { hasConfig: false, failure: 'unavailable' }).primary).toBe('reload');
    expect(view('portFailed', { hasConfig: false, failure: 'busy' }).primary).toBe('connect');
  });

  test('header steps follow the flow', () => {
    const step = (s: L) => view(s, { hasConfig: true }).step;
    expect(['checking', 'scanned', 'ready', 'writing', 'done', 'recovering'].map((s) => step(s as L))).toEqual([1, 3, 4, 5, 6, 'recover']);
  });
});
