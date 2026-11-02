import { describe, expect, test } from 'vitest';
import type { SlotInfo, StatusResp } from '../wasm/dagon.ts';
import { previousVersion } from './slots.ts';

const V100 = 0x0100_0000;
const V110 = 0x0101_0000;
const slot = (state: number, version: number): SlotInfo => ({ state, version, digest16: new Uint8Array(16) });
const status = (active: number, a: SlotInfo, b: SlotInfo): StatusResp => ({
  slots: [a, b, slot(0, 0)],
  active,
  confirmed: active,
  attempts: 0,
  lastReason: 0,
  logGeneration: 1,
});

describe('previous version offered by "前のバージョンに戻す"', () => {
  test('the other slot, confirmed or written before the running version', () => {
    expect(previousVersion(status(1, slot(4, V100), slot(4, V110)))).toBe('1.0.0');
    expect(previousVersion(status(1, slot(1, V100), slot(4, V110)))).toBe('1.0.0');
  });

  test('not a newer written image, an empty or rolled-back slot, or nothing running', () => {
    expect(previousVersion(status(0, slot(4, V100), slot(1, V110)))).toBeNull();
    expect(previousVersion(status(0, slot(4, V100), slot(0, 0)))).toBeNull();
    expect(previousVersion(status(1, slot(5, V100), slot(4, V110)))).toBeNull();
    expect(previousVersion(status(0xff, slot(4, V100), slot(4, V110)))).toBeNull();
    expect(previousVersion(null)).toBeNull();
  });
});
