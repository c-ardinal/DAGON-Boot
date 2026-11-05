"""Builds each MCU package's stage0 and checks it fits its region.

    python scripts/stage0_size.py            # build every stage0, check, print sizes
    python scripts/stage0_size.py <elf> <mcu id>   # check one built image
    python scripts/stage0_size.py --targets        # the targets they build for

An MCU whose definition (targets/mcu/<id>/dagon-mcu.toml) has `[layout]
stage0 = { at, size }` and whose package has a `stage0/` folder is built
in release once for a node without a board (the stage0's firmware lint
boot folder, `bootcore/lint`) and once for every board whose definition
(targets/boards/<id>/dagon-board.toml) names that MCU: the board puts its
boot LED and halt pattern into stage0, so each is a different image. Each
is measured: the bytes its loadable segments put into flash (code,
read-only data and the initial data image) must leave `margin(size)`
bytes of the region free. The linker refuses an image over its region;
this keeps the limit's source the definition and fails before the limit.
"""
import os
import pathlib
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
# for every new node with whatever toolchain is current; a new compiler or
# library release, a board's boot pins or a needed fix moves an opt-level
# "z" image by hundreds of bytes, and an image that fits only today turns
# into a first write that cannot be built (as the Waveshare build did, 64
# bytes over, while the board-less build fitted).
MARGIN_DIVISOR = 16


def region(mcu_id: str) -> tuple[int, int]:
    """`[layout] stage0` of the MCU definition: (at, size)."""
    d = tomllib.loads((MCU / mcu_id / "dagon-mcu.toml").read_text(encoding="utf-8"))
    s = d["layout"]["stage0"]
    return s["at"], s["size"]


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


def check(elf_path: pathlib.Path, mcu_id: str, label: str = "") -> str | None:
    at, size = region(mcu_id)
    used = flash_bytes(elf_path.read_bytes(), at, size)
    name = f"{mcu_id} stage0{label}"
    print(f"{name}: {used} of {size} bytes ({size - used} free, margin {margin(size)})")
    why = verdict(used, size)
    return f"{name}: {why}" if why else None


def configurations(mcu_id: str) -> list[str | None]:
    """The boards a stage0 is built for: None (a node without a board),
    then every board whose definition names the MCU."""
    boards = []
    for f in sorted(BOARDS.glob("*/dagon-board.toml")):
        d = tomllib.loads(f.read_text(encoding="utf-8"))
        if d.get("mcu") == mcu_id:
            boards.append(d["id"])
    return [None, *boards]


def boot_folder_toml(mcu_id: str, board: str) -> str:
    """A boot folder (`DAGON_BOOT_CRATE`) for a node named `size` on
    `board`: all a stage0 build reads of it."""
    return (
        "[package]\nname = \"dagon-stage0-size\"\nversion = \"0.0.0\"\n"
        "edition = \"2021\"\npublish = false\n\n"
        "[package.metadata.dagon.boot]\n"
        f"node = \"size\"\nmcu = \"{mcu_id}\"\nboard = \"{board}\"\n"
    )


def stage0_mcus() -> list[str]:
    out = []
    for d in sorted(MCU.glob("*/dagon-mcu.toml")):
        defn = tomllib.loads(d.read_text(encoding="utf-8"))
        if "stage0" in defn.get("layout", {}) and (d.parent / "stage0").is_dir():
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
                folder = pkg / "target" / "stage0-size" / board
                folder.mkdir(parents=True, exist_ok=True)
                (folder / "Cargo.toml").write_text(boot_folder_toml(mcu, board), encoding="utf-8")
                out = folder / "target"
            env = dict(os.environ, DAGON_BOOT_CRATE=str(folder), CARGO_TARGET_DIR=str(out))
            env.setdefault("DAGON_ALLOW_TEST_KEY", "1")
            label = f" ({board or 'no board'})"
            if subprocess.run(["cargo", "build", "--release"], cwd=pkg, env=env).returncode:
                # The linker refuses an image over its region.
                failed.append(f"{mcu} stage0{label}: does not build (over its region?)")
                continue
            why = check(out / target_of(mcu) / "release" / name, mcu, label)
            if why:
                failed.append(why)
    if failed:
        sys.exit("\n".join(failed))


if __name__ == "__main__":
    if sys.argv[1:] == ["--targets"]:
        print(" ".join(sorted({target_of(m) for m in stage0_mcus()})))
    elif len(sys.argv) == 3:
        why = check(pathlib.Path(sys.argv[1]), sys.argv[2])
        if why:
            sys.exit(why)
    else:
        build_all()
