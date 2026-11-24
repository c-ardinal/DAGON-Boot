//! The boot core's side of update mode in the simulator: on each link kind
//! of each MCU the boot core listens for the window its definition gives,
//! and the RP2040's boot core writes the app through the same session the
//! app's agent runs.

use dagon_agent::writer::BootCore;
use dagon_agent::Agent;
use dagon_ed25519::Ed25519Compact;
use dagon_events::{compose, Bounds, Composed, Layer, Nop, Published, Watchdog};
use dagon_proto::payload::{mode, reboot_mode, target, SetPending, XferBegin, XferChunk};
use dagon_proto::{split_status, Cmd, Frame, Status};
use dagon_sim::{hub_config, listen, sample, Layout, Run, SimFlash, SimNode};
use dagon_topology::defs::Registry;

/// The listen window on every link kind of the three MCUs: the first STAY
/// the hub sends after it releases the node's reset (design: 2 ms after,
/// then every 5 ms) arrives within the window on a kind that takes part,
/// and keeps the boot core in update mode; on a kind that does not (USB,
/// which enumerates first) the boot core opens no window, and a STAY
/// after the window is not taken. A project's window of 0 opens none.
#[test]
fn the_listen_window_on_each_link_kind_of_the_three_mcus() {
    let mut seen = Vec::new();
    for m in Registry::builtin().mcus() {
        let r = m.boot_receive.as_ref().expect("the boot stage receives");
        for (&kind, &takes_part) in &r.kinds {
            let at = listen::first_stay_ms(kind).expect("the simulator has the kind");
            let window = m.listen_window_ms(kind);
            assert_eq!(listen::stays(m, kind, at), takes_part, "{} {kind:?}", m.id);
            if takes_part {
                assert_eq!(window, r.listen_window_ms, "{} {kind:?}", m.id);
                let late = f64::from(window) + 1.0;
                assert!(!listen::stays(m, kind, late), "{} {kind:?}: late", m.id);
            } else {
                assert_eq!(window, 0, "{} {kind:?}", m.id);
            }
            let mut none = m.clone();
            none.boot_receive.as_mut().unwrap().listen_window_ms = 0;
            assert!(!listen::stays(&none, kind, 0.0), "{} {kind:?}: 0 ms", m.id);
            seen.push(format!("{}/{}", m.id, kind.name()));
        }
    }
    seen.sort();
    assert_eq!(
        seen,
        [
            "esp32s3/uart",
            "esp32s3/usb-cdc",
            "rp2040/i2c",
            "rp2040/uart",
            "rp2040/usb-cdc",
            "stm32f401re/i2c"
        ]
    );
}

type Chain = Composed<&'static Published<(), 4>, Nop, Nop, fn() -> u64>;

fn chain() -> Chain {
    let events: &'static mut Published<(), 4> = Box::leak(Box::new(Published::new()));
    let now: fn() -> u64 = || 0;
    compose(
        Layer::App,
        events,
        Nop,
        Nop,
        Bounds::new(now, Watchdog::new(4000, 8000)),
    )
}

fn call(
    a: &mut Agent<dagon_sim::SimStore, Chain, Ed25519Compact, BootCore>,
    seq: u8,
    cmd: Cmd,
    p: &[u8],
) -> Status {
    let mut buf = [0u8; 1200];
    let n = Frame {
        flags: 0,
        seq,
        cmd: cmd as u8,
        payload: p,
    }
    .encode(&mut buf)
    .unwrap();
    let reply = a.handle(&buf[..n]).expect("reply");
    let (f, _) = Frame::decode(reply.frame, 1200).unwrap();
    split_status(f.payload).unwrap().0
}

/// The RP2040's boot core, in update mode, writes the app into its
/// confirmed slot through the shared session and sets it pending; the
/// node's next start tries it.
#[test]
fn the_rp2040_boot_core_writes_the_app() {
    let mut node = SimNode::new(
        hub_config(),
        sample::PUBKEY,
        SimFlash::rp2040_4m(7),
        Layout::of("rp2040"),
    );
    node.store().provision_core(sample::HUB_CORE_1_0_0).unwrap();
    node.provision(sample::HUB_1_0_0, 0).unwrap();
    assert_eq!(node.run(), Run::App(0));
    let cfg = dagon_agent::Config {
        mode: mode::UPDATE,
        ..hub_config()
    };
    let store = node.store().clone();
    let mut core: Agent<_, _, _, BootCore> =
        Agent::for_writer(cfg, Ed25519Compact::new(sample::PUBKEY), store, chain());
    let img = sample::HUB_1_0_0;
    let mut p = [0u8; 1100];
    let n = XferBegin {
        target: target::APP,
        slot: 0,
        total_len: img.len() as u32,
        hdr_len: 512,
    }
    .encode(&mut p)
    .unwrap();
    let mut seq = 0u8;
    let mut ok = |core: &mut Agent<_, _, _, BootCore>, cmd, p: &[u8]| {
        seq += 1;
        assert_eq!(call(core, seq, cmd, p), Status::Ok, "{cmd:?}");
    };
    ok(&mut core, Cmd::XferBegin, &p[..n]);
    let (hdr, body) = img.split_at(512);
    for (cmd, part) in [(Cmd::XferHdr, hdr), (Cmd::XferData, body)] {
        for (i, c) in part.chunks(1000).enumerate() {
            let n = XferChunk {
                offset: (i * 1000) as u32,
                data: c,
            }
            .encode(&mut p)
            .unwrap();
            ok(&mut core, cmd, &p[..n]);
        }
    }
    ok(&mut core, Cmd::XferEnd, &[]);
    let n = SetPending {
        target: target::APP,
        slot: 0,
    }
    .encode(&mut p)
    .unwrap();
    ok(&mut core, Cmd::SetPending, &p[..n]);
    node.reset(1);
    assert_eq!(node.run(), Run::App(0));
    let st = node.status();
    assert_eq!(st.slots[0].state, dagon_proto::payload::SlotState::Trying);
}

/// One request frame of `cmd` with payload `p`; the reply's status and
/// payload after it.
fn ask(node: &mut SimNode, seq: u8, cmd: Cmd, p: &[u8], now: u64) -> (Status, Vec<u8>) {
    let mut buf = [0u8; 700];
    let n = Frame {
        flags: 0,
        seq,
        cmd: cmd as u8,
        payload: p,
    }
    .encode(&mut buf)
    .unwrap();
    let reply = node.handle_frame(&buf[..n], now).expect("an answer");
    let (f, _) = Frame::decode(&reply, 1200).unwrap();
    let (s, rest) = split_status(f.payload).unwrap();
    (s, rest.to_vec())
}

fn hello_mode(node: &mut SimNode, now: u64) -> u8 {
    let req = dagon_proto::payload::HelloReq {
        proto_ver: dagon_proto::PROTO_VERSION as u32,
        max_payload: 1024,
    };
    let mut p = [0u8; 16];
    let n = req.encode(&mut p).unwrap();
    let (s, body) = ask(node, 9, Cmd::Hello, &p[..n], now);
    assert_eq!(s, Status::Ok);
    dagon_proto::payload::HelloResp::parse(&body).unwrap().mode
}

fn hub() -> SimNode {
    let mut node = SimNode::new(
        hub_config(),
        sample::PUBKEY,
        SimFlash::rp2040_4m(7),
        Layout::of("rp2040"),
    );
    node.store().provision_core(sample::HUB_CORE_1_0_0).unwrap();
    node.provision(sample::HUB_1_0_0, 0).unwrap();
    node
}

/// REBOOT mode 1: the app records the hand-over and resets; the boot core
/// consumes it and answers in update mode, then leaves after
/// `EXIT_IDLE_MS` without a request (a normal start: the app again, no
/// stay left to take).
#[test]
fn the_app_hands_over_and_the_boot_core_leaves_update_mode_when_idle() {
    use dagon_agent::update_mode::EXIT_IDLE_MS;
    let mut node = hub();
    let mut p = [0u8; 8];
    let n = dagon_proto::payload::encode_reboot(reboot_mode::UPDATE, &mut p).unwrap();
    assert_eq!(ask(&mut node, 1, Cmd::Reboot, &p[..n], 0).0, Status::Ok);
    assert_eq!(node.run(), Run::BootCore(mode::UPDATE));
    assert_eq!(hello_mode(&mut node, 10), mode::UPDATE);
    let st = node.status();
    assert_eq!(st.active, dagon_format::statelog::slot::NONE, "no app runs");
    // A REBOOT mode 1 in update mode stays there.
    assert_eq!(ask(&mut node, 2, Cmd::Reboot, &p[..n], 20).0, Status::Ok);
    assert_eq!(node.run(), Run::BootCore(mode::UPDATE));
    let idle = u64::from(EXIT_IDLE_MS);
    node.tick(20 + idle - 1);
    assert_eq!(node.run(), Run::BootCore(mode::UPDATE));
    node.tick(20 + idle);
    assert_eq!(node.run(), Run::App(0));
    node.reset(20 + idle + 1);
    assert_eq!(node.run(), Run::App(0), "the stay was taken once");
}

/// No app to start (both app slots erased): the boot core serves in
/// recovery mode, which no idle time ends.
#[test]
fn nothing_to_start_keeps_the_boot_core_in_recovery_mode() {
    use dagon_port::Flash;
    let mut node = hub();
    for &(at, len) in Layout::of("rp2040").slots {
        node.flash().erase(at, len).unwrap();
    }
    node.reset(0);
    assert_eq!(node.run(), Run::BootCore(mode::RECOVERY));
    assert_eq!(hello_mode(&mut node, 1), mode::RECOVERY);
    node.tick(10 * 60_000);
    assert_eq!(node.run(), Run::BootCore(mode::RECOVERY));
}

/// Stage0 present but no boot core it can start (both core slots and
/// both app slots erased): nothing serves, recovery mode included.
#[test]
fn no_boot_core_to_start_serves_no_recovery_mode() {
    use dagon_port::Flash;
    let mut node = hub();
    let layout = Layout::of("rp2040");
    for &(at, len) in layout.slots.iter().chain(layout.cores.unwrap().slots) {
        node.flash().erase(at, len).unwrap();
    }
    node.reset(0);
    assert!(!matches!(node.run(), Run::BootCore(_)), "{:?}", node.run());
}

/// A transfer the boot core heard nothing of for `IDLE_TIMEOUT_MS` is
/// dropped, as `update_mode::serve_in` drops it: its next chunk is
/// refused.
#[test]
fn the_boot_core_drops_a_transfer_left_idle() {
    use dagon_port::Flash;
    let mut node = hub();
    for &(at, len) in Layout::of("rp2040").slots {
        node.flash().erase(at, len).unwrap();
    }
    node.reset(0);
    assert_eq!(node.run(), Run::BootCore(mode::RECOVERY));
    let img = sample::HUB_1_0_0;
    let mut p = [0u8; 600];
    let n = XferBegin {
        target: target::APP,
        slot: 0,
        total_len: img.len() as u32,
        hdr_len: 512,
    }
    .encode(&mut p)
    .unwrap();
    assert_eq!(ask(&mut node, 1, Cmd::XferBegin, &p[..n], 1).0, Status::Ok);
    let chunk = |offset: usize, p: &mut [u8]| {
        XferChunk {
            offset: offset as u32,
            data: &img[offset..offset + 32],
        }
        .encode(p)
        .unwrap()
    };
    let n = chunk(0, &mut p);
    assert_eq!(ask(&mut node, 2, Cmd::XferHdr, &p[..n], 2).0, Status::Ok);
    let idle = u64::from(dagon_agent::IDLE_TIMEOUT_MS);
    node.tick(2 + idle);
    let n = chunk(32, &mut p);
    assert_ne!(
        ask(&mut node, 3, Cmd::XferHdr, &p[..n], 3 + idle).0,
        Status::Ok
    );
}
