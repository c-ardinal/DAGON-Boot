// The design rules every component's CSS must keep (design-rules.md): spacing
// on the 4 px scale, type sizes only from the scale in tokens.css. Run over
// every .svelte and .css file under src.
import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';

const files = Object.keys(import.meta.glob(['../**/*.svelte', '../**/*.css'])).map((p) => new URL(p, import.meta.url));
const read = (u: URL): string => new TextDecoder().decode(readFileSync(u));
const name = (u: URL) => u.pathname.split('/src/')[1];

/** The type scale: one role, one token; nothing else sizes text. */
const SCALE = ['display', 'version', 'title', 'heading', 'body', 'small', 'caption', 'mono'];
const ROOT_PX = 16;
const STEP = 4;

/** The `<style>` of a .svelte file, or the whole .css file. */
function css(u: URL): string {
  const s = read(u);
  return u.pathname.endsWith('.css') ? s : (/<style[^>]*>([\s\S]*?)<\/style>/.exec(s)?.[1] ?? '');
}

/** Lengths in a declaration value: `12px`, `0.75rem`, `-8px`; `0` and `var()` are left alone. */
const lengths = (value: string) => [...value.matchAll(/(-?\d*\.?\d+)(px|rem)/g)].map((m) => (m[2] === 'rem' ? Number(m[1]) * ROOT_PX : Number(m[1])));
const onScale = (px: number) => Math.abs(px) % STEP === 0;

/** Properties that are spacing: the distance between things and the room inside them. */
const SPACING = /^\s*(padding|margin|gap|row-gap|column-gap|inset|top|right|bottom|left)(-[a-z]+)?\s*:\s*([^;]+);/gm;

describe('design rules', () => {
  test('files are found', () => {
    expect(files.length).toBeGreaterThan(20);
  });

  test('every spacing value is on the 4 px scale', () => {
    const faults: string[] = [];
    for (const u of files) {
      for (const m of css(u).matchAll(SPACING)) {
        const bad = lengths(m[3]).filter((px) => !onScale(px));
        if (bad.length) faults.push(`${name(u)}: ${m[0].trim()}`);
      }
    }
    expect(faults).toEqual([]);
  });

  test('spacing tokens are on the 4 px scale', () => {
    const t = read(new URL('./tokens.css', import.meta.url));
    const bad = [...t.matchAll(/--(s-\d|p-[a-z-]+|m-edge|h-[a-z-]+|w-[a-z-]+|i-[a-z]+):\s*([^;]+);/g)].filter((m) => lengths(m[2]).some((px) => !onScale(px)));
    expect(bad.map((m) => m[0])).toEqual([]);
  });

  test('text is sized only by the type scale tokens', () => {
    const faults: string[] = [];
    const sizeOf = /^\s*font(-size)?\s*:\s*([^;]+);/gm;
    for (const u of files) {
      if (u.pathname.endsWith('tokens.css')) continue;
      for (const m of css(u).matchAll(sizeOf)) {
        const v = m[2].trim();
        if (v === 'inherit') continue;
        // The size: the first token or length of a `font:` shorthand, or the value of `font-size:`.
        const size = m[1] ? v : (/(var\(--t-[a-z0-9-]+\)|\d*\.?\d+(?:px|rem|em|%))(?=\s*\/|\s)/.exec(v)?.[1] ?? v);
        // Only scale tokens size text: no literal length, and every --t- token named is one of the scale (a canvas card may hold a token's size over the zoom).
        const tokens = [...size.matchAll(/var\(--t-([a-z]+)\)/g)].map((t) => t[1]);
        const ok = tokens.length > 0 && tokens.every((t) => SCALE.includes(t)) && !/\d(px|rem|em)/.test(size);
        if (!ok) faults.push(`${name(u)}: ${m[0].trim()}`);
      }
    }
    expect(faults).toEqual([]);
  });

  test('tokens.css defines the type scale and nothing beside it', () => {
    const t = read(new URL('./tokens.css', import.meta.url));
    const defined = [...new Set([...t.matchAll(/--t-([a-z0-9-]+):/g)].map((m) => m[1]))];
    expect(defined.sort()).toEqual([...SCALE].sort());
  });

  test('line heights are on the 4 px scale', () => {
    const faults: string[] = [];
    for (const u of files) {
      for (const m of css(u).matchAll(/^\s*(font\s*:[^;]*\/\s*|line-height\s*:\s*)([^;\s]+)/gm)) {
        const lh = m[2].trim();
        if (/^(1|0\.\d+|1\.\d+|normal|inherit)$/.test(lh) && !lh.endsWith('px')) continue;
        if (lengths(lh).some((px) => !onScale(px))) faults.push(`${name(u)}: ${m[0].trim()}`);
      }
    }
    expect(faults).toEqual([]);
  });
});
