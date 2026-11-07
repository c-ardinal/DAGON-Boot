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
 * Header step indicator in three steps: ① つなぐ ② ファイル ③ 書き込み.
 * The operate machine's six steps fold into them (STEP.of3); recovery shows its name only.
 */
export const STEP = {
  label: '手順',
  of: '/03',
  names: { 1: 'つなぐ', 2: 'ファイル', 3: '書き込み', recover: '書き込み' } as Record<1 | 2 | 3 | 'recover', string>,
  of3: (s: Step): 1 | 2 | 3 | 'recover' => (s === 'recover' ? s : s <= 3 ? 1 : s === 4 ? 2 : 3),
} as const;

/** M48: hub connection in the header (no node counts here). */
export const HUB = {
  idle: '○ 未接続',
  checking: '… 親機 確認中',
  connected: '● 接続済',
  reconnecting: '… 親機 再接続中',
  lost: '× 親機 切断',
  cli: '● 親機 CLI 使用中',
} as const satisfies Record<HubState, string>;

/** Bottom-band buttons (fixed names); per-node names are in BUTTON_FOR. */
export const BUTTON = {
  connectDiscover: '親機を探す',
  connect: '接続する',
  reselect: '親機を選び直す',
  reload: '構成を読み直す',
  push: '親機へ送る',
  pushAgain: '構成を送り直す',
  scanAgain: 'ノードを探し直す',
  defineFix: '準備で構成を直す',
  chooseFile: 'パッケージファイルを選ぶ',
  useLatest: '最新のリリースを使う',
  otherFile: '別のファイルを選ぶ',
  update: 'ノードへ書き込む',
  reconnect: '接続し直す',
  recoverAgain: 'もう一度試す',
} as const satisfies Record<Exclude<Action, 'recover' | 'updateResponding'>, string>;

/** Buttons named after a node or a count. */
export const BUTTON_FOR = {
  recover: (name: string) => `${name}へ書き込む`,
  updateResponding: (n: number) => `応答する ${n} つだけ先に書き込む`,
  /** M50: the update after a recovery, over the nodes not written yet. */
  updateRest: '残りのノードへ書き込む',
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
  M04: { chip: '× 未検出', sentence: 'USB ケーブルを挿し直してから、もう一度押してください。' },
  M05: { chip: '× 使用中', sentence: '親機を使っているタブやアプリ（dagon CLI を含む）を閉じてから、もう一度押してください。' },
  M06: { chip: '× 許可なし', sentence: 'ターミナルに表示されたアドレスから開き直してください。' },
  M08: { chip: '× 応答なし', sentence: '一覧から「DAGON hub」を選び直してください。' },
  M10: { sentence: '実際の配線と同じか確かめてから、親機へ送ってください。' },
  M12: { chip: '× 送信失敗', sentence: 'USB ケーブルを確かめてから、もう一度送ってください。' },
  M13: { sentence: 'パッケージファイルをこの画面に置くか、選んでください。' },
  /** ready with a build to write: the main action writes it. */
  M13built: { sentence: '書き込むノードにチェックを入れて、『書き込む』を押してください。' },
  /** M14 / M15 / M28: label-value pair, no sentence. */
  M14: { pair: ['目安', '約 1 分'] },
  M18: { sentence: '何も書いていないので、準備でもう一度ビルドするか、正しいファイルを選んでください。' },
  M18b: { sentence: '何も書いていないので、別のファイルを選んでください。' },
  M19: { sentence: '何も書いていないので、別のファイルを選んでください。' },
  M20: { sentence: '何も書いていないので、この構成向けのファイルを選んでください。' },
  M22: { sentence: '終わるまで電源と USB を抜かず、このページを開いたままにしてください。途中で止まっても、ノードは前のバージョンのまま動きます。' },
  M33: { sentence: '基板どうしの配線が抜けていないか確かめてから、もう一度試してください。' },
  M34: { sentence: 'USB ケーブルを挿し直してから、もう一度つないでください。' },
  M35: { chip: '× 作成失敗', sentence: 'そのフォルダに書き込めるか確かめてから、もう一度選んでください。' },
  /** M58: "最新のリリースを使う" found no release in the project. */
  M58: { chip: '× ビルドなし', sentence: '準備でビルドするか、パッケージファイルを選んでください。' },
  /** M57: part of the page (dagon-wasm) did not load. */
  M57: { chip: '× 読み込み失敗', sentence: 'このページの一部を読み込めませんでした。ページを開き直してください。' },
  /** M26: reconnectNeeded (the dialog's sentence, and the band's once the dialog is closed). */
  M26: { sentence: '『もう一度つなぐ』を押して、同じ機器を選んでください。' },
  M59: { sentence: 'そのまま待ってください（数秒で終わります）。' },
  M60: { sentence: '見つかったノードが降りてくるまで、そのまま待ってください。' },
  M61: { sentence: '送り終わるまで、USB を抜かないでください。' },
  /**
   * The answering nodes are updated first, a silent one from its card:
   * nodesSilent, with a power and wiring hint for the silent ones.
   */
  M62b: { sentence: '応答しないノードは電源と配線を確かめ、応答するノードから先に書き込んでください。' },
  /** M62: nodesSilent when the answering nodes need nothing more (already updated, or after 「応答する n つだけ先に書き込む」). */
  M62: { pair: ['目安', '約 1 分'], sentence: '印の付いたノードは、書き込むと元どおりに動きます。' },
  M63: { sentence: '確かめ終わるまで、そのまま待ってください。' },
  M64: { sentence: '親機が再起動したら自動でつなぎ直すので、そのまま待ってください。' },
  M65: { pair: ['目安', '約 1 分'], sentence: '止まったノードへ書き込むと、新しいバージョンで動きます。' },
  M66: { sentence: 'すべてのノードへ書き込みました。' },
  M66checked: { sentence: 'チェックしたノードへ書き込みました。' },
  M66none: { sentence: 'チェックしたノードはすでにこのパッケージの内容なので、書き込みませんでした。' },
  M67: { sentence: 'CLI の作業が終わると、自動でつなぎ直します。' },
} as const satisfies Record<string, BandCopy>;

/**
 * The header's つなぐ / 切断 and why it cannot be
 * used in a state (the band says it when it is pressed then).
 */
export const HUB_TOGGLE = {
  connect: '接続する',
  disconnect: '切断する',
  locked: {
    choosing: '一覧で選び終わるまで、そのまま待ってください。',
    scanning: '探し終わるまで、切断できません。',
    sending: '送り終わるまで、切断できません。',
    writing: '書き込みが終わるまで、切断できません。',
    hubRestarting: '親機とつなぎ直すまで、切断できません。',
    reconnectNeeded: '親機とつなぎ直すまで、切断できません。',
    recovering: '書き込みが終わるまで、切断できません。',
    cliTurn: 'CLI の作業が終わるまで、切断できません。',
  } as Partial<Record<string, string>>,
} as const;

/** Elapsed and remaining time under the dot number while updating or recovering. */
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

/** lost's cause chip when the device the CLI gave back was not this 親機. */
export const BAND_CHIP = {
  otherHub: '× 別の親機',
  notHub: '× 親機以外',
  silent: '× 応答なし',
} as const;

/** M29: beside the recover button when nodes after the stopped one are not written yet. */
export const BAND_FOR = {
  M29: (stuck: string) => ({ pair: ['目安', '約 1 分'] as const, sentence: `${stuck}を直すと、残りのノードへ書き込めます。` }),
  /** done when M66 is not true: the nodes that did not answer were not updated. */
  M66some: (n: number) => ({ sentence: `応答しなかった ${n} つのノードは前のままなので、電源と配線を確かめてから書き込んでください。` }),
  /** done after 「前のバージョンに戻す」. */
  /** done when some nodes were already on the package. */
  M66part: (wrote: number, same: number) => ({ sentence: `${wrote} つのノードへ書き込み、${same} つのノードはすでに同じ内容なので、書き込みませんでした。` }),
  M66back: (name: string, version: string) => ({ sentence: `${name}は前のバージョン ${version} で動いています。` }),
  /** done after a recovery with nothing left to update. */
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

/**
 * The state-lock table: each state's
 * short name for the status line and, per operation it stops, the reason
 * shown when that operation is pressed (information style, never red).
 */
export const LOCK = {
  controls: {
    edit: '構成の編集',
    gen: '雛形を作る',
    build: 'ビルド',
    flash: '書き込み',
    release: 'リリース',
    update: '書き込み',
    recover: '書き込み',
    rollback: '前のバージョンに戻す',
    connect: 'つなぐ・切断',
    project: 'プロジェクトの切替',
    open: 'プロジェクトを開く',
    ask: 'DAGON に聞く',
    apply: 'DAGON の案を反映',
    link: 'DAGON と連携',
    file: 'パッケージファイルを置く',
  },
  idle: '待機',
  stateLabel: '今の状態',
  stateTitle: '今の状態。使えない操作は押すと理由を表示します',
  states: {
    demo: {
      name: 'デモの再生中',
      why: {
        link: 'デモを再生しています',
        edit: 'デモを再生しています。「■ 止める」を押すと操作できます',
        gen: 'デモを再生しています',
        build: 'デモを再生しています',
        flash: 'デモを再生しています',
        release: 'デモを再生しています',
        update: 'デモを再生しています',
        recover: 'デモを再生しています',
        rollback: 'デモを再生しています',
        connect: 'デモを再生しています',
        project: 'デモを再生しています',
        apply: 'デモを再生しています',
        file: 'デモを再生しています',
      },
    },
    updating: {
      name: '書き込み中',
      why: {
        edit: '送った構成と食い違うので、書き込み中は構成を変えられません',
        gen: '書き込みが終わるまで使えません',
        build: '書き込みが終わるまで使えません',
        flash: '書き込みで親機を使っているので、書き込めません',
        release: '書き込みが終わるまで使えません',
        update: '書き込んでいます。終わるまでお待ちください',
        recover: '書き込みが終わってから使えます',
        rollback: '書き込みが終わってから使えます',
        connect: '書き込み中に切断すると、書いている途中のノードは前のバージョンのまま残ります。終わるまでお待ちください',
        project: '書き込みが終わるまで使えません',
        apply: '書き込み中は構成を変えられません',
        file: '書き込みが終わるまで使えません',
      },
    },
    recovering: {
      name: '書き込み中',
      why: {
        edit: '書き込み中は構成を変えられません',
        flash: '親機を使っているので、書き込めません',
        update: '書き込みが終わってから使えます',
        recover: '書き込んでいます',
        rollback: '書き込みが終わってから使えます',
        connect: '書き込み中に切断すると、書き込みが止まります',
        project: '書き込みが終わるまで使えません',
        apply: '書き込み中は構成を変えられません',
        file: '書き込みが終わるまで使えません',
      },
    },
    flashing: {
      name: '書き込み中',
      why: {
        edit: '書いている構成と食い違うので、書き込み中は構成を変えられません',
        gen: '書き込みが終わるまで使えません',
        build: '書き込みが終わるまで使えません',
        flash: '書き込んでいます',
        release: '書き込みが終わるまで使えません',
        update: '書き込みが終わってから使えます',
        recover: '書き込みが終わってから使えます',
        rollback: '書き込みが終わってから使えます',
        connect: '書き込みで USB を使っているので、切断できません',
        project: '書き込みが終わるまで使えません',
        apply: '書き込みが終わるまで使えません',
      },
    },
    building: {
      name: 'ビルド中',
      why: {
        edit: 'ビルドで構成を使っているので、ビルド中は構成を変えられません',
        gen: 'ビルドが終わってから使えます',
        build: 'ビルドしています',
        flash: 'ビルドが終わってから使えます',
        release: 'ビルドが終わってから使えます',
        update: 'ビルドが終わってから使えます',
        project: 'ビルドが終わるまで使えません',
        apply: 'ビルド中は構成を変えられません',
      },
    },
    unsaved: {
      name: '保存中',
      why: {
        gen: '構成を保存しています。0.5 秒ほどで終わります',
        build: '構成を保存しています。0.5 秒ほどで終わります',
        flash: '構成を保存しています',
        release: '構成を保存しています',
        update: '保存した構成で書き込むので、保存が終わるまでお待ちください',
        project: '保存が終わってから切り替えられます',
      },
    },
    connecting: {
      name: '接続の途中',
      why: {
        connect: '親機とつないでいる途中です',
        update: '親機とつないでいる途中です',
        recover: '親機とつないでいる途中です',
        rollback: '親機とつないでいる途中です',
        file: '親機とつないでいる途中です',
      },
    },
    disconnected: {
      name: '未接続',
      why: {
        update: '親機とつないでいません。「親機をつなぐ」を押してください',
        recover: '親機とつないでいません',
        rollback: '親機とつないでいません',
      },
    },
    thinking: {
      name: 'DAGON の返事待ち',
      why: {
        ask: 'DAGON の返事を待っています',
        apply: 'DAGON の返事を待っています',
      },
    },
    /** The public demo page: only the simulator in this browser; the rest needs dagon serve on the user's PC. */
    site: {
      name: 'デモ',
      why: {
        gen: 'このデモはファイルを作りません。手元の PC で dagon serve を起動すると使えます',
        build: 'このデモはビルドしません。手元の PC で dagon serve を起動すると使えます',
        flash: 'このデモは実機につなぎません。手元の PC で dagon serve を起動すると使えます',
        release: 'このデモはリリースを作りません。手元の PC で dagon serve を起動すると使えます',
        open: 'このデモはファイルを開いたり保存したりしません。手元の PC で dagon serve を起動すると使えます',
        ask: 'このデモは AI につなぎません。手元の PC で dagon serve を起動すると使えます',
        apply: 'このデモは AI につなぎません。手元の PC で dagon serve を起動すると使えます',
        link: 'このデモは鍵を扱いません。手元の PC で dagon serve を起動すると使えます',
        file: 'このデモはファイルを読みません。用意してあるパッケージファイルで書き込めます',
      },
    },
    cliNone: {
      name: 'CLI 未接続',
      why: {
        link: '鍵は CLI に置くので、先にターミナルで dagon serve を起動してください',
        gen: 'CLI とつながっていません。ターミナルで dagon serve を起動してください',
        build: 'CLI とつながっていません',
        flash: 'CLI とつながっていません',
        release: 'CLI とつながっていません',
        ask: 'DAGON は CLI を通して話すので、CLI とつながっていないと使えません',
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
  menuLink: 'DAGON と連携する',
  menuTalk: 'DAGON と話す',
  menuUnlink: 'DAGON との連携を外す',
  link: {
    heading: 'DAGON と連携する',
    intro: 'DAGON は連携したエージェントに構成について問い合わせて答えます。送ることへの同意は、この連携のときに一度だけいただきます。連携したあとの質問は確かめずに送ります。',
    agent: '送り先',
    agentNote: '手元の dagon serve を通して送ります',
    way: 'つなぎ方',
    /** What each kind of way needs on this machine (the provider is named only here, at linking). */
    ready: '準備',
    wayNote: {
      command: 'このパソコンでログインしたエージェントのプログラムを使います。ログインの情報はこのページにも送る文にも入りません',
      'messages-http': 'API キーを「CLI 連携」で登録するか、ターミナルで dagon ai key set を実行してください。キーは OS の資格情報ストアに置き、このページにも送る文にも入りません',
    } as Record<string, string>,
    wayMissing: 'このつなぎ方は、このパソコンでは使えません。連携は先に済ませておけます。',
    sends: '送るもの',
    neverSends: '送らないもの',
    undo: '連携は、メニューの「DAGON との連携を外す」でいつでも外せます。外すと同意も取り消します。',
    agree: '同意して連携する',
    cancel: '連携をやめる',
  },
  linked: (provider: string, at: string) => `DAGON と連携した。送り先（${provider}）と送る中身への同意を記録した（${at}）`,
  unlinked: 'DAGON との連携を外し、同意を取り消した。API キーは「CLI 連携」で削除できる',
  demoBubble: 'デモでは会話できません。dagon serve で連携すると相談できます。',
  hello: '選んだノードやリンクについて聞けます。/ を押すと入力欄に移ります。',
  thinking: '返事を待っています…',
  placeholder: '質問を入力（/ キーでここに移ります）',
  send: '質問を送る',
  sendWhy: '質問を入力すると送れます',
  questions: [
    ['problem', '問題を聞く'],
    ['next', '次の手順を聞く'],
    ['propose', '変更案を頼む'],
    ['failure', '失敗の理由を聞く'],
  ] as const,
  showDiff: '差分を見る',
  apply: '案を構成に反映する',
  applied: '反映した。「元に戻す」（Ctrl+Z）で戻せる',
  allTalk: '会話をすべて見る（下の「会話」タブ）',
  diffCaption: '変更の案（反映したあとも「元に戻す」で戻せます）',
  talkNone: '会話はありません。DAGON の吹き出しか / キーで質問してください。',
  you: 'あなた',
  markOpen: 'クリックで開く',
  markClose: 'クリックで畳む',
  question: 'DAGON への質問',
  errors: {
    AGENT_NOT_LINKED: { text: 'DAGON と連携していません。メニューの「DAGON と連携する」から連携してください。', failed: false },
    AGENT_UNAVAILABLE: { text: '連携した方法がこのパソコンで使えません。連携の手順に書いた準備（プログラムを入れる、API キーを登録する）をしてください', failed: false },
    /** The OS credential store cannot be used: the text is serve's reason (what is missing and how to start one). */
    AGENT_KEY_STORE: { label: '× 保存できない', text: 'OS の資格情報ストアを使えないため、API キーを保存できません', failed: true },
    KEY_REFUSED: { label: '× 受け付けない', text: 'API キーは空白を含まない英数字と記号で、1024 文字までです', failed: true },
    AGENT_BUSY: { text: 'DAGON はほかの質問に答えています。少し待ってから聞いてください', failed: false },
    AGENT_TIMEOUT: { text: '× 返事が時間内に届きませんでした。もう一度聞いてください', failed: true },
    AGENT_FAILED: { text: '× 返事を受け取れませんでした。もう一度聞いてください', failed: true },
    DEMO: { text: 'デモでは DAGON と連携できません', failed: false },
    OLD_CLI: { text: 'この CLI は DAGON に対応していません。新しい dagon で dagon serve を起動し直してください', failed: false },
    AGENT_CONSENT_VERSION: {
      label: '× 利用規約の確認',
      text: '利用規約が更新されています。連携のために確認してください。',
      failed: true,
    },
    AGENT_CONTEXT_TOO_LARGE: {
      label: '× 長すぎる',
      text: '文字数が多すぎて送れません',
      failed: true,
    },
  },
} as const;

/** The bottom drawer and its tabs. */
/** The drawer's timeline of an update. */
export const TIMELINE = {
  label: '書き込みのタイムライン',
  time: '時間',
  none: '書き込みを始めると、ノードごとの段階がここに時間の順で並びます。',
} as const;

/** The status row. */
export const STATUS = {
  label: '状態',
  key: '鍵',
  keyTitle: 'リリースを確かめる鍵の ID',
  progress: '進み具合',
  saving: '保存中…',
  saved: '保存済み',
} as const;

/** The canvas controls bottom-right. */
export const ZOOM = {
  label: '表示の操作',
  out: 'キャンバスを縮小する',
  fit: 'キャンバス全体を表示する',
  in: 'キャンバスを拡大する',
  maxWhy: 'これ以上大きくできません',
  minWhy: 'これ以上小さくできません',
} as const;

/** The parts window. */
export const PARTS = {
  heading: '部品',
  hint: 'ドラッグで置く',
  close: '部品を閉じる',
  open: '部品を開く',
} as const;

/** The floating sheet's spoken name, and its close button. */
export const SHEET = '選んだものの詳細';
export const SHEET_CLOSE = 'シートを閉じる';

export const DRAWER = {
  label: '下の引き出し',
  problems: '問題',
  log: '記録',
  talk: '会話',
  logNone: '記録はありません。',
  close: 'パネルを閉じる',
  jobs: '作業',
  output: '出力',
  timeline: 'タイムライン',
  outputNone: 'まだ出力はありません。',
  /** The handle on the open drawer's top edge. */
  resize: 'パネルの高さを変える（ドラッグ）',
} as const;

/** Canvas: uplink cards, the empty canvas, dot numbers. */
export const CANVAS = {
  /** Uplink card of the 親機 this page is connected to now. */
  pc: 'このパソコン（ブラウザ）',
  /** Uplink card by each 親機's uplink host as the 構成 writes it (none written: dagon on a PC). */
  pcBy: { 'pc-cli': 'パソコン（dagon）', browser: 'ブラウザ（パソコン・スマホ）', embedded: '組込み機器' } as Record<string, string>,
  /** C02 */
  empty: '親機を USB でパソコンにつないでから、『つないで、自動で見つける』を押してください。',
  /** After the uplink card of a 親機 this page is not connected to. */
  notThisPc: '未接続',
  /** S17: canvas description for screen readers. */
  describe: '矢印でノードを移り、Enter で選びます。ほかのキーは「困ったとき」の「キー操作」にあります。',
  /** M49 dot-number labels. */
  dotResponding: '応答',
  dotFound: '発見',
  /** M49 during an update, and after it stopped. */
  dotProgress: '全ノードの進み具合',
  dotPercent: '%',
  dotStopped: '% 停止',
  dotProgressName: (n: number, stopped: boolean) => `全ノードの進み具合 ${n}%${stopped ? '、停止' : ''}`,
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
  /** The hub's own connection line: its uplink kind and what drives it. */
  usb: 'USB（パソコン）',
  uplinkVia: (kind: string, host: string) =>
    `${kind}（${({ 'pc-cli': 'パソコン', browser: 'ブラウザ', embedded: '組込み機器' } as Record<string, string>)[host] ?? 'パソコン'}）`,
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
  unchanged: '✓ 書き込み不要',
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
    E_COMPAT: '× 機種違い',
    E_FLASH: '× 書き込み失敗',
    // dagon-host's release check, in the words of the update file's verdict.
    source: '× 配布元不明',
    key: '× 配布元不明',
    topology: '× 構成不一致',
    format: '× 形式違い',
  } as Record<string, string>,
  refusedOther: '× 受付拒否',
  /** S04 during a recovery. */
  lastResort: '◎ 最後の手段',
  cannotFix: '× 書き込めず',
  /** W7: full spoken state of a chip. */
  respondingName: '応答あり',
  silentName: '応答なし、2 回続けて返事がない',
  trialName: '確認待ち',
  /** Spoken name of a node: the visible short chips spelled out. */
  name: (p: { name: string; product: string; mcu: string; role: string; via: string; state: string; version: string }) =>
    [p.name, `${p.product} · ${p.mcu}`, p.role, p.via, p.state, p.version && `バージョン ${p.version}`].filter(Boolean).join('、'),
  viaName: (via: string) => `親機と ${via}`,
  /** A node updating does not reach yet: a calm note, not a fault. */
  notYet: '◇ 書き込めない',
  notYetWhy: {
    'second-hub': '2 つ目の親機の側にあるため',
    deep: '子機の先にあるため',
    'link-kind': 'このつながりの種類では、書き込みが届かないため',
    unreachable: 'どの親機からもリンクがたどれないため',
    'cut-invalid': '書き込みが届く部分だけでは確認を通らないため（dagon topo check で理由を見られます）',
  } as Record<string, string>,
  notYetName: (why: string) => `書き込めない、${why}`,
} as const;

/** Link chip and its spoken name. */
export const LINK = {
  /** CAN and Ethernet carry no wires the page draws: the kind alone. */
  /** The bus, the value it needs (address or speed: ), its wires. */
  bundle: (kind: string, n: number, shared: number, value = '') => {
    const head = value ? `${kind} ${value}` : kind;
    return n ? `${head} · ${n} 本${shared ? `（うち共有 ${shared}）` : ''}` : head;
  },
  /** The lower port of a card. */
  portOut: (name: string) => `${name} の下のポート（ここから子のノードへドラッグ）`,
  name: (label: string, from: string, to: string, addr: string) => `${label}、${from}から ${to}へのリンク${addr ? `、${addr}` : ''}`,
} as const;

/** Why a port drag is refused (a failure: red). */
export const LINK_WHY = {
  taken: 'このノードはもう親機とつながっています',
  under: (name: string) => `このノードはもう ${name} の下です`,
  loop: 'このノードの下にあるノードにはつなげません',
} as const;

/** Right column. */
export const DETAIL = {
  /** M40 */
  nothingSelected: 'ノードを選ぶと、ここに詳細が出ます。',
  /** M42 */
  introHeading: 'はじめに',
  /** M42: the same three steps as the header. */
  introSteps: ['つなぐ', 'ファイル', '書き込み'],
  introNote: '止まっても 書き直せる',
  /** S36 */
  rtt: '往復',
  lastSeen: '最後の応答',
  attempts: '試した回数',
  lastReason: '最後に戻った理由',
  ms: (n: number) => `${n} ms`,
  none: 'なし',
  /** A version the node did not tell (VERSION left it out, or the node predates VERSION). */
  unknown: '不明',
  /** Values of the last-reason code (STATUS last_reason). */
  reasons: { 1: '中身の破損', 2: '配布元不明', 3: '起動失敗', 5: '起動できるバージョンなし' } as Record<number, string>,
  details: '詳しく見る',
} as const;

/** M38: a node on trial that this page did not apply (the detail, above its buttons). */
export const TRIAL_CHOICE = {
  sentence: 'このバージョンのままにするか、前のバージョンに戻すかを選んでください。',
  keep: 'このバージョンのままにする',
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

/** Slot section. Inside it the rows are A / B / 予備. */
export const SLOT = {
  heading: 'スロット',
  /** The boot loader has its own two slots (dagon_proto VERSION's boot core slots). */
  coreHeading: 'ブートローダのスロット',
  rows: ['A', 'B', '予備'],
  coreRows: ['A', 'B'],
  /** Slot state u8 → word, in general words: what the slot holds now. */
  states: ['空', '書き込み済み', '次に起動', '確認待ち', '確認済み', '戻された'],
  inUse: 'いま動作中',
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
} as const;

/** The file check: the file and dagon-host's verdict on it (the one place for the verdict). */
export const FILE = {
  heading: '書き込む',
  /** File input label (the drop place). */
  choose: 'パッケージファイルを選ぶ',
  size: (bytes: number) => `${Math.max(1, Math.round(bytes / 1024))} KB`,
  sourceBad: '× 配布元不明',
  topologyBad: '× 構成不一致',
  notRelease: '× 形式違い',
} as const;

/** The hub reconnect dialog, and M27 beside the hub while the page reconnects by itself (hubRestarting). */
export const RECONNECT = {
  heading: '親機の再接続',
  sentence: BAND.M26.sentence,
  auto: '自動の再接続',
  left: (s: number) => `あと ${s} s`,
  /** why the page could not reconnect by itself (the diagnostic log under 詳しく見る). */
  why: '詳しく見る',
  notes: {
    away: '前の親機 使用不可',
    none: '同じ種類の機器 なし',
    several: (n: number) => `同じ種類の機器 ${n} つ（どれか決められない）`,
    silent: '現れた機器 応答なし',
    notHub: '現れた機器 子機',
    otherHub: '現れた機器 別の親機',
    version: (v: string, trial: boolean) => `現れた親機 ${v}${trial ? '' : '（前のバージョン）'}`,
  },
} as const;

/** Help in the header, and the operating-mode key list. */
export const HELP = {
  heading: '困ったとき',
  close: '困ったときを閉じる',
  /** Tool help, independent of any project: what the user sees, its real cause, and what to do. */
  rows: [
    ['「接続する」を押しても機器の一覧が空', 'パソコンが親機を USB 機器として見ていません。充電専用のケーブルはデータが通らないので、データ用のケーブルに替え、別の USB 口も試してください。'],
    ['一覧に親機が出ない', '親機に DAGON がまだ書き込まれていないか、電源が入っていません。初めての親機は書込の「書き込む」から USB で直接書き込みます。'],
    ['接続しても「応答なし」のノードがある', '親機からそのノードへの配線か電源が切れています。配線表（表の「配線」）と同じにつながっているか確かめてください。'],
    ['「ビルドする」が使えない', '構成に問題が残っているか、dagon serve が動いていません。下の「問題」を直し、ターミナルで dagon serve を起動してください。'],
    ['「書き込む」が使えない', 'まだビルドしていないか、親機とつないでいません。準備で「ビルドする」、書込で「接続する」の順です。'],
    ['書き込みが止まって見える', '1 つのノードに 1 分ほどかかります。カードの % が増えていれば進んでいます。途中で止まっても、ノードは前のバージョンのまま動きます。'],
    ['書き込み後にノードが前のバージョンに戻った', '新しいバージョンが起動を続けられず、DAGON が自動で前のバージョンに戻しました。アプリを直してバージョンを上げ、もう一度書き込んでください。'],
  ],
  keysHeading: 'キー操作',
  keysClose: 'キー操作を閉じる',
  keys: [
    ['Tab', 'ノードへ移る'],
    ['矢印', '隣のノードへ'],
    ['Enter', '選ぶ'],
    ['Esc', 'シート・メニュー・一覧を閉じる'],
    ['Delete', '選んだノードやリンクを消す（確認してから）'],
    ['Ctrl + Z', '元に戻す'],
    ['Ctrl + Shift + Z', 'やり直す'],
    ['Ctrl + J', '下のパネルを開く・閉じる'],
    ['L', '選んだノードのつながる親を選ぶ'],
    ['F', 'キャンバス全体を表示する'],
    ['Ctrl + ホイール', 'キャンバスを拡大・縮小する'],
    ['ドラッグ', 'キャンバスやノードを動かす'],
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
  A22: (define: boolean) => (define ? '準備モード' : '書込モード'),
  A09: (name: string, size: string) => `${name} ${size}、確認中`,
  A10ok: (n: number, skipped = 0) => `配布元 確認済み、構成 一致、対象 ${n}${skipped ? `、書き込めない ${skipped} つは飛ばす` : ''}`,
  A10source: '配布元 確認できない',
  A10topology: '構成 不一致',
  A10format: 'パッケージファイルではない',
  A11: (name: string) => `${name} 書き込み`,
  A11percent: (n: number) => `進み具合 ${n}%`,
  A12: '親機 再起動中、自動で再接続',
  A13: (name: string) => `書き込み 停止、${name}`,
  A13core: (name: string) => `${name} ブートローダは元のまま`,
  A13previous: (name: string) => `${name} 前のバージョン`,
  A14: (version: string) => `完了、${version}`,
  A21timeout: '親機 再接続 時間切れ',
  A15: (name: string, version: string) => `${name} ${version} に戻した`,
  A16: (name: string, rest: number) => `${name} 書き込み済み、残り ${rest}`,
  /** "最新のリリースを使う" found no release in the project. */
  A22latest: 'リリース なし',
} as const;

/** Words that never appear in operating-mode guidance, chips or results. */
export const FORBIDDEN_WORDS = [
  '署名', 'ROM', 'CDC', 'ブートコア', 'エラーコード', 'トポロジ',
  'マニフェスト', 'ハッシュ', 'ポート', 'シリアル', 'フラッシュ', 'ファームウェア',
  'パーティション', 'コマンド', 'タイムアウト', 'WASM', 'Web Serial', '確定', '試行中',
  '巻き戻り', '不正', '致命的', '無効', '復旧の層', 'ブートコア',
] as const;

/** S07: short names of the check rules (problem chips and the problem list). */
export const RULE_SHORT: Record<string, string> = {
  'V-01': 'ファイルの形式',
  'V-02': '名前の重なりとノードの数',
  'V-03': '親機の数',
  'V-04': '子機どうしのリンク',
  'V-05': 'つながっていないノード',
  'V-06': '起動の方式',
  'V-07': '定義に無いピン',
  'V-08': 'ピンの重なり',
  'V-09': 'UART のピン',
  'V-10': 'UART の数',
  'V-11': '起動の切り替え方',
  'V-12': '直し方の順番',
  'V-13': '最後の手段のピン',
  'V-14': '書き込みの順番',
  'V-15': '互換の決まり',
  'V-16': 'フラッシュの大きさ',
  'V-17': 'I2C のピン',
  'V-18': 'アドレスの重なり',
  'V-20': 'I2C の速さ',
  'V-21': 'I2C でつなげないノード',
  'V-22': 'リセット・起動の配線 未設定',
  'V-23': '書き込めない部分',
  'V-24': '使えないつながり',
  'V-25': '親機にできないマイコン',
  'V-26': 'UART の速さ',
  /** Checked by the page (dagon.toml's [board.<id>]), not by dagon-topology. */
  'B-01': 'ボード 未設定',
  'B-02': '容量不足',
};

/** Names a fix sentence needs: the node (呼び名), its board, its id, and whether there are too many nodes. */
export interface FixWords {
  to: string;
  board: string;
  id: string;
  many: boolean;
  /** The node limit. */
  max: number;
  /** A size such as "512 KiB". */
  size?: string;
  /** The node is one updating skips: its link is not a thing to fix. */
  skipped?: boolean;
  /** For the fix sentences, read from what the check reads: the pins the pin's own node defines, the pairs its ROM loader listens on, the speeds every node on the bus names. */
  pins?: string;
  pairs?: string;
  hzs?: string;
}

/** How to fix each rule, one sentence. */
export const RULE_FIX: Record<string, (w: FixWords) => string> = {
  'V-01': () => 'dagon の新しいバージョンで書き出したファイルを読み込んでください。',
  'V-02': (w) => (w.many ? `ノードを ${w.max} までにしてください。` : `名前「${w.id}」の 2 つのどちらかを変えてください。`),
  'V-03': () => '親機を 1 つだけにしてください。',
  'V-04': () => '子機どうしのリンクを消し、親機とつないでください。',
  'V-05': (w) => `${w.to}を親機とリンクでつないでください。`,
  'V-06': (w) => `『直す』で ${w.board} に合う起動の方式にしてください。`,
  'V-07': (w) => `そのノードの定義にあるピン（${w.pins ?? '定義のピン'}）のどれかに変えてください。`,
  'V-08': () => '重なったピンのどちらか 1 か所を、空いているピンに変えてください。',
  'V-09': () => '『直す』で親機のもう一方の UART に替えてください。',
  'V-10': () => 'UART のリンクを 1 本減らしてください。',
  'V-11': (w) => `${w.to}の起動の切り替え方を選んでください（最後の手段で直すときに使います）。`,
  'V-12': (w) => `${w.to}の直し方の順番から、最後の手段を外してください。`,
  'V-13': (w) => `${w.to}を、そのボードの I2C の組（${w.pairs ?? '定義の組'}）のどれかにつないでください。`,
  'V-14': () => '『直す』で親機を最後にして並べてください。',
  'V-15': () => '互換の決まりから、無いノードを外してください。',
  'V-16': () => '既定の配置に戻してください。',
  'V-17': (w) => `共有の 2 本線（I2C）を親機の定義にあるピン（${w.pins ?? '定義のピン'}）にしてください。`,
  'V-18': (w) => `${w.to}のアドレスを、空いている番号に変えてください。`,
  'V-20': (w) => `共有の 2 本線（I2C）の速さを、つながるノードの定義にあるもの（${w.hzs ?? '定義の速さ'}）のどれかにしてください。`,
  'V-21': (w) => `${w.to}は UART でつないでください。`,
  'V-22': (w) => `最後の手段で復旧するには、『直す』で ${w.to}のリセット・起動の配線を設定してください（このままでも親機へ送れます）。`,
  'V-23': () => 'この構成のままで正しく使えます。いまの書き込みはこの部分に届かないため、書き込みではこの部分を飛ばします。',
  'V-24': (w) =>
    w.skipped
      ? `${w.to}のつながりは、使えない種類です。${w.to}は書き込みで飛ばすので、このままで使えます。`
      : `${w.to ? `${w.to}の` : ''}つながりは、まだ使えない種類です。プロジェクトを作るには、使える種類を選んでください（このままでも保存できます）。`,
  'V-25': (w) => `${w.to}の代わりに、親機にできるマイコンのノードを親機にしてください。`,
  'V-26': () => 'UART の速さを、つなぐ両方のマイコンで誤差が 2 % 以内になる値にしてください。',
  'B-01': (w) => `『直す』で ${w.to}のボードの容量を入れてください。`,
  'B-02': (w) => `${w.to}は容量が ${w.size ?? ''} 以上のボードにしてください。`,
};

/** Header: the two modes (準備｜書込). */
export const MODE = {
  operate: '書込',
  define: '準備',
  label: 'モード',
  /** Why a mode cannot be chosen, on hover. */
  offWhy: { locked: '書き込みが終わるまで、モードは変えられません', operate: '構成を作り、問題を直すと、書込を使えます' },
  /** 書込 or 親機へ送る pressed while the 構成 is not in the project's YAML. */
  lockedUnsaved: { sentence: '構成を保存すると、書込と親機へ送るを使えます。' },
  /** M69: 書込 pressed with no 構成 or one with problems. */
  lockedProblems: { sentence: '構成を作り、問題を直すと、書込を使えます。' },
} as const;

/** Define mode. */
export const DEFINE = {
  /** The parts column. */
  listHeading: 'ノードリスト',
  /** Parts column: choose by product or by MCU. */
  byProduct: '製品',
  byMcu: 'マイコン',
  chooseBy: '選び方',
  listHelp: '選んで Enter で置けます。',
  place: '置く',
  /** The parts window. */
  search: '製品・マイコンを探す',
  byMcuOwn: 'マイコン',
  ownBoard: 'ボード',
  count: (n: number) => `${n} 個`,
  placed: '配置済み',
  firstHeading: 'はじめに',
  connectDiscover: '親機を探す',
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
  dotProblems: '問題',
  dotNotYet: '◇ 未対応',
  dotProblemsName: (p: number, n: number, l: number) => `問題 ${p}、ノード ${n}、リンク ${l}`,
  /** S25: nothing selected and nothing to fix. */
  nothingSelected: 'ノードかリンクを選ぶと、ここで直せます。',
  /** The problem list. */
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
  /** Bottom band. */
  emptySentence: '親機を USB でパソコンにつないでから、『つないで、自動で見つける』を押してください。',
  /** E5: 書込 from 準備. */
  toOperate: '書込へ',
  /** Writes the 構成 back to the project's YAML through dagon serve. */
  save: '保存',
  fixSentence: '直すと、ビルドできます。',
  readySentence: 'ビルドしてから、書込で書き込みます。',
  buildingSentence: 'ビルドが終わるまで、そのまま待ってください。',
  builtSentence: '書込に切り替えて、ノードへ書き込みます。',
  skeletonFirst: '先に雛形を作ると、ビルドできます。',
  /** Nodes updating skips; the 構成 is valid as it is. */
  notYetSentence: (k: number) => `書き込めないノードが ${k} つあります。構成はこのままで使え、書き込みではそのノードを飛ばします。`,
  /** The 構成 of the release file chosen in 書込 replaces this one (undoable, saved to the YAML). */
  importFileTopology: 'ファイルの構成を取り込む',
  sendSentence: '親機へ送ると、書き込めるようになります。',
  send: '親機へ送る',
  connectSend: 'つないで、親機へ送る',
  unreadableChip: '× 形式違い',
  unreadableSentence: 'dagon で書き出した .yaml ファイルを選んでください。',
  /** GET /v1/definitions 500: the open project's MCU/board definitions do not load (serve's terminal says why). */
  defsChip: '× 定義エラー',
  defsSentence: 'このプロジェクトの MCU・ボード定義を読み込めません。理由は dagon serve の端末に出ています。',
} as const;

/** Inspector labels; 詳しい設定 shows the schema names as they are. */
export const INSPECT = {
  name: '呼び名',
  id: 'ID（英数字）',
  role: '役割',
  roles: { coordinator: '親機', node: '子機' } as Record<string, string>,
  product: '製品名・MCU',
  connectTo: 'つながる親',
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
  uartTx: (from: string) => `${from}の TX ピン`,
  uartRx: (from: string) => `${from}の RX ピン`,
  /** A value from a file that the definition does not name: kept, marked, refused by the check. */
  undefinedPin: '（定義にない）',
  maxAttempts: '起動を試す回数',
  listenWindow: '返事を待つ時間（ミリ秒）',
  flashMap: 'メモリの配置',
  tag: '目印',
  baud: '速さ（ボーレート）',
  /** No speed both ends' definitions name: the link cannot be set up with these MCUs. */
  noBaud: 'この 2 つのマイコンに共通の速さがありません',
  busSda: 'SDA のピン（親機）',
  busScl: 'SCL のピン（親機）',
  removeLink: 'このリンクを消す',
  removeNode: 'このノードを消す',
  bus: '共有の 2 本線（I2C）',
  busPins: 'ピン',
  speed: '速さ',
  /** No speed every node's definition names: the bus cannot be set up with these MCUs. */
  noSpeed: 'このバスのマイコンに共通の速さがありません',
  /** A bus speed as the user reads it. */
  hz: (hz: number) => `${hz / 1000} kHz`,
  pullups: 'プルアップ',
  pullupKinds: { external: '外付け', board: '基板上' } as Record<string, string>,
  inUse: (what: string) => `（使用中: ${what}）`,
  /** The board of a node chosen by MCU. */
  board: 'ボード',
  boardOwn: 'ボード定義なし',
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
  /** The 親機's hop towards the PC. */
  uplink: '接続元',
  uplinkHost: '接続元の機器',
  hosts: { 'pc-cli': 'パソコンの dagon', browser: 'ブラウザ', embedded: '組込み機器' } as Record<string, string>,
  /** On a kind the firmware does not implement yet. */
  notYet: '未対応',
  notYetOption: (label: string) => `${label}（未対応）`,
} as const;

/** The confirmation before anything is deleted (Delete key, trash button, the table's 消す). */
export const CONFIRM = {
  node: (name: string) => `${name}を消す`,
  link: (from: string, to: string) => `${from} → ${to} のリンクを消す`,
  nodeSentence: 'このノードと、つながるリンクを構成から消します。元に戻す（Ctrl+Z）で戻せます。',
  linkSentence: 'このリンクを構成から消します。元に戻す（Ctrl+Z）で戻せます。',
  remove: '消す',
  cancel: 'やめる',
  /** The other destructive actions: the same modal, their own words. */
  unlink: { title: 'DAGON との連携を外す', sentence: 'AI のエージェントとのつながりと、送ることへの同意を取り消します。質問はできなくなります。', label: '連携を外す' },
  key: { title: 'API キーを削除する', sentence: '登録した API キーを OS の資格情報ストアから消します。もう一度使うには登録し直します。', label: '削除する' },
  rollback: (name: string, version: string) => ({ title: `${name}を前のバージョンに戻す`, sentence: `${name}を前のバージョン ${version} で起動し直します。いまのバージョンは残り、もう一度書き込めます。`, label: '戻す' }),
} as const;

/** The table view. */
export const TABLE = {
  tabs: ['ノード', 'リンク', '配線'],
  nodeCols: ['番号', '呼び名', '製品名・MCU', '役割', 'つなぎ方', 'ピン'],
  linkCols: ['元', '先', '種類', 'アドレス', 'ピン'],
  addNode: 'ノードを加える',
  addLink: 'リンクを加える',
  addLinkWhy: 'つながっていない子機はありません',
  edit: '編集',
  remove: '消す',
} as const;

/** The define mode's CLI features. */
export const CLI_NAV = {
  heading: 'CLI 専用の機能',
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

/** Header CLI link. */
export const CLI_STATE = {
  none: 'CLI 未連携',
  connecting: 'CLI 接続中…',
  stopped: 'CLI 停止',
  denied: 'CLI 未連携',
  ready: (v: string) => `CLI ${v}`,
} as const;

/** CLI 連携. */
export const CLI_LINK = {
  steps: ['ターミナルを開く', 'dagon serve を実行する', '表示されたアドレスをこのブラウザで開く'],
  withoutCli: ['CLI なしで使える', 'グラフの編集・書込'] as const,
  deniedChip: '× 合言葉違い',
  deniedSentence: 'ターミナルに表示されたアドレスを、そのまま開き直してください。',
  stoppedSentence: 'ターミナルで dagon serve を実行し直してください。',
  needCli: 'ターミナルで dagon serve を実行し、表示されたアドレスを開いてください。',
  reconnect: 'CLI につなぎ直す',
  workspace: '作業場所',
  /** The API key of each way that takes one, kept in the OS credential store through serve. */
  keys: {
    heading: 'API キー',
    detail: 'キーは手元の dagon serve を通して OS の資格情報ストアに保存します。このページには残りません。ターミナルでは dagon ai key set でも登録できます。',
    set: '登録済み',
    unset: '未登録',
    input: 'キー',
    save: 'キーを保存する',
    saveWhy: 'キーを入力してください',
    remove: 'キーを削除する',
    removeWhy: 'OS の資格情報ストアからこのキーを削除します。削除すると、このつなぎ方では DAGON に聞けなくなります',
  },
  key: '署名の鍵',
  yes: 'あり',
  no: 'なし',
  features: '使える機能',
  featureNames: { gen: '雛形を作る', build: 'ビルド', sign: '署名', release: 'リリースを作る', flash: '書き込む', topology_save: '構成を作業場所に保存' } as Record<string, string>,
  unsupported: '非対応',
} as const;

/** 作業. */
export const JOBS = {
  heading: '作業',
  cols: ['種類', '対象', '状態', '経過', '結果'],
  kinds: { build: 'ビルド', sign: '署名', release: 'リリース', gen: '雛形', flash: '書き込み', conform: '適合試験', ai_topology: '構成の案' } as Record<string, string>,
  states: { queued: '待ち', running: '実行中', question: '質問あり', succeeded: '成功', failed: '失敗', cancelled: '中止', unknown: '不明' } as Record<string, string>,
  none: '作業 なし',
  started: '始めた時刻',
  cancel: 'ジョブを中止',
  copyLog: 'ジョブのログをコピー',
  failSentence: 'ログの赤い行を確かめてから、もう一度ビルドしてください。',
  answer: 'つないだ',
} as const;

/** 書き込み. */
export const FLASH = {
  heading: '書き込み',
  write: '書き込む',
  /** How each MCU is connected comes from its definition ([connection], dagon-wasm's mcu_connection). */
  via: 'つなぎ方',
  hubFirst: '親機がないと子機を確かめられないので、親機から順に書き込みます。',
  /** the confirmation before serve writes the board (its job question). */
  confirm: (name: string) => `${name} に書き込みます。よろしいですか。`,
  /** a family serve's flash does not write yet (ESP32-S3). */
  otherTools: 'このマイコンは、今は ESP のツールで書き込んでください。',
  yes: 'はい',
  no: 'いいえ',
  done: '✓ 書き込み済み',
  running: '● 実行中',
  notFound: '× 未検出',
} as const;

/** The 構成 differences of a release made for another 構成 (otherTopology). */
export const DIFF = {
  heading: '構成の違い',
  cols: ['ノード・リンク', 'この構成', 'ファイル'],
  none: 'なし',
  /** The chip on the graph's differing nodes. */
  chip: '△ 差分',
  spoken: '違い',
  /** No topology in the file: links and addresses were not compared. */
  linksUnchecked: 'リンク・アドレス 未確認',
} as const;

/**
 * Saving the 構成 to the project's YAML: the edits stay on the page, marked 未保存, until a save goes through.
 */
export const SAVE = {
  unsaved: '● 未保存',
  /** 422: the CLI's own check refused the 構成. */
  invalid: { chip: '× 保存失敗', sentence: '問題を直してから、もう一度保存してください。' },
  /** 422 in dagon demo: an uplink the demo cannot simulate. */
  demoLink: {
    chip: '× デモ非対応',
    sentence: 'デモで試せる親機の接続元は、パソコンかブラウザから USB・UART・Wi-Fi でつないだものです。そのどれかを選んでください。',
  },
  /** 409: a job holds the project. */
  busy: { chip: '… 作業中', sentence: '作業が終わってから、もう一度保存してください。' },
  failed: { chip: '× 保存失敗', sentence: 'ターミナルで dagon serve が動いているか確かめてから、もう一度保存してください。' },
} as const;

/** leaving the project while its 構成 could not be saved (the dialog). */
export const UNSAVED = {
  heading: '保存していない変更があります',
  sentence: 'もう一度保存するか、変更を捨てて進むかを選んでください。',
  save: '構成を保存し直す',
  discard: '変更を捨てて進む',
  stay: '編集に戻る',
} as const;

/** The start screen (始め). */
export const START = {
  heading: '始め',
  new: 'プロジェクトを作る',
  /** 「プロジェクトを作る」's modal: how, then name and place. */
  create: {
    title: 'プロジェクトを作る',
    how: '作り方',
    empty: '空から作る',
    sample: '見本から作る',
    cancel: 'やめる',
    next: '名前と場所を決める',
    back: '戻る',
    pick: '場所を選ぶ',
    where: '作る場所',
    go: 'プロジェクトを作る',
  },
  /** Under the product's name on the start page. */
  sub: 'ノードとリンクの構成を描くと、ノードごとのアプリの雛形を作れます。',
  open: 'プロジェクトを開く',
  recent: '最近のプロジェクト',
  recentNone: 'なし',
  up: '上へ',
  upWhy: 'いちばん上のフォルダです',
  goWhy: '名前を入れ、場所を選ぶと作れます',
  place: '場所',
  name: '名前',
  cantMake: '× 作成失敗',
  cantOpen: '× 読み込み失敗',
  loading: '読込み中…',
  /** The folder of a project (it holds dagon.toml). */
  project: '● プロジェクト',
  /**
   * Why serve did not open or make it: 404 / 422 / 409 / 403. A folder that cannot be written is M35.
   */
  missing: { chip: '× フォルダ違い', sentence: 'dagon.toml のあるフォルダを選んでください。' },
  key: { chip: '× 鍵エラー', sentence: 'dagon serve のターミナルで鍵の理由を確かめてから、もう一度選んでください。' },
  busy: { chip: '… 作業中', sentence: '作業が終わってから、もう一度選んでください。' },
  refused: { chip: '× 範囲外', sentence: 'ホームフォルダの中のフォルダを選んでください。' },
  exists: { chip: '× 名前の重複', sentence: '使われていないフォルダの名前を入れてください。' },
  /** S46: a folder with a 構成 but no dagon.toml. */
  initAsk: 'このフォルダに dagon.toml を作りますか。',
} as const;

/** `dagon demo`: the simulator, marked wherever the page is. */
export const SIM = {
  badge: '模擬',
  label: '模擬の親機と子機（実機ではありません）',
  /** Before connecting: no device list appears for the simulated 親機. */
  connect: { sentence: '模擬の親機につなぎます。機器を選ぶ一覧は出ません。' },
  /** The demo page while watching: it reads no files, so its prepared package is what 書き込む writes. */
  siteWrite: '「書き込む」で、用意してある新しいバージョンを書き込めます。',
} as const;

/** The menu (メニュー) and its one confirmation. */
export const MENU = {
  button: 'メニューを開く',
  restart: '最初からやり直す',
  groups: { project: 'プロジェクト', define: '構成', connect: 'つなぐ', dagon: 'DAGON', help: '困ったとき' },
  save: '保存する',
  arrange: '自動で並べる',
  place: 'パッケージファイルを選んで書き込む',
  rollback: '前のバージョンに戻す',
  applyProposal: 'DAGON の案を反映する',
  cli: 'CLI の接続と API キーを見る',
  /** The view toggle, in the menu only while the bar is too narrow to hold it. */
  viewTable: '表で見る',
  viewGraph: 'グラフで見る',
  keys: 'キー操作',
  copyLog: '記録をコピーする',
  copyUrl: 'この URL をコピーする',
  copied: 'コピーした',
  /** What each item does, under its name (a first-time user reads it before pressing). */
  hint: {
    new: '構成を描くところから始めます（いまのプロジェクトは保存されます）',
    open: 'パソコンのフォルダから dagon.toml のあるプロジェクトを開きます',
    save: '構成をプロジェクトの YAML にいま保存します（ふだんは自動で保存します）',
    undo: '構成への最後の変更を取り消します',
    redo: '取り消した変更をもう一度行います',
    parts: '左に部品の窓を開きます（ドラッグでキャンバスに置けます）',
    arrange: '動かしたカードを親機から順の並びに戻します',
    fit: 'ノードがすべて見える大きさと位置にします',
    viewTable: 'ノード・リンク・配線を表にして見ます',
    viewGraph: 'ノードとリンクを図にして見ます',
    connect: 'USB でつないだ親機と通信を始めます',
    disconnect: '親機との通信を止めます（USB は抜いても大丈夫です）',
    place: 'パソコンにある .dgb ファイルを選び、その中身をノードへ書き込みます',
    rollback: '選んだノードを、前のバージョンで起動し直します',
    link: 'AI のエージェントとつなぎ、構成について質問できるようにします',
    talk: 'DAGON の吹き出しを開きます',
    apply: 'DAGON が出した構成の変更案を構成に入れます',
    cli: 'dagon serve とのつながりと、登録した API キーを見ます',
    unlink: 'AI のエージェントとのつながりと同意を取り消します',
    help: 'よくある困りごとと、その原因と直し方',
    keys: 'キーボードでできる操作の一覧',
    copyLog: '下の「記録」の内容をクリップボードにコピーします',
    copyUrl: 'このページのアドレスをコピーします',
    restart: 'このページの記録を消して、始めの画面に戻ります',
  },
  /** Why an item cannot be used now. */
  off: {
    notDefine: '準備で使えます。',
    notGraph: 'グラフの表示で使えます。',
    notConnected: '親機につないでから使えます。',
    notIdle: '今の動きが終わってから使えます。',
    noNode: 'ノードを選んでから使えます。',
    noProposal: 'DAGON の案はありません。',
    noUndo: '取り消す変更はありません。',
    noRedo: 'やり直す変更はありません。',
    partsOpen: '部品の窓はもう開いています。',
    arranged: 'すでに親機から順の並びです。',
    noLog: '記録はまだありません。',
    noProject: 'プロジェクトを開くと使えます。',
    demo: 'デモでは使えません。dagon serve で使えます。',
  },
  restartHeading: 'やり直しの確認',
  restartSentence: 'このページの記録を消して、始めの画面に戻ります（元に戻せません。プロジェクトのファイルと基板の中のプログラムは変わりません）。',
  restartConfirm: '消して最初からやり直す',
  cancel: 'やり直しをやめる',
} as const;

/** Fills {name} slots in a copy string. */
export const fill = (s: string, v: Record<string, string | number>): string => s.replace(/\{(\w+)\}/g, (m, k: string) => (k in v ? String(v[k]) : m));

export const SKELETON = {
  make: '雛形を作る',
  makeWhy: 'ノードを置き、問題を直すと作れます',
  title: '雛形を作る',
  state: {
    running: '… 作成中',
    done: '✓ 作成済',
    failed: '! 失敗',
  },
  where: '作った場所',
  next: {
    '1': 'エディタで、各ノードの src/app.rs にアプリのコードを書く（main.rs と wiring.rs は DAGON が作るので、触らなくてよい）',
    '2': '準備で「ビルドする」、続けて書込で「書き込む」',
    heading: '次にすること',
  },
  fail: {
    fix: '理由を直してから、「もう一度作る」を押します。作成済みのノードはそのまま残ります。',
  },
  startFailed: 'dagon serve が受け付けませんでした',
  retry: 'もう一度作る',
  cancel: '中止',
} as const;

export const NOTICE = {
  close: '閉じる',
  more: '詳しく見る',
  reason: '理由',
  fix: '直し方',
  after: 'このあと',
} as const;

export const BUILD = {
  /** A node with no [build.*] in dagon.toml: nothing to build until the skeleton is made. */
  noSkeleton: { reason: 'このノードの雛形がまだありません。', stale: 'DAGON のソースが変わったので、雛形が古くなっています。', fix: '雛形を作ると、ビルドできるようになります。' },
  run: 'ビルドする',
  runningWhy: 'ビルドしています',
  title: 'ビルドする',
  state: {
    running: '… ビルド中',
    failed: '! ビルド失敗',
  },
  toast: {
    done: 'ビルド済 {n} ノード',
  },
  fail: {
    fix: 'エディタで {file} の {line} 行目を直してから、もう一度ビルドします。',
    others: 'ほかの {n} ノードはビルド済みです。',
    plain: '理由を直してから、もう一度ビルドします。',
  },
  startFailed: 'dagon serve が受け付けませんでした',
  retry: 'もう一度ビルドする',
  askAgent: '失敗の理由を聞く',
} as const;

/** The project's layout file (呼び名 and card places). */
export const LAYOUT = {
  refused: '× 配置を読めない',
  refusedDetail: '配置のファイルを読めないので、自動の配置で開きました。ファイルは変えず、配置と呼び名の変更は保存しません。',
} as const;

/** The project's 構成 could not be read as it opened: nothing replaces it. */
export const TOPOLOGY_READ = {
  label: '× 構成を読めない',
  detail: '構成のファイルを読めないので、変更は保存しません。プロジェクトを開き直してください。',
} as const;

export const WRITE = {
  noneChecked: { label: 'ノードを選ぶ', detail: '書き込むノードのカードにチェックを入れてください。' },
  alreadyOn: (v: string, coreToo: boolean) => ({ label: `すべて ${v}`, detail: `チェックしたノードはすでに ${v} で動いて${coreToo ? 'いて、ブートローダも同じなので' : 'いるので'}、書き込みませんでした。` }),
  pickVersion: 'バージョンを選んで書き込む',
  run: '書き込む',
  /** Why 書き込む cannot be pressed, on hover. */
  offWhy: { notConnected: '接続すると書き込めます', busy: '今の書き込みが終わると、また書き込めます' },
  /** Why the version list cannot be opened, on hover. */
  pickOffWhy: '接続してバージョンが分かると選べます',
  /** Why a card's check cannot be changed, on hover. */
  checksLockedWhy: '書き込みが終わるまで、チェックは変えられません',
  go: '{n} ノードへ書き込む',
  toast: {
    done: '書き込み済み {n} ノード',
    none: '書き込み不要',
  },
  state: {
    writing: '… 書き込み中',
    done: '✓ 書き込み済み',
    rolledBack: '▲ 戻した',
  },
  retry: 'もう一度書き込む',
  /** The header's version picker: its accessible name, and the latest build when its nodes' versions differ. */
  version: 'バージョン',
  latest: '最新のビルド',
  /** A node card's check: its accessible name. */
  pick: (name: string) => `${name}へ書き込む`,
  /** On a card while it is written: the version it gets. */
  to: (v: string) => `→ ${v}`,
  toCore: (v: string) => `→ ブートローダ ${v}`,
  /** On a card after the write: it went back, or it could not be written. */
  previous: '↺ 前のバージョンに戻った',
  failed: '× 書き込めなかった',
  /** The real cause and the next step for a write that could not be made, by what the node or the link said. */
  why: {
    silent: { reason: 'ノードが親機の呼びかけに応答しませんでした。', fix: 'ノードの電源と、親機との配線を確かめてから、もう一度書き込んでください。' },
    E_SIGNATURE: { reason: 'ノードがパッケージの配布元を確かめられませんでした。別の鍵で作られたパッケージです。', fix: 'このプロジェクトの鍵でビルドし直してから、もう一度書き込んでください。' },
    E_KEY: { reason: 'ノードがパッケージの配布元を確かめられませんでした。別の鍵で作られたパッケージです。', fix: 'このプロジェクトの鍵でビルドし直してから、もう一度書き込んでください。' },
    E_DIGEST: { reason: '届いた中身が途中で壊れていました。', fix: '親機との配線とケーブルを確かめてから、もう一度書き込んでください。' },
    E_COMPAT: { reason: 'パッケージがこのノードの機種に合いません。', fix: '構成のマイコンとボードを確かめ、ビルドし直してから書き込んでください。' },
    E_FLASH: { reason: 'ノードが自分の中身に書き込めませんでした。', fix: 'ノードの電源を入れ直してから、もう一度書き込んでください。' },
    refused: { reason: 'ノードがこのパッケージの受け取りを断りました。', fix: '下の「記録」で理由を確かめ、ビルドし直してから書き込んでください。' },
    cannotFix: { reason: '送り直しも予備からの起動も通りませんでした。', fix: 'このノードを USB で直接つなぎ、カードの「書き込む」から書き込んでください。' },
  } as Record<string, { reason: string; fix: string }>,
  /** The written package could not be kept in the project (a toast; writing went on). */
  keepFailed: { label: '× 保存できない', detail: '書き込んだパッケージをプロジェクトの out/deploy に保存できませんでした。' },
  rolled: {
    reason: '新しいバージョンで起動を続けられず、前のバージョン {version} に戻りました。ノードは動いています。',
    next: 'アプリを直してバージョンを上げ、ビルドしてから、もう一度書き込んでください。',
  },
  /** The new boot core failed its check: the node runs its confirmed one, the app was not written. */
  coreBack: {
    label: '↺ ブートローダは元のまま',
    detail: '新しいブートローダが自分の検査に通らなかったので、動いていたブートローダに戻りました。ノードは動いています。アプリは書き込んでいません。',
    next: 'ブートローダを直してバージョンを上げ、ビルドしてから、もう一度書き込んでください。',
  },
  /** A silent node with a ROM entry: said once, before anything is touched. */
  rom: {
    silent: '× 応答なし',
    body: '{node} が応答しません。書き込むと、中身をすべて書き換えます。配線の抜けでも応答しなくなるので、確かめてから進めてください。',
  },
  factory: {
    intro: '{node} の DAGON が応答しないため、USB で直接書き込みます。初めての書き込みと、DAGON が消えたときだけの手順です。',
    steps: '手順',
    drive: 'ドライブ',
    waiting: '… 待機中',
    after: '書き終わったら閉じて、接続します。残りのノードは「書き込む」で DAGON 経由で書き込みます。応答しない子機は、そのカードの「書き込む」から書き込みます。',
    cancel: '中止',
    childIntro: '{node} は親機から届かないため、この子機を直接つないで書き込みます。',
    howToConnect: 'つなぎ方',
    cancelLocked: '書き込み中',
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
    connected: '接続済 {node}',
    lost: '接続が切れた {node}',
  },
} as const;

