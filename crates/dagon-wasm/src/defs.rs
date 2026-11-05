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
            crate::define::set(&o, "label", m.connection.label.as_str().into());
            crate::define::set(&o, "hint", m.connection.hint.as_str().into());
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
                crate::define::set(&o, "tx", tx.into());
                crate::define::set(&o, "rx", rx.into());
                o.into()
            }
            None => JsValue::NULL,
        }
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
