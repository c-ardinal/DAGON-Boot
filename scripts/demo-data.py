"""Rebuilds the demo releases from the firmware in this checkout.

Builds the hub app (RP2040) and the seg app (F401, shows its version on the
7-segment display) for slots A and B with DAGON_PUBKEY embedded, signs each
build as 1.0.0 and 1.1.0, and writes into <out>:

    images/<version>/<node>-<slot>.img
    release-1.0.dgb, release-1.1.dgb
    release-1.1-tampered.dgb         one bit of every image header flipped
                                     (payload_sha256[0]); item CRCs left stale
    release-1.1-tampered-crcfix.dgb  same, item CRCs recomputed, so only the
                                     signature chain catches it
    SHA256SUMS

The tampered header fails the manifest header_sha256 check on the PC and the
header signature on a node (E_SIGNATURE) with --no-host-verify. Use
`apply --node hub` or `--node seg` to show each node refusing. Same inputs
give byte-identical outputs.

<out> must be a new or empty folder, or one this script made before (it
holds the marker file .dagon-demo-data); never the repository, a folder
containing it, or the home folder. On every run the script deletes only
its own earlier outputs there (images/, the .dgb files, SHA256SUMS) and
keeps the cargo build cache in <out>/build. The CLI is built into
<out>/build/cli so nothing is written inside the repository, and the run
stops before building anything when --key is not the secret half of
--pubkey.

    python scripts/demo-data.py --key <secret key file> --pubkey <public key file> [--out <dir>]
"""
import argparse
import hashlib
import os
import pathlib
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERSIONS = {"1.0.0": "release-1.0.dgb", "1.1.0": "release-1.1.dgb"}
# node id -> (firmware package dir under the repository root, Rust target triple)
FIRMWARE = {
    "hub": ("targets/rp2040-hub", "thumbv6m-none-eabi"),
    "seg": ("samples/f401-app", "thumbv7em-none-eabihf"),
}
SLOTS = ("a", "b")
# Bundle layout (crates/dagon-format/src/bundle.rs, header.rs).
BUNDLE_HEAD, BUNDLE_ITEM = 16, 16
IMAGE_DESTS = (1, 2)  # HUB_DIRECT, SUB_STORE
TAMPER_AT = 0x028  # first byte of payload_sha256, inside the signed header


def run(cmd, env=None):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    subprocess.run([str(c) for c in cmd], check=True, env=env)


# Cargo target directory for the CLI, outside the repository (set in main).
CLI_TARGET = None


def dagon_cmd(*args):
    return ["cargo", "run", "-q", "--locked", "--manifest-path", ROOT / "Cargo.toml",
            "--target-dir", CLI_TARGET, "-p", "dagon-cli", "--", *args]


def dagon(*args):
    run(dagon_cmd(*args))


def public_key_of(cli_output):
    """The hex public key in `dagon key pub` output."""
    for line in cli_output.splitlines():
        if line.startswith("public_key:"):
            return line.split(":", 1)[1].strip().lower()
    sys.exit("dagon key pub printed no public key")


def check_key_pair(cli_output, pubkey_text):
    """Exits unless the signing key's public half equals the --pubkey file."""
    if public_key_of(cli_output) != pubkey_text.strip().lower():
        sys.exit("--key is not the secret half of --pubkey: the firmware would reject "
                 "every image; pass the matching pair")


MARKER = ".dagon-demo-data"
TAMPERED = ("release-1.1-tampered.dgb", "release-1.1-tampered-crcfix.dgb")
SUMS = "SHA256SUMS"


def outputs(out):
    """The files and folders this script writes into `out` (not the cache)."""
    return [out / "images", *(out / r for r in VERSIONS.values()),
            *(out / t for t in TAMPERED), out / SUMS]


def check_out(out, root=ROOT, home=None):
    """Exits unless `out` is safe to write into and clean (see the module help)."""
    home = (home or pathlib.Path.home()).resolve()
    if out == root or root in out.parents or out in root.parents:
        sys.exit(f"--out must not be the repository, lie inside it or contain it ({root})")
    if out == home or out in home.parents or out.parent == out:
        sys.exit("--out must not be the home folder, one of its parents or a drive root")
    if out.exists() and not out.is_dir():
        sys.exit(f"--out is not a folder: {out}")
    if out.exists() and any(out.iterdir()) and not (out / MARKER).is_file():
        sys.exit(f"{out} is not empty and was not made by this script; pick a new folder")


def clean_out(out):
    """Marks `out` as this script's and deletes only its earlier outputs."""
    out.mkdir(parents=True, exist_ok=True)
    (out / MARKER).write_text("output folder of scripts/demo-data.py\n")
    for p in outputs(out):
        if p.is_dir() and not p.is_symlink():
            shutil.rmtree(p)
        elif p.exists() or p.is_symlink():
            p.unlink()


def build(node, slot, out, pubkey):
    """Builds `node`'s app for `slot` with the package's own cargo config."""
    pkg_path, triple = FIRMWARE[node]
    pkg_dir = ROOT / pkg_path
    pkg = pkg_dir.name
    env = {k: v for k, v in os.environ.items() if k != "DAGON_ALLOW_TEST_KEY"}
    env["DAGON_PUBKEY"] = str(pubkey)
    tgt = out / "build" / f"{pkg}-{slot}"
    cmd = ["cargo", "build", "--release", "--locked",
           "--manifest-path", pkg_dir / "Cargo.toml",
           "--config", pkg_dir / ".cargo" / "config.toml",
           "--target-dir", tgt]
    if slot == "b":
        cmd += ["--features", "slot-b"]
    run(cmd, env)
    return tgt / triple / "release" / pkg


def tamper(src, dst, fix_crc):
    """Copies bundle `src` to `dst` with one header bit flipped per image."""
    d = bytearray(src.read_bytes())
    flipped = 0
    for k in range(d[6]):
        item = BUNDLE_HEAD + BUNDLE_ITEM * k
        dest = d[item + 1]
        off, length = struct.unpack_from("<II", d, item + 4)
        if dest not in IMAGE_DESTS:
            continue
        d[off + TAMPER_AT] ^= 0x01
        flipped += 1
        if fix_crc:
            struct.pack_into("<I", d, item + 12, zlib.crc32(d[off:off + length]))
    if flipped == 0:
        sys.exit(f"{src}: no image items to tamper with")
    dst.write_bytes(d)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--key", required=True, type=pathlib.Path, help="secret signing key file")
    ap.add_argument("--pubkey", required=True, type=pathlib.Path,
                    help="public key file embedded in the firmware")
    ap.add_argument("--out", type=pathlib.Path,
                    default=pathlib.Path(tempfile.gettempdir()) / "dagon-demo-data")
    ap.add_argument("--topology", type=pathlib.Path, default=ROOT / "topology" / "sample.yaml")
    a = ap.parse_args()
    out, key, pubkey = a.out.resolve(), a.key.resolve(), a.pubkey.resolve()
    check_out(out)
    for f in (key, pubkey, a.topology):
        if not f.is_file():
            sys.exit(f"not a file: {f}")

    global CLI_TARGET
    CLI_TARGET = out / "build" / "cli"
    pub = subprocess.run([str(c) for c in dagon_cmd("key", "pub", "--key", key)],
                         check=True, capture_output=True, text=True)
    check_key_pair(pub.stdout, pubkey.read_text())
    clean_out(out)

    elves = {(n, s): build(n, s, out, pubkey) for n in FIRMWARE for s in SLOTS}

    images = out / "images"
    for version, release in VERSIONS.items():
        vdir = images / version
        for (node, slot), elf in elves.items():
            dagon("image", "sign", elf, "--node", node, "--version", version, "--slot", slot,
                  "--out", vdir / f"{node}-{slot}.img", "--topology", a.topology, "--key", key)
        dagon("release", "build", vdir, "--name", release.removesuffix(".dgb"),
              "--out", out / release, "--topology", a.topology, "--key", key)

    tamper(out / "release-1.1.dgb", out / "release-1.1-tampered.dgb", fix_crc=False)
    tamper(out / "release-1.1.dgb", out / "release-1.1-tampered-crcfix.dgb", fix_crc=True)

    files = sorted(f for p in outputs(out) if p.name != SUMS
                   for f in ([p] if p.is_file() else p.rglob("*")) if f.is_file())
    sums = [f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(out).as_posix()}"
            for p in files]
    (out / SUMS).write_text("\n".join(sums) + "\n")
    print("\n".join(sums))
    print(f"demo data in {out}")


if __name__ == "__main__":
    main()
