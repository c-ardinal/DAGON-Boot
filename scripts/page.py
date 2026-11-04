"""Builds the browser page into crates/dagon-cli/page/, where the dagon
executable embeds it (so `dagon serve` needs no Node or wasm-pack, and
`cargo install` from git gets the page).

The page is key-agnostic: `dagon serve` hands it the developer's release
key at start. It is built with an all-zero placeholder key, which verifies
nothing, so a page that does not take the served key refuses every release
rather than trusting a built-in one.

    python scripts/page.py build   # npm ci (if needed), npm run build, copy
    python scripts/page.py check   # exit 1 when the page is older than its sources

page/SOURCE holds the SHA-256 over the page's sources (web/ and the Rust
crates compiled into its WebAssembly); `check` compares it, without Node.
"""
import hashlib
import os
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEB = ROOT / "web"
PAGE = ROOT / "crates" / "dagon-cli" / "page"
SOURCE = PAGE / "SOURCE"
# Crates compiled into the page's WebAssembly (dagon-wasm and what it uses).
CRATES = ["dagon-wasm", "dagon-host", "dagon-format", "dagon-proto", "dagon-topology",
          "dagon-sign", "dagon-romlink", "dagon-conform", "dagon-port", "dagon-core",
          "dagon-agent"]
WEB_SKIP = {"node_modules", "dist", "pkg"}
# The definition files dagon-topology embeds (its build.rs discovers them; a
# test keeps this list equal to what it embeds).
EMBEDDED = ROOT / "crates" / "dagon-topology" / "builtin-files.txt"
PLACEHOLDER_KEY = "0" * 64


def source_files():
    files = []
    for p in sorted(WEB.rglob("*")):
        rel = p.relative_to(WEB).parts
        if p.is_file() and not WEB_SKIP.intersection(rel):
            files.append(p)
    for c in CRATES:
        files += sorted(p for p in (ROOT / "crates" / c).rglob("*")
                        if p.is_file() and "target" not in p.relative_to(ROOT).parts)
    files += embedded_files()
    files.append(ROOT / "Cargo.lock")
    return files


def embedded_files():
    """The targets/ files the page's WebAssembly embeds."""
    lines = EMBEDDED.read_text(encoding="utf-8").splitlines()
    return [ROOT / line for line in lines if line.strip()]


def source_hash():
    h = hashlib.sha256()
    for p in source_files():
        h.update(p.relative_to(ROOT).as_posix().encode())
        h.update(b"\0")
        # Line endings do not change the page.
        h.update(p.read_bytes().replace(b"\r\n", b"\n"))
        h.update(b"\0")
    return h.hexdigest()


def build():
    npm = shutil.which("npm") or sys.exit("npm is needed to build the page")
    env = {k: v for k, v in os.environ.items() if k != "DAGON_ALLOW_TEST_KEY"}
    env["DAGON_PUBKEY"] = PLACEHOLDER_KEY
    if not (WEB / "node_modules").is_dir():
        subprocess.run([npm, "ci"], cwd=WEB, check=True, env=env)
    subprocess.run([npm, "run", "build"], cwd=WEB, check=True, env=env)
    dist = WEB / "dist"
    if not (dist / "index.html").is_file():
        sys.exit("npm run build produced no dist/index.html")
    if PAGE.exists():
        for child in PAGE.iterdir():
            if child.is_dir():
                shutil.rmtree(child)
            else:
                child.unlink()
    shutil.copytree(dist, PAGE, dirs_exist_ok=True)
    SOURCE.write_text(source_hash() + "\n")
    size = sum(p.stat().st_size for p in PAGE.rglob("*") if p.is_file())
    print(f"page: {size} bytes in {PAGE}")


def check():
    if not SOURCE.is_file():
        sys.exit("no built page; run: python scripts/page.py build")
    if SOURCE.read_text().strip() != source_hash():
        sys.exit("the built page is older than its sources; run: python scripts/page.py build")
    print("page is up to date")


if __name__ == "__main__":
    {"build": build, "check": check}.get(sys.argv[1] if len(sys.argv) > 1 else "",
                                         lambda: sys.exit(__doc__))()
