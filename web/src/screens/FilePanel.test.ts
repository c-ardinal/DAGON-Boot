import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { FILE, PLAN, WRITE, fill } from '../app/copy.ts';
import FilePanel from './FilePanel.svelte';

const file = { name: 'desk-1.1.0.dgb', size: 4096, bytes: new Uint8Array(), verdict: 'pass' as const };
const nodes = [
  { id: 'hub', name: '[1] メイン', from: '1.0.0', to: '1.1.0', checked: true, stage: '' },
  { id: 'seg', name: '[2] 7セグ', from: '1.0.0', to: '1.1.0', checked: false, stage: '' },
  { id: 'watch', name: '[3] ウォッチ', from: '1.0.0', to: '1.1.0', checked: true, stage: '' },
];
const html = (props: Record<string, unknown> = {}) =>
  render(FilePanel, {
    props: { file, nodes, onchoose: () => undefined, go: { label: fill(WRITE.go, { n: 2 }), off: false, onclick: () => undefined }, ...props },
  }).body;
const rows = (b: string) => [...b.matchAll(/<tr[^>]*data-node="([^"]+)"[^>]*>([\s\S]*?)<\/tr>/g)].map((m) => ({ id: m[1], html: m[2] }));

describe('write sheet: what it holds', () => {
  test('no pre-write check rows (配布元・構成・対象・対象外); 目安 stays', () => {
    const b = html({ minutes: 3 });
    for (const w of [FILE.source, FILE.topology, FILE.targets, FILE.sourceOk, FILE.topologyOk, '対象外']) expect(b, w).not.toContain(`>${w}<`);
    expect(b).toContain(`>${PLAN.estimateLabel}<`);
    expect(b).toContain(PLAN.minutes(3));
  });
});

describe('write sheet: node choice', () => {
  test('the per-node table has the pen columns ノード・今・新・進み in that order', () => {
    const b = html();
    const at = [FILE.nodes.node, FILE.nodes.now, FILE.nodes.next, FILE.nodes.progress].map((w) => b.indexOf(`>${w}<`));
    expect(at.every((i) => i > 0)).toBe(true);
    expect([...at].sort((x, y) => x - y)).toEqual(at);
  });

  test('every node is listed with a check that follows the choice, and its 今 → 新 versions', () => {
    const r = rows(html());
    expect(r.map((x) => x.id)).toEqual(['hub', 'seg', 'watch']);
    for (const [i, on] of [true, false, true].entries()) {
      expect(r[i].html).toMatch(/<input[^>]*type="checkbox"/);
      expect(/<input[^>]*checked/.test(r[i].html), r[i].id).toBe(on);
      expect(r[i].html).toContain('>1.0.0<');
      expect(r[i].html).toContain('>1.1.0<');
    }
    expect(r[0].html).toContain(`aria-label="[1] メイン"`);
  });

  test('the primary names how many nodes are checked', () => {
    expect(html()).toContain(`>${fill(WRITE.go, { n: 2 })}<`);
  });

  test('while writing the checks are locked', () => {
    const r = rows(html({ locked: true }));
    expect(r).toHaveLength(3);
    expect(r.every((x) => /<input[^>]*disabled/.test(x.html))).toBe(true);
  });
});

describe('write sheet: the kept versions', () => {
  const versions = [
    { id: 'v1.1.0', label: '1.1.0' },
    { id: 'v1.0.0', label: '1.0.0' },
  ];
  test('the header picks among the kept versions, the one on the sheet selected', () => {
    const b = html({ versions, current: 'v1.0.0' });
    const sel = /<select[^>]*aria-label="([^"]+)"[^>]*>([\s\S]*?)<\/select>/.exec(b);
    expect(sel?.[1]).toBe(FILE.version);
    const opts = [...(sel?.[2] ?? '').matchAll(/<option[^>]*value="([^"]+)"[^>]*>([^<]*)<\/option>/g)].map((m) => [m[1], m[2]]);
    expect(opts).toEqual([
      ['v1.1.0', '1.1.0'],
      ['v1.0.0', '1.0.0'],
    ]);
    expect(/<option[^>]*value="v1.0.0"[^>]*selected/.test(sel?.[2] ?? '')).toBe(true);
  });

  test('without kept versions the header names the file', () => {
    const b = html();
    expect(b).not.toContain('<select');
    expect(b).toContain('desk-1.1.0.dgb');
  });
});
