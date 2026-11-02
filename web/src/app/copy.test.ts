import { describe, expect, test } from 'vitest';
import * as copy from './copy.ts';
import { MACHINE, view, type Action } from './machine.ts';

const strings = (v: unknown): string[] =>
  typeof v === 'string' ? [v] : v && typeof v === 'object' ? Object.values(v).flatMap(strings) : [];

/** Templates filled with sample values, so their fixed words are checked too. */
const filled = [
  copy.BUTTON_FOR.recover('ウォッチ'),
  copy.BUTTON_FOR.updateResponding(2),
  copy.CANVAS.dotRespondingName(3, 3),
  copy.CANVAS.dotFoundName(3),
  copy.NODE.name({ name: '7セグ', product: 'NUCLEO-F401RE', mcu: 'STM32F401RE', role: '子機', via: '親機と I2C 0x42', state: '応答あり', version: '1.0.0' }),
  copy.LINK.bundle('I2C', 5, 3),
  copy.LINK.name('I2C · 5 本（うち共有 3）', 'メイン', '7セグ', '0x42'),
  copy.DETAIL.ms(8),
  copy.ANNOUNCE.A06(3),
  copy.ANNOUNCE.A08(2, 3),
  copy.ANNOUNCE.A21silent('ウォッチ'),
  copy.BAND_FOR.M29('7セグ').sentence,
  copy.CANVAS.dotProgressName(64, true),
  copy.FILE.size(8416),
  copy.RECONNECT.left(30),
  copy.ANNOUNCE.A09('release-1.1.dgb', '595 KB'),
  copy.ANNOUNCE.A10ok(3),
  copy.ANNOUNCE.A11('7セグ'),
  copy.ANNOUNCE.A11percent(50),
  copy.ANNOUNCE.A13('7セグ'),
  copy.ANNOUNCE.A13previous('メイン'),
  copy.ANNOUNCE.A14('1.1.0'),
  copy.ANNOUNCE.A15('7セグ', '1.0.0'),
  copy.ANNOUNCE.A16('7セグ', 2),
  copy.NODE_ACTION.back('1.0.0'),
];

const shownExcept = (...skip: string[]) =>
  Object.entries(copy)
    .filter(([k]) => !['FORBIDDEN_WORDS', ...skip].includes(k))
    .flatMap(([, v]) => strings(v))
    .concat(filled);

const shown = shownExcept();

describe('copy', () => {
  test.each(copy.FORBIDDEN_WORDS)('no screen text uses %s', (word) => {
    // 確定 is allowed only as a slot value in the slot section.
    const texts = word === '確定' ? shownExcept('SLOT', 'TRIAL_CHOICE') : shown;
    expect(texts.filter((s) => s.includes(word))).toEqual([]);
  });

  test('no ASCII ellipsis, no colon in label-value pairs', () => {
    expect(shown.filter((s) => s.includes('...') || s.includes('：') || /\S:\s/.test(s))).toEqual([]);
  });

  test('slots are called スロット (A, B, 予備 inside the slot section), never 面', () => {
    expect(shown.filter((s) => /(?:[AB]|予備)\s?面/.test(s))).toEqual([]);
    expect(copy.SLOT.heading).toBe('スロット');
  });

  test('screen words are ノード / リンク / 配線 and バージョン, never 台, 線 alone or 版', () => {
    expect(shown.filter((s) => /台|版|(?<![配本])線(?!表)/.test(s))).toEqual([]);
  });

  test('sentences are one polite sentence without exclamation marks', () => {
    const sentences = [
      ...Object.values(copy.BAND).flatMap((b) => ('sentence' in b ? [b.sentence] : [])),
      copy.UNSUPPORTED.sentence,
      copy.RECONNECT.sentence,
      copy.TRIAL_CHOICE.sentence,
      copy.BAND_FOR.M29('7セグ').sentence,
      copy.CANVAS.empty,
      copy.DETAIL.nothingSelected,
      ...copy.HELP.rows.map(([, s]) => s),
    ];
    // M57's two sentences are the copy table's own: the cause, then the step.
    const twoSentences: string[] = [copy.BAND.M57.sentence];
    for (const s of sentences) {
      expect(s.match(/。/g)?.length, s).toBe(twoSentences.includes(s) ? 2 : 1);
      expect(s.endsWith('。'), s).toBe(true);
      expect(s, s).not.toMatch(/[!！]/);
    }
  });

  test('chips are a symbol and a short word, never a sentence', () => {
    const { responding, silent, trial, writing, waiting, done, unwritten, rebooting, previous } = copy.NODE;
    const { sourceOk, sourceBad, topologyOk, topologyBad, notRelease } = copy.FILE;
    const chips = [responding, silent, trial, writing, waiting, done, unwritten, rebooting, previous, ...Object.values(copy.NODE.refused), copy.NODE.refusedOther, copy.NODE.lastResort, copy.NODE.cannotFix, sourceOk, sourceBad, topologyOk, topologyBad, notRelease, ...Object.values(copy.HUB)];
    for (const c of chips) {
      expect(c, c).toMatch(/^(?:[●○×…↑↺✓▲◎]|▼▼) \S/);
      expect(c, c).not.toMatch(/ます|ました|。/);
    }
  });

  test('every button the machine can show has a name', () => {
    const actions = new Set<Action>();
    for (const s of Object.keys(MACHINE) as (keyof typeof MACHINE)[]) {
      for (const hasConfig of [false, true]) {
        const v = view(s, { hasConfig });
        for (const a of [v.primary, ...v.secondary]) if (a) actions.add(a);
      }
    }
    for (const a of actions) {
      const name = a in copy.BUTTON ? copy.BUTTON[a as keyof typeof copy.BUTTON] : a in copy.BUTTON_FOR ? 'fn' : '';
      expect(name, a).not.toBe('');
    }
  });

  test('every failure chip in the band comes with the sentence that says how to go on', () => {
    for (const b of Object.values(copy.BAND)) {
      if ('chip' in b && b.chip.startsWith('×')) expect('sentence' in b && b.sentence, b.chip).toBeTruthy();
    }
  });
});
