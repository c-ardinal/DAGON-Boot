"""No fixed count of image slots outside the wire codecs.

An image kind has the slots its MCU definition gives, one or two. The code
that chooses among them (the boot decision and stage0's choice, the
agent's image state, the host's slot choice, the simulator's stores, the
conformance cases, the CLI's and the Console's slot actions and views)
iterates the slots it is given. This guard fails on a new fixed slot count
there: an array of two slots, "the other slot" as `1 - x`, or a literal
list of the A and B slots. The wire codecs (dagon-proto, dagon-format)
keep their fixed room for A and B (`StatusResp::IMAGE_SLOTS`) and are not
read, nor are the Console's labels of the STATUS entries. Tests are not
read either: their fixtures are two-slot or one-slot nodes by design. The
Kani proofs (`proofs.rs`, built only under `cfg(kani)`) are read as tests.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import unittest

from rust_source import ROOT, code_only

RUST_CRATES = (
    "dagon-core",
    "dagon-agent",
    "dagon-host",
    "dagon-sim",
    "dagon-conform",
    "dagon-cli",
    "dagon-wasm",
)
WEB = ROOT / "web" / "src"
# The Console's labels of the STATUS entries (A, B, 予備): the wire's names.
LABELS = "web/src/app/copy.ts"

# "The other slot" as one minus the running one, on a line about slots.
ONE_MINUS = re.compile(r"\b1\s*-\s*\(?[A-Za-z_][\w.]*\b")
# A literal list of the A and B slots.
AB_LIST = re.compile(
    r"\[\s*(?:slot_binding|slot|Binding|SlotName)::A\s*,\s*(?:slot_binding|slot|Binding|SlotName)::B\s*\]"
    r"|\[\s*0u8\s*,\s*1\s*\]"
    r"|\[\s*'a'\s*,\s*'b'\s*\]"
    r"|\[\s*'A'\s*,\s*'B'\s*\]"
    r'|\[\s*"[Aa]"\s*,\s*"[Bb]"'
    r"|\.\.2\s*\]"
    r"|slice\(\s*0\s*,\s*2\s*\)"
)
# A slot count fixed in a constant, and what is sized or counted by one.
FIXED_COUNT = re.compile(
    r"const\s+\w*SLOTS\w*\s*:\s*(?:usize|u8)\s*=\s*2\b"
    r"|\[[^\[\];]*;\s*\w*SLOTS\w*\s*\]"
    r"|\b0\s*\.\.=?\s*\w*SLOTS\w*\b"
)
# The two slot numbers as a literal list, on a line about slots.
ZERO_ONE = re.compile(r"\[\s*0\s*,\s*1\s*\]")
# An array of two slots: of slot-like elements, or a `slots` field of two.
TWO_SLOTS = re.compile(
    r"\[[^\[\];]*(?:Slot|State|Machine|Img|\(u32,\s*u32\))[^\[\];]*;\s*2\s*\]"
    r"|slots:\s*\[[^\]]*;\s*2\s*\]"
)
# An inline test module: `#[cfg(test)]`, attributes or comments, `mod x {`.
TEST_MODULE = re.compile(r"#\[cfg\(test\)\]\s*(?:(?://[^\n]*|#\[[^\]]*\])\s*)*mod\s+\w+\s*\{")


def rust_sources():
    """Path -> text of each non-test Rust source of RUST_CRATES (a file's
    inline test module left out)."""
    out = {}
    for c in RUST_CRATES:
        src = ROOT / "crates" / c / "src"
        for p in src.rglob("*.rs"):
            rel = p.relative_to(src)
            if p.name in ("tests.rs", "proofs.rs") or p.name.startswith("tests_") or "tests" in rel.parts:
                continue
            text = p.read_text(encoding="utf-8")
            cut = TEST_MODULE.search(text)
            out[p.relative_to(ROOT).as_posix()] = text[: cut.start()] if cut else text
    return out


def web_sources():
    """Path -> text of each Console source (tests, the generated package
    and the labels left out)."""
    return {
        p.relative_to(ROOT).as_posix(): p.read_text(encoding="utf-8")
        for p in WEB.rglob("*")
        if p.suffix in (".ts", ".svelte")
        and ".test." not in p.name
        and "pkg" not in p.relative_to(WEB).parts
        and p.relative_to(ROOT).as_posix() != LABELS
    }


def findings(path, raw, code):
    """(path:line, text) of each fixed slot count in one file: `raw` for the
    literal lists (their quotes matter), `code` (comments and literals
    blanked) for the rest."""
    found = []
    for n, (line, plain) in enumerate(zip(raw.splitlines(), code.splitlines()), 1):
        about_slots = "slot" in line.lower()
        if (
            AB_LIST.search(line)
            or (about_slots and ONE_MINUS.search(plain))
            or (about_slots and ZERO_ONE.search(plain))
            or TWO_SLOTS.search(plain)
            or FIXED_COUNT.search(plain)
        ):
            found.append((f"{path}:{n}", line.strip()))
    return found


def all_findings():
    found = []
    for path, text in rust_sources().items():
        found += findings(path, text, code_only(text))
    for path, text in web_sources().items():
        found += findings(path, text, text)
    return found


class SlotCount(unittest.TestCase):
    def test_no_fixed_slot_count_outside_the_wire_codecs(self):
        self.assertEqual(all_findings(), [])

    def test_the_guard_sees_each_shape(self):
        for line in [
            "let other = 1 - slot;",
            "let x = st.slots[(1 - st.active) as usize];",
            "for s in [slot_binding::A, slot_binding::B] {",
            "let heads = [0u8, 1].map(|s| head(s));",
            "for slot in ['a', 'b'] {",
            "slots: [image::State; 2],",
            "app: [image::Machine; 2],",
            "pub slots: [(u32, u32); 2],",
            "for s in &st.slots[..2] {",
            "return st.slots.slice(0, 2).map((s, i) => s);",
            "const other = s.slots[1 - s.active];",
            "const SLOTS: usize = 2;",
            "let mut tried_slot = [false; SLOTS];",
            "for s in 0..SLOTS as u8 {",
            "for s in [slot::A, slot::B] {",
            "for b in [Binding::A, Binding::B] {",
            "for slot in [0, 1] {",
        ]:
            self.assertTrue(findings("x", line, line), line)
        for line in [
            "let mut p = [0u8; 2];",
            "pub fn tried(cell: [u32; 2], p: &SlotRef) -> u8 {",
            "let u = 1 - t;",
            "for i in [0, 1] {",
            "const ROOM: usize = StatusResp::IMAGE_SLOTS;",
            "for s in 0..self.app_slots {",
        ]:
            self.assertFalse(findings("x", line, line), line)


if __name__ == "__main__":
    unittest.main()
