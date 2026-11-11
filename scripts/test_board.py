"""What every board tells its MCU package's start has one home.

- One `trait BoardCore`, in mcu-common, declaring exactly the board id,
  the flash size, the bring-up port and the board extension, each with its
  type or signature and its default.
- Comments and literals are not code (rust_source.code_only).
- No other trait declares `const BOARD_ID` or `const FLASH_SIZE`: an MCU
  package's `BoardDef` extends `BoardCore` instead of repeating them.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import unittest

from rust_source import code_only, rust_texts

HOME = "targets/mcu/common/src/hooks.rs"
SHARED_CONSTS = ("BOARD_ID", "FLASH_SIZE")
MEMBER_NAMES = (*SHARED_CONSTS, "bring_up_port", "extension")

TRAIT = re.compile(r"\btrait\s+(\w+)")
CONST = re.compile(r"const\s+(\w+)")
SPACE_BY_PUNCT = re.compile(r"\s*([^\w\s'])\s*")


def normal(item):
    """One declaration with whitespace kept only between two words."""
    return SPACE_BY_PUNCT.sub(r"\1", " ".join(item.split()))


# BoardCore's members, whole: name, type or signature, and default body.
MEMBERS = sorted(
    normal(m)
    for m in (
        "const BOARD_ID: &'static str;",
        "const FLASH_SIZE: usize;",
        "fn bring_up_port() -> Option<&'static mut dyn BoardBringUp> { None }",
        "fn extension() -> Option<BoardExtension> { None }",
    )
)


def items(code, start):
    """The top-level items of the block opening at `code[start]` (`{`),
    each normalized: one ends at a `;` or at the `}` closing its body."""
    out, depth, begin = [], 0, start + 1
    for i in range(start + 1, len(code)):
        c = code[i]
        if c == "}" and depth == 0:
            break
        depth += {"{": 1, "}": -1}.get(c, 0)
        if (c == ";" or c == "}") and depth == 0:
            if code[begin:i].strip():
                out.append(normal(code[begin : i + 1]))
            begin = i + 1
    return out


def traits(code):
    """(name, top-level items) of every trait with a body in `code`, whose
    comments and literals are already blanked (code_only)."""
    out = []
    for m in TRAIT.finditer(code):
        brace, semi = code.find("{", m.end()), code.find(";", m.end())
        if brace >= 0 and (semi < 0 or brace < semi):
            out.append((m.group(1), items(code, brace)))
    return out


def findings(texts):
    """Every break of the rule over `texts` (path -> Rust source)."""
    found, homes = [], []
    for path, text in texts.items():
        for name, declared in traits(code_only(text)):
            if name == "BoardCore":
                homes.append(path)
                if sorted(declared) != MEMBERS:
                    found.append(f"{path}: BoardCore declares {sorted(declared)}, not {MEMBERS}")
                continue
            repeated = [m[1] for d in declared if (m := CONST.match(d)) and m[1] in SHARED_CONSTS]
            if repeated:
                found.append(f"{path}: trait {name} declares {repeated}; extend BoardCore")
    if homes != [HOME]:
        found.append(f"trait BoardCore defined in {homes}, not once in {HOME}")
    return found


CORE = """pub trait BoardCore {
    const BOARD_ID: &'static str;
    const FLASH_SIZE: usize;
    fn bring_up_port() -> Option<&'static mut dyn BoardBringUp> {
        None
    }
    fn extension() -> Option<BoardExtension> {
        None
    }
}
"""
BOARD_DEF = """pub trait BoardDef: BoardCore {
    type UserIo;
    fn split(p: Peripherals) -> (DagonParts, Self::UserIo);
}
"""


class OneBoardCore(unittest.TestCase):
    def test_the_tree_holds(self):
        self.assertEqual(findings(rust_texts()), [])

    def test_the_model_tree_holds(self):
        self.assertEqual(findings({HOME: CORE, "mcu/node.rs": BOARD_DEF}), [])

    def test_the_check_catches_a_second_board_core(self):
        self.assertTrue(findings({HOME: CORE, "mcu/node.rs": CORE}))

    def test_the_check_catches_a_missing_board_core(self):
        self.assertTrue(findings({"mcu/node.rs": BOARD_DEF}))

    def test_the_check_catches_a_missing_member(self):
        for name in MEMBER_NAMES:
            body = "\n".join(l for l in CORE.splitlines() if not re.search(rf"\b{name}\b", l))
            self.assertTrue(findings({HOME: body}), name)

    def test_the_check_catches_a_repeated_const(self):
        for name in SHARED_CONSTS:
            repeated = BOARD_DEF.replace("    type UserIo;", f"    const {name}: usize;\n    type UserIo;")
            self.assertTrue(findings({HOME: CORE, "mcu/node.rs": repeated}), name)

    def test_the_check_catches_a_duplicate_named_after_a_comment(self):
        for planted in (
            "pub trait /* planted */ BoardCore {}\n",
            "pub trait // planted\nBoardCore {}\n",
            "pub trait r#BoardCore {}\n",
        ):
            self.assertTrue(findings({HOME: CORE, "mcu/node.rs": BOARD_DEF + planted}), planted)

    def test_fakes_in_comments_and_strings_pass(self):
        for extra in (
            "/* pub trait BoardCore { const BOARD_ID: u8; } /* nested } */ { */\n",
            "// pub trait BoardDef { const FLASH_SIZE: u8; } {\n",
            'const S: &str = "pub trait BoardCore { \\" } {";\n',
            'const R: &str = r#"pub trait BoardCore { "# }"#;\n',
            'const B: &[u8] = b"trait X { const BOARD_ID: u8; } {";\n',
            'const BR: &[u8] = br##"trait X { const FLASH_SIZE: u8; "# {"##;\n',
            "const C: char = '{'; const E: char = '\\''; const BC: u8 = b'}';\n",
        ):
            self.assertEqual(findings({HOME: CORE + extra, "mcu/node.rs": BOARD_DEF}), [], extra)
        for member in ("    // const BOARD_ID: u8;\n", "    /* const FLASH_SIZE: usize; */\n"):
            board_def = BOARD_DEF.replace("    type UserIo;\n", member + "    type UserIo;\n")
            self.assertEqual(findings({HOME: CORE, "mcu/node.rs": board_def}), [], member)

    def test_a_fake_brace_does_not_hide_a_repeated_const(self):
        for brace in ('    fn s() -> &\'static str { "{" }\n', "    // {\n", "    fn c() -> char { '{' }\n"):
            hidden = "pub trait BoardDef: BoardCore {\n" + brace + "    const BOARD_ID: &'static str;\n}\n"
            self.assertTrue(findings({HOME: CORE, "mcu/node.rs": hidden}), brace)

    def test_the_check_catches_an_altered_type(self):
        for old, new in (
            ("const BOARD_ID: &'static str;", "const BOARD_ID: u32;"),
            ("const FLASH_SIZE: usize;", "const FLASH_SIZE: u32;"),
            ("fn bring_up_port() -> Option<&'static mut dyn BoardBringUp>", "fn bring_up_port() -> bool"),
            ("fn extension() -> Option<BoardExtension>", "fn extension(&self) -> Option<BoardExtension>"),
        ):
            self.assertIn(old, CORE)
            self.assertTrue(findings({HOME: CORE.replace(old, new)}), new)

    def test_the_check_catches_an_altered_default(self):
        default = " {\n        None\n    }"
        port = "fn bring_up_port() -> Option<&'static mut dyn BoardBringUp>"
        for new in (port + " {\n        true\n    }", port + ";"):
            self.assertTrue(findings({HOME: CORE.replace(port + default, new)}), new)
        self.assertTrue(findings({HOME: CORE.replace("const FLASH_SIZE: usize;", "const FLASH_SIZE: usize = 0;")}))

    def test_whitespace_and_comments_inside_board_core_pass(self):
        spaced = CORE.replace(
            "    fn extension() -> Option<BoardExtension> {\n",
            "    /// doc {\n    fn extension( ) ->Option< BoardExtension >\n    { // none }\n",
        )
        self.assertNotEqual(spaced, CORE)
        self.assertEqual(findings({HOME: spaced}), [])


if __name__ == "__main__":
    unittest.main()
