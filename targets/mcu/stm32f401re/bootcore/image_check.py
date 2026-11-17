"""This boot core's own check of a built image, which
scripts/bootcore_size.py runs for every configuration it builds:

    python image_check.py <elf> <boot folder>

The listen window and update mode sleep with WFE while the I2C event
interrupt of the link is raised (`dagon_mcu_stm32f401re::boot_link`). If
that line ever reaches the core as an exception, its vector must be a
handler that silences it, never cortex-m-rt's DefaultHandler (a `loop {}`
that only the watchdog ends). So for a node whose board has a DAGON I2C
bus, the image's `<CONTROLLER>_EV` and `_ER` vectors resolve to their own
handlers. Prints each finding; exits 1 when there is one.
"""
import pathlib
import struct
import sys
import tomllib

SHT_SYMTAB = 2


def symbols(elf: bytes) -> dict[str, int]:
    """Name to value of an ELF32 little-endian image's symbol table."""
    shoff, = struct.unpack_from("<I", elf, 0x20)
    shentsize, shnum, _ = struct.unpack_from("<HHH", elf, 0x2E)
    secs = [struct.unpack_from("<IIIIIIIIII", elf, shoff + i * shentsize) for i in range(shnum)]
    out = {}
    for sec in secs:
        if sec[1] != SHT_SYMTAB:
            continue
        strtab = secs[sec[6]]
        for i in range(sec[5] // 16):
            n, value, *_ = struct.unpack_from("<IIIBBH", elf, sec[4] + i * 16)
            o = strtab[4] + n
            out[elf[o:elf.index(b"\0", o)].decode("ascii", "replace")] = value & ~1
    return out


def link_controller(folder: pathlib.Path) -> str | None:
    """The controller of the board's DAGON I2C bus (None: no board, or
    none such)."""
    board = folder / "board.toml"
    if not board.is_file():
        return None
    b = tomllib.loads(board.read_text(encoding="utf-8"))
    bus = next((x for x in b.get("bus", []) if x.get("dagon") and x.get("kind") == "i2c"), None)
    return bus["controller"].upper() if bus else None


def findings(elf: bytes, folder: pathlib.Path) -> list[str]:
    ctl = link_controller(folder)
    if ctl is None:
        return []
    syms = symbols(elf)
    default = {syms[n] for n in ("DefaultHandler", "DefaultHandler_") if n in syms}
    out = []
    for line in (f"{ctl}_EV", f"{ctl}_ER"):
        if line not in syms:
            out.append(f"no vector {line} in the image")
        elif syms[line] in default:
            out.append(f"vector {line} is DefaultHandler: a wake-up taken as an exception hangs the boot core")
    return out


if __name__ == "__main__":
    found = findings(pathlib.Path(sys.argv[1]).read_bytes(), pathlib.Path(sys.argv[2]))
    for f in found:
        print(f)
    sys.exit(1 if found else 0)
