import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { SAMPLE_CONFIG } from '../app/sample.ts';
import { STATE } from '../app/copy.ts';
import Graph from './Graph.svelte';

const html = (props: Record<string, unknown> = {}) =>
  render(Graph, {
    props: { config: SAMPLE_CONFIG, live: {}, connected: false, sending: false, selected: null, onselect: () => undefined, dot: null, ...props },
  }).body;
const checks = (b: string) => [...b.matchAll(/<button[^>]*role="checkbox"[^>]*>/g)].map((m) => m[0]);

describe('the node being built (準備)', () => {
  test('its card says 作業中; the others stay as they were', () => {
    const b = html({ busy: 'hub' });
    const labels = [...b.matchAll(/<button class="node[^>]*aria-label="([^"]+)"/g)].map((m) => m[1]);
    expect(labels.filter((l) => l.endsWith(STATE.busy))).toHaveLength(1);
    expect(labels.filter((l) => l.endsWith(STATE.idle)).length).toBe(labels.length - 1);
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
