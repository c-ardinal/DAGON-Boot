"""What every board tells its MCU package's start has one home.

- One `trait BoardCore`, in mcu-common, declaring exactly the board id,
  the flash size, the bring-up port and the board extension.
- No other trait declares `const BOARD_ID` or `const FLASH_SIZE`: an MCU
  package's `BoardDef` extends `BoardCore` instead of repeating them.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUST_DIRS = ("crates", "targets", "samples")
HOME = "targets/mcu/common/src/hooks.rs"
MEMBERS = {"BOARD_ID", "FLASH_SIZE", "bring_up_port", "extension"}
SHARED_CONSTS = ("BOARD_ID", "FLASH_SIZE")

TRAIT = re.compile(r"\btrait\s+(\w+)[^{;]*\{")
MEMBER = re.compile(r"^\s*(?:const|fn|type)\s+(\w+)", re.M)


def rust_files():
    for d in RUST_DIRS:
        for p in (ROOT / d).rglob("*.rs"):
            if "target" not in p.relative_to(ROOT).parts:
                yield p


def rel(p):
    return p.relative_to(ROOT).as_posix()


def traits(text):
    """(name, body) of every trait in `text`, the body up to its closing
    brace (default method bodies included)."""
    out = []
    for m in TRAIT.finditer(text):
        depth, i = 1, m.end()
        while depth and i < len(text):
            depth += {"{": 1, "}": -1}.get(text[i], 0)
            i += 1
        out.append((m.group(1), text[m.end() : i - 1]))
    return out


def members(body):
    """Names the trait declares at its own level (not inside a default
    method's body)."""
    depth, top = 0, []
    for line in body.splitlines():
        if depth == 0:
            top.append(line)
        depth += line.count("{") - line.count("}")
    return set(MEMBER.findall("\n".join(top)))


def findings(texts):
    """Every break of the rule over `texts` (path -> Rust source)."""
    found, homes = [], []
    for path, text in texts.items():
        for name, body in traits(text):
            declared = members(body)
            if name == "BoardCore":
                homes.append(path)
                if declared != MEMBERS:
                    found.append(f"{path}: BoardCore declares {sorted(declared)}, not {sorted(MEMBERS)}")
            elif declared & set(SHARED_CONSTS):
                found.append(f"{path}: trait {name} declares {sorted(declared & set(SHARED_CONSTS))}; extend BoardCore")
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
        texts = {rel(p): p.read_text(encoding="utf-8") for p in rust_files()}
        self.assertEqual(findings(texts), [])

    def test_the_model_tree_holds(self):
        self.assertEqual(findings({HOME: CORE, "mcu/node.rs": BOARD_DEF}), [])

    def test_the_check_catches_a_second_board_core(self):
        self.assertTrue(findings({HOME: CORE, "mcu/node.rs": CORE}))

    def test_the_check_catches_a_missing_board_core(self):
        self.assertTrue(findings({"mcu/node.rs": BOARD_DEF}))

    def test_the_check_catches_a_missing_member(self):
        for name in MEMBERS:
            body = "\n".join(l for l in CORE.splitlines() if not re.search(rf"\b{name}\b", l))
            self.assertTrue(findings({HOME: body}), name)

    def test_the_check_catches_a_repeated_const(self):
        for name in SHARED_CONSTS:
            repeated = BOARD_DEF.replace("    type UserIo;", f"    const {name}: usize;\n    type UserIo;")
            self.assertTrue(findings({HOME: CORE, "mcu/node.rs": repeated}), name)


if __name__ == "__main__":
    unittest.main()
