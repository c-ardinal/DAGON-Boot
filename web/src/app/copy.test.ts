import { describe, expect, test } from 'vitest';
import * as copy from './copy.ts';

const strings = (v: unknown): string[] =>
  typeof v === 'string' ? [v] : v && typeof v === 'object' ? Object.values(v).flatMap(strings) : [];

const shown = Object.entries(copy)
  .filter(([k]) => k !== 'FORBIDDEN_WORDS')
  .flatMap(([, v]) => strings(v));

describe('copy', () => {
  test.each(copy.FORBIDDEN_WORDS)('no screen text uses %s', (word) => {
    expect(shown.filter((s) => s.includes(word))).toEqual([]);
  });

  test('no ASCII ellipsis, no colon in label-value pairs', () => {
    expect(shown.filter((s) => s.includes('...') || s.includes('：'))).toEqual([]);
  });

  test('slots are called スロット (Aスロット, Bスロット, 予備スロット), never 面', () => {
    expect(shown.filter((s) => /(?:[AB]|予備)\s?面/.test(s))).toEqual([]);
  });

  test('sentences are one sentence in polite form', () => {
    const sentences = [...Object.values(copy.BAND).map((b) => b.sentence), copy.UNSUPPORTED.sentence];
    for (const s of sentences) {
      expect(s.match(/。/g)?.length, s).toBe(1);
      expect(s.endsWith('ください。'), s).toBe(true);
      expect(s, s).not.toMatch(/[!！]/);
    }
  });

  test('every failure band names its button', () => {
    for (const b of Object.values(copy.BAND)) expect(b.button.length).toBeGreaterThan(0);
  });
});
