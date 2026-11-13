import { describe, expect, test } from 'vitest';
import type { SlotInfo, StatusResp } from '../wasm/dagon.ts';
import { appRows, coreRows, imageRows, previousVersion } from './slots.ts';

const V100 = 0x0100_0000;
const V110 = 0x0101_0000;
const AB = [0, 1];
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
    expect(previousVersion(status(1, slot(4, V100), slot(4, V110)), AB)).toBe('1.0.0');
    expect(previousVersion(status(1, slot(1, V100), slot(4, V110)), AB)).toBe('1.0.0');
  });

  test('not a newer written image, an empty or rolled-back slot, or nothing running', () => {
    expect(previousVersion(status(0, slot(4, V100), slot(1, V110)), AB)).toBeNull();
    expect(previousVersion(status(0, slot(4, V100), slot(0, 0)), AB)).toBeNull();
    expect(previousVersion(status(1, slot(5, V100), slot(4, V110)), AB)).toBeNull();
    expect(previousVersion(status(0xff, slot(4, V100), slot(4, V110)), AB)).toBeNull();
    expect(previousVersion(null, AB)).toBeNull();
  });

  test('a node with one slot has no previous version', () => {
    expect(previousVersion(status(0, slot(4, V110), slot(4, V100)), [0])).toBeNull();
  });
});

describe('the slots a node shows are the ones its MCU definition gives', () => {
  test('app slots, the factory slot with them, and boot loader slots', () => {
    expect(imageRows('stm32f401re')).toEqual(AB);
    expect(appRows('stm32f401re')).toEqual(AB);
    expect(coreRows('stm32f401re')).toEqual(AB);
    expect(imageRows('esp32s3')).toEqual(AB);
    expect(appRows('esp32s3')).toEqual([0, 1, 2]);
    expect(coreRows('esp32s3')).toEqual([]);
  });

  test('an MCU the definitions do not name shows every entry', () => {
    expect(appRows(undefined)).toEqual([0, 1, 2]);
    expect(imageRows('no-such-mcu')).toEqual(AB);
    expect(coreRows(undefined)).toEqual(AB);
  });
});
