"""Builds each boot core and relay firmware for a call graph and checks the
wire codec in it (the `dagon_proto` crate) against the budgets its MCU
definition gives: flash, worst-case stack and static RAM.

    python scripts/codec_budget.py                       # build every image, check, print
    python scripts/codec_budget.py <elf> <mcu id> <image>  # check one built image
    python scripts/codec_budget.py --targets             # the targets they build for

The budgets are the definition's data (targets/mcu/<id>/dagon-mcu.toml):
`[codec.bootcore]` and `[codec.relay]`, each `{ flash_bytes, stack_bytes,
ram_bytes }`. The images are those the boot core size check
(bootcore_size.py: every board of the MCU and none) and the relay RAM check
(relay_ram.py) build, from the same packages and boot folders; an MCU with
such an image and no budget for it fails.

Each is built a second time, with `-Z emit-stack-sizes` (rustc's own record
of every function's stack frame, read on the pinned toolchain through
RUSTC_BOOTSTRAP): the flag moves the layout by a few bytes, so the images
the size checks measure are not these. From that ELF:

- flash: the sizes of the codec's function and data symbols placed in a
  loadable, non-writable section (code the compiler inlined into a caller
  counts for the caller);
- static RAM: the sizes of the codec's data symbols in a writable section;
- stack: the deepest path of the call graph from any codec function (but
  its `core::fmt` impls, `is_root`), each function's frame added once per
  call on the path. The graph is read from the code: every Thumb call and
  branch out of a function. A call or branch whose target is no function
  start, an indirect call or branch (but a handler row's, `dispatch`), a
  function with no frame record whose code does not give its frame, and
  recursion each fail the image: the figure is then not a bound. What a
  handler row's action takes is the node's, not the codec's.

A symbol is the codec's when the outermost crate of its Rust (v0) symbol
name is `dagon_proto`: its own functions, their generic copies and the
closures in them, not a function of another crate the codec calls.
"""
import os
import pathlib
import re
import struct
import subprocess
import sys

import bootcore_size
import relay_ram
import stage0_size

CRATE = "dagon_proto"
IMAGES = ("bootcore", "relay")
FIELDS = ("flash_bytes", "stack_bytes", "ram_bytes")
STACK_FLAGS = '["-Zemit-stack-sizes"]'
# The one indirect call the codec makes: a handler row (`Handler`'s
# methods, in `handler.rs`) calls its decoder and its action through the
# row's function pointers. Its decoder is `Req::decoder(cmd)` (a private
# field only `Handler::new` sets), a function of `payload::decode`.
HANDLER = re.compile(r"11dagon_proto7handlerINtB[0-9a-zA-Z]*_7Handler")
DECODERS = re.compile(r"11dagon_proto7payload6decode\d")
SHT_SYMTAB, SHF_WRITE, SHF_ALLOC = 2, 1, 2
STT_FUNC, STT_OBJECT = 2, 1


# --- ELF ------------------------------------------------------------------

def sections(elf: bytes) -> dict[str, tuple]:
    """Section headers by name, and all of them in order under "": (name
    offset, type, flags, addr, offset, size, link, info, align, entsize)."""
    if elf[:4] != b"\x7fELF" or elf[4] != 1 or elf[5] != 1:
        raise ValueError("not a 32-bit little-endian ELF")
    shoff, = struct.unpack_from("<I", elf, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", elf, 0x2E)
    heads = [struct.unpack_from("<10I", elf, shoff + i * shentsize) for i in range(shnum)]
    names = heads[shstrndx][4]

    def name(h):
        return elf[names + h[0]:elf.index(b"\0", names + h[0])].decode()

    return {name(h): h for h in heads} | {"": heads}


def symbols(elf: bytes) -> list[tuple[str, int, int, int, int]]:
    """Every symbol: (name, value, size, type, section index)."""
    heads = sections(elf)[""]
    out = []
    for h in heads:
        if h[1] != SHT_SYMTAB:
            continue
        strtab = heads[h[6]][4]
        for o in range(h[4], h[4] + h[5], 16):
            nm, value, size, info, _, shndx = struct.unpack_from("<IIIBBH", elf, o)
            out.append((elf[strtab + nm:elf.index(b"\0", strtab + nm)].decode(),
                        value, size, info & 15, shndx))
    return out


def uleb(data: bytes, at: int) -> tuple[int, int]:
    value = shift = 0
    while True:
        b = data[at]
        at += 1
        value |= (b & 0x7F) << shift
        shift += 7
        if b < 0x80:
            return value, at


def frames(elf: bytes) -> dict[int, int]:
    """`.stack_sizes`: each function's frame by its address (Thumb bit
    cleared)."""
    s = sections(elf).get(".stack_sizes")
    out = {}
    if s:
        at = s[4]
        while at < s[4] + s[5]:
            addr, = struct.unpack_from("<I", elf, at)
            out[addr & ~1], at = uleb(elf, at + 4)
    return out


# --- names ----------------------------------------------------------------

CRATE_ROOT = re.compile(r"C(?:s[0-9a-zA-Z]*_)?(\d+)")


def crate(name: str) -> str | None:
    """The outermost crate of a Rust v0 symbol name: the crate root its
    leftmost path starts from (None: not a v0 name)."""
    if not name.startswith("_R"):
        return None
    m = CRATE_ROOT.search(name, 2)
    if not m:
        return None
    n = int(m[1])
    return name[m.end():m.end() + n]


# --- call graph -----------------------------------------------------------

class Function:
    """One function of the image: its name, its code from `start` to `end`
    and the data inside that code (literal pools, jump tables)."""

    def __init__(self, name: str, start: int, code: bytes, data: list[tuple[int, int]]):
        self.name, self.start, self.end, self.code, self.data = (
            name, start, start + len(code), code, data)

    def in_data(self, addr: int) -> bool:
        return any(a <= addr < b for a, b in self.data)

    def halfwords(self) -> tuple[int, ...]:
        return struct.unpack_from(f"<{len(self.code) // 2}H", self.code)


MAPPING = re.compile(r"\$[dt](\..*)?")


def functions(elf: bytes) -> dict[int, Function]:
    """Every function of the image by its start, with its code and the
    data inside it (from the `$d` and `$t` mapping symbols)."""
    heads = sections(elf)[""]
    syms = symbols(elf)
    maps = sorted((v, n[1]) for n, v, _, _, _ in syms if MAPPING.fullmatch(n))
    out = {}
    for name, value, size, kind, shndx in syms:
        if kind != STT_FUNC or not size or shndx >= len(heads):
            continue
        h = heads[shndx]
        start = value & ~1
        code = elf[h[4] + start - h[3]:h[4] + start - h[3] + size]
        data, mark = [], None
        for addr, k in maps:
            if addr >= start + size:
                break
            if k == "d" and mark is None:
                mark = max(addr, start)
            elif k == "t" and mark is not None:
                if addr > mark:
                    data.append((mark, addr))
                mark = None
        if mark is not None:
            data.append((mark, start + size))
        out[start] = Function(name, start, code, data)
    return out


def sign(value: int, bits: int) -> int:
    return value - (1 << bits) if value & (1 << (bits - 1)) else value


def wide(h: int) -> bool:
    """Whether the halfword `h` starts a 32-bit Thumb instruction."""
    return h >> 11 in (0x1D, 0x1E, 0x1F)


def transfers(f: Function) -> list[tuple[str, int | None, int]]:
    """Each transfer of control in `f`'s code that may leave it: (kind,
    target, at). Kinds: `call` (BL), `branch` (B, B<c>, B.W, B<c>.W: out of
    the function a tail call), `indirect` (BLX or BX to a register other
    than LR, MOV or ADD to PC, LDR to PC) and `unknown` (BLX to an address:
    ARM state, which no M profile core runs). Returns (POP or LDM with PC,
    BX LR) are not transfers."""
    out = []
    hw = f.halfwords()
    i = 0
    while i < len(hw):
        at = f.start + 2 * i
        h = hw[i]
        if f.in_data(at):
            i += 1
            continue
        if wide(h) and i + 1 < len(hw):
            out += wide_transfer(h, hw[i + 1], at)
            i += 2
            continue
        i += 1
        pc = at + 4
        if h & 0xF000 == 0xD000 and (h >> 8) & 0xF < 0xE:
            out.append(("branch", pc + sign(h & 0xFF, 8) * 2, at))
        elif h & 0xF800 == 0xE000:
            out.append(("branch", pc + sign(h & 0x7FF, 11) * 2, at))
        elif h & 0xFF87 == 0x4487 and f.in_data(at + 2):
            out += table_jump(f, at)
        elif h & 0xFF87 == 0x4780 or h & 0xFF87 in (0x4687, 0x4487):
            out.append(("indirect", None, at))
        elif h & 0xFF87 == 0x4700 and (h >> 3) & 0xF != 14:
            out.append(("indirect", None, at))
    return out


def table_jump(f: Function, at: int) -> list[tuple[str, int | None, int]]:
    """The branches of an ARMv6-M jump table: LLVM's ADD PC, Rm with the
    byte table right after it, each entry half the distance from PC (at +
    4) to its case. Every case is inside the function (else the jump is
    `unknown`)."""
    end = next(b for a, b in f.data if a <= at + 2 < b)
    cases = f.code[at + 2 - f.start:end - f.start]
    targets = {at + 4 + 2 * b for b in cases}
    if all(f.start <= t < f.end for t in targets):
        return [("branch", t, at) for t in sorted(targets)]
    return [("unknown", None, at)]


def wide_transfer(h: int, h2: int, at: int) -> list[tuple[str, int | None, int]]:
    """The transfer a 32-bit Thumb instruction makes, if any."""
    pc = at + 4
    if h & 0xF800 == 0xF000 and h2 & 0x8000:
        s = (h >> 10) & 1
        if h2 & 0x5000 == 0:  # B<c>.W (T3); condition 111x: a control instruction
            if (h >> 6) & 0xE == 0xE:
                return []
            imm = ((s << 20) | (((h2 >> 11) & 1) << 19) | (((h2 >> 13) & 1) << 18)
                   | ((h & 0x3F) << 12) | ((h2 & 0x7FF) << 1))
            return [("branch", pc + sign(imm, 21), at)]
        j1, j2 = (h2 >> 13) & 1, (h2 >> 11) & 1
        imm = ((s << 24) | ((1 - (j1 ^ s)) << 23) | ((1 - (j2 ^ s)) << 22)
               | ((h & 0x3FF) << 12) | ((h2 & 0x7FF) << 1))
        kind = {0xD000: "call", 0x9000: "branch", 0xC000: "unknown"}[h2 & 0xD000]
        return [(kind, pc + sign(imm, 25), at)]
    # LDR to PC, but POP {PC} (LDR PC, [SP], #4).
    loads_pc = (h2 >> 12 == 0xF and h & 0xFFF0 in (0xF850, 0xF8D0)
                and not (h == 0xF85D and h2 & 0x0FFF == 0x0B04))
    # LDM with PC from another base than SP (from SP it is a return).
    loads_pc |= h & 0xFFD0 in (0xE890, 0xE910) and bool(h2 & 0x8000) and h & 0xF != 13
    return [("indirect", None, at)] if loads_pc else []


def prologue_frame(f: Function) -> int | None:
    """The frame of a function with no `.stack_sizes` record (code the
    toolchain ships prebuilt, compiler-builtins), from its code: what its
    PUSH and SUB SP instructions take, added. None when it moves SP in a
    way this does not read (from a register, a wide SUB)."""
    total, i = 0, 0
    hw = f.halfwords()
    while i < len(hw):
        h = hw[i]
        if f.in_data(f.start + 2 * i):
            i += 1
            continue
        if wide(h) and i + 1 < len(hw):
            h2 = hw[i + 1]
            if h == 0xE92D:  # PUSH.W
                total += 4 * bin(h2).count("1")
            elif h & 0xFBEF == 0xF1AD and (h2 >> 8) & 0xF == 0xD:  # SUB.W SP
                return None
            elif h == 0xF84D and h2 & 0x0FFF == 0x0D04:  # STR Rt, [SP, #-4]!
                total += 4
            i += 2
            continue
        if h & 0xFE00 == 0xB400:
            total += 4 * bin(h & 0x1FF).count("1")
        elif h & 0xFF80 == 0xB080:
            total += 4 * (h & 0x7F)
        elif h & 0xFF87 in (0x4685, 0x4485):  # MOV SP, Rm / ADD SP, Rm
            return None
        i += 1
    return total


def reenters(f: Function, kind: str) -> bool:
    """Whether a `kind` transfer to `f`'s own start calls `f` again: a call
    always; a branch when `f` starts with a PUSH (it runs its prologue
    again: a tail call to itself, not a loop)."""
    h = f.halfwords()[0] if f.code else 0
    return kind == "call" or h & 0xFE00 == 0xB400 or h == 0xE92D


def is_codec(name: str) -> bool:
    return crate(name) == CRATE


def is_root(name: str) -> bool:
    """Whether the stack is measured from function `name`: a codec
    function but its `core::fmt` trait impls (Debug, Display: text for a
    person, run through `core::fmt`'s `dyn Write` by the caller's
    diagnostics, never on the wire)."""
    return is_codec(name) and not (name.startswith("_RNvX") and "4core3fmt" in name)


def dispatch(funcs: dict[int, Function], f: Function) -> set[int] | None:
    """The functions an indirect call of a handler row (`HANDLER`) reaches:
    its decoder, one of `DECODERS`; its action is the node's code, past the
    codec, so it adds nothing here. None: `f` is no handler row."""
    if not (is_codec(f.name) and HANDLER.search(f.name)):
        return None
    return {a for a, g in funcs.items() if is_codec(g.name) and DECODERS.search(g.name)}


def deepest(funcs: dict[int, Function], recorded: dict[int, int]) -> tuple[int, str, list[str]]:
    """The deepest stack from any codec function of `funcs`, with the
    frames `recorded`: (bytes, the function it starts from, why the figure
    is not a bound)."""
    why = []

    def frame(f: Function) -> int:
        if f.start in recorded:
            return recorded[f.start]
        v = prologue_frame(f)
        if v is None:
            why.append(f"{f.name}: no frame record, and its code does not give its frame")
            return 0
        return v

    def callees(f: Function) -> set[int]:
        out = set()
        for kind, target, at in transfers(f):
            if kind == "indirect":
                rows = dispatch(funcs, f)
                if rows is None:
                    why.append(f"{f.name}: indirect call at {at:#x}")
                out |= rows or set()
            elif kind == "unknown":
                why.append(f"{f.name}: a transfer at {at:#x} to no known target")
            elif not f.start <= target < f.end or (target == f.start and reenters(f, kind)):
                if target in funcs:
                    out.add(target)
                else:
                    why.append(f"{f.name}: {kind} at {at:#x} to {target:#x}, no function start")
        return out

    depth, busy = {}, set()

    def deep(a: int) -> int:
        if a in depth:
            return depth[a]
        if a in busy:
            why.append(f"recursion at {funcs[a].name}")
            return 0
        busy.add(a)
        f = funcs[a]
        depth[a] = frame(f) + max((deep(t) for t in callees(f)), default=0)
        busy.discard(a)
        return depth[a]

    best, root = 0, ""
    for a, f in sorted(funcs.items()):
        if is_root(f.name) and deep(a) > best:
            best, root = depth[a], f.name
    return best, root, sorted(set(why))


# --- measure and check ----------------------------------------------------

def measure(elf: bytes) -> tuple[dict[str, int], str, list[str]]:
    """The codec's bytes in the image, by budget field; the function its
    deepest stack starts from; why the stack figure is not a bound."""
    heads = sections(elf)[""]
    flash = ram = 0
    for name, _, size, kind, shndx in symbols(elf):
        if kind not in (STT_FUNC, STT_OBJECT) or shndx >= len(heads) or not is_codec(name):
            continue
        flags = heads[shndx][2]
        if not flags & SHF_ALLOC:
            continue
        if flags & SHF_WRITE:
            ram += size
        else:
            flash += size
    depth, root, why = deepest(functions(elf), frames(elf))
    return {"flash_bytes": flash, "stack_bytes": depth, "ram_bytes": ram}, root, why


def budget(mcu_id: str, image: str) -> dict | None:
    """`[codec.<image>]` of the MCU definition (None: it gives none)."""
    return stage0_size.definition(mcu_id).get("codec", {}).get(image)


def verdict(used: dict[str, int], limits: dict) -> list[str]:
    """Why `used` does not pass `limits` (empty: it does)."""
    return [f"{field.removesuffix('_bytes')} {used[field]} bytes, over its {limits[field]}"
            for field in FIELDS if used[field] > limits[field]]


def check(elf_path: pathlib.Path, mcu_id: str, image: str, label: str = "") -> list[str]:
    """Checks the codec in one built image; why it does not pass."""
    name = f"{mcu_id} {image} codec{label}"
    limits = budget(mcu_id, image)
    if limits is None:
        return [f"{name}: the definition has no [codec.{image}]"]
    used, root, why = measure(elf_path.read_bytes())
    print(name + ": " + ", ".join(
        f"{f.removesuffix('_bytes')} {used[f]} of {limits[f]}" for f in FIELDS)
        + f" (deepest from {root or 'no codec function'})", flush=True)
    return [f"{name}: {w}" for w in verdict(used, limits) + why]


def builds() -> list[tuple[str, str, str | None]]:
    """(MCU, image, board) of every image built: each boot core the boot
    core size check builds and each relay firmware the relay RAM check
    builds."""
    out = [(m, "bootcore", b) for m in bootcore_size.bootcore_mcus()
           for b in stage0_size.configurations(m)]
    return out + [(m, "relay", None) for m in relay_ram.relay_mcus()]


def build(mcu: str, image: str, board: str | None) -> pathlib.Path | None:
    """Builds one image with its frame records; its ELF (None: it did not
    build)."""
    env = dict(os.environ, DAGON_TARGET_TAG="size", RUSTC_BOOTSTRAP="1")
    env.setdefault("DAGON_ALLOW_TEST_KEY", "1")
    if image == "bootcore":
        pkg, target, example = bootcore_size.package(mcu), bootcore_size.target_of(mcu), "size"
        env["DAGON_BOOT_CRATE"] = str(bootcore_size.folder_of(mcu, board))
    else:
        pkg, target, example = relay_ram.package(mcu), relay_ram.target_of(mcu), "relay"
    out = pkg / "target" / "codec-budget" / (board or "no-board")
    env["CARGO_TARGET_DIR"] = str(out)
    cmd = ["cargo", "build", "--release", "--example", example, "--target", target,
           "--config", f"target.{target}.rustflags={STACK_FLAGS}"]
    if subprocess.run(cmd, cwd=pkg, env=env).returncode:
        return None
    return out / target / "release" / "examples" / example


def build_all() -> None:
    failed = []
    for mcu, image, board in builds():
        label = f" ({board or 'no board'})" if image == "bootcore" else ""
        if budget(mcu, image) is None:
            failed.append(f"{mcu} {image} codec: the definition has no [codec.{image}]")
            continue
        elf = build(mcu, image, board)
        if elf is None:
            failed.append(f"{mcu} {image} codec{label}: does not build")
            continue
        failed += check(elf, mcu, image, label)
    if failed:
        sys.exit("\n".join(failed))


if __name__ == "__main__":
    if sys.argv[1:] == ["--targets"]:
        print(" ".join(sorted({bootcore_size.target_of(m) for m in bootcore_size.bootcore_mcus()}
                              | {relay_ram.target_of(m) for m in relay_ram.relay_mcus()})))
    elif len(sys.argv) == 4 and sys.argv[3] in IMAGES:
        why = check(pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3])
        if why:
            sys.exit("\n".join(why))
    elif sys.argv[1:]:
        sys.exit(__doc__)
    else:
        build_all()
