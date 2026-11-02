"""Prints the third-party dependency licence tables for the README.

Reads every Cargo.lock in the repository (the root workspace and each
firmware package under targets/) through `cargo metadata`, and
web/package-lock.json. Run from the repository root:

    python scripts/licences.py > deps.md
"""
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def cargo_workspaces():
    yield ROOT, "crates/"
    for lock in sorted((ROOT / "targets").glob("*/Cargo.lock")):
        yield lock.parent, lock.parent.name


def cargo_packages():
    """(name, version) -> (licence, set of users)."""
    found = {}
    for path, user in cargo_workspaces():
        out = subprocess.run(
            ["cargo", "+stable", "metadata", "--format-version", "1", "--locked",
             "--manifest-path", str(path / "Cargo.toml")],
            check=True, capture_output=True, encoding="utf-8", cwd=path,
        ).stdout
        for p in json.loads(out)["packages"]:
            if p.get("source") is None:  # this repository's own crates
                continue
            key = (p["name"], p["version"])
            lic = p.get("license") or ("file: " + (p.get("license_file") or "?"))
            found.setdefault(key, [lic, set()])[1].add(user)
    return found


def npm_packages():
    lock = json.loads((ROOT / "web" / "package-lock.json").read_text(encoding="utf-8"))
    rows = {}
    for path, p in lock["packages"].items():
        if not path:
            continue
        name = path.split("node_modules/")[-1]
        rows[(name, p.get("version", "?"))] = p.get("license", "?")
    return rows


def main():
    crates = cargo_packages()
    by_name = {}
    for (name, ver), (lic, users) in crates.items():
        e = by_name.setdefault(name, [set(), set(), set()])
        e[0].add(ver)
        e[1].add(lic)
        e[2] |= users
    print(f"Rust crates: {len(by_name)} packages, {len(crates)} name/version pairs. "
          "\"Used by\": `crates/` is the host workspace, other names are firmware "
          "packages under `targets/`.\n")
    print("| Crate | Versions | Licence | Used by |")
    print("|---|---|---|---|")
    for name in sorted(by_name, key=str.lower):
        vers, lics, users = by_name[name]
        used = ", ".join(sorted(users))
        print(f"| {name} | {', '.join(sorted(vers))} | {' / '.join(sorted(lics))} | {used} |")
    npm = npm_packages()
    print(f"\nnpm packages (`web/package-lock.json`): {len(npm)}, all build and test "
          "time only except the Svelte runtime, which ships in the page.\n")
    print("| Package | Version | Licence |")
    print("|---|---|---|")
    for (name, ver), lic in sorted(npm.items()):
        print(f"| {name} | {ver} | {lic} |")
    summary = {}
    for (lic, _) in crates.values():
        summary[lic] = summary.get(lic, 0) + 1
    for lic in npm.values():
        summary[lic] = summary.get(lic, 0) + 1
    print("\nLicence expressions seen:", file=sys.stderr)
    for lic, n in sorted(summary.items(), key=lambda x: -x[1]):
        print(f"  {n:4d}  {lic}", file=sys.stderr)


if __name__ == "__main__":
    main()
