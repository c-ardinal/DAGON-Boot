// Every visible and announced string of the page. Chips and labels use the
// "label value" / "symbol + word" form; sentences appear only where the user
// acts, one sentence each. Ids are the copy-table ids.

export const PRODUCT = 'DAGON BOOT';

export const SKIP = {
  canvas: 'キャンバスへ',
  next: '次にすることへ',
} as const;

/** Header step indicator: label, number and step name. */
export const STEP = {
  label: '手順',
  connect: { n: '01/06', name: 'つなぐ' },
} as const;

/** M48: hub connection in the header (no node counts here). */
export const HUB = {
  idle: '○ 親機 未接続',
  checking: '… 親機 確認中',
  connected: '● 親機 接続中',
  lost: '× 親機 切断',
} as const;

/** M48 accessible names ("親機、接続中"). */
export const HUB_NAME = {
  idle: '親機、未接続',
  checking: '親機、確認中',
  connected: '親機、接続中',
  lost: '親機、切断',
} as const;

export interface BandCopy {
  /** Cause chip, shown before the sentence (absent when shown elsewhere). */
  chip?: string;
  sentence: string;
  button: string;
}

/** Bottom band per connect state. */
export const BAND = {
  M03: {
    sentence: '次に出る一覧で「DAGON hub」を選び、『接続』を押してください。',
    button: 'パソコンとつなぐ',
  },
  M04: {
    chip: '× 見つからない',
    sentence: 'USB ケーブルを挿し直してから、もう一度押してください。',
    button: 'パソコンとつなぐ',
  },
  M05: {
    chip: '× ほかで使用中',
    sentence: '親機を使っているタブやアプリ（dagon CLI を含む）を閉じてから、もう一度押してください。',
    button: 'パソコンとつなぐ',
  },
  M06: {
    chip: '× 許可なし',
    sentence: '配布ページのリンクから開き直してください。',
    button: 'パソコンとつなぐ',
  },
  M08: {
    chip: '× 応答なし',
    sentence: '一覧から「DAGON hub」を選び直してください。',
    button: '選び直す',
  },
  M34: {
    sentence: 'USB ケーブルを挿し直してから、もう一度つないでください。',
    button: 'もう一度つなぐ',
  },
} as const satisfies Record<string, BandCopy>;

/** M01: browser without Web Serial. */
export const UNSUPPORTED = {
  heading: '非対応のブラウザ',
  supportLabel: '対応',
  supportValue: 'Chrome 89 以降・Edge',
  sentence: 'この URL を対応のブラウザで開いてください。',
  button: 'この URL をコピー',
} as const;

/** Node card on the canvas. */
export const NODE = {
  layerL0: 'L0 パソコン',
  layerL1: 'L1 親機',
  layerL2: 'L2 子機',
  /** M49 dot-number label and its full accessible name. */
  dotResponding: '応答',
  dotRespondingName: (n: number, of: number) => `応答 ${n}/${of}、${of} つのノードのうち ${n} つが応答`,
  pc: 'このパソコン（ブラウザ）',
  hub: '親機',
  /** S01: empty receptacle in L1. */
  hubSlot: '親機を置く場所',
  /** S04 / S38 */
  responding: '● 応答あり',
  trying: '● 試している',
  details: '詳しく見る',
} as const;

/** Visually hidden announcements (role="status" / alert). */
export const ANNOUNCE = {
  A05: '親機 接続、ノード確認中',
  A21lost: '親機 切断',
} as const;

/** Words that never appear in operating-mode guidance, chips or results. */
export const FORBIDDEN_WORDS = [
  '署名', 'ROM', 'CDC', 'スロット', 'ブートローダ', 'エラーコード', 'トポロジ',
  'マニフェスト', 'ハッシュ', 'ポート', 'シリアル', 'フラッシュ', 'ファームウェア',
  'パーティション', 'コマンド', 'タイムアウト', 'WASM', 'Web Serial', '確定', '試行中',
  '巻き戻り', 'エラー', '不正', '致命的', '無効',
] as const;
