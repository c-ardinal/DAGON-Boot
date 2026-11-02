import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { HUB_TOGGLE, STEP } from '../app/copy.ts';
import type { HubState, Step } from '../app/machine.ts';
import Header from './Header.svelte';

const base = { drawer: false, drawerOpen: false, ondrawer: () => undefined, onhub: () => undefined };
const head = (p: { step: Step | null; hub: HubState; hubLocked?: string | null; done?: boolean; mode?: 'operate' | 'define' }) =>
  render(Header, { props: { ...base, ...p } }).body;
/** The toggle button's markup. */
const toggle = (body: string) => /<button class="secondary toggle[^>]*>[^<]*<\/button>/.exec(body)?.[0] ?? '';

describe('header 親機 toggle', () => {
  test.each([
    ['idle', HUB_TOGGLE.connect],
    ['lost', HUB_TOGGLE.connect],
    ['connected', HUB_TOGGLE.disconnect],
    ['checking', HUB_TOGGLE.disconnect],
  ] as const)('%s: offers %s', (hub, label) => {
    const b = toggle(head({ step: 4, hub }));
    expect(b).toContain(label);
    expect(b).toContain('aria-disabled="false"');
  });

  test.each(['L19', 'L20', 'L24', 'L28'] as const)('%s: held (focusable, aria-disabled) with its reason', (s) => {
    const why = HUB_TOGGLE.locked[s]!;
    const body = head({ step: 5, hub: s === 'L28' ? 'cli' : s === 'L20' ? 'reconnecting' : 'connected', hubLocked: why });
    expect(toggle(body)).toContain('aria-disabled="true"');
    expect(toggle(body)).toContain('aria-describedby="hub-why"');
    expect(body).toContain(why);
  });
});

describe('the three steps always show in 運用', () => {
  /** Each step's own classes (scoping class left out) and its aria-current. */
  const items = (body: string) =>
    [...body.matchAll(/<li\b([^>]*)>/g)].map(([, attrs]) => [
      (/class="([^"]*)"/.exec(attrs)?.[1] ?? '').replace(/svelte-\w+/g, '').trim(),
      attrs.includes('aria-current="step"') ? 'aria-current="step"' : '',
    ]);

  test.each([
    [1, ['now', 'aria-current="step"'], 0],
    [4, ['now', 'aria-current="step"'], 1],
    [5, ['now', 'aria-current="step"'], 2],
  ] as const)('step %s highlights one of つなぐ・ファイル・更新', (step, current, at) => {
    const body = head({ step, hub: 'connected' });
    for (const n of Object.values([STEP.names[1], STEP.names[2], STEP.names[3]])) expect(body).toContain(n);
    const li = items(body);
    expect(li).toHaveLength(3);
    expect(li[at]).toEqual(current);
    expect(li.filter(([, c]) => c).length).toBe(1);
  });

  test('done: all three are marked done, none current', () => {
    const li = items(head({ step: 6, hub: 'connected', done: true }));
    expect(li.map(([c]) => c)).toEqual(['past', 'past', 'past']);
  });

  test('recovery shows in the third place', () => {
    const body = head({ step: 'recover', hub: 'connected' });
    expect(body).toContain(STEP.names.recover);
    expect(items(body)[2][1]).toBe('aria-current="step"');
  });

  test('定義 has no steps (owner 11/3)', () => {
    expect(items(head({ step: 4, hub: 'connected', mode: 'define' }))).toEqual([]);
  });
});
