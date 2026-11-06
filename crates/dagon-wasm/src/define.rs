//! Define mode: reading a topology YAML into a plain object the page edits,
//! and the board definitions' defaults for placing a node. The page writes the
//! object back as YAML (or as JSON, which the YAML parser reads too) and
//! checks it with `topo_check`, so every rule stays in dagon-topology.

use crate::defs::with_registry;
use dagon_topology::schema::{ControlKind, LinkKind, RomEntry, UartIface};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;
use yaml_serde::Value;

pub(crate) fn set(o: &Object, k: &str, v: JsValue) {
    Reflect::set(o, &k.into(), &v).expect("plain object");
}

fn to_js(v: &Value) -> JsValue {
    match v {
        Value::Null => JsValue::NULL,
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => n
            .as_i64()
            .map(|i| JsValue::from(i as f64))
            .or_else(|| n.as_f64().map(JsValue::from))
            .unwrap_or(JsValue::NULL),
        Value::String(s) => s.as_str().into(),
        Value::Sequence(s) => s.iter().map(to_js).collect::<Array>().into(),
        Value::Mapping(m) => {
            let o = Object::new();
            for (k, v) in m {
                let key = match k {
                    Value::String(s) => s.clone(),
                    other => yaml_serde::to_string(other)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                };
                set(&o, &key, to_js(v));
            }
            o.into()
        }
        Value::Tagged(t) => to_js(&t.value),
    }
}

/// A topology YAML as a plain object (numbers as numbers, `0x42` as 66);
/// throws with the parser's message when it is not YAML. Whether it is a
/// valid topology is `topo_check`'s answer.
#[wasm_bindgen]
pub fn topo_parse(yaml: &str) -> Result<JsValue, JsError> {
    let v: Value = yaml_serde::from_str(yaml).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(to_js(&v))
}

/// The 構成 release `bundle` was made for, as a plain object like
/// `topo_parse` gives, with the ids, boards and node pins of `current` (the
/// 構成 on the page) where tags match; null when the release has no topology
/// item. Throws when `bundle` is not a release.
#[wasm_bindgen]
pub fn release_topology(bundle: &[u8], current: &str) -> Result<JsValue, JsError> {
    crate::release_topo::release_topology_value(bundle, current)
        .map(|v| v.as_ref().map_or(JsValue::NULL, to_js))
        .map_err(|e| JsError::new(&e))
}

/// The part updating reaches (dagon_topology::cut_with): `{reach: [id],
/// skipped: {id: why}}`. `reach` is in the compiled topology's node order
/// (the indices dagon-host uses); every other node is in `skipped` with
/// `second-hub`, `deep`, `link-kind`, `unreachable`, or
/// `cut-invalid` for all when the part does not pass the checks on its
/// own. `null` when the YAML does not parse or has errors (its problems
/// say what to fix first).
#[wasm_bindgen]
pub fn topo_cut(yaml: &str) -> JsValue {
    let Ok(t) = dagon_topology::parse_str(yaml) else {
        return JsValue::NULL;
    };
    let cut = with_registry(|reg| {
        dagon_topology::check_with(reg, &t)
            .is_pushable()
            .then(|| dagon_topology::cut_with(reg, &t))
    });
    let Some(cut) = cut else {
        return JsValue::NULL;
    };
    let reach: Array = cut
        .reach
        .iter()
        .map(|id| JsValue::from(id.as_str()))
        .collect();
    let skipped = Object::new();
    for (id, why) in &cut.skipped {
        set(&skipped, id, why.code().into());
    }
    let o = Object::new();
    set(&o, "reach", reach.into());
    set(&o, "skipped", skipped.into());
    o.into()
}

fn link_kind(k: LinkKind) -> &'static str {
    k.name()
}

/// Link kinds `mcu`'s firmware implements: `{up: [kind], down: [kind]}`
/// (the MCU definition's `[links]`; the rest are "not available yet").
#[wasm_bindgen]
pub fn link_kinds(mcu: &str) -> JsValue {
    use dagon_topology::links::{available_in, Dir};
    let names = |d: Dir| -> Array {
        with_registry(|reg| {
            available_in(reg, mcu, d)
                .iter()
                .map(|k| JsValue::from(k.name()))
                .collect()
        })
    };
    let o = Object::new();
    set(&o, "up", names(Dir::Up).into());
    set(&o, "down", names(Dir::Down).into());
    o.into()
}

/// The MCU table for choosing a node by MCU: `[{id, port, flashMap,
/// flashBytes, gpioCount, boards: [board id]}]`. `flashBytes` is what the
/// flash map needs, so a board without that much flash cannot hold it.
#[wasm_bindgen]
pub fn supported_mcus() -> JsValue {
    with_registry(|reg| {
        reg.mcus()
            .iter()
            .map(|m| {
                let o = Object::new();
                set(&o, "id", m.id.as_str().into());
                set(&o, "port", m.port.as_str().into());
                let fm = m.default_flash_map();
                set(&o, "flashMap", fm.name.as_str().into());
                set(&o, "flashBytes", fm.bytes.into());
                set(&o, "gpioCount", m.gpio_count.into());
                set(&o, "name", m.name.as_str().into());
                set(&o, "coordinator", m.coordinator.is_some().into());
                JsValue::from(o)
            })
            .collect::<Array>()
            .into()
    })
}

fn control_kind(k: ControlKind) -> &'static str {
    match k {
        ControlKind::Full => "full",
        ControlKind::Cooperative => "cooperative",
    }
}

fn rom_entry(r: RomEntry) -> &'static str {
    match r {
        RomEntry::ForceDownloadBoot => "force-download-boot",
        RomEntry::M5pm1 => "m5pm1",
        RomEntry::Stm32I2cRom => "stm32-i2c-rom",
    }
}

fn iface(i: UartIface) -> &'static str {
    match i {
        UartIface::HwUart0 => "hw-uart0",
        UartIface::PioUart => "pio-uart",
    }
}

/// What the board table says about placing board `id`: `{mcu, port,
/// flashMap, i2cTarget, header: [pin], i2cPairs: [{sda, scl}], link:
/// {kind, iface, addr, baud, control, romEntry} | null}`; null for an
/// unknown board.
#[wasm_bindgen]
pub fn board_template(id: &str) -> JsValue {
    with_registry(|reg| board_template_in(reg, id))
}

fn board_template_in(reg: &dagon_topology::defs::Registry, id: &str) -> JsValue {
    let Some(b) = reg.board(id) else {
        return JsValue::NULL;
    };
    let m = reg.mcu(&b.mcu);
    let o = Object::new();
    set(&o, "mcu", b.mcu.as_str().into());
    set(
        &o,
        "port",
        m.map_or(JsValue::NULL, |m| m.port.as_str().into()),
    );
    let flash = m.map(|m| m.default_flash_map().name.as_str());
    set(&o, "flashMap", flash.map_or(JsValue::NULL, JsValue::from));
    set(&o, "i2cTarget", b.i2c_target.into());
    let header: Array = b
        .header
        .iter()
        .map(|h| JsValue::from(h.pin.as_str()))
        .collect();
    set(&o, "header", header.into());
    let pairs: Array = b
        .i2c_pairs
        .iter()
        .map(|p| {
            let x = Object::new();
            set(&x, "sda", p.sda.as_str().into());
            set(&x, "scl", p.scl.as_str().into());
            JsValue::from(x)
        })
        .collect();
    set(&o, "i2cPairs", pairs.into());
    let link = b.default_link.map_or(JsValue::NULL, |l| {
        let x = Object::new();
        set(&x, "kind", link_kind(l.kind).into());
        set(
            &x,
            "iface",
            l.iface.map_or(JsValue::NULL, |i| iface(i).into()),
        );
        set(&x, "addr", l.addr.map_or(JsValue::NULL, JsValue::from));
        set(&x, "baud", l.baud.map_or(JsValue::NULL, JsValue::from));
        set(&x, "control", control_kind(l.control).into());
        set(
            &x,
            "romEntry",
            l.rom_entry.map_or(JsValue::NULL, |r| rom_entry(r).into()),
        );
        x.into()
    });
    set(&o, "link", link);
    let hub = b.coordinator.as_ref().map_or(JsValue::NULL, |h| {
        let i2c = Object::new();
        set(&i2c, "controller", h.i2c.controller.as_str().into());
        set(&i2c, "sda", h.i2c.sda.into());
        set(&i2c, "scl", h.i2c.scl.into());
        let uart = Object::new();
        set(&uart, "tx", h.pio_uart.tx.into());
        set(&uart, "rx", h.pio_uart.rx.into());
        let x = Object::new();
        set(&x, "i2c", i2c.into());
        set(&x, "pioUart", uart.into());
        x.into()
    });
    set(&o, "coordinator", hub);
    o.into()
}
