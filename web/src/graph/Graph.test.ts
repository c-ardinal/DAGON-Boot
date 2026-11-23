import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { SAMPLE_CONFIG, type Link, type Node } from '../app/sample.ts';
import { NODE, STATE } from '../app/copy.ts';
import Graph from './Graph.svelte';

const html = (props: Record<string, unknown> = {}) =>
  render(Graph, {
    props: { config: SAMPLE_CONFIG, live: {}, connected: false, selected: null, onselect: () => undefined, ...props },
  }).body;
const checks = (b: string) => [...b.matchAll(/<button[^>]*role="checkbox"[^>]*>/g)].map((m) => m[0]);

describe('building', () => {
  test('a node being built carries the edge light and no words stand in for it', () => {
    const b = html({ building: ['seg'], chips: {} });
    expect(b).toMatch(/class="node st-busy[^"]*"[^>]*data-key="node:seg"/);
    expect(b).not.toMatch(/class="node st-busy[^"]*"[^>]*data-key="node:hub"/);
    expect(b).not.toContain('ビルド中');
  });
});

describe('the role chip on a card', () => {
  const node = (id: string, no: number, role: 'hub' | 'node' = 'node'): Node => ({ id, no, name: id, role, product: 'P', mcu: 'M', via: '', reset: null, boot: null });
  const link = (from: string, to: string): Link => ({ ...SAMPLE_CONFIG.links[1], from, to, wires: [] });
  const chips = (nodes: Node[], links: Link[]) => {
    const b = html({ config: { ...SAMPLE_CONFIG, nodes, links, order: nodes.map((n) => n.id), reach: [] } });
    return Object.fromEntries(nodes.map((n) => [n.id, new RegExp(`<b[^>]*>\\[${n.no}\\] ${n.id}</b> ?<span class="role[^"]*">([^<]*)</span>`).exec(b)?.[1]]));
  };

  test('counts the links from the nearest coordinator above, starting again at each coordinator', () => {
    // c1 → a; c1 → c2 (a second coordinator) → b → d.
    const got = chips([node('c1', 1, 'hub'), node('a', 2), node('c2', 3, 'hub'), node('b', 4), node('d', 5)], [link('c1', 'a'), link('c1', 'c2'), link('c2', 'b'), link('b', 'd')]);
    expect(got).toEqual({ c1: NODE.roleHub, a: NODE.roleNode, c2: NODE.roleHub, b: NODE.roleNode, d: NODE.roleGrandchild });
    expect(NODE.roleDeeper(3)).toBe('コーディネータから3段目');
  });

  test('a node no coordinator is above (nobody links to it, below such a node, or round a loop) is ノード', () => {
    // x → y with nothing above x; p → q → p.
    const got = chips([node('c1', 1, 'hub'), node('x', 2), node('y', 3), node('p', 4), node('q', 5)], [link('x', 'y'), link('p', 'q'), link('q', 'p')]);
    expect(got).toEqual({ c1: NODE.roleHub, x: NODE.roleOther, y: NODE.roleOther, p: NODE.roleOther, q: NODE.roleOther });
  });
});

describe('node checks on the cards (書込)', () => {
  test('each node given a check has one, beside its card, saying whether it is written', () => {
    const b = html({ checks: { hub: true, seg: false }, oncheck: () => undefined });
    const c = checks(b);
    expect(c).toHaveLength(2);
    expect(c.filter((x) => x.includes('aria-checked="true"'))).toHaveLength(1);
    expect(c.filter((x) => x.includes('aria-checked="false"'))).toHaveLength(1);
    // Never inside the card's own button.
    expect(/<button class="node[^>]*>(?:(?!<\/button>)[\s\S])*role="checkbox"/.test(b)).toBe(false);
  });

  test('without checks (準備) no card has one', () => {
    expect(checks(html())).toEqual([]);
  });
});
