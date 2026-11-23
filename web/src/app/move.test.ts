import { describe, expect, test } from 'vitest';
import { STEP_FAR_PX, STEP_PX, stepOf } from './move.ts';

const key = (key: string, mods: Partial<Record<'altKey' | 'shiftKey' | 'ctrlKey' | 'metaKey', boolean>> = {}) => ({ key, altKey: false, shiftKey: false, ctrlKey: false, metaKey: false, ...mods });

describe('Alt + arrow steps', () => {
  test('each arrow with Alt steps that way; Shift makes the step four times as long', () => {
    expect(stepOf(key('ArrowUp', { altKey: true }))).toEqual({ x: 0, y: -STEP_PX });
    expect(stepOf(key('ArrowDown', { altKey: true }))).toEqual({ x: 0, y: STEP_PX });
    expect(stepOf(key('ArrowLeft', { altKey: true }))).toEqual({ x: -STEP_PX, y: 0 });
    expect(stepOf(key('ArrowRight', { altKey: true, shiftKey: true }))).toEqual({ x: STEP_FAR_PX, y: 0 });
    expect(STEP_FAR_PX).toBe(4 * STEP_PX);
  });
  test('a plain arrow (the focus move), Ctrl or Meta with it, and other keys step nothing', () => {
    expect(stepOf(key('ArrowUp'))).toBeNull();
    expect(stepOf(key('ArrowUp', { altKey: true, ctrlKey: true }))).toBeNull();
    expect(stepOf(key('ArrowUp', { altKey: true, metaKey: true }))).toBeNull();
    expect(stepOf(key('a', { altKey: true }))).toBeNull();
  });
});
