"""The host's time limits have one home, and no MCU has an assigned number.

- dagon-host's normal dependencies, followed through path dependencies,
  reach neither dagon-conform nor dagon-sim (renamed and workspace entries
  resolved as in test_relay.py).
- Every `const` declared in dagon_port::timeout (found by reading that
  module, never listed here) has no second definition of the same name in
  non-test Rust under crates/ or targets/.
- No non-test Rust under crates/ or targets/ holds a table of MCU numbers
  (a `mod mcu_id`) or a definition's `wire_id`: an MCU's identity is taken
  from its definition's id (`McuId::of_name`), so a new MCU is data.

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
CONST = re.compile(r"\bconst\s+([A-Za-z_]\w*)\s*:")
MCU_TABLE = re.compile(r"\bmod\s+mcu_id\b|\bwire_id\b")


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


def mcu_table_problems(codes):
    return [
        f"{path} holds an assigned MCU number: {m.group(0)}"
        for path, code in production(codes, CONST_DIRS)
        if (m := MCU_TABLE.search(code))
    ]


def dependency_problems(read):
    reached = relay.dependency_closure(HOST_MANIFEST, read, relay.normal_deps)
    return [f"dagon-host reaches {sorted(reached & HOST_FORBIDDEN)}"] if reached & HOST_FORBIDDEN else []


def problems(texts, read=relay.read_manifest):
    codes = {path: relay.code_of(text) for path, text in texts.items()}
    return dependency_problems(read) + const_problems(codes) + mcu_table_problems(codes)


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

    def test_an_assigned_mcu_number_fails(self):
        for code in ("pub mod mcu_id { pub const RP2040: u16 = 1; }", "struct McuDef { wire_id: u16 }"):
            for path in ("crates/dagon-format/src/header.rs", "targets/mcu/build/src/lib.rs"):
                with self.subTest(code=code, path=path):
                    self.assertTrue(problems(self.planted(path, f"\n{code}\n")))

    def test_an_identity_taken_from_the_id_passes(self):
        for extra in (
            "\nfn f(h: &ImageHeader, d: &McuDef) -> bool { h.mcu_id == d.mcu_id() }\n",
            "\n// the old wire_id\n",
            "\n#[cfg(test)]\nmod tests {\n    const OLD: &str = \"wire_id = 1\";\n}\n",
        ):
            with self.subTest(extra=extra):
                self.assertEqual(problems(self.planted("crates/dagon-sim/src/lib.rs", extra)), [])


if __name__ == "__main__":
    unittest.main()
