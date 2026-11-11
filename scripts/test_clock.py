"""One clock trait per role.

- `trait Clock` is defined once, in dagon-events (the synchronous
  microsecond clock), and `trait HostClock` once, in dagon-romlink (the
  host's asynchronous millisecond clock).
- No other public type, alias or re-export is named `Clock`: the only ones
  are that definition and its re-export at the root of dagon-events.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUST_DIRS = ("crates", "targets", "samples")
CLOCK = "crates/dagon-events/src/compose.rs"
HOST_CLOCK = "crates/dagon-romlink/src/lib.rs"
CLOCK_NAMES = [
    (CLOCK, "pub trait Clock"),
    ("crates/dagon-events/src/lib.rs", "pub use"),
]

COMMENT = re.compile(r"//[^\n]*")
PUBLIC_ITEM = re.compile(r"\bpub\s+(?:struct|enum|union|trait|type)\s+Clock\b")
PUBLIC_USE = re.compile(r"\bpub\s+use\b[^;]*\bClock\b[^;]*;")


def rust_texts():
    texts = {}
    for d in RUST_DIRS:
        for p in (ROOT / d).rglob("*.rs"):
            if "target" not in p.relative_to(ROOT).parts:
                texts[p.relative_to(ROOT).as_posix()] = COMMENT.sub("", p.read_text(encoding="utf-8"))
    return texts


def trait_files(texts, name):
    """Paths defining `trait <name>`, once per definition."""
    return [path for path, text in texts.items() for _ in re.finditer(rf"\btrait\s+{name}\b", text)]


def clock_names(texts):
    """(path, kind) of every public item, alias or re-export named `Clock`."""
    found = []
    for path, text in texts.items():
        found += [(path, " ".join(m.group(0).split())) for m in PUBLIC_ITEM.finditer(text)]
        found += [(path, "pub use") for _ in PUBLIC_USE.finditer(text)]
    return sorted(found)


def problems(texts):
    out = []
    if (t := trait_files(texts, "Clock")) != [CLOCK]:
        out.append(f"trait Clock in {t}, expected only {CLOCK}")
    if (t := trait_files(texts, "HostClock")) != [HOST_CLOCK]:
        out.append(f"trait HostClock in {t}, expected only {HOST_CLOCK}")
    if (n := clock_names(texts)) != sorted(CLOCK_NAMES):
        out.append(f"public items named Clock: {n}, expected {sorted(CLOCK_NAMES)}")
    return out


class OneClockPerRole(unittest.TestCase):
    def setUp(self):
        self.texts = rust_texts()

    def planted(self, path, extra):
        return {**self.texts, path: self.texts.get(path, "") + extra}

    def test_the_tree_has_one_clock_per_role(self):
        self.assertEqual(problems(self.texts), [])

    def test_a_second_definition_fails(self):
        self.assertTrue(problems(self.planted("crates/dagon-port/src/lib.rs", "\npub trait Clock {\n    fn now_us(&self) -> u64;\n}\n")))
        self.assertTrue(problems(self.planted("crates/dagon-host/src/lib.rs", "\ntrait HostClock {}\n")))

    def test_a_missing_definition_fails(self):
        without_clock = {**self.texts, CLOCK: self.texts[CLOCK].replace("pub trait Clock", "pub trait Ticks")}
        self.assertTrue(problems(without_clock))
        without_host = {**self.texts, HOST_CLOCK: self.texts[HOST_CLOCK].replace("pub trait HostClock", "pub trait Ticks")}
        self.assertTrue(problems(without_host))

    def test_an_alias_fails(self):
        self.assertTrue(problems(self.planted("crates/dagon-host/src/lib.rs", "\npub use dagon_romlink::HostClock as Clock;\n")))
        self.assertTrue(problems(self.planted("crates/dagon-host/src/lib.rs", "\npub type Clock = u64;\n")))

    def test_a_second_re_export_fails(self):
        self.assertTrue(problems(self.planted("crates/dagon-agent/src/lib.rs", "\npub use dagon_events::{\n    Bounds,\n    Clock,\n};\n")))

    def test_a_private_import_passes(self):
        self.assertEqual(problems(self.planted("crates/dagon-agent/src/lib.rs", "\nuse dagon_events::Clock;\n")), [])


if __name__ == "__main__":
    unittest.main()
