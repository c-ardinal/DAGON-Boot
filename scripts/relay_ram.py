"""Builds each MCU package's relay firmware and checks its static RAM
against the budget its MCU definition gives.

    python scripts/relay_ram.py                 # build every relay firmware, check, print sizes
    python scripts/relay_ram.py <elf> <mcu id>  # check one built firmware
    python scripts/relay_ram.py --targets       # the targets they build for

The budget is the definition's data (targets/mcu/<id>/dagon-mcu.toml):
`[relay] ram_bytes`, the bytes the relaying app firmware's static data
(`.bss` and `.data`) may take. An MCU whose definition has no
`[[relay_link]]` relays over nothing and has no relay firmware to build;
one that has them needs the budget and the package's `examples/relay.rs`
(the relay with every relay link wired and no app tasks of its own).

The example is built in release for the target the package's firmware
lint names (`[package.metadata.dagon] lint.target`).
"""
import os
import pathlib
import struct
import subprocess
import sys
import tomllib

import stage0_size

MCU = stage0_size.MCU
EXAMPLE = "relay"
SECTIONS = (".bss", ".data")


def budget(mcu_id: str) -> int | None:
    """The bytes the relay firmware's static data may take on `mcu_id`
    (None: the definition gives none)."""
    return stage0_size.definition(mcu_id).get("relay", {}).get("ram_bytes")


def relays(mcu_id: str) -> bool:
    """Whether a node of `mcu_id` relays (its definition has relay links)."""
    return bool(stage0_size.definition(mcu_id).get("relay_link"))


def ram_bytes(elf: bytes) -> int:
    """The sizes of the ELF32 little-endian image's `.bss` and `.data`
    sections, added."""
    if elf[:4] != b"\x7fELF" or elf[4] != 1 or elf[5] != 1:
        raise ValueError("not a 32-bit little-endian ELF")
    shoff, = struct.unpack_from("<I", elf, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", elf, 0x2E)
    headers = [struct.unpack_from("<IIIIII", elf, shoff + i * shentsize) for i in range(shnum)]
    names = headers[shstrndx][4]

    def name(h):
        start = names + h[0]
        return elf[start:elf.index(b"\0", start)].decode()

    return sum(h[5] for h in headers if name(h) in SECTIONS)


def verdict(used: int, size: int) -> str | None:
    """Why `used` bytes do not pass a `size`-byte budget (None: they do)."""
    return f"{used} bytes of static RAM, over its {size}-byte budget" if used > size else None


def check(elf_path: pathlib.Path, mcu_id: str) -> list[str]:
    """Checks one built firmware; why it does not pass."""
    size = budget(mcu_id)
    name = f"{mcu_id} relay firmware"
    if size is None:
        return [f"{name}: the definition has no [relay] ram_bytes"]
    used = ram_bytes(elf_path.read_bytes())
    print(f"{name}: {used} of {size} bytes of static RAM ({size - used} free)")
    why = verdict(used, size)
    return [f"{name}: {why}"] if why else []


def mcus() -> list[str]:
    return [d.parent.name for d in sorted(MCU.glob("*/dagon-mcu.toml"))]


def package(mcu_id: str) -> pathlib.Path:
    return MCU / mcu_id


def relay_mcus() -> list[str]:
    return [m for m in mcus() if relays(m)]


def target_of(mcu_id: str) -> str:
    """The target the package's firmware lint names."""
    manifest = tomllib.loads((package(mcu_id) / "Cargo.toml").read_text(encoding="utf-8"))
    return manifest["package"]["metadata"]["dagon"]["lint"]["target"]


def build_all() -> None:
    failed = []
    for mcu in mcus():
        if not relays(mcu):
            print(f"{mcu}: no [[relay_link]] in its definition, no relay firmware to build")
            continue
        if budget(mcu) is None:
            failed.append(f"{mcu}: the definition has relay links but no [relay] ram_bytes")
            continue
        pkg = package(mcu)
        if not (pkg / "examples" / f"{EXAMPLE}.rs").is_file():
            failed.append(f"{mcu}: the definition has relay links, "
                          f"but {pkg.relative_to(stage0_size.ROOT)} has no examples/{EXAMPLE}.rs")
            continue
        out = pkg / "target" / "relay-ram"
        env = dict(os.environ, CARGO_TARGET_DIR=str(out), DAGON_TARGET_TAG="size")
        env.setdefault("DAGON_ALLOW_TEST_KEY", "1")
        target = target_of(mcu)
        build = ["cargo", "build", "--release", "--example", EXAMPLE, "--target", target]
        if subprocess.run(build, cwd=pkg, env=env).returncode:
            failed.append(f"{mcu} relay firmware: does not build")
            continue
        failed += check(out / target / "release" / "examples" / EXAMPLE, mcu)
    if failed:
        sys.exit("\n".join(failed))


if __name__ == "__main__":
    if sys.argv[1:] == ["--targets"]:
        print(" ".join(sorted({target_of(m) for m in relay_mcus()})))
    elif len(sys.argv) == 3:
        why = check(pathlib.Path(sys.argv[1]), sys.argv[2])
        if why:
            sys.exit("\n".join(why))
    else:
        build_all()
