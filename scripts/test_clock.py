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
LIB = "crates/dagon-events/src/lib.rs"
CLOCK_NAMES = [
    (CLOCK, "pub trait Clock"),
    (LIB, "pub use compose::Clock"),
]

PUBLIC_ITEM = re.compile(r"\bpub\s+(?:struct|enum|union|trait|type)\s+Clock\b")
PUBLIC_USE = re.compile(r"\bpub\s+use\b([^;]*);")
RAW_STRING = re.compile(r'b?r(#*)"')
USE_TOKEN = re.compile(r"::|[{},*]|\w+")


def rust_texts():
    texts = {}
    for d in RUST_DIRS:
        for p in (ROOT / d).rglob("*.rs"):
            if "target" not in p.relative_to(ROOT).parts:
                texts[p.relative_to(ROOT).as_posix()] = p.read_text(encoding="utf-8")
    return texts


def code_only(text):
    """`text` with each comment (line, nested block) and each string, raw
    string, char and byte literal replaced by one space; lifetimes stay."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            i = text.find("\n", i)
            i = n if i < 0 else i
        elif text.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if text.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif text.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        elif (m := RAW_STRING.match(text, i)) and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            close = '"' + m.group(1)
            j = text.find(close, m.end())
            i = n if j < 0 else j + len(close)
        elif c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
            i += 1
        elif c == "'" and i + 1 < n and text[i + 1] == "\\":
            j = text.find("'", i + 3)
            i = n if j < 0 else j + 1
        elif c == "'" and i + 2 < n and text[i + 2] == "'":
            i += 3
        else:
            out.append(c)
            i += 1
            continue
        out.append(" ")
    return "".join(out)


def use_tree(tokens, i, prefix):
    """(path, public name) of each name a use tree brings in, from tokens[i];
    returns them and the index after the tree."""
    path = list(prefix)
    while True:
        t = tokens[i]
        if t == "::":
            i += 1
            continue
        if t == "{":
            found, i = [], i + 1
            while tokens[i] != "}":
                sub, i = use_tree(tokens, i, path)
                found += sub
                if tokens[i] == ",":
                    i += 1
            return found, i + 1
        if t == "*":
            return [((*path, "*"), "*")], i + 1
        path.append(t)
        i += 1
        if i < len(tokens) and tokens[i] == "::":
            continue
        if path[-1] == "self":
            path.pop()
        name = path[-1]
        if i < len(tokens) and tokens[i] == "as":
            name, i = tokens[i + 1], i + 2
        return [(tuple(path), name)], i


def re_exports(code):
    """(path, public name) of every `pub use` in `code`."""
    found = []
    for m in PUBLIC_USE.finditer(code):
        tokens = USE_TOKEN.findall(m.group(1))
        if tokens:
            found += use_tree(tokens, 0, [])[0]
    return found


def trait_files(codes, name):
    """Paths defining `trait <name>`, once per definition."""
    return [path for path, code in codes.items() for _ in re.finditer(rf"\btrait\s+{name}\b", code)]


def clock_names(codes):
    """(path, kind) of every public item, alias or re-export named `Clock`;
    a re-export's kind names its path, without a leading `crate`."""
    found = []
    for path, code in codes.items():
        found += [(path, " ".join(m.group(0).split())) for m in PUBLIC_ITEM.finditer(code)]
        for source, name in re_exports(code):
            if name == "Clock":
                source = source[1:] if source[0] == "crate" else source
                found.append((path, "pub use " + "::".join(source)))
    return sorted(found)


def problems(texts):
    codes = {path: code_only(text) for path, text in texts.items()}
    out = []
    if (t := trait_files(codes, "Clock")) != [CLOCK]:
        out.append(f"trait Clock in {t}, expected only {CLOCK}")
    if (t := trait_files(codes, "HostClock")) != [HOST_CLOCK]:
        out.append(f"trait HostClock in {t}, expected only {HOST_CLOCK}")
    if (n := clock_names(codes)) != sorted(CLOCK_NAMES):
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

    def test_an_alias_after_a_string_holding_slashes_fails(self):
        self.assertTrue(problems(self.planted("crates/dagon-host/src/lib.rs", '\nconst S: &str = "//"; pub type Clock = u64;\n')))

    def test_a_renamed_root_re_export_fails(self):
        lib = "crates/dagon-events/src/lib.rs"
        renamed = {**self.texts, lib: self.texts[lib].replace("Bounds, Clock,", "Bounds,") + "\npub use other::Clock as Ticks;\n"}
        self.assertNotEqual(renamed[lib], self.texts[lib])
        self.assertTrue(problems(renamed))

    def test_the_root_re_export_has_one_path(self):
        lib = "crates/dagon-events/src/lib.rs"
        without = self.texts[lib].replace("Bounds, Clock,", "Bounds,")
        self.assertNotEqual(without, self.texts[lib])
        self.assertEqual(problems({**self.texts, lib: without + "\npub use crate::compose::Clock;\n"}), [])
        self.assertTrue(problems({**self.texts, lib: without + "\npub use other::Clock;\n"}))

    def test_a_definition_in_a_comment_or_string_passes(self):
        for extra in (
            "\n/* pub trait Clock {} /* nested */ pub type Clock = u8; */\n",
            '\nconst DOC: &str = "pub trait Clock {} \\" pub type Clock = u8;";\n',
            '\nconst RAW: &str = r##"pub use x::Clock; "# trait Clock"##;\n',
            "\nconst Q: char = '\"'; const E: char = '\\''; const B: u8 = b'\"'; const T: &str = \"pub trait Clock\";\n",
            "\nfn f<'a>(x: &'a str) -> &'a str { x } // pub trait Clock\n",
        ):
            self.assertEqual(problems(self.planted("crates/dagon-host/src/lib.rs", extra)), [], extra)

    def test_a_private_import_passes(self):
        self.assertEqual(problems(self.planted("crates/dagon-agent/src/lib.rs", "\nuse dagon_events::Clock;\n")), [])


if __name__ == "__main__":
    unittest.main()
