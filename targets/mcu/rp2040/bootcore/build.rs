//! The boot LED and halt pattern come from the board of the node whose
//! boot folder `DAGON_BOOT_CRATE` names (`board.rs`), and the clock from
//! the MCU's `[[bootcore.clock]]` setup for that board's `[clock]`
//! (`clock.rs`; the internal one for a node without a board); the build
//! fails without the folder, and without the stamp `dagon gen` writes
//! there once the boot crate's dependency graph passed its check for the
//! current inputs.
//!
//! The links the boot core receives on besides USB (`link.rs`) are the
//! board's DAGON buses (`dagon = true`): its I2C bus as a target at the
//! node's address from the topology (`LINK_ADDR` of its boot wiring, given
//! to `open_i2c!`), else the board's `[default_link] addr`, and its UART bus at the bus's `baud`
//! (else `[default_link] baud`), each with the listen window the node's
//! MCU definition gives its kind (its `mcu.toml` in the boot folder, the
//! project's overrides applied). A node without such a bus opens no link
//! of that kind.

use std::fmt::Write;
use std::{env, fs, path::PathBuf};

use dagon_topology::defs::board_boot::{BoardBus, BusKind, HaltPattern};
use dagon_topology::defs::codegen::{boot_build, boot_verify_stamp, bootcore_clock_build};
use dagon_topology::defs::{BoardDef, McuDef};
use dagon_topology::schema::LinkKind;

fn main() {
    boot_verify_stamp();
    let board = boot_build(None);
    bootcore_clock_build(board.as_ref());
    link_build(board.as_ref());
    let Some(b) = board else {
        return;
    };
    let boot = b.boot.unwrap_or_default();
    assert!(
        boot.halt.map(|h| h.pattern) != Some(HaltPattern::Code),
        "board {}: this boot image shows a halt as blink or off, not code",
        b.id
    );
    assert!(
        boot.led.is_none()
            || boot
                .led
                .as_deref()
                .and_then(|p| p.strip_prefix("GPIO"))
                .is_some(),
        "board {}: [boot] led must be a GPIOn pin",
        b.id
    );
}

/// The embassy-rp name of a definition pin (`GPIO16` → `PIN_16`).
fn pin(board: &str, p: &Option<String>) -> String {
    let p = p.as_deref().expect("a DAGON bus names its pins");
    let n = p
        .strip_prefix("GPIO")
        .unwrap_or_else(|| panic!("board {board}: pin {p} is not a GPIOn pin"));
    format!("PIN_{n}")
}

fn dagon_bus(board: Option<&BoardDef>, kind: BusKind) -> Option<&BoardBus> {
    board?.buses.iter().find(|x| x.dagon && x.kind == kind)
}

/// Writes `link.rs`: `LISTEN_WINDOW_MS`, the link types `I2cLink` and
/// `UartLink` (`NoLink` for a kind the node does not open) and
/// `open_i2c!(p)` / `open_uart!(p)`, which open them from the HAL's
/// peripherals `p`.
fn link_build(board: Option<&BoardDef>) {
    let folder = PathBuf::from(env::var_os("DAGON_BOOT_CRATE").expect("DAGON_BOOT_CRATE"));
    let path = folder.join("mcu.toml");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mcu: McuDef = toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let id = board.map_or("", |b| b.id.as_str());
    let default = board.and_then(|b| b.default_link.as_ref());
    let i2c = dagon_bus(board, BusKind::I2c).map(|bus| (bus, default.and_then(|d| d.addr)));
    let uart = dagon_bus(board, BusKind::Uart).map(|bus| {
        let baud = bus.baud.or(default.and_then(|d| d.baud));
        (
            bus,
            baud.unwrap_or_else(|| panic!("board {id}: its DAGON UART bus states no baud")),
        )
    });
    let mut window = 0;
    let mut irqs = String::new();
    let mut rs = String::new();
    match i2c {
        Some((bus, addr)) => {
            window = window.max(mcu.listen_window_ms(LinkKind::I2c));
            let ctl = bus.controller.to_uppercase();
            assert!(
                ctl == "I2C0" || ctl == "I2C1",
                "board {id}: DAGON I2C bus on {ctl}"
            );
            let _ = writeln!(
                irqs,
                "    {ctl}_IRQ => embassy_rp::i2c::InterruptHandler<embassy_rp::peripherals::{ctl}>;"
            );
            let _ = write!(
                rs,
                "/// The board's DAGON bus `{}` as the boot core's I2C target, at the\n\
                 /// node's address from the topology, else the board's {addr:?}.\n\
                 pub type I2cLink = dagon_mcu_rp2040::boot_links::I2cTarget<'static, embassy_rp::peripherals::{ctl}>;\n\
                 macro_rules! open_i2c {{\n    ($p:ident, $addr:expr) => {{\n        \
                 match dagon_mcu_rp2040::boot_receive::link_addr($addr, {addr:?}) {{\n            Some(a) => {{\n                \
                 let mut c = embassy_rp::i2c_slave::Config::default();\n                \
                 c.addr = u16::from(a);\n                c.general_call = false;\n                \
                 let bus = embassy_rp::i2c_slave::I2cSlave::new($p.{ctl}, $p.{}, $p.{}, LinkIrqs, c);\n                \
                 Some(dagon_mcu_rp2040::boot_links::I2cTarget::new(bus, embassy_rp::pac::{ctl}, a))\n            \
                 }}\n            None => None,\n        }}\n    }};\n}}\n",
                bus.id,
                pin(id, &bus.scl),
                pin(id, &bus.sda),
            );
        }
        None => rs.push_str(
            "/// No DAGON I2C bus: no I2C link.\n\
             pub type I2cLink = dagon_mcu_rp2040::boot_receive::NoLink;\n\
             macro_rules! open_i2c {\n    ($p:ident, $addr:expr) => {{\n        \
             let _: Option<u8> = $addr;\n        None::<I2cLink>\n    }};\n}\n",
        ),
    }
    match uart {
        Some((bus, baud)) => {
            window = window.max(mcu.listen_window_ms(LinkKind::Uart));
            let ctl = bus.controller.to_uppercase();
            let (tx, rx) = (pin(id, &bus.tx), pin(id, &bus.rx));
            let (ty, open) = match ctl.as_str() {
                "UART0" | "UART1" => {
                    let _ = writeln!(
                        irqs,
                        "    {ctl}_IRQ => embassy_rp::uart::BufferedInterruptHandler<embassy_rp::peripherals::{ctl}>;"
                    );
                    (
                        "dagon_mcu_rp2040::boot_links::HwUart".to_string(),
                        format!(
                            "static BUFS: static_cell::StaticCell<([u8; 64], [u8; 2048])> = static_cell::StaticCell::new();\n        \
                             let (tb, rb) = BUFS.init(([0; 64], [0; 2048]));\n        \
                             let mut c = embassy_rp::uart::Config::default();\n        c.baudrate = {baud};\n        \
                             let s = dagon_mcu_rp2040::boot_links::HwUart(embassy_rp::uart::BufferedUart::new($p.{ctl}, $p.{tx}, $p.{rx}, LinkIrqs, tb, rb, c));"
                        ),
                    )
                }
                "PIO0" | "PIO1" => {
                    let _ = writeln!(
                        irqs,
                        "    {ctl}_IRQ_0 => embassy_rp::pio::InterruptHandler<embassy_rp::peripherals::{ctl}>;"
                    );
                    (
                        format!("dagon_mcu_rp2040::links::PioStream<'static, embassy_rp::peripherals::{ctl}, 0, 1>"),
                        format!(
                            "use embassy_rp::pio_programs::uart::*;\n        \
                             let embassy_rp::pio::Pio {{ mut common, sm0, sm1, .. }} = embassy_rp::pio::Pio::new($p.{ctl}, LinkIrqs);\n        \
                             let tx_prog = PioUartTxProgram::new(&mut common);\n        \
                             let rx_prog = PioUartRxProgram::new(&mut common);\n        \
                             let tx = PioUartTx::new({baud}, &mut common, sm0, $p.{tx}, &tx_prog);\n        \
                             let rx = PioUartRx::new({baud}, &mut common, sm1, $p.{rx}, &rx_prog);\n        \
                             // The PIO serves the link until the boot core leaves (links released).\n        \
                             core::mem::forget(common);\n        \
                             let s = dagon_mcu_rp2040::links::PioStream {{ tx, rx }};"
                        ),
                    )
                }
                _ => panic!("board {id}: a DAGON UART bus on {ctl}, not a UART or a PIO"),
            };
            let _ = write!(
                rs,
                "/// The board's DAGON bus `{}` as the boot core's UART at {baud} bit/s.\n\
                 pub type UartLink = dagon_mcu_rp2040::boot_links::UartTarget<{ty}>;\n\
                 macro_rules! open_uart {{\n    ($p:ident) => {{{{\n        {open}\n        \
                 Some(dagon_mcu_rp2040::boot_links::UartTarget::new(s))\n    }}}};\n}}\n",
                bus.id,
            );
        }
        None => rs.push_str(
            "/// No DAGON UART bus: no UART link.\n\
             pub type UartLink = dagon_mcu_rp2040::boot_receive::NoLink;\n\
             macro_rules! open_uart {\n    ($p:ident) => {\n        None::<UartLink>\n    };\n}\n",
        ),
    }
    if !irqs.is_empty() {
        let _ = write!(
            rs,
            "embassy_rp::bind_interrupts!(struct LinkIrqs {{\n{irqs}}});\n"
        );
    }
    let _ = write!(
        rs,
        "/// The listen window on the links that take part, ms (0: none).\n\
         pub const LISTEN_WINDOW_MS: u16 = {window};\n"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("link.rs"), rs).expect("OUT_DIR is writable");
}
