// Every visible and announced string of the page. Chips and labels use the
// "label value" / "symbol + word" form; sentences appear only where the user
// acts, one sentence each. Comments name the copy-table ids.
import type { Action, HubState, Step } from './machine.ts';

export const PRODUCT = 'DAGON BOOT';

export const SKIP = {
  canvas: 'キャンバスへ',
  next: '次にすることへ',
} as const;

/** Header step indicator: "手順 02/06 構成を用意"; recovery shows its name only. */
export const STEP = {
  label: '手順',
  of: '/06',
  names: { 1: 'つなぐ', 2: '構成を用意', 3: '親機へ送る', 4: 'ファイル', 5: '更新', 6: '完了', recover: '復旧' } as Record<Step, string>,
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
  useLatest: '配布中の最新バージョンを使う',
  otherFile: '別のファイルを選ぶ',
  resendFileTopology: 'ファイルの構成で送り直す',
  update: '更新',
  reconnect: 'もう一度つなぐ',
  recoverAgain: 'もう一度直す',
  copyLog: '記録をコピー',
  retrySample: 'もう一度読み込む',
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
  M06: { chip: '× 許可なし', sentence: '配布ページのリンクから開き直してください。' },
  M08: { chip: '× 応答なし', sentence: '一覧から「DAGON hub」を選び直してください。' },
  M10: { sentence: '机の上と同じか確かめてから、親機へ送ってください。' },
  M12: { chip: '× 送れない', sentence: 'USB ケーブルを確かめてから、もう一度送ってください。' },
  M13: { sentence: '更新ファイルをこの画面に置くか、選んでください。' },
  /** M14 / M15 / M28: label-value pair, no sentence. */
  M14: { pair: ['目安', '約 1 分'] },
  M18: { sentence: '何も書いていないので、配布ページから正しいファイルを取り直してください。' },
  M18b: { sentence: '何も書いていないので、別のファイルを選んでください。' },
  M19: { sentence: '何も書いていないので、別のファイルを選んでください。' },
  M20: { sentence: '何も書いていないので、この構成向けのファイルを選んでください。' },
  M22: { sentence: '終わるまで電源と USB を抜かず、このページを開いたままにしてください（止まっても前のバージョンのまま動きます）。' },
  M33: { sentence: '基板どうしの配線が抜けていないか確かめてから、もう一度直してください。' },
  M34: { sentence: 'USB ケーブルを挿し直してから、もう一度つないでください。' },
  M35: { chip: '× 見本を読めない', sentence: 'インターネットにつながっているか確かめてから、もう一度読み込んでください。' },
  /** M57: part of the page (dagon-wasm) did not load. */
  M57: { chip: '× 読み込めない', sentence: 'このページの一部を読み込めませんでした。ページを開き直してください。' },
} as const satisfies Record<string, BandCopy>;

/** M29: beside the recover button when nodes after the stopped one are not written yet. */
export const BAND_FOR = {
  M29: (stuck: string) => ({ pair: ['目安', '約 1 分'] as const, sentence: `${stuck}を直すと、残りのノードを更新できます。` }),
} as const;

/** M01: browser without Web Serial. */
export const UNSUPPORTED = {
  heading: '非対応のブラウザ',
  supportLabel: '対応',
  supportValue: 'Chrome 89 以降・Edge',
  sentence: 'この URL を対応のブラウザで開いてください。',
  button: 'この URL をコピー',
} as const;

/** Canvas: layers, empty receptacles, dot numbers. */
export const CANVAS = {
  layerL0: 'L0 パソコン',
  layerL1: 'L1 親機',
  layerL2: 'L2 子機',
  pc: 'このパソコン（ブラウザ）',
  /** S01 */
  hubSlot: '親機を置く場所',
  nodeSlot: '子機を置く場所',
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
  mcu: 'MCU',
  /** The hub's own connection line. */
  usb: 'USB（パソコン）',
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
} as const;

/** Link chip (S18) and its spoken name. */
export const LINK = {
  bundle: (kind: string, n: number, shared: number) => `${kind} · ${n} 本${shared ? `（うち共有 ${shared}）` : ''}`,
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
  introSteps: ['構成を用意（見本か自動で見つける）', '親機へ送る', '更新'],
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

/** SC-O7: the hub reconnect dialog (M26, M27). */
export const RECONNECT = {
  heading: '親機の再接続',
  sentence: '『もう一度つなぐ』を押して、同じ機器を選んでください。',
  auto: '自動の再接続',
  left: (s: number) => `あと ${s} s`,
  over: '終了',
} as const;

/** Help in the header, and the operating-mode key list (S06). */
export const HELP = {
  heading: '困ったとき',
  rows: [
    ['一覧に何も出ない', 'データ用のケーブルに替えてください（充電専用では出ません）。'],
    ['「DAGON hub」が無い', '親機の画面が点いているか確かめてください。'],
    ['何を置けばいいか分からない', '『つないで、自動で見つける』を押してください。'],
    ['手元に無い', '『配布中の最新バージョンを使う』を押してください。'],
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
  A10ok: (n: number) => `配布元 確認済み、構成 一致、対象 ${n}`,
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
  /** "配布中の最新バージョンを使う" could not fetch the published release. */
  A22latest: '配布中の最新バージョンを読み込めませんでした',
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
};

/** Names a §11.2 sentence needs: the node (呼び名), its board, its id, and whether there are too many nodes. */
export interface FixWords {
  to: string;
  board: string;
  id: string;
  many: boolean;
}

/** §11.2: how to fix each rule, one sentence. */
export const RULE_FIX: Record<string, (w: FixWords) => string> = {
  'V-01': () => 'dagon の新しいバージョンで書き出したファイルを読み込んでください。',
  'V-02': (w) => (w.many ? 'ノードを 8 つまでにしてください。' : `名前「${w.id}」の 2 つのどちらかを変えてください。`),
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
};

/** Header: the two modes (運用｜定義) and whether the configuration was sent (S24). */
export const MODE = {
  operate: '運用',
  define: '定義',
  label: 'モード',
  unsent: '構成 未送信',
  sent: '構成 送信済み',
} as const;

/** Define mode (SC-D1–D3, SC-T, SC-M, §11.1). */
export const DEFINE = {
  /** SC-D1 left column. */
  listHeading: 'ノードリスト',
  listHelp: '選んで Enter で置けます。',
  place: '置く',
  placed: '配置済み',
  firstHeading: 'はじめての方へ',
  placeSample: '見本を置く',
  connectDiscover: 'つないで、自動で見つける',
  /** Canvas toolbar. */
  undo: '元に戻す',
  redo: 'やり直す',
  graph: 'グラフ',
  table: '表',
  exportYaml: '書き出す',
  importYaml: '読み込む',
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
  fix: '直す',
  ruleId: '規則 ID',
  yamlPath: 'YAML の場所',
  details: '詳しく見る',
  problemName: (short: string, rule: string, sentence: string) => `${short}、規則 ${rule}、${sentence}`,
  /** Bottom band (C02, C05, C06, C09). */
  emptySentence: '親機を USB でパソコンにつないでから、『つないで、自動で見つける』を押してください。',
  fixSentence: '直すと、親機へ送れます。',
  sendSentence: '親機へ送ると、更新できるようになります。',
  send: '親機へ送る',
  connectSend: 'つないで、親機へ送る',
  unreadableChip: '× 読めない形式',
  unreadableSentence: 'dagon で書き出した .yaml ファイルを選んでください。',
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
