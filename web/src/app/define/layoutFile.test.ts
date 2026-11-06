import { describe, expect, test } from 'vitest';
import type { Layout } from './cli.svelte.ts';
import { LayoutFile, type LayoutPort } from './layoutFile.ts';

const SAVED: Layout = { format: 1, names: { seg: '7セグ', hub: '親機' }, positions: { seg: { x: 1, y: 2 } } };

/** A serve whose GET answers what `answer` says at that moment. */
function port(answer: () => Layout | null | 'refused' | 'unreachable') {
  const puts: Layout[] = [];
  const p: LayoutPort = {
    layoutRefused: null,
    loadLayout: async () => {
      const a = answer();
      if (a === 'unreachable') throw new Error('no answer');
      if (a === 'refused') {
        p.layoutRefused = 'layout format 2; this dagon reads 1';
        return null;
      }
      return a;
    },
    saveLayout: async (l) => (puts.push(l), true),
  };
  return { p, puts };
}

describe('the layout file is never written over unread', () => {
  for (const failure of ['refused', 'unreachable'] as const) {
    test(`${failure}: neither a drag nor a name writes it`, async () => {
      const { p, puts } = port(() => failure);
      const f = new LayoutFile(p);
      expect(await f.open()).toBe(false);
      expect(await f.change({ positions: { hub: { x: 9, y: 9 } } })).toBe(false);
      expect(await f.change({ names: { hub: 'x' } })).toBe(false);
      expect(puts).toEqual([]);
      expect(f.unread).toBe(true);
    });
  }

  test('once it can be read, what it holds stays under the change', async () => {
    let answer: Layout | 'unreachable' = 'unreachable';
    const { p, puts } = port(() => answer);
    const f = new LayoutFile(p);
    await f.open();
    answer = SAVED;
    expect(await f.change({ positions: { hub: { x: 9, y: 9 } } })).toBe(true);
    expect(puts).toEqual([{ format: 1, names: SAVED.names, positions: { seg: { x: 1, y: 2 }, hub: { x: 9, y: 9 } } }]);
    expect(f.unread).toBe(false);
  });

  test('a file that was read is written whole, as the page has it', async () => {
    const { p, puts } = port(() => SAVED);
    const f = new LayoutFile(p);
    expect(await f.open()).toBe(true);
    await f.change({ positions: {} });
    await f.change({ names: { seg: 'A' } });
    expect(puts[1]).toEqual({ format: 1, names: { seg: 'A' }, positions: {} });
  });

  test('no file yet, or no layout feature, is no failure', async () => {
    const { p, puts } = port(() => null);
    const f = new LayoutFile(p);
    expect(await f.open()).toBe(true);
    await f.change({ names: { seg: 'A' } });
    expect(puts).toEqual([{ format: 1, names: { seg: 'A' } }]);
  });
});
