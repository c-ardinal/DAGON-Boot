"""Lists the standalone Cargo packages whose tests run on the host, for CI.

    python scripts/ci_crates.py          # one directory per line
    python scripts/ci_crates.py --json   # a JSON array, for a job matrix

A standalone package is a Cargo.toml with its own [workspace] outside the
root workspace (firmware crates, board and MCU packages, samples); the root
workspace's crates are tested as a whole. One has host tests when its own
sources (not those of a package nested below it, not vendor/ or target/) hold
a #[test] or it has a tests/ directory.
"""
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SKIP_DIRS = {"target", "vendor", "node_modules", ".git"}
TEST_MARK = re.compile(r"#\[(?:\w+::)*test\]")


def root_member_dirs():
    """Directories of the root workspace's packages, as cargo sees them."""
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1",
         "--manifest-path", str(ROOT / "Cargo.toml")],
        check=True, capture_output=True, text=True).stdout)
    return sorted(pathlib.Path(p["manifest_path"]).parent.resolve() for p in meta["packages"])


def package_dirs():
    members = set(root_member_dirs())
    return sorted(
        p.parent for p in ROOT.rglob("Cargo.toml")
        if p.parent.resolve() not in members | {ROOT.resolve()}
        and not SKIP_DIRS.intersection(p.relative_to(ROOT).parts)
        and re.search(r"^\[workspace\]", p.read_text(encoding="utf-8"), re.M)
    )


def own_files(pkg, nested):
    for p in pkg.rglob("*"):
        rel = p.relative_to(pkg).parts
        if p.is_file() and not SKIP_DIRS.intersection(rel) \
                and not any(n in p.parents for n in nested):
            yield p


def has_host_tests(pkg, nested):
    if (pkg / "tests").is_dir():
        return True
    return any(TEST_MARK.search(p.read_text(encoding="utf-8", errors="replace"))
               for p in own_files(pkg, nested) if p.suffix == ".rs")


def test_dirs():
    pkgs = package_dirs()
    return [p.relative_to(ROOT).as_posix() for p in pkgs
            if has_host_tests(p, [q for q in pkgs if q != p and p in q.parents])]


if __name__ == "__main__":
    dirs = test_dirs()
    print(json.dumps(dirs) if "--json" in sys.argv else "\n".join(dirs))
