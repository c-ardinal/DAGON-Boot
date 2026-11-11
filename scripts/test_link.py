"""The frame link contract has one home, and the guide shows real code.

- One `trait Link` (dagon-port's); link kinds as an enum only where
  `dagon_format::with_link_kinds!` generates them; no `Agent::run`;
  dagon-port does not depend on dagon-agent (dagon-agent depends on it).
- The existing-firmware guide's serving loop is the MCU definitions'
  serving loop, whole: one contiguous run of its lines from the link's
  kind to the end of the loop, comments aside; every `dagon_*` path the
  guide names exists.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUST_DIRS = ("crates", "targets", "samples")
PORT = ROOT / "crates/dagon-port"
SERVE = ROOT / "targets/mcu/common/src/serve.rs"
GUIDES = [
    ROOT / "site/src/content/docs/how-to/existing-firmware.md",
    ROOT / "site/src/content/docs/en/how-to/existing-firmware.md",
]
# Enums naming a few link kinds with data of their own, mapped to and from
# the generated kind type: (file, enum) -> why it is not a second list.
SUBSET_ENUMS = {
    ("crates/dagon-sim/src/timing.rs", "Medium"): "the simulator's uplinks with their timing; Medium::of maps a LinkKind",
}


def rust_files():
    for d in RUST_DIRS:
        for p in (ROOT / d).rglob("*.rs"):
            if "target" not in p.relative_to(ROOT).parts:
                yield p


def rel(p):
    return p.relative_to(ROOT).as_posix()


def kind_variants():
    text = (ROOT / "crates/dagon-format/src/link.rs").read_text(encoding="utf-8")
    return set(re.findall(r"^\s*(\w+) = \d+, \"[a-z0-9-]+\";", text, re.M))


ENUM = re.compile(r"\benum\s+(\w+)[^{;]*\{(.*?)^\s*\}", re.M | re.S)
VARIANT = re.compile(r"^\s*([A-Z]\w*)\s*([,(={]?)", re.M)


def kind_enums(text, kinds):
    """Field-less enums whose variants are all link kinds (or a kind with a
    suffix, as `I2cTarget`), two or more by their exact name. An enum with
    data per variant (an address, pins) or with other members (`Spi` of a
    bus kind) is a different list."""
    out = []
    for m in ENUM.finditer(text):
        body = re.sub(r"//[^\n]*|#\[[^\]]*\]", "", m.group(2))
        variants = VARIANT.findall(body)
        names = [v for v, _ in variants]
        unit = all(sep in ",=" for _, sep in variants)
        kindish = all(any(n.startswith(k) for k in kinds) for n in names)
        if unit and kindish and len(kinds & set(names)) >= 2:
            out.append(m.group(1))
    return out


def code_lines(text):
    """Non-blank, non-comment lines, stripped."""
    lines = (l.strip() for l in text.splitlines())
    return [l for l in lines if l and not l.startswith("//")]


def rust_blocks(md):
    return re.findall(r"^\s*```rust\n(.*?)^\s*```", md, re.M | re.S)


LOOP_FIRST = "let (via, addr) = (link.kind().code(), link.addr());"
LOOP_LAST = "return Exit::Reboot;"


def is_excerpt(snippet, source):
    """`snippet` equals a contiguous run of `source` lines (both already
    stripped of indentation, blank lines and comments)."""
    n = len(snippet)
    return n > 0 and any(source[i : i + n] == snippet for i in range(len(source) - n + 1))


def is_whole_loop(snippet, source):
    """The excerpt runs from the link's kind to the end of the loop, so no
    branch of it (a failed write, a failed flush) is left out."""
    if not snippet or snippet[0] != LOOP_FIRST or LOOP_LAST not in snippet:
        return False
    tail = snippet[snippet.index(LOOP_LAST) + 1 :]
    return tail == ["}", "}", "}"] and is_excerpt(snippet, source)


def trait_link_files(texts):
    """Paths (keys of `texts`) defining a `trait Link`, once per definition."""
    return [path for path, text in texts.items() for _ in re.finditer(r"\btrait\s+Link\b", text)]


def dependency_names(manifest):
    names = set()

    def walk(table):
        for key, value in table.items():
            if not isinstance(value, dict):
                continue
            if key.endswith("dependencies"):
                names.update(value)
            else:
                walk(value)

    walk(manifest)
    return names


class OneLinkContract(unittest.TestCase):
    def test_one_trait_link(self):
        found = trait_link_files({rel(p): p.read_text(encoding="utf-8") for p in rust_files()})
        self.assertEqual(found, ["crates/dagon-port/src/lib.rs"])

    def test_the_check_catches_a_second_trait_link(self):
        texts = {"a.rs": "pub trait Link {\n}\n", "b.rs": "trait  Link: Sized {}\n", "c.rs": "trait Linked {}\n"}
        self.assertEqual(trait_link_files(texts), ["a.rs", "b.rs"])

    def test_the_check_catches_a_dependency_on_dagon_agent(self):
        for manifest in (
            {"dependencies": {"dagon-agent": {"path": "../dagon-agent"}}},
            {"dev-dependencies": {"dagon-agent": "0.1"}},
            {"target": {"cfg(test)": {"dependencies": {"dagon-agent": "0.1"}}}},
        ):
            self.assertIn("dagon-agent", dependency_names(manifest), manifest)
        self.assertNotIn("dagon-agent", dependency_names({"dependencies": {"dagon-format": "0.1"}}))

    def test_link_kinds_are_generated_only(self):
        kinds = kind_variants()
        self.assertGreaterEqual(len(kinds), 7)
        found = []
        for p in rust_files():
            for name in kind_enums(p.read_text(encoding="utf-8"), kinds):
                if (rel(p), name) not in SUBSET_ENUMS:
                    found.append(f"{rel(p)}: enum {name}")
        self.assertEqual(found, [], "a hand-written list of link kinds; generate it with dagon_format::with_link_kinds!")

    def test_the_check_catches_a_hand_written_kind_enum(self):
        old = "pub enum LinkKind {\n    Uart,\n    I2cTarget,\n    UsbCdc,\n}\n"
        self.assertEqual(kind_enums(old, kind_variants()), ["LinkKind"])

    def test_no_agent_run(self):
        agent = "\n".join(p.read_text(encoding="utf-8") for p in (ROOT / "crates/dagon-agent/src").rglob("*.rs"))
        self.assertIsNone(re.search(r"\bfn\s+run\s*[<(]", agent))
        texts = [*rust_files(), *(ROOT / "site/src/content/docs").rglob("*.md*")]
        found = [rel(p) for p in texts if re.search(r"Agent::run\b|\bagent\.run\(", p.read_text(encoding="utf-8"))]
        self.assertEqual(found, [])

    def test_port_does_not_depend_on_agent(self):
        manifest = tomllib.loads((PORT / "Cargo.toml").read_text(encoding="utf-8"))
        self.assertNotIn("dagon-agent", dependency_names(manifest))
        self.assertIsNone(re.search(r"\bdagon_agent\b", (PORT / "src/lib.rs").read_text(encoding="utf-8")))


class ExistingFirmwareGuide(unittest.TestCase):
    def test_serving_loop_is_the_firmware_loop(self):
        source = code_lines(SERVE.read_text(encoding="utf-8"))
        snippets = []
        for g in GUIDES:
            blocks = rust_blocks(g.read_text(encoding="utf-8"))
            self.assertTrue(blocks, g)
            snippets.append([code_lines(b) for b in blocks])
            for b in blocks:
                lines = code_lines(b)
                self.assertTrue(is_whole_loop(lines, source), f"{rel(g)}: not the whole loop of {rel(SERVE)}, line for line:\n" + "\n".join(lines))
        self.assertEqual(snippets[0], snippets[1], "the Japanese and English guides show the same code")

    def test_handle_loop_parts_are_shown(self):
        code = "\n".join(rust_blocks(GUIDES[0].read_text(encoding="utf-8")))
        for part in ("read_frame", "agent.handle(", "write_frame", "flush()", "before_reboot()"):
            self.assertIn(part, code)

    def test_named_dagon_paths_exist(self):
        for g in GUIDES:
            for path in sorted(set(re.findall(r"\bdagon_\w+(?:::\w+)+", g.read_text(encoding="utf-8")))):
                crate, *_, item = path.split("::")
                src = ROOT / "crates" / crate.replace("_", "-") / "src"
                text = "\n".join(p.read_text(encoding="utf-8") for p in src.rglob("*.rs")) if src.is_dir() else ""
                defined = rf"\bpub (?:const |async )*(?:fn|struct|trait|enum|const|type|mod) {item}\b|\bpub use [^;]*\b{item}\b"
                self.assertRegex(text, defined, f"{rel(g)} names {path}")

    def test_the_excerpt_check_rejects_a_changed_loop(self):
        source = code_lines(SERVE.read_text(encoding="utf-8"))
        start = source.index(LOOP_FIRST)
        end = source.index(LOOP_LAST, start) + 4
        whole = source[start:end]
        self.assertTrue(is_whole_loop(whole, source))
        # The write result dropped, so a reply that failed could still reboot.
        wrote = whole.index("let wrote = match host.intercept(agent, request) {")
        dropped = whole[:wrote] + ["match host.intercept(agent, request) {"] + whole[wrote + 1 :]
        self.assertFalse(is_whole_loop(dropped, source))
        for i in range(1, len(whole) - 1):
            self.assertFalse(is_whole_loop(whole[:i] + whole[i + 1 :], source), f"line {i} left out: {whole[i]}")
        self.assertFalse(is_whole_loop(whole[:-1], source))
        self.assertFalse(is_whole_loop(DROPPED_WRITE_RESULT, source))


# An earlier version of the guide's loop: lines of the real loop in order,
# with the write result dropped. It must not pass.
DROPPED_WRITE_RESULT = [
    "let (via, addr) = (link.kind().code(), link.addr());",
    "loop {",
    "let read = link.read_frame().await;",
    "let request = match read {",
    "Ok(r) => r,",
    "Err(LinkError::Timeout) => {",
    "continue;",
    "}",
    "Err(LinkError::Closed) => {",
    "return Exit::Closed;",
    "}",
    "Err(e) => {",
    "host.on_error(e);",
    "continue;",
    "}",
    "};",
    "agent.request(request, via);",
    "let mut reboot = None;",
    "match agent.handle(request) {",
    "Some(reply) => {",
    "reboot = reply.reboot.then_some(reply.reboot_delay_ms);",
    "link.write_frame(reply.frame).await",
    "}",
    "None => link.no_reply().await,",
    "};",
    "if let Some(delay_ms) = reboot {",
    "if link.flush().await.is_ok() {",
    "host.wait_ms(delay_ms).await;",
    "agent.before_reboot();",
    "return Exit::Reboot;",
    "}",
    "}",
    "}",
]


if __name__ == "__main__":
    unittest.main()
