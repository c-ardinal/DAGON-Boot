import { describe, expect, test } from 'vitest';
import { NODE, STATE, WRITE } from './copy.ts';
import { lightsStill, nodeState, resultChip, STATE_ICON } from './nodeState.ts';

describe('the chip a node keeps after a write', () => {
  test('a node the stopped write never reached: not written, no error chip, its state stays what the watch saw', () => {
    const c = resultChip('unwritten')!;
    expect(c).toEqual({ chip: NODE.unwritten, spoken: '未書き込み', silent: false });
    expect(nodeState({ connected: true, responding: true, chip: c.chip })).toBe('alive');
  });

  test.each(['refused', 'silent', 'cannotFix'] as const)('a node that failed (%s) is an error', (stage) => {
    const c = resultChip(stage)!;
    expect(c).toMatchObject({ chip: WRITE.failed, silent: true });
    expect(nodeState({ connected: true, responding: true, chip: c.chip })).toBe('error');
  });

  test('a node that went back asks for attention, not an error', () => {
    const c = resultChip('previous')!;
    expect(c).toMatchObject({ chip: WRITE.previous, silent: false });
    expect(nodeState({ connected: true, responding: true, chip: c.chip })).toBe('attention');
  });

  test('confirmed or still running: no result chip', () => {
    for (const stage of ['done', 'waiting', 'writing'] as const) expect(resultChip(stage)).toBeNull();
  });
});

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
