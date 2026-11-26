//! The MCU and board definitions the page works with: the builtin set
//! until `load_definitions` gives the JSON `dagon serve` sends (the
//! project's DAGON source and its own boards and MCUs).

use std::cell::RefCell;

use dagon_topology::defs::Registry;
use js_sys::Object;
use wasm_bindgen::prelude::*;

thread_local! {
    static LOADED: RefCell<Option<Registry>> = const { RefCell::new(None) };
}

/// Runs `f` with the definitions in use.
pub(crate) fn with_registry<R>(f: impl FnOnce(&Registry) -> R) -> R {
    LOADED.with(|r| match &*r.borrow() {
        Some(reg) => f(reg),
        None => f(Registry::builtin()),
    })
}

/// Replaces the definitions with `json` (`Registry::to_json`'s form, as
/// `/v1/definitions` serves it), validated as on load; throws with the
/// reason when they do not load.
#[wasm_bindgen]
pub fn load_definitions(json: &str) -> Result<(), JsError> {
    let reg = Registry::from_json(json).map_err(|e| JsError::new(&e.to_string()))?;
    LOADED.with(|r| *r.borrow_mut() = Some(reg));
    Ok(())
}

/// Back to the builtin definitions (a project without its own, or whose
/// definitions do not load: nothing of the previous project stays).
#[wasm_bindgen]
pub fn use_builtin_definitions() {
    LOADED.with(|r| *r.borrow_mut() = None);
}

/// How the user connects `mcu` for a write: `{label, hint}`, or null for an
/// unknown MCU.
#[wasm_bindgen]
pub fn mcu_connection(mcu: &str) -> JsValue {
    with_registry(|reg| {
        reg.mcu(mcu).map_or(JsValue::NULL, |m| {
            let o = Object::new();
            crate::js::set(&o, "label", m.connection.label.as_str().into());
            crate::js::set(&o, "hint", m.connection.hint.as_str().into());
            o.into()
        })
    })
}

/// `mcu`'s trial watchdog period in ms (`[watchdog] trial_ms`): how long a
/// new image may take to start and pass its self-check; none without one.
#[wasm_bindgen]
pub fn mcu_trial_ms(mcu: &str) -> Option<u32> {
    with_registry(|reg| {
        reg.mcu(mcu)
            .and_then(|m| m.watchdog.as_ref())
            .map(|w| w.trial_ms)
    })
}

/// The STATUS entries a node of `mcu` fills for `target` (`app`, with the
/// agent's factory slot; `bootcore`; `agent`), by wire slot (0 A, 1 B, 2
/// factory), from its definition's slots; none for an unknown MCU or target.
#[wasm_bindgen]
pub fn status_rows(mcu: &str, target: &str) -> Option<Vec<u8>> {
    use dagon_topology::defs::TargetName;
    let t = match target {
        "app" => TargetName::App,
        "bootcore" => TargetName::Bootcore,
        "agent" => TargetName::Agent,
        _ => return None,
    };
    with_registry(|reg| reg.mcu(mcu).map(|m| m.status_rows(t)))
}

/// Whether `mcu` can be the coordinator (its definition has `[coordinator]`).
#[wasm_bindgen]
pub fn coordinator_capable(mcu: &str) -> bool {
    with_registry(|reg| reg.mcu(mcu).is_some_and(|m| m.coordinator.is_some()))
}

/// `mcu`'s first UART0 pin pair as the coordinator (`[coordinator]`
/// `uart0_tx[0]`, `uart0_rx[0]`): `{tx, rx}`, or null without one.
#[wasm_bindgen]
pub fn coordinator_uart0(mcu: &str) -> JsValue {
    with_registry(|reg| {
        let c = reg.mcu(mcu).and_then(|m| m.coordinator.as_ref());
        match c.and_then(|c| Some((*c.uart0_tx.first()?, *c.uart0_rx.first()?))) {
            Some((tx, rx)) => {
                let o = Object::new();
                crate::js::set(&o, "tx", tx.into());
                crate::js::set(&o, "rx", rx.into());
                o.into()
            }
            None => JsValue::NULL,
        }
    })
}

/// The UART speeds a link between MCUs `a` and `b` may run at: those both MCU
/// definitions' `[link_uart] bauds` name; an end whose definition names
/// none leaves nothing, as the check (V-26) refuses every speed on it. The Console offers only
/// these, as the topology check (V-26) accepts only these.
#[wasm_bindgen]
pub fn uart_bauds(a: &str, b: &str) -> Vec<u32> {
    with_registry(|reg| {
        let lists: Vec<&[u32]> = [a, b]
            .into_iter()
            .map(|id| {
                reg.mcu(id)
                    .and_then(|m| m.link_uart.as_ref())
                    .map_or(&[][..], |u| u.bauds.as_slice())
            })
            .collect();
        let Some((first, rest)) = lists.split_first() else {
            return Vec::new();
        };
        first
            .iter()
            .copied()
            .filter(|b| rest.iter().all(|l| l.contains(b)))
            .collect()
    })
}

/// The I2C bus speeds a bus joining the nodes on `mcus` may run at: those
/// every MCU definition's `[link_i2c] hzs` name; one whose definition names
/// none leaves nothing, as the check (V-20) refuses every speed on it.
#[wasm_bindgen]
pub fn i2c_hzs(mcus: Vec<String>) -> Vec<u32> {
    with_registry(|reg| {
        let lists: Vec<&[u32]> = mcus
            .iter()
            .map(|id| {
                reg.mcu(id)
                    .and_then(|m| m.link_i2c.as_ref())
                    .map_or(&[][..], |u| u.hzs.as_slice())
            })
            .collect();
        let Some((first, rest)) = lists.split_first() else {
            return Vec::new();
        };
        first
            .iter()
            .copied()
            .filter(|b| rest.iter().all(|l| l.contains(b)))
            .collect()
    })
}

/// The `[[link_port]]` name a new end of a link of kind code `kind` on a node of `mcu`
/// (and `board`, or none) starts at, `iface` the UART link's (`hw-uart0`, `pio-uart`); null when
/// the MCU has no port of that kind (dagon_topology::default_port).
#[wasm_bindgen]
pub fn default_port(
    mcu: &str,
    board: Option<String>,
    kind: u8,
    iface: Option<String>,
) -> Option<String> {
    use dagon_topology::schema::UartIface;
    let kind = crate::kinds::of_code(kind)?;
    let iface = match iface.as_deref() {
        Some("hw-uart0") => Some(UartIface::HwUart0),
        Some("pio-uart") => Some(UartIface::PioUart),
        _ => None,
    };
    with_registry(|reg| {
        dagon_topology::default_port(reg, mcu, board.as_deref(), kind, iface).map(str::to_string)
    })
}

/// The pins a hw-uart0 link from a node on `mcu` (and `board`, or none) may use:
/// `{tx: [..], rx: [..]}`, what V-09 and V-07 accept together.
#[wasm_bindgen]
pub fn uart0_choices(mcu: &str, board: Option<String>) -> JsValue {
    with_registry(|reg| {
        let (tx, rx) = dagon_topology::uart0_choices(reg, mcu, board.as_deref());
        let o = Object::new();
        let _ = js_sys::Reflect::set(
            &o,
            &"tx".into(),
            &tx.into_iter().map(JsValue::from).collect::<js_sys::Array>(),
        );
        let _ = js_sys::Reflect::set(
            &o,
            &"rx".into(),
            &rx.into_iter().map(JsValue::from).collect::<js_sys::Array>(),
        );
        o.into()
    })
}

/// The pin pairs `mcu`'s ROM I2C bootloader listens on: `[{sda, scl}]`, what V-13 accepts.
#[wasm_bindgen]
pub fn rom_i2c_pairs(mcu: &str) -> JsValue {
    with_registry(|reg| {
        let pairs = reg
            .mcu(mcu)
            .and_then(|m| m.rom_i2c.as_ref())
            .map_or(&[][..], |r| r.pairs.as_slice());
        pairs
            .iter()
            .map(|p| {
                let o = Object::new();
                let _ = js_sys::Reflect::set(&o, &"sda".into(), &p.sda.as_str().into());
                let _ = js_sys::Reflect::set(&o, &"scl".into(), &p.scl.as_str().into());
                JsValue::from(o)
            })
            .collect::<js_sys::Array>()
            .into()
    })
}

/// The I2C addresses of the ROM loaders defined (never probed with DAGON
/// frames, reserved on a bus with a node reached through its ROM).
#[wasm_bindgen]
pub fn rom_i2c_addrs() -> Vec<u8> {
    with_registry(|reg| {
        reg.mcus()
            .iter()
            .filter_map(|m| m.rom_i2c.as_ref().map(|r| r.addr))
            .collect()
    })
}
