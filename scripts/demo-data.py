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
# node id -> (firmware package dir under targets/, Rust target triple)
FIRMWARE = {
    "hub": ("rp2040-hub", "thumbv6m-none-eabi"),
    "seg": ("f401-app", "thumbv7em-none-eabihf"),
}
SLOTS = ("a", "b")
# Bundle layout (crates/dagon-format/src/bundle.rs, header.rs).
BUNDLE_HEAD, BUNDLE_ITEM = 16, 16
IMAGE_DESTS = (1, 2)  # HUB_DIRECT, SUB_STORE
TAMPER_AT = 0x028  # first byte of payload_sha256, inside the signed header


def run(cmd, env=None):
    print("+", " ".join(str(c) for c in cmd), flush=True)
    subprocess.run([str(c) for c in cmd], check=True, env=env)


def dagon(*args):
    run(["cargo", "run", "-q", "--locked", "--manifest-path", ROOT / "Cargo.toml",
         "-p", "dagon-cli", "--", *args])


def build(node, slot, out, pubkey):
    """Builds `node`'s app for `slot` with the package's own cargo config."""
    pkg, triple = FIRMWARE[node]
    pkg_dir = ROOT / "targets" / pkg
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
    ap.add_argument("--topology", type=pathlib.Path, default=ROOT / "topology" / "desk-demo.yaml")
    a = ap.parse_args()
    out, key, pubkey = a.out.resolve(), a.key.resolve(), a.pubkey.resolve()
    if out == ROOT or ROOT in out.parents:
        sys.exit(f"--out must lie outside the repository ({ROOT})")
    for f in (key, pubkey, a.topology):
        if not f.is_file():
            sys.exit(f"not a file: {f}")

    elves = {(n, s): build(n, s, out, pubkey) for n in FIRMWARE for s in SLOTS}

    images = out / "images"
    shutil.rmtree(images, ignore_errors=True)
    for version, release in VERSIONS.items():
        vdir = images / version
        for (node, slot), elf in elves.items():
            dagon("image", "sign", elf, "--node", node, "--version", version, "--slot", slot,
                  "--out", vdir / f"{node}-{slot}.img", "--topology", a.topology, "--key", key)
        dagon("release", "build", vdir, "--name", release.removesuffix(".dgb"),
              "--out", out / release, "--topology", a.topology, "--key", key)

    tamper(out / "release-1.1.dgb", out / "release-1.1-tampered.dgb", fix_crc=False)
    tamper(out / "release-1.1.dgb", out / "release-1.1-tampered-crcfix.dgb", fix_crc=True)

    files = sorted(p for p in out.rglob("*") if p.is_file() and "build" not in p.relative_to(out).parts)
    sums = [f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(out).as_posix()}"
            for p in files if p.name != "SHA256SUMS"]
    (out / "SHA256SUMS").write_text("\n".join(sums) + "\n")
    print("\n".join(sums))
    print(f"demo data in {out}")


if __name__ == "__main__":
    main()
