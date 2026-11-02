import { describe, expect, test } from 'vitest';
import { lengthTable, neighbour, pointAt, tAtLength, tangentAt, tieLengths, verticalLink, wireOffsets } from './geometry.ts';

describe('link geometry', () => {
  const b = verticalLink({ x: 100, y: 0 }, { x: 300, y: 400 });

  test('leaves and enters vertically', () => {
    expect(tangentAt(b, 0)).toEqual({ x: 0, y: 1 });
    expect(tangentAt(b, 1)).toEqual({ x: 0, y: 1 });
    expect(pointAt(b, 0.5)).toEqual({ x: 200, y: 200 });
  });

  test('arc length round trip', () => {
    const table = lengthTable(b);
    const total = table[table.length - 1];
    expect(total).toBeGreaterThan(400);
    const p = pointAt(b, tAtLength(table, 32));
    expect(Math.hypot(p.x - 100, p.y)).toBeCloseTo(32, 0);
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
