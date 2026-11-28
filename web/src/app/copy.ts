// Every visible and announced string of the page. Chips and labels use the
// "label value" / "symbol + word" form and end in a noun; explanations end in
// 〜します, requests to the user in 〜してください; sentences appear only where
// the user acts, one sentence each.
// One file on purpose: the wording rules above are checked across all of it at once, so it is not split by screen.
import type { Action, HubState, Step } from './machine.ts';

export const PRODUCT = 'DAGON Console';

export const SKIP = {
  canvas: 'キャンバスへ',
  next: '後続の手順へ',
} as const;

/**
 * Header step indicator in three steps: ① 接続 ② ファイル ③ 書込.
 * The operate machine's six steps fold into them (STEP.of3); recovery shows its name only.
 */
export const STEP = {
  label: '手順',
  of: '/03',
  names: { 1: '接続', 2: 'ファイル', 3: '書込', recover: '書込' } as Record<1 | 2 | 3 | 'recover', string>,
  of3: (s: Step): 1 | 2 | 3 | 'recover' => (s === 'recover' ? s : s <= 3 ? 1 : s === 4 ? 2 : 3),
} as const;

/** The connection to the node connected to the PC, in the header (no node counts here). */
export const HUB = {
  idle: '○ 未接続',
  checking: '… PC接続ノード 確認中',
  connected: '● 接続済',
  reconnecting: '… PC接続ノード 再接続中',
  lost: '× PC接続ノード 切断',
  cli: '● CLIがPC接続ノードを使用中',
} as const satisfies Record<HubState, string>;

/** Bottom-band buttons (fixed names); per-node names are in BUTTON_FOR. */
export const BUTTON = {
  connectDiscover: 'PC接続ノードの探索',
  connect: '接続',
  reselect: 'PC接続ノードの再選択',
  reload: '構成の再読込',
  defineFix: '準備での構成修正',
  chooseFile: 'パッケージファイルの選択',
  useLatest: '最新リリースの使用',
  otherFile: '別ファイルの選択',
  update: 'ノードへの書込',
  reconnect: '再接続',
  recoverAgain: '再試行',
} as const satisfies Record<Exclude<Action, 'recover' | 'updateResponding'>, string>;

/** Buttons named after a node or a count. */
export const BUTTON_FOR = {
  recover: (name: string) => `${name}への書込`,
  updateResponding: (n: number) => `応答する${n}ノードへの優先書込`,
  /** The update after a recovery, over the nodes not written yet. */
  updateRest: '未書込ノードへの書込',
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
  choose: { sentence: '表示される一覧で「DAGON hub」を選択し、『接続』をクリックしてください。' },
  notFound: { chip: '× 未検出', sentence: 'USBケーブルの再挿入後に再度クリックしてください。' },
  portBusy: { chip: '× 使用中', sentence: 'PC接続ノードを使用中のタブやアプリ（dagon CLIも対象）を終了後に再度クリックしてください。' },
  denied: { chip: '× 許可なし', sentence: 'ターミナルに表示されたアドレスへ再アクセスしてください。' },
  noResponse: { chip: '× 応答なし', sentence: '一覧で「DAGON hub」を再選択してください。' },
  /** The node connected to the PC answered HELLO in another wire version. */
  wireVersion: { chip: '× 通信形式のバージョン不一致', sentence: 'PC接続ノードとこの画面の通信形式のバージョンを一致させてください。' },
  ready: { sentence: 'パッケージファイルをこの画面へドロップするか選択してください。' },
  /** ready with a build to write: the main action writes it. */
  readyBuilt: { sentence: '書込対象のノードをチェックし、『書込』をクリックしてください。' },
  /** Label-value pair, no sentence. */
  estimate: { pair: ['推定', '約1分'] },
  sourceRefused: { sentence: '書込は未実行のため、準備で再ビルドするか適正なファイルを選択してください。' },
  keyRefused: { sentence: '書込は未実行のため、別のファイルを選択してください。' },
  formatRefused: { sentence: '書込は未実行のため、別のファイルを選択してください。' },
  topologyRefused: { sentence: '書込は未実行のため、この構成向けのファイルを選択してください。' },
  writing: { sentence: '完了まで電源とUSBを維持し、このページの表示を継続してください。途中で停止しても、ノードは再書込が可能な状態で動作します。' },
  recoverFailed: { sentence: '基板間の配線の欠落を確認後に再試行してください。' },
  lost: { sentence: 'USBケーブルの再挿入後に再接続してください。' },
  folderFailed: { chip: '× 作成失敗', sentence: 'そのフォルダへの保存が可能か確認後に再選択してください。' },
  /** "最新リリースの使用" found no release in the project. */
  noRelease: { chip: '× ビルドなし', sentence: '準備でビルドするかパッケージファイルを選択してください。' },
  /** Part of the page (dagon-wasm) did not load. */
  pageLoadFailed: { chip: '× 読込失敗', sentence: 'このページの一部の読込に失敗しました。ページを再読込してください。' },
  /** reconnectNeeded (the dialog's sentence, and the band's once the dialog is closed). */
  reconnectNeeded: { sentence: '『再接続』をクリックし、同一の機器を選択してください。' },
  checking: { sentence: '完了まで数秒間待機してください。' },
  scanning: { sentence: '検出したノードの表示まで待機してください。' },
  /**
   * The answering nodes are updated first, a silent one from its card:
   * nodesSilent, with a power and wiring hint for the silent ones.
   */
  nodesSilent: { sentence: '応答なしのノードは電源と配線を確認し、応答するノードへ優先して書込を実行してください。' },
  /** nodesSilent when the answering nodes need nothing more (already updated, or after 「応答するnノードへの優先書込」). */
  silentLeft: { pair: ['推定', '約1分'], sentence: 'マーク表示のノードは書込後に正常に動作します。' },
  verifying: { sentence: '確認の完了まで待機してください。' },
  hubRestarting: { sentence: 'PC接続ノードの再起動後の自動再接続まで待機してください。' },
  stopped: { pair: ['推定', '約1分'], sentence: '停止したノードは書込後に新バージョンで動作します。' },
  doneAll: { sentence: '全ノードへの書込が完了しました。' },
  doneChecked: { sentence: 'チェックしたノードへの書込が完了しました。' },
  doneNone: { sentence: 'チェックしたノードはこのパッケージと同一内容のため、書込を省略しました。' },
  cliTurn: { sentence: 'CLIの作業完了後に自動で再接続します。' },
} as const satisfies Record<string, BandCopy>;

/**
 * The header's 接続 / 切断 and why it cannot be
 * used in a state (the band says it when it is pressed then).
 */
export const HUB_TOGGLE = {
  connect: '接続',
  disconnect: '切断',
  locked: {
    choosing: '一覧での選択完了まで待機してください。',
    scanning: '探索の完了まで切断は不可です。',
    writing: '書込の完了まで切断は不可です。',
    hubRestarting: 'PC接続ノードとの再接続まで切断は不可です。',
    reconnectNeeded: 'PC接続ノードとの再接続まで切断は不可です。',
    recovering: '書込の完了まで切断は不可です。',
    cliTurn: 'CLIの作業完了まで切断は不可です。',
  } as Partial<Record<string, string>>,
} as const;

/** Elapsed and remaining time under the dot number while updating or recovering. */
export const TIME = {
  elapsed: '経過',
  left: '残時間',
  soon: '完了間近',
  /** m:ss, or h:mm:ss past an hour. */
  clock: (ms: number) => {
    const s = Math.max(0, Math.floor(ms / 1000));
    const two = (n: number) => String(n).padStart(2, '0');
    return s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${Math.floor(s / 60)}:${two(s % 60)}`;
  },
  about: (ms: number) => (ms < 30_000 ? '完了間近' : `約${Math.ceil(ms / 60_000)}分`),
} as const;

/** lost's cause chip when the device the CLI gave back was not this PC-connected node. */
export const BAND_CHIP = {
  otherHub: '× 別のコーディネータ',
  notHub: '× コーディネータ以外',
  silent: '× 応答なし',
} as const;

/** Beside the recover button when nodes after the stopped one are not written yet. */
export const BAND_FOR = {
  stoppedAt: (stuck: string) => ({ pair: ['推定', '約1分'] as const, sentence: `${stuck}の修正後に未書込ノードへの書込が可能です。` }),
  /** done when doneAll is not true: the nodes that did not answer were not updated. */
  doneSome: (n: number) => ({ sentence: `応答なしの${n}ノードは旧バージョンのままのため、電源と配線の確認後に書込を実行してください。` }),
  /** done when dagon-host sent nothing to nodes that were never reached (needsFirstWrite): their first write goes from their card. */
  doneFirst: (n: number) => ({ sentence: `${SILENCE.neverReached.word}の${n}ノードへの送信はありません。各ノードの「${WRITE.run}」で最初の書込を実行してください。` }),
  /** done after 「前のバージョンへの復元」. */
  /** done when some nodes were already on the package. */
  donePart: (wrote: number, same: number) => ({ sentence: `${wrote}ノードへの書込が完了し、同一内容の${same}ノードは書込を省略しました。` }),
  doneBack: (name: string, version: string) => ({ sentence: `${name}は前のバージョン${version}で動作中です。` }),
  /** done after a recovery with nothing left to update. */
  doneFixed: (name: string, version: string) => ({ sentence: `${name}は復旧して${version}で動作中です。` }),
} as const;

/** Browser without Web Serial. */
export const UNSUPPORTED = {
  heading: '非対応のブラウザ',
  supportLabel: '対応',
  supportValue: 'Chrome 89以降・Edge',
  sentence: 'このURLを対応ブラウザで表示してください。',
  button: 'このURLのコピー',
} as const;

/**
 * The state-lock table: each state's
 * short name for the status line and, per operation it stops, the reason
 * shown when that operation is pressed (information style, never red).
 */
export const LOCK = {
  controls: {
    edit: '構成の編集',
    gen: 'テンプレート生成',
    build: 'ビルド',
    flash: '書込',
    release: 'リリース',
    update: '書込',
    recover: '書込',
    rollback: '前のバージョンへの復元',
    connect: '接続・切断',
    project: 'プロジェクトの切替',
    open: 'プロジェクトのオープン',
    ask: 'DAGONへの質問',
    apply: 'DAGONの案の反映',
    link: 'DAGONとの連携',
    file: 'パッケージファイルの配置',
  },
  idle: '待機',
  stateLabel: '現在の状態',
  stateTitle: '現在の状態。使用不可の操作はクリックで理由を表示します',
  states: {
    demo: {
      name: 'デモ再生中',
      why: {
        link: 'デモを再生中です',
        edit: 'デモを再生中です',
        gen: 'デモを再生中です',
        build: 'デモを再生中です',
        flash: 'デモを再生中です',
        release: 'デモを再生中です',
        update: 'デモを再生中です',
        recover: 'デモを再生中です',
        rollback: 'デモを再生中です',
        connect: 'デモを再生中です',
        project: 'デモを再生中です',
        apply: 'デモを再生中です',
        file: 'デモを再生中です',
      },
    },
    updating: {
      name: '書込中',
      why: {
        edit: '送信した構成と不一致になるため、書込中は構成の変更は不可です',
        gen: '書込の完了まで使用不可です',
        build: '書込の完了まで使用不可です',
        flash: '書込でPC接続ノードを使用中のため書込は不可です',
        release: '書込の完了まで使用不可です',
        update: '書込中です。完了まで待機してください',
        recover: '書込の完了後に使用可能です',
        rollback: '書込の完了後に使用可能です',
        connect: '書込中に切断すると、書込途中のノードは再書込が可能な状態で残存します。完了まで待機してください',
        project: '書込の完了まで使用不可です',
        apply: '書込中は構成の変更は不可です',
        file: '書込の完了まで使用不可です',
      },
    },
    recovering: {
      name: '書込中',
      why: {
        edit: '書込中は構成の変更は不可です',
        flash: 'PC接続ノードを使用中のため書込は不可です',
        update: '書込の完了後に使用可能です',
        recover: '書込中です',
        rollback: '書込の完了後に使用可能です',
        connect: '書込中に切断すると書込が停止します',
        project: '書込の完了まで使用不可です',
        apply: '書込中は構成の変更は不可です',
        file: '書込の完了まで使用不可です',
      },
    },
    flashing: {
      name: '書込中',
      why: {
        edit: '書込中の構成と不一致になるため、書込中は構成の変更は不可です',
        gen: '書込の完了まで使用不可です',
        build: '書込の完了まで使用不可です',
        flash: '書込中です',
        release: '書込の完了まで使用不可です',
        update: '書込の完了後に使用可能です',
        recover: '書込の完了後に使用可能です',
        rollback: '書込の完了後に使用可能です',
        connect: '書込でUSBを使用中のため切断は不可です',
        project: '書込の完了まで使用不可です',
        apply: '書込の完了まで使用不可です',
      },
    },
    building: {
      name: 'ビルド中',
      why: {
        edit: 'ビルドで構成を使用中のため、ビルド中は構成の変更は不可です',
        gen: 'ビルドの完了後に使用可能です',
        build: 'ビルド中です',
        flash: 'ビルドの完了後に使用可能です',
        release: 'ビルドの完了後に使用可能です',
        update: 'ビルドの完了後に使用可能です',
        project: 'ビルドの完了まで使用不可です',
        apply: 'ビルド中は構成の変更は不可です',
      },
    },
    unsaved: {
      name: '保存中',
      why: {
        gen: '構成を保存中です。約0.5秒で完了します',
        build: '構成を保存中です。約0.5秒で完了します',
        flash: '構成を保存中です',
        release: '構成を保存中です',
        update: '保存した構成で書込を実行するため、保存の完了まで待機してください',
        project: '保存の完了後に切替が可能です',
      },
    },
    connecting: {
      name: '接続中',
      why: {
        connect: 'PC接続ノードと接続中です',
        update: 'PC接続ノードと接続中です',
        recover: 'PC接続ノードと接続中です',
        rollback: 'PC接続ノードと接続中です',
        file: 'PC接続ノードと接続中です',
      },
    },
    disconnected: {
      name: '未接続',
      why: {
        update: 'PC接続ノードと未接続です。「接続」をクリックしてください',
        recover: 'PC接続ノードと未接続です',
        rollback: 'PC接続ノードと未接続です',
      },
    },
    thinking: {
      name: 'DAGONの回答待機',
      why: {
        ask: 'DAGONの回答を待機中です',
        apply: 'DAGONの回答を待機中です',
      },
    },
    /** The public demo page: only the simulator in this browser; the rest needs dagon serve on the user's PC. */
    site: {
      name: 'デモ',
      why: {
        gen: 'このデモはファイルを生成しません。ローカルのPCでdagon serveを起動すると使用可能です',
        build: 'このデモはビルドしません。ローカルのPCでdagon serveを起動すると使用可能です',
        flash: 'このデモは実機に接続しません。ローカルのPCでdagon serveを起動すると使用可能です',
        release: 'このデモはリリースを作成しません。ローカルのPCでdagon serveを起動すると使用可能です',
        open: 'このデモはファイルのオープンと保存をしません。ローカルのPCでdagon serveを起動すると使用可能です',
        ask: 'このデモはAIに接続しません。ローカルのPCでdagon serveを起動すると使用可能です',
        apply: 'このデモはAIに接続しません。ローカルのPCでdagon serveを起動すると使用可能です',
        link: 'このデモはキーを管理しません。ローカルのPCでdagon serveを起動すると使用可能です',
        file: 'このデモはファイルの読込をしません。用意したパッケージファイルで書込が可能です',
      },
    },
    cliNone: {
      name: 'CLI未接続',
      why: {
        link: 'キーはCLI側で保管するため、事前にターミナルでdagon serveを起動してください',
        gen: 'CLIと未接続です。ターミナルでdagon serveを起動してください',
        build: 'CLIと未接続です',
        flash: 'CLIと未接続です',
        release: 'CLIと未接続です',
        ask: 'DAGONはCLI経由で通信するため、CLIと未接続の状態では使用不可です',
      },
    },
  },
} as const;

/**
 * DAGON, the page's agent. The character and
 * every UI word say DAGON; the provider's name comes from serve and is shown
 * only in the linking step.
 */
export const AGENT = {
  name: 'DAGON',
  menuLink: 'DAGONとの連携',
  menuTalk: 'DAGONとの対話',
  menuUnlink: 'DAGONとの連携解除',
  link: {
    heading: 'DAGONとの連携',
    intro: 'DAGONは連携したエージェントに構成について照会して回答します。送信への同意はこの連携時に1回だけ取得します。連携後の質問は確認なしで送信します。',
    agent: '送信先',
    agentNote: 'ローカルのdagon serve経由で送信します',
    way: '接続方法',
    /** What each kind of way needs on this machine (the provider is named only here, at linking). */
    ready: '準備',
    wayNote: {
      command: 'このパソコンでログインしたエージェントのプログラムを使用します。ログイン情報はこのページと送信文のどちらにも格納しません',
      'messages-http': 'APIキーを「CLI連携」で登録するか、ターミナルでdagon ai key setを実行してください。キーはOSの資格情報ストアに格納し、このページと送信文のどちらにも格納しません',
    } as Record<string, string>,
    wayMissing: 'この接続方法はこのパソコンでは使用不可です。連携の事前完了は可能です。',
    sends: '送信対象',
    neverSends: '送信対象外',
    undo: '連携はメニューの「DAGONとの連携解除」でいつでも解除が可能です。解除時に同意も取消します。',
    agree: '同意と連携',
    cancel: '連携の中止',
  },
  linked: (provider: string, at: string) => `DAGONと連携。送信先（${provider}）と送信内容への同意を記録（${at}）`,
  unlinked: 'DAGONとの連携を解除し、同意を取消。APIキーは「CLI連携」で削除が可能',
  demoBubble: 'デモでは会話は不可です。dagon serveで連携すると相談が可能です。',
  hello: '選択したノードやリンクについて質問が可能です。/キーで入力欄へ移動します。',
  thinking: '回答を待機中…',
  placeholder: '質問を入力（/キーでここへ移動）',
  send: '質問の送信',
  sendWhy: '質問の入力後に送信が可能です',
  questions: [
    ['problem', '問題の質問'],
    ['next', '後続手順の質問'],
    ['propose', '変更案の依頼'],
    ['failure', '失敗理由の質問'],
  ] as const,
  showDiff: '差分の表示',
  apply: '案の構成への反映',
  applied: '反映済。「取消」（Ctrl+Z）で復元が可能',
  allTalk: '全会話の表示（下部の「会話」タブ）',
  diffCaption: '変更案（反映後も「取消」で復元が可能です）',
  talkNone: '会話はありません。DAGONの会話ウィンドウか/キーで質問してください。',
  you: 'あなた',
  markOpen: 'クリックで展開',
  markClose: 'クリックで非表示',
  question: 'DAGONへの質問',
  errors: {
    AGENT_NOT_LINKED: { text: 'DAGONと未連携です。メニューの「DAGONとの連携」で連携してください。', failed: false },
    AGENT_UNAVAILABLE: { text: '連携した方法がこのパソコンで使用不可です。連携手順に記載の準備（プログラムのインストール、APIキーの登録）を実施してください', failed: false },
    /** The OS credential store cannot be used: the text is serve's reason (what is missing and how to start one). */
    AGENT_KEY_STORE: { label: '× 保存不可', text: 'OSの資格情報ストアが使用不可のため、APIキーの保存は不可です', failed: true },
    KEY_REFUSED: { label: '× 受付不可', text: 'APIキーは空白なしの英数字と記号の最大1024文字です', failed: true },
    AGENT_BUSY: { text: 'DAGONは別の質問に回答中です。一定時間の待機後に再度質問してください', failed: false },
    AGENT_TIMEOUT: { text: '× 回答が時間内に未到達です。再度質問してください', failed: true },
    AGENT_FAILED: { text: '× 回答の受信に失敗しました。再度質問してください', failed: true },
    DEMO: { text: 'デモではDAGONとの連携は不可です', failed: false },
    OLD_CLI: { text: 'このCLIはDAGONに非対応です。最新のdagonでdagon serveを再起動してください', failed: false },
    AGENT_CONSENT_VERSION: {
      label: '× 利用規約の確認',
      text: '利用規約が更新されています。連携のために確認してください。',
      failed: true,
    },
    AGENT_CONTEXT_TOO_LARGE: {
      label: '× 文字数超過',
      text: '文字数の超過で送信は不可です',
      failed: true,
    },
  },
} as const;

/** The bottom drawer and its tabs. */
/** The drawer's timeline of an update. */
export const TIMELINE = {
  label: '書込のタイムライン',
  time: '時間',
  none: '書込の開始後にノードごとの段階をここに時系列で表示します。',
} as const;

/** The status row. */
export const STATUS = {
  label: '状態',
  key: 'キー',
  keyTitle: 'リリース検証用のキーのID',
  progress: '進捗',
  saving: '保存中…',
  saved: '保存済',
} as const;

/** The canvas controls bottom-right. */
export const ZOOM = {
  label: '表示の操作',
  out: 'キャンバスの縮小',
  fit: 'キャンバス全体の表示',
  in: 'キャンバスの拡大',
  maxWhy: '最大の倍率です',
  minWhy: '最小の倍率です',
} as const;

/** The parts window. */
export const PARTS = {
  heading: '部品',
  hint: 'ドラッグで配置',
  close: '部品の非表示',
  open: '部品の表示',
} as const;

/** The floating sheet's spoken name, and its close button. */
export const SHEET = '選択対象の詳細';
export const SHEET_CLOSE = 'シートの非表示';

export const DRAWER = {
  label: '下部ドロワー',
  problems: '問題',
  log: '記録',
  talk: '会話',
  logNone: '記録はありません。',
  close: 'パネルの非表示',
  jobs: '作業',
  output: '出力',
  timeline: 'タイムライン',
  outputNone: '出力はありません。',
  /** The handle on the open drawer's top edge. */
  resize: 'パネルのサイズ変更（ドラッグ）',
  /** The button beside the tabs that sets the height without a drag. */
  raise: 'パネルの最大化',
  lower: 'パネルの最小化',
} as const;

/** Canvas: uplink cards, the empty canvas, dot numbers. */
export const CANVAS = {
  /** Uplink card of the coordinator this page is connected to now. */
  pc: 'このパソコン（ブラウザ）',
  /** Uplink card by each coordinator's uplink host as the 構成 writes it (none written: dagon on a PC). */
  pcBy: { 'pc-cli': 'パソコン（dagon）', browser: 'ブラウザ（パソコン・スマホ）', embedded: '組込機器' } as Record<string, string>,
  /** The empty canvas. */
  empty: 'PC接続ノードをUSBでパソコンに接続後に、『PC接続ノードの探索』をクリックしてください。',
  /** After the uplink card of a coordinator this page is not connected to. */
  notThisPc: '未接続',
  /** Canvas description for screen readers. */
  describe: '矢印キーでノード間を移動し、Enterで選択します。他のキーは「トラブル対処」の「キー操作」に記載しています。',
  /** Dot-number labels. */
  dotResponding: '応答',
  dotFound: '発見',
  /** Dot number during an update, and after it stopped. */
  dotProgress: '全ノードの進捗',
  dotPercent: '%',
  dotStopped: '%停止',
  dotProgressName: (n: number, stopped: boolean) => `全ノードの進捗${n}%${stopped ? '、停止' : ''}`,
  dotRespondingName: (n: number, of: number) => `応答${n}/${of}、${of}ノード中${n}ノードが応答`,
  dotFoundName: (n: number) => `発見${n}`,
  /** The link to the wiring table. */
  wiringLink: '配線表の表示',
  /** Over the canvas after the menu's move item, until the click that places the card. */
  moveHint: (name: string) => `${name}の移動先をクリックしてください（Escで中止）。`,
} as const;

/** Node card. */
export const NODE = {
  /** The role on the card: the coordinator, then the links between a node and the nearest coordinator its links lead up to. */
  roleHub: 'コーディネータ',
  roleNode: 'コーディネータから1段目',
  /** Two links from the nearest coordinator, and the rows below that. */
  roleGrandchild: 'コーディネータから2段目',
  roleDeeper: (d: number) => `コーディネータから${d}段目`,
  /** A node whose links lead up to no coordinator (none above it, or round a loop), and a node named without its place. */
  roleOther: 'ノード',
  mcu: 'MCU',
  /** The coordinator's own connection line: its uplink kind and what drives it. */
  usb: 'USB（パソコン）',
  uplinkVia: (kind: string, host: string) =>
    `${kind}（${({ 'pc-cli': 'パソコン', browser: 'ブラウザ', embedded: '組込機器' } as Record<string, string>)[host] ?? 'パソコン'}）`,
  /** Pin headings and the no-wire value. */
  reset: 'リセット',
  boot: '起動',
  noWire: 'なし（本体側で切替）',
  /** Node state chips. */
  responding: '● 応答あり',
  silent: '× 応答なし',
  trial: '● 確認待機',
  /** Node state chips during an update. */
  writing: '▼▼ 書込中',
  waiting: '… 待機',
  done: '✓ 完了',
  unwritten: '… 未書込',
  unchanged: '✓ 書込不要',
  rebooting: '… 再起動中',
  previous: '↺ 前のバージョン',
  /**
   * A node that answered and refused the update, by dagon-host's reason (the
   * last-reason words of the detail where they exist). No answer stays
   * `silent`; a node back on its previous version is `previous`.
   */
  refused: {
    E_SIGNATURE: '× 配布元不明',
    E_KEY: '× 配布元不明',
    E_DIGEST: '× 破損',
    E_COMPAT: '× 機種不一致',
    E_FLASH: '× 書込失敗',
    // dagon-host's release check, in the words of the update file's verdict.
    source: '× 配布元不明',
    key: '× 配布元不明',
    topology: '× 構成不一致',
    format: '× 形式不一致',
  } as Record<string, string>,
  refusedOther: '× 受付拒否',
  /** Node state chips during a recovery. */
  lastResort: '◎ 最後の手段',
  cannotFix: '× 書込不可',
  /** Full spoken state of a chip. */
  respondingName: '応答あり',
  silentName: '応答なし、2回連続で応答なし',
  trialName: '確認待機',
  /**
   * Spoken name of a node: the card's visible words in their order, nothing put between them
   * (WCAG 2.5.3: the visible label is a run of the name), then what only the marks show.
   */
  name: (p: { no: number; name: string; role: string; version: string; product: string; meta: string; slots: string; chip: string; note: string; state: string }) =>
    [`[${p.no}] ${p.name}`, p.role, p.version, p.product, p.meta, p.slots, p.chip, p.note, p.state].filter(Boolean).join('、'),
  /** A node updating skips: a calm note, not a fault. */
  skipped: '◇ 書込不可',
  skippedWhy: {
    'second-hub': '2つ目のコーディネータの配下のため',
    deep: 'コーディネータとの間に複数のリンクがあるため',
    'link-kind': 'この接続の種類では書込が到達しないため',
    unreachable: 'どのコーディネータからもリンクの経路がないため',
    'cut-invalid': '書込の到達範囲のみでは検証に不合格のため（dagon topo checkで理由を確認可能）',
  } as Record<string, string>,
  skippedName: (why: string) => `書込不可、${why}`,
} as const;

/** Link chip and its spoken name. */
export const LINK = {
  /** CAN and Ethernet carry no wires the page draws: the kind alone. */
  /** The bus, the value it needs (address or speed: ), its wires. */
  bundle: (kind: string, n: number, shared: number, value = '') => {
    const head = value ? `${kind} ${value}` : kind;
    return n ? `${head} · ${n}本${shared ? `（うち共有${shared}）` : ''}` : head;
  },
  /** The lower connector of a card, the handle a link is dragged from. */
  portOut: (name: string) => `${name}の下端の接続口（ドラッグで接続先のノードへ接続）`,
  name: (label: string, from: string, to: string, addr: string) => `${label}、${from}から${to}へのリンク${addr ? `、${addr}` : ''}`,
} as const;

/** Why a link drag is refused (a failure: red). */
export const LINK_WHY = {
  pair: 'このノードとは接続済です',
} as const;

/** Right column. */
export const DETAIL = {
  /** Nothing selected. */
  nothingSelected: 'ノードの選択時にここに詳細を表示します。',
  /** The first steps, before anything is selected. */
  introHeading: '最初の手順',
  /** The same three steps as the header. */
  introSteps: ['接続', 'ファイル', '書込'],
  introNote: '停止しても再書込が可能',
  /** Link figures of the node. */
  rtt: '往復',
  lastSeen: '最終応答',
  attempts: '試行回数',
  lastReason: '最終復帰理由',
  ms: (n: number) => `${n} ms`,
  none: 'なし',
  /** A version the node did not tell (VERSION left it out, or the node predates VERSION). */
  unknown: '不明',
  /** Values of the last-reason code (STATUS last_reason). */
  reasons: { 1: '内容の破損', 2: '配布元不明', 3: '起動失敗', 5: '起動可能なバージョンなし' } as Record<number, string>,
  details: '詳細表示',
} as const;

/** A node on trial that this page did not apply (the detail, above its buttons). */
export const TRIAL_CHOICE = {
  sentence: 'このバージョンを維持するか、前のバージョンがある場合はそのバージョンへ復元するかを選択してください。',
  keep: 'このバージョンの維持',
  back: '前のバージョンへの復元',
} as const;

/** The detail's actions and the recovery figures under 詳細表示. */
export const NODE_ACTION = {
  /** Going back to the previous version, shown when the other slot holds a version to go back to. */
  back: (version: string) => `前のバージョン（${version}）への復元`,
  /** 詳細表示: how the last recovery got through. */
  path: '経路',
  paths: { retransfer: '再送信', factory: '予備への送信', rom: '最後の手段' },
  romRtt: '往復の最大値',
  romAttempts: '再試行',
} as const;

/** Slot section. Its rows are labelled by STATUS entry (A / B / 予備); a node shows the ones its MCU definition gives. */
export const SLOT = {
  heading: 'スロット',
  /** The boot loader's own slots, labelled by STATUS entry (dagon_proto VERSION's boot core slots). */
  coreHeading: 'ブートローダのスロット',
  rows: ['A', 'B', '予備'],
  coreRows: ['A', 'B'],
  /** Slot state u8 → word, in general words: what the slot holds now. */
  states: ['空', '書込済', '次回起動', '確認待機', '確認済', 'ロールバック済'],
  inUse: '現在動作中',
  previous: '前のバージョン',
  /** The watch's factory slot holds an image. */
  present: 'あり',
} as const;

/** Wiring table. */
export const WIRING = {
  heading: '配線表',
  cols: ['信号', 'PC側のノード', '接続先のノード', '共有'],
  shared: '共有',
  own: '—',
  ownName: 'このノード専用',
  legendHeading: '表の凡例',
  legend: [
    ['共有', 'SDA・SCLはI2Cの接続先の全ノード、GNDは全ノードで共通の配線'],
    ['—', 'このノード専用の配線'],
  ],
  copy: '配線表のコピー',
} as const;

/** The file check: the file and dagon-host's verdict on it (the one place for the verdict). */
export const FILE = {
  heading: '書込',
  /** File input label (the drop place). */
  choose: 'パッケージファイルの選択',
  size: (bytes: number) => `${Math.max(1, Math.round(bytes / 1024))} KB`,
  sourceBad: '× 配布元不明',
  topologyBad: '× 構成不一致',
  notRelease: '× 形式不一致',
} as const;

/** The reconnect dialog of the node connected to the PC, and the countdown beside it while the page reconnects by itself (hubRestarting). */
export const RECONNECT = {
  heading: 'PC接続ノードの再接続',
  sentence: BAND.reconnectNeeded.sentence,
  auto: '自動再接続',
  left: (s: number) => `あと${s}秒`,
  /** why the page could not reconnect by itself (the diagnostic log under 詳細表示). */
  why: '詳細表示',
  notes: {
    away: '前のPC接続ノード 使用不可',
    none: '同種の機器 なし',
    several: (n: number) => `同種の機器${n}個（特定不可）`,
    silent: '出現した機器 応答なし',
    notHub: '出現した機器 コーディネータ以外',
    otherHub: '出現した機器 別のコーディネータ',
    wire: (v: number) => `出現した機器 通信形式のバージョン${v}（非対応）`,
    version: (v: string, trial: boolean) => `出現したPC接続ノード${v}${trial ? '' : '（前のバージョン）'}`,
  },
} as const;

/** Help in the header, and the operating-mode key list. */
export const HELP = {
  heading: 'トラブル対処',
  close: 'トラブル対処の非表示',
  /** Tool help, independent of any project: what the user sees, its real cause, and what to do. */
  rows: [
    ['「接続」のクリック後も機器の一覧が空白', 'パソコンがPC接続ノードをUSB機器として認識していません。充電専用ケーブルはデータ通信が不可のため、データ通信用のケーブルに交換し、別のUSB端子でも試行してください。'],
    ['一覧にPC接続ノードが未表示', 'PC接続ノードにDAGONが未書込か、電源が未投入です。初回のPC接続ノードは書込モードの「書込」でUSB経由の直接書込を実行します。'],
    ['接続後も「応答なし」のノードが存在', 'PC側のノードからそのノードへの配線か電源が切断状態です。配線表（表の「配線」）との一致を確認してください。'],
    ['「ビルド」が使用不可', '構成に問題が残存しているか、dagon serveが停止中です。下部の「問題」を修正し、ターミナルでdagon serveを起動してください。'],
    ['「書込」が使用不可', 'ビルドが未実行か、PC接続ノードと未接続です。準備で「ビルド」、書込で「接続」の順序で実行してください。'],
    ['書込の停滞', '1ノードの書込に約1分を要します。カードの%が増加中であれば書込は継続中です。途中で停止しても、ノードは再書込が可能な状態で動作します。'],
    ['書込後にノードが前のバージョンへ復帰', '新バージョンでの起動継続に失敗し、DAGONが自動で前のバージョンへ復元しました。アプリを修正してバージョンアップ後に再度書込を実行してください。'],
  ],
  keysHeading: 'キー操作',
  keysClose: 'キー操作の非表示',
  keys: [
    ['Tab', 'ノードへ移動'],
    ['矢印', '隣接ノードへ移動'],
    ['Alt + 矢印', 'ノードやDAGONマークの移動（Shift併用で4倍の距離）'],
    ['Enter', '選択'],
    ['Esc', 'シート・メニュー・一覧の非表示'],
    ['Delete', '選択したノードやリンクの削除（確認後）'],
    ['Ctrl + Z', '取消'],
    ['Ctrl + Shift + Z', '再実行'],
    ['Ctrl + J', '下部パネルの表示・非表示'],
    ['L', 'PC側のノードの選択（キャンバス上）'],
    ['F', 'キャンバス全体の表示（キャンバス上）'],
    ['Ctrl+ホイール', 'キャンバスの拡大・縮小'],
    ['ドラッグ', 'キャンバスやノードの移動'],
  ],
} as const;

/** Visually hidden announcements (role="status" / alert). */
export const ANNOUNCE = {
  hubConnected: 'PC接続ノード 接続、ノード確認中',
  found: (n: number) => `発見${n}`,
  responding: (n: number, of: number) => `応答${n}/${of}`,
  nodeSilent: (name: string) => `${name}、応答なし`,
  hubLost: 'PC接続ノード 切断',
  placed: (product: string, hub: boolean) => `${product}を配置、${hub ? 'コーディネータ' : 'ノード'}`,
  removed: (name: string) => `${name}を削除、Ctrl+Zで復元が可能`,
  moved: (name: string) => `${name}を移動`,
  moveCancelled: '移動を中止',
  problems: (errors: number, warnings: number) => `問題${errors}、注意${warnings}`,
  mode: (define: boolean) => (define ? '準備モード' : '書込モード'),
  fileChecking: (name: string, size: string) => `${name} ${size}、確認中`,
  fileOk: (n: number, skipped = 0) => `配布元 確認済、構成 一致、対象${n}${skipped ? `、書込不可の${skipped}ノードはスキップ` : ''}`,
  fileSource: '配布元 確認不可',
  fileTopology: '構成 不一致',
  fileFormat: 'パッケージファイル以外',
  writing: (name: string) => `${name}へ書込`,
  progress: (n: number) => `進捗${n}%`,
  hubRestarting: 'PC接続ノード 再起動中、自動で再接続',
  stopped: (name: string) => `書込 停止、${name}`,
  coreBack: (name: string) => `${name}、新ブートローダは不使用`,
  previous: (name: string) => `${name}、前のバージョン`,
  done: (version: string) => `完了、${version}`,
  reconnectTimeout: 'PC接続ノード 再接続 時間超過',
  rolledBack: (name: string, version: string) => `${name}を${version}に復元`,
  recovered: (name: string, rest: number) => `${name}、書込済、未書込${rest}`,
  /** "最新リリースの使用" found no release in the project. */
  noRelease: 'リリース なし',
} as const;

/** Words that never appear in operating-mode guidance, chips or results. */
export const FORBIDDEN_WORDS = [
  '署名', 'ROM', 'CDC', 'ブートコア', 'エラーコード', 'トポロジ',
  'マニフェスト', 'ハッシュ', 'ポート', 'シリアル', 'フラッシュ', 'ファームウェア',
  'パーティション', 'コマンド', 'タイムアウト', 'WASM', 'Web Serial', '確定', '試行中',
  '巻き戻', '不正', '致命的', '無効', '復旧の層', 'ブートコア',
] as const;

/** Short names of the check rules (problem chips and the problem list). */
export const RULE_SHORT: Record<string, string> = {
  'V-01': 'ファイルの形式',
  'V-02': '名称の重複とノード数',
  'V-03': 'コーディネータの数',
  'V-04': 'コーディネータ以外のノード間のリンク',
  'V-05': '未接続のノード',
  'V-06': '起動の方式',
  'V-07': '定義外のピン',
  'V-08': 'ピンの重複',
  'V-09': 'UARTのピン',
  'V-10': 'UARTの数',
  'V-11': '起動の切替方法',
  'V-12': '修正方法の順序',
  'V-13': '最後の手段のピン',
  'V-14': '書込の順序',
  'V-15': '互換の規則',
  'V-16': 'フラッシュのサイズ',
  'V-17': 'I2Cのピン',
  'V-18': 'アドレスの重複',
  'V-20': 'I2Cの速度',
  'V-21': 'I2C接続が不可のノード',
  'V-22': 'リセット・起動の配線 未設定',
  'V-23': '書込不可の部分',
  'V-24': '使用不可の接続',
  'V-25': 'コーディネータに不適合のマイコン',
  'V-26': 'UARTの速度',
  'V-27': 'リンクの接続口',
};

/** Names a fix sentence needs: the node (表示名), its board, its id, and whether there are too many nodes. */
export interface FixWords {
  to: string;
  board: string;
  id: string;
  many: boolean;
  /** The node limit. */
  max: number;
  /** The node is one updating skips: its link is not a thing to fix. */
  skipped?: boolean;
  /** For the fix sentences, read from what the check reads: the pins the pin's own node defines, the pairs its ROM loader listens on, the speeds every node on the bus names. */
  pins?: string;
  pairs?: string;
  hzs?: string;
}

/** How to fix each rule, one sentence. */
export const RULE_FIX: Record<string, (w: FixWords) => string> = {
  'V-01': () => 'dagonの新バージョンで出力したファイルの読込を実行してください。',
  'V-02': (w) => (w.many ? `ノードを${w.max}個以下にしてください。` : `名称「${w.id}」の重複する2つのどちらかを変更してください。`),
  'V-03': () => 'コーディネータを1つだけにしてください。',
  'V-04': () => 'コーディネータ以外のノード間のリンクを削除し、コーディネータと接続してください。',
  'V-05': (w) => `${w.to}をコーディネータとリンクで接続してください。`,
  'V-06': (w) => `『修正』で${w.board}に適合する起動の方式にしてください。`,
  'V-07': (w) => `そのノードの定義にあるピン（${w.pins ?? '定義のピン'}）のどれかに変更してください。`,
  'V-08': () => '重複したピンのどちらか1か所を未使用のピンに変更してください。',
  'V-09': () => '『修正』でコーディネータの別のUARTに交換してください。',
  'V-10': () => 'UARTのリンクを1本削減してください。',
  'V-11': (w) => `${w.to}の起動の切替方法を選択してください（最後の手段での修正時に使用します）。`,
  'V-12': (w) => `${w.to}の修正方法の順序から最後の手段を除外してください。`,
  'V-13': (w) => `${w.to}をそのボードのI2Cの組（${w.pairs ?? '定義の組'}）のどれかに接続してください。`,
  'V-14': () => '『修正』でコーディネータが最後になる順序にしてください。',
  'V-15': () => '互換の規則から不在のノードを除外してください。',
  'V-16': () => '既定の配置に復元してください。',
  'V-17': (w) => `共有の2本線（I2C）をコーディネータの定義にあるピン（${w.pins ?? '定義のピン'}）にしてください。`,
  'V-18': (w) => `${w.to}のアドレスを未使用の番号に変更してください。`,
  'V-20': (w) =>
    `共有の2本線（I2C）の速度を接続するノードの定義にある値（${w.hzs ?? '定義の速度'}）のどれかにしてください。バスを駆動するノードのクロックが原因の場合は、水晶を記述したボードを指定してください。`,
  'V-21': (w) => `${w.to}はUARTで接続してください。`,
  'V-22': (w) => `最後の手段で復旧するには、『修正』で${w.to}のリセット・起動の配線を設定してください（現状のままでも書込は可能です）。`,
  'V-23': () => 'この構成のままで正常に使用が可能です。書込はこの部分に到達しないため、この部分をスキップします。',
  'V-24': (w) =>
    w.skipped
      ? `${w.to}の接続は使用不可の種類です。${w.to}は書込でスキップするため、現状のままで使用が可能です。`
      : `${w.to ? `${w.to}の` : ''}接続は使用不可の種類です。プロジェクトの作成には、使用可能な種類を選択してください（現状のままでも保存は可能です）。`,
  'V-25': (w) => `${w.to}の代替として、コーディネータに適合するマイコンのノードをコーディネータにしてください。`,
  'V-26': () => 'UARTの速度は、接続する両方のマイコンで誤差が定義の許容範囲内になる値にしてください。クロックが原因の場合は、水晶を記述したボードを指定してください。',
  'V-27': () => 'リンクの端の接続口は、そのマイコンの定義にあるリンクと同種のものにしてください。',
};

/** Header: the two modes (準備｜書込). */
export const MODE = {
  operate: '書込',
  define: '準備',
  label: 'モード',
  /** Why a mode cannot be chosen, on hover. */
  offWhy: { locked: '書込の完了までモードの変更は不可です', operate: '構成を作成し問題を修正すると、書込が使用可能です' },
  /** 書込 pressed while the 構成 is not in the project's YAML. */
  lockedUnsaved: { sentence: '構成を保存すると書込が使用可能になります。' },
  /** 書込 pressed with no 構成 or one with problems. */
  lockedProblems: { sentence: '構成を作成し問題を修正すると、書込が使用可能になります。' },
} as const;

/** Define mode. */
export const DEFINE = {
  /** The parts column. */
  listHeading: 'ノードリスト',
  /** Parts column: choose by product or by MCU. */
  byProduct: '製品',
  byMcu: 'マイコン',
  chooseBy: '選択方法',
  listHelp: '選択してEnterで配置します。',
  place: '配置',
  /** The parts window. */
  search: '製品・マイコンの検索',
  byMcuOwn: 'マイコン',
  ownBoard: 'ボード',
  count: (n: number) => `${n}個`,
  placed: '配置済',
  firstHeading: '最初の手順',
  connectDiscover: 'PC接続ノードの探索',
  /** Canvas toolbar. */
  undo: '取消',
  redo: '再実行',
  graph: 'グラフ',
  table: '表',
  exportYaml: 'YAML出力',
  importYaml: 'YAML取込',
  zoomIn: '拡大',
  zoomOut: '縮小',
  /** Dot numbers in define mode. */
  dotNodes: 'ノード',
  dotLinks: 'リンク',
  dotNodesName: (n: number, l: number) => `ノード${n}、リンク${l}`,
  dotProblems: '問題',
  dotSkipped: '◇ 書込不可',
  dotProblemsName: (p: number, n: number, l: number) => `問題${p}、ノード${n}、リンク${l}`,
  /** Nothing selected and nothing to fix. */
  nothingSelected: 'ノードかリンクの選択時にここで修正が可能です。',
  /** The problem list. */
  problems: (n: number) => `! 問題${n}`,
  problemsNone: '問題0',
  cautions: (m: number) => `▲ 注意${m}`,
  /** Parts updating skips and links of a kind the firmware does not run, after the cautions. */
  unsupported: (k: number) => `◇ 制限${k}`,
  fix: '修正',
  ruleId: '規則ID',
  yamlPath: 'YAMLの位置',
  details: '詳細表示',
  problemName: (short: string, rule: string, sentence: string) => `${short}、規則${rule}、${sentence}`,
  /** Bottom band. */
  emptySentence: 'PC接続ノードをUSBでパソコンに接続後に、『PC接続ノードの探索』をクリックしてください。',
  /** 書込 from 準備. */
  toOperate: '書込へ',
  /** Writes the 構成 back to the project's YAML through dagon serve. */
  save: '保存',
  fixSentence: '修正後にビルドが可能です。',
  readySentence: 'ビルド後に書込モードで書込を実行します。',
  buildingSentence: 'ビルドの完了まで待機してください。初回は1ノード1〜3分が基準です。',
  /** The running build is at its `nth` node of `total`. */
  buildingAt: (nth: number, total: number) => `ビルド中です（ノード${nth}/${total}）。初回は1ノード1〜3分が基準です。`,
  builtSentence: '書込モードへの切替後にノードへの書込を実行します。',
  skeletonFirst: 'テンプレートの事前生成でビルドが可能になります。',
  /** Nodes updating skips; the 構成 is valid as it is. */
  skippedSentence: (k: number) => `書込不可のノードが${k}個あります。構成は現状のままで使用が可能です。書込ではそのノードをスキップします。`,
  /** The 構成 of the release file chosen in 書込 replaces this one (undoable, saved to the YAML). */
  importFileTopology: 'ファイルの構成の取込',
  unreadableChip: '× 形式不一致',
  unreadableSentence: 'dagonで出力した.yamlファイルを選択してください。',
  /** GET /v1/definitions 500: the open project's MCU/board definitions do not load (serve's terminal says why). */
  defsChip: '× 定義エラー',
  defsSentence: 'このプロジェクトのMCU・ボード定義の読込に失敗しました。理由はdagon serveの端末に表示しています。',
} as const;

/** Inspector labels; 詳細設定 shows the schema names as they are. */
export const INSPECT = {
  name: '表示名',
  id: 'ID（英数字）',
  role: '役割',
  roles: { coordinator: 'コーディネータ', node: 'ノード' } as Record<string, string>,
  product: '製品名・MCU',
  connectTo: 'PC側のノード',
  none: 'なし',
  reset: 'リセット',
  boot: '起動',
  unset: '未設定',
  advanced: '詳細設定',
  kind: '種類',
  addr: 'アドレス（16進）',
  freeAddr: '未使用の番号',
  nodePins: '相手側のピン',
  uartPins: 'PC側のノードのピン',
  uartTx: (from: string) => `${from}のTXピン`,
  uartRx: (from: string) => `${from}のRXピン`,
  /** A value from a file that the definition does not name: kept, marked, refused by the check. */
  undefinedPin: '（定義外）',
  maxAttempts: '起動の試行回数',
  listenWindow: '応答の待機時間（ミリ秒）',
  flashMap: 'メモリの配置',
  tag: 'タグ',
  baud: '速度（ボーレート）',
  /** No speed both ends' definitions name: the link cannot be set up with these MCUs. */
  noBaud: 'この2つのマイコンに共通の速度がありません',
  busSda: 'SDAのピン（PC側のノード）',
  busScl: 'SCLのピン（PC側のノード）',
  removeLink: 'このリンクの削除',
  removeNode: 'このノードの削除',
  bus: '共有の2本線（I2C）',
  busPins: 'ピン',
  speed: '速度',
  /** No speed every node's definition names: the bus cannot be set up with these MCUs. */
  noSpeed: 'このバスのマイコンに共通の速度がありません',
  /** A bus speed as the user reads it. */
  hz: (hz: number) => `${hz / 1000} kHz`,
  pullups: 'プルアップ',
  pullupKinds: { external: '外部', board: '基板上' } as Record<string, string>,
  inUse: (what: string) => `（使用中: ${what}）`,
  boardOwn: 'ボード定義なし',
  /** The flash size the MCU definition's default flash map lays out (not a claim about the flash the chip owns). */
  flashSize: 'フラッシュ（定義の配置）',
  flashValue: (b: number) => (b >= 1 << 20 ? `${b / (1 << 20)} MiB` : `${b / (1 << 10)} KiB`),
  /** The coordinator's hop towards the PC. */
  uplink: 'ホスト接続',
  uplinkHost: 'ホスト機器',
  hosts: { 'pc-cli': 'パソコンのdagon', browser: 'ブラウザ', embedded: '組込機器' } as Record<string, string>,
  /** On a kind the MCU firmware at an end does not run. */
  unavailable: '使用不可',
  unavailableOption: (label: string) => `${label}（使用不可）`,
} as const;

/**
 * Each link kind's words, keyed by the name a 構成 writes (dagon-wasm's link_kind_table; a
 * test keeps the keys equal to it): `label` on the canvas, the table and the file check,
 * `choice` in the inspector's list of kinds.
 */
export const LINK_KIND_WORDS: Record<string, { label: string; choice: string }> = {
  'usb-cdc': { label: 'USB', choice: 'USB' },
  uart: { label: 'UART', choice: 'UART（1対1の配線）' },
  i2c: { label: 'I2C', choice: 'I2C（共有の2本線）' },
  ble: { label: 'BLE', choice: 'BLE' },
  'wifi-tcp': { label: 'Wi-Fi', choice: 'Wi-Fi（TCP）' },
  ethernet: { label: 'Ethernet', choice: 'Ethernet（コーディネータ間）' },
  can: { label: 'CAN', choice: 'CAN（コーディネータ間）' },
};

/** The confirmation before anything is deleted (Delete key, trash button, the table's 削除). */
export const CONFIRM = {
  node: (name: string) => `${name}の削除`,
  link: (from: string, to: string) => `${from} → ${to}のリンクの削除`,
  nodeSentence: 'このノードとその接続リンクを構成から削除します。取消（Ctrl+Z）で復元が可能です。',
  linkSentence: 'このリンクを構成から削除します。取消（Ctrl+Z）で復元が可能です。',
  remove: '削除',
  cancel: 'キャンセル',
  /** The other destructive actions: the same modal, their own words. */
  unlink: { title: 'DAGONとの連携解除', sentence: 'AIのエージェントとの接続と送信への同意を取消します。以後の質問は不可になります。', label: '連携解除' },
  key: { title: 'APIキーの削除', sentence: '登録したAPIキーをOSの資格情報ストアから削除します。再度の使用には再登録が必要です。', label: '削除' },
  rollback: (name: string, version: string) => ({ title: `${name}の前のバージョンへの復元`, sentence: `${name}を前のバージョン${version}で再起動します。現在のバージョンは保持するため再度の書込が可能です。`, label: '復元' }),
} as const;

/** The table view. */
export const TABLE = {
  tabs: ['ノード', 'リンク', '配線'],
  nodeCols: ['番号', '表示名', '製品名・MCU', '役割', '接続方法', 'ピン'],
  linkCols: ['接続元', '接続先', '種類', 'アドレス', 'ピン'],
  addNode: 'ノードの追加',
  addLink: 'リンクの追加',
  addLinkWhy: '未接続のノードはありません',
  edit: '編集',
  remove: '削除',
} as const;

/** The define mode's CLI features. */
export const CLI_NAV = {
  heading: 'CLI専用の機能',
  items: {
    link: ['01', 'CLI連携'],
    jobs: ['04', '作業'],
    release: ['05', '署名とリリース'],
    project: ['07', 'プロジェクト'],
    flash: ['08', '書込'],
  } as Record<string, [string, string]>,
  running: (n: number) => `作業、${n}件実行中`,
  none: 'CLI未連携',
} as const;

/** Header CLI link. */
export const CLI_STATE = {
  none: 'CLI未連携',
  connecting: 'CLI接続中…',
  stopped: 'CLI停止',
  denied: 'CLI未連携',
  ready: (v: string) => `CLI ${v}`,
} as const;

/** CLI連携. */
export const CLI_LINK = {
  steps: ['ターミナルの起動', 'dagon serveの実行', '表示されたアドレスをこのブラウザで表示'],
  withoutCli: ['CLIなしで使用可能', 'グラフの編集・書込'] as const,
  deniedChip: '× パスフレーズ不一致',
  deniedSentence: 'ターミナルに表示されたアドレスへ変更なしで再アクセスしてください。',
  stoppedSentence: 'ターミナルでdagon serveを再実行してください。',
  needCli: 'ターミナルでdagon serveを実行し、表示されたアドレスへアクセスしてください。',
  reconnect: 'CLIへの再接続',
  workspace: '作業フォルダ',
  /** The API key of each way that takes one, kept in the OS credential store through serve. */
  keys: {
    heading: 'APIキー',
    detail: 'キーはローカルのdagon serve経由でOSの資格情報ストアに保存します。このページには残存しません。ターミナルではdagon ai key setでも登録が可能です。',
    set: '登録済',
    unset: '未登録',
    input: 'キー',
    save: 'キーの保存',
    saveWhy: 'キーを入力してください',
    remove: 'キーの削除',
    removeWhy: 'OSの資格情報ストアからこのキーを削除します。削除後はこの接続方法でのDAGONへの質問が不可になります',
  },
  key: '署名キー',
  yes: 'あり',
  no: 'なし',
  features: '使用可能な機能',
  featureNames: { gen: 'テンプレート生成', build: 'ビルド', sign: '署名', release: 'リリース作成', flash: '書込', topology_save: '構成の作業フォルダへの保存' } as Record<string, string>,
  unsupported: '非対応',
} as const;

/** 作業. */
export const JOBS = {
  heading: '作業',
  cols: ['種類', '対象', '状態', '経過', '結果'],
  kinds: { build: 'ビルド', sign: '署名', release: 'リリース', gen: 'テンプレート', flash: '書込', conform: '適合試験', doctor: '環境診断', ai_topology: '構成の案', ai_port: 'ボード移植' } as Record<string, string>,
  states: { queued: '待機', running: '実行中', question: '質問あり', succeeded: '成功', failed: '失敗', cancelled: '中止', unknown: '不明' } as Record<string, string>,
  none: '作業 なし',
  started: '開始時刻',
  cancel: 'ジョブの中止',
  copyLog: 'ジョブのログのコピー',
  failSentence: 'ログの赤色の行を確認後に再度ビルドしてください。',
  answer: '接続済',
} as const;

/** 書込. */
export const FLASH = {
  heading: '書込',
  write: '書込',
  /** How each MCU is connected comes from its definition ([connection], dagon-wasm's mcu_connection). */
  via: '接続方法',
  hubFirst: 'コーディネータがないと他のノードの確認が不可のため、コーディネータから順に書込を実行します。',
  /** the confirmation before serve writes the board (its job question). */
  confirm: (name: string) => `${name}への書込を実行しますか。`,
  /** A family serve's flash does not write (ESP32-S3). */
  otherTools: 'このマイコンはESPのツールで書込を実行してください。',
  yes: 'はい',
  no: 'いいえ',
  done: '✓ 書込済',
  running: '● 実行中',
  notFound: '× 未検出',
} as const;

/** The 構成 differences of a release made for another 構成 (otherTopology). */
export const DIFF = {
  heading: '構成の差異',
  cols: ['ノード・リンク', 'この構成', 'ファイル'],
  none: 'なし',
  /** The chip on the graph's differing nodes. */
  chip: '△ 差分',
  spoken: '差異',
  /** No topology in the file: links and addresses were not compared. */
  linksUnchecked: 'リンク・アドレス 未確認',
} as const;

/**
 * Saving the 構成 to the project's YAML: the edits stay on the page, marked 未保存, until a save goes through.
 */
export const SAVE = {
  unsaved: '● 未保存',
  /** 422: the CLI's own check refused the 構成. */
  invalid: { chip: '× 保存失敗', sentence: '問題の修正後に再度保存してください。' },
  /** 409: a job holds the project. */
  busy: { chip: '… 作業中', sentence: '作業の完了後に再度保存してください。' },
  failed: { chip: '× 保存失敗', sentence: 'ターミナルでdagon serveの動作を確認後に再度保存してください。' },
} as const;

/** leaving the project while its 構成 could not be saved (the dialog). */
export const UNSAVED = {
  heading: '未保存の変更',
  sentence: '再度保存するか、変更を破棄して続行するかを選択してください。',
  save: '構成の再保存',
  discard: '変更の破棄と続行',
  stay: '編集へ復帰',
} as const;

/** The start screen (開始). */
export const START = {
  heading: '開始',
  new: 'プロジェクト作成',
  /** 「プロジェクト作成」's modal: how, then name and place. */
  create: {
    title: 'プロジェクト作成',
    how: '作成方法',
    empty: '空白から作成',
    sample: 'サンプルから作成',
    cancel: 'キャンセル',
    next: '名称と保存先の決定',
    back: '前へ',
    pick: '保存先の選択',
    where: '作成先',
    go: 'プロジェクト作成',
  },
  /** Under the product's name on the start page. */
  sub: 'ノードとリンクの構成を描画すると、ノードごとのアプリのテンプレートを生成します。',
  open: 'プロジェクトのオープン',
  recent: '最近のプロジェクト',
  recentNone: 'なし',
  up: '上位フォルダへ',
  upWhy: '最上位のフォルダです',
  goWhy: '名称の入力と保存先の選択後に作成が可能です',
  place: '保存先',
  name: '名称',
  cantMake: '× 作成失敗',
  cantOpen: '× 読込失敗',
  loading: '読込中…',
  /** The folder of a project (it holds dagon.toml). */
  project: '● プロジェクト',
  /**
   * Why serve did not open or make it: 404 / 422 / 409 / 403. A folder that cannot be written is BAND.folderFailed.
   */
  missing: { chip: '× フォルダ不一致', sentence: 'dagon.tomlのあるフォルダを選択してください。' },
  key: { chip: '× キーエラー', sentence: 'dagon serveのターミナルでキーエラーの理由を確認後に再度選択してください。' },
  busy: { chip: '… 作業中', sentence: '作業の完了後に再度選択してください。' },
  refused: { chip: '× 範囲外', sentence: 'ホームフォルダ内のフォルダを選択してください。' },
  exists: { chip: '× 名称の重複', sentence: '未使用のフォルダ名を入力してください。' },
  /** A folder with a 構成 but no dagon.toml. */
  initAsk: 'このフォルダにdagon.tomlを作成しますか。',
} as const;

/** The simulator (the demo page, the development stand-in), marked wherever the page is. */
export const SIM = {
  badge: '模擬',
  label: '模擬のノード（実機ではありません）',
  /** Before connecting: no device list appears for the simulated PC-connected node. */
  connect: { sentence: '模擬のPC接続ノードに接続します。機器の選択一覧は表示しません。' },
  /** The demo page while watching: it reads no files, so its prepared package is what 書込 writes. */
  siteWrite: '「書込」で用意した新バージョンの書込が可能です。',
} as const;

/** The menu (メニュー) and its one confirmation. */
export const MENU = {
  button: 'メニューの表示',
  restart: 'リスタート',
  groups: { project: 'プロジェクト', define: '構成', connect: '接続操作', dagon: 'DAGON', help: 'トラブル対処' },
  save: '保存',
  arrange: '自動整列',
  moveCard: '選択ノードの移動',
  moveMark: 'DAGONマークの移動',
  place: 'パッケージファイルの選択と書込',
  rollback: '前のバージョンへの復元',
  applyProposal: 'DAGONの案の反映',
  cli: 'CLIの接続とAPIキーの表示',
  port: 'エージェントによるボード移植',
  /** The view toggle, in the menu only while the bar is too narrow to hold it. */
  viewTable: '表形式の表示',
  viewGraph: 'グラフ形式の表示',
  keys: 'キー操作',
  copyLog: '記録のコピー',
  copyUrl: 'このURLのコピー',
  /** Every looping motion (the mark, the water, the attention light) paused or running again (WCAG 2.2.2). */
  motion: 'アニメーションの停止と再開',
  copied: 'コピー完了',
  /** What each item does, under its name (a first-time user reads it before pressing). */
  hint: {
    new: '構成の描画から開始します（現在のプロジェクトは保存します）',
    open: 'パソコンのフォルダにあるdagon.tomlのプロジェクトをオープンします',
    save: '構成をプロジェクトのYAMLに即時保存します（通常は自動で保存します）',
    undo: '構成への最後の変更を取消します',
    redo: '取消した変更を再実行します',
    parts: '左側に部品のウィンドウを表示します（ドラッグでキャンバスに配置が可能です）',
    arrange: '移動したカードをコーディネータ起点の配列に再整列します',
    moveCard: '選択したノードのカードをキャンバスのクリック位置へ移動します（Alt+矢印キーでも移動が可能です）',
    moveMark: 'DAGONのマークをクリック位置へ移動します（Alt+矢印キーでも移動が可能です）',
    fit: '全ノードを表示するサイズと位置に調整します',
    viewTable: 'ノード・リンク・配線を表形式で表示します',
    viewGraph: 'ノードとリンクを図で表示します',
    connect: 'USBで接続したPC接続ノードと通信を開始します',
    disconnect: 'PC接続ノードとの通信を停止します（USBの抜去も問題ありません）',
    place: 'パソコンにある.dgbファイルを選択し、その内容のノードへの書込を実行します',
    rollback: '選択したノードを前のバージョンで再起動します',
    link: 'AIのエージェントと接続し、構成についての質問を可能にします',
    talk: 'DAGONの会話ウィンドウを表示します',
    apply: 'DAGONが提示した構成の変更案を構成に反映します',
    cli: 'dagon serveとの接続と登録したAPIキーを表示します',
    port: '連携したエージェントが、DAGONを新規のボードへ移植します',
    unlink: 'AIのエージェントとの接続と同意を取消します',
    help: 'よくある問題の原因と対処方法',
    keys: 'キーボードでの操作の一覧',
    copyLog: '下部の「記録」の内容をクリップボードにコピーします',
    copyUrl: 'このページのアドレスをコピーします',
    motionStop: '反復動作するアニメーションを停止します',
    motionResume: '停止中のアニメーションを再開します',
    restart: 'このページの記録を削除し、開始画面へ移動します',
  },
  /** Why an item cannot be used now. */
  off: {
    notDefine: '準備で使用可能です。',
    notGraph: 'グラフ表示で使用可能です。',
    notConnected: 'PC接続ノードへの接続後に使用可能です。',
    notIdle: '現在の動作の完了後に使用可能です。',
    noNode: 'ノードの選択後に使用可能です。',
    noPrevious: '復元可能な前のバージョンはありません。',
    noProposal: 'DAGONの案はありません。',
    noUndo: '取消可能な変更はありません。',
    noRedo: '再実行可能な変更はありません。',
    partsOpen: '部品のウィンドウは表示中です。',
    arranged: 'コーディネータを起点とする配列に整列済です。',
    noLog: '記録はありません。',
    noProject: 'プロジェクトのオープン後に使用可能です。',
    demo: 'デモでは使用不可です。dagon serveで使用可能です。',
  },
  restartHeading: 'リスタートの確認',
  restartSentence: 'このページの記録を削除し、開始画面へ移動します（復元は不可です。プロジェクトのファイルと基板内のプログラムは無変更です）。',
  restartConfirm: '記録の削除とリスタート',
  cancel: 'リスタートの中止',
} as const;

/** Fills {name} slots in a copy string. */
export const fill = (s: string, v: Record<string, string | number>): string => s.replace(/\{(\w+)\}/g, (m, k: string) => (k in v ? String(v[k]) : m));

export const SKELETON = {
  make: 'テンプレート生成',
  makeWhy: 'ノードの配置と問題の修正後に生成が可能です',
  title: 'テンプレート生成',
  state: {
    running: '… 生成中',
    done: '✓ 生成済',
    failed: '! 失敗',
  },
  where: '生成先',
  next: {
    '1': 'エディタで各ノードのsrc/app.rsにアプリのコードを記述（main.rsとwiring.rsはDAGONが生成するため変更は不要）',
    '2': '準備で「ビルド」の後に、書込で「書込」',
    heading: '後続の手順',
  },
  fail: {
    fix: '理由の修正後に「再生成」をクリックしてください。生成済のノードは保持します。',
  },
  startFailed: 'dagon serveが受付を拒否しました',
  retry: '再生成',
  cancel: '中止',
} as const;

/** ボード移植 (`dagon ai port`): the linked agent ports DAGON to a board in the project's DAGON clone. */
export const PORT = {
  title: 'ボード移植',
  state: {
    running: '… 移植中',
    done: '✓ 完了',
    failed: '! 失敗',
  },
  intro: '連携したエージェントが、プロジェクトの参照するDAGONのソースで、指定のボードへの移植と適合試験を実行します。',
  mcu: 'MCUのID',
  board: 'ボードのID',
  idNote: '英数字と「-」「_」「.」の最大32文字（rp2040、picoなど）',
  idBad: '英数字と「-」「_」「.」の1〜32文字で入力してください',
  kind: '移植の種類',
  kinds: { S: 'DAGONのブート段階を使用', V: 'ベンダーの2nd stageを使用' },
  description: 'ボードの説明（任意）',
  descriptionNote: 'フラッシュの部品、接続ピン、データシートの記載など、エージェントへの補足情報',
  descriptionBad: '説明は32 KiB以内で入力してください',
  consent: {
    heading: '同意事項',
    items: [
      'エージェントは、DAGONのソースのフォルダでファイルの変更とコマンドの実行を、確認なしで実施します',
      '移植は最長6時間継続し、連携したAPIキーの利用料金が発生します',
      'ソースのファイルとコマンドの出力をエージェントの提供元へ送信します',
    ],
    agree: '上記の全項目に同意',
  },
  start: '移植の開始',
  startWhy: 'IDの入力と同意の後に開始が可能です',
  cancel: '移植の中止',
  log: '作業記録',
  logNone: 'エージェントの開始を待機中です。',
  report: 'エージェントの報告',
  figures: (turns: number, usd: number | null) => (usd === null ? `${turns}ターン` : `${turns}ターン、${usd.toFixed(2)} USD`),
  after: '報告の合格数を確認後、DAGONのソースの変更をレビューしてください。移植したボードは、dagon serveの再起動後に部品の一覧へ追加されます。',
  fail: {
    fix: '理由の解消後に再度開始してください。下記の手順でClaude Codeから実行する方法もあります。',
  },
  retry: '入力への復帰',
  /** The same port without `dagon ai port`: the skill run from the agent's own program. */
  manual: {
    heading: 'Claude Codeでの実行手順',
    steps: ['DAGONのソースのフォルダでClaude Codeを起動', '下記のコマンドを入力（MCUのID、ボードのID、種類の順）', '完了後にdagon serveを再起動'],
    command: (mcu: string, board: string, kind: string) => `/dagon:dagon-port ${mcu || '<mcu>'} ${board || '<board>'} ${kind}`,
  },
  /** Why the page cannot start the port itself. */
  off: {
    noFeature: 'このdagon serveはボード移植に非対応です。下記の手順で実行が可能です。',
    notLinked: 'DAGONと未連携です。メニューの「DAGONとの連携」でAPIキーの接続方法と連携するか、下記の手順で実行してください。',
  },
} as const;

export const NOTICE = {
  close: 'クローズ',
  more: '詳細表示',
  reason: '理由',
  fix: '対処方法',
  after: '以降の手順',
} as const;

export const BUILD = {
  /** A node with no [build.*] in dagon.toml: nothing to build until the skeleton is made. */
  noSkeleton: { reason: 'このノードのテンプレートは未生成です。', stale: 'DAGONのソースが変更されたため、テンプレートは最新ではありません。', fix: 'テンプレートの生成後にビルドが可能になります。' },
  run: 'ビルド',
  runningWhy: 'ビルド中です',
  title: 'ビルド',
  state: {
    running: '… ビルド中',
    failed: '! ビルド失敗',
  },
  toast: {
    done: '{n}ノードのビルド完了',
  },
  fail: {
    fix: 'エディタで{file}の{line}行目を修正後に再度ビルドしてください。',
    others: '他の{n}ノードはビルド済です。',
    plain: '理由の修正後に再度ビルドしてください。',
  },
  startFailed: 'dagon serveが受付を拒否しました',
  retry: '再ビルド',
  askAgent: '失敗理由の質問',
} as const;

/** The project's layout file (表示名 and card places). */
export const LAYOUT = {
  refused: '× 配置の読込不可',
  refusedDetail: '配置のファイルの読込に失敗したため、自動の配置で表示しました。ファイルは変更しません。配置と表示名の変更も保存しません。',
} as const;

/** The project's 構成 could not be read as it opened: nothing replaces it. */
export const TOPOLOGY_READ = {
  label: '× 構成の読込不可',
  detail: '構成のファイルの読込に失敗したため、変更は保存しません。プロジェクトを再度オープンしてください。',
} as const;

/**
 * The three cases of a node that does not answer, by dagon-host's key: it
 * answered before (途絶), the relaying node's records are complete and hold
 * none of it (通信実績なし), or the records cannot tell (履歴不明). The CLI
 * and the design say the same three.
 */
export const SILENCE = {
  lost: { word: '途絶', chip: '× 途絶' },
  neverReached: { word: '通信実績なし', chip: '× 通信実績なし' },
  historyUnknown: { word: '履歴不明', chip: '× 履歴不明' },
} as const;

export const WRITE = {
  noneChecked: { label: 'ノード未選択', detail: '書込対象のノードのカードをチェックしてください。' },
  alreadyOn: (v: string, coreToo: boolean) => ({ label: `全ノード${v}`, detail: `チェックしたノードは${v}で動作中${coreToo ? 'でブートローダも同一のため' : 'のため'}、書込を省略しました。` }),
  pickVersion: 'バージョン選択と書込',
  run: '書込',
  /** Why 書込 cannot be pressed, on hover. */
  offWhy: { notConnected: '接続後に書込が可能です', busy: '現在の書込の完了後に再度書込が可能です' },
  /** Why the version list cannot be opened, on hover. */
  pickOffWhy: '接続後のバージョン判明時に選択が可能です',
  /** Why a card's check cannot be changed, on hover. */
  checksLockedWhy: '書込の完了までチェックの変更は不可です',
  go: '{n}ノードへの書込',
  toast: {
    done: '{n}ノードの書込完了',
    none: '書込不要',
  },
  state: {
    writing: '… 書込中',
    done: '✓ 書込済',
    rolledBack: '▲ 復元済',
  },
  retry: '再書込',
  /** The header's version picker: its accessible name, and the latest build when its nodes' versions differ. */
  version: 'バージョン',
  latest: '最新のビルド',
  /** A node card's check: its accessible name. */
  pick: (name: string) => `${name}への書込`,
  /** On a card while it is written: the version it gets. */
  to: (v: string) => `→ ${v}`,
  toCore: (v: string) => `→ ブートローダ${v}`,
  /** On a card after the write: it went back, or it could not be written. */
  previous: '↺ 前のバージョンへ復帰',
  failed: '× 書込失敗',
  /** The real cause and the next step for a write that could not be made, by what the node or the link said. */
  why: {
    silent: { reason: 'ノードがPC接続ノードからの要求に応答しませんでした。', fix: 'ノードの電源とPC側のノードとの配線を確認後に再度書込を実行してください。' },
    LOST: { reason: '通信実績が存在するノードが応答しませんでした（途絶）。', fix: 'ノードの電源とPC側のノードとの配線を確認後に再度書込を実行してください。' },
    'HISTORY UNKNOWN': {
      reason: 'ノードが応答せず、PC接続ノードの通信実績の記録では過去の応答の有無が判定不能でした（履歴不明）。',
      fix: 'ノードの電源とPC側のノードとの配線を確認後に再度書込を実行してください。未書込のノードは、USBで直接接続してカードの「書込」で最初の書込を実行してください。',
    },
    E_SIGNATURE: { reason: 'ノードでパッケージの配布元の検証に失敗しました。別のキーで作成したパッケージです。', fix: 'このプロジェクトのキーで再ビルド後に再度書込を実行してください。' },
    E_KEY: { reason: 'ノードでパッケージの配布元の検証に失敗しました。別のキーで作成したパッケージです。', fix: 'このプロジェクトのキーで再ビルド後に再度書込を実行してください。' },
    E_DIGEST: { reason: '受信した内容が途中で破損していました。', fix: 'PC側のノードとの配線とケーブルを確認後に再度書込を実行してください。' },
    E_COMPAT: { reason: 'パッケージがこのノードの機種に不適合です。', fix: '構成のマイコンとボードを確認し、再ビルド後に書込を実行してください。' },
    E_FLASH: { reason: 'ノードが自身のメモリへの書込に失敗しました。', fix: 'ノードの電源の再投入後に再度書込を実行してください。' },
    'WIRE VERSION': { reason: 'ノードの通信形式のバージョンがこの画面と不一致です。', fix: 'ノードとこの画面の通信形式のバージョンを一致させてください。' },
    refused: { reason: 'ノードがこのパッケージの受信を拒否しました。', fix: '下部の「記録」で理由を確認し、再ビルド後に書込を実行してください。' },
    cannotFix: { reason: '再送信も予備からの起動も失敗しました。', fix: 'このノードをUSBで直接接続し、カードの「書込」で書込を実行してください。' },
  } as Record<string, { reason: string; fix: string }>,
  /** The written package could not be kept in the project (a toast; writing went on). */
  keepFailed: { label: '× 保存不可', detail: '書込済パッケージのout/deploy（プロジェクト内）への保存に失敗しました。' },
  rolled: {
    reason: '新バージョンでの起動継続に失敗し、前のバージョン{version}に復帰しました。ノードは動作中です。',
    next: 'アプリを修正してバージョンアップし、ビルド後に再度書込を実行してください。',
  },
  /** What a stopped run did for the release's compatibility constraints (ノード間のバージョンの整合). */
  constraints: {
    realigned: (k: number) => `互換制約を維持するため、書込済の${k}ノードを書込前のバージョンへ復帰しました。`,
    incompatible: (k: number) => `互換制約に違反した状態のノードが${k}個あります。`,
  },
  /** The new boot core failed its check: the node runs its confirmed core, or with one boot core slot its confirmed app; the app was not written. */
  coreBack: {
    label: '↺ 新ブートローダは不使用',
    detail: '新ブートローダが自己検査に不合格のため、使用していません。ブートローダのスロットが2つのノードは前のブートローダで、1つのノードは現在のアプリで動作中です。アプリの書込は未実行です。',
    next: 'ブートローダを修正してバージョンアップし、ビルド後に再度書込を実行してください。',
  },
  /** A silent node with a ROM entry: said once, before anything is touched. */
  rom: {
    silent: '× 応答なし',
    body: '{node}が応答しません。書込の実行で全内容を上書きします。配線の欠落でも応答なしになるため、確認後に続行してください。',
  },
  factory: {
    intro: '{node}のDAGONが応答しないため、USBで直接書込を実行します。最初の書込とDAGONの消失時のみの手順です。',
    steps: '手順',
    drive: 'ドライブ',
    waiting: '… 待機中',
    after: '書込の完了後にこの画面をクローズして接続します。他のノードは「書込」でDAGON経由の書込を実行します。応答しないノードはそのカードの「書込」で書込を実行します。',
    cancel: '中止',
    childIntro: '{node}はPC接続ノードから到達不可のため、このノードを直接接続して書込を実行します。',
    howToConnect: '接続方法',
    cancelLocked: '書込中',
  },
} as const;

export const STATE = {
  idle: '未接続',
  alive: '応答あり',
  silent: '応答なし',
  busy: '作業中',
  attention: '要確認',
  error: 'エラー',
} as const;

export const CONN = {
  toast: {
    connected: '{node}と接続',
    lost: '{node}との接続が切断',
  },
} as const;
