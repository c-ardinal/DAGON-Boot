import { describe, expect, test } from 'vitest';
import { bandOf } from './band.ts';
import { BAND, BUTTON } from './copy.ts';
import type { ConnectFailure } from './machine.ts';

describe('bottom band', () => {
  test('empty canvas: the connect sentence, find automatically as the main button, the sample beside it', () => {
    const b = bandOf('L02', { hasConfig: false });
    expect(b.sentence).toBe(BAND.M03.sentence);
    expect(b.primary?.label).toBe(BUTTON.connectDiscover);
    expect(b.secondary.map((s) => s.label)).toEqual([BUTTON.placeSample]);
  });

  test.each([
    ['L04', 'not-found', BAND.M04, BUTTON.connect],
    ['L05', 'busy', BAND.M05, BUTTON.connect],
    ['L05', 'denied', BAND.M06, BUTTON.connect],
    ['L05', 'unavailable', BAND.M57, BUTTON.reload],
    ['L07', 'no-response', BAND.M08, BUTTON.reselect],
  ] as const)('%s %s: cause chip, how to go on, and the button', (s, failure, copy, button) => {
    const b = bandOf(s, { hasConfig: false, failure: failure as ConnectFailure });
    expect([b.chip, b.sentence, b.primary?.label]).toEqual([copy.chip, copy.sentence, button]);
  });

  test('found or placed, not yet sent: check the desk, then send', () => {
    const b = bandOf('L09', { hasConfig: true });
    expect([b.sentence, b.primary?.label]).toEqual([BAND.M10.sentence, BUTTON.push]);
  });

  test('sending has no button and no sentence; a failed send offers sending again', () => {
    expect(bandOf('L10', { hasConfig: true })).toEqual({ primary: null, secondary: [] });
    const b = bandOf('L11', { hasConfig: true });
    expect([b.chip, b.primary?.label]).toEqual([BAND.M12.chip, BUTTON.pushAgain]);
  });

  test('watching: choose the update file, or use the latest release', () => {
    const b = bandOf('L12', { hasConfig: true });
    expect([b.sentence, b.primary?.label, b.secondary[0].label]).toEqual([BAND.M13.sentence, BUTTON.chooseFile, BUTTON.useLatest]);
  });

  test('a node stopped answering: the time chip and its recovery button by name, no sentence', () => {
    const b = bandOf('L13', { hasConfig: true }, { stuck: 'ウォッチ', responding: 2 });
    expect([b.pair?.join(' '), b.sentence, b.primary?.label, b.secondary[0].label]).toEqual([
      '目安 約 1 分',
      undefined,
      'ウォッチを復旧する',
      '応答する 2 つだけ先に更新',
    ]);
  });
});
