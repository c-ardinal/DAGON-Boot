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


class Stated(unittest.TestCase):
    """A node's last word counts, wherever the files state it."""

    def board(self, dts):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "boards/x/x.dts"
            path.parent.mkdir(parents=True)
            path.write_text(dts, encoding="utf-8")
            return tomllib.loads(imp.convert(d, path, "x", "rp2040", TAG))["board"]["x"]

    def test_an_overlay_rewires_an_led(self):
        b = self.board(
            "/ { leds { compatible = \"gpio-leds\"; led0: led_0 { gpios = <&gpio0 25 0>; }; }; };\n"
            "&flash0 { reg = <0 0x1000>; };\n"
            "&led0 { gpios = <&gpio0 16 0>; };\n"
        )
        self.assertEqual(b["pin"], [{"pin": "GPIO16", "name": "led0"}])

    def test_a_pin_written_in_hexadecimal_is_that_pin(self):
        b = self.board(
            "/ { leds { compatible = \"gpio-leds\"; led0: led_0 { gpios = <&gpio0 0x10 0>; }; };\n"
            "    c: connector { gpio-map = <0x0 0 &gpio0 0x1c 0>;	/* GP28 */ }; };\n"
            "&flash0 { reg = <0 0x1000>; };\n"
        )
        self.assertEqual(b["pin"], [{"pin": "GPIO16", "name": "led0"}])
        self.assertEqual(b["header"], [{"pin": "GPIO28", "label": "GP28"}])

    def test_a_line_comment_states_nothing(self):
        b = self.board(
            "/ { leds { compatible = \"gpio-leds\"; led0: led_0 { gpios = <&gpio0 25 0>; }; }; };\n"
            "&flash0 { reg = <0 0x1000>; }; /* see http://example.invalid/pico */\n"
            "// &led0 { gpios = <&gpio0 16 0>; };\n"
        )
        self.assertEqual(b["pin"], [{"pin": "GPIO25", "name": "led0"}])

    def test_a_connector_inside_a_block_comment_states_nothing(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/* c: connector { gpio-map = <0 0 &gpio0 16 0>; }; */\n"
            "/ { d: connector { gpio-map = <0 0 &gpio0 2 0>,	/* GP2 */\n"
            "    /* <1 0 &gpio0 3 0>, */\n"
            "    <2 0 &gpio0 4 0>; /* GP4 */ }; };\n"
        )
        self.assertEqual(
            b["header"], [{"pin": "GPIO2", "label": "GP2"}, {"pin": "GPIO4", "label": "GP4"}]
        )

    def test_an_overlay_of_a_connector_replaces_its_rows(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/ { c: connector { gpio-map = <0 0 &gpio0 0 0>;	/* GP0 */ }; };\n"
            "&c { gpio-map = <0 0 &gpio0 16 0>;	/* GP16 */ };\n"
        )
        self.assertEqual(b["header"], [{"pin": "GPIO16", "label": "GP16"}])

    def test_an_overlay_that_only_enables_a_bus_keeps_what_its_declaration_gives(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/ { soc { usart2: serial@40004400 { current-speed = <115200>;"
            " pinctrl-0 = <&usart2_tx_pa2 &usart2_rx_pa3>; status = \"disabled\"; }; }; };\n"
            "&usart2 { status = \"okay\"; };\n"
        )
        self.assertEqual(
            b["bus"],
            [
                {
                    "id": "usart2",
                    "kind": "uart",
                    "controller": "usart2",
                    "tx": "PA2",
                    "rx": "PA3",
                    "baud": 115200,
                }
            ],
        )

    def test_a_node_stated_again_by_its_path_is_merged(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/ { leds { compatible = \"gpio-leds\"; led0: led_0 { gpios = <&gpio0 25 0>; }; }; };\n"
            "/ { leds { led_0 { gpios = <&gpio0 7 0>; }; led_1 { gpios = <&gpio0 8 0>; }; }; };\n"
        )
        self.assertEqual(
            b["pin"], [{"pin": "GPIO7", "name": "led0"}, {"pin": "GPIO8", "name": "led_1"}]
        )

    def test_a_deleted_property_and_node_are_gone(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/ { leds { compatible = \"gpio-leds\";\n"
            "    led0: led_0 { gpios = <&gpio0 25 0>; }; led1: led_1 { gpios = <&gpio0 8 0>; }; }; };\n"
            "&led0 { /delete-property/ gpios; };\n"
            "/ { leds { /delete-node/ led_1; }; };\n"
        )
        self.assertNotIn("pin", b)

    def test_a_deleted_node_gives_no_bus(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/ { soc { usart2: serial@40004400 { current-speed = <115200>;"
            " pinctrl-0 = <&usart2_tx_pa2 &usart2_rx_pa3>; status = \"okay\"; }; }; };\n"
            "/ { soc { /delete-node/ serial@40004400; }; };\n"
        )
        self.assertNotIn("bus", b)

    def test_a_node_deleted_by_its_label_is_gone(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/ { leds { compatible = \"gpio-leds\";\n"
            "    led0: led_0 { gpios = <&gpio0 25 0>; }; led1: led_1 { gpios = <&gpio0 8 0>; }; }; };\n"
            "/delete-node/ &led0;\n"
            "/ { leds { /delete-node/ &led1; }; };\n"
        )
        self.assertNotIn("pin", b)

    def test_a_node_referred_to_by_its_path_is_that_node(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/ { leds { compatible = \"gpio-leds\";\n"
            "    led0: led_0 { gpios = <&gpio0 25 0>; }; led1: led_1 { gpios = <&gpio0 8 0>; }; }; };\n"
            "&{/leds/led_0} { gpios = <&gpio0 16 0>; };\n"
            "/delete-node/ &{/leds/led_1};\n"
        )
        self.assertEqual(b["pin"], [{"pin": "GPIO16", "name": "led0"}])

    def test_an_include_followed_by_a_comment_is_read(self):
        for comment in ("// wiring", "/* wiring */"):
            with tempfile.TemporaryDirectory() as d:
                path = Path(d) / "boards/x/x.dts"
                path.parent.mkdir(parents=True)
                (path.parent / "leds.dtsi").write_text(
                    "/ { leds { compatible = \"gpio-leds\"; led0: led_0 { gpios = <&gpio0 25 0>; }; }; };\n",
                    encoding="utf-8",
                )
                path.write_text(
                    f"#include \"leds.dtsi\" {comment}\n&flash0 {{ reg = <0 0x1000>; }};\n",
                    encoding="utf-8",
                )
                b = tomllib.loads(imp.convert(d, path, "x", "rp2040", TAG))["board"]["x"]
            self.assertEqual(b["pin"], [{"pin": "GPIO25", "name": "led0"}], comment)

    def test_an_expression_with_shifts_and_comparisons_ends_where_it_ends(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/ { other { a = <(1 << 2) (8 >> 1)>; b = <(1 > 2) (1 < 2) (2 >= 1)>; c = [00 ff]; };\n"
            "    leds { compatible = \"gpio-leds\"; led0: led_0 { gpios = <&gpio0 25 0>; }; }; };\n"
        )
        self.assertEqual(b["pin"], [{"pin": "GPIO25", "name": "led0"}])

    def test_a_quoted_include_not_beside_the_file_is_looked_for_in_the_roots(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "boards/x/x.dts"
            path.parent.mkdir(parents=True)
            common = Path(d) / "dts/common"
            common.mkdir(parents=True)
            (common / "leds.dtsi").write_text(
                "/ { leds { compatible = \"gpio-leds\"; led0: led_0 { gpios = <&gpio0 25 0>; }; }; };\n",
                encoding="utf-8",
            )
            path.write_text(
                "#include \"leds.dtsi\"\n&flash0 { reg = <0 0x1000>; };\n", encoding="utf-8"
            )
            b = tomllib.loads(imp.convert(d, path, "x", "rp2040", TAG))["board"]["x"]
        self.assertEqual(b["pin"], [{"pin": "GPIO25", "name": "led0"}])

    def test_rows_of_a_connector_in_one_cell_list_are_each_a_row(self):
        b = self.board(
            "&flash0 { reg = <0 0x1000>; };\n"
            "/ { c: connector { gpio-map = <0 0 &gpio0 1 0 1 0 &gpio0 2 (A | B)\n"
            "    2 0 &gpio0 3 0>;	/* GP3 */ }; };\n"
        )
        self.assertEqual(
            b["header"],
            [{"pin": "GPIO1"}, {"pin": "GPIO2"}, {"pin": "GPIO3", "label": "GP3"}],
        )

    def test_a_file_included_again_states_again_unless_it_guards_itself(self):
        led = "/ { leds { compatible = \"gpio-leds\"; led0: led_0 { gpios = <&gpio0 25 0>; }; }; };\n"
        for guard, pin in (
            ("", "GPIO25"),
            ("#ifndef LEDS_DTSI\n#define LEDS_DTSI\n", "GPIO16"),
            ("#pragma once\n", "GPIO16"),
        ):
            with tempfile.TemporaryDirectory() as d:
                path = Path(d) / "boards/x/x.dts"
                path.parent.mkdir(parents=True)
                (path.parent / "leds.dtsi").write_text(
                    guard + "#include \"leds.dtsi\"\n" + led, encoding="utf-8"
                )
                path.write_text(
                    "#include \"leds.dtsi\"\n&flash0 { reg = <0 0x1000>; };\n"
                    "&led0 { gpios = <&gpio0 16 0>; };\n#include \"leds.dtsi\"\n",
                    encoding="utf-8",
                )
                text = imp.convert(d, path, "x", "rp2040", TAG)
            b = tomllib.loads(text)["board"]["x"]
            self.assertEqual(b["pin"], [{"pin": pin, "name": "led0"}], guard)
            self.assertEqual(text.count("leds.dtsi"), 1, guard)

    def test_a_deleted_node_whose_declaration_is_not_read_gives_no_bus(self):
        dts = (
            "&pinctrl { uart0_default: uart0_default {\n"
            "    group1 { pinmux = <UART0_TX_P0>; };\n"
            "    group2 { pinmux = <UART0_RX_P1>; input-enable; };\n"
            "}; };\n"
            "&flash0 { reg = <0 0x1000>; };\n"
            "&uart0 { status = \"okay\"; pinctrl-0 = <&uart0_default>; };\n"
        )
        self.assertEqual(len(self.board(dts)["bus"]), 1)
        self.assertNotIn("bus", self.board(dts + "/delete-node/ &uart0;\n"))

    def test_an_include_inside_a_block_comment_is_not_read(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "boards/x/x.dts"
            path.parent.mkdir(parents=True)
            (path.parent / "leds.dtsi").write_text(
                "/ { leds { compatible = \"gpio-leds\"; led0: led_0 { gpios = <&gpio0 25 0>; }; }; };\n",
                encoding="utf-8",
            )
            path.write_text(
                "/*\n#include \"leds.dtsi\"\n*/\n"
                "#include \"leds.dtsi\"\n"
                "&flash0 { reg = <0 0x1000>; };\n",
                encoding="utf-8",
            )
            b = tomllib.loads(imp.convert(d, path, "x", "rp2040", TAG))["board"]["x"]
        self.assertEqual(b["pin"], [{"pin": "GPIO25", "name": "led0"}])

    def test_an_overlay_of_a_pin_control_group_moves_the_bus(self):
        b = self.board(
            "&pinctrl { uart0_default: uart0_default {\n"
            "    group1 { pinmux = <UART0_TX_P0>; };\n"
            "    group2 { pinmux = <UART0_RX_P1>; input-enable; };\n"
            "}; };\n"
            "&flash0 { reg = <0 0x1000>; };\n"
            "&uart0 { status = \"okay\"; pinctrl-0 = <&uart0_default>; };\n"
            "&uart0_default { group1 { pinmux = <UART0_TX_P12>; }; };\n"
        )
        self.assertEqual((b["bus"][0]["tx"], b["bus"][0]["rx"]), ("GPIO12", "GPIO1"))

    def test_a_later_declaration_of_flash0_gives_its_size(self):
        b = self.board(
            "/ { soc { flash0: flash@0 { compatible = \"soc-nv-flash\"; }; }; };\n"
            "/ { soc { flash0: flash@0 { reg = <0 DT_SIZE_K(512)>; }; }; };\n"
        )
        self.assertEqual(b["flash_bytes"], 512 * 1024)

    def test_a_later_size_of_flash0_replaces_an_earlier_one(self):
        b = self.board(
            "/ { soc { flash0: flash@0 { reg = <0 DT_SIZE_M(1)>; }; }; };\n"
            "/ { soc { flash0: flash@0 { reg = <0 DT_SIZE_M(4)>; }; }; };\n"
        )
        self.assertEqual(b["flash_bytes"], 4 * 1024 * 1024)


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

    def test_a_pin_that_is_not_a_number(self):
        dts = "/ { leds { compatible = \"gpio-leds\"; a: a { gpios = <&gpio0 LED_PIN 0>; }; }; };"
        with self.assertRaisesRegex(imp.ImportError_, "LED_PIN"):
            self.convert(dts)

    def test_a_node_that_is_never_closed(self):
        with self.assertRaisesRegex(imp.ImportError_, "never closed"):
            self.convert("/ { leds { ")

    def test_a_board_whose_flash_size_is_in_no_file_read(self):
        with self.assertRaisesRegex(imp.ImportError_, "flash0"):
            self.convert("/ { };")

    def test_a_pin_control_node_not_in_the_files_read(self):
        dts = "&flash0 { reg = <0 0x1000>; };\n&i2c0 { pinctrl-0 = <&elsewhere>; };"
        with self.assertRaisesRegex(imp.ImportError_, "elsewhere"):
            self.convert(dts)


if __name__ == "__main__":
    unittest.main()
