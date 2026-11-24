"""Builds each MCU package's boot core and checks it fits the boot core slot
its MCU definition gives, part by part and in static RAM against the
package's budget (CT-16, with stage0_size.py for the stage0s).

    python scripts/bootcore_size.py                 # build every boot core, check, print sizes
    python scripts/bootcore_size.py <elf> <mcu id>  # check one built image
    python scripts/bootcore_size.py --targets       # the targets they build for

The budget is the definition's data (targets/mcu/<id>/dagon-mcu.toml): the
smallest `capacity` of its `[[slot_layout]]` entries with `target =
"bootcore"`, since one boot core source is built for each of those slots
and each image must fit the slot it is written to. An MCU whose definition
has no such entry has no DAGON boot core (its vendor's second stage loads
the images) and is not built; one that has them needs a boot core package
(`bootcore/`, with `examples/size.rs`: the boot core of a node without
extensions).

The package is built in release once for a node without a board (its
firmware lint boot folder, `bootcore/lint`) and once for every board whose
definition names the MCU, from that board's boot folder, the one the
stage0 size check builds from (`stage0_size.size_folder`): the board puts
its boot LED, halt pattern and clock into the boot core. The image is what
the signer takes from the ELF (`dagon_sign::image::payload_from_file`):
from the lowest to the highest address its loadable segments fill.

Each build also writes its linker map, and the package's
`bootcore/budget.toml` splits the image into parts as a stage0's does
(`stage0_size.read_budget`: ordered `[[part]]` entries, the last
`other`), attributed from the map over the image's span; a part over its
`max_bytes`, or parts that do not add up to the image, fail. Its `[ram]
max_bytes` bounds the static RAM (`stage0_size.ram_bytes`: the allocated,
writable sections). A boot core is updated in the field, so it keeps no
margin of its slot: the parts are where a change shows.

A boot core that stage0 copies into RAM and runs from there (ESP32-S3:
its `budget.toml` has `[image]`, and the definition gives it no boot core
slot) is built once with the budget's `features` and checked against
`[image] max_bytes` (the RAM it is linked into), its parts from the symbol
table (`stage0_size.check_loaded`), as a ROM-loaded stage0 is.

A boot core package may check its own images further: its
`image_check.py`, run with the built ELF and the boot folder, prints what
fails (an MCU's vector table, say).

An image also fails when it links a name of the event catalogs (read from
the Events page they generate): firmware decides on event codes and a
generated transition table, and names are looked up only where a person
reads them.
"""
import os
import pathlib
import re
import struct
import subprocess
import sys
import tomllib

import stage0_size

MCU = stage0_size.MCU
PT_LOAD = 1
EXAMPLE = "size"
# The page the event catalogs generate (dagon-catalog's docs test).
EVENTS_PAGE = stage0_size.ROOT / "site" / "src" / "content" / "docs" / "en" / "reference" / "events.md"
NAME_MIN = 6
# A boot core package's own check of its built image (package_check).
PACKAGE_CHECK = "image_check.py"
BUDGET = "budget.toml"


def budget(mcu_id: str) -> int | None:
    """The bytes a boot core image may take on `mcu_id` (None: the MCU has
    no boot core slot)."""
    slots = [s["capacity"] for s in stage0_size.definition(mcu_id).get("slot_layout", [])
             if s.get("target") == "bootcore"]
    return min(slots) if slots else None


def image_bytes(elf: bytes) -> int:
    """Bytes from the lowest to the highest address the ELF32
    little-endian image's loadable segments fill."""
    if elf[:4] != b"\x7fELF" or elf[4] != 1 or elf[5] != 1:
        raise ValueError("not a 32-bit little-endian ELF")
    phoff, = struct.unpack_from("<I", elf, 0x1C)
    phentsize, phnum = struct.unpack_from("<HH", elf, 0x2A)
    spans = []
    for i in range(phnum):
        p_type, _, _, p_paddr, p_filesz = struct.unpack_from("<IIIII", elf, phoff + i * phentsize)
        if p_type == PT_LOAD and p_filesz:
            spans.append((p_paddr, p_paddr + p_filesz))
    if not spans:
        raise ValueError("no loadable data")
    return max(e for _, e in spans) - min(s for s, _ in spans)


def image_data(elf: bytes) -> bytes:
    """The bytes of the ELF32 little-endian image's loadable segments, in
    address order: what is written to flash."""
    phoff, = struct.unpack_from("<I", elf, 0x1C)
    phentsize, phnum = struct.unpack_from("<HH", elf, 0x2A)
    parts = []
    for i in range(phnum):
        p_type, p_offset, _, p_paddr, p_filesz = struct.unpack_from(
            "<IIIII", elf, phoff + i * phentsize)
        if p_type == PT_LOAD and p_filesz:
            parts.append((p_paddr, elf[p_offset:p_offset + p_filesz]))
    return b"".join(d for _, d in sorted(parts))


def catalog_names(page: pathlib.Path = EVENTS_PAGE) -> set[str]:
    """Every name the event catalogs hold, read from the Events page they
    generate: machines, states, kinds and transition methods. Names shorter
    than `NAME_MIN` are left out, as short words occur by chance in code
    bytes; a linked catalog carries its long names with its short ones."""
    names = set()
    for line in page.read_text(encoding="utf-8").splitlines():
        if m := re.match(r"### Machine `(\w+)`", line):
            names.add(m[1])
        elif line.startswith("States: "):
            names.update(re.findall(r"`(\w+)`", line))
        elif m := re.match(r"\| `\w+\.(\w+)` \| 0x[0-9a-f]+ \| `(\w+)`", line):
            names.update(m.groups())
    return {n for n in names if len(n) >= NAME_MIN}


def linked_names(data: bytes, names: set[str]) -> list[str]:
    """The catalog names `data` contains."""
    return sorted(n for n in names if n.encode() in data)


def verdict(used: int, size: int) -> str | None:
    """Why `used` bytes do not pass in a `size`-byte slot (None: they do)."""
    return f"{used} bytes, over its {size}-byte slot" if used > size else None


def budget_file(mcu_id: str) -> pathlib.Path:
    """The boot core package's parts and RAM budget."""
    return package(mcu_id) / BUDGET


def image_start(elf: bytes) -> int:
    """The lowest address the ELF32 image's loadable segments fill."""
    phoff, = struct.unpack_from("<I", elf, 0x1C)
    phentsize, phnum = struct.unpack_from("<HH", elf, 0x2A)
    starts = []
    for i in range(phnum):
        p_type, _, _, p_paddr, p_filesz = struct.unpack_from("<IIIII", elf, phoff + i * phentsize)
        if p_type == PT_LOAD and p_filesz:
            starts.append(p_paddr)
    return min(starts)


def check(elf_path: pathlib.Path, mcu_id: str, label: str = "",
          map_path: pathlib.Path | None = None) -> list[str]:
    """Checks one built image (and its parts, given its map); why it does
    not pass."""
    size = budget(mcu_id)
    name = f"{mcu_id} boot core{label}"
    if size is None:
        return [f"{name}: the definition has no boot core slot"]
    elf = elf_path.read_bytes()
    used = image_bytes(elf)
    print(f"{name}: {used} of {size} bytes ({size - used} free)", flush=True)
    why = verdict(used, size)
    out = [f"{name}: {why}"] if why else []
    out += catalog_check(elf, name)
    path = budget_file(mcu_id)
    if not path.is_file():
        return out + [f"{name}: no {path.relative_to(stage0_size.ROOT)}"]
    out += stage0_size.check_ram(elf, path, name)
    if map_path is not None:
        parts = stage0_size.read_budget(path)
        items = stage0_size.map_items(map_path.read_text(encoding="utf-8", errors="replace"),
                                      image_start(elf), used)
        text, bad = stage0_size.parts_report(stage0_size.attribute(items, parts), parts, used)
        print(text, end="")
        elf_path.with_name(elf_path.name + ".parts.txt").write_text(text, encoding="utf-8")
        out += [f"{name}: {b}" for b in bad]
    return out


def catalog_check(elf: bytes, name: str) -> list[str]:
    linked = linked_names(image_data(elf), catalog_names())
    return [f"{name}: links event catalog names {', '.join(linked)}"] if linked else []


def ram_loaded() -> list[str]:
    """MCUs whose boot core stage0 copies into RAM (its `budget.toml` has
    `[image]`)."""
    return [m for m in mcus()
            if budget_file(m).is_file()
            and "image" in tomllib.loads(budget_file(m).read_text(encoding="utf-8"))]


def check_ram_loaded(elf_path: pathlib.Path, mcu_id: str) -> list[str]:
    """Checks one built RAM-loaded boot core; why it does not pass."""
    name = f"{mcu_id} boot core"
    return (stage0_size.check_loaded(elf_path, budget_file(mcu_id), name, keep_margin=False)
            + catalog_check(elf_path.read_bytes(), name))


def package_check(pkg: pathlib.Path, elf: pathlib.Path, folder: pathlib.Path,
                  name: str) -> list[str]:
    """The boot core package's own check of one built image, when it has
    one (`<package>/image_check.py <elf> <boot folder>`: what only that
    MCU's image can be checked for); each line it prints is a finding."""
    hook = pkg / PACKAGE_CHECK
    if not hook.is_file():
        return []
    r = subprocess.run([sys.executable, "-I", str(hook), str(elf), str(folder)],
                       capture_output=True, text=True)
    if r.returncode == 0:
        return []
    lines = (r.stdout + r.stderr).strip().splitlines() or ["failed"]
    return [f"{name}: {line}" for line in lines]


def mcus() -> list[str]:
    return [d.parent.name for d in sorted(MCU.glob("*/dagon-mcu.toml"))]


def package(mcu_id: str) -> pathlib.Path:
    return MCU / mcu_id / "bootcore"


def bootcore_mcus() -> list[str]:
    return [m for m in mcus() if budget(m) is not None]


def target_of(mcu_id: str) -> str:
    """The target the boot core package's own cargo config names."""
    config = package(mcu_id) / ".cargo" / "config.toml"
    return tomllib.loads(config.read_text(encoding="utf-8"))["build"]["target"]


def folder_of(mcu_id: str, board: str | None) -> pathlib.Path:
    if board is None:
        return package(mcu_id) / "lint"
    return stage0_size.size_folder(mcu_id, board)


def build_ram_loaded(mcu: str) -> list[str]:
    pkg = package(mcu)
    name = tomllib.loads((pkg / "Cargo.toml").read_text(encoding="utf-8"))["package"]["name"]
    features = tomllib.loads(budget_file(mcu).read_text(encoding="utf-8"))["image"]["features"]
    env = dict(os.environ, DAGON_TARGET_TAG="size")
    env.setdefault("DAGON_ALLOW_TEST_KEY", "1")
    build = ["cargo", "build", "--release", "--bin", name, "--features", ",".join(features)]
    if subprocess.run(build, cwd=pkg, env=env).returncode:
        return [f"{mcu} boot core: does not build"]
    return check_ram_loaded(pkg / "target" / target_of(mcu) / "release" / name, mcu)


def build_all() -> None:
    failed = []
    for mcu in ram_loaded():
        failed += build_ram_loaded(mcu)
    for mcu in mcus():
        if budget(mcu) is None:
            if mcu not in ram_loaded():
                print(f"{mcu}: no boot core slot in its definition, no boot core to build")
            continue
        pkg = package(mcu)
        if not (pkg / "examples" / f"{EXAMPLE}.rs").is_file():
            failed.append(f"{mcu}: the definition has a boot core slot, "
                          f"but {pkg.relative_to(stage0_size.ROOT)} has no examples/{EXAMPLE}.rs")
            continue
        for board in stage0_size.configurations(mcu):
            label = f" ({board or 'no board'})"
            folder = folder_of(mcu, board)
            if not folder.is_dir():
                failed.append(f"{mcu} boot core{label}: no boot folder {folder}")
                continue
            out = pkg / "target" / "bootcore-size" / (board or "no-board")
            env = dict(os.environ, DAGON_BOOT_CRATE=str(folder), CARGO_TARGET_DIR=str(out),
                       DAGON_TARGET_TAG="size")
            env.setdefault("DAGON_ALLOW_TEST_KEY", "1")
            elf = out / target_of(mcu) / "release" / "examples" / EXAMPLE
            link_map = elf.with_name(EXAMPLE + ".map")
            build = ["cargo", "rustc", "--release", "--example", EXAMPLE, "--",
                     "-C", f"link-arg=-Map={link_map}"]
            if subprocess.run(build, cwd=pkg, env=env).returncode:
                # The boot core links into its slot: an image over it fails there.
                failed.append(f"{mcu} boot core{label}: does not build (over its slot?)")
                continue
            failed += check(elf, mcu, label, link_map)
            failed += package_check(pkg, elf, folder, f"{mcu} boot core{label}")
    if failed:
        sys.exit("\n".join(failed))


if __name__ == "__main__":
    if sys.argv[1:] == ["--targets"]:
        print(" ".join(sorted({target_of(m) for m in bootcore_mcus()})))
    elif len(sys.argv) == 3:
        why = check(pathlib.Path(sys.argv[1]), sys.argv[2])
        if why:
            sys.exit("\n".join(why))
    else:
        build_all()
