import { readdirSync, readFileSync } from 'node:fs';
import { afterEach, describe, expect, test, vi } from 'vitest';
import { draw, fade, lateFade, rise, tokenMs, waterEnd } from './motion.ts';

const SRC = new URL('../', import.meta.url);
const read = (rel: string): string => new TextDecoder().decode(readFileSync(new URL(rel, SRC)));
const under = (dir: string): string[] =>
  readdirSync(new URL(dir, SRC), { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? under(`${dir}${e.name}/`) : [`${dir}${e.name}`]));
const sources = under('').filter((f) => /\.(svelte|css)$/.test(f));
/** A source's CSS: a stylesheet whole, a component's <style> blocks; comments out. */
const cssOf = (rel: string) =>
  (rel.endsWith('.css') ? read(rel) : [...read(rel).matchAll(/<style>([\s\S]*?)<\/style>/g)].map((m) => m[1]).join('\n')).replace(/\/\*[\s\S]*?\*\//g, '');
const motionDecls = (css: string) => [...css.matchAll(/(?:^|[;{\s])((?:transition|animation)(?:-[a-z-]+)?)\s*:\s*([^;}]+)/g)].map((m) => ({ prop: m[1], value: m[2] }));

const tokens = read('styles/tokens.css');
const defined = new Set([...tokens.matchAll(/(--(?:dur|ease)-[\w-]+)\s*:/g)].map((m) => m[1]));
/** What tokens.css changes under reduced motion. */
function reducedTokens(): Record<string, string> {
  const at = tokens.indexOf('@media (prefers-reduced-motion: reduce)');
  if (at < 0) return {};
  const end = tokens.indexOf('}', tokens.indexOf('{', tokens.indexOf(':root', at)));
  return Object.fromEntries([...tokens.slice(at, end).matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]));
}
const baseValue = (name: string) => new RegExp(`${name}\\s*:\\s*([^;]+);`).exec(tokens)?.[1].trim() ?? '';

/** Stands in for the browser's reading of the tokens, with or without reduced motion. */
function stubTokens(isReduced: boolean) {
  const reduced = reducedTokens();
  vi.stubGlobal('document', { documentElement: {} });
  vi.stubGlobal('getComputedStyle', () => ({
    getPropertyValue: (name: string) => {
      const v = isReduced && reduced[name] !== undefined ? reduced[name] : baseValue(name);
      return v === 'var(--dur-reduced)' ? baseValue('--dur-reduced') : v;
    },
  }));
}
afterEach(() => vi.unstubAllGlobals());

describe('motion values come only from the tokens', () => {
  test('no transition or animation carries its own duration or curve', () => {
    for (const rel of sources) {
      if (rel === 'styles/tokens.css') continue;
      for (const { prop, value } of motionDecls(cssOf(rel))) {
        const own = value.replace(/var\(--[\w-]+\)/g, '');
        expect(own, `${rel}: ${prop}: ${value}`).not.toMatch(/\d|\b(?:ease|ease-in|ease-out|ease-in-out|linear|cubic-bezier|steps)\b/);
      }
    }
  });

  test('every motion token used is defined in tokens.css, and no curve lives elsewhere', () => {
    for (const rel of sources) {
      const css = cssOf(rel);
      for (const m of css.matchAll(/var\((--(?:dur|ease)-[\w-]+)\)/g)) expect(defined, `${rel}: ${m[1]}`).toContain(m[1]);
      if (rel !== 'styles/tokens.css') expect(css, rel).not.toMatch(/cubic-bezier/);
    }
  });

  test('no component gives a Svelte transition its own time, and the module holds none', () => {
    for (const rel of sources.filter((f) => f.endsWith('.svelte'))) {
      expect(read(rel), rel).not.toMatch(/(?:transition|in|out|animate):\w+=\{\{[^}]*\b(?:duration|delay|easing)\b/);
    }
    expect(read('app/motion.ts')).not.toMatch(/\b\d+\s*ms\b|duration:\s*\d/);
  });

  test('a token reads as milliseconds, whichever unit it is written in', () => {
    stubTokens(false);
    expect(tokenMs('--dur-ui')).toBe(200);
    vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: () => '4.2s' }));
    expect(tokenMs('--dur-any')).toBe(4200);
  });

  test('a transition takes its time and curve from the tokens', () => {
    stubTokens(false);
    const c = rise({} as Element);
    expect(c.duration).toBe(200);
    // --ease-out runs fast at first: well past the middle of the way at half the time.
    expect(c.easing!(0)).toBeCloseTo(0, 5);
    expect(c.easing!(0.5)).toBeGreaterThan(0.8);
    expect(c.easing!(1)).toBeCloseTo(1, 5);
  });
});

describe('continuous motion is for work in progress only', () => {
  test('whatever loops belongs to a busy state, and stops under reduced motion', () => {
    for (const rel of sources) {
      const css = cssOf(rel);
      for (const m of css.matchAll(/([^{}]+)\{[^{}]*\banimation[^;}]*\binfinite\b/g)) {
        expect(m[1].trim(), `${rel}: what loops`).toMatch(/busy|thinking|crest/);
        expect(css, `${rel}: stopped under reduced motion`).toMatch(/prefers-reduced-motion: reduce\)\s*\{[\s\S]*animation: none/);
      }
    }
  });
});

describe('reduced motion turns every transition into an immediate or fade-only change', () => {
  test('in one place: nothing shifts or scales, movement is immediate, fades are short', () => {
    const reduced = reducedTokens();
    expect(reduced['--enter-shift']).toBe('0px');
    expect(reduced['--enter-scale']).toBe('1');
    expect(reduced['--move']).toBe('0');
    expect(reduced['--dur-line']).toBe('0ms');
    expect(reduced['--dur-rise']).toBe('0ms');
    for (const d of ['--dur-ui', '--dur-quick', '--dur-settle']) expect(reduced[d], d).toBe('var(--dur-reduced)');
  });

  test.each([
    ['rise', () => rise({} as Element)],
    ['fade', () => fade({} as Element)],
    ['draw', () => draw({} as Element)],
    ['lateFade', () => lateFade({} as HTMLElement)],
    ['waterEnd', () => waterEnd({ style: { transform: 'translateY(40%)' } } as unknown as HTMLElement, { confirmed: false })],
  ])('%s moves nothing and takes the short fade', (_name, make) => {
    stubTokens(true);
    const c = make();
    const halfway = c.css!(0.5, 0.5);
    expect(halfway).not.toMatch(/translate:\s*0 [1-9]|scale:\s*0\.|inset\(0 0 [1-9]|translateY\((?!40%)/);
    expect(c.duration).toBeLessThanOrEqual(150);
  });

  test('without it, a surface enters from below and a little smaller', () => {
    stubTokens(false);
    expect(rise({} as Element).css!(0, 1)).toMatch(/opacity:\s*0;\s*translate:\s*0 8px;\s*scale:\s*0\.97/);
  });

  test("a confirmed node's water rises to the top; a stopped node's falls away", () => {
    stubTokens(false);
    const el = { style: { transform: 'translateY(40%)' } } as unknown as HTMLElement;
    expect(waterEnd(el, { confirmed: true }).css!(1, 0)).toContain('translateY(40%)');
    expect(waterEnd(el, { confirmed: true }).css!(0, 1)).toContain('translateY(0%)');
    expect(waterEnd(el, { confirmed: false }).css!(0, 1)).toContain('translateY(100%)');
  });
});

describe('the five places use the one system', () => {
  test.each([
    ['screens/Modal.svelte', 'transition:modal'],
    ['screens/Restart.svelte', 'transition:modal'],
    ['screens/Unsaved.svelte', 'transition:modal'],
    ['screens/LinkDialog.svelte', 'transition:modal'],
    ['screens/Reconnect.svelte', 'transition:modal'],
    ['screens/Toast.svelte', 'transition:rise'],
    ['screens/Start.svelte', 'transition:fade'],
    ['App.svelte', 'transition:rise'],
    ['graph/Graph.svelte', 'transition:rise'],
    ['graph/Graph.svelte', 'transition:draw'],
    ['graph/Graph.svelte', 'out:waterEnd'],
  ])('%s has %s', (rel, directive) => {
    expect(read(rel)).toContain(directive);
  });

  test('a dialog and its scrim enter and leave in CSS (the top layer has no other way), from the tokens', () => {
    const base = cssOf('styles/base.css');
    expect(base).toMatch(/@starting-style\s*\{[^}]*dialog\[open\]/);
    expect(base).toMatch(/dialog::backdrop\s*\{[^}]*transition/);
    expect(base).toMatch(/overlay var\(--dur-ui\) allow-discrete/);
  });
});
