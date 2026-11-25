"""A relay link id has one source: the MCU definition's `[[relay_link]]`,
read by the definitions loader and generated into firmware constants.

No durable document (Markdown under the repository, build output and
dependencies left out) and no Rust comment restates a port's link id
(`link 1 = I2C0`, `2 = PIO UART`, `i2c0 is link 1`); the definition is the
one copy. Raw golden wire bytes are not a restatement.

Production Rust under crates/ and targets/ (tests and `#[cfg(test)]`
modules left out):

- names no global link id constant (`link::I2C0`, `link::PIO_UART`, or an
  `I2C0`/`PIO_UART` link constant of its own);
- gives no link id as a literal where a link id goes: a `link_id()` that
  returns a number, a `link: N` or `id: N` field, a `named(index, name, N,
  addr)` node, a `Recovery`'s link;
- maps no interface or link kind to a link id in a match
  (`iface::… => N`, `LinkKind::… => N`);
- holds no hand-written relay link list: no `RelayLinkDef { … }` literal
  and no type named `RelayLink` (the generated `RELAY_LINKS` is in build
  output, not in the tree).

Tests and fixtures may keep their own `RelayLinkDef { … }` tables with
their own ids, and take a production id from a definition or a generated
constant; they fail on a link id written as a literal at a RELAY use site
(`Relay { link: N, … }`, `named(index, name, N, addr)`, a `(N, addr)` RELAY
target, a `Linked { link: N, … }` host target) and on a constant that
copies a production id (`const I2C0: u8 = 1;`, `const RELAY_LINK_…: u8 =
N;`). Raw expected golden wire bytes (byte arrays) are not a use site.

The Console (TypeScript and Svelte under web/src, the generated wasm
package left out) writes no link id as a literal (`link: N`, `link === N`,
`const LINK_… = N`), tests and its stand-in hub included, and, outside its
one entry to dagon-wasm (`src/wasm/dagon.ts`, which names the connected
node's `local` target), builds no target of its own (`kind: 'linked'`, a
`target: { kind: … }` literal): every node's target is the one dagon-host
gives (`setTimings`, `topo_targets`).

    python -m unittest discover -s scripts -p "test_link_ids.py"
"""
import functools
import re
import unittest

from rust_source import ROOT, rust_texts
from test_relay import code_of, is_test_path, production

DIRS = ("crates/", "targets/")
# The definitions loader: the one place that turns `[[relay_link]]` into
# `RelayLinkDef` values.
DEFINITIONS = "crates/dagon-topology/src/defs/"

# The link id constants dagon-proto held. (`iface::PIO_UART` is a topology
# interface code, not a link id.)
GLOBAL_CONST = re.compile(r"\blink\s*::\s*(?:I2C0|PIO_UART)\b|\bconst\s+I2C0\s*:")
# A link id as a literal; 0 is "no link" (a node's own entry), never an id.
ID = r"(?!0\b)\d"
PROD_LITERAL = (
    (re.compile(rf"\bfn\s+link_id\s*\([^)]*\)\s*->\s*u8\s*\{{\s*{ID}"), "returns a literal link id"),
    (re.compile(rf"(?<![\w.])link\s*:\s*{ID}"), "writes a link id as a literal (`link: N`)"),
    (re.compile(rf"\bRelayLinkDef\s*\{{[^}}]*\bid\s*:\s*{ID}"), "writes a relay link id as a literal"),
    (re.compile(rf"\bnamed\s*\([^,()]+,[^,()]+,\s*{ID}"), "names a node's link id as a literal"),
)
KIND_MATCH = re.compile(r"\b(?:iface|LinkKind)\s*::\s*\w+\s*(?:\|[^=]*)?=>\s*(?:Some\s*\(\s*)?\(?\s*(?:\d|RELAY_LINK_\w+|link\s*::)")
SECOND_LIST = (
    # A value, not the type's own `struct`/`impl` block.
    (re.compile(r"(?<!struct )(?<!impl )(?<!for )\bRelayLinkDef\s*\{"), "holds a hand-written relay link list (`RelayLinkDef { .. }`)"),
    (re.compile(r"\b(?:struct|enum|type)\s+RelayLink\b"), "declares a second relay link type (`RelayLink`)"),
)
TEST_LITERAL = (
    (re.compile(rf"\bRelay\s*\{{[^}}]*?(?<![\w.])link\s*:\s*{ID}"), "a RELAY with a literal link id"),
    (re.compile(rf"\bnamed\s*\([^,()]+,[^,()]+,\s*{ID}"), "a linked node with a literal link id"),
    (re.compile(rf"\bSome\s*\(\s*\(\s*{ID}\d*\s*,\s*[\w:]*ADDR\w*\s*\)\s*\)"), "a RELAY target with a literal link id"),
    (re.compile(rf"\bLinked\s*\{{[^}}]*?(?<![\w.])link\s*:\s*{ID}"), "a host target with a literal link id"),
    (re.compile(r"\bconst\s+(?:I2C0|PIO_UART|RELAY_LINK_\w+|\w*LINK_ID\w*)\s*:\s*u8\s*=\s*\d"), "a copied production link id"),
)


# A port's link id said in words: "link 1 = I2C0", "link id 2: pio-uart",
# "2 = PIO UART", "i2c0 is link 1", "I2C0 = 1".
PORT = r"(?:i2c\d|i2c|pio[ _-]?uart|uart\d|usart\d|usb)"
RESTATED = re.compile(
    rf"\blink(?:\s+id)?\s*{ID}\d*\s*(?:=|:|is\b)\s*\S"
    rf"|(?<![\w/]){ID}\d*\s*=\s*{PORT}\b"
    rf"|\b{PORT}\s*(?:=|is)\s*(?:link\s*(?:id\s*)?)?{ID}\d*\b",
    re.IGNORECASE,
)
DOC_SKIP = ("target", "node_modules", "dist", ".git")


def doc_texts():
    """Path -> text of every Markdown file of the repository, build output
    and dependencies left out."""
    texts = {}
    for p in ROOT.rglob("*.md"):
        rel = p.relative_to(ROOT)
        if not any(part in DOC_SKIP for part in rel.parts):
            texts[rel.as_posix()] = p.read_text(encoding="utf-8")
    return texts


def comment_text(text):
    """The `//` comments of Rust `text` (where a document's words live in code)."""
    return "\n".join(m.group(1) for m in re.finditer(r"//+!?(.*)", text))


def restated(texts, of=lambda t: t):
    """`path: restates a port's link id: ...` for each text that does."""
    return [f"{path}: restates a port's link id: {m.group(0)}"
            for path, text in sorted(texts.items()) if (m := RESTATED.search(of(text)))]


def is_test_file(path):
    """A test path, or a `tests_*.rs` module (declared under `#[cfg(test)]`)."""
    return is_test_path(path) or path.rsplit("/", 1)[-1].startswith("tests_")


@functools.lru_cache(maxsize=None)
def file_problems(path, text):
    out = []
    if not path.startswith(DIRS):
        return out
    code = code_of(text)
    if m := GLOBAL_CONST.search(code):
        out.append(f"{path} names a global link id ({' '.join(m.group(0).split())})")
    if is_test_file(path):
        tests = code
    else:
        prod = production(code)
        tests = "".join(c if p != c else " " for p, c in zip(prod, code))
        for rule, why in PROD_LITERAL + SECOND_LIST:
            if path.startswith(DEFINITIONS) and (rule, why) in SECOND_LIST:
                continue
            if m := rule.search(prod):
                out.append(f"{path} {why}: {' '.join(m.group(0).split())}")
        if m := KIND_MATCH.search(prod):
            out.append(f"{path} maps an interface to a link id: {' '.join(m.group(0).split())}")
    for rule, why in TEST_LITERAL:
        if m := rule.search(tests):
            out.append(f"{path}: {why}: {' '.join(m.group(0).split())}")
    return out


def problems(texts):
    return [p for path, text in sorted(texts.items()) for p in file_problems(path, text)]


WEB = "web/src/"
WEB_ENTRY = "web/src/wasm/dagon.ts"
WEB_LITERAL = re.compile(rf"(?<![\w.])link\s*(?:===?|:)\s*{ID}|\bLINK_\w+\s*=\s*\d")
WEB_TARGET = re.compile(r"\bkind\s*:\s*'linked'|\btarget\s*:\s*[^,;\n]*?\{\s*kind\s*:")
JS_COMMENT = re.compile(r"/\*.*?\*/|(?<![:'\"])//[^\n]*", re.DOTALL)


def web_texts():
    """Path -> text of the Console's TypeScript and Svelte files, the
    generated wasm package and dependencies left out."""
    texts = {}
    for pattern in ("*.ts", "*.svelte"):
        for p in (ROOT / WEB).rglob(pattern):
            rel = p.relative_to(ROOT).as_posix()
            if "/pkg/" not in rel and "node_modules" not in rel:
                texts[rel] = p.read_text(encoding="utf-8")
    return texts


def web_problems(texts):
    """Console files that write a link id or build a target of their own."""
    out = []
    for path, text in sorted(texts.items()):
        code = JS_COMMENT.sub(" ", text)
        if m := WEB_LITERAL.search(code):
            out.append(f"{path} writes a link id as a literal: {m.group(0)}")
        production = not path.endswith(".test.ts")
        if production and path != WEB_ENTRY and (m := WEB_TARGET.search(code)):
            out.append(f"{path} builds a target instead of passing dagon-host's: {m.group(0)}")
    return out


def doc_problems(docs, rust):
    """Durable documents and Rust comments that restate a port's link id."""
    return restated(docs) + restated(rust, comment_text)


class LinkIds(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.texts = rust_texts()

    def planted(self, path, extra):
        """What planting `extra` in `path` adds to what the tree already holds."""
        before = set(problems(self.texts))
        after = problems({**self.texts, path: self.texts.get(path, "") + "\n" + extra})
        return [p for p in after if p not in before]

    def test_the_tree_holds(self):
        self.assertEqual(problems(self.texts), [])

    def test_no_document_or_comment_restates_a_port_link_id(self):
        self.assertEqual(doc_problems(doc_texts(), self.texts), [])

    def test_a_restated_link_id_in_a_document_or_comment_fails(self):
        for text in (
            "- Frames on USB: RELAY (link 1 = I2C0, 2 = PIO UART) is forwarded\n",
            "RELAY link id 2: pio-uart\n",
            "i2c0 is link 1 on the RP2040\n",
            "Link 3 = USART2.\n",
        ):
            with self.subTest(text=text):
                self.assertTrue(doc_problems({"targets/x/README.md": text}, {}))
                self.assertTrue(doc_problems({}, {"crates/x/src/lib.rs": f"// {text}"}))

    def test_words_about_links_and_golden_bytes_pass(self):
        for text in (
            "The relay link id (1 to 254) comes from `[[relay_link]]`.\n",
            "seg on I2C0 0x42 and watch on the PIO UART\n",
            "rear MUX_IO_1/2 = UART0 TX G43 / RX G44\n",
            "len 4 + 18; the link id of the RP2040's i2c0 entry\n",
        ):
            with self.subTest(text=text):
                self.assertEqual(doc_problems({"targets/x/README.md": text}, {}), [])
        code = "const BYTES: [u8; 7] = [0x50, 22, 0, 1, 0x42, 0x2C, 0x01];\n"
        self.assertEqual(doc_problems({}, {"crates/dagon-proto/tests/golden_frames.rs": code}), [])

    def test_a_global_link_constant_fails_anywhere(self):
        for path, code in (
            ("targets/mcu/rp2040/src/planted.rs", "fn f() -> u8 { link::I2C0 }\n"),
            ("crates/dagon-sim/src/planted.rs", "const X: u8 = payload::link::PIO_UART;\n"),
            ("crates/dagon-sim/tests/planted.rs", "const X: u8 = link :: I2C0;\n"),
            ("crates/dagon-proto/src/planted.rs", "pub const I2C0: u8 = 1;\n"),
        ):
            with self.subTest(path=path):
                self.assertTrue(self.planted(path, code))

    def test_a_literal_link_id_in_production_fails(self):
        for code in (
            "impl X { fn link_id(&self) -> u8 { 1 } }\n",
            "fn f() -> Recovery { Recovery { lines: l, link: 1 } }\n",
            "static S: [LinkedNode; 1] = [named(1, \"seg\", 1, 0x42)];\n",
        ):
            with self.subTest(code=code):
                self.assertTrue(self.planted("targets/mcu/rp2040/src/relay.rs", code))

    def test_a_kind_to_id_match_in_production_fails(self):
        for code in (
            "fn id(i: u8) -> u8 { match i { iface::I2C_BUS => 1, _ => 2 } }\n",
            "fn id(k: LinkKind) -> u8 { match k { LinkKind::Uart => RELAY_LINK_PIO_UART, _ => 0 } }\n",
            "fn id(k: LinkKind) -> Option<(u8, u8)> { match k { LinkKind::I2c => Some((1, 0)), _ => None } }\n",
        ):
            with self.subTest(code=code):
                self.assertTrue(self.planted("crates/dagon-host/src/lib.rs", code))

    def test_a_second_hand_written_list_in_production_fails(self):
        for code in (
            "pub const LINKS: [RelayLinkDef; 1] = [RelayLinkDef { id: ID, port: 1, kind: K, addressing: A, max_payload: 1 }];\n",
            "pub struct RelayLink { pub id: u8 }\n",
        ):
            with self.subTest(code=code):
                self.assertTrue(self.planted("targets/mcu/rp2040/src/lib.rs", code))

    def test_the_type_itself_and_the_definitions_loader_pass(self):
        for path, code in (
            ("crates/dagon-port/src/planted.rs",
             "pub struct RelayLinkDef {\n    pub id: u8,\n}\nimpl RelayLinkDef {\n}\nimpl Clone for RelayLinkDef {\n}\n"),
            ("crates/dagon-topology/src/defs/planted.rs",
             "fn f(r: &R) -> RelayLinkDef { RelayLinkDef { id: r.id, port: 0, kind: K, addressing: A, max_payload: 1 } }\n"),
        ):
            with self.subTest(path=path):
                self.assertEqual(self.planted(path, code), [])

    def test_own_tables_and_ids_from_a_definition_pass_in_tests(self):
        for path, code in (
            ("crates/dagon-relay/tests/relay.rs",
             "const BUS: RelayLinkDef = RelayLinkDef { id: 7, port: 0, kind: LinkKind::I2c, addressing: Addressing::SevenBit, max_payload: 32 };\n"
             "fn r() -> Relay<'static> { Relay { link: BUS.id, addr: 0x40, timeout_ms: 5, inner: &[] } }\n"),
            ("crates/dagon-proto/tests/golden_frames.rs", "const BYTES: [u8; 4] = [0x50, 22, 0, 1];\n"),
            ("targets/mcu/rp2040/tests/sim_parity.rs", "fn n() -> LinkedNode { named(1, \"seg\", RELAY_LINK_I2C0, 0x42) }\n"),
            ("crates/dagon-relay/src/nodes.rs",
             "#[cfg(test)]\nmod t {\n    const B: RelayLinkDef = RelayLinkDef { id: 9, port: 2, kind: K, addressing: A, max_payload: 1 };\n}\n"),
        ):
            with self.subTest(path=path):
                self.assertEqual(self.planted(path, code), [])

    def test_a_literal_or_copied_id_at_a_relay_site_in_tests_fails(self):
        for path, code in (
            ("crates/dagon-sim/tests/hub.rs", "fn r() -> Relay<'static> { Relay { link: 1, addr: 0x42, timeout_ms: 5, inner: &[] } }\n"),
            ("crates/dagon-sim/tests/sample.rs", "const SEG: Option<(u8, u8)> = Some((1, NODE_I2C_ADDR));\n"),
            ("targets/boards/waveshare-rp2040-touch-lcd-1.28/src/screen/tests.rs", "fn n() -> LinkedNode { named(1, \"seg\", 1, 0x42) }\n"),
            ("crates/dagon-proto/tests/golden_frames.rs", "const I2C0: u8 = 1;\n"),
            ("crates/dagon-relay/src/nodes.rs", "#[cfg(test)]\nmod t {\n    const RELAY_LINK_I2C0: u8 = 1;\n}\n"),
        ):
            with self.subTest(path=path):
                self.assertTrue(self.planted(path, code))

    def test_the_console_holds(self):
        self.assertEqual(web_problems(web_texts()), [])

    def test_a_link_id_or_own_target_in_the_console_fails(self):
        for path, code in (
            ("web/src/serial/mock.ts", "const LINK_I2C0 = 1;\n"),
            ("web/src/serial/mock.ts", "const key = link === 2 ? 'uart' : null;\n"),
            ("web/src/app/operate.test.ts", "const t = { kind: 'linked', link: 1, addr: 0x42, maxPayload: 32 };\n"),
            ("web/src/app/define/model.ts", "const n = { target: n.role === 'coordinator' ? { kind: 'hub', addr: 0 } : x };\n"),
            ("web/src/app/operate.svelte.ts", "const t: Target = { kind: 'linked', link: l, addr: a, maxPayload: m };\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertTrue(web_problems({path: code}))

    def test_the_entry_comments_and_targets_from_the_host_pass(self):
        for path, code in (
            (WEB_ENTRY, "export const LOCAL: Target = { kind: 'local' };\n"),
            ("web/src/app/operate.svelte.ts", "// link: 1 was the I2C link\nthis.targetsBy = h.setTimings(yaml);\n"),
            ("web/src/app/operate.test.ts", "expect(t.kind === 'linked' && t.link === to.link).toBe(true);\n"),
        ):
            with self.subTest(path=path):
                self.assertEqual(web_problems({path: code}), [])

    def test_a_literal_link_id_in_a_host_target_in_tests_fails(self):
        code = "const SEG: Target = Target::Linked { link: 1, addr: 0x42, max_payload: 255 };\n"
        self.assertTrue(self.planted("crates/dagon-host/tests/sim.rs", code))
        own = "fn t(d: RelayLinkDef) -> Target { Target::Linked { link: d.id, addr: 0x42, max_payload: 1 } }\n"
        self.assertEqual(self.planted("crates/dagon-host/tests/sim.rs", own), [])

    def test_comments_strings_and_other_folders_pass(self):
        for path, code in (
            ("targets/mcu/rp2040/src/links.rs", "// link::I2C0 was the I2C link\nconst S: &str = \"link: 1\";\n"),
            ("site/src/x.rs", "const X: u8 = link::I2C0;\n"),
        ):
            with self.subTest(path=path):
                self.assertEqual(self.planted(path, code), [])


if __name__ == "__main__":
    unittest.main()
