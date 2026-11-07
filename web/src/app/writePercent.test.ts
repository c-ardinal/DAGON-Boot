import { describe, expect, test } from 'vitest';
import { writePercent, type WriteTrack } from './operate.svelte.ts';

// The number and its weights are dagon-host's (crates/dagon-host/src/percent.rs, tested there); the page reads the same calculation.
describe('one % per node from transfer to confirmation', () => {
  const start: WriteTrack = { shown: 0, xferMs: null, t0: 1000, rebootAt: null, done: false };

  test('during the transfer: the number dagon-host told the node, whatever the clock says', () => {
    expect(writePercent({ ...start, shown: 16 }, 2000, 4000)).toBe(16);
    expect(writePercent({ ...start, shown: 16 }, 90_000, 4000)).toBe(16);
  });

  test('during the trial: the transfer time plus the time on trial', () => {
    const k = { ...start, shown: 33, xferMs: 2000, rebootAt: 5000 };
    expect(writePercent(k, 7000, 4000)).toBe(66);
    // The deadline passing does not reach 100: only the confirmation does.
    expect(writePercent(k, 60_000, 4000)).toBe(99);
  });

  test('confirmed is 100', () => {
    expect(writePercent({ ...start, done: true }, 0, 4000)).toBe(100);
  });

  test('without a trial deadline: bytes, then 99 until confirmation', () => {
    expect(writePercent({ ...start, shown: 40 }, 2000, null)).toBe(40);
    expect(writePercent({ ...start, shown: 90, xferMs: 2000, rebootAt: 5000 }, 7000, null)).toBe(99);
  });

  test('what was said is never gone under', () => {
    expect(writePercent({ ...start, shown: 60, xferMs: 2000, rebootAt: 5000 }, 5100, 4000)).toBe(60);
  });
});
