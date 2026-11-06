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

/** An editor as the page has it: what it holds, taking what a late read brings under its own. */
function editor() {
  const e = {
    names: {} as Record<string, string>,
    positions: {} as Record<string, { x: number; y: number }>,
    adopt: (l: Layout) => {
      e.names = { ...l.names, ...e.names };
      e.positions = { ...l.positions, ...e.positions };
    },
  };
  return e;
}

describe('the layout file is never written over unread', () => {
  for (const failure of ['refused', 'unreachable'] as const) {
    test(`${failure}: neither a drag nor a name writes it`, async () => {
      const { p, puts } = port(() => failure);
      const e = editor();
      const f = new LayoutFile(p, e.adopt);
      expect(await f.open()).toBe(false);
      e.positions = { hub: { x: 9, y: 9 } };
      expect(await f.change(() => ({ positions: e.positions }))).toBe(false);
      e.names = { hub: 'x' };
      expect(await f.change(() => ({ names: e.names }))).toBe(false);
      expect(puts).toEqual([]);
      expect(f.unread).toBe(true);
    });
  }

  test('recovered through a drag, a later rename keeps the names it did not touch', async () => {
    let answer: Layout | 'unreachable' = 'unreachable';
    const { p, puts } = port(() => answer);
    const e = editor();
    const f = new LayoutFile(p, e.adopt);
    await f.open();
    answer = SAVED;
    e.positions = { hub: { x: 9, y: 9 } };
    expect(await f.change(() => ({ positions: e.positions }))).toBe(true);
    expect(puts[0]).toEqual({ format: 1, names: SAVED.names, positions: { seg: { x: 1, y: 2 }, hub: { x: 9, y: 9 } } });
    expect(e.names).toEqual(SAVED.names);
    e.names = { ...e.names, seg: 'A' };
    await f.change(() => ({ names: e.names }));
    expect(puts[1].names).toEqual({ seg: 'A', hub: '親機' });
    expect(puts[1].positions).toEqual({ seg: { x: 1, y: 2 }, hub: { x: 9, y: 9 } });
  });

  test('a drag and a rename close together: the rename is asked for after the read brought the names', async () => {
    let answer: Layout | 'unreachable' = 'unreachable';
    const { p, puts } = port(() => answer);
    const e = editor();
    const f = new LayoutFile(p, e.adopt);
    await f.open();
    answer = SAVED;
    const drag = f.change(() => ({ positions: e.positions }));
    const rename = f.change(() => ({ names: { ...e.names, seg: 'A' } }));
    await Promise.all([drag, rename]);
    expect(puts[1].names).toEqual({ seg: 'A', hub: '親機' });
  });

  test('a file that was read is written whole, as the page has it', async () => {
    const { p, puts } = port(() => SAVED);
    const f = new LayoutFile(p, editor().adopt);
    expect(await f.open()).toBe(true);
    await f.change(() => ({ positions: {} }));
    await f.change(() => ({ names: { seg: 'A' } }));
    expect(puts[1]).toEqual({ format: 1, names: { seg: 'A' }, positions: {} });
  });

  test('no file yet, or no layout feature, is no failure', async () => {
    const { p, puts } = port(() => null);
    const f = new LayoutFile(p, editor().adopt);
    expect(await f.open()).toBe(true);
    await f.change(() => ({ names: { seg: 'A' } }));
    expect(puts).toEqual([{ format: 1, names: { seg: 'A' } }]);
  });
});
