"""Writes the site's reference pages from their single sources.

    python site/scripts/gen.py           # write the pages
    python site/scripts/gen.py --check   # exit 1 when a page differs from its source

Each page says which source it comes from; nobody edits a generated page.
The sources are read as text (Rust and TOML) with Python 3.11+; the command
line page runs `dagon help-markdown` (DAGON_BIN, else `cargo run`).
"""
import functools
import os
import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "site" / "src" / "content" / "docs" / "reference"
REGEN = "python site/scripts/gen.py"


def page(title, description, sources, body):
    """A generated page: frontmatter, the note naming its sources, then the body."""
    names = ", ".join(f"`{s}`" for s in sources)
    return (
        f"---\ntitle: {title}\ndescription: {description}\neditUrl: false\n---\n\n"
        f":::note\nGenerated from {names}; do not edit. Regenerate with `{REGEN}`.\n:::\n\n"
        f"{body.strip()}\n"
    )


def cell(text):
    """A Markdown table cell: one line, pipes escaped, rustdoc links as plain code, angle brackets outside code as text."""
    text = re.sub(r"\[(`[^`]+`)\]", r"\1", " ".join(str(text).split())).replace("|", "\\|")
    parts = text.split("`")
    return "`".join(p if i % 2 else p.replace("<", "&lt;").replace(">", "&gt;") for i, p in enumerate(parts))


def doc_before(lines, i):
    """The `///` comment directly above line i, as one paragraph."""
    out = []
    j = i - 1
    # Attributes and plain `//` notes may sit between the doc comment and the item.
    while j >= 0 and lines[j].strip().startswith(("#[", "//")):
        s = lines[j].strip()
        if s.startswith("///"):
            out.insert(0, s[3:].strip())
        j -= 1
    return " ".join(out).strip()


# ---------------------------------------------------------------- serve API

SERVE = ROOT / "crates" / "dagon-cli" / "src" / "serve"
ARM = re.compile(r'^\s*\("(GET|POST|PUT|DELETE)",\s*\[([^\]]*)\]\)\s*=>\s*(.+?),?\s*$')
CALL = re.compile(r"((?:super::\w+::)?\w+)\(")
SKIP_CALLS = {"Ok", "Err", "start", "Response::error"}


def fn_doc(name):
    """The doc comment of serve function `name` (`super::module::f` looks in module.rs)."""
    module, _, fn = name.rpartition("::")
    files = [SERVE / f"{module.split('::')[-1]}.rs"] if module else [SERVE / "api.rs", SERVE / "mod.rs"]
    pat = re.compile(rf"^\s*(pub(\([a-z]+\))? )?fn {re.escape(fn)}\(")
    for f in files:
        lines = f.read_text(encoding="utf-8").splitlines()
        for i, line in enumerate(lines):
            if pat.match(line):
                return doc_before(lines, i)
    return ""


def handler(rhs):
    """The function that answers an arm: a job's runner for `start(...)`, else the first call."""
    if rhs.startswith("start("):
        return rhs.rstrip(")").rsplit(",", 1)[-1].strip()
    calls = [c for c in CALL.findall(rhs) if c not in SKIP_CALLS]
    return calls[0] if calls else ""


def route_path(parts):
    segs = []
    for p in (x.strip() for x in parts.split(",") if x.strip()):
        segs.append(p.strip('"') if p.startswith('"') else "{" + p + "}")
    return "/v1/" + "/".join(segs)


def serve_api():
    api = (SERVE / "api.rs").read_text(encoding="utf-8")
    refused = re.search(r"const DEMO_REFUSED: \[&str; \d+\] = \[(.*?)\];", api, re.S)
    refused = re.findall(r'"([^"]+)"', refused.group(1)) if refused else []
    rows = []
    router = api.split("pub fn handle(", 1)[1].split("\n    };", 1)[0].splitlines()
    for i, line in enumerate(router):
        m = ARM.match(line)
        if not m:
            continue
        method, parts, rhs = m.groups()
        path = route_path(parts)
        # An arm answered in place carries its note as a `//` comment above it.
        above = router[i - 1].strip() if i else ""
        doc = fn_doc(handler(rhs)) or (above[2:].strip() if above.startswith("//") else "")
        # A doc that starts with its own route ("POST /v1/sim/write: ...") keeps only the rest.
        doc = re.sub(r"^(GET|POST|PUT|DELETE) /v1/\S*( \([^)]*\)| \{[^}]*\})?:\s*", "", doc)
        doc = doc[:1].upper() + doc[1:]
        short = path[len("/v1/"):]
        demo = any(short == r or short.startswith(r + "/") for r in refused)
        rows.append(f"| `{method}` | `{path}` | {'refused' if demo else ''} | {cell(doc)} |")
    events = re.search(r'path == "(/v1/events)"', (SERVE / "mod.rs").read_text(encoding="utf-8"))
    body = f"""
Every route is under `dagon serve`, on the loopback address it prints. Every
`/v1/` request needs the passphrase from the address serve printed, as
`Authorization: Bearer <passphrase>` (401 without it), and passes serve's
Host and Origin checks (403 otherwise).

| Method | Path | `dagon demo` | What it does |
|---|---|---|---|
{chr(10).join(rows)}

`{events.group(1) if events else "/v1/events"}` is a WebSocket: the page sends the passphrase as its
first message, then receives serve's events (job progress, logs, results,
questions and the hub hand-over).

A route marked *refused* answers 403 while `dagon demo` runs: the demo has
its own simulator and project and never touches real hardware, other
projects or an agent.
"""
    return page("Serve HTTP API", "Every route of dagon serve, from its router.",
                ["crates/dagon-cli/src/serve/api.rs"], body)


# ---------------------------------------------------------------- agents

AGENTS = ROOT / "crates" / "dagon-cli" / "src" / "agents"
FIELD = re.compile(r"^\s*(?:pub )?(\w+): (.+),\s*$")


def rust_struct(path, name):
    """(field, type, optional, doc) of `struct name` in path (public or not)."""
    lines = path.read_text(encoding="utf-8").splitlines()
    start = next(i for i, l in enumerate(lines) if re.match(rf"\s*(?:pub )?struct {name}\b", l))
    out = []
    for i in range(start + 1, len(lines)):
        if lines[i].strip() == "}":
            break
        m = FIELD.match(lines[i])
        if not m:
            continue
        attrs = []
        j = i - 1
        while j > start and lines[j].strip().startswith(("#[", "///")):
            attrs.append(lines[j])
            j -= 1
        if any(re.search(r"serde\(.*\bskip\b", a) for a in attrs):
            continue
        optional = any("serde(default" in a for a in attrs) or m.group(2).startswith("Option<")
        # The key the file uses: serde's rename, else the field's name.
        renamed = next((r.group(1) for a in attrs if (r := re.search(r'rename = "([^"]+)"', a))), m.group(1))
        out.append((renamed, m.group(2), optional, doc_before(lines, i)))
    return out


def rust_enum_values(path, name):
    """kebab-case names and docs of the unit variants of `pub enum name`."""
    lines = path.read_text(encoding="utf-8").splitlines()
    start = next(i for i, l in enumerate(lines) if re.match(rf"\s*pub enum {name}\b", l))
    out = []
    for i in range(start + 1, len(lines)):
        if lines[i].strip() == "}":
            break
        m = re.match(r"^\s*(\w+),\s*$", lines[i])
        if m:
            kebab = re.sub(r"(?<!^)([A-Z])", r"-\1", m.group(1)).lower()
            out.append((kebab, doc_before(lines, i)))
    return out


TYPES = {
    "String": "string",
    "u32": "integer",
    "Vec<String>": "array of strings",
    "Vec<WayDef>": "`[[ways]]` tables",
    "Transport": "`[ways.transport]` table",
    "SystemVia": "string",
    "bool": "boolean",
    "Capabilities": "`[capabilities]` table",
    "Limits": "`[limits]` table",
}


def type_word(t):
    inner = re.fullmatch(r"Option<(.+)>", t)
    return TYPES.get(inner.group(1) if inner else t, f"`{t}`")


def field_table(fields, skip=()):
    rows = [
        f"| `{n}` | {type_word(t)} | {'no' if opt else 'yes'} | {cell(d)} |"
        for n, t, opt, d in fields if n not in skip
    ]
    return "| Key | Type | Required | Meaning |\n|---|---|---|---|\n" + "\n".join(rows)


def agent_definition():
    mod = AGENTS / "mod.rs"
    src = mod.read_text(encoding="utf-8")
    fmt = re.search(r"pub const FORMAT: u32 = (\d+);", src).group(1)
    kinds = re.findall(r'"([a-z-]+)"', re.search(r"pub const KINDS: \[&str; \d+\] = \[(.*?)\];", src).group(1))
    agent = rust_struct(mod, "AgentDef")
    way = rust_struct(mod, "WayDef")
    command = rust_struct(AGENTS / "command.rs", "Command")
    http = rust_struct(AGENTS / "http.rs", "MessagesHttp")
    via = rust_enum_values(AGENTS / "command.rs", "SystemVia")
    builtin = []
    for f in sorted((ROOT / "crates" / "dagon-cli" / "agents").glob("*.toml")):
        a = tomllib.loads(f.read_text(encoding="utf-8"))
        ways = ", ".join(f"`{w['id']}` ({w['transport']['kind']})" for w in a["ways"])
        builtin.append(f"| `{a['id']}` | {cell(a['name'])} | {cell(a['provider'])} | {ways} |")
    body = f"""
An agent is a TOML file: the builtin ones ship with `dagon`, and you add your
own in `~/.dagon/agents/*.toml` (never in a project). Unknown keys, a kind
DAGON does not implement or an id that is taken make the file not load.

Definition format: **{fmt}** (`dagon_agent = {fmt}`). A dagon that reads another
number refuses the file and says why.

## Agent

{field_table(agent)}

## `[[ways]]`

{field_table(way)}

## `[ways.transport]`

`kind` is one of: {", ".join(f"`{k}`" for k in kinds)}.

### `kind = "command"`

{field_table(command)}

`system` is one of: {", ".join(f"`{v}` ({cell(d)})" for v, d in via)}.

### `kind = "messages-http"`

{field_table(http)}

## Builtin agents

| Id | Name | Provider | Ways |
|---|---|---|---|
{chr(10).join(builtin)}
"""
    return page("Agent definition format", "The keys of an agent definition file, from the types dagon reads it into.",
                ["crates/dagon-cli/src/agents/", "crates/dagon-cli/agents/"], body)


# ---------------------------------------------------------------- boards and MCUs

def size(n):
    return f"{n // (1024 * 1024)} MiB" if n % (1024 * 1024) == 0 else f"{n // 1024} KiB"


def boards():
    mcus, rows_m, rows_b = {}, [], []
    for f in sorted((ROOT / "targets" / "mcu").glob("*/dagon-mcu.toml")):
        m = tomllib.loads(f.read_text(encoding="utf-8"))
        mcus[m["id"]] = m["name"]
        flash = ", ".join(size(x["bytes"]) for x in m.get("flash_map", []))
        links = m.get("links", {})
        hub = "yes" if "coordinator" in m else ""
        rows_m.append(
            f"| `{m['id']}` | {cell(m['name'])} | `{m['port']}` | {flash} | {size(m['ram']['size']) if 'ram' in m else ''} "
            f"| {', '.join(links.get('up', []))} | {', '.join(links.get('down', []))} | {hub} "
            f"| `{m.get('writer', {}).get('kind', '')}` |"
        )
    for f in sorted((ROOT / "targets" / "boards").glob("*/dagon-board.toml")):
        b = tomllib.loads(f.read_text(encoding="utf-8"))
        link = b.get("default_link", {}).get("kind", "—")
        rows_b.append(
            f"| `{b['id']}` | {cell(b['product'])} | {mcus.get(b['mcu'], b['mcu'])} (`{b['mcu']}`) "
            f"| {size(b['flash_bytes'])} | {link} |"
        )
    body = f"""
Every MCU package in `targets/mcu/` and board package in `targets/boards/`
of this release. Adding a package folder adds a row; the format of both
files is in [MCU and board definitions](/reference/definitions/).

## MCUs

| Id | Name | Port | Flash | RAM | Up links | Down links | Can be 親機 | First write |
|---|---|---|---|---|---|---|---|---|
{chr(10).join(rows_m)}

## Boards

| Id | Product | MCU | Flash | Default link |
|---|---|---|---|---|
{chr(10).join(rows_b)}
"""
    return page("Boards and MCUs", "Every board and MCU this release defines, from targets/.",
                ["targets/mcu/*/dagon-mcu.toml", "targets/boards/*/dagon-board.toml"], body)


# ---------------------------------------------------------------- command line

@functools.cache
def cli_markdown():
    """`dagon help-markdown`: every command, from the clap definitions (DAGON_BIN, else built with cargo)."""
    cmd = [os.environ["DAGON_BIN"]] if os.environ.get("DAGON_BIN") else [
        "cargo", "run", "-q", "--locked", "-p", "dagon-cli", "--bin", "dagon", "--"]
    out = subprocess.run([*cmd, "help-markdown"], cwd=ROOT, capture_output=True, text=True, encoding="utf-8")
    if out.returncode != 0:
        sys.exit(f"dagon help-markdown failed:\n{out.stderr}")
    return out.stdout.replace("\r\n", "\n")


def cli():
    body = f"""
Every `dagon` command, its arguments and options, as `dagon <command> --help`
prints them. Most commands take their defaults from `./dagon.toml` (or
`--config <file>`).

{cli_markdown()}
"""
    return page("Command line (dagon)", "Every dagon command, from its clap definitions.",
                ["crates/dagon-cli/src/cli.rs"], body)


# ---------------------------------------------------------------- contracts and components

def contracts():
    path = ROOT / "compat.toml"
    text = path.read_text(encoding="utf-8")
    compat = tomllib.loads(text)
    # The comment beside each format number says where it is written; tomllib drops comments.
    section = re.search(r"^\[formats\]\n(.*?)^\[", text, re.M | re.S).group(1)
    notes = dict(re.findall(r"^([a-z0-9-]+)\s*=\s*\d+\s*#\s*(.*)$", section, re.M))
    rows_f = [f"| `{k}` | {v} | {cell(notes.get(k, ''))} |" for k, v in compat["formats"].items()]
    rows_c = []
    for c in compat.get("component", []):
        fmts = ", ".join(f"`{k}` {'/'.join(map(str, v))}" for k, v in c.get("formats", {}).items())
        pkgs = ", ".join(f"`{p}`" for p in c.get("packages", []))
        rows_c.append(f"| `{c['name']}` | {c['version']} | {pkgs} | {fmts} |")
    body = f"""
Every contract between DAGON's components and the number it is at, and
every component with its version and the format numbers it reads and
writes, as `compat.toml` holds them. The rules for changing them are in
[Versions and releases](/explanation/versioning/).

## Contracts

| Contract | Format | Where the number is written |
|---|---|---|
{chr(10).join(rows_f)}

## Components

| Component | Version | Packages | Formats |
|---|---|---|---|
{chr(10).join(rows_c)}
"""
    return page("Contracts and components", "Every contract's format number and every component's version, from compat.toml.",
                ["compat.toml"], body)


# ---------------------------------------------------------------- tools by MCU

def tools():
    rows = []
    for f in sorted((ROOT / "targets" / "mcu").glob("*/dagon-mcu.toml")):
        m = tomllib.loads(f.read_text(encoding="utf-8"))
        for d in m.get("doctor", []):
            how = f"`{' '.join(d['fix'])}`" if "fix" in d else ""
            note = d.get("hint", "") + (" (takes a while)" if d.get("slow") else "")
            rows.append(f"| {cell(m['name'])} | {cell(d['what'])} | {how} | {cell(note)} |")
    body = f"""
The tools each MCU needs besides Rust, as its MCU definition's `[[doctor]]`
entries name them. `dagon doctor` checks them in a project (every MCU
outside one), and `dagon doctor --fix` runs the install commands that need
no administrator rights.

| MCU | Tool | Install | Note |
|---|---|---|---|
{chr(10).join(rows)}
"""
    return page("Tools by MCU", "What dagon doctor checks for each MCU, from the MCU definitions.",
                ["targets/mcu/*/dagon-mcu.toml"], body)


# ---------------------------------------------------------------- Rust API

def crate_doc(src):
    """The first paragraph of a crate's `//!` documentation."""
    out = []
    lines = src.read_text(encoding="utf-8").splitlines() if src.is_file() else []
    first = next((i for i, line in enumerate(lines) if line.startswith("//!")), len(lines))
    for line in lines[first:]:
        if not line.startswith("//!"):
            break
        text = line[3:].strip()
        if not text and out:
            break
        out.append(text)
    return " ".join(out)


def rust_api():
    ws = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]
    rows = []
    for pattern in ws["members"]:
        for manifest in sorted(ROOT.glob(f"{pattern}/Cargo.toml")):
            cargo = tomllib.loads(manifest.read_text(encoding="utf-8"))
            pkg = cargo["package"]
            lib = manifest.parent / "src" / "lib.rs"
            # rustdoc names a crate's page after its library, else after its first binary.
            target = (pkg["name"] if lib.is_file() else cargo.get("bin", [{"name": pkg["name"]}])[0]["name"]).replace("-", "_")
            what = pkg.get("description") or crate_doc(lib if lib.is_file() else manifest.parent / "src" / "main.rs")
            rows.append(f"| [`{pkg['name']}`](/api/{target}/) | {cell(what)} |")
    body = f"""
The Rust API documentation (rustdoc) of every crate in the workspace is
published with these pages; each name below opens its crate.

| Crate | What it holds |
|---|---|
{chr(10).join(rows)}
"""
    return page("Rust API", "The workspace crates and their rustdoc.", ["Cargo.toml"], body)


PAGES = {
    "serve-api.md": serve_api,
    "agent-definition.md": agent_definition,
    "boards.md": boards,
    "tools.md": tools,
    "contracts.md": contracts,
    "cli.md": cli,
    "rust-api.md": rust_api,
}


# ---------------------------------------------------------------- regions of hand-written pages

EXTENSION = ROOT / "crates" / "dagon-extension" / "src" / "manifest.rs"


def extension_keys():
    """The key tables of dagon-extension.toml, from the types the manifest is read into."""
    return (f"{field_table(rust_struct(EXTENSION, 'Raw'))}\n\n"
            f"`[capabilities]`:\n\n{field_table(rust_struct(EXTENSION, 'Capabilities'))}\n\n"
            f"`[limits]`:\n\n{field_table(rust_struct(EXTENSION, 'Limits'))}\n")


DEFS = ROOT / "crates" / "dagon-topology" / "src" / "defs"


@functools.cache
def defs_source():
    return "\n".join(p.read_text(encoding="utf-8") for p in sorted(DEFS.glob("*.rs")))


def is_table(name):
    """Whether type `name` is written as a TOML table: a struct, or an enum tagged by a key."""
    src = defs_source()
    if re.search(rf"pub struct {name}\b", src):
        return True
    m = re.search(rf"pub enum {name}\b", src)
    return bool(m) and "tag =" in src[max(0, m.start() - 200):m.start()].splitlines()[-1]


def def_rows(path, struct, prefix=""):
    """Key rows of a definition file's table, from the struct it is read into."""
    rows = []
    for key, t, optional, doc in rust_struct(path, struct):
        m = re.fullmatch(r"(Option|Vec)<(.+)>", t)
        outer, inner = (m.group(1), m.group(2).split("::")[-1]) if m else ("", t.split("::")[-1])
        full = f"{prefix}.{key}" if prefix else key
        if is_table(inner):
            shown, kind = (f"`[[{full}]]`", "tables") if outer == "Vec" else (f"`[{full}]`", "table")
        else:
            shown = f"`{key}`"
            kind = {"String": "string", "bool": "boolean"}.get(inner, "integer" if re.fullmatch(r"[ui]\d+", inner) else "string")
            kind = f"array of {kind}s" if outer == "Vec" else kind
        rows.append(f"| {shown} | {kind} | {'no' if optional else 'yes'} | {cell(doc)} |")
    return "| Key | Type | Required | Meaning |\n|---|---|---|---|\n" + "\n".join(rows) + "\n"


REGION_BEGIN ="<!-- generated from {}: python site/scripts/gen.py -->\n"
REGION_END = "<!-- end of generated table -->\n"
# Hand-written page -> (what its region is generated from, the region's text).
# (hand-written page, what the region is generated from, the region's text); a page may hold several.
REGIONS = [
    ("extension-manifest.md", "dagon-extension's manifest types", extension_keys),
    ("definitions.md", "dagon-topology's McuDef", lambda: def_rows(DEFS / "mod.rs", "McuDef")),
    ("definitions.md", "dagon-topology's Isolation", lambda: def_rows(DEFS / "isolation.rs", "Isolation", "isolation")),
    ("definitions.md", "dagon-topology's BoardDef", lambda: def_rows(DEFS / "mod.rs", "BoardDef")),
]


def with_region(text, source, body):
    begin = REGION_BEGIN.format(source)
    a = text.index(begin) + len(begin)
    b = text.index(REGION_END, a)
    return text[:a] + body + text[b:]


def main(argv):
    check = "--check" in argv
    stale = []
    for name in dict.fromkeys(n for n, _, _ in REGIONS):
        path = OUT / name
        old = path.read_text(encoding="utf-8")
        new = old
        for _, source, make in (r for r in REGIONS if r[0] == name):
            new = with_region(new, source, make())
        if check and new != old:
            stale.append(path.as_posix())
        elif not check:
            path.write_text(new, encoding="utf-8", newline="\n")
    for name, make in PAGES.items():
        text = make()
        path = OUT / name
        if check:
            if not path.is_file() or path.read_text(encoding="utf-8") != text:
                stale.append(path.as_posix())
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8", newline="\n")
    if stale:
        print("generated pages differ from their sources; run: " + REGEN)
        print("\n".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
