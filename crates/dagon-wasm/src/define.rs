//! Define mode: reading a topology YAML into a plain object the page edits,
//! and the board table's defaults for placing a node. The page writes the
//! object back as YAML (or as JSON, which the YAML parser reads too) and
//! checks it with `topo_check`, so every rule stays in dagon-topology.

use dagon_topology::boards::{self, BOARDS};
use dagon_topology::schema::{ControlKind, LinkKind, RomEntry, UartIface};
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;
use yaml_serde::Value;

fn set(o: &Object, k: &str, v: JsValue) {
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

fn link_kind(k: LinkKind) -> &'static str {
    k.name()
}

/// The MCU table for choosing a node by MCU: `[{id, port, flashMap,
/// flashBytes, gpioCount, boards: [board id]}]`. `flashBytes` is what the
/// flash map needs, so a board without that much flash cannot hold it.
#[wasm_bindgen]
pub fn supported_mcus() -> JsValue {
    boards::MCUS
        .iter()
        .map(|m| {
            let o = Object::new();
            set(&o, "id", m.id.into());
            set(&o, "port", m.port.into());
            let fm = boards::default_flash_map(m.id);
            set(&o, "flashMap", fm.map_or(JsValue::NULL, |f| f.name.into()));
            set(
                &o,
                "flashBytes",
                fm.map_or(JsValue::NULL, |f| f.bytes.into()),
            );
            set(&o, "gpioCount", m.gpio_count.into());
            let names: Array = BOARDS
                .iter()
                .filter(|b| b.mcu == m.id)
                .map(|b| JsValue::from(b.mcu_name))
                .take(1)
                .collect();
            set(&o, "name", names.get(0));
            JsValue::from(o)
        })
        .collect::<Array>()
        .into()
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
    let Some(b) = BOARDS.iter().find(|b| b.id == id) else {
        return JsValue::NULL;
    };
    let o = Object::new();
    set(&o, "mcu", b.mcu.into());
    let port = boards::MCUS.iter().find(|m| m.id == b.mcu).map(|m| m.port);
    set(&o, "port", port.map_or(JsValue::NULL, JsValue::from));
    let flash = boards::FLASH_MAPS
        .iter()
        .find(|f| f.mcu == b.mcu)
        .map(|f| f.name);
    set(&o, "flashMap", flash.map_or(JsValue::NULL, JsValue::from));
    set(&o, "i2cTarget", b.i2c_target.into());
    let header: Array = b.header.iter().map(|h| JsValue::from(h.pin)).collect();
    set(&o, "header", header.into());
    let pairs: Array = b
        .i2c_pairs
        .iter()
        .map(|p| {
            let x = Object::new();
            set(&x, "sda", p.sda.into());
            set(&x, "scl", p.scl.into());
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
    o.into()
}
