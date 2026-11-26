"""Builds each sample app and checks it fits the app slot its MCU
definition gives, and its image and static RAM against the sample's own
budget (CT-16, with bootcore_size.py for the boot cores).

    python scripts/sample_size.py            # build every sample app, check, print sizes
    python scripts/sample_size.py --targets  # the rustup targets they build for

A sample app is a package under samples/ with a `budget.toml`:

    mcu = "<mcu id>"      # targets/mcu/<id>/dagon-mcu.toml
    [image]
    max_bytes = N         # the most bytes the image may take
    [ram]
    max_bytes = N         # the most static RAM
    skip = [".stack"]     # sections that are not static data (optional)

Static RAM is the sizes of the ELF's allocated, writable sections, added
(as `stage0_size.ram_bytes`), without those `skip` names: a vendor's link
script may give the stack, or a placeholder that only moves the location
counter, a section of its own.

The hard limit is the definition's data: the smallest `capacity` of its
`[[slot_layout]]` entries with `target = "app"`, since one source is built
for each of those slots. The budget is what a change is seen against: an
app is the user's own, so a sample keeps most of its slot free, and the
gate is there to show the sample that grew (a dependency pulled in, a
table no longer shared), not to fill the slot.

The package is built in release with its own cargo config (target, link
arguments, target tag). The image is what the signer takes from the ELF
(`dagon_sign::image::payload_from_file`): from the lowest to the highest
address its loadable segments fill. An app slot that is a partition (its
`[[slot_layout]]` has `partition`: the vendor's second stage maps the
image's segments, they are not laid out by address) is measured as an
upper bound instead: the segments' bytes, each segment's header, the image
header and trailer, and the alignment padding of the two mapped segments.
"""
import os
import pathlib
import struct
import subprocess
import sys
import tomllib

import bootcore_size
import stage0_size

ROOT = stage0_size.ROOT
SAMPLES = ROOT / "samples"
BUDGET = "budget.toml"
PT_LOAD = 1
# A partition image (ESP-IDF app image): header and extended header, each
# segment's header, checksum padding and SHA-256, and the padding that
# aligns each of the two mapped segments (code, read-only data) to a page.
PART_HEADER = 24
PART_SEGMENT_HEADER = 8
PART_TRAILER = 16 + 32
PART_PAGE = 0x1_0000
PART_MAPPED = 2
SKIPPED = ("target", "node_modules")


def packages() -> list[pathlib.Path]:
    """Every sample app: the folders under samples/ with a budget."""
    return sorted(p.parent for p in SAMPLES.glob(f"**/{BUDGET}")
                  if not set(SKIPPED) & set(p.relative_to(SAMPLES).parts))


def read_budget(pkg: pathlib.Path) -> dict:
    return tomllib.loads((pkg / BUDGET).read_text(encoding="utf-8"))


def app_slots(mcu_id: str) -> list[dict]:
    return [s for s in stage0_size.definition(mcu_id).get("slot_layout", [])
            if s.get("target") == "app"]


def slot_bytes(mcu_id: str) -> int | None:
    """The bytes an app image may take on `mcu_id` (None: the definition
    has no app slot)."""
    slots = [s["capacity"] for s in app_slots(mcu_id)]
    return min(slots) if slots else None


def in_partition(mcu_id: str) -> bool:
    """Whether the app slots of `mcu_id` are partitions."""
    return any("partition" in s for s in app_slots(mcu_id))


def segments(elf: bytes) -> list[int]:
    """The file sizes of the ELF32 little-endian image's loadable segments."""
    if elf[:4] != b"\x7fELF" or elf[4] != 1 or elf[5] != 1:
        raise ValueError("not a 32-bit little-endian ELF")
    phoff, = struct.unpack_from("<I", elf, 0x1C)
    phentsize, phnum = struct.unpack_from("<HH", elf, 0x2A)
    out = []
    for i in range(phnum):
        p_type, _, _, _, p_filesz = struct.unpack_from("<IIIII", elf, phoff + i * phentsize)
        if p_type == PT_LOAD and p_filesz:
            out.append(p_filesz)
    return out


def partition_bytes(sizes: list[int]) -> int:
    """An upper bound of the partition image holding segments of `sizes`."""
    padded = sum((s + 3) & ~3 for s in sizes)
    return (PART_HEADER + padded + PART_SEGMENT_HEADER * (len(sizes) + PART_MAPPED)
            + PART_TRAILER + PART_MAPPED * PART_PAGE)


def image_bytes(elf: bytes, mcu_id: str) -> int:
    if in_partition(mcu_id):
        return partition_bytes(segments(elf))
    return bootcore_size.image_bytes(elf)


SHF_WRITE, SHF_ALLOC = 0x1, 0x2


def ram_sections(elf: bytes) -> list[tuple[str, int]]:
    """(name, size) of the ELF32 little-endian image's allocated, writable
    sections."""
    shoff, = struct.unpack_from("<I", elf, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", elf, 0x2E)
    names = struct.unpack_from("<IIIIII", elf, shoff + shstrndx * shentsize)[4]
    out = []
    for i in range(shnum):
        name, _, flags, _, _, size = struct.unpack_from("<IIIIII", elf, shoff + i * shentsize)
        if flags & (SHF_WRITE | SHF_ALLOC) == SHF_WRITE | SHF_ALLOC:
            at = names + name
            out.append((elf[at:elf.index(b"\0", at)].decode(), size))
    return out


def check_ram(elf: bytes, budget: dict, name: str) -> list[str]:
    """The image's static RAM against the budget's `[ram]`; why it does
    not pass."""
    ram = budget.get("ram", {})
    most, skip = ram.get("max_bytes"), ram.get("skip", [])
    if most is None:
        return [f"{name}: its {BUDGET} has no [ram] max_bytes"]
    sections = ram_sections(elf)
    absent = sorted(set(skip) - {n for n, _ in sections})
    used = sum(size for n, size in sections if n not in skip)
    print(f"{name}: {used} of {most} bytes of static RAM ({most - used} free)", flush=True)
    out = [f"{name}: its {BUDGET} skips {n}, which the image does not have" for n in absent]
    if used > most:
        out.append(f"{name}: {used} bytes of static RAM, over its {most}")
    return out


def verdicts(name: str, used: int, slot: int, most: int | None) -> list[str]:
    """Why an image of `used` bytes does not pass in a `slot`-byte slot
    with a budget of `most` bytes."""
    out = []
    if used > slot:
        out.append(f"{name}: {used} bytes, over its {slot}-byte slot")
    if most is None:
        out.append(f"{name}: its {BUDGET} has no [image] max_bytes")
    elif most > slot:
        out.append(f"{name}: its budget of {most} bytes is over its {slot}-byte slot")
    elif used > most:
        out.append(f"{name}: {used} bytes, over its budget of {most}")
    return out


def check(elf_path: pathlib.Path, pkg: pathlib.Path) -> list[str]:
    """Checks one built sample app; why it does not pass."""
    name = pkg.relative_to(ROOT).as_posix()
    b = read_budget(pkg)
    mcu = b.get("mcu")
    if mcu is None or not (stage0_size.MCU / mcu / "dagon-mcu.toml").is_file():
        return [f"{name}: its {BUDGET} names no MCU definition (mcu = {mcu!r})"]
    slot = slot_bytes(mcu)
    if slot is None:
        return [f"{name}: the definition of {mcu} has no app slot"]
    elf = elf_path.read_bytes()
    used = image_bytes(elf, mcu)
    most = b.get("image", {}).get("max_bytes")
    bound = " at most" if in_partition(mcu) else ""
    print(f"{name}:{bound} {used} bytes, budget {most}, slot {slot} ({slot - used} free)",
          flush=True)
    return verdicts(name, used, slot, most) + check_ram(elf, b, name)


def target_of(pkg: pathlib.Path) -> str:
    """The target the package's own cargo config names."""
    config = pkg / ".cargo" / "config.toml"
    return tomllib.loads(config.read_text(encoding="utf-8"))["build"]["target"]


def bin_of(pkg: pathlib.Path) -> str:
    """The package's binary: its first `[[bin]]`, else the package name."""
    manifest = tomllib.loads((pkg / "Cargo.toml").read_text(encoding="utf-8"))
    bins = manifest.get("bin", [])
    return bins[0]["name"] if bins else manifest["package"]["name"]


def build_all() -> None:
    failed = []
    found = packages()
    if not found:
        sys.exit(f"no sample app with a {BUDGET} under samples/")
    for pkg in found:
        name = pkg.relative_to(ROOT).as_posix()
        out = pkg / "target" / "sample-size"
        env = dict(os.environ, CARGO_TARGET_DIR=str(out))
        env.setdefault("DAGON_ALLOW_TEST_KEY", "1")
        binary = bin_of(pkg)
        if subprocess.run(["cargo", "build", "--release", "--bin", binary],
                          cwd=pkg, env=env).returncode:
            # An app links into its slot: an image over it fails there.
            failed.append(f"{name}: does not build (over its slot?)")
            continue
        failed += check(out / target_of(pkg) / "release" / binary, pkg)
    if failed:
        sys.exit("\n".join(failed))


def rustup_targets() -> list[str]:
    """The targets rustup adds (a vendor toolchain brings its own: the
    package has a rust-toolchain.toml)."""
    return sorted({target_of(p) for p in packages()
                   if not (p / "rust-toolchain.toml").is_file()})


if __name__ == "__main__":
    if sys.argv[1:] == ["--targets"]:
        print(" ".join(rustup_targets()))
    else:
        build_all()
