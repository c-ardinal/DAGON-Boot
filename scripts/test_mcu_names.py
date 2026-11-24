"""MCU and board knowledge comes from the definitions, not from code that
names a chip or a board.

Every MCU id (`targets/mcu/*/dagon-mcu.toml`) and board id
(`targets/boards/*/dagon-board.toml`) is looked for, as a whole word in
any case, in the MCU-independent code: non-test Rust under crates/ and
the Console's sources under web/src. Comments are left out; string
literals and identifiers count (`"rp2040"`, `RP2040_USB_HZ`). The ids come
from the definition folders found, the way the built-in definitions are
found (dagon-topology's build.rs reads every package folder), so a new MCU
or board is covered with no change here.

A file may name them only as ALLOWED lists it, at most that many times,
with the reason; a file that names them more, a file not listed, and an
entry whose file names none any more fail. targets/ and samples/ are left
out: an MCU or board package and a sample are that MCU's or board's own
code. Test code (integration tests, `tests.rs`, `tests/` modules, files a
`#[cfg(test)] mod name;` declares, `#[cfg(test)]` modules, `*.test.ts`) is
left out too.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import tempfile
import tomllib
import unittest
from pathlib import Path

import test_relay as relay
from rust_source import ROOT, blanked, rust_texts
from test_builtin_registry import declared_test_files

# File -> (most names it may hold, why naming a chip or board is right there).
ALLOWED = {
    "crates/dagon-format/src/header.rs": (
        2,
        "mcu_id wire constants of the built-in images' header (to be replaced by a hash of the MCU's name)",
    ),
    "crates/dagon-topology/src/defs/clock.rs": (
        13,
        "PLL_RULES rows are PLL designs named after the chip they come from; a definition picks a row by its pll fields, never by its MCU id",
    ),
    "crates/dagon-cli/src/esp_image.rs": (
        1,
        "the chip name of the ESP-IDF image format esptool writes (its own list of chips, not DAGON's)",
    ),
    "crates/dagon-cli/src/esp_image_qemu.rs": (
        5,
        "the vendor QEMU's machine name and the ESP32-S3 boot stage the QEMU check runs (a test bench of that one chip)",
    ),
    "crates/dagon-cli/src/esp_image_qemu_bootcore.rs": (
        3,
        "the vendor QEMU's machine name and the ESP32-S3 boot core the QEMU check runs (a test bench of that one chip)",
    ),
    "crates/dagon-cli/src/simrun.rs": (
        3,
        "the built-in sample world of `dagon sim` (its images and ROM maps)",
    ),
    "crates/dagon-sim/src/flash.rs": (
        4,
        "the simulator's fixed sample flashes",
    ),
    "crates/dagon-sim/src/lib.rs": (
        8,
        "the simulator's fixed sample world",
    ),
    "crates/dagon-sim/src/rom.rs": (
        1,
        "the simulator's fixed sample ROM loader (its default chip's erase times)",
    ),
    "crates/dagon-sim/src/store.rs": (
        3,
        "the simulator's fixed sample layouts",
    ),
    "crates/dagon-sim/src/world.rs": (
        2,
        "the one vendor-stage2 layout and flash the simulator models, picked by the id its fixed layout carries",
    ),
    "web/src/app/sample.ts": (
        8,
        "the Console demo's fixed sample nodes (the demo has no project and no definitions of its own)",
    ),
}


def definition_ids(root=ROOT):
    """The ids of every MCU and board definition folder under `root`."""
    found = [
        tomllib.loads(p.read_text(encoding="utf-8"))["id"]
        for pattern in ("targets/mcu/*/dagon-mcu.toml", "targets/boards/*/dagon-board.toml")
        for p in root.glob(pattern)
    ]
    return sorted(found)


def name_pattern(ids):
    alternatives = "|".join(re.escape(i) for i in sorted(ids, key=len, reverse=True))
    return re.compile(rf"(?<![A-Za-z0-9])(?:{alternatives})(?![A-Za-z0-9])", re.I)


def web_texts():
    """Path -> text of the Console's non-test sources."""
    out = {}
    for p in (ROOT / "web/src").rglob("*"):
        if p.suffix not in (".ts", ".svelte") or p.name.endswith(".test.ts"):
            continue
        out[p.relative_to(ROOT).as_posix()] = p.read_text(encoding="utf-8")
    return out


def web_comments_blanked(text):
    """A Console source (`.ts`, `.svelte`) with each comment blanked: `//`
    and `/* */` outside its '…', "…" and `…` strings, and `<!-- -->` of a
    Svelte file's markup; spaces in their place, line breaks kept, so it is
    as long as `text`."""
    out, i, n = list(text), 0, len(text)

    def blank(start, end):
        for k in range(start, min(end, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = text[i]
        if text.startswith("<!--", i):
            j = text.find("-->", i + 4)
            j = n if j < 0 else j + 3
            blank(i, j)
        elif text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            blank(i, j)
        elif c in "'\"`":
            j = i + 1
            while j < n and text[j] != c and (c == "`" or text[j] != "\n"):
                j += 2 if text[j] == "\\" else 1
            j += 1
        else:
            j = i + 1
        i = j
    return "".join(out)


def production_texts():
    """Path -> text of every file in scope (crates/ Rust, web/src)."""
    rust = {path: text for path, text in rust_texts().items() if path.startswith("crates/")}
    return {**rust, **web_texts()}


def counts(texts, ids):
    """Path -> how many times its non-test code names a definition id."""
    pattern = name_pattern(ids)
    blank = {
        path: blanked(text) if path.endswith(".rs") else (web_comments_blanked(text), "")
        for path, text in texts.items()
    }
    tests = declared_test_files({path: code for path, (_, code) in blank.items() if path.endswith(".rs")})
    out = {}
    for path, (no_comments, code) in sorted(blank.items()):
        rust = path.endswith(".rs")
        if rust and (relay.is_test_path(path) or path in tests):
            continue
        spans = relay.cfg_test_spans(code) if rust else []
        n = sum(1 for m in pattern.finditer(no_comments) if not any(a <= m.start() < b for a, b in spans))
        if n:
            out[path] = n
    return out


def problems(texts, ids):
    found = counts(texts, ids)
    out = []
    for path, n in found.items():
        if path not in ALLOWED:
            out.append(
                f"{path}: names an MCU or board ({n}x); read the fact from the definitions (defs::Registry), "
                "or add the file to ALLOWED with its reason"
            )
        elif n > ALLOWED[path][0]:
            out.append(f"{path}: names an MCU or board {n}x, ALLOWED gives {ALLOWED[path][0]}")
    out += [f"ALLOWED entry {path} names no MCU or board any more: remove it" for path in ALLOWED if path not in found]
    return out


class McuAndBoardNames(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.texts = production_texts()
        cls.ids = definition_ids()

    def test_only_allowed_files_name_an_mcu_or_board(self):
        self.assertEqual(problems(self.texts, self.ids), [])
        self.assertTrue(all(n > 0 and reason.strip() for n, reason in ALLOWED.values()))

    def test_the_ids_are_the_definitions_found(self):
        for known in ("rp2040", "stm32f401re", "esp32s3", "nucleo-f401re", "m5stopwatch"):
            self.assertIn(known, self.ids)
        with tempfile.TemporaryDirectory() as d:
            folder = Path(d) / "targets/mcu/fake-m1"
            folder.mkdir(parents=True)
            (folder / "dagon-mcu.toml").write_text('id = "fake-m1"\n', encoding="utf-8")
            self.assertEqual(definition_ids(Path(d)), ["fake-m1"])

    def test_a_new_name_fails(self):
        planted = {**self.texts, "crates/dagon-cli/src/deploy.rs": self.texts["crates/dagon-cli/src/deploy.rs"]
                   + '\nfn plan() -> &\'static str { if true { "stm32f401re" } else { "RP2040" } }\n'}
        self.assertTrue(any("deploy.rs" in p and "2x" in p for p in problems(planted, self.ids)), problems(planted, self.ids))
        ident = {**self.texts, "crates/dagon-host/src/lib.rs": self.texts["crates/dagon-host/src/lib.rs"]
                 + "\nconst ESP32S3_BAUD: u32 = 1;\n"}
        self.assertTrue(any("dagon-host/src/lib.rs" in p for p in problems(ident, self.ids)))
        web = {**self.texts, "web/src/app/copy.ts": self.texts["web/src/app/copy.ts"] + "\nexport const X = 'nucleo-f401re';\n"}
        self.assertTrue(any("copy.ts" in p for p in problems(web, self.ids)))
        over = {**self.texts, "crates/dagon-sim/src/store.rs": self.texts["crates/dagon-sim/src/store.rs"]
                + '\nfn more() -> [&\'static str; 9] { ["rp2040"; 9] }\n'}
        self.assertTrue(any("store.rs" in p and "ALLOWED gives" in p for p in problems(over, self.ids)))

    def test_comments_tests_and_longer_words_pass(self):
        planted = {**self.texts, "crates/dagon-cli/src/deploy.rs": self.texts["crates/dagon-cli/src/deploy.rs"]
                   + "\n// rp2040\n/* esp32s3 */\nconst F: &str = \"rp2040b\";\n"
                   "#[cfg(test)]\nmod more {\n    fn f() -> &'static str { \"stm32f401re\" }\n}\n"}
        self.assertEqual(problems(planted, self.ids), [])

    def test_console_strings_and_comments_are_read_as_typescript(self):
        ids = ["rp2040"]
        self.assertEqual(counts({"web/src/a.ts": "export const u = 'https://example.com/rp2040';\n"}, ids), {"web/src/a.ts": 1})
        self.assertEqual(counts({"web/src/a.ts": "const t = `x // ${'rp2040'}`;\n"}, ids), {"web/src/a.ts": 1})
        self.assertEqual(counts({"web/src/a.ts": "const q = 'abc\"def'; // rp2040\n/* rp2040 */\n"}, ids), {})
        self.assertEqual(counts({"web/src/A.svelte": "<!-- rp2040 -->\n<p>{x}</p>\n"}, ids), {})

    def test_an_unused_allowed_entry_fails(self):
        without = {**self.texts, "web/src/app/sample.ts": "export const NONE = 1;\n"}
        self.assertTrue(any("sample.ts" in p and "remove it" in p for p in problems(without, self.ids)))


if __name__ == "__main__":
    unittest.main()
