"""No production code reads the built-in definitions where a project's own
(its overrides included) apply.

Every call of `Registry::builtin()` in non-test Rust under crates/,
targets/ and samples/ sits in a function of ALLOWED, each with the reason
the built-in set is right there; any other call fails, and so does an
entry of ALLOWED that no call uses any more. Test code (integration tests,
`tests.rs`, `tests/` modules, files a `#[cfg(test)] mod name;` declares and
`#[cfg(test)]` modules) is left out.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import unittest
from pathlib import PurePosixPath

import test_relay as relay
from rust_source import code_only, rust_texts

# (file, enclosing function) -> why the built-in definitions are right there.
ALLOWED = {
    ("crates/dagon-topology/src/compile.rs", "compile"):
        "the built-in sample topologies of `dagon demo` and `dagon sim` (compile_str goes through it); a project compiles with compile_with",
    ("crates/dagon-topology/src/compile.rs", "mcu_id_of"):
        "wire ids of the built-in sample images of `dagon demo` and `dagon sim`; a project reads its registry's MCU",
    ("crates/dagon-sign/src/image.rs", "slot"):
        "slots of the built-in sample images of `dagon demo` and `dagon sim`; a project signs with slot_with",
    ("crates/dagon-sim/src/store.rs", "trial_attempts"):
        "the simulator's modelled boot stage over its fixed sample layouts; no project is in scope",
    ("crates/dagon-sim/src/mcu.rs", "simulated_mcu"):
        "the simulator's modelled nodes (layouts, flash, relay links) over its fixed sample MCUs; no project is in scope",
    ("crates/dagon-cli/src/simrun.rs", "world"):
        "`dagon sim` scenarios run the built-in sample world",
    ("crates/dagon-cli/src/simrun.rs", "host"):
        "node timings of the built-in sample world of `dagon sim`",
    ("crates/dagon-cli/src/simrun.rs", "s4"):
        "ROM maps of the built-in sample world of `dagon sim`",
    ("crates/dagon-cli/src/simrun.rs", "seg_at"):
        "the seg's target from the hub of the built-in sample world of `dagon sim`",
    ("crates/dagon-cli/src/serve/api.rs", "registry_of"):
        "`dagon serve` with no project open has no definitions but the built-in ones",
    ("crates/dagon-wasm/src/defs.rs", "with_registry"):
        "the page's definitions before it loads a project's (load_definitions sends the merged ones)",
}

CALL = re.compile(r"\bRegistry\s*::\s*builtin\s*\(")
FN = re.compile(r"\bfn\s+([A-Za-z_]\w*)")
TEST_DECL = re.compile(r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")


def declared_test_files(codes):
    """Files that a `#[cfg(test)] mod name;` declares (name.rs or name/mod.rs
    beside the declaring module's own folder)."""
    out = set()
    for path, code in codes.items():
        p = PurePosixPath(path)
        folder = p.parent if p.name in ("mod.rs", "lib.rs", "main.rs") else p.parent / p.stem
        for m in TEST_DECL.finditer(code):
            out.add(str(folder / f"{m.group(1)}.rs"))
            out.add(str(folder / m.group(1) / "mod.rs"))
    return out


def calls(texts):
    """(file, enclosing function) of every production `Registry::builtin()` call."""
    codes = {path: code_only(text) for path, text in texts.items()}
    tests = declared_test_files(codes)
    found = []
    for path, code in sorted(codes.items()):
        if relay.is_test_path(path) or path in tests:
            continue
        spans = relay.cfg_test_spans(code)
        for m in CALL.finditer(code):
            if any(a <= m.start() < b for a, b in spans):
                continue
            fns = FN.findall(code[:m.start()])
            found.append((path, fns[-1] if fns else "<top level>"))
    return found


def problems(texts):
    found = calls(texts)
    out = [
        f"{path}: Registry::builtin() in `{fn}` reads the built-in definitions where a project's apply; "
        "pass the project's Registry (crate defs::registry), or add the function to ALLOWED with its reason"
        for path, fn in found
        if (path, fn) not in ALLOWED
    ]
    out += [f"ALLOWED entry {key} has no Registry::builtin() call any more: remove it" for key in ALLOWED if key not in found]
    return out


class BuiltinRegistryCallers(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.texts = rust_texts()

    def planted(self, path, extra):
        return {**self.texts, path: self.texts.get(path, "") + extra}

    def test_every_production_call_is_allowed_with_its_reason(self):
        self.assertEqual(problems(self.texts), [])
        self.assertTrue(all(reason.strip() for reason in ALLOWED.values()))

    def test_a_new_production_call_fails(self):
        planted = self.planted("crates/dagon-cli/src/deploy.rs", "\nfn plan() { let r = Registry::builtin(); }\n")
        self.assertTrue(any("`plan`" in p for p in problems(planted)), problems(planted))
        qualified = self.planted("crates/dagon-host/src/lib.rs", "\nfn t() { dagon_topology::defs::Registry :: builtin(); }\n")
        self.assertTrue(any("`t`" in p for p in problems(qualified)))

    def test_test_code_comments_and_strings_pass(self):
        planted = self.planted(
            "crates/dagon-cli/src/deploy.rs",
            "\n// Registry::builtin()\nconst S: &str = \"Registry::builtin()\";\n"
            "#[cfg(test)]\nmod more {\n    fn f() { Registry::builtin(); }\n}\n",
        )
        self.assertEqual(problems(planted), [])
        declared = {**self.texts, "crates/dagon-cli/src/x.rs": "#[cfg(test)]\nmod x_tests;\n",
                    "crates/dagon-cli/src/x/x_tests.rs": "fn f() { Registry::builtin(); }\n"}
        self.assertEqual(problems(declared), [])

    def test_an_unused_allowed_entry_fails(self):
        without = {**self.texts, "crates/dagon-sim/src/store.rs": self.texts["crates/dagon-sim/src/store.rs"].replace("Registry::builtin()", "reg")}
        self.assertTrue(any("trial_attempts" in p for p in problems(without)))


if __name__ == "__main__":
    unittest.main()
