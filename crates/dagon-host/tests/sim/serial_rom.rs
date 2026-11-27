//! The cooperative ROM path: the sample's watch (ESP32-S3 on the hub's
//! PIO UART link) switches to its ROM serial loader on ENTER_ROM, and its
//! recovery agent is rewritten through the hub's UART_RAW.

use super::*;
use dagon_format::header::HEADER_SIZE;
use dagon_host::rom::RomSlot;
use dagon_host::serial_rom::{self, SerialRomMap, MANUAL_SYNC_MS};
use dagon_host::{AgentAssets, EspError};
use dagon_sim::{sample_hub, Layout, SimNode, WATCH_UART_PORT};

const SAMPLE: &str = include_str!("../../../../topology/sample.yaml");
const WATCH: u8 = 2;
const V2: Version = Version::new(2, 0, 0);

/// The sample with every node on 1.0.0, and a host that knows its
/// configuration.
fn sample() -> (Sim, Host<Sim, Sim>) {
    let s = sim_of(sample_hub(7));
    let mut h = Host::new(s.clone(), s.clone());
    let topo = dagon_topology::parse_str(SAMPLE).unwrap();
    let reg = dagon_topology::defs::Registry::builtin();
    h.timings = dagon_host::node_timings(reg, &topo).unwrap();
    h.local = dagon_host::local_node(reg, &topo, "hub");
    (s, h)
}

/// The simulated watch keeps its recovery agent as the layout gives it:
/// the header right before the body.
fn sim_map() -> SerialRomMap {
    let (base, len) = Layout::of("esp32s3").factory.unwrap();
    SerialRomMap {
        mcu_id: image::mcu_id_of("esp32s3").unwrap(),
        flash_bytes: 0x100_0000,
        agent: RomSlot {
            header: base,
            load: base + HEADER_SIZE as u32,
            max_payload: len - HEADER_SIZE as u32,
        },
    }
}

/// A signed recovery agent for the watch, `len` bytes of body.
fn agent(v: Version, len: usize) -> Vec<u8> {
    agent_for("watch-s3", v, len)
}

/// The same for a board of target tag `tag`.
fn agent_for(tag: &str, v: Version, len: usize) -> Vec<u8> {
    let (flags, caps) = image::default_bits(image_kind::AGENT);
    let spec = ImageSpec {
        image_kind: image_kind::AGENT,
        port_kind: image::port_kind_of("vendor-stage2").unwrap(),
        mcu_id: image::mcu_id_of("esp32s3").unwrap(),
        slot: image::slot("esp32s3", image_kind::AGENT, 0).unwrap(),
        version: v,
        build_flags: flags,
        release_seq: 1,
        min_bootcore_version: Version::new(1, 0, 0),
        target_tag: target_tag(tag),
        proto_version: 1,
        capabilities: caps,
    };
    // Every byte value, the loader's frame bytes among them.
    let body: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(13)).collect();
    sign_image(&spec, &body, &key()).unwrap()
}

fn assets<'a>(img: &'a [u8], topo: &'a [u8], maps: &'a [SerialRomMap]) -> AgentAssets<'a> {
    AgentAssets {
        topology: topo,
        pubkey: &dagon_sim::sample::PUBKEY,
        agent: img,
        maps,
        confirmed: true,
        manual_sync_ms: 0,
    }
}

fn with_watch<R>(s: &Sim, f: impl FnOnce(&mut SimNode) -> R) -> R {
    let mut hub = s.relay.borrow_mut();
    f(&mut hub.uart(WATCH_UART_PORT).unwrap().node)
}

/// What the watch's factory slot holds, `len` bytes from its start.
fn factory(s: &Sim, len: usize) -> Vec<u8> {
    let (base, _) = Layout::of("esp32s3").factory.unwrap();
    with_watch(s, |w| w.flash().peek(base, len as u32).unwrap().to_vec())
}

#[test]
fn the_recovery_agent_is_rewritten_through_the_rom_serial_loader() {
    let (s, mut h) = sample();
    let (topo, maps) = (topology(SAMPLE), [sim_map()]);
    let img = agent(V2, 70_000);
    assert_ne!(factory(&s, img.len()), img);
    let boots = with_watch(&s, |w| w.boots());

    let mut seen = Vec::new();
    let (stats, v) = block_on(
        h.rewrite_agent(WATCH, &assets(&img, &topo, &maps), &mut |e| {
            seen.push((e.phase, e.percent))
        }),
    )
    .unwrap();

    assert_eq!(v, V2);
    assert_eq!(factory(&s, img.len()), img);
    assert_eq!(stats.blocks as usize, img.len().div_ceil(1024));
    assert_eq!(count(&s, Cmd::EnterRom), 1);
    // The loader restarted the node, and its app answers again.
    with_watch(&s, |w| {
        assert!(!w.in_download());
        assert_eq!(w.boots(), boots + 1);
    });
    let to = target_of(&dagon_topology::parse_str(SAMPLE).unwrap(), "watch");
    let st = block_on(h.status(to)).unwrap();
    assert_eq!(st.hello.active_version, Version::new(1, 0, 0));
    assert_eq!(seen.first(), Some(&(phase::ROM, 0)));
    assert_eq!(seen.last(), Some(&(phase::CONFIRM, 100)));
    assert!(seen.windows(2).all(|p| p[0].1 <= p[1].1), "{seen:?}");
}

#[test]
fn nothing_is_touched_before_the_confirmation_or_with_a_wrong_image() {
    let (s, mut h) = sample();
    let (topo, maps) = (topology(SAMPLE), [sim_map()]);
    let img = agent(V2, 4000);
    let before = factory(&s, img.len());

    let unasked = AgentAssets {
        confirmed: false,
        ..assets(&img, &topo, &maps)
    };
    let r = block_on(h.rewrite_agent(WATCH, &unasked, &mut |_| {}));
    assert!(
        matches!(r, Err(Error::RomNotConfirmed { node: WATCH })),
        "{r:?}"
    );

    let mut bad = img.clone();
    *bad.last_mut().unwrap() ^= 1;
    let r = block_on(h.rewrite_agent(WATCH, &assets(&bad, &topo, &maps), &mut |_| {}));
    assert!(matches!(r, Err(Error::Format(_))), "{r:?}");
    // An app image is no recovery agent.
    let app = dagon_sim::sample::WATCH_1_0_0;
    let r = block_on(h.rewrite_agent(WATCH, &assets(app, &topo, &maps), &mut |_| {}));
    assert!(matches!(r, Err(Error::Format(_))), "{r:?}");
    // A signed agent of the same MCU for another board is not this node's.
    let other = agent_for("other-s3", V2, 4000);
    let r = block_on(h.rewrite_agent(WATCH, &assets(&other, &topo, &maps), &mut |_| {}));
    assert!(matches!(r, Err(Error::Format(_))), "{r:?}");
    // The seg has no ROM serial loader, the hub no relaying node before it.
    for node in [SEG, 0] {
        let r = block_on(h.rewrite_agent(node, &assets(&img, &topo, &maps), &mut |_| {}));
        assert!(matches!(r, Err(Error::Unreachable { .. })), "{node}: {r:?}");
    }

    assert_eq!(count(&s, Cmd::EnterRom), 0);
    assert_eq!(count(&s, Cmd::UartRaw), 0);
    assert_eq!(factory(&s, img.len()), before);
    assert!(!with_watch(&s, |w| w.in_download()));
}

/// An uplink that carries less than a UART link's buffer (the relaying
/// node behind I2C): every UART_RAW stays within it, and the same agent
/// is written.
#[test]
fn a_narrow_way_to_the_relaying_node_carries_smaller_pieces() {
    let mut s = sim_of(sample_hub(7));
    s.max = dagon_proto::max_payload::I2C;
    let mut h = Host::new(s.clone(), s.clone());
    let parsed = dagon_topology::parse_str(SAMPLE).unwrap();
    let reg = dagon_topology::defs::Registry::builtin();
    h.timings = dagon_host::node_timings(reg, &parsed).unwrap();
    h.local = dagon_host::local_node(reg, &parsed, "hub");
    let (topo, maps) = (topology(SAMPLE), [sim_map()]);
    let img = agent(V2, 9000);

    block_on(h.rewrite_agent(WATCH, &assets(&img, &topo, &maps), &mut |_| {})).unwrap();

    assert_eq!(factory(&s, img.len()), img);
    let most = *s.largest.borrow();
    assert!(most <= s.max, "{most} > {}", s.max);
}

/// A watch whose firmware does not answer cannot switch by itself: with
/// no time given for a person to bring it into the loader, nothing is
/// sent to its line; with it, SYNC goes on until the loader answers.
#[test]
fn a_silent_node_is_written_once_a_person_brought_it_into_the_loader() {
    let (s, mut h) = sample();
    let (topo, maps) = (topology(SAMPLE), [sim_map()]);
    let img = agent(V2, 9000);
    s.relay
        .borrow_mut()
        .uart(WATCH_UART_PORT)
        .unwrap()
        .link
        .silent = true;

    let r = block_on(h.rewrite_agent(WATCH, &assets(&img, &topo, &maps), &mut |_| {}));
    assert!(
        matches!(r, Err(Error::Unreachable { node: WATCH })),
        "{r:?}"
    );
    assert_eq!(count(&s, Cmd::UartRaw), 0);

    // Nobody presses the buttons: the loader never answers.
    let manual = AgentAssets {
        manual_sync_ms: 2_000,
        ..assets(&img, &topo, &maps)
    };
    let r = block_on(h.rewrite_agent(WATCH, &manual, &mut |_| {}));
    assert!(
        matches!(r, Err(Error::SerialRom(EspError::NoSync))),
        "{r:?}"
    );

    // The person did: the line carries again, and the loader holds it.
    {
        let mut hub = s.relay.borrow_mut();
        let stream = hub.uart(WATCH_UART_PORT).unwrap();
        stream.link.silent = false;
        stream.node.enter_download();
    }
    let manual = AgentAssets {
        manual_sync_ms: MANUAL_SYNC_MS,
        ..assets(&img, &topo, &maps)
    };
    let (_, v) = block_on(h.rewrite_agent(WATCH, &manual, &mut |_| {})).unwrap();
    assert_eq!(v, V2);
    assert_eq!(factory(&s, img.len()), img);
}

/// The built-in ESP32-S3 keeps the agent's header away from its body: the
/// body goes first, the header last.
#[test]
fn the_definitions_map_writes_the_header_last() {
    let maps = serial_rom::serial_rom_maps(dagon_topology::defs::Registry::builtin());
    let esp = image::mcu_id_of("esp32s3").unwrap();
    assert_eq!(maps.len(), 1, "{maps:?}");
    let map = maps[0];
    assert_eq!(
        (
            map.mcu_id,
            map.flash_bytes,
            map.agent.load,
            map.agent.header
        ),
        (esp, 0x100_0000, 0x1_0000, 0x71_2000)
    );
    let img = agent(V2, 5000);
    let topo = topology(SAMPLE);
    let view = dagon_format::topology::TopologyView::parse(&topo).unwrap();
    let watch = view.node(usize::from(WATCH));
    assert!(serial_rom::check_agent(&img, &dagon_sim::sample::PUBKEY, &map, &watch).is_ok());
    let regions = serial_rom::regions(&img, &map);
    let at: Vec<(u32, usize)> = regions.iter().map(|r| (r.addr, r.data.len())).collect();
    assert_eq!(at, [(0x1_0000, 5000), (0x71_2000, HEADER_SIZE)]);
    // The simulated layout's header sits right before its body: one region.
    let one = serial_rom::regions(&img, &sim_map());
    assert_eq!(
        (one.len(), one[0].addr, one[0].data.len()),
        (1, sim_map().agent.header, img.len())
    );

    // The link to the watch names the method; no link to the seg does.
    assert_eq!(serial_rom::enter_method(&view, WATCH), Some(1));
    assert_eq!(serial_rom::enter_method(&view, SEG), None);
}
