"""DAGON's software architecture, for someone who wants to change it: the node firmware's
layers and functional stacks, module by module, and the PC side's layers.

gen.py writes the figures to assets/architecture/ for the READMEs and into the site's
Explanation page, so this file is the one source. test_site.py checks every source path
named, every dependency arrow and every image tag against the tree and `cargo metadata`.
"""
from xml.sax.saxutils import escape

CORE, TRAIT, DATA = "core", "trait", "data"
MCU = "targets/mcu/{}/src/{}"


def box(id, kind, ja, en, sub_ja, sub_en=None, pkg=(), mark=""):
    return dict(id=id, kind=kind, title={"ja": ja, "en": en}, sub={"ja": sub_ja, "en": sub_ja if sub_en is None else sub_en}, pkg=(pkg,) if isinstance(pkg, str) else pkg, mark=mark)


def m(ja, en, shown, pkg, paths, images="", kind=CORE):
    """One module: its role, its crate or module in small text, the package and source paths the test checks, the images using it."""
    return dict(title={"ja": ja, "en": en}, shown=shown, pkg=pkg, paths=paths, images=images, kind=kind)


STACKS = {"ja": ("通信", "メモリ", "書き込みとブート", "システム"), "en": ("Communication", "Memory", "Write and boot", "System")}
# Layers top to bottom; each holds one cell per stack, each cell its modules.
LAYERS = [
    dict(name=("アプリ", "Application"), rule=("公開 API だけを使います。", "Public API only."),
         cells=[[m("あなたのアプリ", "Your app", "nodes/<id>/src/app.rs", "f401-app", ["targets/mcu/stm32f401re/template/src/app.rs"], "A", TRAIT)], [],
                [], [m("拡張", "Extensions", "dagon-extension.toml", "hub-boot-screen", ["targets/boot-extension-template"], "BA", TRAIT)]]),
    dict(name=("インターフェース", "Interface"), rule=("アプリと拡張が触れる\n面です。イベントを作る\nのは DAGON だけです。", "All an app or extension\nmay touch. Only DAGON\nmakes events."),
         cells=[[], [m("イメージの読み取り", "Image view", "dagon_agent::ImageView", "dagon-agent", ["crates/dagon-agent/src/images.rs"], "A")],
                [m("起動", "Start", "node::start, App", "dagon-mcu-*", ["targets/mcu/stm32f401re/src/node.rs", "targets/mcu/common/src/hooks.rs"], "A")],
                [m("拡張のトレイト", "Extension trait", "dagon_events::extension", "dagon-events", ["crates/dagon-events/src/extension.rs"], "BA"),
                 m("イベントの受信", "Event reader", "dagon_events::Reader", "dagon-events", ["crates/dagon-events/src/published.rs"], "A")]]),
    dict(name=("サービス", "Services"), rule=("crates/ の中はマイコン、\nボード、サンプルを\n知りません。下の\nトレイトだけに\n依存します。", "crates/ knows no MCU,\nno board and no\nsample; it relies on\nthe traits below."),
         cells=[[m("プロトコル", "Protocol", "dagon_proto", "dagon-proto", ["crates/dagon-proto/src/lib.rs"], "A"),
                 m("フレームの切り出し", "Framing", "dagon_mcu_common::frames", "dagon-mcu-common", ["targets/mcu/common/src/frames.rs"], "A"),
                 m("応答のループ", "Serving loop", "dagon_mcu_common::serve", "dagon-mcu-common", ["targets/mcu/common/src/serve.rs"], "A"),
                 m("リレー（親機）", "Relay (hub)", "dagon_mcu_rp2040::relay", "dagon-mcu-rp2040", ["targets/mcu/rp2040/src/relay"], "A")],
                [m("イメージのストア", "Image store", "dagon_agent::images", "dagon-agent", ["crates/dagon-agent/src/images.rs"], "A"),
                 m("状態ログ", "State log", "dagon_core::log", "dagon-core", ["crates/dagon-core/src/log.rs"], "SBA"),
                 m("ログの形式", "Log format", "dagon_format::statelog", "dagon-format", ["crates/dagon-format/src/statelog.rs"], "SBA")],
                [m("書き込みのエージェント", "Write agent", "dagon_agent::agent", "dagon-agent", ["crates/dagon-agent/src/agent.rs"], "A"),
                 m("署名と本体の検証", "Signature, body check", "dagon_format::header", "dagon-format", ["crates/dagon-format/src/header.rs"], "SBA"),
                 m("ブートの判断", "Boot decision", "dagon_core::boot", "dagon-core", ["crates/dagon-core/src/boot.rs"], "B"),
                 m("ブートローダの選択", "Boot loader choice", "dagon_core::bootcore", "dagon-core", ["crates/dagon-core/src/bootcore.rs"], "SBA"),
                 m("ブートの手順", "Boot sequence", "dagon_boot", "dagon-boot", ["crates/dagon-boot/src/lib.rs"], "B"),
                 m("ウォッチドッグのガード", "Watchdog guard", "dagon_mcu_common::guard", "dagon-mcu-common", ["targets/mcu/common/src/guard.rs"], "A")],
                [m("イベントと状態機械", "Events, state machines", "dagon_events::machine", "dagon-events", ["crates/dagon-events/src/machine.rs"], "BA"),
                 m("チェーンの合成", "Chain composition", "dagon_events::compose", "dagon-events", ["crates/dagon-events/src/compose.rs"], "BA"),
                 m("公開の状態", "Published state", "dagon_events::published", "dagon-events", ["crates/dagon-events/src/published.rs"], "A"),
                 m("画面の内容", "Screen contents", "dagon_mcu_rp2040::view", "dagon-mcu-rp2040", ["targets/mcu/rp2040/src/view.rs"], "A")]]),
    dict(name=("抽象化", "Abstraction"), rule=("トレイトと定義です。\nマイコンとボードの\n知識は、ここから下に\nだけ置きます。", "Traits and definitions.\nMCU and board\nknowledge lives from\nhere down."),
         cells=[[m("リンクのトレイト", "Link traits", "Link, StreamPort, I2cBus", "dagon-port", ["crates/dagon-port/src/lib.rs"], "A"),
                 m("1 つのリンクの面", "One link interface", "dagon_mcu_common::link", "dagon-mcu-common", ["targets/mcu/common/src/link.rs"], "A"),
                 m("リンクの種類（閉じた一覧）", "Link kinds (closed list)", "dagon_format::link", "dagon-format", ["crates/dagon-format/src/link.rs"], "A")],
                [m("フラッシュのトレイト", "Flash trait", "dagon_port::Flash", "dagon-port", ["crates/dagon-port/src/lib.rs"], "SBA"),
                 m("スロットのストアのトレイト", "Slot store trait", "dagon_agent::SlotStore", "dagon-agent", ["crates/dagon-agent/src/lib.rs"], "A"),
                 m("フラッシュの共有", "Shared flash", "dagon_mcu_common::shared", "dagon-mcu-common", ["targets/mcu/common/src/shared.rs"], "A")],
                [m("時間とリセットのトレイト", "Watchdog, reset traits", "Watchdog, Reset, Clock", "dagon-port", ["crates/dagon-port/src/lib.rs"], "SBA"),
                 m("ブート切り替えのトレイト", "Boot select traits", "BootSelectS, BootSelectV", "dagon-port", ["crates/dagon-port/src/lib.rs"], "BA")],
                [m("置き換えられるポート", "Replaceable ports", "dagon_events::ports", "dagon-events", ["crates/dagon-events/src/ports.rs"], "BA"),
                 m("マイコンの定義", "MCU definition", "dagon-mcu.toml", "dagon-topology", ["targets/mcu/rp2040/dagon-mcu.toml"], "SBA", DATA),
                 m("ボードの定義", "Board definition", "dagon-board.toml", "dagon-topology", ["targets/boards/nucleo-f401re/dagon-board.toml"], "BA", DATA)]]),
    dict(name=("マイコンのドライバ", "MCU drivers"), rule=("トレイトの実装です。\nマイコンとボードの\n定義のコードだけが\nHAL を使います。", "Trait implementations.\nOnly MCU and board\ncode uses the HAL."),
         cells=[[m("USB CDC", "USB CDC", "dagon_mcu_rp2040::usb", "dagon-mcu-rp2040", ["targets/mcu/rp2040/src/usb.rs"], "A", TRAIT),
                 m("I2C", "I2C", "rp2040::links, stm32f401re::mailbox", "dagon-mcu-*", ["targets/mcu/rp2040/src/links.rs", "targets/mcu/stm32f401re/src/mailbox.rs"], "A", TRAIT),
                 m("UART", "UART", "common::uart, esp32s3::links", "dagon-mcu-*", ["targets/mcu/common/src/uart.rs", "targets/mcu/esp32s3/src/links.rs"], "A", TRAIT)],
                [m("フラッシュのドライバ", "Flash driver", "dagon_mcu_*::flash", "dagon-mcu-*", ["targets/mcu/rp2040/src/flash.rs", "targets/mcu/stm32f401re/src/flash.rs", "targets/mcu/esp32s3/src/flash.rs"], "SBA", TRAIT),
                 m("スロットのストア", "Slot store", "dagon_mcu_*::store", "dagon-mcu-*", ["targets/mcu/rp2040/src/store.rs", "targets/mcu/stm32f401re/src/store.rs", "targets/mcu/esp32s3/src/store.rs"], "A", TRAIT),
                 m("フラッシュの配置", "Flash layout", "dagon_mcu_*::layout", "dagon-mcu-*", ["targets/mcu/rp2040/src/layout.rs", "targets/mcu/stm32f401re/src/layout.rs", "targets/mcu/esp32s3/src/layout.rs"], "SBA", TRAIT)],
                [m("ウォッチドッグのドライバ", "Watchdog driver", "dagon_mcu_*::watchdog", "dagon-mcu-*", ["targets/mcu/rp2040/src/watchdog.rs", "targets/mcu/stm32f401re/src/watchdog.rs", "targets/mcu/esp32s3/src/sys.rs"], "SBA", TRAIT),
                 m("試行のセル", "Trial cell", "dagon_mcu_stm32f401re::trial", "dagon-mcu-stm32f401re", ["targets/mcu/stm32f401re/src/trial.rs"], "SBA", TRAIT),
                 m("イメージの起動", "Starting an image", "rp2040::arm, esp32s3::boot", "dagon-mcu-*", ["targets/mcu/rp2040/src/arm.rs", "targets/mcu/esp32s3/src/boot.rs"], "SBA", TRAIT)],
                [m("実行とクロック", "Executor, clocks", "dagon_mcu_*::exec, ::clocks", "dagon-mcu-*", ["targets/mcu/rp2040/src/exec.rs", "targets/mcu/stm32f401re/src/clocks.rs"], "BA", TRAIT),
                 m("ディスプレイ", "Display", "dagon_board_*::display, ::gc9a01", "dagon-board-waveshare-rp2040-touch-lcd-1-28", ["targets/boards/waveshare-rp2040-touch-lcd-1.28/src/display.rs", "targets/boards/waveshare-rp2040-touch-lcd-1.28/src/gc9a01.rs"], "A", TRAIT)]]),
]
HAL = ("embassy-rp", "embassy-stm32", "esp-hal", "cortex-m")
# One arrow per stack between layers, each a Cargo dependency a test finds; the order is the stacks'.
LAYER_DEPS = [
    [("f401-app", "dagon-mcu-stm32f401re"), None, None, ("hub-boot-screen", "dagon-events")],
    [None, ("dagon-agent", "dagon-format"), ("dagon-mcu-stm32f401re", "dagon-agent"), ("dagon-mcu-stm32f401re", "dagon-events")],
    [("dagon-mcu-common", "dagon-port"), ("dagon-core", "dagon-port"), ("dagon-boot", "dagon-port"), ("dagon-events", "dagon-format")],
    [("dagon-mcu-rp2040", "dagon-port"), ("dagon-mcu-stm32f401re", "dagon-port"), ("dagon-mcu-esp32s3", "dagon-port"), ("dagon-board-waveshare-rp2040-touch-lcd-1-28", "dagon-mcu-rp2040")],
    [("dagon-mcu-rp2040", "embassy-rp"), ("dagon-mcu-stm32f401re", "embassy-stm32"), ("dagon-mcu-esp32s3", "esp-hal"), ("dagon-mcu-rp2040", "cortex-m")],
]
# Each image's packages: a module tagged with an image is in their dependencies.
IMAGE_PKGS = {"S": ("dagon-rp2040-stage0", "dagon-stm32f401re-stage0"), "B": ("dagon-rp2040-bootcore", "dagon-stm32f401re-bootcore"),
              "A": ("f401-app", "rp2040-hub", "dagon-mcu-esp32s3")}
# A trusted boot extension reaches from the interface to the HAL, past the sandbox.
TRUSTED = ("hub-boot-screen", ("dagon-events", "embassy-rp"))
# crates/ that may name no MCU or board.
NO_MCU = ("dagon-port", "dagon-format", "dagon-proto", "dagon-core", "dagon-boot", "dagon-events", "dagon-agent")

PC = [
    dict(name=("画面とコマンド", "Page and command"),
         rule=("同じ手順を共有します。\n手順をここに書きません。", "Share one set of procedures;\nnone are written here."),
         rows=[[box("console", CORE, "DAGON Console", "DAGON Console", "web/"),
                box("cli", CORE, "dagon コマンド", "dagon command", "dagon-cli", pkg="dagon-cli")]]),
    dict(name=("API", "API"),
         rule=("ページはここを通して\nファイルとデバイスに届きます。", "The page reaches files and\ndevices through here."),
         rows=[[box("serve", CORE, "serve API", "serve API", "dagon-cli::serve", pkg="dagon-cli"),
                box("wasm", CORE, "ページの Rust", "Rust in the page", "dagon-wasm", pkg="dagon-wasm"),
                box("agents", DATA, "エージェントの定義", "Agent definitions", "~/.dagon/agents/*.toml")]]),
    dict(name=("手順", "Procedures"),
         rule=("Transport の先を知りません。", "Knows nothing past Transport."),
         rows=[[box("host", CORE, "dagon-host", "dagon-host", "Host, Transport", pkg="dagon-host"),
                box("conform", CORE, "dagon-conform", "dagon-conform", "適合試験", "conformance tests", "dagon-conform"),
                box("romlink", CORE, "dagon-romlink", "dagon-romlink", "ROM ローダ", "ROM loader", "dagon-romlink")]]),
    dict(name=("形式と署名", "Formats, signing"),
         rule=("純粋なライブラリで、\n転送も画面も知りません。", "Plain libraries: no\ntransport, no page."),
         rows=[[box("sign", CORE, "dagon-sign", "dagon-sign", "鍵、署名", "keys, signing", "dagon-sign"),
                box("topology", CORE, "dagon-topology", "dagon-topology", "構成、定義", "setup, definitions", "dagon-topology"),
                box("pproto", CORE, "dagon-proto", "dagon-proto", "リンクのプロトコル", "link protocol", "dagon-proto"),
                box("pformat", CORE, "dagon-format", "dagon-format", "バイトのレイアウト", "byte layouts", "dagon-format")]]),
    dict(name=("転送", "Transports"),
         rule=("dagon_host::Transport を\n実装するだけです。", "Implement\ndagon_host::Transport only."),
         rows=[[box("serial", TRAIT, "シリアル", "Serial", "dagon-cli::serial"),
                box("webserial", TRAIT, "Web Serial", "Web Serial", "dagon-wasm::host"),
                box("sim", TRAIT, "シミュレータ", "Simulator", "dagon-cli::simrun, dagon-sim")]]),
]
PC_EDGES = [("cli", "host"), ("cli", "sign"), ("cli", "topology"), ("wasm", "host"), ("wasm", "pproto"), ("wasm", "pformat"), ("host", "conform"),
            ("host", "romlink"), ("host", "pproto"), ("host", "pformat"), ("sign", "topology"), ("sign", "pformat"), ("pproto", "pformat"), ("topology", "pformat")]
# Not Cargo dependencies: the page's calls, and transports implementing the trait.
PC_LINKS = [("console", "serve"), ("console", "wasm"), ("serial", "host"), ("webserial", "host"), ("sim", "host")]

LEGEND = {
    "ja": (("固定の中核", "トレイトを実装するコード", "データだけ"),
           "S: stage0（リセットの後で最初に動き、ブートローダを選ぶ）  B: ブートローダ A/B  A: アプリ A/B。矢印は Cargo の依存の向きです。",
           "ESP32-S3 は、stage0 とブートローダの代わりにベンダーの 2nd stage が起動します。"),
    "en": (("Fixed core", "Code implementing a trait", "Data only"),
           "S: stage0 (runs first after a reset and picks the boot loader)  B: boot loader A/B  A: app A/B. Arrows are Cargo dependencies.",
           "On the ESP32-S3 the vendor's 2nd stage starts the app, in place of stage0 and the boot loader."),
}
TEXT = {
    "ja": {"trusted": "信頼されたブート拡張（特権で動き、HAL に直接触れます）", "node": "ノードのファームウェア", "pc": "PC 側", "pc_note": "矢印は Cargo の依存です。点線は呼び出しと、トレイトの実装です。"},
    "en": {"trusted": "Trusted boot extension (privileged, reaches the HAL itself)", "node": "Node firmware", "pc": "PC side", "pc_note": "Arrows are Cargo dependencies; dashed lines are calls and trait implementations."},
}

STYLE = """<style>
.dg-bg{fill:#fff}.dg-band{fill:#f1f4f8;stroke:#c9d1dc}
.dg-core{fill:#fff;stroke:#7b8794}.dg-trait{fill:#e3effb;stroke:#2f6fb0;stroke-width:2;stroke-dasharray:6 3}.dg-data{fill:#e2f4e6;stroke:#2d8a4e;stroke-width:2}
.dg-t{fill:#16202c;font:600 14px system-ui,'Segoe UI','Noto Sans JP',sans-serif}
.dg-s{fill:#46525f;font:12.5px system-ui,'Segoe UI','Noto Sans JP',sans-serif}
.dg-h{fill:#16202c;font:700 15px system-ui,'Segoe UI','Noto Sans JP',sans-serif}
.dg-a{stroke:#6b7886;stroke-width:1.4;fill:none;marker-end:url(#dg-arrow)}.dg-d{stroke-dasharray:4 3}.dg-m{fill:#6b7886}
.dg-g{fill:#2f6fb0;font:700 12px system-ui,'Segoe UI','Noto Sans JP',sans-serif;letter-spacing:1px}
@media (prefers-color-scheme:dark){.dg-bg{fill:#0f151c}.dg-band{fill:#1a232e;stroke:#37455a}
.dg-core{fill:#0f151c;stroke:#6d7c8f}.dg-trait{fill:#14283d;stroke:#6db1f2}.dg-data{fill:#12301d;stroke:#5cc985}
.dg-t,.dg-h{fill:#eef2f7}.dg-s{fill:#b4bfcb}.dg-a{stroke:#8d9bab}.dg-m{fill:#8d9bab}.dg-g{fill:#6db1f2}}
</style>"""
BOX_H, ROW_GAP, PAD = 54, 26, 12
TOP = 44


def _text(x, y, cls, value, anchor="middle"):
    return f'<text x="{round(x, 1)}" y="{round(y, 1)}" class="{cls}" text-anchor="{anchor}">{escape(value)}</text>'


def _head(w, h, label):
    return [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" role="img" aria-label="{escape(label)}" style="max-width:100%;height:auto">', STYLE,
            '<defs><marker id="dg-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" class="dg-m"/></marker></defs>',
            f'<rect width="{w}" height="{h}" class="dg-bg"/>']


def _stack(lang, layers, x0, x1, label_w):
    """Lays out the bands; returns (band svg, box svg, positions by id, band rows y, bottom)."""
    i = 0 if lang == "ja" else 1
    bands, boxes, pos, ranges = [], [], {}, []
    y = TOP
    for layer in layers:
        rows = layer["rows"]
        h = PAD * 2 + len(rows) * BOX_H + (len(rows) - 1) * ROW_GAP
        bands.append(f'<rect x="{x0}" y="{y}" width="{x1 - x0}" height="{h}" rx="8" class="dg-band"/>')
        bands.append(_text(x0 + 10, y + 24, "dg-h", layer["name"][i], "start"))
        for n, line in enumerate(layer["rule"][i].split("\n")):
            bands.append(_text(x0 + 10, y + 44 + n * 16, "dg-s", line, "start"))
        by = y + PAD
        for row in rows:
            left, width = x0 + label_w, x1 - x0 - label_w - 10
            w = (width - 8 * (len(row) - 1)) / len(row)
            for n, b in enumerate(row):
                bx = left + n * (w + 8)
                pos[b["id"]] = (bx, by, w)
                boxes.append(f'<rect x="{round(bx, 1)}" y="{by}" width="{round(w, 1)}" height="{BOX_H}" rx="6" class="dg-{b["kind"]}"/>')
                boxes.append(_text(bx + w / 2, by + 22, "dg-t", b["title"][lang]))
                boxes.append(_text(bx + w / 2, by + 41, "dg-s", b["sub"][lang]))
            by += BOX_H + ROW_GAP
        ranges.append((y, h))
        y += h + 10
    return bands, boxes, pos, ranges, y


def _arrow(pos, a, b, dashed=False):
    ax, ay, aw = pos[a]
    bx, by, bw = pos[b]
    cls = "dg-a dg-d" if dashed else "dg-a"
    if ay == by:  # same row
        (x1, x2) = (ax, bx + bw) if bx < ax else (ax + aw, bx)
        return f'<path d="M{round(x1, 1)} {ay + BOX_H / 2}L{round(x2, 1)} {by + BOX_H / 2}" class="{cls}"/>'
    down = by > ay
    sy, ey = (ay + BOX_H, by) if down else (ay, by + BOX_H)
    sx = ax + aw / 2 + (bx + bw / 2 - ax - aw / 2) * 0.15
    ex = bx + bw / 2 + (ax + aw / 2 - bx - bw / 2) * 0.15
    return f'<path d="M{round(sx, 1)} {sy}L{round(ex, 1)} {ey}" class="{cls}"/>'


def _legend(lang, y):
    kinds, line, note = LEGEND[lang]
    out, x = [], 16
    for kind, words in zip((CORE, TRAIT, DATA), kinds):
        out.append(f'<rect x="{x}" y="{y}" width="26" height="16" rx="3" class="dg-{kind}"/>')
        out.append(_text(x + 32, y + 13, "dg-s", words, "start"))
        x += 260
    out.append(_text(16, y + 36, "dg-s", line, "start"))
    out.append(_text(16, y + 56, "dg-s", note, "start"))
    return out


MOD_H, MOD_GAP = 46, 6


def node_svg(lang):
    i = 0 if lang == "ja" else 1
    w, label_w, stack_w, gap, bar_w = 1272, 154, 256, 6, 34
    x0 = 16 + label_w
    stack_x = [x0 + n * (stack_w + gap) for n in range(4)]
    out, y, ranges, gaps = [], 52, [], []
    for s_x, name in zip(stack_x, STACKS[lang]):
        out.append(_text(s_x + stack_w / 2, 40, "dg-h", name))
    body = []
    for layer in LAYERS:
        rows = max(len(c) for c in layer["cells"])
        h = 16 + rows * MOD_H + (rows - 1) * MOD_GAP
        body.append(f'<rect x="16" y="{y}" width="{x0 + 4 * stack_w + 3 * gap + 8 - 16}" height="{h}" rx="8" class="dg-band"/>')
        body.append(_text(26, y + 24, "dg-h", layer["name"][i], "start"))
        for n, line in enumerate(layer["rule"][i].split("\n")):
            body.append(_text(26, y + 44 + n * 16, "dg-s", line, "start"))
        for s_x, cell in zip(stack_x, layer["cells"]):
            for n, mod in enumerate(cell):
                by = y + 8 + n * (MOD_H + MOD_GAP)
                body.append(f'<rect x="{s_x}" y="{by}" width="{stack_w}" height="{MOD_H}" rx="5" class="dg-{mod["kind"]}"/>')
                body.append(_text(s_x + 10, by + 19, "dg-t", mod["title"][lang], "start"))
                body.append(_text(s_x + 10, by + 37, "dg-s", mod["shown"], "start"))
                body.append(_text(s_x + stack_w - 8, by + 18, "dg-g", " ".join(mod["images"]), "end"))
        ranges.append((y, h))
        y += h + 22
    # the HAL under the drivers, across the stacks
    hal_y = y
    body.append(f'<rect x="16" y="{hal_y}" width="{x0 + 4 * stack_w + 3 * gap + 8 - 16}" height="44" rx="8" class="dg-band"/>')
    body.append(_text(26, hal_y + 27, "dg-h", "HAL", "start"))
    for s_x, name in zip(stack_x, HAL):
        body.append(f'<rect x="{s_x}" y="{hal_y + 7}" width="{stack_w}" height="30" rx="5" class="dg-core"/>')
        body.append(_text(s_x + stack_w / 2, hal_y + 27, "dg-t", name))
    ends = [r[0] + r[1] for r in ranges]
    starts = [r[0] for r in ranges[1:]] + [hal_y]
    arrows = []
    for deps, e, st in zip(LAYER_DEPS, ends, starts):
        for s_x, dep in zip(stack_x, deps):
            if dep:
                arrows.append(f'<path d="M{s_x + stack_w / 2} {e - 2}L{s_x + stack_w / 2} {st}" class="dg-a"/>')
    bottom = hal_y + 44
    h = bottom + 82
    bx = x0 + 4 * stack_w + 3 * gap + 14
    top = ranges[1][0]
    trusted = [f'<rect x="{bx}" y="{top}" width="{bar_w}" height="{bottom - top}" rx="6" class="dg-trait"/>',
               f'<text transform="translate({bx + 23} {(top + bottom) / 2}) rotate(-90)" class="dg-t" text-anchor="middle">{escape(TEXT[lang]["trusted"])}</text>']
    head = _head(w, h, TEXT[lang]["node"]) + [_text(16, 26, "dg-h", TEXT[lang]["node"], "start")]
    return "".join(head + out + body + arrows + trusted + _legend(lang, bottom + 12) + ["</svg>"]) + "\n"


def pc_svg(lang):
    w = 1000
    bands, boxes, pos, _, bottom = _stack(lang, PC, 16, w - 16, 216)
    h = bottom + 26
    out = _head(w, h, TEXT[lang]["pc"]) + [_text(16, 28, "dg-h", TEXT[lang]["pc"], "start")] + bands
    out += [_arrow(pos, a, b) for a, b in PC_EDGES] + [_arrow(pos, a, b, True) for a, b in PC_LINKS] + boxes
    out.append(_text(16, bottom + 12, "dg-s", TEXT[lang]["pc_note"], "start"))
    out.append("</svg>")
    return "".join(out) + "\n"


FIGURES = {"node": node_svg, "pc": pc_svg}


def pc_boxes():
    return {b["id"]: b for layer in PC for row in layer["rows"] for b in row}


def modules():
    return [mod for layer in LAYERS for cell in layer["cells"] for mod in cell]
