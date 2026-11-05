import { describe, expect, test } from 'vitest';
import { keptChoices } from './kept.ts';

describe('kept versions in the version picker', () => {
  test('a package giving every node one version is named by it; mixed ones by their id', () => {
    const list = [
      { id: 'v1.2.0-0a1b2c3d', versions: { hub: '1.2.0', seg: '1.0.0' }, bytes: 9 },
      { id: 'v1.1.0', versions: { hub: '1.1.0', seg: '1.1.0' }, bytes: 9 },
    ];
    expect(keptChoices(list)).toEqual([
      { id: 'v1.2.0-0a1b2c3d', label: 'v1.2.0-0a1b2c3d' },
      { id: 'v1.1.0', label: '1.1.0' },
    ]);
    expect(keptChoices([])).toEqual([]);
  });
});
