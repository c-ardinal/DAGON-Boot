import { describe, expect, test } from 'vitest';
import { writePercent, type WriteTrack } from './operate.svelte.ts';

// One % per node over its whole write: the transfer by bytes, then the node's
// trial (its MCU definition's [watchdog] trial_ms) by time, the two parts
// weighted by how long each takes.
describe('one % per node from transfer to confirmation', () => {
  const start = { t0: 1000, xferPct: 0, xferMs: null, rebootAt: null, done: false, floor: 0 };

  test('nothing sent yet is 0', () => {
    expect(writePercent(start, 1500, 4000)).toBe(0);
  });

  test('during the transfer: elapsed over the projected transfer plus the trial', () => {
    // 50 % of the bytes in 1 s: the transfer takes 2 s, then 4 s of trial.
    expect(writePercent({ ...start, xferPct: 50 }, 2000, 4000)).toBeCloseTo((100 * 1000) / 6000, 5);
  });

  test('during the trial: the transfer time plus the time on trial', () => {
    const k = { ...start, xferPct: 100, xferMs: 2000, rebootAt: 5000 };
    expect(writePercent(k, 7000, 4000)).toBeCloseTo((100 * 4000) / 6000, 5);
    // The deadline passing does not reach 100: only the confirmation does.
    expect(writePercent(k, 60_000, 4000)).toBe(99);
  });

  test('confirmed is 100', () => {
    expect(writePercent({ ...start, done: true }, 0, 4000)).toBe(100);
  });

  test('without a trial deadline only the bytes count, and never 100 before confirmation', () => {
    expect(writePercent({ ...start, xferPct: 40 }, 2000, null)).toBe(40);
    expect(writePercent({ ...start, xferPct: 100, xferMs: 2000, rebootAt: 5000 }, 7000, null)).toBe(99);
  });

  test('a second image (boot core, then app) never moves the % back', () => {
    expect(writePercent({ ...start, floor: 60, xferPct: 10 }, 1100, 4000)).toBe(60);
  });

  test('read on the page tick, between events, the % never moves back and stays under 100 until confirmation', () => {
    const trial = 4000;
    const TICK = 250;
    let k: WriteTrack = { ...start };
    let now = 1000;
    let lastTick = now;
    const shown: number[] = [];
    // Every event redraws with the clock of the last tick; every tick redraws with the new clock.
    const step = (ms: number, event?: (at: number) => WriteTrack) => {
      for (let t = 0; t < ms; t += 50) {
        now += 50;
        if (now - lastTick >= TICK) {
          lastTick = now;
          shown.push(writePercent(k, lastTick, trial));
        }
      }
      if (event) k = event(now);
      shown.push(writePercent(k, lastTick, trial));
    };
    for (const image of [0, 1]) {
      k = { ...k, floor: image ? writePercent(k, now, trial) : 0, t0: now, xferPct: 0, xferMs: null, rebootAt: null };
      const t0 = now;
      for (let pct = 5; pct <= 100; pct += 5) step(90, () => ({ ...k, xferPct: pct }));
      step(50, (at) => ({ ...k, xferMs: at - t0 }));
      step(200, (at) => ({ ...k, rebootAt: at }));
      step(6000);
    }
    expect(Math.max(...shown)).toBeLessThan(100);
    shown.forEach((p, i) => expect(p, `sample ${i}`).toBeGreaterThanOrEqual(shown[i - 1] ?? 0));
    expect(writePercent({ ...k, done: true }, lastTick, trial)).toBe(100);
  });
});
