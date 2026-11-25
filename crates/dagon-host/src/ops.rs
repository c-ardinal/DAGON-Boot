//! The procedures: discover, status, apply, confirm, rollback, recover.

use alloc::format;
use alloc::vec::Vec;
use dagon_ed25519::Ed25519Compact;
use dagon_format::bundle::BundleView;
use dagon_format::header::{header_digest16, image_kind, slot_binding, McuId, HEADER_SIZE};
use dagon_format::topology::TopologyView;
use dagon_format::{sha256, ImageHeader, Version};
use dagon_port::timeout::{
    CONFIRM_RETRY_MS, ERASE_FALLBACK_MS, REBOOT_WAIT_MS, REQUEST_MS, VERIFY_MS,
};
use dagon_port::Addressing;
use dagon_proto::payload::{
    self, phase, reboot_mode, target, HelloResp, SetPending, Show, SlotState, StatusResp,
    VersionResp, XferBegin, XferChunk,
};
use dagon_proto::{Cmd, Status};

use crate::progress::{Event, XferTimes};
use crate::rom::RomMap;
use crate::{request, Error, Host, HostClock, Result, RouteError, Target, Transport};

/// REBOOT until the hub's new HELLO, reopening the port meanwhile. A host
/// margin, not an MCU or link property: it stays well inside the 60 s the
/// hub keeps its trial watchdog fed, so CONFIRM always lands in time. Public
/// at the crate root; the Console reads it through dagon-wasm's
/// `host_reconnect_ms`.
pub const HUB_RECONNECT_MS: u32 = 45_000;
/// Pause between reopen attempts while the hub is away.
pub const REOPEN_MS: u32 = 1_000;
/// RELAY timeout per address while discovering.
pub const DISCOVER_MS: u32 = 20;
const I2C_SCAN: core::ops::RangeInclusive<u8> = 0x08..=0x77;
/// XFER_HDR / XFER_DATA bytes before the data: the offset.
const CHUNK_HEAD: usize = XferChunk::OVERHEAD;
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
    /// The node the PC is connected to.
    pub hub: HelloResp,
    /// Nodes that answered RELAY HELLO, in the order of the connected
    /// node's relay table, each link's by address.
    pub nodes: Vec<(Target, HelloResp)>,
}

/// One planned node's result. New results are added over time: a caller
/// handles one it does not know as a node left unconfirmed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Outcome<E> {
    /// Running and confirmed on this version.
    Done(Version),
    /// Stopped here; later nodes are `NotStarted`.
    Failed(Error<E>),
    NotStarted,
    /// A linked node that answered nothing in this session, not even HELLO
    /// (the relaying node said so): no DAGON runs on it, as on a node added to the
    /// configuration and never written. Nothing was sent to it and the run went on with the other nodes; it
    /// takes its first write directly (the factory write, or the ROM path
    /// where its MCU and link have one: [`Host::write_path`]).
    NeedsFirstWrite,
    /// Written and confirmed on the release's version, then taken back to
    /// this one (the version it ran before): another node of the run stayed
    /// off the release's version, and a compatibility constraint of the
    /// release ties the two ([`crate::realign`]).
    Realigned(Version),
    /// Runs this version, and a compatibility constraint of the release
    /// with a node that stayed off the release's version does not hold: it
    /// could not be taken back.
    Incompatible(Version),
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

impl<T: Transport, C: HostClock> Host<T, C> {
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
        let core_status = request(|b| payload::encode_status_req(target::BOOTCORE, b));
        let body = self.call(to, Cmd::Status, &core_status, REQUEST_MS).await?;
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

    /// HELLO of the connected node, then a RELAY HELLO over every entry of
    /// its relay table ([`Host::local`]): a short one to every 7-bit
    /// address of a seven-bit link but the ROM loaders' (`rom_addrs`, from
    /// the MCU definitions: never probed with DAGON frames), one to the
    /// address of a fixed-address link. Without a configuration only the
    /// connected node is asked.
    pub async fn discover(&mut self, rom_addrs: &[u8]) -> Result<Discovery, T::Error> {
        let hub = self.hello(Target::Local).await?;
        let links = match &self.local {
            Ok(me) => me.links.clone(),
            Err(RouteError::NoConfiguration) => Vec::new(),
            Err(e) => return Err(Error::Route(e.clone())),
        };
        let mut nodes = Vec::new();
        for (to, wait) in probes(&links, rom_addrs) {
            match self.hello_within(to, wait).await {
                Ok(h) => nodes.push((to, h)),
                Err(Error::Transport(e)) => return Err(Error::Transport(e)),
                Err(_) => {}
            }
        }
        Ok(Discovery { hub, nodes })
    }

    /// Checks `bundle` (signature chain under `pubkey`, and that it was built
    /// for `topology`, the caller's current canonical binary), then updates
    /// the nodes: the others in update order, the node the PC is connected
    /// to last. `nodes` limits the run to these topology node indices. Stops
    /// at the first node that fails; a linked node that has answered nothing
    /// in this session is [`Outcome::NeedsFirstWrite`] and the run goes on.
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
        // Every target is resolved before the first byte goes out: a route
        // that fails sends nothing.
        let local = self
            .local
            .as_ref()
            .map_err(|e| Error::Route(e.clone()))?
            .index;
        let planned = plan(&b, &topo, nodes, local)
            .into_iter()
            .map(|node| Ok((node, self.target_of(&topo, node)?)))
            .collect::<core::result::Result<Vec<_>, RouteError>>()
            .map_err(Error::Route)?;

        let mut out = Vec::new();
        let mut before = Vec::new();
        let mut failed = false;
        for (node, target) in planned {
            let (was, outcome) = if failed {
                (None, Outcome::NotStarted)
            } else {
                self.apply_node(&b, node, target, on_event).await
            };
            failed |= matches!(outcome, Outcome::Failed(_));
            before.push(was);
            out.push(NodeOutcome {
                node,
                target,
                outcome,
            });
        }
        if failed {
            self.realign(&b, &mut out, &before, on_event).await;
        }
        Ok(out)
    }

    /// After a run that stopped at a node: takes the nodes written before it
    /// back to the version they ran, where a compatibility constraint of the
    /// release no longer holds with a node that stayed off the release's
    /// version ([`crate::realign::realign`]). `before` is the app version
    /// each node of `out` kept when its turn came ([`kept`]).
    async fn realign(
        &mut self,
        b: &BundleView<'_>,
        out: &mut [NodeOutcome<T::Error>],
        before: &[Option<Version>],
        on_event: &mut dyn FnMut(Event),
    ) {
        let m = b.manifest();
        let constraints: Vec<_> = (0..m.constraint_count()).map(|i| m.constraint(i)).collect();
        if constraints.is_empty() {
            return;
        }
        let mut nodes = Vec::new();
        for (r, was) in out.iter().zip(before) {
            let release = app_version(b, r.node);
            let (before, running) = match (&r.outcome, *was) {
                (Outcome::Done(v), Some(was)) => (was, release.unwrap_or(*v)),
                (Outcome::Failed(_), Some(was)) => match self.hello(r.target).await {
                    // Left on trial, the release's version is not kept: the
                    // node goes back by itself.
                    Ok(h) if h.trial => (was, was),
                    Ok(h) => (was, h.active_version),
                    Err(_) => continue,
                },
                // Nothing was sent to a node that was not started, or whose
                // state could not be read: it keeps what it kept.
                (Outcome::NotStarted, _) | (Outcome::Failed(_), None) => {
                    let v = match self.status(r.target).await {
                        Ok(st) => kept(&st),
                        Err(_) => match self.hello(r.target).await {
                            Ok(h) => h.active_version,
                            Err(_) => continue,
                        },
                    };
                    (v, v)
                }
                _ => continue,
            };
            nodes.push(crate::realign::Node {
                index: r.node,
                release: release.unwrap_or(running),
                before,
                running,
            });
        }
        let plan = crate::realign::realign(&constraints, &nodes);
        for (r, was) in out.iter_mut().zip(before) {
            // A node that failed and still came up on the release's version,
            // confirmed, goes back like one that is done; it keeps its
            // failure as its result.
            let done = match r.outcome {
                Outcome::Done(v) => Some(v),
                Outcome::Failed(_) => None,
                _ => continue,
            };
            if let (Some(v), true) = (done, plan.stuck.contains(&r.node)) {
                r.outcome = Outcome::Incompatible(v);
            }
            if !plan.back.contains(&r.node) {
                continue;
            }
            let slots = slots_of(b, r.node, image_kind::APP);
            let went = self.rollback(r.target, r.node, &slots, on_event).await;
            if let Err(e) = &went {
                self.note(&format!("{:?}: could not go back: {}", r.target, reason(e)));
            }
            if done.is_none() {
                continue;
            }
            r.outcome = match went {
                Ok(v) => Outcome::Realigned(v),
                // The switch may have happened before the error: what the
                // node runs now is read again, not assumed.
                Err(e) => match self.hello(r.target).await {
                    Ok(h) if !h.trial && Some(h.active_version) == *was => {
                        Outcome::Realigned(h.active_version)
                    }
                    Ok(h) if !h.trial => Outcome::Incompatible(h.active_version),
                    // On trial or silent, no version of the node is settled.
                    _ => Outcome::Failed(e),
                },
            };
        }
    }

    /// One planned node of an apply: [`Outcome::NeedsFirstWrite`] when the
    /// relaying node says it answers nothing and it has answered nothing in this
    /// session, else its update. A node that answered earlier in the session
    /// and is silent now ran DAGON: it fails and stops the run.
    async fn apply_node(
        &mut self,
        b: &BundleView<'_>,
        node: u8,
        to: Target,
        on_event: &mut dyn FnMut(Event),
    ) -> (Option<Version>, Outcome<T::Error>) {
        let st = match self.status(to).await {
            Ok(st) => st,
            Err(e) if is_silent(&e) && !self.opened.contains(&to) => {
                return (None, Outcome::NeedsFirstWrite)
            }
            Err(e) => return (None, Outcome::Failed(e)),
        };
        let outcome = match self.update_with(b, node, to, &st, false, on_event).await {
            Ok(v) => Outcome::Done(v),
            Err(e) => Outcome::Failed(e),
        };
        (Some(kept(&st)), outcome)
    }

    async fn update_node(
        &mut self,
        b: &BundleView<'_>,
        node: u8,
        to: Target,
        rewrite: bool,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Version, T::Error> {
        let st = self.status(to).await?;
        self.update_with(b, node, to, &st, rewrite, on_event).await
    }

    /// [`Host::update_node`] from the STATUS `st` just read.
    async fn update_with(
        &mut self,
        b: &BundleView<'_>,
        node: u8,
        to: Target,
        st: &NodeState,
        rewrite: bool,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Version, T::Error> {
        self.write = Default::default();
        let r = self.update_inner(b, node, to, st, rewrite, on_event).await;
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
                wrote: self.write.wrote(),
                at_ms,
                version: v,
                times: None,
                boot_core: false,
            });
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
        let core = self.update_core(b, node, to, st, on_event).await;
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
        self.report(on_event, node, phase::TRANSFER, 0, v);
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
        self.report_verified(on_event, node, v, times);
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
        st: &NodeState,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<bool, T::Error> {
        let app = st.hello.active_version;
        let core_slots = slots_of(b, node, image_kind::BOOTCORE);
        let cores: Vec<&[u8]> = core_slots
            .iter()
            .filter_map(|&s| image_of(b, node, image_kind::BOOTCORE, s))
            .collect();
        if cores.is_empty() {
            return Ok(false);
        }
        let cs = self.boot_core_status(to).await?;
        let running = cs.slots.get(usize::from(cs.active)).copied();
        // By version, as apps: a changed boot core is written only when its
        // version (its boot crate's) was raised.
        let installed = slots_settled(&cs)
            && cores.iter().any(|img| {
                img.get(..HEADER_SIZE)
                    .and_then(|h| <&[u8; HEADER_SIZE]>::try_from(h).ok())
                    .and_then(|h| ImageHeader::parse(h).ok())
                    .is_some_and(|h| running.is_some_and(|r| r.version == h.version))
            });
        if installed {
            return Ok(false);
        }
        // The only core is not rewritten under an app trial (the node
        // refuses too): the user confirms or rolls the app back first.
        if core_slots.len() == 1 && st.hello.trial {
            return Err(Error::AppOnTrial { running: app });
        }
        let slot = free_slot(&core_slots, &cs).ok_or(Error::NoImage { node })?;
        let img = image_of(b, node, image_kind::BOOTCORE, slot).ok_or(Error::NoImage { node })?;
        let raw: &[u8; HEADER_SIZE] = img
            .get(..HEADER_SIZE)
            .and_then(|h| h.try_into().ok())
            .ok_or(Error::Format(dagon_format::FormatError::Truncated))?;
        let v = ImageHeader::parse(raw)?.version;
        let (hdr, body) = img.split_at(HEADER_SIZE);
        self.core_write = true;
        self.report(on_event, node, phase::TRANSFER, 0, v);
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
        self.report_verified(on_event, node, v, times);
        let sp = SetPending {
            target: target::BOOTCORE,
            slot,
        };
        let p = request(|b| sp.encode(b));
        self.call(to, Cmd::SetPending, &p, VERIFY_MS).await?;
        self.report(on_event, node, phase::REBOOT, 0, v);
        match self
            .call(to, Cmd::Reboot, &reboot_request(), REQUEST_MS)
            .await
        {
            Ok(_) | Err(Error::Timeout | Error::Relay(Status::Timeout)) => {}
            Err(e) => return Err(e),
        }
        self.await_version(to, app).await?;
        let after = self.boot_core_status(to).await?;
        // The new core itself, confirmed: with one boot core slot the
        // record naming that slot may still be the old core's.
        let digest = header_digest16(raw);
        let took_over = after.confirmed == slot
            && after
                .slots
                .get(usize::from(slot))
                .is_some_and(|s| s.state == SlotState::Confirmed && s.digest16 == digest);
        if !took_over {
            // The core that runs instead; none when stage0 started the app.
            let back = (after.slots.get(usize::from(after.active)))
                .filter(|s| s.state == SlotState::Confirmed)
                .copied();
            return Err(Error::BootCoreRolledBack {
                expected: v,
                running: back.map(|s| s.version).unwrap_or_default(),
                reason: after.last_reason,
            });
        }
        self.report(on_event, node, phase::CONFIRM, 100, v);
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
            self.report(on_event, node, phase::CONFIRM, 100, running);
            return Ok(running);
        }
        let slots = slots_of(b, node, image_kind::APP);
        let slot = free_slot(&slots, &st.status).ok_or(Error::NoImage { node })?;
        let img = image_for(b, node, slot).ok_or(Error::NoImage { node })?;
        // The app never writes the slot it runs from: its boot core does.
        let hello = if runs_from(st, slot) {
            self.hand_over(to).await?
        } else {
            st.hello
        };
        // Unverified releases reach here too: check the length, not assume.
        let raw: &[u8; HEADER_SIZE] = img
            .get(..HEADER_SIZE)
            .and_then(|h| h.try_into().ok())
            .ok_or(Error::Format(dagon_format::FormatError::Truncated))?;
        let h = ImageHeader::parse(raw)?;
        let v = h.version;
        let (hdr, body) = img.split_at(HEADER_SIZE);
        self.report(on_event, node, phase::TRANSFER, 0, v);
        let xfer = Xfer {
            tgt: target::APP,
            slot,
            hdr,
            body,
            show: Some((node, v)),
        };
        let times = self.transfer(to, &hello, xfer, on_event).await?;
        self.report_verified(on_event, node, v, times);
        self.switch_and_confirm(to, slot, v, node, on_event).await?;
        Ok(v)
    }

    /// REBOOT mode 1: the app hands over to its boot core's update mode
    /// (DES-RB-030); the boot core's HELLO once it answers in that mode.
    async fn hand_over(&mut self, to: Target) -> Result<HelloResp, T::Error> {
        let p = request(|b| payload::encode_reboot(reboot_mode::UPDATE, b));
        // Not resent, as REBOOT mode 0: the HELLO polls settle a lost reply.
        match self.call(to, Cmd::Reboot, &p, REQUEST_MS).await {
            Ok(_) | Err(Error::Timeout | Error::Relay(Status::Timeout)) => {}
            Err(e) => return Err(e),
        }
        match self
            .await_hello(to, |h| h.mode == payload::mode::UPDATE)
            .await?
        {
            Ok(h) => Ok(h),
            Err(Some(h)) => Err(Error::NotHandedOver { mode: h.mode }),
            Err(None) => Err(Error::Timeout),
        }
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
        let p = request(|b| sp.encode(b));
        self.call(to, Cmd::SetPending, &p, VERIFY_MS).await?;
        self.report(on_event, node, phase::REBOOT, 0, v);
        // Not resent: a node that already reset has forgotten the seq and
        // would reboot again. A lost reply is settled by the HELLO polls.
        match self
            .call(to, Cmd::Reboot, &reboot_request(), REQUEST_MS)
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
        self.report(on_event, node, phase::CONFIRM, 100, v);
        Ok(())
    }

    /// CONFIRM, sent again after each [`CONFIRM_RETRY_MS`] pause while the
    /// target answers E_STATE; then E_STATE fails it as before.
    async fn confirm_trial(&mut self, to: Target, v: Version) -> Result<(), T::Error> {
        let mut pauses = CONFIRM_RETRY_MS.iter();
        loop {
            let r = self
                .call(to, Cmd::Confirm, &confirm_request(), VERIFY_MS)
                .await;
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
        match self.await_hello(to, |h| h.active_version == v).await? {
            Ok(h) => Ok(h),
            Err(Some(h)) => Err(Error::WrongVersion {
                expected: v,
                running: h.active_version,
                trial: h.trial,
            }),
            Err(None) => Err(Error::Timeout),
        }
    }

    /// Polls HELLO after REBOOT until one answers as `want` (hub: reopening
    /// the port); at the end of the window, `Err` with the last HELLO that
    /// answered otherwise, if any.
    async fn await_hello(
        &mut self,
        to: Target,
        want: impl Fn(&HelloResp) -> bool,
    ) -> Result<core::result::Result<HelloResp, Option<HelloResp>>, T::Error> {
        let (limit, pause) = match to {
            Target::Local => (HUB_RECONNECT_MS, REOPEN_MS),
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
            let reopened = to != Target::Local
                || self.transport.reconnect_kind() == crate::Reconnect::Stays
                || self.reopen().await.is_ok();
            if reopened {
                match self.hello_within(to, REQUEST_MS).await {
                    Ok(h) if want(&h) => return Ok(Ok(h)),
                    Ok(h) => other = Some(h),
                    Err(Error::Transport(_) | Error::Timeout | Error::Relay(_)) => {}
                    Err(e) => return Err(e),
                }
            }
            if self.clock.now_ms() >= deadline {
                return Ok(Err(other));
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
        self.call(to, Cmd::Confirm, &confirm_request(), VERIFY_MS)
            .await?;
        Ok(ConfirmOutcome::Confirmed(v))
    }

    /// Back to the version in another of the node's app slots: SET_PENDING,
    /// REBOOT, its HELLO, CONFIRM. `node` is the topology index used for
    /// events; `slots` are the node's app slots (STATUS entries) from its
    /// MCU definition: no other entry is ever taken, so a node with one
    /// slot has no previous version.
    pub async fn rollback(
        &mut self,
        to: Target,
        node: u8,
        slots: &[u8],
        on_event: &mut dyn FnMut(Event),
    ) -> Result<Version, T::Error> {
        self.write = Default::default();
        let st = self.status(to).await?.status;
        let other = rollback_slot(&st, slots).ok_or(Error::NoPreviousVersion)?;
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
        let to = self.target_of(&topo, node).map_err(Error::Route)?;
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
            addr: map.addr,
            erase: &map.erase,
            regions: &regions,
        };
        self.report(on_event, node, phase::ROM, 0, v);
        let stats = {
            // The relay tells its recovery targets apart by the link the
            // node is on: the one RELAY reaches it over.
            let via = match to {
                Target::Linked { link, .. } => Some(link),
                _ => None,
            };
            let mut link = crate::rom::HubRom::new(self, on_event, node, map.addr, via, v);
            dagon_romlink::RomLoader::flash(&map.loader, &mut link, &plan).await
        };
        let stats = match stats {
            Ok(s) => s,
            Err(e) => return Err(Error::Rom(e)),
        };
        self.set_applied(to, v);
        self.await_version(to, v).await?;
        self.report(on_event, node, phase::CONFIRM, 100, v);
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
        let to = self.target_of(&topo, node).map_err(Error::Route)?;
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
        let erase = match show.map(|(n, _)| n) {
            Some(n) => self.erase_wait_ms(n, tgt, slot, hdr.len() as u32, body.len() as u32),
            None => ERASE_FALLBACK_MS,
        };
        let begin = XferBegin {
            target: tgt,
            slot,
            total_len: (hdr.len() + body.len()) as u32,
            hdr_len: hdr.len() as u16,
        };
        // The tell comes before XFER_BEGIN, so the number is there when the
        // node's app turns to the transfer, after the erase.
        let mut tell = show;
        let mut told = None;
        self.tell(to, &mut tell, &mut told).await;
        let p = request(|b| begin.encode(b));
        let n = p.len();
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
                    self.report(on_event, node, phase::TRANSFER, shown, v);
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
        let chunk = XferChunk {
            offset: offset as u32,
            data,
        };
        let p = request(|b| chunk.encode(b));
        let r = self.call(to, cmd, &p, wait).await?;
        let next = payload::parse_next_offset(&r).map_err(|_| Error::BadResponse)?;
        if next as usize != offset + data.len() {
            return Err(Error::BadResponse);
        }
        Ok(())
    }

    /// Reports `ev` to the caller; returns its number.
    pub(crate) fn report(
        &mut self,
        on_event: &mut dyn FnMut(Event),
        node: u8,
        ph: u8,
        percent: u8,
        version: Version,
    ) -> u8 {
        let (shown, at_ms) = self.shown_of(node, ph, percent);
        on_event(Event {
            node,
            phase: ph,
            percent,
            shown,
            wrote: self.write.wrote(),
            at_ms,
            version,
            times: None,
            boot_core: self.core_write,
        });
        shown
    }

    /// VERIFY of an image written in `times`.
    fn report_verified(
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
            wrote: self.write.wrote(),
            at_ms,
            version,
            times: Some(times),
            boot_core: self.core_write,
        });
    }

    /// The node's one number for this report, and the moment it was reckoned.
    fn shown_of(&mut self, node: u8, ph: u8, percent: u8) -> (u8, u64) {
        // A node without an entry is counted with the fallback's trial.
        let trial = Some(self.timings_of(node).trial_ms);
        let now = self.clock.now_ms();
        let shown = self
            .write
            .report(node, ph, percent, self.core_write, now, trial);
        (shown, now)
    }

    /// One SHOW, never resent.
    async fn show_to(&mut self, to: Target, show: &Show<'_>) -> Result<(), T::Error> {
        let mut p = [0u8; Show::MAX_LEN];
        let n = show.encode(&mut p).map_err(|_| Error::BadResponse)?;
        self.call_once(to, Cmd::Show, &p[..n], REQUEST_MS)
            .await
            .map(|_| ())
    }

    /// Tells the node being written its one number when it moved, for its
    /// app, the connected node and a linked one alike. Telling must not cost a write: one try per step. An
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

/// No DAGON answers at the node: the relaying node relayed its first request (HELLO)
/// and answered that the node did not, after every resend, so nothing else
/// was sent to it. A relaying node that does not answer is no such evidence.
fn is_silent<E>(e: &Error<E>) -> bool {
    matches!(e, Error::Relay(Status::Timeout))
}

fn reboot_request() -> Vec<u8> {
    request(|b| payload::encode_reboot(reboot_mode::NORMAL, b))
}

fn confirm_request() -> Vec<u8> {
    request(|b| payload::encode_confirm(target::APP, b))
}

/// XFER data bytes per chunk for a peer announcing `max_payload` on a link
/// carrying `link_max`; `None` when not even one byte fits.
fn chunk_len(max_payload: u16, link_max: usize) -> Option<usize> {
    (max_payload as usize)
        .min(link_max)
        .checked_sub(CHUNK_HEAD)
        .filter(|&c| c > 0)
}

/// The slot rollback returns to: another of the node's app `slots` than the
/// running one, Confirmed (we run on trial) or Written (confirmed before the
/// running slot took over); never a Reverted, pending or empty one, and
/// never a STATUS entry outside `slots`. A node with one slot has none.
fn rollback_slot(st: &StatusResp, slots: &[u8]) -> Option<u8> {
    if !slots.contains(&st.active) {
        return None;
    }
    slots
        .iter()
        .copied()
        .filter(|&s| s != st.active)
        .find(|&s| {
            st.slots
                .get(usize::from(s))
                .is_some_and(|i| matches!(i.state, SlotState::Confirmed | SlotState::Written))
        })
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
        b.verify(&Ed25519Compact::new(*k))?;
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
/// `roms` has a map for its MCU, and a ROM I2C link joins it to a relay
/// built for its loader's address: the address of that relay's ROM
/// recovery peer ([`TopologyView::rom_peer`], the relay's choice too). A
/// relay with several ROM links reaches every peer whose loader answers at
/// that one address.
fn rom_map<'m>(topo: &TopologyView<'_>, node: u8, roms: &'m [RomMap]) -> Option<&'m RomMap> {
    if (node as usize) >= topo.node_count() {
        return None;
    }
    let map_of = |n: u8| {
        let mcu = topo.node(usize::from(n)).mcu_id;
        roms.iter().find(|m| m.mcu_id == mcu)
    };
    let map = map_of(node)?;
    let has_loader = |w: McuId| roms.iter().any(|m| m.mcu_id == w);
    let linked = |r: u8| {
        (0..topo.link_count())
            .map(|i| topo.link(i))
            .filter(|l| l.reaches_rom_i2c())
            .any(|l| l.other(r).is_some_and(|e| e.node == node))
    };
    let relayed = (0..topo.node_count())
        .filter_map(|r| u8::try_from(r).ok())
        .any(|r| {
            linked(r)
                && topo
                    .rom_peer(r, has_loader)
                    .and_then(map_of)
                    .is_some_and(|p| p.addr == map.addr)
        });
    relayed.then_some(map)
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

/// Every text [`reason`] gives. Tools that tell a reason in their own
/// words (the page) read this list, so a reason added to [`reason`] fails
/// their checks until it has its words.
pub const REASONS: &[&str] = &[
    "E_SIGNATURE",
    "E_KEY",
    "E_COMPAT",
    "E_DIGEST",
    "E_FLASH",
    "E_STATE",
    "E_RANGE",
    "REJECTED",
    "NO ANSWER",
    "WIRE VERSION",
    "ROLLED BACK",
    "BOOT CORE ROLLED BACK",
    "NO BOOT CORE TRIAL",
    "APP ON TRIAL",
    "NOT HANDED OVER",
    "NO IMAGE",
    "ROM NOT CONFIRMED",
    "FAILED",
];

/// Short text naming why an update failed (ASCII, at most 32 bytes).
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
        // It answered, in another wire version: not silence.
        Error::WireVersion { .. } => "WIRE VERSION",
        Error::WrongVersion { .. } => "ROLLED BACK",
        Error::BootCoreRolledBack { .. } => "BOOT CORE ROLLED BACK",
        Error::BootCoreRefused => "NO BOOT CORE TRIAL",
        Error::AppOnTrial { .. } => "APP ON TRIAL",
        Error::NotHandedOver { .. } => "NOT HANDED OVER",
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

/// What [`Host::discover`] asks over `links`: each target and its wait.
fn probes(links: &[dagon_port::RelayLinkDef], rom_addrs: &[u8]) -> Vec<(Target, u32)> {
    let mut out = Vec::new();
    for d in links {
        let to = |addr| Target::Linked {
            link: d.id,
            addr,
            max_payload: d.max_payload,
        };
        match d.addressing {
            Addressing::SevenBit => out.extend(
                I2C_SCAN
                    .filter(|a| !rom_addrs.contains(a))
                    .map(|a| (to(a), DISCOVER_MS)),
            ),
            Addressing::Fixed(a) => out.push((to(a), REQUEST_MS)),
            // A way of addressing this version does not know: nothing to ask.
            _ => {}
        }
    }
    out
}

/// Nodes with an APP or boot core image in the release, filtered by
/// `only`: the others by update order, then `local`, the node the PC is
/// connected to (which relays to the others).
fn plan(b: &BundleView<'_>, topo: &TopologyView<'_>, only: Option<&[u8]>, local: u8) -> Vec<u8> {
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
    nodes.sort_by_key(|&n| (n == local, topo.node(n as usize).order_rank, n));
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
        && slots_settled(&st.status)
        && st
            .status
            .slots
            .get(usize::from(active))
            .is_some_and(|s| app_digest(b, node, active) == Some(s.digest16))
}

/// The running slot is the confirmed one and no slot waits for the next boot
/// or is on trial: what runs now is what the next reset runs.
fn slots_settled(st: &StatusResp) -> bool {
    st.active == st.confirmed
        && st
            .slots
            .get(usize::from(st.active))
            .is_some_and(|s| s.state == SlotState::Confirmed)
        && !st
            .slots
            .iter()
            .any(|s| matches!(s.state, SlotState::Pending | SlotState::Trying))
}

/// Whether the node's app runs from `slot`, so only its boot core may
/// write it (a node with one slot).
fn runs_from(st: &NodeState, slot: u8) -> bool {
    st.hello.mode == payload::mode::APP && st.status.active == slot
}

/// Of the node's `slots`, the one to write: one neither running nor
/// confirmed, or the only one when the node has one slot (an image other
/// than the one in it writes it).
fn free_slot(slots: &[u8], st: &StatusResp) -> Option<u8> {
    match slots {
        [only] => Some(*only),
        _ => slots
            .iter()
            .copied()
            .find(|&s| s != st.active && s != st.confirmed),
    }
}

/// The slots of `kind` the release has images for on `node`: each slot an
/// image is bound to, both image slots for an image bound to any slot.
fn slots_of(b: &BundleView<'_>, node: u8, kind: u8) -> Vec<u8> {
    let m = b.manifest();
    let mut slots: Vec<u8> = (0..m.entry_count())
        .map(|i| m.entry(i))
        .filter(|e| e.node_index == node && e.image_kind == kind)
        .flat_map(|e| match e.slot_binding {
            slot_binding::ANY => (0..StatusResp::IMAGE_SLOTS as u8).collect(),
            s => alloc::vec![s],
        })
        .collect();
    slots.sort_unstable();
    slots.dedup();
    slots
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

/// The app version a node keeps: the one it runs, or its confirmed one
/// while it runs on trial (a version on trial is taken back unless it is
/// confirmed).
fn kept(st: &NodeState) -> Version {
    match st.status.slots.get(usize::from(st.status.confirmed)) {
        Some(slot) if st.hello.trial => slot.version,
        _ => st.hello.active_version,
    }
}

/// The newest app version the release gives `node`.
fn app_version(b: &BundleView<'_>, node: u8) -> Option<Version> {
    let m = b.manifest();
    (0..m.entry_count())
        .map(|i| m.entry(i))
        .filter(|e| e.node_index == node && e.image_kind == image_kind::APP)
        .map(|e| e.version)
        .max()
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
        assert_eq!(chunk_len(5, 256), None);
        assert_eq!(chunk_len(6, 256), Some(1));
        assert_eq!(chunk_len(1088, 256), Some(251), "link limit wins");
    }

    /// The slot written is one the release has an image for: the free one
    /// of two, the only one of one (another image than the one running
    /// there writes it), none while every slot is taken.
    #[test]
    fn the_slot_written_is_one_of_the_nodes_slots() {
        let st = |active, confirmed| StatusResp {
            active,
            confirmed,
            ..Default::default()
        };
        assert_eq!(free_slot(&[0, 1], &st(0, 0)), Some(1));
        assert_eq!(free_slot(&[0, 1], &st(1, 1)), Some(0));
        assert_eq!(free_slot(&[0, 1], &st(1, 0)), None, "on trial in B");
        assert_eq!(free_slot(&[0], &st(0, 0)), Some(0));
        assert_eq!(free_slot(&[0], &st(2, 0)), Some(0), "from the factory slot");
        assert_eq!(free_slot(&[], &st(0, 0)), None);
    }

    /// One slot holds the only image: no previous version to go back to.
    #[test]
    fn a_one_slot_node_has_no_previous_version() {
        let mut st = StatusResp {
            active: 0,
            confirmed: 0,
            ..Default::default()
        };
        st.slots[0].state = SlotState::Confirmed;
        assert_eq!(rollback_slot(&st, &[0]), None);
        // A non-empty entry for a slot the node does not have is never taken.
        st.slots[1].state = SlotState::Written;
        assert_eq!(rollback_slot(&st, &[0]), None);
        // Nor is a running slot outside them.
        st.active = 1;
        assert_eq!(rollback_slot(&st, &[0]), None);
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
            rollback_slot(&st, &[0, 1])
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
