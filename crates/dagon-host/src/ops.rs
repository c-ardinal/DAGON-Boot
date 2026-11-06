//! The procedures: discover, status, push_topology, apply, confirm,
//! rollback, recover.

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use dagon_conform::timeout::{erase_ms, REBOOT_WAIT_MS, REQUEST_MS, VERIFY_MS};
use dagon_format::bundle::BundleView;
use dagon_format::header::{image_kind, slot_binding, HEADER_SIZE};
use dagon_format::topology::{self, TopologyView};
use dagon_format::{sha256, ImageHeader, Version};
use dagon_proto::payload::{
    self, phase, reboot_mode, target, HelloResp, SetPending, Show, SlotState, StatusResp,
    VersionResp, XferBegin, XferChunk,
};
use dagon_proto::{Cmd, Status};

use crate::rom::RomMap;
use crate::{Clock, Error, Host, Result, Target, Transport};

/// REBOOT until the hub's new HELLO, reopening the port meanwhile. Stays
/// well inside the 60 s the hub keeps its trial watchdog fed, so CONFIRM
/// always lands in time.
pub const HUB_RECONNECT_MS: u32 = 45_000;
/// Nodes (F401, watch) do not feed their watchdog on trial: an image gets
/// about [`NODE_TRIAL_MS`] per boot attempt before it is reset. The new
/// version's HELLO is polled this often so CONFIRM lands well inside one
/// attempt (one poll costs at most two [`REQUEST_MS`] round trips).
pub const NODE_POLL_MS: u32 = 300;
/// Trial watchdog of a node's boot attempt: the shortest of the builtin MCU
/// definitions' `[watchdog] trial_ms` (CONFIRM must land inside it).
pub const NODE_TRIAL_MS: u32 = 4_000;
/// The longest of them (a node's full local rollback takes up to its
/// attempts of it). A test keeps both equal to the definitions.
pub const NODE_TRIAL_MAX_MS: u32 = 8_000;
/// Boot attempts before a node rolls back (F401; the watch tries once).
pub const NODE_MAX_ATTEMPTS: u32 = 3;
const _: () = {
    // Poll pause plus a missed poll (RELAY wait + node wait) fits a trial.
    assert!(NODE_POLL_MS + 2 * REQUEST_MS < NODE_TRIAL_MS);
    // Waiting for the new HELLO covers a node's full local rollback
    // (its attempts of the longest trial period plus start-up), so silence there is not
    // taken for a lost node.
    assert!(REBOOT_WAIT_MS >= NODE_MAX_ATTEMPTS * (NODE_TRIAL_MAX_MS + 1_000) + 2_000);
};
/// Pauses before CONFIRM is sent again after E_STATE: the image runs but its
/// self-check does not pass yet (its tasks are still starting, e.g. the
/// watch sample's first heartbeat behind the display start-up). Together
/// (2.1 s) they stay inside one boot attempt ([`NODE_TRIAL_MS`]), part of
/// which the reboot and the HELLO poll have already used; an image whose
/// self-check never passes is still reset by its trial watchdog and rolls
/// back. A target that confirms at once never waits.
pub const CONFIRM_RETRY_MS: [u32; 5] = [100, 200, 400, 600, 800];
const _: () = {
    let mut total = 0;
    let mut i = 0;
    while i < CONFIRM_RETRY_MS.len() {
        total += CONFIRM_RETRY_MS[i];
        i += 1;
    }
    assert!(total + NODE_POLL_MS + 2 * REQUEST_MS < NODE_TRIAL_MS);
};
/// Pause between reopen attempts while the hub is away.
pub const REOPEN_MS: u32 = 1_000;
/// RELAY timeout per address while discovering.
pub const DISCOVER_MS: u32 = 20;
const I2C_SCAN: core::ops::RangeInclusive<u8> = 0x08..=0x77;
/// XFER_HDR / XFER_DATA bytes before the data: the offset.
const CHUNK_HEAD: usize = 4;
const PERCENT_STEP: u8 = 10;
/// The recovery agent's slot (STATUS slot 2).
const FACTORY_SLOT: u8 = 2;

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
    /// The phase's own progress: the bytes of this image during TRANSFER.
    pub percent: u8,
    /// The node's one progress number (`percent.rs`), what its display is told.
    pub shown: u8,
    /// The host's clock when this number was reckoned: a caller that counts
    /// the trial's time on from it reckons with the same moments.
    pub at_ms: u64,
    pub version: Version,
    /// On the VERIFY event of a written image: how long its phases took.
    pub times: Option<XferTimes>,
    /// About the node's boot core, written before its app: its CONFIRM is
    /// not the node's (the app is confirmed after it).
    pub boot_core: bool,
}

/// How long one image's transfer took, by phase, in ms of the host's clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XferTimes {
    /// The round trip of the request the node erases the slot on: the last
    /// header chunk (XFER_BEGIN for an image without header). The header's
    /// other requests are in no phase.
    pub erase_ms: u32,
    /// The image body (XFER_DATA), with the displays' progress updates.
    pub data_ms: u32,
    /// XFER_END: the node's digest over what it wrote.
    pub verify_ms: u32,
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

    /// STATUS of the target's boot core slots (version and header digest of
    /// each, the running and the confirmed core). A target whose MCU keeps
    /// no boot core slots answers `Unsupported` (`Error::Rejected`); one
    /// that predates the request answers `Arg`.
    pub async fn boot_core_status(&mut self, to: Target) -> Result<StatusResp, T::Error> {
        let body = self
            .call(to, Cmd::Status, &[target::BOOTCORE], REQUEST_MS)
            .await?;
        StatusResp::parse(&body).map_err(|_| Error::BadResponse)
    }

    /// VERSION of the target: its stage0, boot core and app slots, wire
    /// version and MCU id, each when the node could read it. `None`: its
    /// agent predates VERSION (it answers `Unsupported`): the versions are
    /// unknown, not an error.
    pub async fn version(&mut self, to: Target) -> Result<Option<VersionResp>, T::Error> {
        let body = match self.call(to, Cmd::Version, &[], REQUEST_MS).await {
            Ok(b) => b,
            Err(Error::Rejected {
                status: Status::Unsupported,
                ..
            }) => return Ok(None),
            Err(e) => return Err(e),
        };
        VersionResp::parse(&body)
            .map(Some)
            .map_err(|_| Error::BadResponse)
    }

    /// Hub HELLO, then a short RELAY HELLO to every I2C address but the ROM
    /// loaders' (`rom_addrs`, from the MCU definitions: never probed with
    /// DAGON frames) and one on the UART.
    pub async fn discover(&mut self, rom_addrs: &[u8]) -> Result<Discovery, T::Error> {
        let hub = self.hello(Target::Hub).await?;
        let mut nodes = Vec::new();
        let i2c = I2C_SCAN.filter(|a| !rom_addrs.contains(a)).map(Target::I2c);
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
        self.transfer(Target::Hub, &hub, xfer, &mut quiet)
            .await
            .map(|_| ())
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
        self.apply_release(bundle, topology, Some(pubkey), nodes, on_event)
            .await
    }

    /// [`Host::apply`] with the host-side signature and digest checks
    /// skipped (`pubkey = None`), so the nodes' own checks reject a tampered
    /// image (E_SIGNATURE, E_DIGEST) for demonstration. The bundle must
    /// still parse and the topology must still match.
    pub async fn apply_unverified(
        &mut self,
        bundle: &[u8],
        topology: &[u8],
        nodes: Option<&[u8]>,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Vec<NodeOutcome<T::Error>>, T::Error> {
        self.apply_release(bundle, topology, None, nodes, on_event)
            .await
    }

    async fn apply_release(
        &mut self,
        bundle: &[u8],
        topology: &[u8],
        pubkey: Option<&[u8; 32]>,
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
                Some(to) => match self.update_node(&b, node, to, false, on_event).await {
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
        rewrite: bool,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Version, T::Error> {
        self.write = Default::default();
        let st = self.status(to).await?;
        let r = self.update_inner(b, node, to, &st, rewrite, on_event).await;
        if let Err(e) = &r {
            let ph = match e {
                Error::Rejected {
                    status: Status::Signature | Status::Key | Status::Compat | Status::Digest,
                    ..
                } => phase::REJECTED,
                _ => phase::FAILED,
            };
            let v = st.hello.active_version;
            let at_ms = self.clock.now_ms();
            on_event(Event {
                node,
                phase: ph,
                percent: 0,
                shown: 0,
                at_ms,
                version: v,
                times: None,
                boot_core: false,
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
        rewrite: bool,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Version, T::Error> {
        let running = st.hello.active_version;
        let core = self.update_core(b, node, to, running, on_event).await;
        self.core_write = false;
        let core = core?;
        let agent = self.update_agent(b, node, to, on_event).await?;
        if core || agent {
            let st = self.status(to).await?;
            return self.update_app(b, node, to, &st, rewrite, on_event).await;
        }
        self.update_app(b, node, to, st, rewrite, on_event).await
    }

    /// The node's recovery agent (the factory slot of a vendor-stage2 MCU)
    /// from the release, before its app: written while the confirmed app
    /// runs (the node refuses otherwise), nothing selected or rebooted.
    /// `true` when it was written.
    async fn update_agent(
        &mut self,
        b: &BundleView<'_>,
        node: u8,
        to: Target,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<bool, T::Error> {
        const FACTORY: u8 = FACTORY_SLOT;
        let Some(img) = image_of(b, node, image_kind::AGENT, FACTORY) else {
            return Ok(false);
        };
        let raw: &[u8; HEADER_SIZE] = img
            .get(..HEADER_SIZE)
            .and_then(|h| h.try_into().ok())
            .ok_or(Error::Format(dagon_format::FormatError::Truncated))?;
        let want = dagon_format::header::header_digest16(raw);
        let st = self.status(to).await?;
        if st.status.slots[usize::from(FACTORY)].digest16 == want {
            return Ok(false);
        }
        let v = ImageHeader::parse(raw)?.version;
        let (hdr, body) = img.split_at(HEADER_SIZE);
        self.emit(on_event, node, phase::TRANSFER, 0, v).await;
        let xfer = Xfer {
            tgt: target::AGENT,
            slot: FACTORY,
            hdr,
            body,
            show: Some((node, v)),
        };
        let times = self.transfer(to, &st.hello, xfer, on_event).await?;
        let after = self.status(to).await?;
        if after.status.slots[usize::from(FACTORY)].digest16 != want {
            return Err(Error::DigestMismatch);
        }
        self.emit_verified(on_event, node, v, times).await;
        Ok(true)
    }

    /// The node's boot core from the release, before its app (an app may
    /// need the newer core): when the release has boot core images for the
    /// node and the running core's version is none of theirs, the image for the idle
    /// core slot goes in, is set pending, the node reboots, and the new
    /// core must be the confirmed one when the app (`app`, unchanged)
    /// answers again. `true` when a core was written.
    async fn update_core(
        &mut self,
        b: &BundleView<'_>,
        node: u8,
        to: Target,
        app: Version,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<bool, T::Error> {
        let cores: Vec<&[u8]> = [slot_binding::A, slot_binding::B]
            .into_iter()
            .filter_map(|s| image_of(b, node, image_kind::BOOTCORE, s))
            .collect();
        if cores.is_empty() {
            return Ok(false);
        }
        let cs = self.boot_core_status(to).await?;
        let running = cs.slots.get(usize::from(cs.active)).copied();
        // By version, as apps: a changed boot core is written only when its
        // version (its boot crate's) was raised.
        let installed = cores.iter().any(|img| {
            img.get(..HEADER_SIZE)
                .and_then(|h| <&[u8; HEADER_SIZE]>::try_from(h).ok())
                .and_then(|h| ImageHeader::parse(h).ok())
                .is_some_and(|h| running.is_some_and(|r| r.version == h.version))
        });
        if installed {
            return Ok(false);
        }
        let slot = free_slot(&cs).ok_or(Error::NoImage { node })?;
        let img = image_of(b, node, image_kind::BOOTCORE, slot).ok_or(Error::NoImage { node })?;
        let raw: &[u8; HEADER_SIZE] = img
            .get(..HEADER_SIZE)
            .and_then(|h| h.try_into().ok())
            .ok_or(Error::Format(dagon_format::FormatError::Truncated))?;
        let v = ImageHeader::parse(raw)?.version;
        let (hdr, body) = img.split_at(HEADER_SIZE);
        self.core_write = true;
        self.emit(on_event, node, phase::TRANSFER, 0, v).await;
        let hello = self.hello(to).await?;
        let xfer = Xfer {
            tgt: target::BOOTCORE,
            slot,
            hdr,
            body,
            show: Some((node, v)),
        };
        let times = match self.transfer(to, &hello, xfer, on_event).await {
            Err(Error::Rejected {
                cmd: Cmd::XferBegin,
                status: Status::Unsupported,
            }) => return Err(Error::BootCoreRefused),
            r => r?,
        };
        self.emit_verified(on_event, node, v, times).await;
        let mut p = [0u8; 2];
        SetPending {
            target: target::BOOTCORE,
            slot,
        }
        .encode(&mut p)
        .expect("2 bytes");
        self.call(to, Cmd::SetPending, &p, VERIFY_MS).await?;
        self.emit(on_event, node, phase::REBOOT, 0, v).await;
        match self
            .call(to, Cmd::Reboot, &[reboot_mode::NORMAL], REQUEST_MS)
            .await
        {
            Ok(_) | Err(Error::Timeout | Error::Relay(Status::Timeout)) => {}
            Err(e) => return Err(e),
        }
        self.await_version(to, app).await?;
        let after = self.boot_core_status(to).await?;
        if after.confirmed != slot {
            let back = after.slots.get(usize::from(after.active)).copied();
            return Err(Error::BootCoreRolledBack {
                expected: v,
                running: back.map(|s| s.version).unwrap_or_default(),
                reason: after.last_reason,
            });
        }
        self.emit(on_event, node, phase::CONFIRM, 100, v).await;
        Ok(true)
    }

    /// The node's app from the release into its free APP slot; unless
    /// `rewrite`, not when it is what the node runs, so the other slot keeps
    /// the version to go back to. The node's write is then over: its number
    /// says so, as a confirmation does.
    async fn update_app(
        &mut self,
        b: &BundleView<'_>,
        node: u8,
        to: Target,
        st: &NodeState,
        rewrite: bool,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Version, T::Error> {
        let unchanged = !rewrite && app_unchanged(b, node, st);
        if unchanged || !has_kind(b, node, image_kind::APP) {
            let running = st.hello.active_version;
            if unchanged {
                self.note(&format!(
                    "node {node}: app {running} not written, it is what the node runs"
                ));
            }
            self.emit(on_event, node, phase::CONFIRM, 100, running)
                .await;
            return Ok(running);
        }
        let slot = free_slot(&st.status).ok_or(Error::NoImage { node })?;
        let img = image_for(b, node, slot).ok_or(Error::NoImage { node })?;
        // Unverified releases reach here too: check the length, not assume.
        let raw: &[u8; HEADER_SIZE] = img
            .get(..HEADER_SIZE)
            .and_then(|h| h.try_into().ok())
            .ok_or(Error::Format(dagon_format::FormatError::Truncated))?;
        let h = ImageHeader::parse(raw)?;
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
        let times = self.transfer(to, &st.hello, xfer, on_event).await?;
        self.emit_verified(on_event, node, v, times).await;
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
        // Not resent: a node that already reset has forgotten the seq and
        // would reboot again. A lost reply is settled by the HELLO polls.
        match self
            .call(to, Cmd::Reboot, &[reboot_mode::NORMAL], REQUEST_MS)
            .await
        {
            Ok(_) | Err(Error::Timeout | Error::Relay(Status::Timeout)) => {}
            Err(e) => return Err(e),
        }
        self.set_applied(to, v);
        let h = self.await_version(to, v).await?;
        if h.trial {
            self.confirm_trial(to, v).await?;
        }
        self.emit(on_event, node, phase::CONFIRM, 100, v).await;
        Ok(())
    }

    /// CONFIRM, sent again after each [`CONFIRM_RETRY_MS`] pause while the
    /// target answers E_STATE; then E_STATE fails it as before.
    async fn confirm_trial(&mut self, to: Target, v: Version) -> Result<(), T::Error> {
        let mut pauses = CONFIRM_RETRY_MS.iter();
        loop {
            let r = self.call(to, Cmd::Confirm, &[target::APP], VERIFY_MS).await;
            let not_ready = matches!(
                r,
                Err(Error::Rejected {
                    cmd: Cmd::Confirm,
                    status: Status::State
                })
            );
            match pauses.next() {
                Some(&ms) if not_ready => {
                    self.note(&format!(
                        "{to:?}: {v} not ready to confirm (E_STATE); again in {ms} ms"
                    ));
                    self.clock.sleep_ms(ms).await;
                }
                _ => return r.map(|_| ()),
            }
        }
    }

    /// Polls HELLO after REBOOT until `v` answers (hub: reopening the port).
    async fn await_version(&mut self, to: Target, v: Version) -> Result<HelloResp, T::Error> {
        let (limit, pause) = match to {
            Target::Hub => (HUB_RECONNECT_MS, REOPEN_MS),
            _ => (REBOOT_WAIT_MS, self.node_poll_ms),
        };
        let deadline = self.clock.now_ms() + limit as u64;
        // Another version may answer until the reset really happens (the
        // old firmware still running) or for good (rolled back); only the
        // end of the window tells them apart.
        let mut other = None;
        loop {
            // reopens before every hub attempt; add a grace period
            // for slow enumeration if real ports need it.
            // A link that goes away with the hub is opened again; one that
            // stays (a UART adapter) is only polled.
            let reopened = to != Target::Hub
                || self.transport.reconnect_kind() == crate::Reconnect::Stays
                || self.reopen().await.is_ok();
            if reopened {
                match self.hello_within(to, REQUEST_MS).await {
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
        self.write = Default::default();
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
    /// loader: boot core, APP-A, a fresh state log and stage0 (where the
    /// MCU has one), then its HELLO.
    pub async fn recover(
        &mut self,
        node: u8,
        a: &RecoverAssets<'_>,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<(RecoverPath, Version), T::Error> {
        self.write = Default::default();
        let (b, topo) = check_release(a.release, a.topology, Some(a.pubkey))?;
        let to = Target::of_node(&topo, node).ok_or(Error::Unreachable { node })?;
        match self.hello(to).await {
            Ok(h) => {
                let path = if h.mode == payload::mode::AGENT {
                    RecoverPath::Factory
                } else {
                    RecoverPath::Retransfer
                };
                // Chosen by the user: the app goes in again even when it is the same.
                let v = self.update_node(&b, node, to, true, on_event).await?;
                return Ok((path, v));
            }
            Err(Error::Transport(e)) => return Err(Error::Transport(e)),
            Err(_) => {}
        }
        let Some(map) = rom_map(&topo, node, a.roms) else {
            return Err(Error::Unreachable { node });
        };
        let boot = a.bootcore.ok_or(Error::NoImage { node })?;
        crate::rom::check_bootcore(boot, a.pubkey, map)?;
        let app = image_for(&b, node, slot_binding::A).ok_or(Error::NoImage { node })?;
        crate::rom::check_app_a(app, map)?;
        let app_hdr: &[u8; HEADER_SIZE] = app
            .get(..HEADER_SIZE)
            .and_then(|h| h.try_into().ok())
            .ok_or(Error::Format(dagon_format::FormatError::Truncated))?;
        let v = ImageHeader::parse(app_hdr)?.version;
        let boot_hdr: &[u8; HEADER_SIZE] = boot
            .get(..HEADER_SIZE)
            .and_then(|h| h.try_into().ok())
            .ok_or(Error::Format(dagon_format::FormatError::Truncated))?;
        let stage0 = match map.stage0 {
            Some((_, size)) => {
                let s = a.stage0.ok_or(Error::NoImage { node })?;
                if s.is_empty() || s.len() > size as usize {
                    return Err(Error::Format(dagon_format::FormatError::BadField));
                }
                Some(s)
            }
            None => None,
        };
        let log = crate::rom::first_log(boot_hdr, app_hdr, map)?;
        if !a.rom_confirmed {
            return Err(Error::RomNotConfirmed { node });
        }
        let regions = crate::rom::regions(boot, app, &log, stage0, map);
        let plan = dagon_romlink::Plan {
            erase: &map.erase,
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

    /// How a write of topology node `node` would reach it now. Asks the node
    /// HELLO and changes nothing.
    pub async fn write_path(
        &mut self,
        node: u8,
        topology: &[u8],
        roms: &[RomMap],
    ) -> Result<WritePath, T::Error> {
        let topo = TopologyView::parse(topology)?;
        let to = Target::of_node(&topo, node).ok_or(Error::Unreachable { node })?;
        match self.hello(to).await {
            Ok(_) => Ok(WritePath::Transfer),
            Err(Error::Transport(e)) => Err(Error::Transport(e)),
            Err(_) if rom_map(&topo, node, roms).is_some() => Ok(WritePath::Rom),
            Err(_) => Ok(WritePath::Direct),
        }
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
    ) -> Result<XferTimes, T::Error> {
        let since = |t0: u64, t1: u64| t1.saturating_sub(t0).min(u32::MAX.into()) as u32;
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
        // The hub's display already hears every number from `emit`. The tell
        // comes before XFER_BEGIN, so the number is there when the node's
        // display turns to the transfer, after the erase.
        let mut tell = show.filter(|_| to != Target::Hub);
        let mut told = None;
        self.tell(to, &mut tell, &mut told).await;
        let mut p = [0u8; 8];
        let n = begin.encode(&mut p).expect("8 bytes");
        let begin_wait = if hdr.is_empty() { erase } else { REQUEST_MS };
        let t0 = self.clock.now_ms();
        self.call(to, Cmd::XferBegin, &p[..n], begin_wait).await?;
        // The node erases before it answers XFER_BEGIN (no header) or the
        // last header chunk: that request's round trip is the erase phase.
        let mut erase_ms = since(t0, self.clock.now_ms());

        let chunk = chunk_len(hello.max_payload, self.link_max(to)).ok_or(Error::BadResponse)?;
        let hdr_chunks = hdr.len().div_ceil(chunk);
        for (i, c) in hdr.chunks(chunk).enumerate() {
            let last = i + 1 == hdr_chunks;
            let wait = if last { erase } else { REQUEST_MS };
            let t = self.clock.now_ms();
            self.chunk(to, Cmd::XferHdr, i * chunk, c, wait).await?;
            if last {
                erase_ms = since(t, self.clock.now_ms());
            }
        }
        let t1 = self.clock.now_ms();
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
                self.tell(to, &mut tell, &mut told).await;
            }
        }
        let t2 = self.clock.now_ms();
        let d = self.call(to, Cmd::XferEnd, &[], VERIFY_MS).await?;
        let digest = payload::parse_digest(&d).map_err(|_| Error::BadResponse)?;
        if digest != sha256(body) {
            return Err(Error::DigestMismatch);
        }
        Ok(XferTimes {
            erase_ms,
            data_ms: since(t1, t2),
            verify_ms: since(t2, self.clock.now_ms()),
        })
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
        let (shown, at_ms) = self.shown_of(node, ph, percent);
        on_event(Event {
            node,
            phase: ph,
            percent,
            shown,
            at_ms,
            version,
            times: None,
            boot_core: self.core_write,
        });
        self.show(node, ph, shown, version, &[]).await;
    }

    /// VERIFY of an image written in `times`.
    async fn emit_verified(
        &mut self,
        on_event: &mut dyn FnMut(Event),
        node: u8,
        version: Version,
        times: XferTimes,
    ) {
        let (shown, at_ms) = self.shown_of(node, phase::VERIFY, 100);
        on_event(Event {
            node,
            phase: phase::VERIFY,
            percent: 100,
            shown,
            at_ms,
            version,
            times: Some(times),
            boot_core: self.core_write,
        });
        self.show(node, phase::VERIFY, shown, version, &[]).await;
    }

    /// The node's one number for this report, and the moment it was reckoned.
    fn shown_of(&mut self, node: u8, ph: u8, percent: u8) -> (u8, u64) {
        let trial = self
            .trial_ms
            .iter()
            .find(|(n, _)| *n == node)
            .map(|(_, ms)| *ms);
        let now = self.clock.now_ms();
        let shown = self
            .write
            .report(node, ph, percent, self.core_write, now, trial);
        (shown, now)
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
        let _ = self.show_to(Target::Hub, &show).await;
    }

    /// One SHOW, never resent.
    async fn show_to(&mut self, to: Target, show: &Show<'_>) -> Result<(), T::Error> {
        let mut p = [0u8; 7 + payload::SHOW_TEXT_MAX];
        let n = show.encode(&mut p).map_err(|_| Error::BadResponse)?;
        self.call_once(to, Cmd::Show, &p[..n], REQUEST_MS)
            .await
            .map(|_| ())
    }

    /// Tells the node being written its one number when it moved, for its
    /// own display. A display must not cost a write: one try per step. An
    /// agent that predates it answers `Unsupported` and is told nothing more
    /// for this image. A lost one is told again at the next step, so the
    /// number does not freeze; a link that truly failed is the next transfer
    /// request's to report.
    async fn tell(&mut self, to: Target, who: &mut Option<(u8, Version)>, told: &mut Option<u8>) {
        let Some((node, version)) = *who else {
            return;
        };
        let percent = self.write.shown();
        if *told == Some(percent) {
            return;
        }
        let show = Show {
            node,
            phase: phase::TRANSFER,
            percent,
            version,
            text: &[],
        };
        match self.show_to(to, &show).await {
            Ok(()) => *told = Some(percent),
            Err(Error::Rejected {
                status: Status::Unsupported,
                ..
            }) => *who = None,
            Err(_) => {}
        }
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

/// Release `bundle` built for `topology` and, when `pubkey` is given,
/// verified under it. The check [`Host::apply`] runs before sending
/// anything; callers may run it first to show the verdict.
pub fn check_release<'a, E>(
    bundle: &'a [u8],
    topology: &'a [u8],
    pubkey: Option<&[u8; 32]>,
) -> Result<(BundleView<'a>, TopologyView<'a>), E> {
    let b = BundleView::parse(bundle)?;
    if let Some(k) = pubkey {
        b.verify(k)?;
    }
    let topo = TopologyView::parse(topology)?;
    let release = b.manifest().fields().topology_sha256;
    let local = sha256(topology);
    if release != local {
        return Err(Error::TopologyMismatch { release, local });
    }
    Ok((b, topo))
}

/// An STM32F401 on I2C whose link enters the ROM loader through NRST/BOOT0.
/// The ROM map for `node` when it is reached through its ROM I2C loader:
/// an I2C link with ROM entry stm32-i2c-rom and a map for its MCU.
fn rom_map<'m>(topo: &TopologyView<'_>, node: u8, roms: &'m [RomMap]) -> Option<&'m RomMap> {
    if (node as usize) >= topo.node_count() {
        return None;
    }
    let rom = (0..topo.link_count()).map(|i| topo.link(i)).any(|l| {
        l.to == node
            && l.iface == topology::iface::I2C_BUS
            && l.rom_entry == topology::rom_entry::STM32_I2C_ROM
    });
    let mcu = topo.node(node as usize).mcu_id;
    roms.iter().find(|m| rom && m.wire_id == mcu)
}

/// What [`Host::recover`] needs: the release, the current topology binary,
/// the verifying key and, for the ROM path, the signed boot core and, on an
/// MCU with a stage0, stage0's bytes.
#[derive(Clone, Copy, Debug)]
pub struct RecoverAssets<'a> {
    pub release: &'a [u8],
    pub topology: &'a [u8],
    pub pubkey: &'a [u8; 32],
    pub bootcore: Option<&'a [u8]>,
    pub stage0: Option<&'a [u8]>,
    /// ROM maps of the MCUs that have a ROM I2C loader (from the Registry).
    pub roms: &'a [RomMap],
    /// The user was told the node does not answer and its contents will be
    /// rewritten, and went on. Without it the ROM path touches nothing.
    pub rom_confirmed: bool,
}

/// How a write reaches a node ([`Host::write_path`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritePath {
    /// It answers: the normal transfer.
    Transfer,
    /// It does not answer and its MCU and link have the ROM entry.
    Rom,
    /// It does not answer and has no ROM entry: the factory write, directly.
    Direct,
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
        Error::BootCoreRolledBack { .. } => "BOOT CORE ROLLED BACK",
        Error::BootCoreRefused => "NO BOOT CORE TRIAL",
        Error::DigestMismatch => "E_DIGEST",
        Error::NoImage { .. } => "NO IMAGE",
        Error::RomNotConfirmed { .. } => "ROM NOT CONFIRMED",
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

/// Nodes with an APP or boot core image in the release, filtered by
/// `only`: children by update order, then the coordinator.
fn plan(b: &BundleView<'_>, topo: &TopologyView<'_>, only: Option<&[u8]>) -> Vec<u8> {
    let m = b.manifest();
    let mut nodes: Vec<u8> = (0..m.entry_count())
        .map(|i| m.entry(i))
        .filter(|e| {
            matches!(
                e.image_kind,
                image_kind::APP | image_kind::BOOTCORE | image_kind::AGENT
            )
        })
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

/// The header digest of the recovery agent `bundle` holds for `node`: the node
/// gets it written when its factory slot holds another ([`Host::apply`]).
pub fn agent_digest(b: &BundleView<'_>, node: u8) -> Option<[u8; 16]> {
    image_digest(b, node, image_kind::AGENT, FACTORY_SLOT)
}

/// The header digest of the app `bundle` holds for `node` in `slot`. A normal
/// write leaves it out when it is what the node runs in that slot, off trial.
pub fn app_digest(b: &BundleView<'_>, node: u8, slot: u8) -> Option<[u8; 16]> {
    image_digest(b, node, image_kind::APP, slot)
}

fn image_digest(b: &BundleView<'_>, node: u8, kind: u8, slot: u8) -> Option<[u8; 16]> {
    let raw = image_of(b, node, kind, slot)?.get(..HEADER_SIZE)?;
    Some(dagon_format::header::header_digest16(raw.try_into().ok()?))
}

fn app_unchanged(b: &BundleView<'_>, node: u8, st: &NodeState) -> bool {
    let active = st.status.active;
    !st.hello.trial
        && st
            .status
            .slots
            .get(usize::from(active))
            .is_some_and(|s| app_digest(b, node, active) == Some(s.digest16))
}

/// Slot neither running nor confirmed.
fn free_slot(st: &StatusResp) -> Option<u8> {
    [0u8, 1]
        .into_iter()
        .find(|&s| s != st.active && s != st.confirmed)
}

/// APP image of `node` bound to `slot` (or to any slot).
fn image_for<'a>(b: &BundleView<'a>, node: u8, slot: u8) -> Option<&'a [u8]> {
    image_of(b, node, image_kind::APP, slot)
}

/// Image of `kind` for `node` bound to `slot` (or to any slot).
fn image_of<'a>(b: &BundleView<'a>, node: u8, kind: u8, slot: u8) -> Option<&'a [u8]> {
    let m = b.manifest();
    (0..b.item_count()).find_map(|i| {
        let it = b.item(i);
        let e = (it.entry_index as usize) < m.entry_count();
        let e = e.then(|| m.entry(it.entry_index as usize))?;
        let fits = e.node_index == node
            && e.image_kind == kind
            && (e.slot_binding == slot || e.slot_binding == slot_binding::ANY);
        fits.then(|| b.item_bytes(i))
    })
}

/// The release has an image of `kind` for `node`.
fn has_kind(b: &BundleView<'_>, node: u8, kind: u8) -> bool {
    let m = b.manifest();
    (0..m.entry_count())
        .map(|i| m.entry(i))
        .any(|e| e.node_index == node && e.image_kind == kind)
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
