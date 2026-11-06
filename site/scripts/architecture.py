"""DAGON's software architecture: the layered design, as one figure and one table of components.

This is the design, kept by hand here; it is not read from the code. gen.py writes the figure
to assets/architecture/ for the READMEs and, with the table, into the site's Explanation page.
"""
from xml.sax.saxutils import escape

# web/src/styles/tokens.css
INK, LAYER1, LAYER2, LAYER3, PAPER, MUTED, HAIR = "#0a0a0a", "#141413", "#1e1e1c", "#282725", "#f1efe8", "#9a9892", "#2c2b29"
ACCENT, WATER, ATTENTION, PROGRESS = "#3cb4f0", "#123a4c", "#f5a524", "#b48cf2"
COLOURS = (INK, LAYER1, LAYER2, LAYER3, PAPER, MUTED, HAIR, ACCENT, WATER, ATTENTION, PROGRESS)
FONT = "'Zen Kaku Gothic New','Hiragino Sans','Yu Gothic UI','Noto Sans JP',system-ui,sans-serif"

W = 1400
COLUMNS = ("Boot", "Image and Storage", "Update and Comms", "Hub", "Trust", "Liveness", "Isolation", "Observability")
HUB = 3

PC_ROWS = (
    ("PC Front Ends", "full", ("dagon CLI", "dagon serve", "DAGON Console", "Claude Code Plugin")),
    ("PC Host Extensions", "full", ("Host Extension Chain", "Action Definitions", "Agent Definitions")),
    ("PC Host Services", "cols", (("Project Generator", "Definition Registry", "Topology Check"), ("Node Build",), ("Operations",), ("Host Machines",),
                                  ("Release Builder",), ("AI Agent Link",), ("Protection Planner",), ("Event Stream", "Catalog"))),
    ("PC Host Ports", "cols", ((), ("BuildRunner",), ("Transport",), ("confirm_plan",), ("Signer", "KeyStore"), ("Agent",), (), ("Action",))),
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
                        3: ("Relay", "Coordinator", "Release Store", "Node Recovery", "Bus Guardian", "Topology Copy"),
                        5: ("Liveness Guard", "Reboot Gate"),
                        6: ("Entry Gate", "Service ABI", "Operation Cancellation", "Fault Unwind", "Lockup Fuse", "Reset Cleanup"),
                        7: ("Publisher", "Boot Record Writer", "Boot Replay", "VERSION Responder")}),
    ("Core Logic", {0: ("Boot Decision", "Core Chooser"), 1: ("State Log Fold",), 3: ("Delivery Plan", "Recovery Policy", "Constraint Evaluator"),
                    4: ("Trust Rule",), 5: ("Deadline Book",)}),
    ("Replaceable Ports", {0: ("DecisionPolicy", "BoardBringUp"), 1: ("SlotStore", "LogStorage", "SelfCheck"), 2: ("UpdateTransport", "UpdateProtocol", "Recovery"),
                           3: ("ChildLink",), 4: ("ImageVerifier", "Digest", "CryptoProvider"), 5: ("WatchdogDriver",), 7: ("Diagnostics", "Display", "LogSink")}),
    ("Driver Contract", {0: ("BootSelect",), 1: ("Flash",), 3: ("I2cBus", "StreamPort", "SubControl"), 5: ("Clock", "Reset"), 6: ("SandboxArch",)}),
)
BOARD = ("Board Definition", "Board Bring-Up", "Board Extension", "Boot Action Program", "Shared Bus")
MCU = {0: ("Reset and Jump",), 1: ("Flash Driver", "Slot Store Driver"), 2: ("Link Drivers",), 5: ("Watchdog Driver", "Execution Setup"), 6: ("Architecture Kind",)}
MCU_STRIP = "MCU Definition"
SILICON = ("ROM Loader", "boot2", "Vendor 2nd Stage", "Protection Unit", "Peripherals")
LIBRARIES = ("dagon-format", "dagon-proto", "dagon-events", "Retained Codecs")
PROOF = ("dagon-conform", "dagon-sim", "dagon-retained-model")
DIRECT = ("ROM Loader Client", "Trusted Extension")

PORT_ROWS = ("PC Host Ports", "Replaceable Ports", "Driver Contract")

# Component -> one line (Japanese, English).
COMPONENTS = {
    "dagon CLI": ("バイナリ名 dagon のコマンド", "The binary named dagon"),
    "dagon serve": ("Console のためのローカルのサーバ", "The local server for the Console"),
    "DAGON Console": ("ブラウザの UI", "The browser UI"),
    "Claude Code Plugin": ("移植とトポロジ生成のスキル、サブエージェント、フック", "Porting and topology skills, subagents and hooks"),
    "Host Extension Chain": ("ホストの拡張を順に呼ぶチェーン", "The chain of host extensions, in order"),
    "Action Definitions": ("イベントの後で動かすアクションのデータ", "Data for actions run after an event"),
    "Agent Definitions": ("エージェントとそのつなぎ方のデータ", "Data for agents and their ways"),
    "Project Generator": ("—", "—"),
    "Definition Registry": ("対応ボードの表を持つ", "Holds the supported-board table"),
    "Topology Check": ("構成の YAML を読み、検証規則を適用する", "Reads the topology YAML and applies the validation rules"),
    "Node Build": ("dagon.toml の `[build.<id>]` に従ってノードごとに cargo build する", "Runs cargo build per node from `[build.<id>]` in dagon.toml"),
    "Operations": ("apply、status、recover の手順", "The apply, status and recover procedures"),
    "Host Machines": ("ホストの操作の状態機械", "State machines of host operations"),
    "Release Builder": ("検査の後でマニフェストを署名し、パッケージファイル（.dgb）を作る", "Signs the manifest after its checks and makes the package file (.dgb)"),
    "AI Agent Link": ("dagon ai のコマンド", "The dagon ai commands"),
    "Protection Planner": ("拡張の保護マップの計画と確認", "Plans and checks extensions' protection maps"),
    "Event Stream": ("ホストのイベントの配信", "Delivers host events"),
    "Catalog": ("すべてのイベントとポートの一覧", "Lists every event and port"),
    "BuildRunner": ("ビルドを実行するポート", "Port that runs builds"),
    "Transport": ("親機へのバイト列のポート", "Port for the byte stream to the hub"),
    "confirm_plan": ("書き込みの前の確認の判断点", "Decision point before a write"),
    "Signer": ("署名のポート", "Port that signs"),
    "KeyStore": ("署名の鍵の置き場所のポート", "Port for where the signing key is kept"),
    "Agent": ("エージェントのポート", "Port for an agent"),
    "Action": ("アクションのポート", "Port for an action"),
    "Cargo Runner": ("cargo で動くビルド", "Builds through cargo"),
    "Serial": ("CLI のシリアルポートの Transport", "The CLI's serial port Transport"),
    "Web Serial": ("dagon-wasm の Web Serial の Transport", "dagon-wasm's Web Serial Transport"),
    "Simulator Link": ("dagon-sim の模擬リンク", "dagon-sim's simulated link"),
    "ROM Writers": ("PC の dagon-romlink による ROM の手順", "ROM procedures on the PC through dagon-romlink"),
    "File Key Pair": ("ファイルの鍵の組", "Key pair in files"),
    "DAGON Link": ("ホスト、親機、ノードのあいだのフレームとコマンド", "Frames and commands between host, hub and nodes"),
    "Release Bundle": ("パッケージファイル（.dgb）の形式", "Format of the package file (.dgb)"),
    "Image Header": ("署名付きのイメージのヘッダ", "Header of a signed image"),
    "Topology Binary": ("親機が持つ構成の形式", "Format of the setup the hub holds"),
    "VERSION": ("ノードのバージョンと状態の問い合わせ", "Query of a node's versions and state"),
    "User Application": ("利用者のアプリ", "The user's app"),
    "User Extensions": ("利用者の拡張", "The user's extensions"),
    "Library Extensions": ("拡張のチェーンの中ほど", "The middle of the extension chain"),
    "Boot Start": ("ブートのレイヤーの起動", "Starts the boot layer"),
    "App Start": ("アプリのレイヤーの起動", "Starts the app layer"),
    "Relay Start": ("リレーのレイヤーの起動", "Starts the relay layer"),
    "Chain Composer": ("拡張のチェーンの合成", "Composes the extension chain"),
    "Extension API": ("拡張が触れる面", "What an extension may touch"),
    "Stage0 Sequence": ("リセットの後で最初に動く手順", "The first steps after a reset"),
    "Boot Sequence": ("ブートローダの手順", "The boot loader's steps"),
    "Boot Action Interpreter": ("ブートのアクションプログラムの実行", "Runs boot action programs"),
    "Image Lifecycle": ("スロットのイメージの状態機械", "State machine of a slot's image"),
    "Update Session": ("書き込みのセッションの状態機械", "State machine of a write session"),
    "Wire Protocol": ("リンクのメッセージとセッションの対応", "Maps link messages onto the session"),
    "Serving Loop": ("リクエストに答えるループ", "The loop that answers requests"),
    "Recovery Entry": ("復旧の経路への入り口", "Entry to a recovery path"),
    "Relay": ("要求をホップごとに中継し、子機の有無を追跡し、進み具合の表示の状態を持つ", "Forwards requests hop by hop, tracks child presence and holds progress display state"),
    "Coordinator": ("保存したリリースをノードに対して実行する", "Runs a stored release across nodes"),
    "Release Store": ("リリースの面 N/P とその昇格を持つ", "Holds the release faces N/P and their promotion"),
    "Node Recovery": ("ノードごとの復旧の段階と、回数に上限のある再試行", "Per-node recovery tiers with bounded retries"),
    "Bus Guardian": ("共有のバスを直列化し、バスの固まりと切り離しを扱う", "Serialises a shared bus; handles a stuck bus and isolation"),
    "Topology Copy": ("親機が持つ構成のコピー。アトミックに置き換え、親機が自分でピンを動かす前に署名付きマニフェストに照らして確かめる", "The hub's copy of the 構成, replaced atomically and checked against the signed manifest before the hub drives pins by itself"),
    "Liveness Guard": ("タスクの生存を条件とするキック", "Kicks the watchdog on task liveness"),
    "Reboot Gate": ("先に返答し、上限のある遅れを待ってからリセットする", "Replies first, waits a bounded delay, then resets"),
    "Entry Gate": ("拡張の呼び出しの入り口", "Entry to an extension call"),
    "Service ABI": ("拡張から DAGON への唯一の呼び出し", "An extension's only call into DAGON"),
    "Operation Cancellation": ("途中の特権の操作の取り消し", "Cancels a privileged operation part-way"),
    "Fault Unwind": ("フォールトと超過からの戻り", "Returns from a fault or overrun"),
    "Lockup Fuse": ("ロックアップした拡張の記録", "Records an extension that locked up"),
    "Reset Cleanup": ("リセットの後のペリフェラルの初期化", "Reinitialises peripherals after a reset"),
    "Publisher": ("待てる公開の状態を書く", "Writes the waitable published state"),
    "Boot Record Writer": ("ブートのイベントを保持メモリに書く", "Writes boot events to retained memory"),
    "Boot Replay": ("起動時にブートのイベントを出し直す", "Re-emits boot events at start"),
    "VERSION Responder": ("VERSION に答える", "Answers VERSION"),
    "Boot Decision": ("どのスロットを起動するかの判断", "Decides which slot starts"),
    "Core Chooser": ("stage0 がブートローダのスロットを選ぶ", "stage0 picks the boot loader slot"),
    "State Log Fold": ("記録から状態を畳み込む", "Folds records into state"),
    "Delivery Plan": ("順序、試行の窓、昇格の判断", "Order, trial window, promotion judgement"),
    "Recovery Policy": ("状態と失敗の回数から復旧の段階を選ぶ", "Chooses a recovery tier from state and failure counts"),
    "Constraint Evaluator": ("マニフェストの制約を確かめる", "Checks manifest constraints"),
    "Trust Rule": ("署名と鍵の 1 つの認証の決まり", "The one rule for signatures and keys"),
    "Deadline Book": ("ガードの後ろの純粋な期限の計算", "Pure deadline accounting behind the guard"),
    "DecisionPolicy": ("ブートの方針のポート", "Port for boot policy"),
    "BoardBringUp": ("ボードの立ち上げのポート", "Port for bringing up the board"),
    "SlotStore": ("イメージの置き場所のポート", "Port for where images live"),
    "LogStorage": ("状態ログの置き場所のポート", "Port for where the state log lives"),
    "SelfCheck": ("確定のときの自己検査のポート", "Port for the self-check at confirm"),
    "UpdateTransport": ("リンクのバイト列のポート", "Port for a link's bytes"),
    "UpdateProtocol": ("メッセージのポート", "Port for messages"),
    "Recovery": ("追加の復旧の経路のポート", "Port for an added recovery path"),
    "ChildLink": ("親機から子機へのリンクのポート", "Port for the hub's link to a child"),
    "ImageVerifier": ("ヘッダの署名の検証のポート", "Port for verifying a header's signature"),
    "Digest": ("本体のハッシュのポート", "Port for the body hash"),
    "CryptoProvider": ("暗号の基本要素のポート", "Port for crypto primitives"),
    "WatchdogDriver": ("ウォッチドッグのポート", "Port for the watchdog"),
    "Diagnostics": ("追加の診断のポート", "Port for added diagnostics"),
    "Display": ("表示のポート", "Port for display"),
    "LogSink": ("ログの行き先のポート", "Port for where logs go"),
    "BootSelect": ("kind S はジャンプ、kind V はベンダーの選択情報の操作", "Kind S: the jump; kind V: the vendor's selection data"),
    "Flash": ("フラッシュの消去、書き込み、読み出し", "Flash erase, write and read"),
    "I2cBus": ("親機の I2C バス", "The hub's I2C bus"),
    "StreamPort": ("親機のバイト列", "The hub's byte stream"),
    "SubControl": ("子機の制御線", "A child's control lines"),
    "Clock": ("時間", "Time"),
    "Reset": ("システムのリセット", "System reset"),
    "SandboxArch": ("1 つのアーキテクチャの特権の入口とアンワインドのレジスタ", "Privilege entry and unwind registers for one architecture"),
    "Board Definition": ("ボードのデータ", "The board's data"),
    "Board Bring-Up": ("ボードの電源、ピン、バスの準備", "Prepares the board's power, pins and buses"),
    "Board Extension": ("ボードが持つ拡張", "The board's own extension"),
    "Boot Action Program": ("ブートの宣言的なプログラム", "Declarative program at boot"),
    "Shared Bus": ("複数の者が使うボードのバス", "A board bus several parties use"),
    "Reset and Jump": ("リセットとイメージへのジャンプ", "Reset, and the jump to an image"),
    "Flash Driver": ("フラッシュのドライバ", "The flash driver"),
    "Slot Store Driver": ("kind S のログ、または kind V のベンダーデータ", "A kind S log or kind V vendor data"),
    "Link Drivers": ("ノードの Link の実装", "Implement the node's Link"),
    "Watchdog Driver": ("ウォッチドッグのドライバ", "The watchdog driver"),
    "Execution Setup": ("実行モデルに従うエグゼキュータの優先度", "Executor priorities per the execution model"),
    "Architecture Kind": ("保護ハードウェアの種類ごとの実装", "Implementation per protection kind"),
    "MCU Definition": ("マイコンのデータ", "The MCU's data"),
    "ROM Loader": ("マイコンの ROM のローダ", "The MCU's ROM loader"),
    "boot2": ("RP2040 のブート ROM の次に動く 256 バイト", "The 256 bytes after the RP2040's boot ROM"),
    "Vendor 2nd Stage": ("ベンダーの 2nd stage", "The vendor's 2nd stage"),
    "Protection Unit": ("メモリの保護ハードウェア", "Memory protection hardware"),
    "Peripherals": ("マイコンのペリフェラル", "The MCU's peripherals"),
    "dagon-format": ("イメージヘッダ、マニフェスト、状態ログ、トポロジのバイト配置", "Byte layouts of the image header, manifest, state log and topology"),
    "dagon-proto": ("フレーム、コマンド、エラーの定義", "Frames, commands and errors"),
    "dagon-events": ("イベント、状態機械、拡張", "Events, state machines, extensions"),
    "Retained Codecs": ("ブート記録、ヒューズ、構成の本体の形式", "Boot record, fuse and topology body formats"),
    "dagon-conform": ("実機とシミュレータに共通の適合試験", "Conformance tests shared by hardware and the simulator"),
    "dagon-sim": ("模擬フラッシュ、模擬リンク、模擬 ROM ローダ、故障注入", "Simulated flash, links and ROM loader, with fault injection"),
    "dagon-retained-model": ("保持の形式の実行可能なモデル", "Executable model of the retained formats"),
    "ROM Loader Client": ("別のマイコンの ROM ローダを動かす", "Drives another MCU's ROM loader"),
    "Trusted Extension": ("特権で動く拡張", "An extension that runs privileged"),
}

WORDS = {
    "ja": {"title": "DAGON のソフトウェアアーキテクチャ", "component": "コンポーネント", "port": "ポートと取り決め", "contract": "ノードと PC の共通の形式", "hub": "Hub の列は親機のイメージだけ", "vendor": "シリコンとベンダー"},
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
    head = {"ja": "| コンポーネント | 役割 |", "en": "| Component | What it is |"}[lang]
    return "\n".join([head, "|---|---|", *(f"| {name} | {lines[i]} |" for name, lines in COMPONENTS.items())]) + "\n"


FIGURES = {"node": node_svg}
