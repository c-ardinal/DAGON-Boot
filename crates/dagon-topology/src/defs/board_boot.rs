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
    /// What the board package's code takes the pin by (`lcd_cs`):
    /// `a-z`, `0-9`, `_`, starting with a letter.
    pub name: Option<String>,
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
    /// DAGON's own link runs on it: never an extension's bus, and its
    /// controller never an extension's window.
    #[serde(default)]
    pub dagon: bool,
    /// Its pins, by the kind's signals ([`BusKind::signals`]).
    pub sda: Option<String>,
    pub scl: Option<String>,
    pub tx: Option<String>,
    pub rx: Option<String>,
    pub sck: Option<String>,
    pub mosi: Option<String>,
    pub miso: Option<String>,
    /// Clock of an I2C or SPI bus.
    pub hz: Option<u32>,
    /// Bit rate of a UART or CAN bus.
    pub baud: Option<u32>,
}

impl BoardBus {
    /// Its pins as (signal, pin), in the kind's signal order.
    pub fn pins(&self) -> Vec<(&'static str, &str)> {
        [
            ("sda", &self.sda),
            ("scl", &self.scl),
            ("tx", &self.tx),
            ("rx", &self.rx),
            ("sck", &self.sck),
            ("mosi", &self.mosi),
            ("miso", &self.miso),
        ]
        .into_iter()
        .filter_map(|(s, p)| p.as_deref().map(|p| (s, p)))
        .collect()
    }
}

impl BusKind {
    /// The signals a bus of this kind names: (required, optional); a UART
    /// and an SPI bus need at least one of their data signals.
    pub fn signals(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            BusKind::I2c => (&["sda", "scl"], &[]),
            BusKind::Spi => (&["sck"], &["mosi", "miso"]),
            BusKind::Uart => (&[], &["tx", "rx"]),
            BusKind::Can => (&["tx", "rx"], &[]),
        }
    }

    /// Whether the bus has a clock (`hz`) rather than a bit rate (`baud`).
    pub fn clocked(self) -> bool {
        matches!(self, BusKind::I2c | BusKind::Spi)
    }
}

/// `[[peripheral]]`: a board name for one of the MCU's register blocks.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardPeripheral {
    pub id: String,
    /// The MCU definition's `[[peripheral]]` id.
    pub mcu: String,
}

/// A device register by its name: what board package code writes it by.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceReg {
    pub name: String,
    pub reg: u8,
}

/// A register of `writable`: a number, or the name of one of `regs`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum RegRef {
    Reg(u8),
    Name(String),
}

/// `[[device]]`: a device on a board bus, its named registers and the
/// registers anyone outside DAGON may write.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardDevice {
    pub id: String,
    pub bus: String,
    pub addr: u8,
    #[serde(default)]
    pub regs: Vec<DeviceReg>,
    #[serde(default)]
    pub writable: Vec<RegRef>,
}

impl BoardDevice {
    /// The register named `name` in `regs`.
    pub fn reg(&self, name: &str) -> Option<u8> {
        self.regs.iter().find(|r| r.name == name).map(|r| r.reg)
    }

    /// `writable` as register numbers (names resolved; checked on load).
    pub fn writable_regs(&self) -> Vec<u8> {
        self.writable
            .iter()
            .filter_map(|w| match w {
                RegRef::Reg(r) => Some(*r),
                RegRef::Name(n) => self.reg(n),
            })
            .collect()
    }
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
        if let Some(link) = b.buses.iter().find(|l| l.dagon && l.controller == x.mcu) {
            return Err(format!(
                "peripheral '{}': '{}' drives DAGON's link (bus '{}'), so it is never an extension's",
                x.id, x.mcu, link.id
            ));
        }
    }
    for d in &b.devices {
        fail(!code_name(&d.id, b'-'), || {
            format!("device '{}': ids are a-z, 0-9, -, _ from a letter", d.id)
        })?;
        for r in &d.regs {
            fail(!code_name(&r.name, b'_'), || {
                format!(
                    "device '{}': register name '{}' must be a-z, 0-9, _ from a letter",
                    d.id, r.name
                )
            })?;
        }
        unique(
            &format!("register of device '{}'", d.id),
            d.regs.iter().map(|r| r.name.as_str()),
        )?;
        for w in &d.writable {
            if let RegRef::Name(n) = w {
                fail(d.reg(n).is_none(), || {
                    format!("device '{}': writable '{n}' is not in its regs", d.id)
                })?;
            }
        }
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

/// The identifier generated code uses for a definition name (`child-uart`
/// → `child_uart`).
pub fn code_ident(name: &str) -> String {
    name.replace('-', "_")
}

/// Every name board package code is generated with, once each after
/// [`code_ident`]: the boot LED, pin names, buses and their signals.
pub fn code_names(b: &BoardDef) -> Vec<(String, String)> {
    let led = b.boot.as_ref().and_then(|x| x.led.clone());
    let mut out: Vec<(String, String)> = led.map(|p| ("boot_led".into(), p)).into_iter().collect();
    for p in &b.pins {
        if let Some(n) = &p.name {
            out.push((n.clone(), p.pin.clone()));
        }
    }
    for x in &b.buses {
        let bus = code_ident(&x.id);
        out.push((bus.clone(), x.controller.clone()));
        for (s, pin) in x.pins() {
            out.push((format!("{bus}_{s}"), pin.to_string()));
        }
    }
    out
}

/// A name board package code is generated with: `a-z`, `0-9`, `_` (or
/// `-` in ids, `extra`), starting with a letter.
fn code_name(s: &str, extra: u8) -> bool {
    s.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == extra)
}

/// What board package code is generated from: pin names, each bus's pins
/// and speed by its kind, no pin used twice, and the 親機 pins of
/// `[coordinator]` the ones of DAGON's links.
pub(super) fn wiring(b: &BoardDef, _: &McuDef) -> Result<(), String> {
    let mut used = std::collections::BTreeMap::new();
    let mut take = |pin: &str, by: String| match used.insert(pin.to_string(), by.clone()) {
        Some(first) => Err(format!("pin '{pin}' is both {first} and {by}")),
        None => Ok(()),
    };
    for p in &b.pins {
        if let Some(n) = &p.name {
            fail(!code_name(n, b'_'), || {
                format!(
                    "pin '{}': name '{n}' must be a-z, 0-9, _ from a letter",
                    p.pin
                )
            })?;
        }
        take(&p.pin, format!("[[pin]] '{}'", p.pin))?;
    }
    unique("pin name", b.pins.iter().filter_map(|p| p.name.as_deref()))?;
    for x in &b.buses {
        fail(!code_name(&x.id, b'-'), || {
            format!("bus '{}': ids are a-z, 0-9, -, _ from a letter", x.id)
        })?;
        let (need, may) = x.kind.signals();
        let pins = x.pins();
        for s in need {
            fail(!pins.iter().any(|(t, _)| t == s), || {
                format!("bus '{}': a {:?} bus names its {s} pin", x.id, x.kind)
            })?;
        }
        if !may.is_empty() {
            fail(!pins.iter().any(|(t, _)| may.contains(t)), || {
                format!("bus '{}': names at least one of {may:?}", x.id)
            })?;
        }
        for (s, pin) in pins {
            fail(!need.contains(&s) && !may.contains(&s), || {
                format!("bus '{}': a {:?} bus has no {s} pin", x.id, x.kind)
            })?;
            take(pin, format!("{s} of bus '{}'", x.id))?;
        }
        let (speed, other) = if x.kind.clocked() {
            (x.hz, x.baud.map(|_| "baud"))
        } else {
            (x.baud, x.hz.map(|_| "hz"))
        };
        if let Some(o) = other {
            return Err(format!("bus '{}': a {:?} bus has no {o}", x.id, x.kind));
        }
        fail(speed == Some(0), || format!("bus '{}': speed 0", x.id))?;
    }
    unique(
        "generated name (pin, bus, bus signal, boot_led)",
        code_names(b).iter().map(|(n, _)| n.as_str()),
    )?;
    unique(
        "device id after - becomes _",
        b.devices
            .iter()
            .map(|d| code_ident(&d.id))
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str),
    )?;
    coordinator_links(b)
}

/// A board with buses names the 親機 pins of `[coordinator]` (numbers the
/// page wires new links to) as DAGON's links on those pins.
fn coordinator_links(b: &BoardDef) -> Result<(), String> {
    let Some(h) = &b.coordinator else {
        return Ok(());
    };
    if b.buses.is_empty() {
        return Ok(());
    }
    let gpio = |n: u8| format!("GPIO{n}");
    let i2c = b.buses.iter().any(|x| {
        x.dagon
            && x.kind == BusKind::I2c
            && x.controller == h.i2c.controller
            && x.sda.as_deref() == Some(&gpio(h.i2c.sda))
            && x.scl.as_deref() == Some(&gpio(h.i2c.scl))
    });
    fail(!i2c, || {
        "coordinator.i2c is no dagon = true i2c [[bus]] on the same controller and pins".into()
    })?;
    let uart = b.buses.iter().any(|x| {
        x.dagon
            && x.kind == BusKind::Uart
            && x.tx.as_deref() == Some(&gpio(h.pio_uart.tx))
            && x.rx.as_deref() == Some(&gpio(h.pio_uart.rx))
    });
    fail(!uart, || {
        "coordinator.pio_uart is no dagon = true uart [[bus]] on the same pins".into()
    })
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
                fail(!d.writable_regs().contains(reg), || {
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
