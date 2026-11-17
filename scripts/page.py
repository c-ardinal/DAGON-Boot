"""Builds the browser page into crates/dagon-cli/page/, where the dagon
executable embeds it (so `dagon serve` needs no Node or wasm-pack). The page
is build output and never committed: release builds (scripts/release.py) and
CI run this first; from a checkout, run it before building dagon.

The page is key-agnostic: `dagon serve` hands it the developer's release
key at start. It is built with an all-zero placeholder key, which verifies
nothing, so a page that does not take the served key refuses every release
rather than trusting a built-in one.

    python scripts/page.py build   # npm ci (if needed), npm run build, copy

It needs Node.js and npm, wasm-pack and the wasm32-unknown-unknown target.
"""
import os
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
WEB = ROOT / "web"
PAGE = ROOT / "crates" / "dagon-cli" / "page"
PLACEHOLDER_KEY = "0" * 64


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
        shutil.rmtree(PAGE)
    shutil.copytree(dist, PAGE)
    size = sum(p.stat().st_size for p in PAGE.rglob("*") if p.is_file())
    print(f"page: {size} bytes in {PAGE}")


if __name__ == "__main__":
    {"build": build}.get(sys.argv[1] if len(sys.argv) > 1 else "",
                         lambda: sys.exit(__doc__))()
