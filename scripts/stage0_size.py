"""Builds each MCU package's stage0 and checks it fits its region.

    python scripts/stage0_size.py            # build every stage0, check, print sizes
    python scripts/stage0_size.py <elf> <mcu id>   # check one built image
    python scripts/stage0_size.py --targets        # the targets they build for

An MCU whose definition (targets/mcu/<id>/dagon-mcu.toml) has `[layout]
stage0 = { at, size }` and whose package has a `stage0/` folder is built
in release for the board-less lint boot folder (`bootcore/lint`, the
stage0's own firmware lint input) and measured: the bytes its loadable
segments put into flash (code, read-only data and the initial data
image) must not exceed `size`. The linker refuses an image over its
region too; this records the size and keeps the limit's source the
definition.
"""
import os
import pathlib
import struct
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
MCU = ROOT / "targets" / "mcu"
PT_LOAD = 1


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


def check(elf_path: pathlib.Path, mcu_id: str) -> int:
    at, size = region(mcu_id)
    used = flash_bytes(elf_path.read_bytes(), at, size)
    print(f"{mcu_id} stage0: {used} of {size} bytes ({size - used} free)")
    if used > size:
        sys.exit(f"{mcu_id} stage0 is {used} bytes, over its {size}-byte region")
    return used


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
    for mcu in stage0_mcus():
        pkg = MCU / mcu / "stage0"
        env = dict(os.environ, DAGON_BOOT_CRATE=str(MCU / mcu / "bootcore" / "lint"))
        env.setdefault("DAGON_ALLOW_TEST_KEY", "1")
        subprocess.run(["cargo", "build", "--release"], cwd=pkg, env=env, check=True)
        meta = tomllib.loads((pkg / "Cargo.toml").read_text(encoding="utf-8"))
        elf = pkg / "target" / target_of(mcu) / "release" / meta["package"]["name"]
        check(elf, mcu)


if __name__ == "__main__":
    if sys.argv[1:] == ["--targets"]:
        print(" ".join(sorted({target_of(m) for m in stage0_mcus()})))
    elif len(sys.argv) == 3:
        check(pathlib.Path(sys.argv[1]), sys.argv[2])
    else:
        build_all()
