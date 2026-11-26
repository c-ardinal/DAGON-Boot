"""Boards come out of Zephyr's files the same way every time.

- The boards in scripts/fixtures/imported/ are what import_zephyr_board.py
  writes from the pinned files in scripts/fixtures/zephyr-v4.1.0/ (the files
  as Zephyr v4.1.0 has them, Apache-2.0, licence beside them).
- What Zephyr says of the NUCLEO-F401RE agrees with DAGON's own definition
  of that board, written from ST's documents: two sources, one board.
- A pin named in a way the importer does not know is refused.

    python -m unittest discover -s scripts -p "test_*.py"
"""
import tempfile
import tomllib
import unittest
from pathlib import Path

import import_zephyr_board as imp

ROOT = Path(__file__).resolve().parent.parent
TREE = ROOT / "scripts/fixtures/zephyr-v4.1.0"
MADE = ROOT / "scripts/fixtures/imported"
TAG = "v4.1.0"
BOARDS = {
    "rpi-pico": ("boards/raspberrypi/rpi_pico/rpi_pico.dts", "rp2040"),
    "nucleo-f401re-zephyr": ("boards/st/nucleo_f401re/nucleo_f401re.dts", "stm32f401re"),
}


def made(board_id):
    dts, mcu = BOARDS[board_id]
    return imp.convert(TREE, TREE / dts, board_id, mcu, TAG)


class Imported(unittest.TestCase):
    def test_the_kept_boards_are_what_the_importer_writes(self):
        for board_id in BOARDS:
            kept = (MADE / f"{board_id}.toml").read_text(encoding="utf-8")
            self.assertEqual(kept, made(board_id), board_id)

    def test_the_pico_has_its_header_buses_and_led(self):
        b = tomllib.loads(made("rpi-pico"))["board"]["rpi-pico"]
        self.assertEqual(b["product"], "Raspberry Pi Pico")
        self.assertEqual(b["flash_bytes"], 2 * 1024 * 1024)
        self.assertEqual(len(b["header"]), 26)
        self.assertIn({"pin": "GPIO28", "label": "GP28"}, b["header"])
        self.assertEqual(
            b["i2c"],
            [
                {"controller": "i2c0", "sda": "GPIO4", "scl": "GPIO5"},
                {"controller": "i2c1", "sda": "GPIO6", "scl": "GPIO7"},
            ],
        )
        self.assertEqual(b["pin"], [{"pin": "GPIO25", "name": "led0"}])
        self.assertEqual(
            b["bus"],
            [
                {
                    "id": "uart0",
                    "kind": "uart",
                    "controller": "uart0",
                    "tx": "GPIO0",
                    "rx": "GPIO1",
                    "baud": 115200,
                }
            ],
        )

    def test_zephyr_and_dagon_agree_on_the_nucleo(self):
        z = tomllib.loads(made("nucleo-f401re-zephyr"))["board"]["nucleo-f401re-zephyr"]
        own = tomllib.loads(
            (ROOT / "targets/boards/nucleo-f401re/dagon-board.toml").read_text(encoding="utf-8")
        )
        self.assertEqual(z["mcu"], own["mcu"])
        self.assertEqual(z["flash_bytes"], own["flash_bytes"])
        # Every header pin DAGON names carries the same printed name.
        printed = {h["pin"]: h["label"] for h in z["header"]}
        for h in own["header"]:
            if "label" in h:
                self.assertEqual(printed[h["pin"]], h["label"], h)
        # Zephyr's Arduino I2C is one of DAGON's pairs.
        self.assertIn({"controller": "i2c1", "sda": "PB9", "scl": "PB8"}, z["i2c"])
        self.assertIn({"controller": "i2c1", "sda": "PB9", "scl": "PB8"}, own["i2c"])
        # The ST-LINK virtual COM port.
        vcp = next(b for b in own["bus"] if b["id"] == "vcp")
        usart2 = next(b for b in z["bus"] if b["id"] == "usart2")
        self.assertEqual(
            (usart2["controller"], usart2["tx"], usart2["rx"]),
            (vcp["controller"], vcp["tx"], vcp["rx"]),
        )
        # LD2 and B1.
        own_pins = {p["name"]: p["pin"] for p in own["pin"]}
        z_pins = {p["name"]: p["pin"] for p in z["pin"]}
        self.assertEqual(z_pins["green_led_2"], own_pins["led"])
        self.assertEqual(z_pins["user_button"], own_pins["button"])

    def test_the_head_names_every_file_read_and_the_ones_not_found(self):
        text = made("rpi-pico")
        for rel in (
            "boards/raspberrypi/rpi_pico/rpi_pico.dts",
            "boards/raspberrypi/common/rpi_pico-pinctrl-common.dtsi",
            "boards/raspberrypi/rpi_pico/board.yml",
        ):
            self.assertRegex(text, rf"#   {rel} [0-9a-f]{{64}}\n")
        self.assertIn("raspberrypi/rpi_pico/rp2040.dtsi", text.split("[board.")[0])


class Refused(unittest.TestCase):
    def convert(self, dts):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "boards/x/x.dts"
            path.parent.mkdir(parents=True)
            path.write_text(dts, encoding="utf-8")
            return imp.convert(d, path, "x", "rp2040", TAG)

    def test_a_gpio_node_named_another_way(self):
        dts = "/ { leds { compatible = \"gpio-leds\"; a: a { gpios = <&porta 1 0>; }; }; };"
        with self.assertRaisesRegex(imp.ImportError_, "porta"):
            self.convert(dts)

    def test_a_board_whose_flash_size_is_in_no_file_read(self):
        with self.assertRaisesRegex(imp.ImportError_, "flash0"):
            self.convert("/ { };")

    def test_a_pin_control_node_not_in_the_files_read(self):
        dts = "&flash0 { reg = <0 0x1000>; };\n&i2c0 { pinctrl-0 = <&elsewhere>; };"
        with self.assertRaisesRegex(imp.ImportError_, "elsewhere"):
            self.convert(dts)


if __name__ == "__main__":
    unittest.main()
