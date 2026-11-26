import { describe, expect, test } from 'vitest';
import { between, neighbour, pointAt, sideLink, tieLengths, underLink, verticalLink, wireOffsets } from './geometry.ts';

describe('link geometry', () => {
  const b = verticalLink({ x: 100, y: 0 }, { x: 300, y: 400 });

  test('leaves and enters vertically', () => {
    expect(b[1].x).toBe(b[0].x);
    expect(b[2].x).toBe(b[3].x);
    expect(pointAt(b, 0.5)).toEqual({ x: 200, y: 200 });
  });

  test('two cards of one row are joined between their facing sides, level at both ends, whichever is on the left', () => {
    const [l, r] = [{ x: 0, y: 100, w: 200, h: 120 }, { x: 300, y: 100, w: 200, h: 80 }];
    const right = sideLink(l, r);
    expect([right[0], right[3]]).toEqual([{ x: 200, y: 160 }, { x: 300, y: 140 }]);
    expect([right[1].y, right[2].y]).toEqual([right[0].y, right[3].y]);
    expect(right[1].x).toBeGreaterThan(right[0].x);
    const left = sideLink(r, l);
    expect([left[0], left[3]]).toEqual([{ x: 300, y: 140 }, { x: 200, y: 160 }]);
    expect(left[1].x).toBeLessThan(left[0].x);
  });

  test('a card standing between two cards sends their link round below: it leaves and enters from below', () => {
    const [a, mid, c] = [{ x: 0, y: 0, w: 200, h: 100 }, { x: 300, y: 0, w: 200, h: 100 }, { x: 600, y: 0, w: 200, h: 100 }];
    expect(between(a, c, [mid])).toBe(true);
    expect(between(c, a, [mid])).toBe(true);
    expect(between(a, mid, [c])).toBe(false);
    // A card of another row is not between them.
    expect(between(a, c, [{ ...mid, y: 300 }])).toBe(false);
    const u = underLink({ x: 100, y: 100 }, { x: 650, y: 100 });
    expect([u[1].x, u[2].x]).toEqual([100, 650]);
    expect(u[1].y).toBeGreaterThan(100);
    expect(u[2].y).toBe(u[1].y);
    expect(pointAt(u, 0.5).y).toBeGreaterThan(100);
  });

  test('ties sit 32 px from each port, 20 % on short links', () => {
    expect(tieLengths(400)).toEqual([32, 368]);
    expect(tieLengths(150)).toEqual([30, 120]);
  });

  test('wire offsets are centred', () => {
    expect(wireOffsets(5, 4)).toEqual([-8, -4, 0, 4, 8]);
    expect(wireOffsets(3, 8)).toEqual([-8, 0, 8]);
  });

  test('arrow neighbour picks the nearest node in that direction', () => {
    const nodes = [{ x: 400, y: 100 }, { x: 250, y: 400 }, { x: 550, y: 400 }];
    expect(neighbour(nodes[0], nodes, 'down')).toBe(1);
    expect(neighbour(nodes[1], nodes, 'right')).toBe(2);
    expect(neighbour(nodes[1], nodes, 'up')).toBe(0);
    expect(neighbour(nodes[0], nodes, 'up')).toBe(-1);
  });
});
