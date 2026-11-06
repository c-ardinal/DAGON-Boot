import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { SAMPLE_CONFIG } from '../app/sample.ts';
import Graph from './Graph.svelte';

const html = (props: Record<string, unknown> = {}) =>
  render(Graph, {
    props: { config: SAMPLE_CONFIG, live: {}, connected: false, sending: false, selected: null, onselect: () => undefined, ...props },
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
