import { describe, expect, test } from 'vitest';
import { bandOf } from './band.ts';
import { BAND, BUTTON, SIM } from './copy.ts';
import { MACHINE, view, type ConnectFailure, type L } from './machine.ts';

describe('bottom band', () => {
  // Superseded: "見本を置く" beside it; the sample now comes from SC-S0 only (owner 11/3).
  test('empty canvas: the connect sentence, find automatically as the main button, nothing beside it', () => {
    const b = bandOf('L02', { hasConfig: false });
    expect(b.sentence).toBe(BAND.M03.sentence);
    expect(b.primary?.label).toBe(BUTTON.connectDiscover);
    expect(b.secondary).toEqual([]);
  });

  test('dagon demo: before connecting, the band says no device list appears for the simulated 親機', () => {
    for (const st of ['L02', 'L03'] as const) {
      expect(bandOf(st, { hasConfig: true }, { sim: true }).sentence).toBe(SIM.connect.sentence);
    }
    expect(bandOf('L04', { hasConfig: true }, { sim: true }).sentence).toBe(SIM.connect.sentence);
    expect(bandOf('L02', { hasConfig: true }).sentence).toBe(BAND.M03.sentence);
  });

  test('a file for another 構成 (L17): choose another, or fix it in 定義; no sending from here', () => {
    const b = bandOf('L17', { hasConfig: true, refusal: 'topology' });
    expect([b.sentence, b.primary?.label, b.secondary.map((s) => s.label)]).toEqual([BAND.M20.sentence, BUTTON.otherFile, [BUTTON.defineFix]]);
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

  test('sending has no button, only how to wait; a failed send offers sending again', () => {
    expect(bandOf('L10', { hasConfig: true })).toEqual({ ...BAND.M61, primary: null, secondary: [] });
    const b = bandOf('L11', { hasConfig: true });
    expect([b.chip, b.primary?.label]).toEqual([BAND.M12.chip, BUTTON.pushAgain]);
  });

  test('watching: choose the update file, or use the latest release', () => {
    const b = bandOf('L12', { hasConfig: true });
    expect([b.sentence, b.primary?.label, b.secondary[0].label]).toEqual([BAND.M13.sentence, BUTTON.chooseFile, BUTTON.useLatest]);
  });

  // Superseded (owner 11/2): recovery was the main action; updating the answering nodes now comes first.
  test('a node stopped answering: update the answering ones first, recovery beside it, a power and wiring hint', () => {
    const b = bandOf('L13', { hasConfig: true, answering: true }, { stuck: 'ウォッチ', responding: 2 });
    expect([b.sentence, b.primary?.label, b.secondary.map((x) => x.label)]).toEqual([
      BAND.M62b.sentence,
      '応答する 2 つだけ先に更新',
      ['ウォッチを復旧する'],
    ]);
    expect(b.sentence).toMatch(/電源と配線/);
  });

  test('no node answers at all: recovery is the main action, nothing to update first', () => {
    const b = bandOf('L13', { hasConfig: true, answering: false }, { stuck: 'ウォッチ', responding: 0 });
    expect([b.primary?.label, b.secondary]).toEqual(['ウォッチを復旧する', []]);
  });

  // Same-class sweep: one short sentence in every operating state, done and no-answer included (W9, owner 11/2).
  const FAILURE: Partial<Record<L, ConnectFailure>> = { L04: 'not-found', L05: 'busy', L07: 'no-response' };
  test.each((Object.keys(MACHINE) as L[]).filter((s) => s !== 'L01'))('%s: one short sentence and a step to highlight', (s) => {
    const c = { hasConfig: true, failure: FAILURE[s], refusal: 'source' as const };
    const b = bandOf(s, c, { stuck: '7セグ', unwritten: 1 });
    expect(b.sentence, s).toBeTruthy();
    expect(b.sentence!.match(/。/g)?.length, s).toBeLessThanOrEqual(2);
    expect(view(s, c).step, s).not.toBeNull();
  });

  test('done and stopped without nodes left: the result in one sentence', () => {
    expect(bandOf('L23', { hasConfig: true }).sentence).toBe(BAND.M66.sentence);
    expect(bandOf('L22', { hasConfig: true }, { stuck: '7セグ', unwritten: 0 }).sentence).toBe(BAND.M65.sentence);
    expect(bandOf('L20', { hasConfig: true }).sentence).toBe(BAND.M64.sentence);
  });
});
