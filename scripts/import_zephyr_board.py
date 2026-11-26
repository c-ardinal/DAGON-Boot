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
are followed when the file is in the tree, the nodes are merged as the
devicetree compiler merges them (a later statement of a node, by its path or
its label, adds to the earlier and replaces its properties), and the pin
names come from the names Zephyr gives its pin control nodes (`I2C0_SDA_P4`,
`i2c1_scl_pb8`).
A board whose pins are named another way is refused, not guessed.
"""
import argparse
import hashlib
import re
import sys
from pathlib import Path

# An include line; a comment may follow the file on it.
INCLUDE = re.compile(
    r'^[ \t]*#include[ \t]+(?:"([^"]+)"|<([^>]+)>)[ \t]*(?://[^\n]*|/\*[^\n]*?\*/[ \t]*)?$',
    re.M,
)
BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.S)
# A string or a block comment (kept as they are), or a line comment.
LINE_COMMENT = re.compile(r'"(?:\\.|[^"\\\n])*"|/\*.*?\*/|(//[^\n]*)', re.S)
# One row of a connector's `gpio-map`, with the printed name Zephyr keeps in
# the comment after it.
MAP_ROW = re.compile(
    r"<\s*\w+\s+\w+\s+&(\w+)\s+(\w+)\s+\w+\s*>\s*[,;]?[ \t]*(?:\x01(.*?)\x02)?"
)
# A printed name [`labelled`] kept.
NAME = re.compile(r"\x01[^\x02]*\x02")
# A preprocessor line left in the text: it states no node.
PREPROCESSOR = re.compile(
    r"^[ \t]*#[ \t]*(?:include|define|undef|if|ifdef|ifndef|elif|else|endif)\b.*$", re.M
)
GPIOS = re.compile(r"<\s*&(\w+)\s+([^\s>]+)")
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
        # An include inside a block comment states nothing: it stays as
        # written, and goes with its comment.
        commented = [
            m.span() for m in LINE_COMMENT.finditer(text) if m.group(0).startswith("/*")
        ]

        def put(m):
            if any(a <= m.start() < b for a, b in commented):
                return m.group(0)
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


def labelled(text):
    """`text` without its block comments, but for the one-line comment that
    follows a cell list on its line (`<...>, /* GP0 */`): a printed name,
    kept between `\\x01` and `\\x02`. Source inside a comment states nothing."""

    def keep(m):
        line = text[text.rfind("\n", 0, m.start()) + 1 : m.start()]
        name = m.group(0)[2:-2].strip()
        if "\n" not in name and re.search(r">\s*[,;]?[ \t]*$", line):
            return f"\x01{name}\x02"
        return ""

    return BLOCK_COMMENT.sub(keep, text)


def without_line_comments(text):
    """`text` without its `//` comments; strings and block comments stay."""
    return LINE_COMMENT.sub(lambda m: "" if m.group(1) else m.group(0), text)


class Node:
    """One devicetree node as the files leave it: every statement of it,
    wherever it is written, merged in source order."""

    def __init__(self):
        self.props = {}
        self.children = {}
        self.labels = []

    def prop(self, name):
        """The value of property `name` as written, or None."""
        value = self.props.get(name)
        return None if value is None else NAME.sub("", value).strip()

    def walk(self):
        yield self
        for child in self.children.values():
            yield from child.walk()


class Tree:
    """The devicetree of the files read. A node the files only refer to
    (`&uart0 { ... }` with its declaration in a file not read) is kept
    beside the root, under its label."""

    def __init__(self):
        self.root = Node()
        self.labels = {}
        self.loose = []

    def nodes(self):
        yield from self.root.walk()
        for node in self.loose:
            yield from node.walk()

    def at(self, path, make=False):
        """The node at `path` (`/leds/led_0`); one the files have not
        stated is None, or with `make` stated now."""
        node = self.root
        for name in [n for n in path.split("/") if n.strip()]:
            name = name.strip()
            if name not in node.children:
                if not make:
                    return None
                node.children[name] = Node()
            node = node.children[name]
        return node

    def delete(self, gone):
        """Takes node `gone` (None: nothing) and what is under it out of
        the tree, with their labels."""
        if gone is None:
            return
        for parent in list(self.nodes()):
            for name in [n for n, c in parent.children.items() if c is gone]:
                del parent.children[name]
        self.loose = [n for n in self.loose if n is not gone]
        for dropped in gone.walk():
            for label in dropped.labels:
                if self.labels.get(label) is dropped:
                    del self.labels[label]

    def referred(self, label):
        if label not in self.labels:
            node = Node()
            self.loose.append(node)
            self.labels[label] = node
        return self.labels[label]


def statement_end(text, i):
    """The index of the `;` or `{` that ends the statement at `i`, outside
    strings, cell lists and expressions; of the `}` that ends the node; or
    the end of `text`."""
    depth, quoted = 0, False
    while i < len(text):
        ch = text[i]
        if quoted:
            if ch == "\\":
                i += 1
            elif ch == '"':
                quoted = False
        elif ch == '"':
            quoted = True
        elif text.startswith("&{", i):
            # A reference by path, `&{/leds/led_0}`: its braces open no node.
            i = text.find("}", i)
            if i < 0:
                return len(text)
        elif ch in "<([":
            depth += 1
        elif ch in ">)]":
            depth -= 1
        elif depth <= 0 and ch in ";{}":
            return i
        i += 1
    return i


def fill(tree, node, text, i):
    """Reads the statements of `node` (None: the top of a file) from `i`;
    returns the index after its `}`."""
    while True:
        end = statement_end(text, i)
        head = text[i:end].strip()
        if end >= len(text):
            if node is not None:
                raise ImportError_("a `{` is never closed")
            return end
        if text[end] == "}":
            if node is None:
                raise ImportError_("a `}` closes nothing")
            return end + 1
        if text[end] == "{":
            *labels, name = [w for w in re.split(r"\s*:\s*|\s+", head) if w] or [""]
            if name == "/":
                target = tree.root
            elif name.startswith("&{"):
                target = tree.at(name[2:-1], make=True)
            elif name.startswith("&"):
                target = tree.referred(name[1:])
            elif node is None:
                raise ImportError_(f"node {name} is in no node")
            else:
                target = node.children.setdefault(name, Node())
            for label in labels:
                tree.labels[label] = target
                target.labels.append(label)
            i = fill(tree, target, text, end + 1)
            continue
        # A printed name after the `;` belongs to the row before it.
        after = re.match(r"[ \t]*\x01[^\x02]*\x02", text[end + 1 :])
        i = end + 1 + (after.end() if after else 0)
        if head.startswith("/delete-node/"):
            gone = head.split("/", 2)[2].strip()
            if gone.startswith("&{"):
                tree.delete(tree.at(gone[2:-1]))
            elif gone.startswith("&"):
                tree.delete(tree.labels.get(gone[1:]))
            elif node is not None:
                tree.delete(node.children.get(gone))
            continue
        if node is None or not head:
            continue
        if head.startswith("/delete-property/"):
            node.props.pop(head.split("/", 2)[2].strip(), None)
        elif not head.startswith("/"):
            name, _, value = head.partition("=")
            node.props[name.split(":")[-1].strip()] = value + (after.group(0) if after else "")


def parse(text):
    """The [`Tree`] of `text` (the text of [`labelled`])."""
    tree = Tree()
    fill(tree, None, PREPROCESSOR.sub("", text), 0)
    return tree


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


def signals(tree, refs):
    """`{signal: pin}` of the pin control nodes `refs` name."""
    out = {}
    for ref in refs:
        st = ST_MUX.match(ref)
        if st:
            out[st.group(1)] = pin_name(f"gpio{st.group(2)}", st.group(3))
            continue
        node = tree.labels.get(ref)
        if node is None:
            raise ImportError_(f"pin control node {ref} is not in the files read")
        muxes = RP_MUX.findall(" ".join(n.prop("pinmux") or "" for n in node.walk()))
        if not muxes:
            raise ImportError_(f"pin control node {ref} names no pin this importer reads")
        for signal, pin in muxes:
            out[signal.lower()] = pin_name("gpio0", pin)
    return out


def flash_bytes(tree):
    """The size of `flash0`, from its `reg`."""
    node = tree.labels.get("flash0")
    cells = re.fullmatch(r"<\s*(\S+)\s+(.+?)\s*>", (node.prop("reg") if node else None) or "")
    return (number(cells.group(2)) if cells else None) or None


def header(tree):
    """`[(pin, label)]` of every connector row, a pin once."""
    rows, seen = [], set()
    for node in tree.nodes():
        for port, num, label in MAP_ROW.findall(node.props.get("gpio-map", "")):
            pin = pin_name(port, num)
            if pin not in seen:
                seen.add(pin)
                rows.append((pin, label))
    return rows


def named_pins(tree, compatible):
    """`[(name, pin)]` of the children of every `compatible` node, each by
    its label (its node name without one)."""
    out = []
    for node in tree.nodes():
        if f'"{compatible}"' not in (node.prop("compatible") or ""):
            continue
        for name, child in node.children.items():
            g = GPIOS.match(child.prop("gpios") or "")
            if g:
                name = re.sub(r"\W", "_", (child.labels or [name])[0].lower())
                out.append((name, pin_name(g.group(1), g.group(2))))
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
    tree = parse(labelled(without_line_comments(raw)))
    product, yml = full_name(root, start)
    if yml:
        files.append(yml)

    pairs, buses = [], []
    for label in sorted(tree.labels):
        node = tree.labels[label]
        refs = re.findall(r"&(\w+)", node.prop("pinctrl-0") or "")
        if not refs:
            continue
        if I2C.match(label):
            s = signals(tree, refs)
            if "sda" in s and "scl" in s:
                pairs.append((label, s["sda"], s["scl"]))
        elif UART.match(label) and node.prop("status") == '"okay"':
            s = signals(tree, refs)
            baud = number((node.prop("current-speed") or "").strip("<> "))
            if "tx" in s or "rx" in s:
                buses.append((label, s.get("tx"), s.get("rx"), baud))

    pins = named_pins(tree, "gpio-leds") + named_pins(tree, "gpio-keys")
    size = flash_bytes(tree)
    rows = header(tree)
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
