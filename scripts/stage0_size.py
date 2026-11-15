"""Builds each MCU package's stage0, checks it fits its region and checks
each of its parts against the package's budget.

    python scripts/stage0_size.py            # build every stage0, check, print sizes
    python scripts/stage0_size.py <elf> <mcu id> [<map>]   # check one built image
    python scripts/stage0_size.py --targets        # the targets they build for

An MCU whose definition (targets/mcu/<id>/dagon-mcu.toml) has `[stage0]`
(the region: `[layout] stage0` at, `region_bytes`) and whose package has a
`stage0/` folder is built in release once for a node without a board (the
stage0's firmware lint boot folder, `bootcore/lint`) and once for every
board whose definition (targets/boards/<id>/dagon-board.toml) names that
MCU: the board puts its diagnostic output into stage0, so each is a
different image, built from that board's boot folder `stage0/size/<board>`
(`size_folder`). Each is measured: the bytes its loadable segments put
into flash (code, read-only data and the initial data image) must leave
`margin(size)` bytes of the region free. Stage0's linker script refuses an
image that does not (`dagon_mcu_build::STAGE0_ASSERT`); this names the
bytes.

Each build also writes its linker map, and the package's
`stage0/budget.toml` splits the image into parts: an ordered list of
`[[part]]` entries `{ name, match = [prefixes], max_bytes }`, the last one
`other` with no `match`. Each input section of the map belongs to the
first part one of whose prefixes starts its symbol's demangled path (a
leading `<` or `&` aside), its section name or its output section's name;
the rest, and the alignment padding between sections, belong to `other`.
A build fails when the parts do not add up to the image's flash bytes or
when a part is over its `max_bytes`; the parts list is printed and written
next to the image (`<image>.parts.txt`).
"""
import os
import pathlib
import re
import struct
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
MCU = ROOT / "targets" / "mcu"
BOARDS = ROOT / "targets" / "boards"
PT_LOAD = 1
# The part of a stage0 region every build must leave free: 1/16 (1 KiB of
# 16 KiB). Stage0 is never updated in the field, yet its source is rebuilt
# for every new node with the toolchain of that build; a compiler or
# library release, a board's boot pins or a fix moves an opt-level "z"
# image by hundreds of bytes, and an image without spare room turns into a
# first write that cannot be built (as the Waveshare build did, 64 bytes
# over, while the board-less build fitted).
MARGIN_DIVISOR = 16
OTHER = "other"


def definition(mcu_id: str) -> dict:
    return tomllib.loads((MCU / mcu_id / "dagon-mcu.toml").read_text(encoding="utf-8"))


def region(mcu_id: str) -> tuple[int, int]:
    """Stage0's region from the MCU definition: (`[layout] stage0` at,
    `[stage0] region_bytes`)."""
    d = definition(mcu_id)
    return d["layout"]["stage0"]["at"], d["stage0"]["region_bytes"]


def flash_bytes(elf: bytes, at: int, size: int) -> int:
    """Bytes the ELF32 little-endian image's loadable segments place at
    physical addresses inside [at, at + size)."""
    if elf[:4] != b"\x7fELF" or elf[4] != 1 or elf[5] != 1:
        raise ValueError("not a 32-bit little-endian ELF")
    phoff, = struct.unpack_from("<I", elf, 0x1C)
    phentsize, phnum = struct.unpack_from("<HH", elf, 0x2A)
    total = 0
    for i in range(phnum):
        p_type, _, _, p_paddr, p_filesz = struct.unpack_from("<IIIII", elf, phoff + i * phentsize)
        if p_type == PT_LOAD and p_filesz and at <= p_paddr < at + size:
            total = max(total, p_paddr + p_filesz - at)
    return total


def margin(size: int) -> int:
    """Bytes of a `size`-byte stage0 region every build leaves free."""
    return size // MARGIN_DIVISOR


def verdict(used: int, size: int) -> str | None:
    """Why `used` bytes do not pass in a `size`-byte region (None: they do)."""
    if used > size:
        return f"{used} bytes, over its {size}-byte region"
    if used > size - margin(size):
        return f"{used} bytes, inside the {margin(size)}-byte margin of its {size}-byte region"
    return None


# A map line of LLD: VMA, LMA, size, alignment, then the name indented by
# its level (0 output section, 8 input section, 16 symbol).
MAP_LINE = re.compile(r"^\s*([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+(\d+) (\s*)(\S.*)$")
# ARM mapping symbols, which name no code.
MAPPING_SYMBOL = re.compile(r"^\$[atd](\.|$)")


def map_items(text: str, at: int, size: int) -> list[dict]:
    """The input sections of an LLD map placed in flash inside [at, at +
    size), in address order: `lma`, `size`, `align` (its own or its output
    section's, the larger), `names` (its first symbol, its section name,
    its output section's name)."""
    items, out, out_align = [], "", 1
    for line in text.splitlines():
        m = MAP_LINE.match(line)
        if not m:
            continue
        lma, length, align = int(m[2], 16), int(m[3], 16), int(m[4])
        level, name = len(m[5]), m[6]
        if level == 0:
            out, out_align = name, align
            continue
        if level == 8:
            section = re.search(r":\((.*)\)$", name)
            items.append({"lma": lma, "size": length, "align": max(align, out_align),
                          "names": [section[1] if section else name, out]})
        elif level == 16 and items and not MAPPING_SYMBOL.match(name) and len(items[-1]["names"]) == 2:
            items[-1]["names"].insert(0, name)
    return sorted((i for i in items if i["size"] and at <= i["lma"] < at + size),
                  key=lambda i: i["lma"])


LEGACY_ESCAPES = {"$LT$": "<", "$GT$": ">", "$RF$": "&", "$u20$": " ", "..": "::"}


def demangled(name: str) -> str:
    """A symbol name as a Rust path: LLD leaves the escapes of the legacy
    mangling in a trait impl's name (`_$LT$sha2..Sha512$u20$as$u20$...`)."""
    name = name.removeprefix("_") if name.startswith("_$") else name
    for escape, char in LEGACY_ESCAPES.items():
        name = name.replace(escape, char)
    return name


def fits(prefix: str, name: str) -> bool:
    return demangled(name).lstrip("<&").removeprefix("mut ").startswith(prefix)


def read_budget(path: pathlib.Path) -> list[dict]:
    """`[[part]]` entries of a budget.toml, checked: names unique, every
    part but the last has `match`, the last is `other` without one."""
    parts = tomllib.loads(path.read_text(encoding="utf-8")).get("part", [])
    names = [p["name"] for p in parts]
    if not parts or names[-1] != OTHER or "match" in parts[-1]:
        raise ValueError(f"{path}: the last [[part]] is `{OTHER}`, with no match")
    if len(set(names)) != len(names) or any("match" not in p for p in parts[:-1]):
        raise ValueError(f"{path}: part names are unique and every part but `{OTHER}` has match")
    return parts


def attribute(items: list[dict], parts: list[dict]) -> dict[str, int]:
    """Bytes per part: each item to the first part a prefix of which fits
    one of its names, else `other`; the alignment padding before an item
    (less than its alignment) to `other`; a larger gap is not counted."""
    used = {p["name"]: 0 for p in parts}
    end = None
    for i in items:
        if end is not None and 0 < i["lma"] - end < i["align"]:
            used[OTHER] += i["lma"] - end
        part = next((p["name"] for p in parts[:-1]
                     if any(fits(m, n) for m in p["match"] for n in i["names"])), OTHER)
        used[part] += i["size"]
        end = i["lma"] + i["size"]
    return used


def parts_report(used: dict[str, int], parts: list[dict], image: int) -> tuple[str, list[str]]:
    """The parts list as text and why it does not pass (empty: it does)."""
    lines = [f"{'part':<16}{'bytes':>8}{'max':>8}"]
    lines += [f"{p['name']:<16}{used[p['name']]:>8}{p['max_bytes']:>8}" for p in parts]
    total = sum(used.values())
    lines.append(f"{'total':<16}{total:>8}{sum(p['max_bytes'] for p in parts):>8}")
    why = [f"part {p['name']}: {used[p['name']]} bytes, over its {p['max_bytes']}"
           for p in parts if used[p["name"]] > p["max_bytes"]]
    if total != image:
        why.append(f"the parts add up to {total} bytes, the image has {image}")
    return "\n".join(lines) + "\n", why


def check(elf_path: pathlib.Path, mcu_id: str, label: str = "",
          map_path: pathlib.Path | None = None) -> list[str]:
    """Checks one built image (and its parts, given its map); why it
    does not pass."""
    at, size = region(mcu_id)
    used = flash_bytes(elf_path.read_bytes(), at, size)
    name = f"{mcu_id} stage0{label}"
    print(f"{name}: {used} of {size} bytes ({size - used} free, margin {margin(size)})")
    why = verdict(used, size)
    failed = [f"{name}: {why}"] if why else []
    if map_path is not None:
        parts = read_budget(MCU / mcu_id / "stage0" / "budget.toml")
        items = map_items(map_path.read_text(encoding="utf-8", errors="replace"), at, size)
        text, bad = parts_report(attribute(items, parts), parts, used)
        print(text, end="")
        elf_path.with_name(elf_path.name + ".parts.txt").write_text(text, encoding="utf-8")
        failed += [f"{name}: {b}" for b in bad]
    return failed


def configurations(mcu_id: str) -> list[str | None]:
    """The boards a stage0 is built for: None (a node without a board),
    then every board whose definition names the MCU."""
    boards = []
    for f in sorted(BOARDS.glob("*/dagon-board.toml")):
        d = tomllib.loads(f.read_text(encoding="utf-8"))
        if d.get("mcu") == mcu_id:
            boards.append(d["id"])
    return [None, *boards]


def size_folder(mcu_id: str, board: str) -> pathlib.Path:
    """The boot folder (`DAGON_BOOT_CRATE`) of a node named `size` on
    `board`, as `dagon gen` writes a node's: its boot crate manifest, the
    MCU and board definitions and their hashes. The source tree carries
    one per board (`stage0/size/<board>/`; dagon-topology's tests check
    them and DAGON_BLESS=1 rewrites them)."""
    return MCU / mcu_id / "stage0" / "size" / board


def stage0_mcus() -> list[str]:
    out = []
    for d in sorted(MCU.glob("*/dagon-mcu.toml")):
        defn = tomllib.loads(d.read_text(encoding="utf-8"))
        if "stage0" in defn and (d.parent / "stage0").is_dir():
            out.append(d.parent.name)
    return out


def target_of(mcu: str) -> str:
    """The target the stage0 package's own cargo config names."""
    config = MCU / mcu / "stage0" / ".cargo" / "config.toml"
    return tomllib.loads(config.read_text(encoding="utf-8"))["build"]["target"]


def build_all() -> None:
    failed = []
    for mcu in stage0_mcus():
        pkg = MCU / mcu / "stage0"
        name = tomllib.loads((pkg / "Cargo.toml").read_text(encoding="utf-8"))["package"]["name"]
        for board in configurations(mcu):
            if board is None:
                folder, out = MCU / mcu / "bootcore" / "lint", pkg / "target"
            else:
                folder, out = size_folder(mcu, board), pkg / "target" / "stage0-size" / board
            env = dict(os.environ, DAGON_BOOT_CRATE=str(folder), CARGO_TARGET_DIR=str(out))
            env.setdefault("DAGON_ALLOW_TEST_KEY", "1")
            label = f" ({board or 'no board'})"
            elf = out / target_of(mcu) / "release" / name
            link_map = elf.with_name(name + ".map")
            build = ["cargo", "rustc", "--release", "--bin", name, "--",
                     "-C", f"link-arg=-Map={link_map}"]
            if subprocess.run(build, cwd=pkg, env=env).returncode:
                # Stage0's linker script refuses an image without its margin.
                failed.append(f"{mcu} stage0{label}: does not build (over its region less 1/16?)")
                continue
            failed += check(elf, mcu, label, link_map)
    if failed:
        sys.exit("\n".join(failed))


if __name__ == "__main__":
    if sys.argv[1:] == ["--targets"]:
        print(" ".join(sorted({target_of(m) for m in stage0_mcus()})))
    elif len(sys.argv) in (3, 4):
        given = pathlib.Path(sys.argv[3]) if len(sys.argv) == 4 else None
        why = check(pathlib.Path(sys.argv[1]), sys.argv[2], map_path=given)
        if why:
            sys.exit("\n".join(why))
    else:
        build_all()
