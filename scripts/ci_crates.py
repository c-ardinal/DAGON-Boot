"""Lists the standalone Cargo packages whose tests run on the host, for CI.

    python scripts/ci_crates.py          # one directory per line
    python scripts/ci_crates.py --json   # a JSON array, for a job matrix
    python scripts/ci_crates.py --firmware-lint   # firmware clippy runs (JSON)

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
import tomllib

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


def firmware_lints():
    """One clippy run per firmware build a package declares in
    `[package.metadata.dagon.lint]`: `features` (one feature set per run,
    "" for the defaults), `target` (left out: the package's own
    `.cargo/config.toml` names it) and `env` (variables the build needs).
    `toolchain` is the package's own `rust-toolchain.toml` channel, else
    "stable"."""
    runs = []
    for p in package_dirs():
        manifest = tomllib.loads((p / "Cargo.toml").read_text(encoding="utf-8"))
        lint = manifest.get("package", {}).get("metadata", {}).get("dagon", {}).get("lint")
        if lint is None:
            continue
        own = p / "rust-toolchain.toml"
        toolchain = "stable"
        if own.exists():
            toolchain = tomllib.loads(own.read_text(encoding="utf-8"))["toolchain"]["channel"]
        for features in lint["features"]:
            runs.append({
                "dir": p.relative_to(ROOT).as_posix(),
                "target": lint.get("target", ""),
                "features": features,
                "env": lint.get("env", {}),
                "toolchain": toolchain,
            })
    return runs


if __name__ == "__main__":
    if "--firmware-lint" in sys.argv:
        print(json.dumps(firmware_lints()))
        sys.exit(0)
    dirs = test_dirs()
    print(json.dumps(dirs) if "--json" in sys.argv else "\n".join(dirs))
