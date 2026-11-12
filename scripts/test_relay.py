"""The relay's link contract and the relay crate have one home each.

- Exactly one `trait NodeLink`, in dagon-port.
- One production definition each of the frame helpers `hello_request` and
  `hello_answer`, in dagon-port; the host's and the browser's HELLO body
  builders, which share the name, are listed with their reason.
- dagon-port does not depend, directly or through its path dependencies,
  on dagon-agent, dagon-events or dagon-mcu-common.
- dagon-relay's normal dependencies are among RELAY_ALLOWED, and nothing
  they reach through path dependencies is dagon-agent, dagon-events,
  embassy-time, dagon-sim or an MCU or board package (the relay reaches
  the node's agent through its own trait); dagon-sim and dagon-wasm do not
  depend on it.
- dagon-relay names no link id or topology interface of a particular MCU
  (`link::I2C0`, `link::PIO_UART`, `iface::I2C_BUS`, `iface::PIO_UART`):
  the MCU package passes its links in.
- No Rust file names the paths the relay crate replaced:
  `dagon_mcu_rp2040::{nodes, view, cmd}`, the node items once under
  `dagon_mcu_rp2040::relay` (`named`, the node type), or those modules
  inside the RP2040 MCU package, which re-exports nothing of dagon-relay.
  No Rust file names the replaced node names (`SubNode`, `Subs`,
  `MAX_SUBS`) or brings an item in under a sub, parent or child name.
- The relay logic lives in dagon-relay only: it declares the serving
  loop, the link worker, the request port, the configuration and the
  state there; dagon-relay holds no `static` (its state is an instance its
  caller owns) and no struct with a field per link kind (`i2c`, `uart`):
  its links are a list. No production file under `targets/` declares the
  relay logic items the RP2040 package once held (`Pc`, `Port`, `Req`,
  `Resp`, `Op`, `RelayConfig`, the state's statics and functions), the
  RP2040 package keeps its relay glue in one file (`relay.rs`), and no Rust
  file names the old paths (`dagon_mcu_rp2040::relay::{state, pc, ports,
  task, Pc, Port, RelayConfig}`, those modules inside the RP2040 package).
- Neither dagon-relay nor the RP2040 relay glue assumes where the node sits
  in the topology: no `role::HUB`, no item or field named hub, no "hub"
  name, no node index 0 written as this node's (`named(0, ..)`, `index: 0`,
  `index: Some(0)`); the caller injects the node's index and role.
- The node's index is what identifies it: neither dagon-relay nor the
  RP2040 relay glue finds the node by comparing an image tag (several nodes
  may run images with one tag) or takes the index as optional, and each
  template that starts the relay passes the index `dagon gen` writes
  (`wiring::NODE.index`).

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
    "dagon-port", "dagon-proto", "dagon-format",
    "heapless", "embassy-sync", "embassy-futures",
}
# dagon-agent reaches dagon-events, whose crates each register their state
# machines with dagon-catalog; the relay has none.
RELAY_FORBIDDEN = {"dagon-agent", "dagon-events", "embassy-time", "dagon-sim"}
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

# The relay logic dagon-relay declares: (kind, name).
RELAY_LOGIC = (
    ("fn", "serve"), ("fn", "run_link"), ("struct", "Port"),
    ("struct", "State"), ("struct", "LinkRoute"), ("struct", "Config"),
)
# Relay logic items the RP2040 package held; no production file under
# targets/ declares them again.
OLD_LOGIC = re.compile(
    r"\b(?:(?:struct|enum|type)\s+(?:Pc|Port|Req|Resp|Op|RelayConfig|LinkRoute)"
    r"|fn\s+(?:run_link|relayed_seen|pc_quiet|pc_active|expire_progress|set_seen_at|set_seen)"
    r"|static\s+(?:REDRAW|QUIET_UNTIL|I2C_PORT|UART_PORT|NEXT_ID))\b"
)
# Files under targets/ declaring a name OLD_LOGIC matches: path -> why it
# is not the relay's.
OLD_LOGIC_OTHERS = {
    "targets/mcu/esp32s3/src/tests_factory.rs": "the ESP32-S3 host test's PC (a `#[cfg(test)]` module of its lib.rs), not the relay's",
}
OLD_LOGIC_ITEMS = r"(?:state|pc|ports|task|Pc|Port|RelayConfig)"
OLD_LOGIC_PATHS = (
    re.compile(rf"\bdagon_mcu_rp2040\s*::\s*relay\s*::\s*{OLD_LOGIC_ITEMS}\b"),
    re.compile(rf"\bdagon_mcu_rp2040\s*::\s*relay\s*::\s*\{{[^}}]*\b{OLD_LOGIC_ITEMS}\b"),
)
OLD_LOGIC_INSIDE = (
    re.compile(r"\b(?:crate|super)\s*::\s*(?:relay\s*::\s*)?(?:state|pc|ports|task)\s*::"),
    re.compile(r"\bmod\s+(?:state|pc|ports|task)\b"),
)
RP_GLUE = f"{RP_PACKAGE}relay.rs"
# A node's place in the topology written into the relay: a hub role, item,
# field or name, or node index 0 as this node's.
PLACE_IN_CODE = re.compile(
    r"\brole\s*::\s*HUB\b|\b(?:const|static|fn)\s+HUB\b|\bhub\s*:|\bnamed\s*\(\s*0\s*,"
    r"|\bindex\s*:\s*(?:Some\s*\(\s*)?0\b"
)
PLACE_IN_TEXT = re.compile(r'"hub"')
# The node identified by an image tag, or an identity whose index may be
# missing (and so is looked up by tag).
TAG_IDENTITY = re.compile(
    r"\b\w*tag\s*[!=]=|[!=]=\s*[&*]*[\w.:()]*?tag\b|\bindex\s*:\s*Option\s*<"
)
TEMPLATE_MAIN = re.compile(r"^targets/.+/template/src/main\.rs$")
RELAY_SETUP = re.compile(r"\bSetup\s*\{")
GENERATED_INDEX = re.compile(r"\bindex\s*:\s*wiring\s*::\s*NODE\s*\.\s*index\b")
RELAY_STATIC = re.compile(r"\bstatic\s+(?:mut\s+)?\w+\s*:")
STRUCT_BODY = re.compile(r"\bstruct\s+(\w+)[^;{]*\{")
LINK_KIND_FIELD = re.compile(r"(?:^|[{,])\s*(?:pub(?:\([^)]*\))?\s+)?(\w*(?:i2c|uart)\w*)\s*:", re.IGNORECASE)

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


def production(code):
    """`code` with its `#[cfg(test)]` modules blanked."""
    for a, b in reversed(cfg_test_spans(code)):
        code = code[:a] + " " * (b - a) + code[b:]
    return code


def struct_bodies(code):
    """(name, body) of every braced struct in `code`."""
    for m in STRUCT_BODY.finditer(code):
        depth, i = 1, m.end()
        while i < len(code) and depth:
            depth += {"{": 1, "}": -1}.get(code[i], 0)
            i += 1
        yield m.group(1), code[m.end():i - 1]


def relay_logic_problems(codes, texts):
    out = []
    relay = {p: production(c) for p, c in codes.items() if p.startswith(RELAY_SRC) and not is_test_path(p)}
    for kind, name in RELAY_LOGIC:
        if not any(re.search(rf"\b{kind}\s+{name}\b", c) for c in relay.values()):
            out.append(f"dagon-relay declares no {kind} {name}")
    for path, code in sorted(relay.items()):
        if m := RELAY_STATIC.search(code):
            out.append(f"{path} holds a static ({' '.join(m.group(0).split())}): the caller owns the relay's state")
        for name, body in struct_bodies(code):
            for f in LINK_KIND_FIELD.finditer(body):
                out.append(f"{path}: struct {name} has a field {f.group(1)} for one link kind: the links are a list")
    for path, code in sorted({**relay, RP_GLUE: production(codes.get(RP_GLUE, ""))}.items()):
        if m := PLACE_IN_CODE.search(code):
            out.append(f"{path} assumes the node's place ({' '.join(m.group(0).split())}): the caller injects it")
        if m := TAG_IDENTITY.search(code):
            out.append(f"{path} identifies the node by its tag ({' '.join(m.group(0).split())}): the index does")
    for path, code in sorted(codes.items()):
        if TEMPLATE_MAIN.match(path) and RELAY_SETUP.search(code) and not GENERATED_INDEX.search(code):
            out.append(f"{path} starts the relay without the index dagon gen writes (wiring::NODE.index)")
    for path, text in sorted(texts.items()):
        if (path.startswith(RELAY_SRC) and not is_test_path(path) or path == RP_GLUE) and PLACE_IN_TEXT.search(production(text)):
            out.append(f"{path} names this node \"hub\": the caller injects its name")
    for path, code in sorted(codes.items()):
        if path.startswith(f"{RP_PACKAGE}relay/"):
            out.append(f"{path}: the RP2040 package keeps only the relay's glue, in relay.rs")
        if path.startswith("targets/") and not is_test_path(path) and path not in OLD_LOGIC_OTHERS:
            if m := OLD_LOGIC.search(production(code)):
                out.append(f"{path} declares the relay logic item {' '.join(m.group(0).split())}")
        rules = OLD_LOGIC_PATHS + (OLD_LOGIC_INSIDE if path.startswith(RP_PACKAGE) else ())
        for rule in rules:
            if m := rule.search(code):
                out.append(f"{path} names the old relay path {' '.join(m.group(0).split())}")
    return out


def problems(texts, read=read_manifest):
    codes = {path: code_of(text) for path, text in texts.items()}
    out = relay_dependency_problems(read) + relay_source_problems(codes) + relay_logic_problems(codes, texts)
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
            'dagon-agent = { path = "../dagon-agent" }',
            'portable-atomic = "1"',
            'clock = { package = "embassy-time", version = "0.5" }',
            'dagon-mcu-rp2040 = { path = "../../targets/mcu/rp2040" }',
        ):
            for header in ("[dependencies]", "[target.'cfg(target_os = \"none\")'.dependencies]"):
                with self.subTest(entry=entry, header=header):
                    self.assertTrue(self.with_entries({RELAY_MANIFEST: (header, entry)}))

    def test_a_forbidden_package_through_a_path_dependency_fails(self):
        for entry in ('embassy-time = "0.5"', 'dagon-board-x = "0.1"', 'dagon-agent = { path = "../dagon-agent" }'):
            with self.subTest(entry=entry):
                self.assertTrue(self.with_entries({PORT_MANIFEST: ("[dependencies]", entry)}))

    def test_the_simulator_as_a_dev_dependency_passes(self):
        # Under a second name: the relay's host test already names dagon-sim.
        planted = {RELAY_MANIFEST: ("[dev-dependencies]", 'sim = { package = "dagon-sim", path = "../dagon-sim" }')}
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


class RelayLogic(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.texts = rust_texts()

    def planted(self, path, extra):
        return problems({**self.texts, path: self.texts.get(path, "") + "\n" + extra})

    def test_a_static_in_the_relay_fails(self):
        for code in (
            "static STATE: State = State::new();\n",
            "pub static mut SEEN: u8 = 0;\n",
            "pub(crate) static PORTS: [Port; 2] = [Port::new(), Port::new()];\n",
        ):
            with self.subTest(code=code):
                self.assertTrue(self.planted(f"{RELAY_SRC}state.rs", code))

    def test_a_static_in_relay_test_code_or_a_const_passes(self):
        for path, code in (
            (f"{RELAY_SRC}state.rs", "#[cfg(test)]\nmod tests {\n    static N: u8 = 0;\n}\n"),
            (f"{RELAY_SRC}state.rs", "const N: usize = 3;\n// static X: u8 = 0;\n"),
            ("crates/dagon-relay/tests/planted.rs", "static N: u8 = 0;\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertEqual(self.planted(path, code), [])

    def test_a_field_per_link_kind_fails(self):
        for code in (
            "pub struct Links<A, B> { pub i2c: A, pub uart: B }\n",
            "struct Relay {\n    has_i2c: bool,\n}\n",
            "pub struct R<'a> { port: u8, pub(crate) Uart_port: &'a u8 }\n",
        ):
            with self.subTest(code=code):
                self.assertTrue(self.planted(f"{RELAY_SRC}lib.rs", code))

    def test_a_missing_relay_logic_item_fails(self):
        for kind, name in RELAY_LOGIC:
            with self.subTest(name=name):
                texts = {
                    p: re.sub(rf"\b{kind}\s+{name}\b", f"{kind} Renamed{name}", t) if p.startswith(RELAY_SRC) else t
                    for p, t in self.texts.items()
                }
                self.assertTrue(problems(texts))

    def test_relay_logic_declared_under_targets_fails(self):
        for path, code in (
            (f"{RP_PACKAGE}relay.rs", "pub struct Pc { x: u8 }\n"),
            (f"{RP_PACKAGE}relay.rs", "pub enum Op { Relay }\n"),
            (f"{RP_PACKAGE}relay.rs", "pub struct RelayConfig;\n"),
            ("targets/boards/x/src/hal.rs", "pub static REDRAW: Signal<Cs, ()> = Signal::new();\n"),
            ("targets/mcu/common/src/planted.rs", "pub async fn run_link() {}\n"),
            ("targets/rp2040-hub/src/main.rs", "fn pc_quiet() -> bool { true }\n"),
            (f"{RP_PACKAGE}relay/state.rs", "const X: u8 = 0;\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertTrue(self.planted(path, code))

    def test_an_old_relay_logic_path_fails(self):
        for path, code in (
            ("targets/boards/x/src/display.rs", "use dagon_mcu_rp2040::relay::state;\n"),
            ("targets/boards/x/src/hal.rs", "fn f(c: dagon_mcu_rp2040::relay::RelayConfig) {}\n"),
            ("targets/boards/x/src/hal.rs", "use dagon_mcu_rp2040::relay::{self, task};\n"),
            (f"{RP_PACKAGE}relay.rs", "mod ports;\n"),
            (f"{RP_PACKAGE}relay.rs", "pub mod state;\n"),
            (f"{RP_PACKAGE}relay.rs", "fn f() { super::state::home(); }\n"),
            (f"{RP_PACKAGE}node.rs", "use crate::relay::pc::Pc;\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertTrue(self.planted(path, code))

    def test_an_assumed_place_in_the_topology_fails(self):
        for path, code in (
            (f"{RELAY_SRC}pc.rs", "const R: u8 = role::HUB;\n"),
            (f"{RELAY_SRC}nodes.rs", "pub const HUB: LinkedNode = named(1, \"x\", 0, 0);\n"),
            (f"{RELAY_SRC}view.rs", "pub struct H { pub hub: Node }\n"),
            (f"{RELAY_SRC}state.rs", "const OWN: LinkedNode = named(0, \"x\", 0, 0);\n"),
            (f"{RELAY_SRC}state.rs", "fn own() -> LinkedNode { LinkedNode { index: 0, ..Default::default() } }\n"),
            (f"{RELAY_SRC}nodes.rs", "const NAME: &str = \"hub\";\n"),
            (RP_GLUE, "fn r() -> u8 { role::HUB }\n"),
            (RP_GLUE, "const ME: Identity = Identity { index: Some(0), tag: \"\" };\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertTrue(self.planted(path, code))

    def test_an_identity_by_tag_fails(self):
        for path, code in (
            (f"{RELAY_SRC}nodes.rs", "fn f(t: &TopologyView, tag: [u8; 16]) -> Option<usize> {\n    (0..t.node_count()).find(|&i| t.node(i).target_tag == tag)\n}\n"),
            (f"{RELAY_SRC}nodes.rs", "fn f(n: &TopoNode, me: Identity) -> bool { target_tag(me.tag) == n.target_tag }\n"),
            (f"{RELAY_SRC}pc.rs", "fn f(a: &str, b: &str) -> bool { a != b.tag }\n"),
            (f"{RELAY_SRC}nodes.rs", "pub struct Me { pub index: Option<u8> }\n"),
            (RP_GLUE, "pub struct S { pub index: Option<u8> }\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertTrue(self.planted(path, code))

    def test_a_template_without_the_generated_index_fails(self):
        for path in ("targets/mcu/rp2040/template/src/main.rs", "targets/boards/waveshare-rp2040-touch-lcd-1.28/template/src/main.rs"):
            text = self.texts[path]
            self.assertIn("wiring::NODE.index", text)
            for index in ("0", "1"):
                with self.subTest(path=path, index=index):
                    texts = {**self.texts, path: text.replace("wiring::NODE.index", index)}
                    self.assertTrue(problems(texts))

    def test_a_tag_as_a_name_or_in_test_code_passes(self):
        for path, code in (
            (f"{RELAY_SRC}nodes.rs", "fn f(me: Identity) -> [u8; 16] { short_name(&target_tag(me.tag)) }\n"),
            (f"{RELAY_SRC}nodes.rs", "#[cfg(test)]\nmod tests {\n    fn f(a: u8, tag: u8) -> bool { a == tag }\n}\n"),
            ("crates/dagon-relay/tests/planted.rs", "fn f(a: u8, tag: u8) -> bool { a == tag }\n"),
            ("targets/rp2040-hub/src/main.rs", "struct S { index: Option<u8> }\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertEqual(self.planted(path, code), [])

    def test_a_place_in_relay_tests_or_an_injected_role_passes(self):
        for path, code in (
            (f"{RELAY_SRC}nodes.rs", "#[cfg(test)]\nmod tests {\n    const A: u8 = role::HUB;\n    fn f() { named(0, \"hub\", 0, 0); }\n}\n"),
            ("crates/dagon-relay/tests/planted.rs", "const R: u8 = role::HUB;\n"),
            ("targets/rp2040-hub/src/main.rs", "const R: u8 = relay::role::HUB;\n"),
            (RP_GLUE, "fn r(s: Setup) -> u8 { s.role }\n// role::HUB\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertEqual(self.planted(path, code), [])

    def test_the_glue_and_test_code_pass(self):
        for path, code in (
            (f"{RP_PACKAGE}relay.rs", "use dagon_relay::{serve, run_link, Port, State};\npub const TASKS: usize = 3;\n"),
            ("targets/boards/x/src/hal.rs", "use dagon_mcu_rp2040::relay::{self, Links, Setup};\nfn f() { relay::levels::<4>(); }\n"),
            ("targets/mcu/common/tests/planted.rs", "struct Port;\nenum Op { A }\n"),
            ("targets/mcu/common/src/planted.rs", "#[cfg(test)]\nmod tests {\n    struct Pc;\n}\n// struct Port\n"),
        ):
            with self.subTest(path=path, code=code):
                self.assertEqual(self.planted(path, code), [])


if __name__ == "__main__":
    unittest.main()
