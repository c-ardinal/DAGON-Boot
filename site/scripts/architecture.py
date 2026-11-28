"""DAGON's software architecture: the layered design, as one figure and one table of components.

This is the design, kept by hand here; it is not read from the code. gen.py writes the figure
to assets/architecture/ for the READMEs and, with the table, into the site's Explanation page, and
the components this tree lacks (NOT_IN_TREE) into the limitations page.
"""
from xml.sax.saxutils import escape

# web/src/styles/tokens.css
INK, LAYER1, LAYER2, LAYER3, PAPER, MUTED, HAIR = "#0a0a0a", "#141413", "#1e1e1c", "#282725", "#f1efe8", "#9a9892", "#2c2b29"
ACCENT, WATER, ATTENTION, PROGRESS = "#3cb4f0", "#123a4c", "#f5a524", "#b48cf2"
COLOURS = (INK, LAYER1, LAYER2, LAYER3, PAPER, MUTED, HAIR, ACCENT, WATER, ATTENTION, PROGRESS)
FONT = "'Zen Kaku Gothic New','Hiragino Sans','Yu Gothic UI','Noto Sans JP',system-ui,sans-serif"

W = 1400
COLUMNS = ("Boot", "Image and Storage", "Update and Comms", "Hub", "Trust", "Liveness", "Observability")
HUB = 3

PC_ROWS = (
    ("PC Front Ends", "full", ("dagon CLI", "dagon serve", "DAGON Console", "Claude Code Plugin")),
    ("PC Host Extensions", "full", ("Agent Definitions",)),
    ("PC Host Services", "cols", (("Project Generator", "Definition Registry", "Topology Check"), ("Node Build",), ("Operations",), (),
                                  ("Release Builder",), ("AI Agent Link",), (), ("Event Stream", "Catalog"))),
    ("PC Host Ports", "cols", ((), ("BuildRunner",), ("Transport",), ("confirm_plan",), ("Signer", "KeyStore"), ("Agent",), (), ())),
    ("PC Host Adapters", "cols", ((), ("Cargo Runner",), ("Serial", "Web Serial", "Simulator Link"), ("ROM Writers",), ("File Key Pair",), (), (), ())),
)
PC_COLS = 8
CONTRACTS = ("DAGON Link", "Release Bundle", "Image Header", "Topology Binary", "VERSION")
DEVICE_TOP = (
    ("Applications and Extensions", ("User Application", "User Extensions", "Library Extensions")),
    ("Layer Start", ("Boot Start", "App Start", "Relay Start", "Chain Composer", "Extension API")),
)
DEVICE_COLS = (
    ("Layer Services", {0: ("Stage0 Sequence", "Boot Sequence", "Boot Action Interpreter"), 1: ("Image Lifecycle",),
                        2: ("Update Session", "Wire Protocol", "Serving Loop", "Recovery Entry"),
                        3: ("Relay", "Coordinator"),
                        5: ("Liveness Guard", "Reboot Gate"),
                        6: ("Publisher", "VERSION Responder")}),
    ("Core Logic", {0: ("Boot Decision", "Core Chooser"), 1: ("State Log Fold",), 3: ("Constraint Evaluator",),
                    4: ("Trust Rule",), 5: ("Deadline Book",)}),
    ("Replaceable Ports", {0: ("DecisionPolicy", "BoardBringUp"), 1: ("SlotStore", "LogStorage", "SelfCheck"), 2: ("UpdateTransport", "UpdateProtocol", "Recovery"),
                           3: ("NodeLink",), 4: ("ImageVerifier", "Digest", "CryptoProvider"), 5: ("WatchdogDriver",), 6: ("Diagnostics", "Display", "LogSink")}),
    ("Driver Contract", {0: ("BootSelect",), 1: ("Flash",), 3: ("I2cBus", "StreamPort", "SubControl"), 5: ("Reset",)}),
)
BOARD = ("Board Definition", "Board Bring-Up", "Board Extension", "Boot Action Program", "Shared Bus")
MCU = {0: ("Reset and Jump",), 1: ("Flash Driver", "Slot Store Driver"), 2: ("Link Drivers",), 5: ("Watchdog Driver", "Clock", "Execution Setup")}
MCU_STRIP = "MCU Definition"
SILICON = ("ROM Loader", "boot2", "Vendor 2nd Stage", "Peripherals")
LIBRARIES = ("dagon-format", "dagon-proto", "dagon-events", "Retained Codecs")
PROOF = ("dagon-conform", "dagon-sim", "dagon-retained-model")
DIRECT = ("ROM Loader Client",)

PORT_ROWS = ("PC Host Ports", "Replaceable Ports", "Driver Contract")

# Component -> one line (Japanese, English).
COMPONENTS = {
    "dagon CLI": ("バイナリ名dagonのコマンド", "The binary named dagon"),
    "dagon serve": ("DAGON Consoleのためのローカルのサーバ", "The local server for DAGON Console"),
    "DAGON Console": ("ブラウザのUI", "The browser UI"),
    "Claude Code Plugin": ("移植と構成の生成のスキル、サブエージェント、フック", "Porting and topology skills, subagents and hooks"),
    "Agent Definitions": ("エージェントとその接続方法のデータ", "Data for agents and their ways"),
    "Project Generator": ("構成からプロジェクトを作成する（`dagon new`、`dagon gen`）", "Renders a project from the topology (`dagon new`, `dagon gen`)"),
    "Definition Registry": ("ボードとマイコンの定義を保持", "Holds the definitions of boards and MCUs"),
    "Topology Check": ("構成のYAMLを読込し、検証規則を適用する", "Reads the topology YAML and applies the validation rules"),
    "Node Build": ("dagon.tomlの`[build.<id>]`に従ってノードごとにcargo buildする", "Runs cargo build per node from `[build.<id>]` in dagon.toml"),
    "Operations": ("apply、status、recoverの手順", "The apply, status and recover procedures"),
    "Release Builder": ("検査後にマニフェストに署名し、パッケージファイル（.dgb）を作成する", "Signs the manifest after its checks and makes the package file (.dgb)"),
    "AI Agent Link": ("dagon aiのコマンド", "The dagon ai commands"),
    "Event Stream": ("ホストのイベントの配信", "Delivers host events"),
    "Catalog": ("全イベントと全ポートの一覧", "Lists every event and port"),
    "BuildRunner": ("ビルドを実行するポート", "Port that runs builds"),
    "Transport": ("PC接続ノードへのバイト列のポート", "Port for the byte stream to the hub"),
    "confirm_plan": ("書込前の確認の判断点", "Decision point before a write"),
    "Signer": ("署名のポート", "Port that signs"),
    "KeyStore": ("利用者が選択した格納先（環境変数、OSのキーストア、1Passwordなど）から署名のキーを受信するポート", "Port that receives the signing key from where the user chose to keep it (an environment variable, the OS key store, 1Password and so on)"),
    "Agent": ("エージェントのポート", "Port for an agent"),
    "Cargo Runner": ("cargoで実行するビルド", "Builds through cargo"),
    "Serial": ("CLIのシリアルポートのTransport", "The CLI's serial port Transport"),
    "Web Serial": ("dagon-wasmのWeb SerialのTransport", "dagon-wasm's Web Serial Transport"),
    "Simulator Link": ("dagon-simの模擬リンク", "dagon-sim's simulated link"),
    "ROM Writers": ("PCのdagon-romlinkによるROMの手順", "ROM procedures on the PC through dagon-romlink"),
    "File Key Pair": ("ファイルのキーの組", "Key pair in files"),
    "DAGON Link": ("ホスト、コーディネータ、他のノード間のフレームとコマンド", "Frames and commands between host, hub and nodes"),
    "Release Bundle": ("パッケージファイル（.dgb）の形式", "Format of the package file (.dgb)"),
    "Image Header": ("署名付きのイメージのヘッダ", "Header of a signed image"),
    "Topology Binary": ("コンパイル済の構成の形式", "Format of the compiled topology"),
    "VERSION": ("ノードのバージョンと状態の照会", "Query of a node's versions and state"),
    "User Application": ("利用者のアプリ", "The user's app"),
    "User Extensions": ("利用者の拡張", "The user's extensions"),
    "Library Extensions": ("ライブラリが提供する拡張。ボードの拡張と利用者の拡張の中間で動作する", "Extensions a library provides, run between the board's and the user's"),
    "Boot Start": ("ブートのレイヤーの起動", "Starts the boot layer"),
    "App Start": ("アプリのレイヤーの起動", "Starts the app layer"),
    "Relay Start": ("リレーのレイヤーの起動", "Starts the relay layer"),
    "Chain Composer": ("拡張のチェーンの合成", "Composes the extension chain"),
    "Extension API": ("拡張のアクセス可能な範囲", "What an extension may touch"),
    "Stage0 Sequence": ("リセット後に最初に動作する手順", "The first steps after a reset"),
    "Boot Sequence": ("ブートローダの手順", "The boot loader's steps"),
    "Boot Action Interpreter": ("ブートのアクションプログラムの実行", "Runs boot action programs"),
    "Image Lifecycle": ("スロットのイメージの状態機械", "State machine of a slot's image"),
    "Update Session": ("書込のセッションの状態機械", "State machine of a write session"),
    "Wire Protocol": ("リンクのメッセージとセッションの対応", "Maps link messages onto the session"),
    "Serving Loop": ("リクエストに応答するループ", "The loop that answers requests"),
    "Recovery Entry": ("復旧経路への入口", "Entry to a recovery path"),
    "Relay": ("要求をホップごとに、リンク経由で到達するノードへ中継する", "Forwards requests hop by hop to the nodes its links reach"),
    "Coordinator": ("PCがコーディネータを経由して書込する", "The PC writes through the coordinator"),
    "Liveness Guard": ("タスクの生存を条件とするキック", "Kicks the watchdog on task liveness"),
    "Reboot Gate": ("返答を優先し、上限付きの遅延を待機した後にリセットする", "Replies first, waits a bounded delay, then resets"),
    "Publisher": ("待機可能な公開状態を出力する", "Writes the waitable published state"),
    "VERSION Responder": ("VERSIONに応答する", "Answers VERSION"),
    "Boot Decision": ("どのスロットを起動するかの判断", "Decides which slot starts"),
    "Core Chooser": ("stage0がブートローダのスロットを選択する", "stage0 picks the boot loader slot"),
    "State Log Fold": ("記録から状態を集約する", "Folds records into state"),
    "Constraint Evaluator": ("マニフェストの制約を検証する", "Checks manifest constraints"),
    "Trust Rule": ("署名とキーについての唯一の認証規則", "The one rule for signatures and keys"),
    "Deadline Book": ("ガードの後ろの純粋な期限の計算", "Pure deadline accounting behind the guard"),
    "DecisionPolicy": ("ブートの方針のポート", "Port for boot policy"),
    "BoardBringUp": ("ボードの起動処理のポート", "Port for bringing up the board"),
    "SlotStore": ("イメージの格納先のポート", "Port for where images live"),
    "LogStorage": ("状態ログの格納先のポート", "Port for where the state log lives"),
    "SelfCheck": ("確定のときの自己検査のポート", "Port for the self-check at confirm"),
    "UpdateTransport": ("リンクのバイト列のポート", "Port for a link's bytes"),
    "UpdateProtocol": ("メッセージのポート", "Port for messages"),
    "Recovery": ("追加の復旧経路のポート", "Port for an added recovery path"),
    "NodeLink": ("リンク経由で別ノードに到達するポート", "Port for reaching another node over a link"),
    "ImageVerifier": ("ヘッダの署名検証のポート", "Port for verifying a header's signature"),
    "Digest": ("本体のハッシュのポート", "Port for the body hash"),
    "CryptoProvider": ("暗号の基本要素のポート", "Port for crypto primitives"),
    "WatchdogDriver": ("ウォッチドッグのポート", "Port for the watchdog"),
    "Diagnostics": ("追加の診断のポート", "Port for added diagnostics"),
    "Display": ("表示のポート", "Port for display"),
    "LogSink": ("ログの出力先のポート", "Port for where logs go"),
    "BootSelect": ("kind Sはジャンプ、kind Vはベンダーの選択情報の操作", "Kind S: the jump; kind V: the vendor's selection data"),
    "Flash": ("フラッシュの消去、書込、読出", "Flash erase, write and read"),
    "I2cBus": ("コーディネータのI2Cバス", "The hub's I2C bus"),
    "StreamPort": ("コーディネータのバイト列", "The hub's byte stream"),
    "SubControl": ("接続先のノードの制御線", "A child's control lines"),
    "Reset": ("システムのリセット", "System reset"),
    "Board Definition": ("ボードのデータ", "The board's data"),
    "Board Bring-Up": ("ボードの電源、ピン、バスの準備", "Prepares the board's power, pins and buses"),
    "Board Extension": ("ボード固有の拡張", "The board's own extension"),
    "Boot Action Program": ("ブートの宣言的なプログラム", "Declarative program at boot"),
    "Shared Bus": ("複数の主体が使用するボードのバス", "A board bus several parties use"),
    "Reset and Jump": ("リセットとイメージへのジャンプ", "Reset, and the jump to an image"),
    "Flash Driver": ("フラッシュのドライバ", "The flash driver"),
    "Slot Store Driver": ("kind Sのログ、またはkind Vのベンダーデータ", "A kind S log or kind V vendor data"),
    "Link Drivers": ("ノードのLinkの実装", "Implement the node's Link"),
    "Watchdog Driver": ("ウォッチドッグのドライバ", "The watchdog driver"),
    "Clock": ("DAGON自身のマイクロ秒の時計（dagon-eventsの`Clock`）。マイコンのcrateが用意する", "DAGON's own microsecond clock (`Clock` of dagon-events), provided by the MCU package"),
    "Execution Setup": ("実行モデルに従うエグゼキュータの優先度", "Executor priorities per the execution model"),
    "MCU Definition": ("マイコンのデータ", "The MCU's data"),
    "ROM Loader": ("マイコンのROMのローダ", "The MCU's ROM loader"),
    "boot2": ("RP2040のブートROMの直後に動作する256バイト", "The 256 bytes after the RP2040's boot ROM"),
    "Vendor 2nd Stage": ("ベンダーの2nd stage", "The vendor's 2nd stage"),
    "Peripherals": ("マイコンのペリフェラル", "The MCU's peripherals"),
    "dagon-format": ("イメージヘッダ、マニフェスト、状態ログ、構成のバイト配置", "Byte layouts of the image header, manifest, state log and topology"),
    "dagon-proto": ("フレーム、コマンド、エラーの定義", "Frames, commands and errors"),
    "dagon-events": ("イベント、状態機械、拡張", "Events, state machines, extensions"),
    "Retained Codecs": ("構成の本体の形式", "Topology body format"),
    "dagon-conform": ("実機とシミュレータに共通の適合試験", "Conformance tests shared by hardware and the simulator"),
    "dagon-sim": ("模擬フラッシュ、模擬リンク、模擬ROMローダ、故障注入", "Simulated flash, links and ROM loader, with fault injection"),
    "dagon-retained-model": ("保持の形式の実行可能なモデル", "Executable model of the retained formats"),
    "ROM Loader Client": ("別のマイコンのROMローダを駆動する", "Drives another MCU's ROM loader"),
}

ABSENT = ("コードに存在しません。", "Not in the code.")
# The components above that this tree lacks, in whole or in part: the gap each (Japanese, English), which the
# limitations page lists. A port's gap is in its dagon_events::PORTS record; a component that is part of such a
# record names it here instead, and a port is not listed here at all.
NOT_IN_TREE = {
    "AI Agent Link": ("ボード移植の開始方法は、DAGON Consoleのメニュー「エージェントによるボード移植」、CLIの`dagon ai port`、`POST /v1/ai/port`です。"
                      "移植の実行は模擬プログラムとの試験と、実機なしの起動確認だけで、実機での完走は未確認です。",
                      "A port starts from the DAGON Console menu, the CLI (`dagon ai port`) or "
                      "`POST /v1/ai/port`. The run is tested against a stand-in program and by starting it without "
                      "hardware; a port run to its end on hardware is to be confirmed."),
    "ROM Loader Client": ("ESP32-S3のROMシリアルローダー経由の書込（`dagon recover`）は、模擬ローダーとの試験だけです。"
                          "実機では、ENTER_ROM後にローダーがリンクのUARTで受信すること、中継ノードのUARTがローダーの応答を欠落なく受信することが未確認です。",
                          "The write through the ESP32-S3's ROM serial loader (`dagon recover`) is tested against the simulated "
                          "loader only. On hardware it is to be confirmed that the loader listens on the link's UART after ENTER_ROM "
                          "and that the relaying node's UART keeps up with the loader's answers."),
    "Boot Action Interpreter": ABSENT,
    "Retained Codecs": ("ファームウェアに保持形式のコーデックは存在しません。"
                        "構成本体の形式は`dagon-format`の形式（先頭32バイト、ノード40バイト）で、[保持と永続の形式](/explanation/retained-formats/)"
                        "の3.1に記載しています。ノードは構成を保存しません。構成はホストのパッケージにだけ存在し、ターゲット3（旧来の構成の保存先）は予約番号として残存し、どのノードも受理しません。",
                        "The firmware has none of the retained codecs: "
                        "the topology body is `dagon-format`'s (a 32-byte head, 40-byte nodes), as section 3.1 of "
                        "[Retained and persistent formats](/en/explanation/retained-formats/) gives it. No node stores the topology: "
                        "it stays in the host's package, and target 3 (where a node once stored it) stays a reserved "
                        "number no node takes."),
}


def gap_list(lang):
    """The limitations page's list of the components above this tree lacks, in the table's order."""
    i = 0 if lang == "ja" else 1
    return "".join(f"- {name}: {NOT_IN_TREE[name][i]}\n" for name in COMPONENTS if isinstance(NOT_IN_TREE.get(name), tuple))


WORDS = {
    "ja": {"title": "DAGONのソフトウェアアーキテクチャ", "component": "コンポーネント", "port": "ポートと規約", "contract": "ノードとPCの共通形式", "hub": "Hub列はコーディネータのイメージのみ", "vendor": "シリコンとベンダー"},
    "en": {"title": "DAGON's software architecture", "component": "Component", "port": "Port or contract", "contract": "Format shared by PC and node", "hub": "The Hub column is hub images only", "vendor": "Silicon and vendor"},
}

BOX, BOX2 = 21, 33  # one line, two lines
GAP = 4
CHAR = 6.15


def _text(x, y, fill, size, words, weight=500, anchor="middle"):
    return f'<text x="{round(x, 1)}" y="{round(y, 1)}" fill="{fill}" font-family="{FONT}" font-size="{size}" font-weight="{weight}" text-anchor="{anchor}">{escape(words)}</text>'


def _rect(x, y, w, h, fill, stroke="none", rx=3, dash=""):
    extra = f' stroke-dasharray="{dash}"' if dash else ""
    return f'<rect x="{round(x, 1)}" y="{round(y, 1)}" width="{round(w, 1)}" height="{round(h, 1)}" rx="{rx}" fill="{fill}" stroke="{stroke}"{extra}/>'


def _lines(name, width):
    if len(name) * CHAR <= width - 6:
        return [name]
    if " " not in name:
        cut = name.rfind("-") + 1
        return [name[:cut], name[cut:]] if cut else [name]
    words = name.split(" ")
    best = min(range(1, len(words)), key=lambda i: abs(len(" ".join(words[:i])) - len(" ".join(words[i:]))))
    return [" ".join(words[:best]), " ".join(words[best:])]


def _bh(name, width):
    return BOX2 if len(_lines(name, width)) == 2 else BOX


def _box(x, y, w, name, kind="component"):
    fill, stroke, colour, dash = {"component": (LAYER3, "none", PAPER, ""), "port": (WATER, ACCENT, ACCENT, ""), "contract": (LAYER2, ATTENTION, ATTENTION, ""),
                                  "hub": (LAYER3, ATTENTION, PAPER, "3 2"), "hubport": (WATER, ATTENTION, ACCENT, "3 2"), "vendor": (LAYER2, HAIR, MUTED, "")}[kind]
    h = _bh(name, w)
    out = [_rect(x, y, w, h, fill, stroke, dash=dash)]
    lines = _lines(name, w)
    for i, line in enumerate(lines):
        out.append(_text(x + w / 2, y + 14.5 + i * 13, colour, 12, line))
    return out, h


def _label(x, y, w, name, colour):
    words, lines, line = name.split(" "), [], ""
    for word in words:
        if line and len(line + " " + word) * 6.6 > w:
            lines.append(line)
            line = word
        else:
            line = (line + " " + word).strip()
    lines.append(line)
    return [_text(x, y + 15 + i * 14, colour, 12.5, l, 700, "start") for i, l in enumerate(lines)]


def _full(x0, x1, y, names, kind="component"):
    n = len(names)
    w = (x1 - x0 - GAP * (n - 1)) / n
    return [p for i, name in enumerate(names) for p in _box(x0 + i * (w + GAP), y, w, name, kind)[0]]


def _stack_h(names, w):
    return sum(_bh(n, w) for n in names) + GAP * (len(names) - 1) if names else 0


def node_svg(lang):
    t = WORDS[lang]
    body = [_text(10, 22, PAPER, 15, t["title"], 700, "start")]
    y = 32
    # the PC, across the whole width
    label_w, x0, x1 = 112, 126, W - 10
    col_w = (x1 - x0 - GAP * (PC_COLS - 1)) / PC_COLS
    for name, form, cells in PC_ROWS:
        port = name in PORT_ROWS
        h = BOX + 8 if form == "full" else max(_stack_h(c, col_w) for c in cells) + 8
        h = max(h, 8 + 14 * (1 + (len(name) * 6.6 > label_w)))
        body.append(_rect(10, y, W - 20, h, LAYER1, rx=4))
        body.append(_rect(10, y, 3, h, PROGRESS, rx=1))
        body += _label(18, y + 1, label_w, name, PROGRESS)
        if form == "full":
            body += _full(x0, x1, y + 4, cells)
        else:
            for i, names in enumerate(cells):
                by = y + 4
                for n in names:
                    part, bh = _box(x0 + i * (col_w + GAP), by, col_w, n, "port" if port else "component")
                    body += part
                    by += bh + GAP
        y += h + 3
    # the contracts between them
    y += 3
    body.append(_rect(10, y, W - 20, BOX + 8, LAYER1, rx=4))
    body.append(_rect(10, y, 3, BOX + 8, ATTENTION, rx=1))
    body += _label(18, y + 1, label_w, "Contracts", ATTENTION)
    body += _full(x0, x1, y + 4, CONTRACTS, "contract")
    y += BOX + 8 + 6
    # the device: side blocks, then rows
    side_w, row_w = 112, 100
    side_x1 = 10 + side_w
    mx0 = side_x1 + 6 + row_w
    right_w = 112
    mx1 = W - 10 - right_w - 6
    c_w = (mx1 - mx0 - GAP * (len(COLUMNS) - 1)) / len(COLUMNS)
    dev_top = y
    rows = []

    def band(name, h):
        nonlocal y
        rows.append(_rect(side_x1 + 6, y, mx1 - side_x1 - 6, h, LAYER1, rx=4))
        rows.append(_rect(side_x1 + 6, y, 3, h, ACCENT, rx=1))
        rows.extend(_label(side_x1 + 13, y + 1, row_w - 8, name, ACCENT))
        return y

    for name, names in DEVICE_TOP:
        h = BOX + 8 + (12 if len(name) * 6.6 > row_w - 8 else 0) + (16 if len(name) * 6.6 > 1.7 * (row_w - 8) else 0)
        by = band(name, h)
        rows += _full(mx0, mx1, by + (h - BOX) / 2, names)
        y += h + 3
    # column titles
    y += 1
    for i, title in enumerate(COLUMNS):
        rows.append(_text(mx0 + i * (c_w + GAP) + c_w / 2, y + 12, ATTENTION if i == HUB else ACCENT, 12, title, 700))
    y += 18
    for name, cells in DEVICE_COLS:
        port = name in PORT_ROWS
        h = max(_stack_h(cells.get(i, ()), c_w) for i in range(len(COLUMNS))) + 8
        h = max(h, 36)
        by = band(name, h)
        for i in range(len(COLUMNS)):
            cy = by + 4
            for n in cells.get(i, ()):
                kind = ("hubport" if port else "hub") if i == HUB else ("port" if port else "component")
                part, bh = _box(mx0 + i * (c_w + GAP), cy, c_w, n, kind)
                rows += part
                cy += bh + GAP
        y += h + 3
    h = BOX + 8
    by = band("Board Package", h)
    rows += _full(mx0, mx1, by + 4, BOARD)
    y += h + 3
    h = max(_stack_h(MCU.get(i, ()), c_w) for i in range(len(COLUMNS))) + 8 + BOX + GAP
    by = band("MCU Package", h)
    for i in range(len(COLUMNS)):
        cy = by + 4
        for n in MCU.get(i, ()):
            part, bh = _box(mx0 + i * (c_w + GAP), cy, c_w, n)
            rows += part
            cy += bh + GAP
    rows += _full(mx0, mx1, by + h - BOX - 4, (MCU_STRIP,))
    y += h + 3
    h = BOX + 8
    by = band("Silicon and Vendor", h + 8)
    rows += _full(mx0, mx1, by + 8, SILICON, "vendor")
    y += h + 8
    dev_bottom = y
    # side blocks
    side = []
    mid = dev_top + (dev_bottom - dev_top) * 0.56

    def block(x, top, bottom, width, title, names, colour):
        out = [_rect(x, top, width, bottom - top, LAYER1, rx=4), _rect(x, top, width, 3, colour, rx=1)]
        out.extend(_label(x + 7, top + 3, width - 12, title, colour))
        by = top + 24 + (14 if len(title) * 6.6 > width - 12 else 0)
        for n in names:
            part, bh = _box(x + 5, by, width - 10, n)
            out += part
            by += bh + GAP
        return out

    side += block(10, dev_top, mid - 3, side_w, "Libraries", LIBRARIES, ACCENT)
    side += block(10, mid, dev_bottom, side_w, "Proof", PROOF, PROGRESS)
    side += block(mx1 + 6, dev_top, dev_bottom, right_w, "Direct Hardware Parts", DIRECT, ACCENT)
    # legend
    y += 8
    lx, legend = 10, []
    for kind_fill, stroke, dash, words in ((LAYER3, "none", "", t["component"]), (WATER, ACCENT, "", t["port"]), (LAYER2, ATTENTION, "", t["contract"]),
                                           (LAYER3, ATTENTION, "3 2", t["hub"]), (LAYER2, HAIR, "", t["vendor"])):
        legend.append(_rect(lx, y, 22, 14, kind_fill, stroke, dash=dash))
        legend.append(_text(lx + 28, y + 11.5, MUTED, 11.5, words, 400, "start"))
        lx += 28 + len(words) * (11.5 if lang == "ja" else 6) + 20
    height = y + 24
    head = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {round(height)}" role="img" aria-label="{escape(t["title"])}" style="max-width:100%;height:auto">',
            _rect(0, 0, W, height, INK, rx=0)]
    return "".join(head + body + rows + side + ['<g class="legend">'] + legend + ["</g></svg>"]) + "\n"


def component_table(lang):
    i = 0 if lang == "ja" else 1
    head = {"ja": "| コンポーネント | 設計上の役割 |", "en": "| Component | Role in the design |"}[lang]
    return "\n".join([head, "|---|---|", *(f"| {name} | {lines[i]} |" for name, lines in COMPONENTS.items())]) + "\n"


FIGURES = {"node": node_svg}
# The code's own dependency rule, which site/scripts/test_site.py keeps: these crates name no MCU or board.
NO_MCU = ("dagon-port", "dagon-format", "dagon-proto", "dagon-core", "dagon-boot", "dagon-events", "dagon-agent")
HAL = ("embassy-rp", "embassy-stm32", "esp-hal", "cortex-m")
