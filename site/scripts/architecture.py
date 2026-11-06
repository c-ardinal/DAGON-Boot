"""The layer diagram of DAGON, one SVG per language: node side, PC side and the links.

gen.py writes it to assets/architecture/ for the READMEs and into the site's
Explanation page, so this file is the one source. A name in the diagram is a
crate or folder of this repository; test_site.py checks that they exist.
"""
from xml.sax.saxutils import escape

W, H = 1120, 640
NODE_X, NODE_W = 16, 760
PC_X, PC_W = 796, 308
BAND_H, GAP, TOP = 92, 10, 44
LABEL_W = 112

TEXT = {
    "ja": {
        "node": "ノード（親機も子機も同じ層）",
        "pc": "PC",
        "layers": [
            ("アプリ", [("あなたのアプリ", "ノードの app.rs"), ("拡張", "dagon-extension.toml")]),
            ("DAGON の\nサービス", [("dagon-agent", "受信、検証、確定"), ("リレー（親機だけ）", "targets/mcu/rp2040"),
                                ("dagon-events", "イベントとポート"), ("dagon-proto", "dagon-format")]),
            ("ブート", [("stage0", "ブートローダを選ぶ"), ("ブートローダ A/B", "dagon-boot, dagon-core"), ("アプリ A/B", "試行し、戻れる")]),
            ("マイコンと\nボード", [("targets/mcu/*", "dagon-mcu.toml"), ("targets/boards/*", "dagon-board.toml"),
                               ("dagon-port", "ボードのトレイト"), ("HAL", "embassy, esp-hal")]),
        ],
        "hardware": "ハードウェア",
        "hub": ("親機", "RP2040"),
        "children": [("子機", "STM32F401RE"), ("子機", "ESP32-S3")],
        "pc_boxes": [("DAGON Console", "web/, dagon-wasm"), ("dagon コマンド", "dagon-cli, dagon serve"), ("dagon-host", "親機とのやり取りの手順"),
                     ("dagon-sign", "鍵、署名、パッケージ"), ("dagon-topology", "構成の確認"), ("dagon-sim", "ボードなしで試す相手")],
        "note": "ESP32-S3 では、stage0 とブートローダの代わりに、ベンダーの 2nd stage が起動します。",
    },
    "en": {
        "node": "Node (the hub and the children have the same layers)",
        "pc": "PC",
        "layers": [
            ("App", [("Your app", "the node's app.rs"), ("Extensions", "dagon-extension.toml")]),
            ("DAGON\nservices", [("dagon-agent", "receive, verify, confirm"), ("Relay (hub only)", "targets/mcu/rp2040"),
                                 ("dagon-events", "events and ports"), ("dagon-proto", "dagon-format")]),
            ("Boot", [("stage0", "picks the boot loader"), ("Boot loader A/B", "dagon-boot, dagon-core"), ("App A/B", "tried, can go back")]),
            ("MCU and\nboard", [("targets/mcu/*", "dagon-mcu.toml"), ("targets/boards/*", "dagon-board.toml"),
                                ("dagon-port", "board traits"), ("HAL", "embassy, esp-hal")]),
        ],
        "hardware": "Hardware",
        "hub": ("Hub", "RP2040"),
        "children": [("Child", "STM32F401RE"), ("Child", "ESP32-S3")],
        "pc_boxes": [("DAGON Console", "web/, dagon-wasm"), ("dagon command", "dagon-cli, dagon serve"), ("dagon-host", "steps that talk to the hub"),
                     ("dagon-sign", "keys, signing, packages"), ("dagon-topology", "checks the setup"), ("dagon-sim", "a partner without boards")],
        "note": "On the ESP32-S3 the vendor's 2nd stage starts the app, in place of stage0 and the boot loader.",
    },
}

STYLE = """<style>
.dg-bg{fill:#ffffff}.dg-band{fill:#f1f4f8;stroke:#c9d1dc}.dg-box{fill:#ffffff;stroke:#7b8794}
.dg-t{fill:#16202c;font:600 14px system-ui,'Segoe UI','Noto Sans JP',sans-serif}
.dg-s{fill:#46525f;font:12px system-ui,'Segoe UI','Noto Sans JP',sans-serif}
.dg-h{fill:#16202c;font:700 15px system-ui,'Segoe UI','Noto Sans JP',sans-serif}
.dg-a{stroke:#2f6fb0;stroke-width:2;fill:none;marker-end:url(#dg-arrow)}.dg-m{fill:#2f6fb0}
.dg-l{fill:#2f6fb0;font:600 12px system-ui,'Segoe UI','Noto Sans JP',sans-serif}
@media (prefers-color-scheme:dark){.dg-bg{fill:#0f151c}.dg-band{fill:#1a232e;stroke:#37455a}.dg-box{fill:#0f151c;stroke:#6d7c8f}
.dg-t,.dg-h{fill:#eef2f7}.dg-s{fill:#aab5c2}.dg-a{stroke:#6db1f2}.dg-m,.dg-l{fill:#6db1f2}}
</style>"""


def text(x, y, cls, value, anchor="middle"):
    return f'<text x="{x}" y="{y}" class="{cls}" text-anchor="{anchor}">{escape(value)}</text>'


def box(x, y, w, h, title, sub):
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="6" class="dg-box"/>'
            + text(x + w / 2, y + h / 2 - 3, "dg-t", title) + text(x + w / 2, y + h / 2 + 15, "dg-s", sub))


def label(x, y, words):
    lines = words.split("\n")
    return "".join(text(x, y + 20 * i - 10 * (len(lines) - 1), "dg-h", line, "start") for i, line in enumerate(lines))


def svg(lang):
    t = TEXT[lang]
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" role="img" aria-label="{escape(t["node"])}" style="max-width:100%;height:auto">',
           STYLE,
           '<defs><marker id="dg-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" class="dg-m"/></marker></defs>',
           f'<rect width="{W}" height="{H}" class="dg-bg"/>',
           text(NODE_X, 28, "dg-h", t["node"], "start"), text(PC_X, 28, "dg-h", t["pc"], "start")]
    y = TOP
    for name, boxes in t["layers"]:
        out.append(f'<rect x="{NODE_X}" y="{y}" width="{NODE_W}" height="{BAND_H}" rx="8" class="dg-band"/>')
        out.append(label(NODE_X + 12, y + BAND_H / 2 + 5, name))
        inner = NODE_W - LABEL_W - 12
        w = (inner - 8 * (len(boxes) - 1)) / len(boxes)
        for i, (title, sub) in enumerate(boxes):
            out.append(box(NODE_X + LABEL_W + i * (w + 8), y + 12, round(w, 1), BAND_H - 24, title, sub))
        y += BAND_H + GAP
    # hardware: the children on the left, the hub by the PC side
    hw_h = 136
    out.append(f'<rect x="{NODE_X}" y="{y}" width="{NODE_W}" height="{hw_h}" rx="8" class="dg-band"/>')
    out.append(label(NODE_X + 12, y + hw_h / 2 + 5, t["hardware"]))
    cx, cw, ch = NODE_X + LABEL_W, 200, 54
    hx, hw = NODE_X + NODE_W - 212, 200
    hub_mid = y + hw_h / 2
    for i, ((title, sub), link) in enumerate(zip(t["children"], ("I2C", "UART"))):
        cy = y + 10 + i * (ch + 8)
        out.append(box(cx, cy, cw, ch, title, sub))
        mid = cy + ch / 2
        out.append(f'<path d="M{hx} {hub_mid + (i * 2 - 1) * 16}H{cx + cw + 60}V{mid}H{cx + cw + 2}" class="dg-a"/>')
        out.append(text(cx + cw + 30, mid - 6, "dg-l", link))
    out.append(box(hx, y + 10, hw, hw_h - 20, *t["hub"]))
    # PC side
    n = len(t["pc_boxes"])
    py = TOP
    ph = (y - TOP - GAP - 8 * (n - 1)) / n
    for title, sub in t["pc_boxes"]:
        out.append(box(PC_X, round(py, 1), PC_W, round(ph, 1), title, sub))
        py += ph + 8
    usb_y = hub_mid
    out.append(f'<rect x="{PC_X}" y="{y + 10}" width="{PC_W}" height="{hw_h - 20}" rx="6" class="dg-box"/>')
    out.append(text(PC_X + PC_W / 2, usb_y + 5, "dg-t", "USB"))
    out.append(f'<path d="M{PC_X} {usb_y}H{hx + hw + 2}" class="dg-a"/>')
    out.append(text(NODE_X, H - 14, "dg-s", t["note"], "start"))
    out.append("</svg>")
    return "".join(out) + "\n"
