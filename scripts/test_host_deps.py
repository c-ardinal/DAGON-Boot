"""The host's time limits have one home, and no MCU is chosen by its id.

- dagon-host's normal dependencies, followed through path dependencies,
  reach neither dagon-conform nor dagon-sim (renamed and workspace entries
  resolved as in test_relay.py).
- Every `const` declared in dagon_port::timeout (found by reading that
  module, never listed here) has no second definition of the same name in
  non-test Rust under crates/ or targets/.
- No non-test Rust under crates/ compares a value with an `mcu_id::`
  constant, matches on one or imports one by name: a time limit, a size or
  a path for an MCU comes from its definition, so a new MCU is data.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import unittest
from pathlib import Path

import test_relay as relay
from rust_source import rust_texts

HOST_MANIFEST = "crates/dagon-host/Cargo.toml"
HOST_FORBIDDEN = {"dagon-conform", "dagon-sim"}
TIMEOUT = "crates/dagon-port/src/timeout.rs"
CONST_DIRS = ("crates/", "targets/")
MCU_ID_DIR = "crates/"

CONST = re.compile(r"\bconst\s+([A-Za-z_]\w*)\s*:")
MCU_ID = r"\bmcu_id\s*::\s*[A-Z_][A-Z0-9_]*\b"
MCU_ID_RULES = (
    (re.compile(rf"(?:==|!=)\s*(?:[\w:]*::\s*)?{MCU_ID}"), "compares with"),
    (re.compile(rf"{MCU_ID}\s*(?:==|!=)"), "compares with"),
    (re.compile(rf"{MCU_ID}\s*(?:=>|\||if\b)"), "matches on"),
    (re.compile(rf"\bmatches!\s*\([^;]*?{MCU_ID}"), "matches on"),
    (re.compile(r"\buse\s+[^;]*\bmcu_id\s*::\s*(?:\{[^}]*\}|[A-Z_][A-Z0-9_]*|\*)"), "imports"),
)


def production(codes, prefixes):
    """(path, code with its `#[cfg(test)]` modules blanked) of every
    non-test file under `prefixes`."""
    for path, code in sorted(codes.items()):
        if not path.startswith(prefixes) or relay.is_test_path(path):
            continue
        for a, b in reversed(relay.cfg_test_spans(code)):
            code = code[:a] + " " * (b - a) + code[b:]
        yield path, code


def const_items(code, name=None):
    """Names of the const items `code` declares (`name` only, when given),
    once per declaration; a const generic parameter (`<const N: usize>`)
    declares none."""
    out = []
    for m in CONST.finditer(code):
        if code[:m.start()].rstrip().endswith(("<", ",")):
            continue
        if name is None or m.group(1) == name:
            out.append(m.group(1))
    return out


def timeout_consts(codes):
    """Names of the consts declared in dagon_port::timeout, outside its tests."""
    code = dict(production(codes, (TIMEOUT,))).get(TIMEOUT, "")
    return sorted(set(const_items(code)))


def const_problems(codes):
    names = timeout_consts(codes)
    if not names:
        return [f"{TIMEOUT} declares no const (the module is missing or moved)"]
    out = []
    for name in names:
        found = sorted(p for p, code in production(codes, CONST_DIRS) for _ in const_items(code, name))
        if found != [TIMEOUT]:
            out.append(f"const {name} defined in {found}, expected only {TIMEOUT}")
    return out


def mcu_id_problems(codes):
    out = []
    for path, code in production(codes, (MCU_ID_DIR,)):
        for rule, what in MCU_ID_RULES:
            if m := rule.search(code):
                out.append(f"{path} {what} an MCU id: {' '.join(m.group(0).split())}")
    return out


def dependency_problems(read):
    reached = relay.dependency_closure(HOST_MANIFEST, read, relay.normal_deps)
    return [f"dagon-host reaches {sorted(reached & HOST_FORBIDDEN)}"] if reached & HOST_FORBIDDEN else []


def problems(texts, read=relay.read_manifest):
    codes = {path: relay.code_of(text) for path, text in texts.items()}
    return dependency_problems(read) + const_problems(codes) + mcu_id_problems(codes)


class HostDeps(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.texts = rust_texts()

    def planted(self, path, extra):
        return {**self.texts, path: self.texts.get(path, "") + extra}

    def with_entries(self, planted):
        """problems() with `planted` (path -> (table header, entry line)): the
        entry goes first into that table, added when the manifest has none."""
        def read(p):
            text = relay.read_manifest(p)
            if (plant := planted.get(p.as_posix())) is None:
                return text
            header, entry = plant
            head = f"{header}\n"
            if text.startswith(head) or f"\n{head}" in text:
                at = text.index(head) if text.startswith(head) else text.index(f"\n{head}") + 1
                return text[:at + len(head)] + f"{entry}\n" + text[at + len(head):]
            return f"{text}\n{head}{entry}\n"
        return problems(self.texts, read)

    def test_the_tree_holds(self):
        self.assertEqual(problems(self.texts), [])

    def test_the_timeout_consts_are_read_from_the_module(self):
        names = timeout_consts({p: relay.code_of(t) for p, t in self.texts.items()})
        self.assertIn("REQUEST_MS", names)
        planted = {**self.texts, TIMEOUT: self.texts.get(TIMEOUT, "") + "\npub const PLANTED_MS: u32 = 1;\n"}
        codes = {p: relay.code_of(t) for p, t in planted.items()}
        self.assertIn("PLANTED_MS", timeout_consts(codes))

    def test_a_conformance_or_simulator_dependency_fails(self):
        for entry in (
            'dagon-conform = { path = "../dagon-conform" }',
            'dagon-sim = { path = "../dagon-sim" }',
            'limits = { package = "dagon-conform", path = "../dagon-conform" }',
        ):
            for header in ("[dependencies]", "[target.'cfg(target_arch = \"wasm32\")'.dependencies]"):
                with self.subTest(entry=entry, header=header):
                    self.assertTrue(self.with_entries({HOST_MANIFEST: (header, entry)}))

    def test_a_conformance_dependency_through_a_path_dependency_fails(self):
        for m in ("crates/dagon-port/Cargo.toml", "crates/dagon-romlink/Cargo.toml"):
            with self.subTest(manifest=m):
                self.assertTrue(self.with_entries({m: ("[dependencies]", 'c = { package = "dagon-conform", path = "../dagon-conform" }')}))

    def test_a_workspace_conformance_dependency_fails(self):
        planted = {
            "Cargo.toml": ("[workspace.dependencies]", 'conf = { package = "dagon-conform", path = "crates/dagon-conform" }'),
            HOST_MANIFEST: ("[dependencies]", "conf = { workspace = true }"),
        }
        self.assertTrue(self.with_entries(planted))

    def test_a_dev_dependency_passes(self):
        planted = {HOST_MANIFEST: ("[dev-dependencies]", 'dagon-conform = { path = "../dagon-conform" }')}
        self.assertEqual(self.with_entries(planted), [])

    def test_a_second_timeout_const_fails(self):
        for path in ("crates/dagon-host/src/ops.rs", "crates/dagon-sim/src/hub.rs", "targets/mcu/rp2040/src/node.rs"):
            for name in ("REQUEST_MS", "ERASE_MARGIN_MS", "NODE_POLL_MS"):
                with self.subTest(path=path, name=name):
                    self.assertTrue(problems(self.planted(path, f"\nconst {name}: u64 = 1;\n")))

    def test_a_timeout_const_in_test_code_comments_or_strings_passes(self):
        for extra in (
            "\n#[cfg(test)]\nmod tests {\n    const REQUEST_MS: u32 = 1;\n}\n",
            "\n// const REQUEST_MS: u32 = 1;\nconst S: &str = \"const VERIFY_MS: u32 = 1;\";\n",
        ):
            with self.subTest(extra=extra):
                self.assertEqual(problems(self.planted("crates/dagon-host/src/ops.rs", extra)), [])
        self.assertEqual(problems({**self.texts, "crates/dagon-host/tests/x.rs": "const REQUEST_MS: u32 = 1;\n"}), [])

    def test_a_const_generic_parameter_is_no_definition(self):
        extra = "\nfn f<const REQUEST_MS: usize>() {}\nstruct S<T, const VERIFY_MS: u8>(T);\n"
        self.assertEqual(problems(self.planted("crates/dagon-host/src/ops.rs", extra)), [])
        codes = {p: relay.code_of(t) for p, t in self.planted(TIMEOUT, "\nfn g<const PLANTED_MS: usize>() {}\n").items()}
        self.assertNotIn("PLANTED_MS", timeout_consts(codes))

    def test_a_missing_timeout_module_fails(self):
        self.assertTrue(problems({p: t for p, t in self.texts.items() if p != TIMEOUT}))

    def test_choosing_by_mcu_id_fails(self):
        for code in (
            "fn f(id: u16) -> u32 { if id == dagon_format::header::mcu_id::STM32F401XE { 1 } else { 2 } }",
            "fn f(id: u16) -> bool { mcu_id::RP2040 != id }",
            "fn f(id: u16) -> u32 { match id { mcu_id::ESP32S3 => 1, _ => 2 } }",
            "fn f(id: u16) -> u32 { match id { mcu_id::ESP32S3 | mcu_id::RP2040 => 1, _ => 2 } }",
            "fn f(id: u16) -> bool { matches!(id, header::mcu_id::RP2040) }",
            "use dagon_format::header::mcu_id::STM32F401XE;",
            "use dagon_format::header::mcu_id::{RP2040, ESP32S3};",
        ):
            for path in ("crates/dagon-host/src/ops.rs", "crates/dagon-conform/src/pc.rs"):
                with self.subTest(code=code, path=path):
                    self.assertTrue(problems(self.planted(path, f"\n{code}\n")))

    def test_naming_an_mcu_id_without_choosing_by_it_passes(self):
        for extra in (
            "\nconst X: Config = Config { mcu_id: mcu_id::RP2040 };\n",
            "\n// if id == mcu_id::RP2040\n",
            "\n#[cfg(test)]\nmod tests {\n    fn f(id: u16) -> bool { id == mcu_id::RP2040 }\n}\n",
        ):
            with self.subTest(extra=extra):
                self.assertEqual(problems(self.planted("crates/dagon-sim/src/lib.rs", extra)), [])
        self.assertEqual(problems({**self.texts, "crates/dagon-host/tests/x.rs": "fn f(i: u16) -> bool { i == mcu_id::RP2040 }\n"}), [])
        self.assertEqual(problems(self.planted("targets/mcu/stm32f401re/src/layout.rs", "\nfn f(i: u16) -> bool { i == mcu_id::RP2040 }\n")), [])


if __name__ == "__main__":
    unittest.main()
