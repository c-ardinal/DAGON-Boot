"""The frame link contract has one home, and the guide shows real code.

- One `trait Link` (dagon-port's); link kinds as an enum only where
  `dagon_format::with_link_kinds!` generates them; no `Agent::run`;
  dagon-port does not depend on dagon-agent (dagon-agent depends on it).
- The existing-firmware guide's serving loop is lines of the MCU packages'
  serving loop, in order, and every `dagon_*` path it names exists.

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


def in_order(snippet, source):
    """Each snippet line is part of a source line, in source order."""
    it = iter(source)
    return all(any(line in s for s in it) for line in snippet)


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
        found = [rel(p) for p in rust_files() for _ in re.finditer(r"\btrait\s+Link\b", p.read_text(encoding="utf-8"))]
        self.assertEqual(found, ["crates/dagon-port/src/lib.rs"])

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
                self.assertTrue(in_order(lines, source), f"{rel(g)}: not lines of {rel(SERVE)}, in order:\n" + "\n".join(lines))
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

    def test_the_order_check_rejects_a_foreign_line(self):
        self.assertFalse(in_order(["agent.run(&mut link, &mut reset).await;"], ["loop {", "agent.handle(r)"]))
        self.assertTrue(in_order(["match agent.handle(r) {"], ["None => match agent.handle(r) {"]))


if __name__ == "__main__":
    unittest.main()
