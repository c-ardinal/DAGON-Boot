"""Builds the dagon executable for one target into a release archive.

The archive holds `dagon` (`dagon.exe` on Windows) with the browser page
built in (crates/dagon-cli/page, checked to be up to date first). It is
named as cargo-binstall expects it (crates/dagon-cli/Cargo.toml):
`dagon-<target>.tar.gz`, or `.zip` for Windows, next to a `.sha256` file.
Nothing is uploaded or published.

    python scripts/release.py [--target <rust target>] [--out <dir>]

Without --target it builds for this machine (`rustc -vV` host).
"""
import argparse
import hashlib
import pathlib
import subprocess
import sys
import tarfile
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent


def run(cmd):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    subprocess.run([str(c) for c in cmd], check=True, cwd=ROOT)


def host_target():
    out = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
    return next(l.split(":", 1)[1].strip() for l in out.splitlines() if l.startswith("host:"))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--target", default=None)
    ap.add_argument("--out", type=pathlib.Path, default=ROOT / "out" / "release")
    a = ap.parse_args()
    target = a.target or host_target()
    windows = "windows" in target

    run([sys.executable, "-I", ROOT / "scripts" / "page.py", "check"])
    run(["cargo", "build", "--release", "--locked", "-p", "dagon-cli", "--target", target])

    exe = ROOT / "target" / target / "release" / ("dagon.exe" if windows else "dagon")
    a.out.mkdir(parents=True, exist_ok=True)
    archive = a.out / f"dagon-{target}{'.zip' if windows else '.tar.gz'}"
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
