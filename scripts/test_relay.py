"""The relay's link contract and the relay crate have one home each.

- Exactly one `trait NodeLink`, in dagon-port.
- One production definition each of the frame helpers `hello_request` and
  `hello_answer`, in dagon-port; the host's and the browser's HELLO body
  builders, which share the name, are listed with their reason.
- dagon-port does not depend, directly or through its path dependencies,
  on dagon-agent, dagon-events or dagon-mcu-common.
- dagon-relay's normal dependencies are among RELAY_ALLOWED, and nothing
  they reach through path dependencies is embassy-time, dagon-sim or an
  MCU or board package; dagon-sim and dagon-wasm do not depend on it.
- dagon-relay names no link id or topology interface of a particular MCU
  (`link::I2C0`, `link::PIO_UART`, `iface::I2C_BUS`, `iface::PIO_UART`):
  the MCU package passes its links in.
- No Rust file names the paths the relay crate replaced:
  `dagon_mcu_rp2040::{nodes, view, cmd}`, the node items once under
  `dagon_mcu_rp2040::relay` (`named`, the node type), or those modules
  inside the RP2040 MCU package, which re-exports nothing of dagon-relay.
  No Rust file names the replaced node names (`SubNode`, `Subs`,
  `MAX_SUBS`) or brings an item in under a sub, parent or child name.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import functools
import re
import tomllib
import unittest
from pathlib import Path

from rust_source import ROOT, code_only, rust_texts

# The self-tests plant a line in one file of the tree many times over.
code_of = functools.lru_cache(maxsize=None)(code_only)

PORT_LIB = "crates/dagon-port/src/lib.rs"
PORT_MANIFEST = "crates/dagon-port/Cargo.toml"
# Other functions named like a HELLO helper: path -> why it is not a copy.
HELLO_BODIES = {
    "crates/dagon-host/src/lib.rs": "the PC's HELLO payload only (`HelloReq::encode`); the host frames every request itself",
    "crates/dagon-wasm/src/lib.rs": "the browser's HELLO payload only (`HelloReq::encode`), exported to JavaScript",
}
HELPERS = {
    "hello_request": sorted([PORT_LIB, *HELLO_BODIES]),
    "hello_answer": [PORT_LIB],
}
FORBIDDEN_DEPS = {"dagon-agent", "dagon-events", "dagon-mcu-common"}

RELAY_MANIFEST = "crates/dagon-relay/Cargo.toml"
RELAY_SRC = "crates/dagon-relay/src/"
# The relay's dependency direction: what it may name, and what nothing it
# reaches may be (the relay is MCU-independent and no_std; dagon-sim is
# for its tests only).
RELAY_ALLOWED = {
    "dagon-agent", "dagon-port", "dagon-proto", "dagon-format",
    "heapless", "embassy-sync", "embassy-futures", "portable-atomic",
}
RELAY_FORBIDDEN = {"embassy-time", "dagon-sim"}
RELAY_FORBIDDEN_PREFIXES = ("dagon-mcu-", "dagon-board-")
# Packages that must not depend on the relay (the simulator stays as is).
RELAY_FREE = ("crates/dagon-sim/Cargo.toml", "crates/dagon-wasm/Cargo.toml")
MCU_LINK_NAMES = re.compile(r"\b(?:link\s*::\s*(?:I2C0|PIO_UART)|iface\s*::\s*(?:I2C_BUS|PIO_UART))\b")
RP_PACKAGE = "targets/mcu/rp2040/src/"
MOVED = r"(?:nodes|view|cmd)"
OLD_PATHS = (
    re.compile(rf"\bdagon_mcu_rp2040\s*::\s*{MOVED}\b"),
    re.compile(rf"\bdagon_mcu_rp2040\s*::\s*\{{[^}}]*\b{MOVED}\b"),
)
OLD_PATHS_INSIDE = (
    re.compile(rf"\bcrate\s*::\s*{MOVED}\b"),
    re.compile(rf"\bcrate\s*::\s*\{{[^}}]*\b{MOVED}\b"),
    re.compile(rf"\bmod\s+{MOVED}\b"),
    re.compile(r"\bpub\s*(?:\([^)]*\)\s*)?use\s+(?:::\s*)?dagon_relay\b"),
)
OLD_RELAY_ITEMS = r"(?:named|LinkedNode|SubNode|nodes)"
OLD_NAMES = (
    re.compile(rf"\bdagon_mcu_rp2040\s*::\s*relay\s*::\s*{OLD_RELAY_ITEMS}\b"),
    re.compile(rf"\bdagon_mcu_rp2040\s*::\s*relay\s*::\s*\{{[^}}]*\b{OLD_RELAY_ITEMS}\b"),
    re.compile(r"\b(?:SubNode|Subs|MAX_SUBS)\b"),
    re.compile(r"\bas\s+(?:\w*_)?(?:[Ss]ub|[Pp]arent|[Cc]hild|SUB|PARENT|CHILD)(?:[A-Z_]\w*)?\b"),
)

TEST_MOD = re.compile(r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")


def cfg_test_spans(code):
    """(start, end) of each `#[cfg(test)] mod … { … }` in `code`."""
    spans = []
    for m in TEST_MOD.finditer(code):
        depth, i = 1, m.end()
        while i < len(code) and depth:
            depth += {"{": 1, "}": -1}.get(code[i], 0)
            i += 1
        spans.append((m.start(), i))
    return spans


def is_test_path(path):
    """Integration tests, and `tests.rs` files or `tests/` modules."""
    parts = path.split("/")
    return "tests" in parts[:-1] or parts[-1] == "tests.rs"


def definitions(codes, name, pattern):
    """Production paths matching `pattern` (with `{name}`), once per match."""
    found = []
    for path, code in codes.items():
        if is_test_path(path):
            continue
        spans = cfg_test_spans(code)
        for m in re.finditer(pattern.format(name=name), code):
            if not any(a <= m.start() < b for a, b in spans):
                found.append(path)
    return sorted(found)


def manifest_deps(manifest):
    """(key, entry) of every dependency in every dependency table (normal,
    dev, build, and each `[target.*]` one); an entry is a table or a version."""
    out = []

    def walk(table):
        for key, value in table.items():
            if not isinstance(value, dict):
                continue
            if key.endswith("dependencies"):
                out.extend(value.items())
            else:
                walk(value)

    walk({k: v for k, v in manifest.items() if k != "workspace"})
    return out


def resolve(key, entry, manifest_dir, workspace):
    """(package name, manifest path or None) of one dependency: the name is
    `package` when the entry renames it, else the key; `workspace = true`
    takes the entry from the root `[workspace.dependencies]`, whose paths
    are relative to the root."""
    if isinstance(entry, dict) and entry.get("workspace") is True:
        entry, manifest_dir = workspace.get(key, {}), Path(".")
    if not isinstance(entry, dict):
        return key, None
    path = entry.get("path")
    if path is not None:
        path = Path(*_normal((manifest_dir / path / "Cargo.toml").as_posix().split("/")))
    return entry.get("package", key), path


def normal_deps(manifest):
    """(key, entry) of every normal dependency: `[dependencies]` and each
    `[target.*.dependencies]`."""
    out = list(manifest.get("dependencies", {}).items())
    for table in manifest.get("target", {}).values():
        out.extend(table.get("dependencies", {}).items())
    return out


def dependency_closure(manifest_path, read, deps=manifest_deps):
    """Package names of every dependency of the package at `manifest_path`
    (the tables `deps` returns), following path dependencies; `read(path)`
    returns a manifest's TOML text. These packages are members of the root
    workspace, so `workspace = true` resolves there."""
    workspace = tomllib.loads(read(Path("Cargo.toml"))).get("workspace", {}).get("dependencies", {})
    seen, visited, todo = set(), set(), [Path(manifest_path)]
    while todo:
        m = todo.pop()
        if m.as_posix() in visited:
            continue
        visited.add(m.as_posix())
        for key, entry in deps(tomllib.loads(read(m))):
            name, path = resolve(key, entry, m.parent, workspace)
            seen.add(name)
            if path is not None:
                todo.append(path)
    return seen


def _normal(parts):
    out = []
    for p in parts:
        if p == "..":
            out.pop()
        elif p not in ("", "."):
            out.append(p)
    return out


def read_manifest(path):
    return (ROOT / path).read_text(encoding="utf-8")


def relay_dependency_problems(read):
    try:
        manifest = tomllib.loads(read(Path(RELAY_MANIFEST)))
    except FileNotFoundError:
        return [f"{RELAY_MANIFEST} is missing"]
    workspace = tomllib.loads(read(Path("Cargo.toml"))).get("workspace", {}).get("dependencies", {})
    out = []
    direct = {resolve(k, e, Path(RELAY_MANIFEST).parent, workspace)[0] for k, e in normal_deps(manifest)}
    if extra := sorted(direct - RELAY_ALLOWED):
        out.append(f"dagon-relay depends on {extra}, outside {sorted(RELAY_ALLOWED)}")
    reached = dependency_closure(RELAY_MANIFEST, read, normal_deps)
    if bad := sorted(n for n in reached if n in RELAY_FORBIDDEN or n.startswith(RELAY_FORBIDDEN_PREFIXES)):
        out.append(f"dagon-relay reaches {bad}")
    for m in RELAY_FREE:
        if "dagon-relay" in dependency_closure(m, read):
            out.append(f"{m} depends on dagon-relay")
    return out


def relay_source_problems(codes):
    out = []
    for path, code in sorted(codes.items()):
        if path.startswith(RELAY_SRC) and (m := MCU_LINK_NAMES.search(code)):
            out.append(f"{path} names {m.group(0)}: the MCU package passes its links in")
        rules = OLD_PATHS + OLD_NAMES + (OLD_PATHS_INSIDE if path.startswith(RP_PACKAGE) else ())
        for rule in rules:
            if m := rule.search(code):
                out.append(f"{path} names the old path {' '.join(m.group(0).split())}")
    return out


def problems(texts, read=read_manifest):
    codes = {path: code_of(text) for path, text in texts.items()}
    out = relay_dependency_problems(read) + relay_source_problems(codes)
    if (t := definitions(codes, "NodeLink", r"\btrait\s+{name}\b")) != [PORT_LIB]:
        out.append(f"trait NodeLink in {t}, expected only {PORT_LIB}")
    for name, expected in HELPERS.items():
        if (t := definitions(codes, name, r"\bfn\s+{name}\b")) != expected:
            out.append(f"fn {name} in {t}, expected {expected}")
    if bad := sorted(dependency_closure(PORT_MANIFEST, read) & FORBIDDEN_DEPS):
        out.append(f"dagon-port depends on {bad}")
    return out


class OneNodeLinkContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.texts = rust_texts()

    def planted(self, path, extra):
        return {**self.texts, path: self.texts.get(path, "") + extra}

    def test_the_tree_holds(self):
        self.assertEqual(problems(self.texts), [])

    def test_a_second_node_link_trait_fails(self):
        for path in ("targets/mcu/common/src/uart.rs", "crates/dagon-port/src/lib.rs"):
            with self.subTest(path=path):
                self.assertTrue(problems(self.planted(path, "\npub trait NodeLink {}\n")))

    def test_a_missing_node_link_trait_fails(self):
        moved = {**self.texts, PORT_LIB: self.texts[PORT_LIB].replace("trait NodeLink", "trait OtherLink")}
        self.assertNotEqual(moved[PORT_LIB], self.texts[PORT_LIB])
        self.assertTrue(problems(moved))

    def test_a_copy_of_a_hello_helper_fails(self):
        for name in HELPERS:
            for path in ("targets/mcu/common/src/uart.rs", "targets/mcu/rp2040/src/links.rs", "crates/dagon-host/src/lib.rs"):
                with self.subTest(name=name, path=path):
                    self.assertTrue(problems(self.planted(path, f"\npub fn {name}() {{}}\n")))

    def test_a_helper_in_test_code_comments_or_strings_passes(self):
        for extra in (
            "\n#[cfg(test)]\nmod tests {\n    fn hello_answer() {}\n    trait NodeLink {}\n}\n",
            "\n// pub fn hello_request() {}\nconst S: &str = \"trait NodeLink\";\n",
        ):
            with self.subTest(extra=extra):
                self.assertEqual(problems(self.planted("targets/mcu/common/src/uart.rs", extra)), [])
        self.assertEqual(problems({**self.texts, "crates/dagon-port/tests/hello.rs": "fn hello_request() {}\n"}), [])

    def test_code_after_a_test_module_still_counts(self):
        extra = "\n#[cfg(test)]\nmod tests {\n    fn f() { if true { } }\n}\npub fn hello_answer() {}\n"
        self.assertTrue(problems(self.planted("targets/mcu/common/src/uart.rs", extra)))

    def test_a_forbidden_dependency_fails(self):
        for dep in sorted(FORBIDDEN_DEPS):
            with self.subTest(dep=dep):
                direct = read_manifest(PORT_MANIFEST) + f'\n[dev-dependencies]\n{dep} = "0.1"\n'
                self.assertTrue(problems(self.texts, lambda p, t=direct: t if p.as_posix() == PORT_MANIFEST else read_manifest(p)))

    def test_a_forbidden_dependency_through_a_path_dependency_fails(self):
        proto = "crates/dagon-proto/Cargo.toml"
        planted = read_manifest(proto) + '\ndagon-events = { path = "../dagon-events" }\n'
        self.assertTrue(problems(self.texts, lambda p: planted if p.as_posix() == proto else read_manifest(p)))

    def with_manifests(self, planted):
        """problems() with `planted` (path -> text appended to that manifest)."""
        return problems(self.texts, lambda p: read_manifest(p) + planted.get(p.as_posix(), ""))

    def test_a_renamed_dependency_fails(self):
        for table in ("dependencies", "dev-dependencies", "build-dependencies", 'target."cfg(test)".dependencies'):
            for entry in ('{ package = "dagon-agent", version = "0.1" }', '{ package = "dagon-mcu-common", path = "../../targets/mcu/common" }'):
                with self.subTest(table=table, entry=entry):
                    # The manifest ends in its [dependencies] table.
                    header = "" if table == "dependencies" else f"[{table}]\n"
                    self.assertTrue(self.with_manifests({PORT_MANIFEST: f"\n{header}alias = {entry}\n"}))

    def test_a_renamed_dependency_through_a_path_dependency_fails(self):
        proto = "crates/dagon-proto/Cargo.toml"
        self.assertTrue(self.with_manifests({proto: '\nevents_alias = { package = "dagon-events", path = "../dagon-events" }\n'}))

    def test_a_workspace_dependency_fails(self):
        for root, port in (
            ('\n[workspace.dependencies]\nagent_alias = { package = "dagon-agent", path = "crates/dagon-agent" }\n', "\n[dev-dependencies]\nagent_alias = { workspace = true }\n"),
            ('\n[workspace.dependencies]\ndagon-events = { path = "crates/dagon-events" }\n', "\n[dev-dependencies]\ndagon-events = { workspace = true }\n"),
            ('\n[workspace.dependencies]\nshared = { package = "dagon-format", path = "crates/dagon-format" }\n', "\n[dev-dependencies]\nshared = { workspace = true }\n"),
        ):
            with self.subTest(root=root):
                planted = {"Cargo.toml": root, PORT_MANIFEST: port}
                if "dagon-format" in root:
                    planted["crates/dagon-format/Cargo.toml"] = '\nevents_alias = { package = "dagon-events", version = "0.1" }\n'
                self.assertTrue(self.with_manifests(planted))

    def test_a_renamed_allowed_dependency_passes(self):
        self.assertEqual(self.with_manifests({PORT_MANIFEST: '\n[dev-dependencies]\nformat = { package = "dagon-format", path = "../dagon-format" }\n'}), [])


class RelayCrate(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.texts = rust_texts()

    def with_entries(self, planted):
        """problems() with `planted` (path -> (table header, entry line)):
        the entry goes first into that table, which is added when the
        manifest has none."""
        def read(p):
            text = read_manifest(p)
            if (plant := planted.get(p.as_posix())) is None:
                return text
            header, entry = plant
            head = f"{header}\n"
            if text.startswith(head) or f"\n{head}" in text:
                at = text.index(head) if text.startswith(head) else text.index(f"\n{head}") + 1
                return text[:at + len(head)] + f"{entry}\n" + text[at + len(head):]
            return f"{text}\n{head}{entry}\n"
        return problems(self.texts, read)

    def test_a_dependency_outside_the_allowed_set_fails(self):
        for entry in (
            'embassy-time = "0.5"',
            'dagon-sim = { path = "../dagon-sim" }',
            'dagon-events = { path = "../dagon-events" }',
            'clock = { package = "embassy-time", version = "0.5" }',
            'dagon-mcu-rp2040 = { path = "../../targets/mcu/rp2040" }',
        ):
            for header in ("[dependencies]", "[target.'cfg(target_os = \"none\")'.dependencies]"):
                with self.subTest(entry=entry, header=header):
                    self.assertTrue(self.with_entries({RELAY_MANIFEST: (header, entry)}))

    def test_a_forbidden_package_through_a_path_dependency_fails(self):
        for entry in ('embassy-time = "0.5"', 'dagon-board-x = "0.1"'):
            with self.subTest(entry=entry):
                self.assertTrue(self.with_entries({PORT_MANIFEST: ("[dependencies]", entry)}))

    def test_the_simulator_as_a_dev_dependency_passes(self):
        planted = {RELAY_MANIFEST: ("[dev-dependencies]", 'dagon-sim = { path = "../dagon-sim" }')}
        self.assertEqual(self.with_entries(planted), [])

    def test_the_simulator_depending_on_the_relay_fails(self):
        for m in RELAY_FREE:
            for header in ("[dependencies]", "[dev-dependencies]"):
                with self.subTest(manifest=m, header=header):
                    self.assertTrue(self.with_entries({m: (header, 'dagon-relay = { path = "../dagon-relay" }')}))

    def test_a_missing_relay_crate_fails(self):
        def read(p):
            if p.as_posix() == RELAY_MANIFEST:
                raise FileNotFoundError(p)
            return read_manifest(p)
        self.assertTrue(problems(self.texts, read))

    def test_an_mcu_link_name_in_the_relay_fails(self):
        for name in ("link::I2C0", "link :: PIO_UART", "iface::I2C_BUS", "iface::PIO_UART"):
            with self.subTest(name=name):
                planted = {**self.texts, f"{RELAY_SRC}planted.rs": f"const X: u8 = {name};\n"}
                self.assertTrue(problems(planted))

    def test_an_mcu_link_name_in_a_comment_or_outside_the_relay_passes(self):
        planted = {
            **self.texts,
            f"{RELAY_SRC}planted.rs": "// link::I2C0\nconst S: &str = \"iface::PIO_UART\";\n",
            f"{RP_PACKAGE}planted.rs": "const X: u8 = link::I2C0;\n",
        }
        self.assertEqual(problems(planted), [])

    def test_an_old_path_fails(self):
        for path, code in (
            ("targets/boards/x/src/screen.rs", "use dagon_mcu_rp2040::view::Home;\n"),
            ("targets/boards/x/src/screen.rs", "use dagon_mcu_rp2040::{arm, nodes::HUB};\n"),
            ("targets/mcu/rp2040/tests/x.rs", "fn f() { dagon_mcu_rp2040::cmd::relay_fits(); }\n"),
            (f"{RP_PACKAGE}lib.rs", "pub mod nodes;\n"),
            (f"{RP_PACKAGE}relay/mod.rs", "pub use crate::view;\n"),
            (f"{RP_PACKAGE}relay/pc.rs", "use crate::{cmd, node};\n"),
            (f"{RP_PACKAGE}relay/mod.rs", "pub use dagon_relay::nodes::{named, LinkedNode as SubNode};\n"),
            (f"{RP_PACKAGE}relay/mod.rs", "pub use dagon_relay::nodes::named;\n"),
            (f"{RP_PACKAGE}lib.rs", "pub(crate) use ::dagon_relay::view;\n"),
            ("targets/rp2040-hub/src/main.rs", "use dagon_mcu_rp2040::relay::{self, named, RelayConfig};\n"),
            ("targets/rp2040-hub/src/main.rs", "static W: [dagon_mcu_rp2040::relay::LinkedNode; 0] = [];\n"),
            ("targets/rp2040-hub/src/main.rs", "static W: [SubNode; 0] = [];\n"),
            ("targets/boards/x/src/screen.rs", "use dagon_relay::nodes::LinkedNode as Child;\n"),
            ("targets/boards/x/src/screen.rs", "use dagon_relay::nodes::MAX_LINKED as MAX_SUBS;\n"),
            ("crates/dagon-relay/src/nodes.rs", "pub type Subs = LinkedNodes;\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertTrue(problems({**self.texts, path: self.texts.get(path, "") + "\n" + code}))

    def test_the_new_paths_and_other_modules_pass(self):
        planted = {
            **self.texts,
            "targets/boards/x/src/screen.rs": "use dagon_relay::view::Home;\nuse dagon_mcu_rp2040::node;\n// dagon_mcu_rp2040::view\n",
            "crates/dagon-host/src/planted.rs": "mod view;\nuse crate::nodes;\n",
            f"{RP_PACKAGE}planted.rs": "use dagon_relay::nodes::LinkedNode;\nuse crate::sub::SubLines;\nuse x::Node as Peer;\n",
        }
        self.assertEqual(problems(planted), [])


if __name__ == "__main__":
    unittest.main()
