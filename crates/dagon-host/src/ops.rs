//! The procedures: discover, status, push_topology, apply, confirm,
//! rollback, recover.

use alloc::vec;
use alloc::vec::Vec;
use dagon_conform::timeout::{erase_ms, POLL_MS, REBOOT_WAIT_MS, REQUEST_MS, VERIFY_MS};
use dagon_format::bundle::BundleView;
use dagon_format::header::{image_kind, mcu_id, slot_binding, HEADER_SIZE};
use dagon_format::topology::{self, TopologyView};
use dagon_format::{sha256, ImageHeader, Version};
use dagon_proto::payload::{
    self, phase, reboot_mode, target, HelloResp, SetPending, Show, SlotState, StatusResp,
    XferBegin, XferChunk,
};
use dagon_proto::{Cmd, Status};

use crate::{Clock, Error, Host, Result, Target, Transport};

/// REBOOT until the hub's new HELLO, reopening the port meanwhile. Stays
/// well inside the 60 s the hub keeps its trial watchdog fed, so CONFIRM
/// always lands in time.
pub const HUB_RECONNECT_MS: u32 = 45_000;
/// Pause between reopen attempts while the hub is away.
pub const REOPEN_MS: u32 = 1_000;
/// RELAY timeout per address while discovering.
pub const DISCOVER_MS: u32 = 20;
const I2C_SCAN: core::ops::RangeInclusive<u8> = 0x08..=0x77;
/// STM32 ROM loader address; never probed with DAGON frames.
const ROM_ADDR: u8 = 0x39;
/// XFER_HDR / XFER_DATA bytes before the data: the offset.
const CHUNK_HEAD: usize = 4;
const PERCENT_STEP: u8 = 10;

/// HELLO and STATUS of one target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeState {
    pub hello: HelloResp,
    pub status: StatusResp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    pub hub: HelloResp,
    /// Nodes that answered RELAY HELLO, I2C by address, then UART.
    pub nodes: Vec<(Target, HelloResp)>,
}

/// Progress of apply / rollback, also mirrored to the hub LCD with SHOW.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// Topology node index.
    pub node: u8,
    /// SHOW phase (`payload::phase`).
    pub phase: u8,
    pub percent: u8,
    pub version: Version,
}

#[derive(Debug)]
pub enum Outcome<E> {
    /// Running and confirmed on this version.
    Done(Version),
    /// Stopped here; later nodes are `NotStarted`.
    Failed(Error<E>),
    NotStarted,
}

#[derive(Debug)]
pub struct NodeOutcome<E> {
    pub node: u8,
    pub target: Target,
    pub outcome: Outcome<E>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmOutcome {
    /// CONFIRM sent and accepted.
    Confirmed(Version),
    /// Not on trial; nothing sent.
    NotOnTrial(Version),
    /// On trial with a version this session did not apply; nothing sent.
    /// The user picks: confirm with `force`, or roll back.
    NeedsChoice(Version),
}

impl<T: Transport, C: Clock> Host<T, C> {
    pub async fn status(&mut self, to: Target) -> Result<NodeState, T::Error> {
        let hello = self.hello(to).await?;
        let body = self.call(to, Cmd::Status, &[], REQUEST_MS).await?;
        let status = StatusResp::parse(&body).map_err(|_| Error::BadResponse)?;
        Ok(NodeState { hello, status })
    }

    /// Hub HELLO, then a short RELAY HELLO to every I2C address (ROM loader
    /// excluded) and one on the UART.
    pub async fn discover(&mut self) -> Result<Discovery, T::Error> {
        let hub = self.hello(Target::Hub).await?;
        let mut nodes = Vec::new();
        let i2c = I2C_SCAN.filter(|&a| a != ROM_ADDR).map(Target::I2c);
        for (to, wait) in i2c
            .map(|t| (t, DISCOVER_MS))
            .chain([(Target::Uart, REQUEST_MS)])
        {
            match self.hello_within(to, wait).await {
                Ok(h) => nodes.push((to, h)),
                Err(Error::Transport(e)) => return Err(Error::Transport(e)),
                Err(_) => {}
            }
        }
        Ok(Discovery { hub, nodes })
    }

    /// Sends the topology canonical binary to the hub's display copy.
    pub async fn push_topology(&mut self, bin: &[u8]) -> Result<(), T::Error> {
        TopologyView::parse(bin)?;
        let hub = self.hello(Target::Hub).await?;
        let mut quiet = |_: Event| {};
        let xfer = Xfer {
            tgt: target::TOPOLOGY,
            slot: 0,
            hdr: &[],
            body: bin,
            show: None,
        };
        self.transfer(Target::Hub, &hub, xfer, &mut quiet).await
    }

    /// Checks `bundle` (signature chain under `pubkey`, and that it was built
    /// for `topology`, the caller's current canonical binary), pushes the
    /// topology, then updates the nodes: children in update order, the hub
    /// last. `nodes` limits the run to these topology node indices (the hub
    /// only when listed). Stops at the first node that fails.
    pub async fn apply(
        &mut self,
        bundle: &[u8],
        topology: &[u8],
        pubkey: &[u8; 32],
        nodes: Option<&[u8]>,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Vec<NodeOutcome<T::Error>>, T::Error> {
        let (b, topo) = check_release(bundle, topology, pubkey)?;
        self.push_topology(topology).await?;

        let mut plan = plan(&b, &topo, nodes);
        let mut out = Vec::new();
        let mut failed = false;
        for node in plan.drain(..) {
            let target = Target::of_node(&topo, node);
            let outcome = match target {
                _ if failed => Outcome::NotStarted,
                None => Outcome::Failed(Error::Unreachable { node }),
                Some(to) => match self.update_node(&b, node, to, on_event).await {
                    Ok(v) => Outcome::Done(v),
                    Err(e) => Outcome::Failed(e),
                },
            };
            failed |= matches!(outcome, Outcome::Failed(_));
            out.push(NodeOutcome {
                node,
                target: target.unwrap_or(Target::Hub),
                outcome,
            });
        }
        Ok(out)
    }

    async fn update_node(
        &mut self,
        b: &BundleView<'_>,
        node: u8,
        to: Target,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Version, T::Error> {
        let st = self.status(to).await?;
        let r = self.update_inner(b, node, to, &st, on_event).await;
        if let Err(e) = &r {
            let ph = match e {
                Error::Rejected {
                    status: Status::Signature | Status::Key | Status::Compat | Status::Digest,
                    ..
                } => phase::REJECTED,
                _ => phase::FAILED,
            };
            let v = st.hello.active_version;
            on_event(Event {
                node,
                phase: ph,
                percent: 0,
                version: v,
            });
            self.show(node, ph, 0, v, reason(e).as_bytes()).await;
        }
        r
    }

    async fn update_inner(
        &mut self,
        b: &BundleView<'_>,
        node: u8,
        to: Target,
        st: &NodeState,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Version, T::Error> {
        let slot = free_slot(&st.status).ok_or(Error::NoImage { node })?;
        let img = image_for(b, node, slot).ok_or(Error::NoImage { node })?;
        let h = ImageHeader::parse(img[..HEADER_SIZE].try_into().expect("verified"))?;
        let v = h.version;
        let (hdr, body) = img.split_at(HEADER_SIZE);
        self.emit(on_event, node, phase::TRANSFER, 0, v).await;
        let xfer = Xfer {
            tgt: target::APP,
            slot,
            hdr,
            body,
            show: Some((node, v)),
        };
        self.transfer(to, &st.hello, xfer, on_event).await?;
        self.emit(on_event, node, phase::VERIFY, 100, v).await;
        self.switch_and_confirm(to, slot, v, node, on_event).await?;
        Ok(v)
    }

    /// SET_PENDING `slot`, REBOOT, wait for `v` on trial, CONFIRM.
    async fn switch_and_confirm(
        &mut self,
        to: Target,
        slot: u8,
        v: Version,
        node: u8,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<(), T::Error> {
        let sp = SetPending {
            target: target::APP,
            slot,
        };
        let mut p = [0u8; 2];
        sp.encode(&mut p).expect("2 bytes");
        self.call(to, Cmd::SetPending, &p, VERIFY_MS).await?;
        self.emit(on_event, node, phase::REBOOT, 0, v).await;
        self.call(to, Cmd::Reboot, &[reboot_mode::NORMAL], REQUEST_MS)
            .await?;
        self.set_applied(to, v);
        let h = self.await_version(to, v).await?;
        if h.trial {
            self.call(to, Cmd::Confirm, &[target::APP], VERIFY_MS)
                .await?;
        }
        self.emit(on_event, node, phase::CONFIRM, 100, v).await;
        Ok(())
    }

    /// Polls HELLO after REBOOT until `v` answers (hub: reopening the port).
    async fn await_version(&mut self, to: Target, v: Version) -> Result<HelloResp, T::Error> {
        let (limit, pause) = match to {
            Target::Hub => (HUB_RECONNECT_MS, REOPEN_MS),
            _ => (REBOOT_WAIT_MS, POLL_MS),
        };
        let deadline = self.clock.now_ms() + limit as u64;
        // Another version may answer until the reset really happens (the
        // old firmware still running) or for good (rolled back); only the
        // end of the window tells them apart.
        let mut other = None;
        loop {
            // ponytail: reopens before every hub attempt; add a grace period
            // for slow enumeration if real ports need it.
            let reopened = to != Target::Hub || self.transport.reopen().await.is_ok();
            if reopened {
                match self.hello(to).await {
                    Ok(h) if h.active_version == v => return Ok(h),
                    Ok(h) => other = Some(h),
                    Err(Error::Transport(_) | Error::Timeout | Error::Relay(_)) => {}
                    Err(e) => return Err(e),
                }
            }
            if self.clock.now_ms() >= deadline {
                return Err(match other {
                    Some(h) => Error::WrongVersion {
                        expected: v,
                        running: h.active_version,
                        trial: h.trial,
                    },
                    None => Error::Timeout,
                });
            }
            self.clock.sleep_ms(pause).await;
        }
    }

    /// The confirm rule after a manual reconnect: CONFIRM only when `to` is
    /// on trial with the version this session applied, or when `force`
    /// (the user chose to keep the running version).
    pub async fn confirm(&mut self, to: Target, force: bool) -> Result<ConfirmOutcome, T::Error> {
        let h = self.hello(to).await?;
        let v = h.active_version;
        if !h.trial {
            return Ok(ConfirmOutcome::NotOnTrial(v));
        }
        if !force && self.applied(to) != Some(v) {
            return Ok(ConfirmOutcome::NeedsChoice(v));
        }
        self.call(to, Cmd::Confirm, &[target::APP], VERIFY_MS)
            .await?;
        Ok(ConfirmOutcome::Confirmed(v))
    }

    /// Back to the version in the other slot: SET_PENDING, REBOOT, its
    /// HELLO, CONFIRM. `node` is the topology index used for events.
    pub async fn rollback(
        &mut self,
        to: Target,
        node: u8,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Version, T::Error> {
        let st = self.status(to).await?.status;
        let other = rollback_slot(&st).ok_or(Error::NoPreviousVersion)?;
        let v = st.slots[other as usize].version;
        self.switch_and_confirm(to, other, v, node, on_event)
            .await?;
        Ok(v)
    }

    /// Rewrites topology node `node` from the release. A node that answers
    /// HELLO (its app, or the factory agent: HELLO mode = agent) gets its
    /// image through the normal transfer. An STM32F401 that does not answer
    /// and whose link has the I2C ROM entry is rewritten through the ROM
    /// loader: boot core, APP-A and a fresh state log, then its HELLO.
    pub async fn recover(
        &mut self,
        node: u8,
        a: &RecoverAssets<'_>,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<(RecoverPath, Version), T::Error> {
        let (b, topo) = check_release(a.release, a.topology, a.pubkey)?;
        let to = Target::of_node(&topo, node).ok_or(Error::Unreachable { node })?;
        match self.hello(to).await {
            Ok(h) => {
                let path = if h.mode == payload::mode::AGENT {
                    RecoverPath::Factory
                } else {
                    RecoverPath::Retransfer
                };
                let v = self.update_node(&b, node, to, on_event).await?;
                return Ok((path, v));
            }
            Err(Error::Transport(e)) => return Err(Error::Transport(e)),
            Err(_) => {}
        }
        if !rom_reachable(&topo, node) {
            return Err(Error::Unreachable { node });
        }
        let boot = a.bootcore.ok_or(Error::NoImage { node })?;
        crate::rom::check_bootcore(boot, a.pubkey)?;
        let app = image_for(&b, node, slot_binding::A).ok_or(Error::NoImage { node })?;
        crate::rom::check_app_a(app)?;
        let app_hdr: &[u8; HEADER_SIZE] = app[..HEADER_SIZE].try_into().expect("verified");
        let v = ImageHeader::parse(app_hdr)?.version;
        let log = crate::rom::initial_log(app_hdr)?;
        let regions = crate::rom::f401_regions(boot, app, &log);
        let plan = dagon_romlink::Plan {
            erase: &crate::rom::F401_ERASE,
            regions: &regions,
        };
        self.emit(on_event, node, phase::ROM, 0, v).await;
        let stats = {
            let mut link = crate::rom::HubRom::new(self, on_event, node, v);
            dagon_romlink::flash(&mut link, &plan).await
        };
        let stats = match stats {
            Ok(s) => s,
            Err(e) => {
                self.show(node, phase::FAILED, 0, v, b"ROM FAILED").await;
                return Err(Error::Rom(e));
            }
        };
        self.set_applied(to, v);
        self.await_version(to, v).await?;
        self.emit(on_event, node, phase::CONFIRM, 100, v).await;
        Ok((RecoverPath::Rom(stats), v))
    }

    /// XFER_BEGIN, XFER_HDR chunks, XFER_DATA chunks, XFER_END with digest
    /// check. Body progress is reported in [`PERCENT_STEP`] steps when
    /// `x.show` names the node.
    async fn transfer(
        &mut self,
        to: Target,
        hello: &HelloResp,
        x: Xfer<'_>,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<(), T::Error> {
        let Xfer {
            tgt,
            slot,
            hdr,
            body,
            show,
        } = x;
        let erase = erase_ms(hello.mcu_id);
        let begin = XferBegin {
            target: tgt,
            slot,
            total_len: (hdr.len() + body.len()) as u32,
            hdr_len: hdr.len() as u16,
        };
        let mut p = [0u8; 8];
        let n = begin.encode(&mut p).expect("8 bytes");
        let begin_wait = if hdr.is_empty() { erase } else { REQUEST_MS };
        self.call(to, Cmd::XferBegin, &p[..n], begin_wait).await?;

        let chunk = chunk_len(hello.max_payload, to.link_max()).ok_or(Error::BadResponse)?;
        let hdr_chunks = hdr.len().div_ceil(chunk);
        for (i, c) in hdr.chunks(chunk).enumerate() {
            let wait = if i + 1 == hdr_chunks {
                erase
            } else {
                REQUEST_MS
            };
            self.chunk(to, Cmd::XferHdr, i * chunk, c, wait).await?;
        }
        let mut shown = 0u8;
        for (i, c) in body.chunks(chunk).enumerate() {
            self.chunk(to, Cmd::XferData, i * chunk, c, REQUEST_MS)
                .await?;
            let pct = ((i * chunk + c.len()) * 100 / body.len()) as u8;
            if pct >= shown + PERCENT_STEP {
                shown = pct - pct % PERCENT_STEP;
                if let Some((node, v)) = show {
                    self.emit(on_event, node, phase::TRANSFER, shown, v).await;
                }
            }
        }
        let d = self.call(to, Cmd::XferEnd, &[], VERIFY_MS).await?;
        let digest = payload::parse_digest(&d).map_err(|_| Error::BadResponse)?;
        if digest != sha256(body) {
            return Err(Error::DigestMismatch);
        }
        Ok(())
    }

    async fn chunk(
        &mut self,
        to: Target,
        cmd: Cmd,
        offset: usize,
        data: &[u8],
        wait: u32,
    ) -> Result<(), T::Error> {
        let mut p = vec![0u8; CHUNK_HEAD + data.len()];
        XferChunk {
            offset: offset as u32,
            data,
        }
        .encode(&mut p)
        .expect("sized for the chunk");
        let r = self.call(to, cmd, &p, wait).await?;
        let next = payload::parse_next_offset(&r).map_err(|_| Error::BadResponse)?;
        if next as usize != offset + data.len() {
            return Err(Error::BadResponse);
        }
        Ok(())
    }

    /// Reports `ev` and shows it on the hub LCD (best effort: the hub may be
    /// rebooting).
    pub(crate) async fn emit(
        &mut self,
        on_event: &mut dyn FnMut(Event),
        node: u8,
        ph: u8,
        percent: u8,
        version: Version,
    ) {
        on_event(Event {
            node,
            phase: ph,
            percent,
            version,
        });
        self.show(node, ph, percent, version, &[]).await;
    }

    /// SHOW on the hub LCD, best effort.
    async fn show(&mut self, node: u8, ph: u8, percent: u8, version: Version, text: &[u8]) {
        let show = Show {
            node,
            phase: ph,
            percent,
            version,
            text,
        };
        let mut p = [0u8; 7 + payload::SHOW_TEXT_MAX];
        let Ok(n) = show.encode(&mut p) else {
            return;
        };
        let _ = self.call(Target::Hub, Cmd::Show, &p[..n], REQUEST_MS).await;
    }
}

/// XFER data bytes per chunk for a peer announcing `max_payload` on a link
/// carrying `link_max`; `None` when not even one byte fits.
fn chunk_len(max_payload: u16, link_max: usize) -> Option<usize> {
    (max_payload as usize)
        .min(link_max)
        .checked_sub(CHUNK_HEAD)
        .filter(|&c| c > 0)
}

/// The slot rollback returns to: the other slot, Confirmed (we run on
/// trial) or Written (confirmed before the running slot took over); never a
/// Reverted, pending or empty one.
fn rollback_slot(st: &StatusResp) -> Option<u8> {
    let other = match st.active {
        0 => 1u8,
        1 => 0,
        _ => return None,
    };
    let ok = matches!(
        st.slots[other as usize].state,
        SlotState::Confirmed | SlotState::Written
    );
    ok.then_some(other)
}

/// Release `bundle` verified under `pubkey` and built for `topology`.
fn check_release<'a, E>(
    bundle: &'a [u8],
    topology: &'a [u8],
    pubkey: &[u8; 32],
) -> Result<(BundleView<'a>, TopologyView<'a>), E> {
    let b = BundleView::parse(bundle)?;
    b.verify(pubkey)?;
    let topo = TopologyView::parse(topology)?;
    let release = b.manifest().fields().topology_sha256;
    let local = sha256(topology);
    if release != local {
        return Err(Error::TopologyMismatch { release, local });
    }
    Ok((b, topo))
}

/// An STM32F401 on I2C whose link enters the ROM loader through NRST/BOOT0.
fn rom_reachable(topo: &TopologyView<'_>, node: u8) -> bool {
    let n = (node as usize) < topo.node_count();
    let f401 = n && topo.node(node as usize).mcu_id == mcu_id::STM32F401XE;
    let rom = (0..topo.link_count()).map(|i| topo.link(i)).any(|l| {
        l.to == node
            && l.iface == topology::iface::I2C_BUS
            && l.rom_entry == topology::rom_entry::STM32_I2C_ROM
    });
    f401 && rom
}

/// What [`Host::recover`] needs: the release, the current topology binary,
/// the verifying key and, for the ROM path, the signed boot core.
#[derive(Clone, Copy, Debug)]
pub struct RecoverAssets<'a> {
    pub release: &'a [u8],
    pub topology: &'a [u8],
    pub pubkey: &'a [u8; 32],
    pub bootcore: Option<&'a [u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoverPath {
    /// The app answered; the image went through the normal transfer.
    Retransfer,
    /// The factory agent answered and received the image.
    Factory,
    /// Rewritten through the ROM loader.
    Rom(dagon_romlink::Stats),
}

/// Short LCD text for a failed update (ASCII, at most 32 bytes).
pub fn reason<E>(e: &Error<E>) -> &'static str {
    match e {
        Error::Rejected { status, .. } => match status {
            Status::Signature => "E_SIGNATURE",
            Status::Key => "E_KEY",
            Status::Compat => "E_COMPAT",
            Status::Digest => "E_DIGEST",
            Status::Flash => "E_FLASH",
            Status::State => "E_STATE",
            Status::Range => "E_RANGE",
            _ => "REJECTED",
        },
        Error::Timeout | Error::Relay(_) | Error::Transport(_) => "NO ANSWER",
        Error::WrongVersion { .. } => "ROLLED BACK",
        Error::DigestMismatch => "E_DIGEST",
        Error::NoImage { .. } => "NO IMAGE",
        _ => "FAILED",
    }
}

struct Xfer<'a> {
    tgt: u8,
    slot: u8,
    hdr: &'a [u8],
    body: &'a [u8],
    /// Node index and version for progress events.
    show: Option<(u8, Version)>,
}

/// Nodes with an APP image in the release, filtered by `only`: children by
/// update order, then the coordinator.
fn plan(b: &BundleView<'_>, topo: &TopologyView<'_>, only: Option<&[u8]>) -> Vec<u8> {
    let m = b.manifest();
    let mut nodes: Vec<u8> = (0..m.entry_count())
        .map(|i| m.entry(i))
        .filter(|e| e.image_kind == image_kind::APP)
        .map(|e| e.node_index)
        .filter(|&n| (n as usize) < topo.node_count())
        .filter(|n| only.is_none_or(|o| o.contains(n)))
        .collect();
    nodes.sort_by_key(|&n| {
        let t = topo.node(n as usize);
        (t.role == topology::role::COORDINATOR, t.order_rank, n)
    });
    nodes.dedup();
    nodes
}

/// Slot neither running nor confirmed.
fn free_slot(st: &StatusResp) -> Option<u8> {
    [0u8, 1]
        .into_iter()
        .find(|&s| s != st.active && s != st.confirmed)
}

/// APP image of `node` bound to `slot` (or to any slot).
fn image_for<'a>(b: &BundleView<'a>, node: u8, slot: u8) -> Option<&'a [u8]> {
    let m = b.manifest();
    (0..b.item_count()).find_map(|i| {
        let it = b.item(i);
        let e = (it.entry_index as usize) < m.entry_count();
        let e = e.then(|| m.entry(it.entry_index as usize))?;
        let fits = e.node_index == node
            && e.image_kind == image_kind::APP
            && (e.slot_binding == slot || e.slot_binding == slot_binding::ANY);
        fits.then(|| b.item_bytes(i))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_len_needs_room_for_one_byte() {
        assert_eq!(chunk_len(0, 256), None);
        assert_eq!(chunk_len(4, 256), None);
        assert_eq!(chunk_len(5, 256), Some(1));
        assert_eq!(chunk_len(1088, 256), Some(252), "link limit wins");
    }

    #[test]
    fn rollback_only_to_a_previously_confirmed_slot() {
        let with = |active, other: SlotState| {
            let mut st = StatusResp {
                active,
                confirmed: active,
                ..Default::default()
            };
            st.slots[(1 - active.min(1)) as usize].state = other;
            rollback_slot(&st)
        };
        assert_eq!(with(1, SlotState::Written), Some(0));
        assert_eq!(with(0, SlotState::Confirmed), Some(1));
        for bad in [
            SlotState::Reverted,
            SlotState::Empty,
            SlotState::Pending,
            SlotState::Trying,
        ] {
            assert_eq!(with(1, bad), None, "{bad:?}");
        }
        assert_eq!(with(0xFF, SlotState::Written), None, "nothing running");
    }
}
