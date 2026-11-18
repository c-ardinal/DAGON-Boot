import { describe, expect, test } from 'vitest';
import * as copy from './copy.ts';
import { MACHINE, view, type Action } from './machine.ts';
import { MOCK_TEXTS } from './agentMock.ts';
import { link_kind_table } from '../wasm/pkg/dagon_wasm.js';

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
  copy.ANNOUNCE.found(3),
  copy.ANNOUNCE.responding(2, 3),
  copy.ANNOUNCE.nodeSilent('ウォッチ'),
  copy.BAND_FOR.stoppedAt('7セグ').sentence,
  copy.CANVAS.dotProgressName(64, true),
  copy.FILE.size(8416),
  copy.RECONNECT.left(30),
  copy.ANNOUNCE.fileChecking('release-1.1.dgb', '595 KB'),
  copy.ANNOUNCE.fileOk(3),
  copy.ANNOUNCE.writing('7セグ'),
  copy.ANNOUNCE.progress(50),
  copy.ANNOUNCE.stopped('7セグ'),
  copy.ANNOUNCE.previous('メイン'),
  copy.ANNOUNCE.done('1.1.0'),
  copy.ANNOUNCE.rolledBack('7セグ', '1.0.0'),
  copy.ANNOUNCE.recovered('7セグ', 2),
  copy.NODE_ACTION.back('1.0.0'),
  copy.ANNOUNCE.placed('NUCLEO-F401RE', false),
  copy.ANNOUNCE.removed('ウォッチ'),
  copy.ANNOUNCE.problems(2, 1),
  copy.DEFINE.dotNodesName(3, 2),
  copy.DEFINE.problems(2),
  copy.DEFINE.cautions(1),
  copy.INSPECT.inUse('7セグのリセット'),
  ...Object.values(copy.RULE_FIX).map((f) => f({ to: '7セグ', board: 'NUCLEO-F401RE', id: 'seg', many: false, max: 32 })),
];

/**
 * Any argument a template reads: text, a number, a field of an object, a call. With the falsy
 * values beside it, every branch of a template's ternaries shows its fixed words.
 */
const ANY: unknown = new Proxy(function any() {}, {
  get: (_, k) => (k === Symbol.toPrimitive ? (hint: string) => (hint === 'number' ? 2 : 'x') : typeof k === 'symbol' ? undefined : ANY),
  apply: () => ANY,
});
const SAMPLES: unknown[] = [ANY, 0, '', false, true, 1, 'x'];
const ARGS = 4;

/** Every template under `v` (by path), called with each sample value in every argument. */
const templates = (v: unknown, path: string): { path: string; out: string[] }[] => {
  if (typeof v === 'function') {
    const out: string[] = [];
    let called = false;
    for (const s of SAMPLES) {
      try {
        out.push(...strings((v as (...a: unknown[]) => unknown)(...Array<unknown>(ARGS).fill(s))));
        called = true;
      } catch {
        // A sample this template cannot take; another one fills it.
      }
    }
    return called ? [{ path, out }] : [{ path, out: [] }, { path: `${path} (never called)`, out: [] }];
  }
  return v && typeof v === 'object' ? Object.entries(v).flatMap(([k, x]) => templates(x, `${path}.${k}`)) : [];
};

const shownExcept = (...skip: string[]) =>
  Object.entries(copy)
    .filter(([k]) => !['FORBIDDEN_WORDS', ...skip].includes(k))
    .flatMap(([k, v]) => [...strings(v), ...templates(v, k).flatMap((t) => t.out)])
    .concat(filled);

const shown = shownExcept();

describe('the shown strings', () => {
  // Every guard below reads `shown`: a template it never calls would hide its fixed words from all of them.
  test('include every exported template, called', () => {
    const all = templates(copy, 'copy');
    expect(all.filter((t) => t.path.endsWith('(never called)')).map((t) => t.path)).toEqual([]);
    expect(all.length).toBeGreaterThan(60);
    expect(shown).toContain(copy.BAND_FOR.doneSome(1).sentence);
  });
});

describe('copy', () => {
  // The list is for the operate mode's guidance and results; the define mode's CLI screens
  // (CLI 連携, 作業, 署名とリリース, プロジェクト, 書き込み) name signing and the like by design.
  const DEFINE_CLI = ['CLI_NAV', 'CLI_LINK', 'JOBS', 'FLASH'];
  test.each(copy.FORBIDDEN_WORDS)('no screen text uses %s', (word) => {
    // 確定 is allowed only as a slot value in the slot section.
    // 確定 only as a slot value and in the trial choice; フラッシュ only in V-16's name.
    const operate = shownExcept(...DEFINE_CLI);
    // ポート only in the define-mode canvas port's hover tip (LINK.portOut), which names the drag handle.
    const portTips = new Set(SAMPLES.map((s) => copy.LINK.portOut(s as string)));
    const texts = word === '確定' ? shownExcept('SLOT', 'TRIAL_CHOICE', ...DEFINE_CLI) : word === 'フラッシュ' ? operate.filter((s) => s !== copy.RULE_SHORT['V-16'] && s !== copy.INSPECT.flashSize) : word === 'ポート' ? operate.filter((s) => !portTips.has(s)) : operate;
    expect(texts.filter((s) => s.includes(word))).toEqual([]);
  });

  test('no ASCII ellipsis, no colon in label-value pairs', () => {
    // The pin choice writes the in-use note of a pin choice as 「（使用中: …）」 itself.
    const notes = new Set([copy.INSPECT.inUse('7セグのリセット'), ...SAMPLES.map((s) => copy.INSPECT.inUse(s as string))]);
    expect(shown.filter((s) => !notes.has(s) && (s.includes('...') || s.includes('：') || /\S:\s/.test(s)))).toEqual([]);
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
      copy.BAND_FOR.stoppedAt('7セグ').sentence,
      copy.CANVAS.empty,
      copy.DETAIL.nothingSelected,
    ];
    // 困ったとき: each row is the real cause and the next step, two or three polite sentences.
    for (const [, s] of copy.HELP.rows) {
      expect(s.match(/。/g)?.length, s).toBeGreaterThanOrEqual(2);
      expect(s.match(/。/g)?.length, s).toBeLessThanOrEqual(3);
      expect(s.endsWith('。'), s).toBe(true);
      expect(s, s).not.toMatch(/[!！]/);
    }
    // pageLoadFailed says the cause, then the step; writing's reason left its brackets.
    const twoSentences: string[] = [copy.BAND.pageLoadFailed.sentence, copy.BAND.writing.sentence];
    for (const s of sentences) {
      expect(s.match(/。/g)?.length, s).toBe(twoSentences.includes(s) ? 2 : 1);
      expect(s.endsWith('。'), s).toBe(true);
      expect(s, s).not.toMatch(/[!！]/);
    }
  });

  test('chips are a symbol and a short word, never a sentence', () => {
    const { responding, silent, trial, writing, waiting, done, unwritten, rebooting, previous } = copy.NODE;
    const { sourceBad, topologyBad, notRelease } = copy.FILE;
    const chips = [responding, silent, trial, writing, waiting, done, unwritten, rebooting, previous, ...Object.values(copy.NODE.refused), copy.NODE.refusedOther, copy.NODE.lastResort, copy.NODE.cannotFix, sourceBad, topologyBad, notRelease, copy.WRITE.previous, copy.WRITE.failed, copy.BUILD.state.running, ...Object.values(copy.HUB)];
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

describe('the two modes', () => {
  test('are 準備 and 書込; 運用 and 定義 name no mode on the screen', () => {
    expect([copy.MODE.define, copy.MODE.operate]).toEqual(['準備', '書込']);
    expect(shownExcept().filter((s) => s.includes('運用'))).toEqual([]);
  });
});

describe('the writing vocabulary', () => {
  // Writing to nodes is 書き込む / 書き込み中 / 書き込み済み / 書き込めない and the .dgb is the パッケージファイル.
  // 更新 is barred as a word for writing to nodes; the terms of use being updated is not writing.
  const NOT_WRITING = new Set<string>([copy.AGENT.errors.AGENT_CONSENT_VERSION.text]);
  test('no screen text (nor the stand-in agent) says 更新 for writing', () => {
    expect([...shownExcept(), ...MOCK_TEXTS()].filter((s) => s.includes('更新') && !NOT_WRITING.has(s))).toEqual([]);
  });
});

describe('copy keys, screen words and source say what things are', () => {
  // Keys and comments are read by contributors; screen words state what DAGON does. Limits live
  // on the limitations page, not as a pending status on the screen.
  const ID_KEY = /^[A-Z]{1,2}\d{2}[a-z]*$/;
  const keys = (v: unknown): string[] =>
    v && typeof v === 'object' ? Object.entries(v).flatMap(([k, x]) => [k, ...keys(x)]) : [];
  test('no copy key is shaped like a table id (the V-NN rule codes are product codes)', () => {
    expect(keys(copy).filter((k) => ID_KEY.test(k))).toEqual([]);
  });

  // まだ about the user's own work (nothing built, nothing logged) is a fact about them, not about DAGON.
  const USER_STATE = new Set<string>([
    copy.DRAWER.outputNone,
    copy.MENU.off.noLog,
    copy.BUILD.noSkeleton.reason,
    copy.HELP.rows[1][1],
    copy.HELP.rows[4][1],
  ]);
  test('no shown string states a pending status of DAGON', () => {
    const texts = [...shown, ...MOCK_TEXTS()];
    expect(texts.filter((s) => s.includes('未対応') || (s.includes('まだ') && !USER_STATE.has(s)))).toEqual([]);
  });

  const sources = Object.entries(
    import.meta.glob(['../**/*.{ts,svelte}', '!../**/*.test.ts', '!../wasm/pkg/**'], { query: '?raw', import: 'default', eager: true }) as Record<string, string>,
  );
  const comments = (src: string) => src.match(/\/\*[\s\S]*?\*\/|(?<![:'"\w])\/\/[^\n]*|<!--[\s\S]*?-->/g) ?? [];
  const ID_CITED = /(?<![\w-])[A-Z]{1,2}\d{2}[a-z]*(?!\w)|\bUC-[A-Z]\d+/;
  test('the sources are found', () => {
    expect(sources.length).toBeGreaterThan(20);
  });
  test('no identifier says notYet and no comment cites a table id', () => {
    expect(sources.filter(([, s]) => /notYet|NotYet/.test(s)).map(([p]) => p)).toEqual([]);
    expect(sources.flatMap(([p, s]) => comments(s).filter((c) => ID_CITED.test(c)).map((c) => `${p}: ${c}`))).toEqual([]);
  });
});

describe('copy names no sample', () => {
  // Samples are data serve hands out; the screen words never name one (or the hardware it was made on).
  const samples = Object.keys(import.meta.glob('../../../topology/*.yaml')).map((p) => p.split('/').pop()!.replace(/\.yaml$/, ''));
  test('the samples are found', () => {
    expect(samples.length).toBeGreaterThan(0);
  });
  test('no screen text contains a sample name', () => {
    expect(shownExcept().filter((s) => samples.some((n) => s.includes(n)))).toEqual([]);
  });
});

describe('the link kind words', () => {
  // One table of display words, keyed by the names dagon-wasm's kind table gives: a kind added to
  // dagon_format::with_link_kinds! fails here until it has its words, and a key no kind has fails too.
  test('are keyed by every kind and nothing else', () => {
    const names = (link_kind_table() as { name: string }[]).map((k) => k.name);
    expect(Object.keys(copy.LINK_KIND_WORDS).sort()).toEqual([...names].sort());
  });
});
