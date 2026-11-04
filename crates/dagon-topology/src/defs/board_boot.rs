//! What a board definition gives the boot stage and extensions: `[boot]`
//! (the LED, the halt pattern, the power hold line and the boot action
//! program) and the tables extensions' capabilities are checked against
//! (`[[pin]]`, `[[bus]]`, `[[peripheral]]`, `[[device]]`).

use serde::Deserialize;

use super::isolation::BootCapability;
use super::{BoardDef, McuDef};

/// Most ops one event's program holds (`boot-program` format 1).
pub const PROGRAM_OPS_MAX: usize = 32;
/// Longest single `delay`.
pub const PROGRAM_DELAY_MAX_MS: u32 = 20;
/// Most time one event's program takes, bus ops at their timeout.
pub const PROGRAM_TOTAL_MAX_MS: u32 = 50;
/// The timeout a `bus_write` counts as.
pub const PROGRAM_BUS_TIMEOUT_MS: u32 = 5;
/// Most bytes one `bus_write` sends.
pub const PROGRAM_BUS_BYTES_MAX: usize = 8;
/// The `boot-program` format number a generated program table has.
pub const BOOT_PROGRAM_FORMAT: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Active {
    #[default]
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HaltPattern {
    Blink,
    Code,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Halt {
    pub pattern: HaltPattern,
    #[serde(default)]
    pub period_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PinLevel {
    High,
    Low,
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerHold {
    pub pin: String,
    pub level: PinLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LedState {
    On,
    Off,
    Toggle,
}

/// One op of a boot action program (`boot-program` format 1).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    Led {
        state: LedState,
    },
    Gpio {
        pin: String,
        level: PinLevel,
    },
    Delay {
        ms: u32,
    },
    BusWrite {
        device: String,
        reg: u8,
        bytes: Vec<u8>,
    },
}

/// `[[boot.action]]`: ops run when a boot event happens.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    /// A boot event kind (without the `boot.` prefix) the catalog marks
    /// action-capable; checked by `dagon gen` against the catalog.
    pub on: String,
    pub ops: Vec<Op>,
}

/// `[boot]` of a board definition.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardBoot {
    pub led: Option<String>,
    #[serde(default)]
    pub led_active: Active,
    pub halt: Option<Halt>,
    pub power_hold: Option<PowerHold>,
    #[serde(default, rename = "action")]
    pub actions: Vec<Action>,
}

/// `[[pin]]`: a pin extensions and boot actions may name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardPin {
    pub pin: String,
    /// A boot action program may drive it.
    #[serde(default)]
    pub boot: bool,
    /// Levels an extension or program may set (all when empty).
    #[serde(default)]
    pub levels: Vec<PinLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BusKind {
    I2c,
    Spi,
    Uart,
    Can,
}

/// `[[bus]]`: a bus extensions reach through the bus service.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardBus {
    pub id: String,
    pub kind: BusKind,
    /// The MCU `[[peripheral]]` that drives it.
    pub controller: String,
}

/// `[[peripheral]]`: a board name for one of the MCU's register blocks.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardPeripheral {
    pub id: String,
    /// The MCU definition's `[[peripheral]]` id.
    pub mcu: String,
}

/// `[[device]]`: a device on a board bus and the registers anyone outside
/// DAGON may write.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardDevice {
    pub id: String,
    pub bus: String,
    pub addr: u8,
    #[serde(default)]
    pub writable: Vec<u8>,
}

fn fail(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Err(msg())
    } else {
        Ok(())
    }
}

fn unique<'a>(what: &str, ids: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        fail(id.is_empty() || !seen.insert(id), || {
            format!("{what} '{id}' is empty or defined twice")
        })?;
    }
    Ok(())
}

impl BoardDef {
    pub fn pin(&self, name: &str) -> Option<&BoardPin> {
        self.pins.iter().find(|p| p.pin == name)
    }

    pub fn bus(&self, id: &str) -> Option<&BoardBus> {
        self.buses.iter().find(|b| b.id == id)
    }

    pub fn device(&self, id: &str) -> Option<&BoardDevice> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn board_peripheral(&self, id: &str) -> Option<&BoardPeripheral> {
        self.peripherals.iter().find(|p| p.id == id)
    }
}

/// The capability tables: unique names, buses and peripherals naming the
/// MCU's blocks, devices on declared buses at usable addresses.
pub(super) fn tables(b: &BoardDef, m: &McuDef) -> Result<(), String> {
    unique("pin", b.pins.iter().map(|p| p.pin.as_str()))?;
    unique("bus", b.buses.iter().map(|x| x.id.as_str()))?;
    unique("peripheral", b.peripherals.iter().map(|x| x.id.as_str()))?;
    unique("device", b.devices.iter().map(|x| x.id.as_str()))?;
    for x in &b.buses {
        fail(m.peripheral(&x.controller).is_none(), || {
            format!(
                "bus '{}': controller '{}' is no [[peripheral]] of {}",
                x.id, x.controller, m.id
            )
        })?;
    }
    for x in &b.peripherals {
        fail(m.peripheral(&x.mcu).is_none(), || {
            format!(
                "peripheral '{}': '{}' is no [[peripheral]] of {}",
                x.id, x.mcu, m.id
            )
        })?;
    }
    for d in &b.devices {
        let bus = b
            .bus(&d.bus)
            .ok_or_else(|| format!("device '{}': bus '{}' is not in [[bus]]", d.id, d.bus))?;
        fail(
            bus.kind == BusKind::I2c && !super::requirements::I2C_ADDRS.contains(&d.addr),
            || {
                format!(
                    "device '{}': addr {:#04x} is outside 0x08..=0x77",
                    d.id, d.addr
                )
            },
        )?;
    }
    Ok(())
}

/// `[boot]`: pins it names are declared, the program within its limits
/// and using only what the board and MCU allow at boot. Which events a
/// program may run on is the catalog's (checked by `dagon gen`).
pub(super) fn boot(b: &BoardDef, m: &McuDef) -> Result<(), String> {
    let Some(boot) = &b.boot else { return Ok(()) };
    for pin in boot
        .led
        .iter()
        .chain(boot.power_hold.iter().map(|p| &p.pin))
    {
        fail(b.pin(pin).is_none(), || {
            format!("boot: pin '{pin}' is not in [[pin]]")
        })?;
    }
    if let Some(h) = boot.halt {
        fail(h.pattern != HaltPattern::Off && h.period_ms == 0, || {
            "boot.halt.period_ms must be above 0 for a blinking pattern".into()
        })?;
    }
    for a in &boot.actions {
        program(b, m, boot, a)?;
    }
    Ok(())
}

fn program(b: &BoardDef, m: &McuDef, boot: &BoardBoot, a: &Action) -> Result<(), String> {
    let at = format!("boot.action on '{}'", a.on);
    fail(a.ops.is_empty() || a.ops.len() > PROGRAM_OPS_MAX, || {
        format!("{at}: 1..={PROGRAM_OPS_MAX} ops")
    })?;
    let mut total = 0;
    for op in &a.ops {
        match op {
            Op::Led { .. } => fail(boot.led.is_none(), || {
                format!("{at}: led op on a board without [boot] led")
            })?,
            Op::Gpio { pin, level } => {
                let p = b.pin(pin).filter(|p| p.boot).ok_or_else(|| {
                    format!("{at}: pin '{pin}' is not a [[pin]] with boot = true")
                })?;
                fail(!p.levels.is_empty() && !p.levels.contains(level), || {
                    format!("{at}: pin '{pin}' does not allow {level:?}")
                })?;
                let hold = boot.power_hold.as_ref().filter(|h| &h.pin == pin);
                fail(hold.is_some_and(|h| h.level != *level), || {
                    format!("{at}: the power hold pin '{pin}' takes only its hold level")
                })?;
            }
            Op::Delay { ms } => {
                fail(*ms == 0 || *ms > PROGRAM_DELAY_MAX_MS, || {
                    format!("{at}: delay {ms} ms must be 1..={PROGRAM_DELAY_MAX_MS}")
                })?;
                total += ms;
            }
            Op::BusWrite { device, reg, bytes } => {
                fail(!m.boot_capabilities.contains(&BootCapability::I2c), || {
                    format!(
                        "{at}: bus_write needs boot_capabilities = [\"i2c\"] on {}",
                        m.id
                    )
                })?;
                let d = b
                    .device(device)
                    .ok_or_else(|| format!("{at}: device '{device}' is not in [[device]]"))?;
                fail(!d.writable.contains(reg), || {
                    format!("{at}: register {reg:#04x} of '{device}' is not writable")
                })?;
                fail(
                    bytes.is_empty() || bytes.len() > PROGRAM_BUS_BYTES_MAX,
                    || format!("{at}: bus_write sends 1..={PROGRAM_BUS_BYTES_MAX} bytes"),
                )?;
                total += PROGRAM_BUS_TIMEOUT_MS;
            }
        }
    }
    fail(total > PROGRAM_TOTAL_MAX_MS, || {
        format!(
            "{at}: {total} ms is over the {PROGRAM_TOTAL_MAX_MS} ms an event's program may take"
        )
    })
}
