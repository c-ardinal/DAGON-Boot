import { describe, expect, test } from 'vitest';
import { STATE } from './copy.ts';
import { lightsStill, nodeState, STATE_ICON } from './nodeState.ts';

describe('node state', () => {
  test.each([
    [{ connected: false }, 'idle'],
    [{ connected: true, responding: null }, 'idle'],
    [{ connected: true, responding: true, unreachable: true }, 'idle'],
    [{ connected: true, responding: true }, 'alive'],
    [{ connected: true, responding: false }, 'silent'],
    [{ connected: true, responding: true, writing: true }, 'busy'],
    [{ connected: true, responding: true, trial: true }, 'attention'],
    [{ connected: true, chip: '↺ 前のバージョン' }, 'attention'],
    [{ connected: true, chip: '× 配布元不明', writing: true }, 'error'],
    [{ connected: true, responding: true, chip: '! ビルド失敗' }, 'error'],
  ] as const)('%j is %s', (input, state) => {
    expect(nodeState(input)).toBe(state);
  });

  test('every state has its own shape and its word', () => {
    const shapes = Object.values(STATE_ICON);
    expect(new Set(shapes).size).toBe(shapes.length);
    expect(Object.keys(STATE_ICON).sort()).toEqual(Object.keys(STATE).sort());
  });

  test('an error or attention stops the other busy lights', () => {
    expect(lightsStill(['busy', 'alive'])).toBe(false);
    expect(lightsStill(['busy', 'attention'])).toBe(true);
    expect(lightsStill(['busy', 'error'])).toBe(true);
  });
});
