import { describe, expect, test } from 'vitest';
import { LEAVING, MACHINE, hubOf, next, view, type Ctx, type Ev, type L } from './machine.ts';

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
    const seen = new Set<L>(['L01', 'L02']);
    const todo: L[] = ['L02'];
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

  test('a lost device ends in L26 from every connected state except the hub restart (L20, L21)', () => {
    for (const s of STATES) {
      const connected = !['idle', 'lost'].includes(hubOf(s));
      const want = connected && s !== 'L20' && s !== 'L21' ? 'L26' : s;
      expect(next(s, 'lost'), s).toBe(want);
    }
  });

  test.each([
    ['L02', 'connect', 'L03'],
    ['L03', 'dismissed', 'L04'],
    ['L03', 'openFailed', 'L05'],
    ['L03', 'chosen', 'L06'],
    ['L06', 'noAnswer', 'L07'],
    ['L06', 'helloDiscover', 'L08'],
    ['L06', 'helloConfigured', 'L12'],
    ['L07', 'reselect', 'L03'],
    ['L08', 'scanned', 'L09'],
    ['L09', 'push', 'L10'],
    ['L10', 'pushed', 'L12'],
    ['L10', 'pushFailed', 'L11'],
    ['L11', 'pushAgain', 'L10'],
    ['L12', 'watchAlarm', 'L13'],
    ['L12', 'chooseFile', 'L14'],
    ['L13', 'recover', 'L24'],
    ['L15', 'pass', 'L18'],
    ['L18', 'update', 'L19'],
    ['L19', 'hubTurn', 'L20'],
    ['L20', 'autoFailed', 'L21'],
    ['L22', 'recover', 'L24'],
    ['L24', 'fixedRest', 'L18'],
    ['L24', 'gaveUp', 'L25'],
    ['L26', 'reconnect', 'L03'],
    ['L12', 'cliClaim', 'L28'],
    ['L28', 'cliReturn', 'L12'],
  ] as [L, Ev, L][])('%s --%s--> %s', (from, ev, to) => {
    expect(next(from, ev)).toBe(to);
  });

  test('an event that does not apply keeps the state', () => {
    expect(next('L12', 'pushed')).toBe('L12');
  });

  test('empty canvas: find automatically, or place the sample; with a configuration: connect', () => {
    expect(view('L02', { hasConfig: false })).toEqual({ step: 2, primary: 'connectDiscover', secondary: ['placeSample'] });
    expect(view('L02', { hasConfig: true })).toEqual({ step: 1, primary: 'connect', secondary: [] });
  });

  test('a page whose dagon-wasm did not load offers reloading', () => {
    expect(view('L05', { hasConfig: false, failure: 'unavailable' }).primary).toBe('reload');
    expect(view('L05', { hasConfig: false, failure: 'busy' }).primary).toBe('connect');
  });

  test('header steps follow the flow', () => {
    const step = (s: L) => view(s, { hasConfig: true }).step;
    expect(['L06', 'L09', 'L12', 'L19', 'L23', 'L24'].map((s) => step(s as L))).toEqual([1, 3, 4, 5, 6, 'recover']);
  });
});
