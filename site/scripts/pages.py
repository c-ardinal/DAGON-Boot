"""Checks what the two built Console pages carry.

    python site/scripts/pages.py                 # the page dagon serve hands out (crates/dagon-cli/page)
    python site/scripts/pages.py --demo <dir>    # also the public demo page built into <dir>

The served page carries none of the demo: no repository fixture (the test-key
release, the boot core) and no stand-in hub. Neither page carries the
development stand-in for the agent routes, so neither names an agent product
(the pages show DAGON; the served page learns the agents from serve). Exit 1
names what was found.

The public test key itself is not looked for: dagon-sign holds it to mark
images signed with it, so its bytes are in every build of the WebAssembly.
The served page's own release key is the placeholder scripts/page.py builds
it with.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SERVED = ROOT / "crates" / "dagon-cli" / "page"
STAND_IN = re.compile(rb'URLSearchParams\(["`]sim')


def served_problems(page=SERVED):
    out = []
    for p in sorted(page.rglob("*")):
        if not p.is_file():
            continue
        rel = p.relative_to(page).as_posix()
        data = p.read_bytes()
        if re.search(r"(^|/)(sample-|f401-bootcore)", rel):
            out.append(f"{rel}: a repository fixture")
        if STAND_IN.search(data):
            out.append(f"{rel}: the stand-in hub")
        if rel.endswith(".js") and b"Claude" in data:
            out.append(f"{rel}: names Claude")
    return out


def demo_problems(page):
    return [f"{p.relative_to(page).as_posix()}: names Claude" for p in sorted(page.rglob("*.js")) if b"Claude" in p.read_bytes()]


if __name__ == "__main__":
    found = served_problems()
    if "--demo" in sys.argv:
        found += demo_problems(pathlib.Path(sys.argv[sys.argv.index("--demo") + 1]))
    print("\n".join(found) or "the pages carry only what they should")
    sys.exit(1 if found else 0)
