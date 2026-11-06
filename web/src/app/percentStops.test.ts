import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import type { LinkClock } from '../serial/hostLink.ts';
import { MockTransport, type MockOptions } from '../serial/mock.ts';
import { Operate } from './operate.svelte.ts';

const repo = (rel: string) => new Uint8Array(readFileSync(new URL(`../../../${rel}`, import.meta.url)));
const RELEASE = repo('crates/dagon-sim/fixtures/sample-1.1.0.dgb');
const TRIAL_MS = 4000;

/** A session whose nodes count their self-check, and the % as the page would read it at every wait of dagon-host. */
function session(opts: MockOptions, trial = TRIAL_MS) {
  const t = new MockTransport(opts);
  const seen: { state: string; now: number; seg: number; overall: number }[] = [];
  let at = 0;
  let op: Operate | null = null;
  const clock: LinkClock = {
    now: () => {
      at += 50;
      if (op) seen.push({ state: op.state, now: at, seg: op.percentOf('seg', at), overall: op.overallAt(at) });
      return at;
    },
    sleep: (ms) => {
      at += ms;
      return new Promise((r) => setTimeout(r, 0));
    },
  };
  op = new Operate({
    open: async () => {
      await t.open();
      return t;
    },
    reselect: async () => {
      await t.open();
      return t;
    },
    clock,
    latest: async () => RELEASE,
    trialMs: () => trial,
  });
  return { op, seen };
}

async function written(opts: MockOptions, trial = TRIAL_MS) {
  const s = session(opts, trial);
  await s.op.connect();
  await s.op.push();
  s.op.takeFile('sample-1.1.0.dgb', RELEASE);
  await s.op.update();
  return s;
}

const rising = (list: number[]) => list.forEach((p, i) => expect(p, `sample ${i}`).toBeGreaterThanOrEqual(list[i - 1] ?? 0));

describe('the % runs only while work is really in progress', () => {
  test('a stopped write freezes: nothing rises with the clock, for the node or overall', async () => {
    const { op } = await written({ refuse: { 'i2c:66': 0x07 } });
    expect([op.state, op.failed]).toEqual(['stopped', 'seg']);
    const now = op.now();
    const later = now + 60_000;
    expect(op.percentOf('seg', later)).toBe(op.percentOf('seg', now));
    expect(op.overallAt(later)).toBe(op.overallAt(now));
    expect(op.percentOf('seg', later)).toBeLessThan(100);
  }, 60_000);

  test('waiting for the user to choose the hub again freezes too', async () => {
    const { op } = await written({ hubAway: Infinity }, 3_600_000);
    expect(op.state).toBe('reconnectNeeded');
    const now = op.now();
    expect(op.percentOf('hub', now + 60_000)).toBe(op.percentOf('hub', now));
    expect(op.overallAt(now + 60_000)).toBe(op.overallAt(now));
  }, 60_000);

  test('a recovery drives the number: the recovered node alone, rising', async () => {
    const { op, seen } = await written({ refuse: { 'i2c:66': 0x07 } });
    expect(op.state).toBe('stopped');
    seen.length = 0;
    (op as unknown as { transport: MockTransport }).transport.setRefuse('i2c:66');
    await op.writeNode({ id: 'seg' });
    expect(op.progress.seg.stage).toBe('done');
    const during = seen.filter((s) => s.state === 'recovering');
    expect(during.length).toBeGreaterThan(0);
    // It rises on the way; 100 comes only with the node's confirmation.
    expect(during.some((s) => s.seg > 0 && s.seg < 100)).toBe(true);
    rising(during.map((s) => s.seg));
    for (const s of during) expect(s.overall).toBe(Math.floor(s.seg));
    expect(op.percentOf('seg')).toBe(100);
  }, 120_000);

  test('a rollback drives the number too, though it transfers nothing', async () => {
    const { op, seen } = await written({});
    expect(op.state).toBe('done');
    seen.length = 0;
    await op.rollback('seg');
    const during = seen.filter((s) => s.state === 'recovering');
    expect(during.length).toBeGreaterThan(0);
    // It rises on the way; 100 comes only with the node's confirmation.
    expect(during.some((s) => s.seg > 0 && s.seg < 100)).toBe(true);
    rising(during.map((s) => s.seg));
    for (const s of during) expect(s.overall).toBe(Math.floor(s.seg));
  }, 120_000);
});
