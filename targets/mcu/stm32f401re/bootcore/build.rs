//! The halt LED and pattern come from the board of the node whose boot
//! folder `DAGON_BOOT_CRATE` names (`board.rs`, `pins.rs`), and the clock
//! from the MCU's `[[bootcore.clock]]` setup for that board's `[clock]`
//! (`clock.rs`; the internal one for a node without a board); the build
//! fails without the folder, and without the stamp `dagon gen` writes
//! there once the boot crate's dependency graph passed its check for the
//! current inputs.

//!
//! The link the boot core receives on (`link.rs`) is the board's DAGON
//! I2C bus (`dagon = true`) at the board's `[default_link] addr`, with
//! the listen window the node's MCU definition gives for I2C (its
//! `mcu.toml` in the boot folder, the project's overrides applied). A node
//! without such a bus has no link in its boot core.

use std::{env, fs, path::PathBuf};

use dagon_topology::defs::board_boot::BusKind;
use dagon_topology::defs::codegen::{boot_build, boot_verify_stamp, bootcore_clock_build, Hal};
use dagon_topology::defs::{BoardDef, McuDef};
use dagon_topology::schema::LinkKind;

/// Writes `link.rs`: `LISTEN_WINDOW_MS` and `open_link!(p)`, which opens
/// the link from the HAL's peripherals `p` (`None` without one).
fn link_build(board: Option<&BoardDef>) {
    let folder = PathBuf::from(env::var_os("DAGON_BOOT_CRATE").expect("DAGON_BOOT_CRATE"));
    let path = folder.join("mcu.toml");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mcu: McuDef = toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let bus = board.and_then(|b| {
        let bus = b.buses.iter().find(|x| x.dagon && x.kind == BusKind::I2c)?;
        let addr = b.default_link.as_ref()?.addr?;
        Some((bus, addr))
    });
    let rs = match bus {
        Some((bus, addr)) => {
            let up = |s: &Option<String>| {
                s.as_deref()
                    .expect("an I2C bus names its pins")
                    .to_uppercase()
            };
            let ctl = bus.controller.to_uppercase();
            format!(
                "/// The listen window on the link, ms (0: none).\n\
                 pub const LISTEN_WINDOW_MS: u16 = {};\n\
                 /// The board's DAGON bus `{}` as the boot core's I2C target at {addr:#04x}.\n\
                 macro_rules! open_link {{\n    ($p:ident) => {{\n        \
                 Some(dagon_mcu_stm32f401re::boot_link::I2cPort::new(\n            \
                 $p.{ctl}, $p.{}, $p.{}, {addr:#04x}, embassy_stm32::pac::{ctl},\n        ))\n    }};\n}}\n",
                mcu.listen_window_ms(LinkKind::I2c),
                bus.id,
                up(&bus.scl),
                up(&bus.sda),
            )
        }
        None => "/// No link: no listen window.\npub const LISTEN_WINDOW_MS: u16 = 0;\n\
                 macro_rules! open_link {\n    ($p:ident) => {\n        \
                 None::<dagon_mcu_stm32f401re::boot_link::I2cPort>\n    };\n}\n"
            .into(),
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(out.join("link.rs"), rs).unwrap();
}

fn main() {
    boot_verify_stamp();
    let name = |s: &str| s.to_uppercase();
    let b = boot_build(Some(&Hal {
        peripherals: "embassy_stm32::peripherals",
        name: &name,
        lifetime: false,
    }));
    bootcore_clock_build(b.as_ref());
    link_build(b.as_ref());
    println!("cargo::rustc-check-cfg=cfg(boot_led)");
    let Some(b) = b else {
        return;
    };
    if b.boot.as_ref().is_some_and(|x| x.led.is_some()) {
        println!("cargo::rustc-cfg=boot_led");
    }
    let code = b.boot.and_then(|x| x.halt).map(|h| h.pattern);
    assert!(
        code != Some(dagon_topology::defs::board_boot::HaltPattern::Code),
        "board {}: this boot core shows a halt as blink or off, not code",
        b.id
    );
}
