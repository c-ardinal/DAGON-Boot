"""DAGON's node firmware as one layered software map: bands, rows of functional stacks, short-named modules.

gen.py writes the figure to assets/architecture/ for the READMEs and, with the table of
abbreviations, into the site's Explanation page, so this file is the one source.
test_site.py checks every module in MODULES against the tree and `cargo metadata`.
"""
import pathlib
import tomllib
from xml.sax.saxutils import escape

ROOT = pathlib.Path(__file__).resolve().parents[2]
W, H = 1400, 600
BOX_H = 30


def _m(role_ja, role_en, module, crate, paths, needle="", extend=False):
    return dict(role={"ja": role_ja, "en": role_en}, module=module, crate=crate, paths=paths, needle=needle, extend=extend)


RP, STM, ESP = "targets/mcu/rp2040/src/", "targets/mcu/stm32f401re/src/", "targets/mcu/esp32s3/src/"
COMMON, PORT, BOARD = "targets/mcu/common/src/", "crates/dagon-port/src/lib.rs", "targets/boards/waveshare-rp2040-touch-lcd-1.28/src/"

# Abbreviation -> what it is, its module and crate. `needle`: text the first path must hold. `extend`: a contributor writes this.
MODULES = {
    # OS and boot
    "Exec": _m("タスクの実行（embassy）", "Task executor (embassy)", "dagon_mcu_*::exec", "dagon-mcu-rp2040", [RP + "exec.rs", STM + "exec.rs"], extend=True),
    "Stage0": _m("最初に動き、ブートローダを選ぶ", "Runs first, picks the boot loader", "stage0", "dagon-rp2040-stage0", ["targets/mcu/rp2040/stage0/src", "targets/mcu/stm32f401re/stage0/src"], extend=True),
    "BootM": _m("ブートの手順", "Boot sequence", "dagon_boot", "dagon-boot", ["crates/dagon-boot/src/lib.rs"]),
    "BootDec": _m("ブートの判断", "Boot decision", "dagon_core::boot", "dagon-core", ["crates/dagon-core/src/boot.rs"]),
    "BootSel": _m("ブートローダ A/B の選択", "Boot loader A/B choice", "dagon_core::bootcore", "dagon-core", ["crates/dagon-core/src/bootcore.rs"]),
    # System services
    "EvtM": _m("イベントの配信とチェーン", "Event delivery and chain", "dagon_events::compose", "dagon-events", ["crates/dagon-events/src/compose.rs"]),
    "StateM": _m("状態機械", "State machines", "dagon_events::machine", "dagon-events", ["crates/dagon-events/src/machine.rs"]),
    "Publish": _m("公開の状態", "Published state", "dagon_events::published", "dagon-events", ["crates/dagon-events/src/published.rs"]),
    "PortList": _m("置き換えられるポートの一覧", "Catalog of replaceable ports", "dagon_events::ports", "dagon-events", ["crates/dagon-events/src/ports.rs"]),
    "Hooks": _m("アプリのチェーン", "The app's chain", "dagon_mcu_common::hooks", "dagon-mcu-common", [COMMON + "hooks.rs"]),
    "WdgM": _m("ウォッチドッグの管理", "Watchdog manager", "dagon_mcu_common::guard", "dagon-mcu-common", [COMMON + "guard.rs"]),
    "Liveness": _m("タスクの生存の確認", "Task liveness", "dagon_mcu_common::liveness", "dagon-mcu-common", [COMMON + "liveness.rs"]),
    "Snapshot": _m("アプリの状態", "App snapshot", "dagon_agent::snapshot", "dagon-agent", ["crates/dagon-agent/src/snapshot.rs"]),
    "View": _m("画面に出す内容", "What a screen shows", "dagon_mcu_rp2040::view", "dagon-mcu-rp2040", [RP + "view.rs"]),
    # Memory services
    "ImgM": _m("イメージのライフサイクル", "Image lifecycle", "dagon_agent::images", "dagon-agent", ["crates/dagon-agent/src/images.rs"]),
    "LogM": _m("状態ログ", "State log", "dagon_core::log", "dagon-core", ["crates/dagon-core/src/log.rs"]),
    # Crypto services
    "Sign": _m("署名の検証", "Signature check", "dagon_format::verify_signed", "dagon-format", ["crates/dagon-format/src/lib.rs"], "pub fn verify_signed"),
    "Digest": _m("ヘッダと本体の検証", "Header and body check", "dagon_format::header", "dagon-format", ["crates/dagon-format/src/header.rs"], "pub fn verify_header"),
    "Key": _m("組み込む公開鍵", "Built-in public key", "dagon_core::key", "dagon-core", ["crates/dagon-core/src/key.rs"]),
    # Communication services
    "Agent": _m("書き込みのエージェント", "Write agent", "dagon_agent::agent", "dagon-agent", ["crates/dagon-agent/src/agent.rs"]),
    "Serve": _m("応答のループ", "Serving loop", "dagon_mcu_common::serve", "dagon-mcu-common", [COMMON + "serve.rs"]),
    "Relay": _m("親機の中継", "The hub's relay", "dagon_mcu_rp2040::relay", "dagon-mcu-rp2040", [RP + "relay/mod.rs"]),
    "Frame": _m("フレームの切り出し", "Cutting frames", "dagon_mcu_common::frames", "dagon-mcu-common", [COMMON + "frames.rs"]),
    "Nodes": _m("子機の一覧", "The list of children", "dagon_mcu_rp2040::nodes", "dagon-mcu-rp2040", [RP + "nodes.rs"]),
    "HubCmd": _m("親機のコマンド", "Hub commands", "dagon_mcu_rp2040::cmd", "dagon-mcu-rp2040", [RP + "cmd.rs"]),
    # Onboard abstraction
    "WdgIf": _m("ウォッチドッグのトレイト", "Watchdog trait", "dagon_port::Watchdog", "dagon-port", [PORT], "pub trait Watchdog"),
    "RstIf": _m("リセットのトレイト", "Reset trait", "dagon_port::Reset", "dagon-port", [PORT], "pub trait Reset"),
    "ClkIf": _m("時間のトレイト", "Clock trait", "dagon_port::Clock", "dagon-port", [PORT], "pub trait Clock"),
    "BootIf": _m("ブート切り替えのトレイト", "Boot select traits", "dagon_port::BootSelectS, BootSelectV", "dagon-port", [PORT], "pub trait BootSelectV"),
    # Memory abstraction
    "FlsIf": _m("フラッシュのトレイト", "Flash trait", "dagon_port::Flash", "dagon-port", [PORT], "pub trait Flash"),
    "SlotStore": _m("スロットのストアのトレイト", "Slot store trait", "dagon_agent::SlotStore", "dagon-agent", ["crates/dagon-agent/src/lib.rs"], "pub trait SlotStore"),
    "SharedFls": _m("フラッシュの共有", "Shared flash", "dagon_mcu_common::shared", "dagon-mcu-common", [COMMON + "shared.rs"]),
    "MappedFls": _m("マップされたフラッシュの読み出し", "Mapped flash reads", "dagon_mcu_common::mapped", "dagon-mcu-common", [COMMON + "mapped.rs"]),
    # Communication abstraction
    "LinkIf": _m("ノードのリンクのトレイト", "Node link trait", "dagon_port::Link", "dagon-port", [PORT], "pub trait Link"),
    "LinkHop": _m("上りと下りの 1 つの面", "One interface for every hop", "dagon_mcu_common::link", "dagon-mcu-common", [COMMON + "link.rs"]),
    "LinkKinds": _m("リンクの種類の一覧", "List of link kinds", "dagon_format::link", "dagon-format", ["crates/dagon-format/src/link.rs"]),
    "I2cIf": _m("I2C バスのトレイト", "I2C bus trait", "dagon_port::I2cBus", "dagon-port", [PORT], "pub trait I2cBus"),
    "StreamIf": _m("バイト列のトレイト", "Byte stream trait", "dagon_port::StreamPort", "dagon-port", [PORT], "pub trait StreamPort"),
    "SubIf": _m("子機の制御線のトレイト", "Child control line trait", "dagon_port::SubControl", "dagon-port", [PORT], "pub trait SubControl"),
    "UartIf": _m("バイト列の上のリクエスト", "Requests over a byte stream", "dagon_mcu_common::uart", "dagon-mcu-common", [COMMON + "uart.rs"]),
    # I/O
    "Bus": _m("共有のバスとデバイス", "Shared bus and devices", "dagon_events::board", "dagon-events", ["crates/dagon-events/src/board.rs"]),
    "Screen": _m("ホーム画面", "Home screen", "dagon_board_*::screen", "dagon-board-waveshare-rp2040-touch-lcd-1-28", [BOARD + "screen.rs"], extend=True),
    "Display": _m("ディスプレイのタスク", "Display task", "dagon_board_*::display", "dagon-board-waveshare-rp2040-touch-lcd-1-28", [BOARD + "display.rs"], extend=True),
    "Gc9a01": _m("LCD のドライバ", "LCD driver", "dagon_board_*::gc9a01", "dagon-board-waveshare-rp2040-touch-lcd-1-28", [BOARD + "gc9a01.rs"], extend=True),
    # MCU drivers
    "Wdg": _m("ウォッチドッグのドライバ", "Watchdog driver", "dagon_mcu_*::watchdog, esp32s3::sys", "dagon-mcu-rp2040", [RP + "watchdog.rs", STM + "watchdog.rs", ESP + "sys.rs"], extend=True),
    "Clock": _m("クロックの設定", "Clock set-up", "dagon_mcu_stm32f401re::clocks", "dagon-mcu-stm32f401re", [STM + "clocks.rs"], extend=True),
    "Trial": _m("試行のセルとウォッチドッグ", "Trial cell and watchdog", "stm32f401re::trial, esp32s3::trial", "dagon-mcu-stm32f401re", [STM + "trial.rs", ESP + "trial.rs"], extend=True),
    "ImgStart": _m("イメージの起動", "Starting an image", "dagon_mcu_rp2040::arm", "dagon-mcu-rp2040", [RP + "arm.rs"], extend=True),
    "OtaData": _m("otadata の読み書き", "otadata", "dagon_mcu_esp32s3::otadata", "dagon-mcu-esp32s3", [ESP + "otadata.rs"], extend=True),
    "BootSelV": _m("ベンダーのブートの切り替え", "Vendor boot select", "dagon_mcu_esp32s3::boot", "dagon-mcu-esp32s3", [ESP + "boot.rs"], extend=True),
    "RomMode": _m("ROM のダウンロードモード", "ROM download mode", "dagon_mcu_esp32s3::rom", "dagon-mcu-esp32s3", [ESP + "rom.rs"], extend=True),
    # Memory drivers
    "Fls": _m("フラッシュのドライバ", "Flash driver", "dagon_mcu_*::flash", "dagon-mcu-rp2040", [RP + "flash.rs", STM + "flash.rs", ESP + "flash.rs"], extend=True),
    "Store": _m("スロットのストアの実装", "Slot store implementation", "dagon_mcu_*::store", "dagon-mcu-rp2040", [RP + "store.rs", STM + "store.rs", ESP + "store.rs"], extend=True),
    # Communication drivers
    "UsbCdc": _m("USB CDC", "USB CDC", "dagon_mcu_rp2040::usb, esp32s3::links", "dagon-mcu-rp2040", [RP + "usb.rs", ESP + "links.rs"], extend=True),
    "I2c": _m("I2C のコントローラ", "I2C controller", "dagon_mcu_rp2040::links", "dagon-mcu-rp2040", [RP + "links.rs"], extend=True),
    "I2cTgt": _m("I2C のターゲット", "I2C target", "dagon_mcu_stm32f401re::mailbox", "dagon-mcu-stm32f401re", [STM + "mailbox.rs"], extend=True),
    "Uart": _m("UART", "UART", "dagon_mcu_esp32s3::links", "dagon-mcu-esp32s3", [ESP + "links.rs"], extend=True),
    "PioUart": _m("PIO の UART", "PIO UART", "dagon_mcu_rp2040::links", "dagon-mcu-rp2040", [RP + "links.rs"], extend=True),
    "SubCtl": _m("子機の制御線", "Child control lines", "dagon_mcu_rp2040::sub", "dagon-mcu-rp2040", [RP + "sub.rs"], extend=True),
    # Trusted boot extensions
    "BootExt": _m("信頼されたブート拡張", "Trusted boot extension", "targets/boot-extension-template", "hub-boot-screen", ["targets/boot-extension-template", "samples/hub-boot-screen"], extend=True),
    # Libraries
    "Format": _m("バイトのレイアウト", "Byte layouts", "dagon_format", "dagon-format", ["crates/dagon-format/src/lib.rs"]),
    "Proto": _m("リンクのプロトコル", "Link protocol", "dagon_proto", "dagon-proto", ["crates/dagon-proto/src/lib.rs"]),
    "Crc": _m("CRC-32", "CRC-32", "dagon_format::crc32", "dagon-format", ["crates/dagon-format/src/lib.rs"], "pub fn crc32"),
    "Sha": _m("SHA-256", "SHA-256", "dagon_format::Sha256", "dagon-format", ["crates/dagon-format/src/lib.rs"], "pub struct Sha256"),
    "Ed25519": _m("Ed25519", "Ed25519", "ed25519-compact", "ed25519-compact", ["crates/dagon-format/Cargo.toml"], "ed25519-compact"),
    "LE": _m("リトルエンディアンの読み書き", "Little-endian access", "dagon_format::le", "dagon-format", ["crates/dagon-format/src/le.rs"]),
    "Version": _m("イメージのバージョン", "Image version", "dagon_format::version", "dagon-format", ["crates/dagon-format/src/version.rs"]),
    # Common
    "McuDef": _m("マイコンの定義", "MCU definition", "targets/mcu/*/dagon-mcu.toml", "dagon-topology", ["targets/mcu/rp2040/dagon-mcu.toml"], extend=True),
    "BrdDef": _m("ボードの定義", "Board definition", "targets/boards/*/dagon-board.toml", "dagon-topology", ["targets/boards/nucleo-f401re/dagon-board.toml"], extend=True),
    "MemMap": _m("フラッシュの配置", "Flash layout", "dagon_mcu_*::layout", "dagon-mcu-rp2040", [RP + "layout.rs", STM + "layout.rs", ESP + "layout.rs"], extend=True),
    "Pins": _m("定義から生成するピンと定数", "Pins and constants from the definition", "dagon_topology::defs::codegen", "dagon-topology", ["crates/dagon-topology/src/defs/codegen.rs"]),
    "Wiring": _m("生成するブートの配線", "Generated boot wiring", "dagon_cli::extgen", "dagon-cli", ["crates/dagon-cli/src/extgen.rs"], "boot_wiring"),
    "Tag": _m("ノードのターゲットのタグ", "Node's target tag", "dagon_mcu_common::tag", "dagon-mcu-common", [COMMON + "tag.rs"]),
}

COLORS = dict(bg="#ffffff", app="#5f6b7a", api="#c0392b", mcu="#10161d", services="#5b4bb0", abstraction="#1f7a5a", drivers="#a83e66",
              libs="#a8660a", common="#1f6fb0", box="#d9600b", text="#ffffff", legend="#3a4450")

WORDS = {
    "ja": {"app": "アプリ層", "api": "DAGON の API（イベント、フック、ポート）", "mcu": "マイコン", "libs": "ライブラリ", "common": "共通", "os": "OS とブート",
           "sys": "システムサービス", "mem": "メモリサービス", "cry": "暗号サービス", "com": "通信サービス", "ob": "オンボードの抽象化", "mema": "メモリの抽象化", "coma": "通信の抽象化",
           "io": "I/O の抽象化", "mcud": "マイコンのドライバ", "memd": "メモリのドライバ", "comd": "通信のドライバ", "iod": "I/O のドライバ", "cdd": "信頼された\nブート拡張",
           "legend": "点線の箱は、マイコン、ボード、拡張を足すときに書くものです。各箱の一覧はドキュメントの「しくみ」にあります。"},
    "en": {"app": "Application Layer", "api": "DAGON API (events, hooks, ports)", "mcu": "Microcontroller", "libs": "Libraries", "common": "Common", "os": "OS, boot",
           "sys": "System Services", "mem": "Memory Services", "cry": "Crypto Services", "com": "Communication Services", "ob": "Onboard Abstraction", "mema": "Memory Abstraction", "coma": "Communication Abstraction",
           "io": "I/O Abstraction", "mcud": "MCU Drivers", "memd": "Memory Drivers", "comd": "Communication Drivers", "iod": "I/O Drivers", "cdd": "Trusted Boot\nExtensions",
           "legend": "Dotted boxes are what you write to add an MCU, board or extension. The list of boxes is in the docs' Explanation."},
}

STYLE = (f"<style>.a-b{{font:600 12.5px 'Segoe UI',system-ui,'Noto Sans JP',sans-serif;fill:{COLORS['text']}}}"
         f".a-t{{font:700 13.5px 'Segoe UI',system-ui,'Noto Sans JP',sans-serif;fill:{COLORS['text']}}}"
         f".a-l{{font:12px 'Segoe UI',system-ui,'Noto Sans JP',sans-serif;fill:{COLORS['legend']}}}</style>")


def _t(x, y, cls, words, anchor="middle"):
    return f'<text x="{round(x, 1)}" y="{round(y, 1)}" class="{cls}" text-anchor="{anchor}">{escape(words)}</text>'


def _rect(x, y, w, h, fill):
    return f'<rect x="{round(x, 1)}" y="{round(y, 1)}" width="{round(w, 1)}" height="{round(h, 1)}" fill="{fill}"/>'


def _box(x, y, w, h, name):
    dotted = ' stroke="#ffffff" stroke-width="1.2" stroke-dasharray="2 2"' if MODULES[name]["extend"] else ""
    return (f'<rect x="{round(x, 1)}" y="{round(y, 1)}" width="{round(w, 1)}" height="{round(h, 1)}" rx="2" fill="{COLORS["box"]}"{dotted}/>'
            + _t(x + w / 2, y + h / 2 + 4.5, "a-b", name))


def _width(name):
    return max(52, 8.2 * len(name) + 16)


def _flow(x, y, w, h, names):
    """Boxes wrapped into rows, centred in the area."""
    rows, row, used = [], [], 0
    for n in names:
        bw = _width(n)
        if row and used + bw + 6 > w:
            rows.append((row, used))
            row, used = [], 0
        row.append(n)
        used += bw + (6 if used else 0)
    rows.append((row, used))
    total = len(rows) * BOX_H + (len(rows) - 1) * 7
    out = []
    by = y + (h - total) / 2
    for row, used in rows:
        bx = x + (w - used) / 2
        for n in row:
            out.append(_box(bx, by, _width(n), BOX_H, n))
            bx += _width(n) + 6
        by += BOX_H + 7
    return out


def _cell(x0, x1, y0, y1, fill, title, names):
    lines = title.split("\n")
    out = [_rect(x0, y0, x1 - x0, y1 - y0, fill)]
    out += [_t((x0 + x1) / 2, y0 + 18 + i * 17, "a-t", line) for i, line in enumerate(lines)]
    top = y0 + 8 + 17 * len(lines)
    return out + _flow(x0 + 8, top, x1 - x0 - 16, y1 - top - 4, names)


def mcu_names():
    return [tomllib.loads(p.read_text(encoding="utf-8"))["name"] for p in sorted((ROOT / "targets/mcu").glob("*/dagon-mcu.toml"))]


def node_svg(lang):
    w = WORDS[lang]
    c = COLORS
    # rows and columns
    top, y_abs, y_drv, y_end = 96, 292, 408, 546
    os_x0, os_x1 = 86, 196
    cols = dict(sys=(200, 440), mem=(444, 590), cry=(594, 720), com=(724, 1106), io=(1110, 1252), cd=(1256, 1394))
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" role="img" aria-label="{escape(w["app"])}" style="max-width:100%;height:auto">',
           STYLE, _rect(0, 0, W, H, c["bg"])]
    # the two side blocks
    out += _cell(6, 82, 6, 300, c["libs"], w["libs"], [])
    for i, n in enumerate(("Format", "Proto", "Crc", "Sha", "Ed25519", "LE", "Version")):
        out.append(_box(12, 32 + i * 37, 64, 30, n))
    out += _cell(6, 82, 304, y_end, c["common"], w["common"], [])
    for i, n in enumerate(("McuDef", "BrdDef", "MemMap", "Pins", "Wiring", "Tag")):
        out.append(_box(12, 330 + i * 35, 64, 30, n))
    # bands
    out += [_rect(86, 6, W - 92, 40, c["app"]), _t((86 + W - 6) / 2, 31, "a-t", w["app"])]
    out += [_rect(86, 50, W - 92, 42, c["api"]), _t((86 + W - 6) / 2, 76, "a-t", w["api"])]
    # OS and boot, Services down to Drivers
    out.append(_rect(os_x0, top, os_x1 - os_x0, y_end - top, c["services"]))
    out.append(_t((os_x0 + os_x1) / 2, top + 18, "a-t", w["os"]))
    out.append(_box(os_x0 + 5, top + 28, 34, y_end - top - 34, "Exec"))
    for i, n in enumerate(("Stage0", "BootM", "BootDec", "BootSel")):
        out.append(_box(os_x0 + 43, top + 28 + i * 40, 62, 32, n))
    # Services
    S = c["services"]
    out += _cell(*cols["sys"], top, y_abs - 4, S, w["sys"], ["EvtM", "StateM", "Publish", "PortList", "Hooks", "WdgM", "Liveness", "Snapshot", "View"])
    out += _cell(*cols["mem"], top, y_abs - 4, S, w["mem"], ["ImgM", "LogM"])
    out += _cell(*cols["cry"], top, y_abs - 4, S, w["cry"], ["Sign", "Digest", "Key"])
    out += _cell(*cols["com"], top, y_abs - 4, S, w["com"], ["Agent", "Serve", "Relay", "Frame", "Nodes", "HubCmd"])
    # Abstraction: nothing under Crypto, so Memory takes its width
    A = c["abstraction"]
    out += _cell(*cols["sys"], y_abs, y_drv - 4, A, w["ob"], ["WdgIf", "RstIf", "ClkIf", "BootIf"])
    out += _cell(cols["mem"][0], cols["cry"][1], y_abs, y_drv - 4, A, w["mema"], ["FlsIf", "SlotStore", "SharedFls", "MappedFls"])
    out += _cell(*cols["com"], y_abs, y_drv - 4, A, w["coma"], ["LinkIf", "LinkHop", "LinkKinds", "I2cIf", "StreamIf", "SubIf", "UartIf"])
    out += _cell(*cols["io"], top, y_drv - 4, A, w["io"], ["Bus", "Screen"])
    # Drivers
    D = c["drivers"]
    out += _cell(*cols["sys"], y_drv, y_end, D, w["mcud"], ["Wdg", "Clock", "Trial", "ImgStart", "OtaData", "BootSelV", "RomMode"])
    out += _cell(cols["mem"][0], cols["cry"][1], y_drv, y_end, D, w["memd"], ["Fls", "Store"])
    out += _cell(*cols["com"], y_drv, y_end, D, w["comd"], ["UsbCdc", "I2c", "I2cTgt", "Uart", "PioUart", "SubCtl"])
    out += _cell(*cols["io"], y_drv, y_end, D, w["iod"], ["Display", "Gc9a01"])
    # Complex drivers: the whole height
    out += _cell(*cols["cd"], top, y_end, A, w["cdd"], ["BootExt"])
    # Microcontroller
    out += [_rect(86, 550, W - 92, 28, c["mcu"]), _t((86 + W - 6) / 2, 569, "a-t", f"{w['mcu']} ({', '.join(mcu_names())})")]
    out.append(_t(6, 594, "a-l", w["legend"], "start"))
    out.append("</svg>")
    return "".join(out) + "\n"


def legend_table(lang):
    head = {"ja": "| 略称 | 役割 | モジュール | crate |", "en": "| Abbreviation | What it is | Module | Crate |"}[lang]
    rows = [f"| {a} | {m['role'][lang]} | `{m['module']}` | `{m['crate']}` |" for a, m in MODULES.items()]
    return "\n".join([head, "|---|---|---|---|", *rows]) + "\n"


FIGURES = {"node": node_svg}
NO_MCU = ("dagon-port", "dagon-format", "dagon-proto", "dagon-core", "dagon-boot", "dagon-events", "dagon-agent")
HAL = ("embassy-rp", "embassy-stm32", "esp-hal", "cortex-m")
