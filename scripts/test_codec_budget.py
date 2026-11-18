"""codec_budget.py: the budgets come from the MCU definition's `[codec]`, the
codec is the `dagon_proto` crate's symbols, its stack is the deepest path of
the call graph, and an image over a budget, with an indirect call, a call it
cannot follow or recursion fails.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import pathlib
import struct
import tempfile
import unittest

import bootcore_size
import codec_budget
import relay_ram

TEXT, RAM = 0x1000_0000, 0x2000_0000
PUSH_R7_LR, POP_R7_PC, BX_LR, NOP = 0xB580, 0xBD80, 0x4770, 0x46C0
BLX_R3 = 0x4798


def codec(name: str) -> str:
    """A v0 name of a function `name` of dagon_proto."""
    return f"_RNvCs1a_11dagon_proto{len(name)}{name}"


def other(name: str) -> str:
    return f"_RNvCs2b_11dagon_agent{len(name)}{name}"


def bl(at: int, to: int) -> list[int]:
    """BL from `at` to `to`, as two halfwords."""
    off = to - (at + 4)
    s = 1 if off < 0 else 0
    i1, i2 = (off >> 23) & 1, (off >> 22) & 1
    j1, j2 = (1 - i1) ^ s, (1 - i2) ^ s
    return [0xF000 | (s << 10) | ((off >> 12) & 0x3FF),
            0xD000 | (j1 << 13) | (j2 << 11) | ((off >> 1) & 0x7FF)]


def image(funcs: list[tuple[str, list[int], int | None]],
          objects: list[tuple[str, int, bool]] = ()) -> bytes:
    """An ELF32 image: functions (name, halfwords, frame record or None)
    one after the other from TEXT, and data objects (name, size, in RAM)."""
    code, syms, records, at = b"", [], b"", TEXT
    for name, hw, frame in funcs:
        body = struct.pack(f"<{len(hw)}H", *hw)
        syms.append((name, at | 1, len(body), 2, 1))
        if frame is not None:
            records += struct.pack("<IB", at | 1, frame)
        code += body
        at += len(body)
    ram = rodata = 0
    for name, size, in_ram in objects:
        if in_ram:
            syms.append((name, RAM + ram, size, 1, 2))
            ram += size
        else:
            syms.append((name, at + rodata, size, 1, 1))
            rodata += size
    code += b"\0" * rodata
    strtab = b"\0"
    entries = struct.pack("<IIIBBH", 0, 0, 0, 0, 0, 0)
    for name, value, size, kind, shndx in syms:
        entries += struct.pack("<IIIBBH", len(strtab), value, size, 0x10 | kind, 0, shndx)
        strtab += name.encode() + b"\0"
    names = b"\0.text\0.bss\0.stack_sizes\0.symtab\0.strtab\0.shstrtab\0"
    blobs = [code, b"", records, entries, strtab, names]
    offsets, off = [], 52
    for b in blobs:
        offsets.append(off)
        off += len(b)
    shoff = off
    sh = [struct.pack("<10I", *[0] * 10),
          struct.pack("<10I", 1, 1, 6, TEXT, offsets[0], len(code), 0, 0, 2, 0),
          struct.pack("<10I", 7, 8, 3, RAM, offsets[1], ram, 0, 0, 4, 0),
          struct.pack("<10I", 12, 1, 0x80, 0, offsets[2], len(records), 1, 0, 1, 0),
          struct.pack("<10I", 25, 2, 0, 0, offsets[3], len(entries), 5, 1, 4, 16),
          struct.pack("<10I", 33, 3, 0, 0, offsets[4], len(strtab), 0, 0, 1, 0),
          struct.pack("<10I", 41, 3, 0, 0, offsets[5], len(names), 0, 0, 1, 0)]
    head = (b"\x7fELF\x01\x01\x01" + b"\0" * 9
            + struct.pack("<HHIIIIIHHHHHH", 2, 40, 1, TEXT, 0, shoff, 0, 52, 0, 0, 40, len(sh), 6))
    return head + b"".join(blobs) + b"".join(sh)


def chain(extra_b: list[int] | None = None, a_calls_c: bool = False) -> bytes:
    """Codec `a` (frame 16) calls codec `b` (frame 8), which calls agent
    `c` (frame 24, a leaf); `extra_b` is planted into `b`."""
    a_at, b_at = TEXT, TEXT + 8
    b_body = [PUSH_R7_LR, *(extra_b or [NOP, NOP]), POP_R7_PC, NOP]
    c_at = b_at + 2 * (len(b_body) + 2)
    a = [PUSH_R7_LR, *bl(a_at + 2, c_at if a_calls_c else b_at), POP_R7_PC]
    b = [b_body[0], *bl(b_at + 2, c_at), *b_body[1:]]
    return image([(codec("a"), a, 16), (codec("b"), b, 8), (other("c"), [BX_LR, NOP], 24)])


class Names(unittest.TestCase):
    def test_the_crate_is_the_outermost_crate_of_a_v0_name(self):
        self.assertEqual(codec_budget.crate("_RNvNtCseKybgR0jsQZ_11dagon_proto5field4read"),
                         "dagon_proto")
        # A core function made generic over a codec type is core's.
        self.assertEqual(codec_budget.crate(
            "_RNvXs2_NtNtCseold96UKqg_4core5slice5indexINtNtNtB9_3ops5range5RangejE"
            "INtB5_10SliceIndexShE9index_mutCseKybgR0jsQZ_11dagon_proto"), "core")
        # A codec method copied for a node's crate is the codec's.
        self.assertEqual(codec_budget.crate(
            "_RNvMs_NtCseKybgR0jsQZ_11dagon_proto7payloadINtB4_4BitsKjf_E5union"
            "Csg5QEBRMZCIS_21dagon_rp2040_bootcore"), "dagon_proto")
        self.assertIsNone(codec_budget.crate("memcpy"))


class Stack(unittest.TestCase):
    def measure(self, elf: bytes):
        return codec_budget.measure(elf)

    def test_the_stack_is_the_deepest_path_from_a_codec_function(self):
        used, root, why = self.measure(chain())
        self.assertEqual(why, [])
        self.assertEqual(used["stack_bytes"], 16 + 8 + 24)
        self.assertEqual(root, codec("a"))

    def test_a_tail_branch_out_of_a_function_is_an_edge(self):
        # B (T2) from `b` to `c`: c's frame counts under b.
        b_at, c_at = TEXT, TEXT + 6
        b = [PUSH_R7_LR, 0xE000 | (((c_at - (b_at + 2 + 4)) >> 1) & 0x7FF), NOP]
        used, _, why = self.measure(image([(codec("b"), b, 8), (other("c"), [BX_LR], 24)]))
        self.assertEqual((used["stack_bytes"], why), (32, []))

    def test_a_planted_indirect_call_fails(self):
        used, _, why = self.measure(chain(extra_b=[BLX_R3, NOP]))
        self.assertEqual(len(why), 1, why)
        self.assertIn("indirect call", why[0])
        self.assertIn(codec("b"), why[0])
        # BX to a register other than LR, and MOV PC, are indirect too.
        for planted in (0x4718, 0x469F):
            self.assertIn("indirect call", self.measure(chain(extra_b=[planted, NOP]))[2][0])

    def test_a_handler_rows_dispatch_reaches_the_decoders_only(self):
        row = "_RNvMs0_NtCs1a_11dagon_proto7handlerINtB5_7HandleruuE4call"
        decoder = "_RNvNtNtCs1a_11dagon_proto7payload6decode5hello"
        used, root, why = self.measure(image([
            (row, [PUSH_R7_LR, BLX_R3, POP_R7_PC], 16),
            (decoder, [PUSH_R7_LR, POP_R7_PC], 40),
            (other("act"), [BX_LR], 100),
        ]))
        self.assertEqual((used["stack_bytes"], root, why), (56, row, []))

    def test_the_codecs_text_formatting_is_not_measured_from(self):
        debug = "_RNvXs_NtCs1a_11dagon_proto5fieldNtB4_6StatusNtNtCs9z_4core3fmt5Debug3fmt"
        used, _, why = self.measure(image([(debug, [PUSH_R7_LR, BLX_R3, POP_R7_PC], 16)]))
        self.assertEqual((used["stack_bytes"], why), (0, []))
        self.assertEqual(used["flash_bytes"], 6)

    def test_a_return_is_not_an_indirect_call(self):
        for ret in (BX_LR, POP_R7_PC):
            self.assertEqual(self.measure(chain(extra_b=[ret, NOP]))[2], [])

    def test_planted_recursion_fails(self):
        # `b` calls `a` back.
        b_at = TEXT + 8
        _, _, why = self.measure(chain(extra_b=bl(b_at + 6, TEXT)))
        self.assertTrue(any(w.startswith("recursion at") for w in why), why)
        # `b` calls itself.
        _, _, why = self.measure(chain(extra_b=bl(b_at + 6, b_at)))
        self.assertTrue(any(w.startswith("recursion at") for w in why), why)
        # `b` branches back to its PUSH: a tail call to itself.
        back = 0xE000 | (((b_at - (b_at + 6 + 4)) >> 1) & 0x7FF)
        _, _, why = self.measure(chain(extra_b=[back, NOP]))
        self.assertTrue(any(w.startswith("recursion at") for w in why), why)

    def test_any_function_of_the_image_that_calls_itself_fails(self):
        # The codec does not reach it: a memory helper lowered to a call to
        # itself, and one branching back to its PUSH.
        helper, tail = TEXT + 4, TEXT + 12
        code = [(codec("a"), [PUSH_R7_LR, POP_R7_PC], 8),
                ("__aeabi_memcpy", [PUSH_R7_LR, *bl(helper + 2, helper), POP_R7_PC], 8),
                ("__aeabi_memset", [PUSH_R7_LR, 0xE000 | (((tail - (tail + 2 + 4)) >> 1) & 0x7FF),
                                    NOP], None)]
        _, _, why = self.measure(image(code))
        self.assertEqual(why, ["__aeabi_memcpy: calls itself", "__aeabi_memset: calls itself"])

    def test_a_loop_to_the_start_of_a_leaf_without_a_prologue_is_a_loop(self):
        # subs r0, #1; bne <start>; bx lr
        leaf = [0x3801, 0xD000 | ((-3) & 0xFF), BX_LR]
        used, _, why = self.measure(image([(codec("a"), leaf, 0)]))
        self.assertEqual((used["stack_bytes"], why), (0, []))

    def test_a_call_into_the_middle_of_a_function_fails(self):
        _, _, why = self.measure(chain(extra_b=bl(TEXT + 14, TEXT + 2)))
        self.assertTrue(any("no function start" in w for w in why), why)

    def test_a_function_without_a_record_takes_its_frame_from_its_code(self):
        # push {r4-r7, lr}; sub sp, #8: 20 + 8 bytes.
        f = codec_budget.Function("x", TEXT, struct.pack("<3H", 0xB5F0, 0xB082, BX_LR), [])
        self.assertEqual(codec_budget.prologue_frame(f), 28)
        # mov sp, r0: the frame is not known, and the image fails.
        a = [PUSH_R7_LR, 0x4685, POP_R7_PC]
        _, _, why = self.measure(image([(codec("a"), a, None)]))
        self.assertTrue(any("no frame record" in w for w in why), why)

    def test_a_jump_table_inside_the_function_is_not_an_indirect_call(self):
        # ADD PC, R1 followed by a byte table ($d) whose cases stay inside.
        code = struct.pack("<2H", 0x448F, 0x0201) + struct.pack("<3H", NOP, NOP, BX_LR)
        f = codec_budget.Function("x", TEXT, code, [(TEXT + 2, TEXT + 4)])
        self.assertEqual({k for k, _, _ in codec_budget.transfers(f)}, {"branch"})
        out = codec_budget.Function("x", TEXT, struct.pack("<2H", 0x448F, 0x00FF),
                                    [(TEXT + 2, TEXT + 4)])
        self.assertEqual([k for k, _, _ in codec_budget.transfers(out)], ["unknown"])


class Budgets(unittest.TestCase):
    def test_every_image_the_size_checks_build_has_a_budget(self):
        builds = codec_budget.builds()
        self.assertEqual({m for m, i, _ in builds if i == "bootcore"},
                         set(bootcore_size.bootcore_mcus()))
        self.assertEqual({m for m, i, _ in builds if i == "relay"}, set(relay_ram.relay_mcus()))
        for mcu, image_kind, _ in builds:
            limits = codec_budget.budget(mcu, image_kind)
            self.assertIsNotNone(limits, (mcu, image_kind))
            self.assertEqual(set(limits), set(codec_budget.FIELDS), (mcu, image_kind))

    def test_an_mcu_without_the_image_has_no_budget_for_it(self):
        for mcu in set(bootcore_size.mcus()) - set(bootcore_size.bootcore_mcus()):
            self.assertIsNone(codec_budget.budget(mcu, "bootcore"), mcu)
        for mcu in set(relay_ram.mcus()) - set(relay_ram.relay_mcus()):
            self.assertIsNone(codec_budget.budget(mcu, "relay"), mcu)

    def test_at_the_budget_passes_and_one_byte_over_fails(self):
        mcu, image_kind, _ = codec_budget.builds()[0]
        limits = codec_budget.budget(mcu, image_kind)
        self.assertEqual(codec_budget.verdict(dict(limits), limits), [])
        for field in codec_budget.FIELDS:
            used = dict(limits) | {field: limits[field] + 1}
            self.assertEqual(len(codec_budget.verdict(used, limits)), 1, field)

    def test_check_reads_the_image_against_the_definition(self):
        mcu, image_kind, _ = codec_budget.builds()[0]
        limits = codec_budget.budget(mcu, image_kind)
        with tempfile.TemporaryDirectory() as d:
            path = pathlib.Path(d) / "image"
            fits = [(codec("t"), limits["flash_bytes"], False)]
            path.write_bytes(image([(codec("a"), [BX_LR], 0)], fits))
            # The function's 2 bytes and the table: 2 bytes over.
            why = codec_budget.check(path, mcu, image_kind)
            self.assertEqual(len(why), 1, why)
            self.assertIn(f"flash {limits['flash_bytes'] + 2} bytes, over", why[0])
            if limits["ram_bytes"] == 0:
                path.write_bytes(image([(codec("a"), [BX_LR], 0)], [(codec("s"), 1, True)]))
                self.assertIn("ram 1 bytes, over its 0", codec_budget.check(path, mcu, image_kind)[0])

    def test_an_mcu_without_a_budget_is_refused(self):
        self.assertEqual(codec_budget.check(pathlib.Path("unused"), "esp32s3", "bootcore"),
                         ["esp32s3 bootcore codec: the definition has no [codec.bootcore]"])


if __name__ == "__main__":
    unittest.main()
