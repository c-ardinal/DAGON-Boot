import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { render } from 'svelte/server';
import { describe, expect, test } from 'vitest';
import { BUILD, DEFINE, WRITE, ZOOM } from '../app/copy.ts';
import SplitButton from './SplitButton.svelte';
import StopNotice from './StopNotice.svelte';
import ZoomPill from './ZoomPill.svelte';

const SRC = join(__dirname, '..');
const sources = (dir: string): string[] =>
  readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return f === 'pkg' ? [] : sources(p);
    return /\.(ts|svelte)$/.test(f) && f !== 'console.test.ts' ? [p] : [];
  });
const withWord = (w: RegExp) => sources(SRC).filter((p) => w.test(readFileSync(p, 'utf8')));

describe('words that are gone', () => {
  test('自作 is nowhere, tests included: an MCU entry is a ボード', () => {
    expect(withWord(/自作/)).toEqual([]);
    expect(DEFINE.ownBoard).toBe('ボード');
  });

  test('the write order list is gone, its words and its component', () => {
    expect(withWord(/書き込みの順(?!番)|OrderList/)).toEqual([]);
  });

  test('no screen fixes a layout by reading the window width', () => {
    expect(sources(SRC).filter((p) => /\.svelte$/.test(p) && /matchMedia\('\(m(ax|in)-width/.test(readFileSync(p, 'utf8')))).toEqual([]);
  });
});

describe('split button', () => {
  const html = (p = {}) => render(SplitButton, { props: { label: WRITE.run, onmain: () => undefined, pickName: WRITE.pickVersion, options: [{ id: 'latest', label: '1.1.0' }], onpick: () => undefined, ...p } }).body;

  test('the button and its dropdown part are one control, in that order, with no separate select', () => {
    const b = html();
    expect(b).not.toContain('<select');
    expect(b.indexOf(WRITE.run)).toBeLessThan(b.indexOf(`aria-label="${WRITE.pickVersion}"`));
    expect(b.match(/class="split/g)).toHaveLength(1);
  });

  test('nothing to choose: the dropdown part stays and says it is off', () => {
    expect(html({ options: [] })).toMatch(new RegExp(`aria-label="${WRITE.pickVersion}"[^>]*aria-disabled="true"`));
  });
});

describe('a write that went back', () => {
  const html = (p = {}) =>
    render(StopNotice, {
      props: { word: WRITE.previous, target: '[3] watch', card: () => null, reason: 'r', fix: 'f', onclose: () => undefined, onmore: () => undefined, linked: false, onask: () => undefined, ...p },
    }).body;

  test('is told with no dialog and no scrim: it is an alert beside the graph', () => {
    const b = html();
    expect(b).toContain('role="alert"');
    expect(b).not.toContain('<dialog');
  });

  test('offers asking why only with an agent linked', () => {
    expect(html()).not.toContain(BUILD.askAgent);
    expect(html({ linked: true })).toContain(BUILD.askAgent);
  });
});

describe('zoom pill', () => {
  test('+ on top, the readout between + and −, then the fit', () => {
    const b = render(ZoomPill, { props: { scale: 1.5, canIn: true, canOut: true, onin: () => undefined, onout: () => undefined, onfit: () => undefined } }).body;
    const order = [`aria-label="${ZOOM.in}"`, '150%', `aria-label="${ZOOM.out}"`, `aria-label="${ZOOM.fit}"`].map((s) => b.indexOf(s));
    expect(order.every((x) => x >= 0)).toBe(true);
    expect(order).toEqual([...order].sort((a, c) => a - c));
  });
});
