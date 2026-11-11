"""The relay's link contract has one home.

- Exactly one `trait NodeLink`, in dagon-port.
- One production definition each of the frame helpers `hello_request` and
  `hello_answer`, in dagon-port; the host's and the browser's HELLO body
  builders, which share the name, are listed with their reason.
- dagon-port does not depend, directly or through its path dependencies,
  on dagon-agent, dagon-events or dagon-mcu-common.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import tomllib
import unittest
from pathlib import Path

from rust_source import ROOT, code_only, rust_texts

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


def dependency_closure(manifest_path, read):
    """Package names of every dependency of the package at `manifest_path`,
    following path dependencies; `read(path)` returns a manifest's TOML
    text. dagon-port and its path dependencies are members of the root
    workspace, so `workspace = true` resolves there."""
    workspace = tomllib.loads(read(Path("Cargo.toml"))).get("workspace", {}).get("dependencies", {})
    seen, visited, todo = set(), set(), [Path(manifest_path)]
    while todo:
        m = todo.pop()
        if m.as_posix() in visited:
            continue
        visited.add(m.as_posix())
        for key, entry in manifest_deps(tomllib.loads(read(m))):
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


def problems(texts, read=read_manifest):
    codes = {path: code_only(text) for path, text in texts.items()}
    out = []
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


if __name__ == "__main__":
    unittest.main()
