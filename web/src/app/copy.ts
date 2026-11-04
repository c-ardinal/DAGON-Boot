// Every visible and announced string of the page. Chips and labels use the
// "label value" / "symbol + word" form; sentences appear only where the user
// acts, one sentence each. Comments name the copy-table ids.
import type { Action, HubState, Step } from './machine.ts';

export const PRODUCT = 'DAGON Console';

export const SKIP = {
  canvas: 'キャンバスへ',
  next: '次にすることへ',
} as const;

/**
 * Header step indicator in three steps (owner 11/3): ① つなぐ ② ファイル ③ 更新.
 * The operate machine's six steps fold into them (STEP.of3); recovery shows its name only.
 */
export const STEP = {
  label: '手順',
  of: '/03',
  names: { 1: 'つなぐ', 2: 'ファイル', 3: '更新', recover: '復旧' } as Record<1 | 2 | 3 | 'recover', string>,
  of3: (s: Step): 1 | 2 | 3 | 'recover' => (s === 'recover' ? s : s <= 3 ? 1 : s === 4 ? 2 : 3),
} as const;

/** M48: hub connection in the header (no node counts here). */
export const HUB = {
  idle: '○ 親機 未接続',
  checking: '… 親機 確認中',
  connected: '● 親機 接続中',
  reconnecting: '… 親機 再接続中',
  lost: '× 親機 切断',
  cli: '● 親機 CLI 使用中',
} as const satisfies Record<HubState, string>;

/** M48 accessible names ("親機、接続中"). */
export const HUB_NAME = {
  idle: '親機、未接続',
  checking: '親機、確認中',
  connected: '親機、接続中',
  reconnecting: '親機、再接続中',
  lost: '親機、切断',
  cli: '親機、CLI 使用中',
} as const satisfies Record<HubState, string>;

/** CLI link in the header (K0). */
export const CLI_NONE = 'CLI 未連携';

/** Bottom-band buttons (fixed names); per-node names are in BUTTON_FOR. */
export const BUTTON = {
  copyUrl: 'この URL をコピー',
  connectDiscover: 'つないで、自動で見つける',
  connect: 'パソコンとつなぐ',
  placeSample: '見本を置く',
  reselect: '選び直す',
  reload: 'もう一度読み込む',
  push: '親機へ送る',
  pushAgain: 'もう一度送る',
  scanAgain: 'もう一度探す',
  defineFix: '定義で直す',
  chooseFile: '更新ファイルを選ぶ',
  useLatest: '最新のリリースを使う',
  otherFile: '別のファイルを選ぶ',
  update: '更新',
  reconnect: 'もう一度つなぐ',
  recoverAgain: 'もう一度直す',
  copyLog: '記録をコピー',
  retrySample: 'もう一度作る',
  placeInDefine: '定義で置く',
  showJobs: '作業を見る',
} as const satisfies Record<Exclude<Action, 'recover' | 'updateResponding'>, string>;

/** Buttons named after a node or a count (M14, M47). */
export const BUTTON_FOR = {
  recover: (name: string) => `${name}を復旧する`,
  updateResponding: (n: number) => `応答する ${n} つだけ先に更新`,
  /** M50: the update after a recovery, over the nodes not written yet. */
  updateRest: '残りのノードを更新',
} as const;

export interface BandCopy {
  /** Cause chip, shown before the sentence (absent when shown elsewhere). */
  chip?: string;
  /** A label-value pair in place of a sentence. */
  pair?: readonly [string, string];
  sentence?: string;
}

/** Bottom band: the cause chip and the one sentence beside the buttons. */
export const BAND = {
  M03: { sentence: '次に出る一覧で「DAGON hub」を選び、『接続』を押してください。' },
  M04: { chip: '× 見つからない', sentence: 'USB ケーブルを挿し直してから、もう一度押してください。' },
  M05: { chip: '× ほかで使用中', sentence: '親機を使っているタブやアプリ（dagon CLI を含む）を閉じてから、もう一度押してください。' },
  M06: { chip: '× 許可なし', sentence: 'ターミナルに表示されたアドレスから開き直してください。' },
  M08: { chip: '× 応答なし', sentence: '一覧から「DAGON hub」を選び直してください。' },
  M10: { sentence: '机の上と同じか確かめてから、親機へ送ってください。' },
  M12: { chip: '× 送れない', sentence: 'USB ケーブルを確かめてから、もう一度送ってください。' },
  M13: { sentence: '更新ファイルをこの画面に置くか、選んでください。' },
  /** M14 / M15 / M28: label-value pair, no sentence. */
  M14: { pair: ['目安', '約 1 分'] },
  M18: { sentence: '何も書いていないので、定義でリリースをもう一度作るか、正しいファイルを選んでください。' },
  M18b: { sentence: '何も書いていないので、別のファイルを選んでください。' },
  M19: { sentence: '何も書いていないので、別のファイルを選んでください。' },
  M20: { sentence: '何も書いていないので、この構成向けのファイルを選んでください。' },
  M22: { sentence: '終わるまで電源と USB を抜かず、このページを開いたままにしてください（止まっても前のバージョンのまま動きます）。' },
  M33: { sentence: '基板どうしの配線が抜けていないか確かめてから、もう一度直してください。' },
  M34: { sentence: 'USB ケーブルを挿し直してから、もう一度つないでください。' },
  M35: { chip: '× 作れない', sentence: 'そのフォルダに書き込めるか確かめてから、もう一度選んでください。' },
  /** M58: "最新のリリースを使う" found no release in the project. */
  M58: { chip: '× リリース なし', sentence: '定義でリリースを作るか、更新ファイルを選んでください。' },
  /** M57: part of the page (dagon-wasm) did not load. */
  M57: { chip: '× 読み込めない', sentence: 'このページの一部を読み込めませんでした。ページを開き直してください。' },
  /** M26: L21 (the dialog's sentence, and the band's once the dialog is closed). */
  M26: { sentence: '『もう一度つなぐ』を押して、同じ機器を選んでください。' },
  M59: { sentence: 'そのまま待ってください（数秒で終わります）。' },
  M60: { sentence: '見つかったノードが降りてくるまで、そのまま待ってください。' },
  M61: { sentence: '送り終わるまで、USB を抜かないでください。' },
  /**
   * 案 (replaces M62, owner 11/2: the answering nodes are updated first, recovery is the side action):
   * L13, with a power and wiring hint for the silent ones.
   */
  M62b: { sentence: '応答しないノードは電源と配線を確かめ、応答するノードから先に更新してください。' },
  /** M62: L13 when the answering nodes need nothing more (already updated, or after 「応答する n つだけ先に更新」). */
  M62: { pair: ['目安', '約 1 分'], sentence: '印の付いたノードは、復旧すると元どおりに動きます。' },
  M63: { sentence: '確かめ終わるまで、そのまま待ってください。' },
  M64: { sentence: '親機が再起動したら自動でつなぎ直すので、そのまま待ってください。' },
  M65: { pair: ['目安', '約 1 分'], sentence: '止まったノードを復旧すると、新しいバージョンで動きます。' },
  M66: { sentence: 'すべてのノードが新しいバージョンで動いています。' },
  M67: { sentence: 'CLI の作業が終わると、自動でつなぎ直します。' },
} as const satisfies Record<string, BandCopy>;

/**
 * The header's つなぐ / 切断 (S30, owner 11/2) and, 案, why it cannot be
 * used in a state (the band says it when it is pressed then).
 */
export const HUB_TOGGLE = {
  connect: 'パソコンとつなぐ',
  disconnect: '切断',
  locked: {
    L03: '一覧で選び終わるまで、そのまま待ってください。',
    L08: '探し終わるまで、切断できません。',
    L10: '送り終わるまで、切断できません。',
    L19: '更新が終わるまで、切断できません。',
    L20: '親機とつなぎ直すまで、切断できません。',
    L21: '親機とつなぎ直すまで、切断できません。',
    L24: '復旧が終わるまで、切断できません。',
    L28: 'CLI の作業が終わるまで、切断できません。',
  } as Partial<Record<string, string>>,
} as const;

/** 案: before an update, what each target node gets and about how long it takes (L18, SC-O4). */
export const PLAN = {
  up: (from: string, to: string) => `↑ ${from} → ${to}`,
  upName: (from: string, to: string) => `更新あり、${from} から ${to}`,
  same: (v: string) => `＝ 同じ ${v}`,
  sameName: (v: string) => `同じ、${v}`,
  unknown: '?',
  estimate: (min: number) => `目安 約 ${min} 分`,
  estimateLabel: '目安',
  minutes: (min: number) => `約 ${min} 分`,
} as const;

/** S28 and SC-O5: elapsed and remaining time under the dot number while updating or recovering. */
export const TIME = {
  elapsed: '経過',
  left: '残り',
  soon: 'まもなく',
  /** m:ss, or h:mm:ss past an hour. */
  clock: (ms: number) => {
    const s = Math.max(0, Math.floor(ms / 1000));
    const two = (n: number) => String(n).padStart(2, '0');
    return s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${Math.floor(s / 60)}:${two(s % 60)}`;
  },
  about: (ms: number) => (ms < 30_000 ? 'まもなく' : `約 ${Math.ceil(ms / 60_000)} 分`),
} as const;

/** 案 (C-389): L26's cause chip when the device the CLI gave back was not this 親機. */
export const BAND_CHIP = {
  otherHub: '× 別の親機',
  notHub: '× 親機ではない',
  silent: '× 応答なし',
} as const;

/** M29: beside the recover button when nodes after the stopped one are not written yet. */
export const BAND_FOR = {
  M29: (stuck: string) => ({ pair: ['目安', '約 1 分'] as const, sentence: `${stuck}を直すと、残りのノードを更新できます。` }),
  /** 案 (L23 when M66 is not true): the nodes that did not answer were not updated. */
  M66some: (n: number) => ({ sentence: `応答しなかった ${n} つのノードは前のままなので、電源と配線を確かめてから復旧してください。` }),
  /** 案 (L23 after 「前のバージョンに戻す」). */
  M66back: (name: string, version: string) => ({ sentence: `${name}は前のバージョン ${version} で動いています。` }),
  /** 案 (L23 after a recovery with nothing left to update). */
  M66fixed: (name: string, version: string) => ({ sentence: `${name}は直り、${version} で動いています。` }),
} as const;

/** M01: browser without Web Serial. */
export const UNSUPPORTED = {
  heading: '非対応のブラウザ',
  supportLabel: '対応',
  supportValue: 'Chrome 89 以降・Edge',
  sentence: 'この URL を対応のブラウザで開いてください。',
  button: 'この URL をコピー',
} as const;

/** The bottom drawer (DES-UX3-015) and its tabs. */
export const DRAWER = {
  label: '下の引き出し',
  problems: '問題',
  log: '記録',
  talk: '会話',
  logNone: 'まだ記録はありません。',
} as const;

/** Canvas: uplink cards, the empty canvas, dot numbers. */
export const CANVAS = {
  /** Uplink card of the 親機 this page is connected to now. */
  pc: 'このパソコン（ブラウザ）',
  /** Uplink card by each 親機's uplink host as the 構成 writes it (none written: dagon on a PC). */
  pcBy: { 'pc-cli': 'パソコン（dagon）', browser: 'ブラウザ（パソコン・スマホ）', embedded: '自作の機器' } as Record<string, string>,
  /** C02 */
  empty: '親機を USB でパソコンにつないでから、『つないで、自動で見つける』を押してください。',
  /** S17: canvas description for screen readers. */
  describe: '矢印でノードを移り、Enter で選びます。ほかのキーは「困ったとき」の「キー操作」にあります。',
  /** M49 dot-number labels. */
  dotResponding: '応答',
  dotFound: '発見',
  /** M49 during an update, and after it stopped. */
  dotProgress: '進捗',
  dotPercent: '%',
  dotStopped: '% 停止',
  dotProgressName: (n: number, stopped: boolean) => `進捗 ${n}%${stopped ? '、停止' : ''}`,
  dotRespondingName: (n: number, of: number) => `応答 ${n}/${of}、${of} つのノードのうち ${n} つが応答`,
  dotFoundName: (n: number) => `発見 ${n}`,
  /** S19 */
  wiringLink: '配線表を見る',
} as const;

/** Node card. */
export const NODE = {
  roleHub: '親機',
  roleNode: '子機',
  /** A node under a 子機 (TOPO2), and the rows below that. */
  roleGrandchild: '孫機',
  roleDeeper: (d: number) => `孫機の下（${d} 段目）`,
  mcu: 'MCU',
  /** The hub's own connection line: its uplink kind and what drives it (案 beyond USB（パソコン）). */
  usb: 'USB（パソコン）',
  uplinkVia: (kind: string, host: string) =>
    `${kind}（${({ 'pc-cli': 'パソコン', browser: 'ブラウザ', embedded: '自作の機器' } as Record<string, string>)[host] ?? 'パソコン'}）`,
  /** S03 pin headings and the no-wire value. */
  reset: 'リセット',
  boot: '起動',
  noWire: 'なし（本体側で切替）',
  /** S04 / S38 */
  responding: '● 応答あり',
  silent: '× 応答なし',
  trial: '● 確認待ち',
  /** S04 / S38 during an update. */
  writing: '▼▼ 書き込み中',
  waiting: '… 待ち',
  done: '✓ 完了',
  unwritten: '… 未書き込み',
  rebooting: '… 再起動中',
  previous: '↺ 前のバージョン',
  /**
   * A node that answered and refused the update, by dagon-host's reason (the
   * last-reason words of the detail where they exist). No answer stays
   * `silent`; a node back on its previous version is `previous`.
   */
  refused: {
    E_SIGNATURE: '× 配布元を確認できない',
    E_KEY: '× 配布元を確認できない',
    E_DIGEST: '× 中身の破損',
    E_COMPAT: '× 別の機種向け',
    E_FLASH: '× 書き込めない',
    // dagon-host's release check, in the words of the update file's verdict.
    source: '× 配布元を確認できない',
    key: '× 配布元を確認できない',
    topology: '× 構成 不一致',
    format: '× 更新ファイルではない',
  } as Record<string, string>,
  refusedOther: '× 受け付けない',
  /** S04 during a recovery. */
  lastResort: '◎ 最後の手段',
  cannotFix: '× 直せない',
  /** W7: full spoken state of a chip. */
  respondingName: '応答あり',
  silentName: '応答なし、2 回続けて返事がない',
  trialName: '確認待ち',
  /** Spoken name of a node: the visible short chips spelled out. */
  name: (p: { name: string; product: string; mcu: string; role: string; via: string; state: string; version: string }) =>
    [p.name, `${p.product} · ${p.mcu}`, p.role, p.via, p.state, p.version && `バージョン ${p.version}`].filter(Boolean).join('、'),
  viaName: (via: string) => `親機と ${via}`,
  /** A node updating does not reach yet (V-23): a calm note, not a fault. */
  notYet: '◇ まだ更新できない',
  notYetWhy: {
    'second-hub': '2 つ目の親機の側にあるため',
    deep: '子機の先にあるため',
    'link-kind': 'このつながりの種類では、まだ届かないため',
    'too-many': '更新は親機とその子機 7 つまでのため',
    unreachable: 'どの親機からもリンクがたどれないため',
    'cut-invalid': '更新が届く部分だけでは確認を通らないため（dagon topo check で理由を見られます）',
  } as Record<string, string>,
  notYetName: (why: string) => `まだ更新できない、${why}`,
} as const;

/** Link chip (S18) and its spoken name. */
export const LINK = {
  /** CAN and Ethernet carry no wires the page draws: the kind alone. */
  bundle: (kind: string, n: number, shared: number) => (n ? `${kind} · ${n} 本${shared ? `（うち共有 ${shared}）` : ''}` : kind),
  name: (label: string, from: string, to: string, addr: string) => `${label}、${from}から ${to}へのリンク${addr ? `、${addr}` : ''}`,
} as const;

/** Left column (S02, M43). */
export const ORDER = {
  heading: '更新の順',
  none: 'なし',
} as const;

/** Right column. */
export const DETAIL = {
  /** M40 */
  nothingSelected: 'ノードを選ぶと、ここに詳細が出ます。',
  /** M42 */
  introHeading: 'はじめに',
  /** M42: the same three steps as the header. */
  introSteps: ['つなぐ', 'ファイル', '更新'],
  introNote: '止まっても 復旧で戻せる',
  /** S36 */
  rtt: '往復',
  lastSeen: '最後の応答',
  attempts: '試行回数',
  lastReason: '最後に戻った理由',
  ms: (n: number) => `${n} ms`,
  none: 'なし',
  /** Values of the last-reason code (STATUS last_reason). */
  reasons: { 1: '中身の破損', 2: '配布元を確認できない', 3: '新しいバージョンで起動できない', 5: '起動できるバージョンなし' } as Record<number, string>,
  details: '詳しく見る',
} as const;

/** M38: a node on trial that this page did not apply (the detail, above its buttons). */
export const TRIAL_CHOICE = {
  sentence: 'このバージョンで確定するか、前のバージョンに戻すかを選んでください。',
  keep: 'このバージョンで確定する',
  back: '前のバージョンに戻す',
} as const;

/** The detail's actions and the recovery figures under 詳しく見る. */
export const NODE_ACTION = {
  /** UC-O10, shown when the other slot holds a version to go back to. */
  back: (version: string) => `前のバージョン（${version}）に戻す`,
  /** 詳しく見る: how the last recovery got through. */
  path: '経路',
  paths: { retransfer: '送り直し', factory: '予備へ送る', rom: '最後の手段' },
  romRtt: '往復の最大',
  romAttempts: 'やり直し',
} as const;

/** Slot section (M41). Inside it the rows are A / B / 予備. */
export const SLOT = {
  heading: 'スロット',
  rows: ['A', 'B', '予備'],
  /** Slot state u8 → word. */
  states: ['空', '書いた', '次に試す', '確認待ち', '確定', '戻された'],
  inUse: '使用中',
  previous: '前のバージョン',
  /** The watch's factory slot holds an image. */
  present: 'あり',
} as const;

/** S20 wiring table. */
export const WIRING = {
  heading: '配線表',
  cols: ['信号', '親機', '子機', '共有'],
  shared: '共有',
  own: '—',
  ownName: 'このノードだけ',
  legendHeading: '表の見方',
  legend: [
    ['共有', 'SDA・SCL は I2C の子機すべて、GND は全ノードで同じ配線'],
    ['—', 'このノードだけの配線'],
  ],
  copy: '配線表をコピー',
  print: '配線表を印刷',
} as const;

/** SC-O4: the file and dagon-host's verdict on it (the one place for the verdict). */
export const FILE = {
  heading: '更新ファイル',
  /** File input label (the drop place). */
  choose: '更新ファイルを選ぶ',
  size: (bytes: number) => `${Math.max(1, Math.round(bytes / 1024))} KB`,
  checking: '確認中…',
  source: '配布元',
  sourceOk: '✓ 確認済み',
  sourceBad: '× 確認できない',
  topology: '構成',
  topologyOk: '✓ 一致',
  topologyBad: '× 不一致',
  notRelease: '× 更新ファイルではない',
  targets: '対象',
} as const;

/** SC-O7: the hub reconnect dialog (M26), and M27 beside the hub while the page reconnects by itself (L20). */
export const RECONNECT = {
  heading: '親機の再接続',
  sentence: BAND.M26.sentence,
  auto: '自動の再接続',
  left: (s: number) => `あと ${s} s`,
  /** 案: why the page could not reconnect by itself (the diagnostic log under 詳しく見る). */
  why: '詳しく見る',
  notes: {
    away: '前の親機 開けない',
    none: '同じ種類の機器 現れない',
    several: (n: number) => `同じ種類の機器 ${n} つ（どれか決められない）`,
    silent: '現れた機器 応答なし',
    notHub: '現れた機器 子機',
    otherHub: '現れた機器 別の親機',
    version: (v: string, trial: boolean) => `現れた親機 ${v}${trial ? '' : '（前のバージョン）'}`,
  },
} as const;

/** Help in the header, and the operating-mode key list (S06). */
export const HELP = {
  heading: '困ったとき',
  rows: [
    ['一覧に何も出ない', 'データ用のケーブルに替えてください（充電専用では出ません）。'],
    ['「DAGON hub」が無い', '親機の画面が点いているか確かめてください。'],
    ['何を置けばいいか分からない', '『つないで、自動で見つける』を押してください。'],
    ['手元に無い', '『最新のリリースを使う』を押してください。'],
    ['止まって見える', '経過が増えていれば進んでいます（1 つのノードに 1 分ほど）。'],
  ],
  keysHeading: 'キー操作',
  keys: [
    ['Tab', 'ノードへ移る'],
    ['矢印', '隣のノードへ'],
    ['Enter', '選ぶ'],
  ],
} as const;

/** Visually hidden announcements (role="status" / alert). */
export const ANNOUNCE = {
  A05: '親機 接続、ノード確認中',
  A06: (n: number) => `発見 ${n}`,
  A07: '構成 送信済み',
  A08: (n: number, of: number) => `応答 ${n}/${of}`,
  A21silent: (name: string) => `${name} 応答なし`,
  A21lost: '親機 切断',
  A01: (product: string, hub: boolean) => `${product} 配置、${hub ? '親機' : '子機'}`,
  A03: (name: string) => `${name} 削除、Ctrl+Z で戻せる`,
  A04: (errors: number, warnings: number) => `問題 ${errors}、注意 ${warnings}`,
  A22: (define: boolean) => (define ? '定義モード' : '運用モード'),
  A09: (name: string, size: string) => `${name} ${size}、確認中`,
  A10ok: (n: number, skipped = 0) => `配布元 確認済み、構成 一致、対象 ${n}${skipped ? `、まだ更新できない ${skipped} つは飛ばす` : ''}`,
  A10source: '配布元 確認できない',
  A10topology: '構成 不一致',
  A10format: '更新ファイルではない',
  A11: (name: string) => `${name} 書き込み`,
  A11percent: (n: number) => `進捗 ${n}%`,
  A12: '親機 再起動中、自動で再接続',
  A13: (name: string) => `更新 停止、${name}`,
  A13previous: (name: string) => `${name} 前のバージョン`,
  A14: (version: string) => `完了、${version}`,
  A21timeout: '親機 再接続 時間切れ',
  A15: (name: string, version: string) => `${name} ${version} に戻した`,
  A16: (name: string, rest: number) => `${name} 復旧済み、残り ${rest}`,
  /** "最新のリリースを使う" found no release in the project (M58). */
  A22latest: 'リリース なし',
} as const;

/** Words that never appear in operating-mode guidance, chips or results. */
export const FORBIDDEN_WORDS = [
  '署名', 'ROM', 'CDC', 'ブートローダ', 'エラーコード', 'トポロジ',
  'マニフェスト', 'ハッシュ', 'ポート', 'シリアル', 'フラッシュ', 'ファームウェア',
  'パーティション', 'コマンド', 'タイムアウト', 'WASM', 'Web Serial', '確定', '試行中',
  '巻き戻り', 'エラー', '不正', '致命的', '無効',
] as const;

/** S07: short names of the check rules (problem chips and the problem list). */
export const RULE_SHORT: Record<string, string> = {
  'V-01': 'ファイルの形式',
  'V-02': '名前の重なりとノードの数',
  'V-03': '親機の数',
  'V-04': '子機どうしのリンク',
  'V-05': 'つながっていないノード',
  'V-06': '起動の方式',
  'V-07': '親機に無いピン',
  'V-08': 'ピンの重なり',
  'V-09': 'UART のピン',
  'V-10': 'UART の数',
  'V-11': '起動の切り替え方',
  'V-12': '復旧の層',
  'V-13': '最後の手段のピン',
  'V-14': '更新の順番',
  'V-15': '互換の決まり',
  'V-16': 'フラッシュの大きさ',
  'V-17': 'I2C のピン',
  'V-18': 'アドレスの重なり',
  'V-20': 'I2C の速さ',
  'V-21': 'I2C でつなげないノード',
  'V-22': 'リセット・起動の配線 未設定',
  'V-23': 'まだ更新できない部分',
  'V-24': 'まだ使えないつながり',
  'V-25': '親機にできないマイコン',
  /** Checked by the page (dagon.toml's [board.<id>]), not by dagon-topology. */
  'B-01': 'ボード 未設定',
  'B-02': '容量が足りない',
};

/** Names a §11.2 sentence needs: the node (呼び名), its board, its id, and whether there are too many nodes. */
export interface FixWords {
  to: string;
  board: string;
  id: string;
  many: boolean;
  /** A size such as "512 KiB" (B-02). */
  size?: string;
  /** The node is one updating skips for now (V-23): its link is not a thing to fix. */
  skipped?: boolean;
}

/** §11.2: how to fix each rule, one sentence. */
export const RULE_FIX: Record<string, (w: FixWords) => string> = {
  'V-01': () => 'dagon の新しいバージョンで書き出したファイルを読み込んでください。',
  'V-02': (w) => (w.many ? 'ノードを 32 までにしてください。' : `名前「${w.id}」の 2 つのどちらかを変えてください。`),
  'V-03': () => '親機を 1 つだけにしてください。',
  'V-04': () => '子機どうしのリンクを消し、親機とつないでください。',
  'V-05': (w) => `${w.to}を親機とリンクでつないでください。`,
  'V-06': (w) => `『直す』で ${w.board} に合う起動の方式にしてください。`,
  'V-07': () => '親機のピンを、16・17・18・26・27・28 のどれかに変えてください。',
  'V-08': () => '重なったピンのどちらか 1 か所を、空いているピンに変えてください。',
  'V-09': () => '『直す』で PIO の UART に替えてください。',
  'V-10': () => 'UART のリンクを 1 本減らしてください。',
  'V-11': (w) => `${w.to}の起動の切り替え方を選んでください（最後の手段で直すときに使います）。`,
  'V-12': () => '詳しい設定の「復旧の層」から最後の手段を外してください。',
  'V-13': (w) => `${w.to}を PA8・PB4、PB6・PB7、PB10・PB3 のどれかの組につないでください。`,
  'V-14': () => '『直す』で親機を最後にして並べてください。',
  'V-15': () => '互換の決まりから、無いノードを外してください。',
  'V-16': () => '既定の配置に戻してください。',
  'V-17': () => '共有の 2 本線（I2C）を親機の GPIO 16・17 にしてください。',
  'V-18': (w) => `${w.to}のアドレスを、空いている番号に変えてください。`,
  'V-20': () => '共有の 2 本線（I2C）の速さを 400 kHz 以下にしてください。',
  'V-21': (w) => `${w.to}は UART でつないでください。`,
  'V-22': (w) => `最後の手段で復旧するには、『直す』で ${w.to}のリセット・起動の配線を設定してください（このままでも親機へ送れます）。`,
  'V-23': () => 'この構成のままで正しく使えます。いまの更新はこの部分にまだ届かないため、更新と書き込みではこの部分を飛ばします。',
  'V-24': (w) =>
    w.skipped
      ? `${w.to}のつながりは、まだ使えない種類です。${w.to}は更新と書き込みで飛ばすので、このままで使えます。`
      : `${w.to ? `${w.to}の` : ''}つながりは、まだ使えない種類です。プロジェクトを作るには、使える種類を選んでください（このままでも保存できます）。`,
  'V-25': (w) => `${w.to}の代わりに、親機にできるマイコンのノードを親機にしてください。`,
  'B-01': (w) => `『直す』で ${w.to}のボードの容量を入れてください。`,
  'B-02': (w) => `${w.to}は容量が ${w.size ?? ''} 以上のボードにしてください。`,
};

/** Header: the two modes (運用｜定義). */
export const MODE = {
  operate: '運用',
  define: '定義',
  label: 'モード',
  /** 案: 運用 or 親機へ送る pressed while the 構成 is not in the project's YAML. */
  lockedUnsaved: { sentence: '構成を保存すると、運用と親機へ送るを使えます。' },
  /** M69: 運用 pressed with no 構成 or one with problems. */
  lockedProblems: { sentence: '構成を作り、問題を直すと、運用を使えます。' },
} as const;

/** Define mode (SC-D1–D3, SC-T, SC-M, §11.1). */
export const DEFINE = {
  /** SC-D1 left column. */
  listHeading: 'ノードリスト',
  /** §5.2 SC-D1: choose by product or by MCU. */
  byProduct: '製品',
  byMcu: 'マイコン',
  chooseBy: '選び方',
  listHelp: '選んで Enter で置けます。',
  place: '置く',
  placed: '配置済み',
  firstHeading: 'はじめての方へ',
  connectDiscover: 'つないで、自動で見つける',
  /** Canvas toolbar. */
  undo: '元に戻す',
  redo: 'やり直す',
  graph: 'グラフ',
  table: '表',
  exportYaml: '書き出す',
  importYaml: '読み込む',
  zoomIn: '大きく',
  zoomOut: '小さく',
  /** M49 dot numbers in define mode. */
  dotNodes: 'ノード',
  dotLinks: 'リンク',
  dotNodesName: (n: number, l: number) => `ノード ${n}、リンク ${l}`,
  /** S25: nothing selected and nothing to fix. */
  nothingSelected: 'ノードかリンクを選ぶと、ここで直せます。',
  /** SC-D3 (S23, S15). */
  problems: (n: number) => `! 問題 ${n}`,
  problemsNone: '問題 0',
  cautions: (m: number) => `▲ 注意 ${m}`,
  /** S45: parts not updatable yet or links not usable yet, after the cautions. */
  unsupported: (k: number) => `◇ まだ ${k}`,
  fix: '直す',
  ruleId: '規則 ID',
  yamlPath: 'YAML の場所',
  details: '詳しく見る',
  problemName: (short: string, rule: string, sentence: string) => `${short}、規則 ${rule}、${sentence}`,
  /** Bottom band (C02, C05, C06, C09). */
  emptySentence: '親機を USB でパソコンにつないでから、『つないで、自動で見つける』を押してください。',
  /** E5: 運用 from 定義 (§6.2). */
  toOperate: '運用へ',
  /** Writes the 構成 back to the project's YAML through dagon serve. */
  save: '保存',
  fixSentence: '直すと、親機へ送れます。',
  /** C19: E6, nodes updating skips for now (V-23); the 構成 is valid as it is. */
  notYetSentence: (k: number) => `まだ更新できないノードが ${k} つあります。構成はこのままで使え、更新と書き込みではそのノードを飛ばします。`,
  /** §7.6: the 構成 of the release file chosen in 運用 replaces this one (undoable, saved to the YAML). */
  importFileTopology: 'ファイルの構成を取り込む',
  sendSentence: '親機へ送ると、更新できるようになります。',
  send: '親機へ送る',
  connectSend: 'つないで、親機へ送る',
  unreadableChip: '× 読めない形式',
  unreadableSentence: 'dagon で書き出した .yaml ファイルを選んでください。',
  /** GET /v1/definitions 500: the open project's MCU/board definitions do not load (serve's terminal says why). */
  defsChip: '× 定義を読めない',
  defsSentence: 'このプロジェクトの MCU・ボード定義を読み込めません。理由は dagon serve の端末に出ています。',
} as const;

/** SC-D2 inspector labels; 詳しい設定 shows the schema names as they are (§11.4, 2 段目). */
export const INSPECT = {
  name: '呼び名',
  id: '名前 id',
  role: '役割',
  roles: { coordinator: '親機', node: '子機' } as Record<string, string>,
  product: '製品名 · MCU',
  connectTo: 'つなぐ先',
  none: 'なし',
  reset: 'リセット',
  boot: '起動',
  unset: '未設定',
  advanced: '詳しい設定',
  kind: '種類',
  kinds: { i2c: 'I2C（共有の 2 本線）', uart: 'UART（1 対 1 の配線）' } as Record<string, string>,
  addr: 'アドレス（16 進）',
  freeAddr: '空いている番号',
  nodePins: '相手側のピン',
  uartPins: '親機のピン',
  baud: 'ボーレート',
  removeLink: 'このリンクを消す',
  removeNode: 'このノードを消す',
  bus: '共有の 2 本線（I2C）',
  busPins: 'ピン',
  speed: '速さ',
  speeds: { 100000: '100 kHz', 400000: '400 kHz' } as Record<number, string>,
  pullups: 'プルアップ',
  pullupKinds: { external: '外付け', board: '基板上' } as Record<string, string>,
  inUse: (what: string) => `（使用中: ${what}）`,
  /** §4.3: the board of a node chosen by MCU. */
  board: 'ボード',
  boardOwn: '自作',
  flashSize: 'フラッシュ容量',
  flashHelp: 'ボードの資料にある大きさ',
  boardUnset: '▲ ボード 未設定',
  kindsMore: {
    can: 'CAN（親機どうし）',
    ethernet: 'Ethernet（親機どうし）',
    'usb-cdc': 'USB',
    ble: 'BLE',
    'wifi-tcp': 'Wi-Fi（TCP）',
  } as Record<string, string>,
  /** The 親機's hop towards the PC (案). */
  uplink: '上流へのつながり',
  uplinkHost: 'つなぐ先',
  hosts: { 'pc-cli': 'パソコンの dagon', browser: 'ブラウザ（パソコン・スマートフォン）', embedded: '自作の機器' } as Record<string, string>,
  /** On a kind the firmware does not implement yet (案). */
  notYet: 'まだ使えない',
} as const;

/** SC-T: the table view (S03 words in its cells). */
export const TABLE = {
  tabs: ['ノード', 'リンク', '配線'],
  nodeCols: ['番号', '呼び名', '製品名 · MCU', '役割', 'つなぎ方', 'ピン'],
  linkCols: ['元', '先', '種類', 'アドレス', 'ピン'],
  addNode: 'ノードを加える',
  addLink: 'リンクを加える',
  edit: '編集',
  remove: '消す',
} as const;

/** SC-DN: the define mode's CLI features (S16, dev-flow §5.2's 07 and 08). */
export const CLI_NAV = {
  heading: 'CLI が要る機能',
  items: {
    link: ['01', 'CLI 連携'],
    jobs: ['04', '作業'],
    release: ['05', '署名とリリース'],
    project: ['07', 'プロジェクト'],
    flash: ['08', '書き込み'],
  } as Record<string, [string, string]>,
  running: (n: number) => `作業、${n} 件実行中`,
  none: 'CLI 未連携',
} as const;

/** Header CLI link (K0–K3). */
export const CLI_STATE = {
  none: 'CLI 未連携',
  connecting: 'CLI 接続中…',
  stopped: 'CLI 停止',
  denied: 'CLI 未連携',
  ready: (v: string) => `CLI ${v}`,
} as const;

/** SC-D4 CLI 連携 (S09, C11, §7.13). */
export const CLI_LINK = {
  steps: ['ターミナルを開く', 'dagon serve を実行する', '表示されたアドレスをこのブラウザで開く'],
  withoutCli: ['CLI なしで使える', 'グラフの編集・運用'] as const,
  deniedChip: '× 合言葉が違う',
  deniedSentence: 'ターミナルに表示されたアドレスを、そのまま開き直してください。',
  stoppedSentence: 'ターミナルで dagon serve を実行し直してください。',
  needCli: 'ターミナルで dagon serve を実行し、表示されたアドレスを開いてください。',
  reconnect: 'もう一度つなぐ',
  workspace: '作業場所',
  apiKey: 'Claude の API キー',
  key: '署名の鍵',
  yes: 'あり',
  no: 'なし',
  features: '使える機能',
  featureNames: { gen: 'プロジェクトを作る', build: 'ビルド', sign: '署名', release: 'リリースを作る', flash: '書き込む', topology_save: '構成を作業場所に保存' } as Record<string, string>,
  unsupported: '非対応',
} as const;

/** SC-D7 作業 (S35, C14). */
export const JOBS = {
  heading: '作業',
  cols: ['種類', '対象', '状態', '経過', '結果'],
  kinds: { build: 'ビルド', sign: '署名', release: 'リリース', gen: 'プロジェクト', flash: '書き込み', conform: '適合試験', ai_topology: '構成の案' } as Record<string, string>,
  states: { queued: '待ち', running: '実行中', question: '質問あり', succeeded: '成功', failed: '失敗', cancelled: '中止', unknown: '不明' } as Record<string, string>,
  none: '作業 なし',
  started: '始めた時刻',
  cancel: '中止',
  copyLog: 'ログをコピー',
  failSentence: 'ログの赤い行を確かめてから、もう一度ビルドしてください。',
  answer: 'つないだ',
} as const;

/** SC-D8 署名とリリース (S33, C15). */
export const RELEASE_STEPS = {
  heading: '署名とリリース',
  steps: ['ビルド', '署名', 'リリース', '渡す'],
  build: 'ビルド',
  sign: '署名',
  makeRelease: 'リリースを作る',
  useInOperate: 'このリリースを運用で使う',
  keyLabel: '鍵',
  sizeLabel: '大きさ',
  version: 'バージョン',
  versionHelp: '1.1.0 のように数字 3 つで書いてください。',
  name: '名前',
  buildFirst: '先にビルドしてください。',
  signFirst: '先に署名してください。',
  noKey: 'ターミナルで dagon key gen を実行してから、署名してください。',
  fixFirst: '直すと、親機へ送れます。',
  done: '✓ 完了',
} as const;

/** SC-D10 プロジェクト (dev-flow §5.2). */
export const PROJECT = {
  heading: 'プロジェクト',
  make: 'プロジェクトを作る',
  made: '✓ 生成済み',
  notMade: '● 未生成',
  boardUnset: '▲ ボード 未設定',
  writeHere: 'ここに書く',
  fixFirst: '直すと、プロジェクトを作れます。',
} as const;

/** SC-D11 書き込み (dev-flow §3.2, §5.2). */
export const FLASH = {
  heading: '書き込み',
  write: '書き込む',
  /** How each MCU is connected comes from its definition ([connection], dagon-wasm's mcu_connection). */
  via: 'つなぎ方',
  hubFirst: '親機から書き込んでください（親機が無いと子機を確かめられません）。',
  /** 案: the confirmation before serve writes the board (its job question). */
  confirm: (name: string) => `${name} に書き込みます。よろしいですか。`,
  /** 案: a family serve's flash does not write yet (ESP32-S3). */
  otherTools: 'このマイコンは、今は ESP のツールで書き込んでください。',
  yes: 'はい',
  no: 'いいえ',
  done: '✓ 書き込み済み',
  running: '● 実行中',
  notFound: '× 見つからない',
} as const;

/** §7.6: the 構成 differences of a release made for another 構成 (SC-O4, L17). */
export const DIFF = {
  heading: '構成の違い',
  cols: ['ノード・リンク', 'この構成', 'ファイル'],
  none: 'なし',
  /** The chip on the graph's differing nodes. */
  chip: '× 違い',
  spoken: '違い',
  /** No topology in the file: links and addresses were not compared. */
  linksUnchecked: 'リンク・アドレス 未確認',
} as const;

/**
 * Saving the 構成 to the project's YAML (案, §11.0 W1–W3; not yet in the
 * §11.1 table): the edits stay on the page, marked 未保存, until a save goes through.
 */
export const SAVE = {
  unsaved: '● 未保存',
  /** 422: the CLI's own check refused the 構成. */
  invalid: { chip: '× 保存できない', sentence: '問題を直してから、もう一度保存してください。' },
  /** 422 in dagon demo: an uplink the demo cannot simulate (案). */
  demoLink: {
    chip: '× デモでは使えないつながり',
    sentence: 'デモで試せる親機のつながりは USB・UART・Wi-Fi（つなぐ先はパソコンかブラウザ）です。そのどれかを選んでください。',
  },
  /** 409: a job holds the project. */
  busy: { chip: '× 作業中', sentence: '作業が終わってから、もう一度保存してください。' },
  failed: { chip: '× 保存できない', sentence: 'ターミナルで dagon serve が動いているか確かめてから、もう一度保存してください。' },
} as const;

/** 案: leaving the project while its 構成 could not be saved (the dialog). */
export const UNSAVED = {
  heading: '保存していない変更があります',
  sentence: 'もう一度保存するか、変更を捨てて進むかを選んでください。',
  save: 'もう一度保存する',
  discard: '変更を捨てて進む',
  stay: 'やめる',
} as const;

/** SC-S0 始め (S46, M35). */
export const START = {
  heading: '始め',
  new: '新しく作る',
  sample: '見本から新しく作る',
  open: '開く',
  recent: '最近のプロジェクト',
  recentNone: 'なし',
  up: '上へ',
  place: '場所',
  name: '名前',
  cantMake: '× 作れない',
  cantOpen: '× 開けない',
  loading: '読込み中…',
  /** The folder of a project (it holds dagon.toml). */
  project: '● プロジェクト',
  /**
   * Why serve did not open or make it (案, §11.0 W1–W3; not yet in the §11.3
   * table): 404 / 422 / 409 / 403. A folder that cannot be written is M35.
   */
  missing: { chip: '× 開けない', sentence: 'dagon.toml のあるフォルダを選んでください。' },
  key: { chip: '× 鍵を使えない', sentence: 'dagon serve のターミナルで鍵の理由を確かめてから、もう一度選んでください。' },
  busy: { chip: '× 作業中', sentence: '作業が終わってから、もう一度選んでください。' },
  refused: { chip: '× 開けない', sentence: 'ホームフォルダの中のフォルダを選んでください。' },
  exists: { chip: '× 作れない', sentence: 'まだ無いフォルダの名前を入れてください。' },
  /** S46: a folder with a 構成 but no dagon.toml. */
  initAsk: 'このフォルダに dagon.toml を作りますか。',
} as const;

/** `dagon demo`: the simulated desk, marked wherever the page is (案). */
export const SIM = {
  badge: '模擬',
  label: '模擬の親機と子機（実機ではありません）',
  /** Before connecting: no device list appears for the simulated 親機. */
  connect: { sentence: '模擬の親機につなぎます。機器を選ぶ一覧は出ません。' },
} as const;

/** SC-M メニュー and its one confirmation (M53). */
export const MENU = {
  button: 'メニュー',
  restart: '最初からやり直す',
  restartHeading: '最初からやり直しますか',
  restartSentence: 'このページの記録を消して、始めの画面に戻ります（元に戻せません。プロジェクトのファイルと基板の中のプログラムは変わりません）。',
  restartConfirm: '消して最初からやり直す',
  cancel: 'やめる',
} as const;
