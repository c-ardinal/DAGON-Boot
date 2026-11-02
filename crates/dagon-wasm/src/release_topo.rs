//! The 構成 a release was made for, read back from its topology item as a
//! topology the page can edit ("ファイルの構成を取り込む", ui-design §7.6).
//! The canonical binary carries no ids, names, boards or node pins, so those
//! come from the 構成 on the page wherever a node's tag matches; the rest is
//! the binary's. Compiling the result gives the release's topology bytes again.

use dagon_format::header::{mcu_id, port_kind};
use dagon_format::topology::{self as tp, TopologyView};
use dagon_format::BundleView;
use dagon_topology::schema::{Link, Node};
use dagon_topology::Topology;
use yaml_serde::{Mapping, Value};

fn s(v: &str) -> Value {
    Value::String(v.into())
}

fn n(v: impl Into<u64>) -> Value {
    Value::Number(v.into().into())
}

fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut m = Mapping::new();
    for (k, v) in entries {
        m.insert(s(k), v);
    }
    Value::Mapping(m)
}

fn mcu_name(id: u16) -> Option<&'static str> {
    match id {
        mcu_id::RP2040 => Some("rp2040"),
        mcu_id::STM32F401XE => Some("stm32f401re"),
        mcu_id::ESP32S3 => Some("esp32s3"),
        _ => None,
    }
}

fn port_name(k: u8) -> Option<&'static str> {
    match k {
        port_kind::SELF_BOOT => Some("self-boot"),
        port_kind::VENDOR_STAGE2 => Some("vendor-stage2"),
        _ => None,
    }
}

fn tier_name(t: u8) -> Option<&'static str> {
    match t {
        tp::tier::LOCAL => Some("local"),
        tp::tier::REDISTRIBUTE => Some("redistribute"),
        tp::tier::ROM_FORCED => Some("rom-forced"),
        tp::tier::ROM_COOP => Some("rom-coop"),
        _ => None,
    }
}

fn rom_entry_name(r: u8) -> Option<&'static str> {
    match r {
        tp::rom_entry::FORCE_DOWNLOAD_BOOT => Some("force-download-boot"),
        tp::rom_entry::M5PM1 => Some("m5pm1"),
        tp::rom_entry::STM32_I2C_ROM => Some("stm32-i2c-rom"),
        _ => None,
    }
}

fn tag_text(t: &[u8; 16]) -> String {
    let end = t.iter().position(|&b| b == 0).unwrap_or(t.len());
    String::from_utf8_lossy(&t[..end]).into_owned()
}

/// The release's topology item, or `None` when it has none.
fn topology_bytes(bundle: &[u8]) -> Result<Option<&[u8]>, String> {
    let b = BundleView::parse(bundle).map_err(|e| format!("{e:?}"))?;
    Ok(b.topology_item().map(|i| b.item_bytes(i)))
}

/// The page's node for a binary node: the same tag and MCU.
fn matching<'a>(cur: Option<&'a Topology>, tag: &str, mcu: &str) -> Option<&'a Node> {
    cur?.nodes.iter().find(|x| x.tag == tag && x.mcu == mcu)
}

fn node_value(x: &tp::TopoNode, id: &str, tag: &str, page: Option<&Node>) -> Result<Value, String> {
    let mcu = mcu_name(x.mcu_id).ok_or_else(|| format!("unknown mcu_id {}", x.mcu_id))?;
    let port = port_name(x.port_kind).ok_or_else(|| format!("unknown port {}", x.port_kind))?;
    let role = if x.role == tp::role::COORDINATOR {
        "coordinator"
    } else {
        "node"
    };
    let mut m = vec![("id", s(id)), ("mcu", s(mcu))];
    if let Some(b) = page.and_then(|p| p.board.as_deref()) {
        m.push(("board", s(b)));
    }
    m.extend([("role", s(role)), ("port", s(port))]);
    if let Some(f) = page.and_then(|p| p.flash_map.as_deref()) {
        m.push(("flash_map", s(f)));
    }
    m.extend([
        ("max_attempts", n(x.max_attempts)),
        ("listen_window_ms", n(x.listen_window_ms)),
        ("tag", s(tag)),
    ]);
    Ok(map(m))
}

fn link_value(
    l: &tp::TopoLink,
    ids: &[String],
    bus_ids: &[String],
    page: Option<&Link>,
) -> Result<Value, String> {
    let id = |i: u8| {
        ids.get(i as usize)
            .cloned()
            .ok_or("link to an unknown node")
    };
    let mut m = vec![("from", s(&id(l.from)?)), ("to", s(&id(l.to)?))];
    match l.iface {
        tp::iface::I2C_BUS => {
            let bus = bus_ids.get(l.bus_id as usize).ok_or("unknown bus")?;
            m.extend([("kind", s("i2c")), ("bus", s(bus)), ("addr", n(l.i2c_addr))]);
            if let Some(p) = page.and_then(|p| p.node_pins.as_ref()) {
                m.push(("node_pins", map([("sda", s(&p.sda)), ("scl", s(&p.scl))])));
            }
        }
        tp::iface::HW_UART0 | tp::iface::PIO_UART => {
            let iface = if l.iface == tp::iface::HW_UART0 {
                "hw-uart0"
            } else {
                "pio-uart"
            };
            m.extend([
                ("kind", s("uart")),
                ("iface", s(iface)),
                ("pins", map([("tx", n(l.tx_pin)), ("rx", n(l.rx_pin))])),
                ("baud", n(l.baud)),
            ]);
        }
        other => return Err(format!("unknown iface {other}")),
    }
    let kind = if l.control == tp::control::FULL {
        "full"
    } else {
        "cooperative"
    };
    let mut c = vec![("kind", s(kind))];
    if l.nrst_pin != tp::NONE {
        c.push(("nrst", n(l.nrst_pin)));
    }
    if l.boot_pin != tp::NONE {
        c.push(("boot", n(l.boot_pin)));
    }
    if let Some(r) = rom_entry_name(l.rom_entry) {
        c.push(("rom_entry", s(r)));
    }
    m.push(("control", map(c)));
    Ok(map(m))
}

/// Buses by `bus_id`: the page's bus at that position where there is one.
fn buses(v: &TopologyView<'_>, cur: Option<&Topology>) -> (Vec<String>, Vec<Value>) {
    let links: Vec<tp::TopoLink> = (0..v.link_count()).map(|i| v.link(i)).collect();
    let count = links
        .iter()
        .filter(|l| l.iface == tp::iface::I2C_BUS)
        .map(|l| l.bus_id as usize + 1)
        .max()
        .unwrap_or(0);
    (0..count)
        .map(|b| {
            let page = cur.and_then(|t| t.buses.get(b));
            let id = page.map_or_else(|| format!("i2c-{b}"), |p| p.id.clone());
            let l = links
                .iter()
                .find(|l| l.iface == tp::iface::I2C_BUS && l.bus_id as usize == b);
            let (sda, scl, hz) = l.map_or((0, 0, 0), |l| (l.tx_pin, l.rx_pin, l.baud));
            let value = map([
                ("id", s(&id)),
                ("kind", s("i2c")),
                (
                    "controller",
                    s(page.map_or("i2c0", |p| p.controller.as_str())),
                ),
                ("sda", n(sda)),
                ("scl", n(scl)),
                ("speed_hz", n(hz)),
                (
                    "pullups",
                    s(page
                        .and_then(|p| p.pullups.as_deref())
                        .unwrap_or("external")),
                ),
            ]);
            (id, value)
        })
        .unzip()
}

/// The release's 構成 as a topology value, with the page's ids, boards, node
/// pins, buses' names and constraints where they match; `None` without a
/// topology item.
pub fn release_topology_value(bundle: &[u8], current: &str) -> Result<Option<Value>, String> {
    let Some(bytes) = topology_bytes(bundle)? else {
        return Ok(None);
    };
    let v = TopologyView::parse(bytes).map_err(|e| format!("{e:?}"))?;
    let cur = dagon_topology::parse_str(current).ok();
    let cur = cur.as_ref();
    let raw: Vec<tp::TopoNode> = (0..v.node_count()).map(|i| v.node(i)).collect();
    let mut ids = Vec::with_capacity(raw.len());
    let mut nodes = Vec::with_capacity(raw.len());
    let mut recovery = Mapping::new();
    for x in &raw {
        let tag = tag_text(&x.target_tag);
        let page = mcu_name(x.mcu_id).and_then(|m| matching(cur, &tag, m));
        let id = page.map_or_else(|| tag.clone(), |p| p.id.clone());
        nodes.push(node_value(x, &id, &tag, page)?);
        let tiers: Vec<Value> = x
            .recovery_tiers
            .iter()
            .filter_map(|&t| tier_name(t))
            .map(s)
            .collect();
        recovery.insert(s(&id), Value::Sequence(tiers));
        ids.push(id);
    }
    let (bus_ids, bus_values) = buses(&v, cur);
    let links = (0..v.link_count())
        .map(|i| {
            let l = v.link(i);
            let to = ids.get(l.to as usize);
            let page = cur.and_then(|t| t.links.iter().find(|p| Some(&p.to) == to));
            link_value(&l, &ids, &bus_ids, page)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut order: Vec<(u8, &String)> = raw.iter().map(|x| x.order_rank).zip(&ids).collect();
    order.sort_by_key(|(rank, _)| *rank);
    let mut top = vec![
        ("dagon", n(1u8)),
        ("name", s(cur.map_or("release", |t| t.name.as_str()))),
        ("nodes", Value::Sequence(nodes)),
    ];
    if !bus_values.is_empty() {
        top.push(("buses", Value::Sequence(bus_values)));
    }
    if !links.is_empty() {
        top.push(("links", Value::Sequence(links)));
    }
    top.push(("recovery", Value::Mapping(recovery)));
    let order = order.into_iter().map(|(_, id)| s(id)).collect();
    top.push(("update", map([("order", Value::Sequence(order))])));
    let constraints = yaml_serde::from_str::<Value>(current)
        .ok()
        .and_then(|c| c.get("constraints").cloned());
    if let Some(c) = constraints {
        top.push(("constraints", c));
    }
    Ok(Some(map(top)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESK: &str = include_str!("../../../topology/desk-demo.yaml");
    const RELEASE: &[u8] = include_bytes!("../../dagon-sim/fixtures/desk-1.1.0.dgb");

    fn release_topo() -> Vec<u8> {
        topology_bytes(RELEASE).unwrap().unwrap().to_vec()
    }

    #[test]
    fn the_release_topology_compiles_back_to_its_bytes() {
        let v = release_topology_value(RELEASE, DESK).unwrap().unwrap();
        let yaml = yaml_serde::to_string(&v).unwrap();
        assert_eq!(dagon_topology::compile_str(&yaml).unwrap(), release_topo());
        // The page's ids and boards are kept where the tags match.
        let t = dagon_topology::parse_str(&yaml).unwrap();
        let ids: Vec<&str> = t.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["hub", "seg", "watch"]);
        assert_eq!(t.nodes[1].board.as_deref(), Some("nucleo-f401re"));
    }

    #[test]
    fn a_different_page_topology_still_reads_the_release() {
        let other = DESK
            .replace("tag: seg-f401-7seg", "tag: other")
            .replace("id: seg", "id: x");
        let v = release_topology_value(RELEASE, &other).unwrap().unwrap();
        let yaml = yaml_serde::to_string(&v).unwrap();
        assert!(yaml.contains("id: seg-f401-7seg"), "{yaml}");
        // The binary has no node pins: the define screen asks for them (V-01).
        let report = dagon_topology::check_str(&yaml);
        assert!(report.issues.iter().any(|i| i.rule == "V-01"), "{report:?}");
    }

    #[test]
    fn not_a_release_is_an_error() {
        assert!(release_topology_value(b"nope", DESK).is_err());
    }
}
