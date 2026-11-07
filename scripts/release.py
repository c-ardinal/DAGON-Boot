"""Builds the dagon executable for one target into a release archive.

The archive holds `dagon` (`dagon.exe` on Windows) with the browser page
built in (crates/dagon-cli/page, checked to be up to date first). It is
named with the component and version as cargo-binstall expects it
(crates/dagon-cli/Cargo.toml): `dagon-cli-<version>-<target>.tar.gz`, or
`.zip` for Windows, next to a `.sha256` file. The version is the cli entry
of compat.toml, or the version of the tag being released, which may add -rc.N
(site/src/content/docs/explanation/versioning.md).
Nothing is uploaded or published.

    python scripts/release.py [--target <rust target>] [--out <dir>] [--tag <tag>]

Without --target it builds for this machine (`rustc -vV` host). A tag that
does not name the cli at the compat.toml version stops it before any build.
"""
import argparse
import hashlib
import importlib.util
import pathlib
import subprocess
import sys
import tarfile
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("bump_check", ROOT / "scripts" / "bump_check.py")
bc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bc)


def run(cmd):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    subprocess.run([str(c) for c in cmd], check=True, cwd=ROOT)


def host_target():
    out = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
    return next(l.split(":", 1)[1].strip() for l in out.splitlines() if l.startswith("host:"))


def release_version(tag):
    """The version in the archive names; a tag that does not fit raises ValueError."""
    version = next(c["version"] for c in bc.load_components() if c["name"] == "cli")
    return bc.tag_version(tag, "cli", version) if tag else version


def archive_name(version, target):
    windows = "windows" in target
    return f"dagon-cli-{version}-{target}{'.zip' if windows else '.tar.gz'}"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--target", default=None)
    ap.add_argument("--out", type=pathlib.Path, default=ROOT / "out" / "release")
    ap.add_argument("--tag", default=None)
    a = ap.parse_args()
    try:
        version = release_version(a.tag)
    except ValueError as e:
        sys.exit(str(e))
    target = a.target or host_target()
    windows = "windows" in target

    run([sys.executable, "-I", ROOT / "scripts" / "page.py", "check"])
    run(["cargo", "build", "--release", "--locked", "-p", "dagon-cli", "--target", target])

    exe = ROOT / "target" / target / "release" / ("dagon.exe" if windows else "dagon")
    a.out.mkdir(parents=True, exist_ok=True)
    archive = a.out / archive_name(version, target)
    if windows:
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
            z.write(exe, exe.name)
    else:
        with tarfile.open(archive, "w:gz") as t:
            info = t.gettarinfo(str(exe), exe.name)
            info.mode = 0o755
            with open(exe, "rb") as f:
                t.addfile(info, f)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")
    print(f"{archive} {digest}")


if __name__ == "__main__":
    main()
