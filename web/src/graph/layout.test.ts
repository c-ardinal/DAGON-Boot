import { describe, expect, test } from 'vitest';
import { treeLayout } from './layout.ts';

const o = { w: 100, h: () => 50, gapX: 20, gapY: 30 };
const hub = (id: string) => ({ id, hub: true });
const node = (id: string) => ({ id, hub: false });

describe('treeLayout', () => {
  test('one 親機 sits centred over its 子機', () => {
    const l = treeLayout([hub('h'), node('a'), node('b')], [{ from: 'h', to: 'a' }, { from: 'h', to: 'b' }], o);
    expect(l.at.a).toEqual({ x: 0, y: 80, depth: 1 });
    expect(l.at.b).toEqual({ x: 120, y: 80, depth: 1 });
    expect(l.at.h.x).toBe(60);
    expect(l.width).toBe(220);
    expect(l.roots).toEqual(['h']);
  });

  test('several 親機 each get a tree, side by side, at any depth', () => {
    const l = treeLayout(
      [hub('h1'), node('a'), hub('h2'), node('p'), node('s')],
      [{ from: 'h1', to: 'a' }, { from: 'h2', to: 'p' }, { from: 'p', to: 's' }],
      o,
    );
    expect(l.roots).toEqual(['h1', 'h2']);
    expect(l.at.s.depth).toBe(2);
    expect(l.at.s.y).toBe(160);
    // The second tree starts two gaps after the first one.
    expect(l.at.h2.x).toBe(140);
    expect(l.at.s.x).toBe(l.at.h2.x);
  });

  test('a 親機 linked from another node sits under it, not as a root', () => {
    const l = treeLayout([hub('h1'), hub('h2'), node('m')], [{ from: 'h1', to: 'h2' }, { from: 'h2', to: 'm' }], o);
    expect(l.roots).toEqual(['h1']);
    expect(l.at.h2.depth).toBe(1);
    expect(l.at.m.depth).toBe(2);
  });

  test('a 子機 nobody links to stands in the first row; a cycle does not loop', () => {
    const l = treeLayout([hub('h'), node('x'), node('c1'), node('c2')], [{ from: 'c1', to: 'c2' }, { from: 'c2', to: 'c1' }], o);
    expect(l.at.x.depth).toBe(1);
    expect(Object.keys(l.at).sort()).toEqual(['c1', 'c2', 'h', 'x']);
  });

  test('no nodes: nothing placed', () => {
    expect(treeLayout([], [], o)).toEqual({ at: {}, width: 0, height: 0, roots: [] });
  });
});
