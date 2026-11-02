import { describe, expect, test } from 'vitest';
import { SAMPLE_HUB_HELLO } from '../serial/mock.ts';
import type { ConnectFailure } from './connect.ts';
import { bandOf, focusOf, hubOf, statusOf, type View } from './connectView.ts';
import { ANNOUNCE, BAND } from './copy.ts';

describe('connect outcomes', () => {
  test('connected: no band, focus on the next-step area, A05 announced', () => {
    const v: View = { kind: 'connected', hello: SAMPLE_HUB_HELLO };
    expect([bandOf(v), focusOf(v), statusOf(v), hubOf(v)]).toEqual([null, 'next', ANNOUNCE.A05, 'connected']);
  });

  test.each([
    ['not-found', BAND.M04],
    ['busy', BAND.M05],
    ['denied', BAND.M06],
    ['no-response', BAND.M08],
  ] as [ConnectFailure, (typeof BAND)[keyof typeof BAND]][])('%s: focus on the button, status is the band chip', (failure, band) => {
    const v: View = { kind: 'failed', failure };
    expect(bandOf(v)).toBe(band);
    expect(focusOf(v)).toBe('button');
    expect(statusOf(v)).toBe('chip' in band ? band.chip : undefined);
    expect(hubOf(v)).toBe('idle');
  });

  test('choosing keeps the button so focus does not drop', () => {
    expect(focusOf({ kind: 'choosing' })).toBe('button');
    expect(statusOf({ kind: 'choosing' })).toBe('');
  });

  test('checking has no band and announces nothing new', () => {
    expect([bandOf({ kind: 'checking' }), focusOf({ kind: 'checking' }), statusOf({ kind: 'checking' })]).toEqual([null, 'next', '']);
  });

  test('lost offers reconnecting', () => {
    expect(bandOf({ kind: 'lost' })).toBe(BAND.M34);
    expect(focusOf({ kind: 'lost' })).toBe('button');
  });

  test('dagon-wasm missing: reload sentence and button, focus on the button', () => {
    const v: View = { kind: 'unavailable' };
    expect(bandOf(v)).toBe(BAND.LOAD);
    expect(focusOf(v)).toBe('button');
    expect(statusOf(v)).toBe(BAND.LOAD.chip);
    expect(hubOf(v)).toBe('idle');
  });

  test('every state has an exit: a button, or a state the page leaves by itself', () => {
    const states: View[] = [
      { kind: 'idle' },
      { kind: 'choosing' },
      { kind: 'failed', failure: 'busy' },
      { kind: 'lost' },
      { kind: 'unavailable' },
    ];
    for (const v of states) expect(bandOf(v)?.button).toBeTruthy();
  });
});
