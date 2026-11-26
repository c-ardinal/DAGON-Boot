"""Import a board from a Zephyr checkout as a DAGON project board.

    python3 scripts/import_zephyr_board.py <zephyr> <board.dts> --id <id> --mcu <mcu>

`<zephyr>` is the root of a Zephyr tree pinned to a release tag, `<board.dts>`
the board file under it. The result is a `[board.<id>]` table for a project's
dagon.toml, on stdout, whose head names the tag and the sha256 of every file
read, so the same tree gives the same text.

What Zephyr's devicetree says is taken over: the product name, the flash
size, the header pins and their printed names, the I2C pin pairs, the UART
buses, the LEDs and the buttons. What only DAGON decides is not: the default
link, the slots, the writer, the boot LED and its halt pattern, and which
bus carries DAGON's own link. Those stay with whoever adds the board.

The text is read as it stands, without the C preprocessor: `#include` lines
are followed when the file is in the tree, and the pin names come from the
names Zephyr gives its pin control nodes (`I2C0_SDA_P4`, `i2c1_scl_pb8`).
A board whose pins are named another way is refused, not guessed.
"""
import argparse
import hashlib
import re
import sys
from pathlib import Path

INCLUDE = re.compile(r'^[ \t]*#include[ \t]+(?:"([^"]+)"|<([^>]+)>)[ \t]*$', re.M)
BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.S)
# One row of a connector's `gpio-map`, with the printed name Zephyr keeps in
# the comment after it.
MAP_ROW = re.compile(
    r"<\s*\w+\s+\w+\s+&(\w+)\s+(\w+)\s+\w+\s*>\s*[,;]?[ \t]*(?:/\*\s*(.*?)\s*\*/)?"
)
OVERLAY = re.compile(r"&(\w+)\s*\{")
GPIOS = re.compile(r"gpios\s*=\s*<\s*&(\w+)\s+([^\s>]+)")
# RP2040 pin control macro: I2C0_SDA_P4.
RP_MUX = re.compile(r"\b[A-Z][A-Z0-9]*_([A-Z0-9]+)_P(\d+)\b")
# STM32 pin control node: i2c1_scl_pb8.
ST_MUX = re.compile(r"^[a-z][a-z0-9]*_([a-z0-9]+)_p([a-k])(\d+)$")
SIZE = re.compile(r"^DT_SIZE_([KM])\((\d+)\)$")
I2C = re.compile(r"^i2c\d+$")
UART = re.compile(r"^u(?:s)?art\d+$")
INCLUDE_ROOTS = ("dts", "dts/arm", "dts/common", "include")


class ImportError_(Exception):
    """A board this importer does not read."""


def pin_name(port, number):
    """DAGON's name of pin `number` (a cell as written, decimal or
    hexadecimal) of Zephyr GPIO node `port`."""
    try:
        number = int(str(number), 0)
    except ValueError:
        raise ImportError_(f"pin {number} of {port} is not a number") from None
    if port == "gpio0":
        return f"GPIO{number}"
    m = re.fullmatch(r"gpio([a-k])", port)
    if m:
        return f"P{m.group(1).upper()}{number}"
    raise ImportError_(f"GPIO node {port} is not one this importer names pins from")


def read_tree(root, start):
    """The text of `start` with every include found in `root` put in its
    place, the files read (in order) and the includes not found."""
    read, missing = [], []

    def load(path):
        rel = path.relative_to(root).as_posix()
        if rel in read:
            return ""
        read.append(rel)
        text = path.read_text(encoding="utf-8")

        def put(m):
            quoted, angled = m.group(1), m.group(2)
            if quoted:
                found = [path.parent / quoted]
            else:
                found = [root / r / angled for r in INCLUDE_ROOTS]
            for f in found:
                if f.is_file():
                    return load(f.resolve())
            missing.append(quoted or angled)
            return ""

        return INCLUDE.sub(put, text)

    root = root.resolve()
    return load(start.resolve()), read, missing


def body_at(text, brace):
    """The text between the `{` at `brace` and its `}`."""
    depth = 0
    for i in range(brace, len(text)):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return text[brace + 1 : i]
    raise ImportError_("a `{` is never closed")


def own(body):
    """`body` without its child nodes: the node's own properties."""
    out, depth = [], 0
    for ch in body:
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        elif depth == 0:
            out.append(ch)
    return "".join(out)


def prop(body, name):
    """The value of property `name` in `body`, as written."""
    m = re.search(rf"(?<![\w-]){re.escape(name)}\s*=\s*([^;]*);", body)
    return m.group(1).strip() if m else None


def number(text):
    """A devicetree cell as a number, or None."""
    text = text.strip()
    m = SIZE.match(text)
    if m:
        return int(m.group(2)) * (1024 if m.group(1) == "K" else 1024 * 1024)
    try:
        return int(text, 0)
    except ValueError:
        return None


def overlays(text):
    """`{label: own properties}` of every `&label { ... }`, the later text
    of one label after the earlier."""
    found = {}
    for m in OVERLAY.finditer(text):
        found.setdefault(m.group(1), []).append(own(body_at(text, m.end() - 1)))
    return {k: "\n".join(v) for k, v in found.items()}


def last_prop(body, name):
    """The last value given to `name` in `body`: a later overlay wins."""
    all_ = re.findall(rf"(?<![\w-]){re.escape(name)}\s*=\s*([^;]*);", body)
    return all_[-1].strip() if all_ else None


def signals(text, refs):
    """`{signal: pin}` of the pin control nodes `refs` name."""
    out = {}
    for ref in refs:
        st = ST_MUX.match(ref)
        if st:
            out[st.group(1)] = pin_name(f"gpio{st.group(2)}", st.group(3))
            continue
        node = re.search(rf"\b{re.escape(ref)}\s*:\s*[\w-]+\s*\{{", text)
        if not node:
            raise ImportError_(f"pin control node {ref} is not in the files read")
        muxes = RP_MUX.findall(body_at(text, node.end() - 1))
        if not muxes:
            raise ImportError_(f"pin control node {ref} names no pin this importer reads")
        for signal, pin in muxes:
            out[signal.lower()] = pin_name("gpio0", pin)
    return out


def stated(text, label):
    """The own properties of node `label` wherever the files state them,
    in source order: its declarations (`label: name { ... }`) and its
    overlays (`&label { ... }`). A later one wins."""
    found = re.finditer(rf"(?:\b{re.escape(label)}\s*:\s*[\w@-]+|&{re.escape(label)})\s*\{{", text)
    return "\n".join(own(body_at(text, m.end() - 1)) for m in found)


def flash_bytes(text):
    """The size of `flash0`: the last `reg` the files give it."""
    reg = last_prop(stated(text, "flash0"), "reg")
    cells = re.fullmatch(r"<\s*(\S+)\s+(.+?)\s*>", reg or "")
    return (number(cells.group(2)) if cells else None) or None


def header(raw):
    """`[(pin, label)]` of every connector row, a pin once."""
    rows, seen = [], set()
    for m in re.finditer(r"gpio-map\s*=\s*(.*?>\s*;[ \t]*(?:/\*.*?\*/)?)", raw, re.S):
        for port, num, label in MAP_ROW.findall(m.group(1)):
            pin = pin_name(port, num)
            if pin not in seen:
                seen.add(pin)
                rows.append((pin, label))
    return rows


def named_pins(text, compatible):
    """`[(node label, pin)]` of the children of every `compatible` node."""
    out = []
    for m in re.finditer(rf'compatible\s*=\s*"{compatible}"\s*;', text):
        start = text.rfind("{", 0, m.start())
        for child in re.finditer(r"(\w+)\s*:\s*[\w-]+\s*\{", body_at(text, start)):
            # An overlay of the child (`&led0 { gpios = ... }`) rewires it.
            given = last_prop(stated(text, child.group(1)), "gpios")
            g = GPIOS.search(f"gpios = {given}") if given else None
            if g:
                out.append((child.group(1).lower(), pin_name(g.group(1), g.group(2))))
    return out


def full_name(root, start):
    yml = start.parent / "board.yml"
    if yml.is_file():
        m = re.search(r"^\s*full_name:\s*(.+?)\s*$", yml.read_text(encoding="utf-8"), re.M)
        if m:
            return m.group(1).strip("\"'"), yml.resolve().relative_to(root.resolve()).as_posix()
    return None, None


def quote(text):
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def hex_bytes(n):
    digits = f"{n:X}"
    return "0x" + (f"{digits[:-4]}_{digits[-4:]}" if len(digits) > 4 else digits)


def convert(root, start, board_id, mcu, tag):
    """The `[board.<id>]` text of the board file `start` in the tree `root`."""
    root, start = Path(root), Path(start)
    raw, files, missing = read_tree(root, start)
    text = BLOCK_COMMENT.sub("", raw)
    nodes = overlays(text)
    product, yml = full_name(root, start)
    if yml:
        files.append(yml)

    pairs, buses = [], []
    for label in sorted(nodes):
        refs = re.findall(r"&(\w+)", last_prop(nodes[label], "pinctrl-0") or "")
        if not refs:
            continue
        if I2C.match(label):
            s = signals(text, refs)
            if "sda" in s and "scl" in s:
                pairs.append((label, s["sda"], s["scl"]))
        elif UART.match(label) and last_prop(nodes[label], "status") == '"okay"':
            s = signals(text, refs)
            baud = number((last_prop(nodes[label], "current-speed") or "").strip("<> "))
            if "tx" in s or "rx" in s:
                buses.append((label, s.get("tx"), s.get("rx"), baud))

    pins = named_pins(text, "gpio-leds") + named_pins(text, "gpio-keys")
    size = flash_bytes(text)
    rows = header(raw)
    table = f"board.{board_id}" if re.fullmatch(r"[A-Za-z0-9_-]+", board_id) else f"board.{quote(board_id)}"

    out = [
        f"# Imported from Zephyr {tag} (Apache-2.0) by scripts/import_zephyr_board.py.",
        "# Files read (sha256):",
    ]
    for rel in files:
        digest = hashlib.sha256((root / rel).read_bytes()).hexdigest()
        out.append(f"#   {rel} {digest}")
    if missing:
        out.append("# Includes not in the tree, not read: " + ", ".join(sorted(set(missing))))
    out += [
        "# Not in the source, to add by hand where the board needs them: default_link,",
        "# [boot], [clock], [flash], `dagon = true` on the bus of DAGON's own link.",
        f"[{table}]",
        "format = 4",
        f"product = {quote(product or board_id)}",
        f"mcu = {quote(mcu)}",
    ]
    if size is None:
        raise ImportError_("the files read give flash0 no size")
    out.append(f"flash_bytes = {hex_bytes(size)}")
    out.append('drawing = "generic"')
    if rows:
        out.append("header = [")
        for pin, label in rows:
            tail = f", label = {quote(label)}" if label else ""
            out.append(f"    {{ pin = {quote(pin)}{tail} }},")
        out.append("]")
    if pairs:
        out.append("i2c = [")
        for controller, sda, scl in pairs:
            out.append(
                f"    {{ controller = {quote(controller)}, sda = {quote(sda)}, scl = {quote(scl)} }},"
            )
        out.append("]")
    for name, pin in pins:
        out += ["", f"[[{table}.pin]]", f"pin = {quote(pin)}", f"name = {quote(name)}"]
    for label, tx, rx, baud in buses:
        out += ["", f"[[{table}.bus]]", f"id = {quote(label)}", 'kind = "uart"']
        out.append(f"controller = {quote(label)}")
        if tx:
            out.append(f"tx = {quote(tx)}")
        if rx:
            out.append(f"rx = {quote(rx)}")
        if baud:
            out.append(f"baud = {baud}")
    return "\n".join(out) + "\n"


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("zephyr", help="root of the Zephyr tree")
    p.add_argument("board", help="the board's .dts file")
    p.add_argument("--id", required=True, help="board id in the project")
    p.add_argument("--mcu", required=True, help="DAGON MCU id")
    p.add_argument("--tag", required=True, help="the Zephyr release tag of the tree")
    a = p.parse_args(argv)
    try:
        sys.stdout.write(convert(a.zephyr, a.board, a.id, a.mcu, a.tag))
    except ImportError_ as e:
        print(f"import_zephyr_board: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
