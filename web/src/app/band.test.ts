import { describe, expect, test } from 'vitest';
import { bandOf, connectNotice } from './band.ts';
import { BAND, BUTTON, SIM } from './copy.ts';
import { MACHINE, view, type ConnectFailure, type L } from './machine.ts';

describe('how a failed connection is told', () => {
  test('what the user must fix opens a dialog; choosing nothing is a toast; anything else is neither', () => {
    expect(connectNotice('portFailed')).toBe('modal');
    expect(connectNotice('silentHub')).toBe('modal');
    expect(connectNotice('notChosen')).toBe('toast');
    for (const s of ['idle', 'choosing', 'checking', 'ready', 'writing', 'stopped'] as const) expect(connectNotice(s), s).toBeNull();
  });
});

describe('bottom band', () => {
  // No "見本を置く" beside it: the sample comes from the start screen only.
  test('empty canvas: the connect sentence, find automatically as the main button, nothing beside it', () => {
    const b = bandOf('idle', { hasConfig: false });
    expect(b.sentence).toBe(BAND.M03.sentence);
    expect(b.primary?.label).toBe(BUTTON.connectDiscover);
    expect(b.secondary).toEqual([]);
  });

  test('dagon demo: before connecting, the band says no device list appears for the simulated 親機', () => {
    for (const st of ['idle', 'choosing'] as const) {
      expect(bandOf(st, { hasConfig: true }, { sim: true }).sentence).toBe(SIM.connect.sentence);
    }
    expect(bandOf('notChosen', { hasConfig: true }, { sim: true }).sentence).toBe(SIM.connect.sentence);
    expect(bandOf('idle', { hasConfig: true }).sentence).toBe(BAND.M03.sentence);
  });

  test('a file for another 構成 (otherTopology): choose another, or fix it in 定義; no sending from here', () => {
    const b = bandOf('otherTopology', { hasConfig: true, refusal: 'topology' });
    expect([b.sentence, b.primary?.label, b.secondary.map((s) => s.label)]).toEqual([BAND.M20.sentence, BUTTON.otherFile, [BUTTON.defineFix]]);
  });

  test.each([
    ['notChosen', 'not-found', BAND.M04, BUTTON.connect],
    ['portFailed', 'busy', BAND.M05, BUTTON.connect],
    ['portFailed', 'denied', BAND.M06, BUTTON.connect],
    ['portFailed', 'unavailable', BAND.M57, BUTTON.reload],
    ['silentHub', 'no-response', BAND.M08, BUTTON.reselect],
  ] as const)('%s %s: cause chip, how to go on, and the button', (s, failure, copy, button) => {
    const b = bandOf(s, { hasConfig: false, failure: failure as ConnectFailure });
    expect([b.chip, b.sentence, b.primary?.label]).toEqual([copy.chip, copy.sentence, button]);
  });

  test('found or placed, not yet sent: check the wiring, then send', () => {
    const b = bandOf('scanned', { hasConfig: true });
    expect([b.sentence, b.primary?.label]).toEqual([BAND.M10.sentence, BUTTON.push]);
  });

  test('sending has no button, only how to wait; a failed send offers sending again', () => {
    expect(bandOf('sending', { hasConfig: true })).toEqual({ ...BAND.M61, primary: null, secondary: [] });
    const b = bandOf('sendFailed', { hasConfig: true });
    expect([b.chip, b.primary?.label]).toEqual([BAND.M12.chip, BUTTON.pushAgain]);
  });

  test('watching: choose the update file, or use the latest release', () => {
    const b = bandOf('ready', { hasConfig: true });
    expect([b.sentence, b.primary?.label, b.secondary[0].label]).toEqual([BAND.M13.sentence, BUTTON.chooseFile, BUTTON.useLatest]);
  });

  // Updating the answering nodes comes first; recovery is beside it.
  test('a node stopped answering: update the answering ones, a power and wiring hint; it is written from its card', () => {
    const b = bandOf('nodesSilent', { hasConfig: true, answering: true }, { stuck: 'ウォッチ', responding: 2 });
    expect([b.sentence, b.primary?.label, b.secondary.map((x) => x.label)]).toEqual([
      BAND.M62b.sentence,
      '応答する 2 つだけ先に書き込む',
      [],
    ]);
    expect(b.sentence).toMatch(/電源と配線/);
  });

  test('no node answers at all: writing it is the main action, nothing to update first', () => {
    const b = bandOf('nodesSilent', { hasConfig: true, answering: false }, { stuck: 'ウォッチ', responding: 0 });
    expect([b.primary?.label, b.secondary]).toEqual(['ウォッチへ書き込む', []]);
  });

  test('sending failed needs the user: a modal, not a toast', () => {
    expect(connectNotice('sendFailed')).toBe('modal');
  });

  // One short sentence in every operating state, done and no-answer included.
  const FAILURE: Partial<Record<L, ConnectFailure>> = { notChosen: 'not-found', portFailed: 'busy', silentHub: 'no-response' };
  test.each((Object.keys(MACHINE) as L[]).filter((s) => s !== 'unsupported'))('%s: one short sentence and a step to highlight', (s) => {
    const c = { hasConfig: true, failure: FAILURE[s], refusal: 'source' as const };
    const b = bandOf(s, c, { stuck: '7セグ', unwritten: 1 });
    expect(b.sentence, s).toBeTruthy();
    expect(b.sentence!.match(/。/g)?.length, s).toBeLessThanOrEqual(2);
    expect(view(s, c).step, s).not.toBeNull();
  });

  test('done and stopped without nodes left: the result in one sentence', () => {
    expect(bandOf('done', { hasConfig: true }).sentence).toBe(BAND.M66.sentence);
    expect(bandOf('stopped', { hasConfig: true }, { stuck: '7セグ', unwritten: 0 }).sentence).toBe(BAND.M65.sentence);
    expect(bandOf('hubRestarting', { hasConfig: true }).sentence).toBe(BAND.M64.sentence);
  });
});
