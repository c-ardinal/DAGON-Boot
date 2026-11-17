//! Time limits of the protocol, the per-link limits they depend on, and
//! each node's own limits ([`NodeTimings`]), which come from its MCU
//! definition: how long its erase may take and how long its trial lasts.
//!
//! | Stage | Limit |
//! |---|---|
//! | plain request (HELLO, STATUS, XFER_BEGIN, XFER_DATA, non-final XFER_HDR, REBOOT) | [`REQUEST_MS`] |
//! | the request the node erases on (final XFER_HDR, or XFER_BEGIN without header) | [`NodeTimings::erase_limit_ms`]: the planned erase ([`plan_erase`]) + [`ERASE_MARGIN_MS`]; [`ERASE_FALLBACK_MS`] without an erase layout |
//! | XFER_END, SET_PENDING, CONFIRM (verify or write state) | [`VERIFY_MS`] |
//! | CONF_RUN (one node-side test) | [`CONF_RUN_MS`] |
//! | whole image transfer | [`transfer_ms`]: size / link floor ([`link_limits`]) + [`TRANSFER_MARGIN_MS`] |
//! | REBOOT until the expected HELLO (also covers a full rollback) | [`REBOOT_WAIT_MS`] |
//!
//! The node and the host plan an erase with the same two functions
//! ([`erase_ranges`], [`plan_erase`]) on the same [`EraseLayout`], so the
//! erase a node performs and the wait a host allows for it cannot drift.

use crate::LinkKind;

pub const REQUEST_MS: u32 = 300;
pub const VERIFY_MS: u32 = 5_000;
pub const CONF_RUN_MS: u32 = 30_000;
pub const REBOOT_WAIT_MS: u32 = 30_000;
pub const TRANSFER_MARGIN_MS: u32 = 30_000;
/// Lowest accepted effective transfer speed on I2C, bytes per second.
pub const I2C_FLOOR_BPS: u32 = 10_000;
/// Lowest accepted effective transfer speed on UART (and direct USB), bytes
/// per second.
pub const UART_FLOOR_BPS: u32 = 5_000;
/// Pause between HELLO polls while waiting for a node to come back.
pub const POLL_MS: u32 = 500;
/// The host's pause between a node's HELLO polls after REBOOT, so CONFIRM
/// lands well inside one trial (one poll costs at most two [`REQUEST_MS`]
/// round trips: RELAY's wait and the node's).
pub const NODE_POLL_MS: u32 = 300;
/// Pauses before CONFIRM is sent again after E_STATE: the image runs but
/// its self-check does not pass yet (its tasks are still starting). Together
/// they stay inside one trial ([`NodeTimings::check`]); an image whose
/// self-check never passes is still reset by its trial watchdog and rolls
/// back. A target that confirms at once never waits.
pub const CONFIRM_RETRY_MS: [u32; 5] = [100, 200, 400, 600, 800];
/// Erase limit of a node whose definition gives no erase layout.
pub const ERASE_FALLBACK_MS: u32 = 30_000;
/// Added to a planned erase's worst case: the request's own round trip.
pub const ERASE_MARGIN_MS: u32 = 1_000;

/// What the run judges a link kind by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkLimits {
    /// The payload limit an agent on the link announces in HELLO.
    pub max_payload: usize,
    /// Lowest accepted effective transfer speed, bytes per second.
    pub floor_bps: u32,
}

/// The limits of `link`; `None` for a kind there are no limits for, which
/// a caller refuses rather than judge it by another kind's.
pub const fn link_limits(link: LinkKind) -> Option<LinkLimits> {
    use dagon_proto::max_payload;
    let (max_payload, floor_bps) = match link {
        LinkKind::I2c => (max_payload::I2C, I2C_FLOOR_BPS),
        LinkKind::Uart => (max_payload::UART, UART_FLOOR_BPS),
        LinkKind::UsbCdc => (max_payload::USB, UART_FLOOR_BPS),
        _ => return None,
    };
    Some(LinkLimits {
        max_payload,
        floor_bps,
    })
}

/// Time allowed for transferring an image of `len` bytes (header included)
/// over a link with `limits`.
pub const fn transfer_ms(len: u32, limits: LinkLimits) -> u32 {
    (len as u64 * 1000 / limits.floor_bps as u64) as u32 + TRANSFER_MARGIN_MS
}

/// Flash erased in `unit`-byte operations from `start` for `len` bytes,
/// each taking at most `unit_max_ms` (`[execution] erase_regions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EraseRegion {
    pub start: u32,
    pub len: u32,
    pub unit: u32,
    pub unit_max_ms: u32,
}

impl EraseRegion {
    /// Regions from generated `(start, len, unit, unit_max_ms)` rows (a
    /// board's or a node's generated erase regions), in a constant.
    pub const fn table<const N: usize>(rows: [(u32, u32, u32, u32); N]) -> [EraseRegion; N] {
        let mut out = [EraseRegion {
            start: 0,
            len: 0,
            unit: 0,
            unit_max_ms: 0,
        }; N];
        let mut i = 0;
        while i < N {
            let (start, len, unit, unit_max_ms) = rows[i];
            out[i] = EraseRegion {
                start,
                len,
                unit,
                unit_max_ms,
            };
            i += 1;
        }
        out
    }
}

/// Where a slot keeps its image header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderPlace {
    /// At the slot start, the body after it.
    Inline,
    /// At `at`, outside the slot; the body from the slot start.
    Detached { at: u32 },
}

/// One XFER target and slot a node receives into (`[[slot_layout]]`):
/// where its bytes start, the largest body (header excluded) and where its
/// header goes. `slot` is the wire slot (0 = A, 1 = B, 2 = factory).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotLayout {
    pub target: u8,
    pub slot: u8,
    pub start: u32,
    pub capacity: u32,
    pub header: HeaderPlace,
}

/// A node's erase geometry and the slots it receives into: everything an
/// erase is planned from. `R` and `S` hold the lists (static tables on a
/// node, vectors on a host).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EraseLayout<R = &'static [EraseRegion], S = &'static [SlotLayout]> {
    pub regions: R,
    pub slots: S,
}

/// One erase operation: `len` bytes from `addr`, at most `max_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EraseOp {
    pub addr: u32,
    pub len: u32,
    pub max_ms: u32,
}

/// `len` bytes from `start` that a transfer erases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u32,
    pub len: u32,
}

/// Why no erase can be planned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// The layout has no slot for this target and slot.
    NoSlot,
    /// The body is larger than the slot's capacity.
    TooLong,
    /// No erase region holds this address.
    Uncovered { addr: u32 },
}

/// The byte ranges a transfer of `hdr_len` header and `body_len` body bytes
/// into `slot` of `target` erases: the slot from its start through the
/// body's last byte (the header first when inline), and a detached header's
/// own bytes.
pub fn erase_ranges(
    slots: &[SlotLayout],
    target: u8,
    slot: u8,
    hdr_len: u32,
    body_len: u32,
) -> Result<[Option<ByteRange>; 2], PlanError> {
    let s = slots
        .iter()
        .find(|s| s.target == target && s.slot == slot)
        .ok_or(PlanError::NoSlot)?;
    if body_len > s.capacity {
        return Err(PlanError::TooLong);
    }
    let range = |start: u32, len: u64| {
        (len > 0).then_some(ByteRange {
            start,
            len: u32::try_from(len).unwrap_or(u32::MAX),
        })
    };
    Ok(match s.header {
        HeaderPlace::Inline => [
            range(s.start, u64::from(hdr_len) + u64::from(body_len)),
            None,
        ],
        HeaderPlace::Detached { at } => {
            [range(at, hdr_len.into()), range(s.start, body_len.into())]
        }
    })
}

/// The erase operations covering `ranges`, in order: each whole erase unit
/// of the region holding the address, aligned to the region's start.
/// Fails when an address of a range lies in no region.
pub fn plan_erase(
    regions: &[EraseRegion],
    ranges: [Option<ByteRange>; 2],
) -> Result<Plan<'_>, PlanError> {
    let plan = Plan {
        regions,
        ranges,
        range: 0,
        at: ranges[0].map_or(0, |r| r.start.into()),
    };
    let mut check = plan.clone();
    while let Some(op) = check.step() {
        op?;
    }
    Ok(plan)
}

/// The erase limit of `plan`: the sum of its operations' worst cases and
/// [`ERASE_MARGIN_MS`], saturating at `u32::MAX`.
pub fn erase_limit_ms(plan: Plan<'_>) -> u32 {
    let ms = plan.fold(u64::from(ERASE_MARGIN_MS), |t, op| t + u64::from(op.max_ms));
    u32::try_from(ms).unwrap_or(u32::MAX)
}

/// The planned erase operations ([`plan_erase`]), in order.
#[derive(Clone, Debug)]
pub struct Plan<'r> {
    regions: &'r [EraseRegion],
    ranges: [Option<ByteRange>; 2],
    range: usize,
    at: u64,
}

impl Plan<'_> {
    fn step(&mut self) -> Option<Result<EraseOp, PlanError>> {
        let end = |r: ByteRange| u64::from(r.start) + u64::from(r.len);
        while !self
            .ranges
            .get(self.range)?
            .is_some_and(|r| self.at < end(r))
        {
            self.range += 1;
            self.at = self
                .ranges
                .get(self.range)
                .copied()
                .flatten()
                .map_or(0, |r| r.start.into());
        }
        {
            let at = self.at;
            let Some(g) = self
                .regions
                .iter()
                .find(|g| at >= u64::from(g.start) && at < u64::from(g.start) + u64::from(g.len))
            else {
                self.ranges = [None, None];
                return Some(Err(PlanError::Uncovered { addr: at as u32 }));
            };
            let unit = u64::from(g.unit.max(1));
            let addr = u64::from(g.start) + (at - u64::from(g.start)) / unit * unit;
            self.at = addr + unit;
            Some(Ok(EraseOp {
                addr: addr as u32,
                len: g.unit,
                max_ms: g.unit_max_ms,
            }))
        }
    }
}

impl Iterator for Plan<'_> {
    type Item = EraseOp;

    fn next(&mut self) -> Option<EraseOp> {
        self.step()?.ok()
    }
}

impl<R: AsRef<[EraseRegion]>, S: AsRef<[SlotLayout]>> EraseLayout<R, S> {
    /// The erase of a transfer of `hdr_len` + `body_len` bytes into `slot`
    /// of `target`: [`erase_ranges`] then [`plan_erase`].
    pub fn plan(
        &self,
        target: u8,
        slot: u8,
        hdr_len: u32,
        body_len: u32,
    ) -> Result<Plan<'_>, PlanError> {
        let ranges = erase_ranges(self.slots.as_ref(), target, slot, hdr_len, body_len)?;
        plan_erase(self.regions.as_ref(), ranges)
    }

    /// The same layout, borrowed.
    pub fn borrowed(&self) -> EraseLayout<&[EraseRegion], &[SlotLayout]> {
        EraseLayout {
            regions: self.regions.as_ref(),
            slots: self.slots.as_ref(),
        }
    }
}

/// A node's own time limits, from its MCU definition: its erase layout
/// (`[execution] erase_regions` and `[[slot_layout]]`), its trial period
/// (`[watchdog] trial_ms`) and the boots a pending image gets
/// (`[watchdog] trial_attempts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeTimings<L = EraseLayout> {
    pub erase: Option<L>,
    pub trial_ms: u32,
    pub trial_attempts: u8,
}

/// The [`NodeTimings::check`] rule a node's timings break.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingError {
    /// `NODE_POLL_MS + 2 * REQUEST_MS < trial_ms`: a poll pause plus a
    /// missed poll must fit one trial.
    PollMissesTrial,
    /// `sum(CONFIRM_RETRY_MS) + NODE_POLL_MS + 2 * REQUEST_MS < trial_ms`:
    /// the CONFIRM retries must fit one trial.
    ConfirmMissesTrial,
    /// `REBOOT_WAIT_MS >= trial_attempts * (trial_ms + 1000) + 2000`: the
    /// wait for the new HELLO must cover a full local rollback.
    RebootWaitMissesRollback,
}

impl core::fmt::Display for TimingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::PollMissesTrial => {
                "NODE_POLL_MS + 2 x REQUEST_MS must be below watchdog.trial_ms"
            }
            Self::ConfirmMissesTrial => {
                "the CONFIRM retries + NODE_POLL_MS + 2 x REQUEST_MS must be below watchdog.trial_ms"
            }
            Self::RebootWaitMissesRollback => {
                "REBOOT_WAIT_MS must cover watchdog.trial_attempts x (watchdog.trial_ms + 1000) + 2000"
            }
        })
    }
}

impl<L> NodeTimings<L> {
    /// A node no definition describes: the erase limit is
    /// [`ERASE_FALLBACK_MS`]; the trial is the longest and the attempts the
    /// most of the built-in MCU definitions.
    pub const FALLBACK: Self = NodeTimings {
        erase: None,
        trial_ms: 8_000,
        trial_attempts: 3,
    };

    /// Whether the host's fixed waits fit this node's trial: each rule in
    /// `u64`, an overflow failing the rule it occurs in.
    pub fn check(&self) -> Result<(), TimingError> {
        let trial = u64::from(self.trial_ms);
        let poll = u64::from(NODE_POLL_MS) + 2 * u64::from(REQUEST_MS);
        if poll >= trial {
            return Err(TimingError::PollMissesTrial);
        }
        let retries: u64 = CONFIRM_RETRY_MS.iter().map(|&ms| u64::from(ms)).sum();
        if retries + poll >= trial {
            return Err(TimingError::ConfirmMissesTrial);
        }
        let rollback = trial
            .checked_add(1_000)
            .and_then(|t| t.checked_mul(self.trial_attempts.into()))
            .and_then(|t| t.checked_add(2_000));
        match rollback {
            Some(ms) if u64::from(REBOOT_WAIT_MS) >= ms => Ok(()),
            _ => Err(TimingError::RebootWaitMissesRollback),
        }
    }
}

impl<R: AsRef<[EraseRegion]>, S: AsRef<[SlotLayout]>> NodeTimings<EraseLayout<R, S>> {
    /// The limit for the request a transfer of `hdr_len` + `body_len` bytes
    /// into `slot` of `target` erases on. Without a layout, or for a slot it
    /// has no plan for (which the node refuses anyway),
    /// [`ERASE_FALLBACK_MS`].
    pub fn erase_limit_ms(&self, target: u8, slot: u8, hdr_len: u32, body_len: u32) -> u32 {
        self.erase
            .as_ref()
            .and_then(|l| l.plan(target, slot, hdr_len, body_len).ok())
            .map_or(ERASE_FALLBACK_MS, erase_limit_ms)
    }

    /// The same timings, borrowed.
    pub fn borrowed(&self) -> NodeTimings<EraseLayout<&[EraseRegion], &[SlotLayout]>> {
        NodeTimings {
            erase: self.erase.as_ref().map(EraseLayout::borrowed),
            trial_ms: self.trial_ms,
            trial_attempts: self.trial_attempts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_limit_follows_link_floor() {
        let limits = |k| link_limits(k).unwrap();
        assert_eq!(
            transfer_ms(64 * 1024 + 512, limits(LinkKind::I2c)),
            6_604 + 30_000
        );
        assert_eq!(transfer_ms(10_000, limits(LinkKind::Uart)), 32_000);
    }

    const REGIONS: [EraseRegion; 2] = [
        EraseRegion {
            start: 0x1000,
            len: 0x4000,
            unit: 0x1000,
            unit_max_ms: 100,
        },
        EraseRegion {
            start: 0x5000,
            len: 0x2_0000,
            unit: 0x1_0000,
            unit_max_ms: 900,
        },
    ];
    const SLOTS: [SlotLayout; 2] = [
        SlotLayout {
            target: 1,
            slot: 0,
            start: 0x3000,
            capacity: 0x1_0000,
            header: HeaderPlace::Inline,
        },
        SlotLayout {
            target: 1,
            slot: 1,
            start: 0x1_6000,
            capacity: 0x1_0000,
            header: HeaderPlace::Detached { at: 0x1000 },
        },
    ];
    const LAYOUT: EraseLayout = EraseLayout {
        regions: &REGIONS,
        slots: &SLOTS,
    };

    fn ops(p: Plan<'_>) -> [(u32, u32); 4] {
        let mut out = [(0, 0); 4];
        for (o, op) in out.iter_mut().zip(p) {
            *o = (op.addr, op.len);
        }
        out
    }

    #[test]
    fn a_range_across_regions_takes_each_regions_unit() {
        // 0x3000 + 512 + 0x2000: two small units, then the large unit
        // holding 0x5000.
        let p = LAYOUT.plan(1, 0, 512, 0x2200).unwrap();
        assert_eq!(
            ops(p.clone()),
            [
                (0x3000, 0x1000),
                (0x4000, 0x1000),
                (0x5000, 0x1_0000),
                (0, 0)
            ]
        );
        assert_eq!(erase_limit_ms(p), 100 + 100 + 900 + ERASE_MARGIN_MS);
    }

    #[test]
    fn a_detached_header_is_erased_apart_from_the_body() {
        let p = LAYOUT.plan(1, 1, 512, 10).unwrap();
        assert_eq!(
            ops(p),
            [(0x1000, 0x1000), (0x1_5000, 0x1_0000), (0, 0), (0, 0)]
        );
    }

    #[test]
    fn an_unplannable_transfer_is_refused() {
        assert_eq!(LAYOUT.plan(1, 2, 512, 10).err(), Some(PlanError::NoSlot));
        assert_eq!(
            LAYOUT.plan(1, 0, 512, 0x1_0001).err(),
            Some(PlanError::TooLong)
        );
        // Slot B's body runs past the last region.
        assert_eq!(
            LAYOUT.plan(1, 1, 512, 0x1_0000).err(),
            Some(PlanError::Uncovered { addr: 0x2_5000 })
        );
    }

    /// `Plan::step` as written with 64-bit offsets and division: the
    /// definition its 32-bit form must reproduce bit for bit.
    fn step_wide(
        regions: &[EraseRegion],
        ranges: &mut [Option<ByteRange>; 2],
        range: &mut usize,
        at: &mut u64,
    ) -> Option<Result<EraseOp, PlanError>> {
        let end = |r: ByteRange| u64::from(r.start) + u64::from(r.len);
        while !ranges.get(*range)?.is_some_and(|r| *at < end(r)) {
            *range += 1;
            *at = ranges
                .get(*range)
                .copied()
                .flatten()
                .map_or(0, |r| r.start.into());
        }
        let a = *at;
        let Some(g) = regions
            .iter()
            .find(|g| a >= u64::from(g.start) && a < u64::from(g.start) + u64::from(g.len))
        else {
            *ranges = [None, None];
            return Some(Err(PlanError::Uncovered { addr: a as u32 }));
        };
        let unit = u64::from(g.unit.max(1));
        let addr = u64::from(g.start) + (a - u64::from(g.start)) / unit * unit;
        *at = addr + unit;
        Some(Ok(EraseOp {
            addr: addr as u32,
            len: g.unit,
            max_ms: g.unit_max_ms,
        }))
    }

    /// At the ends of the 32-bit address space, with units of 0, 1 and
    /// `u32::MAX` and ranges of the largest length, every step equals
    /// the 64-bit definition's.
    #[test]
    fn plan_steps_match_the_wide_definition_at_the_range_limits() {
        const TOP: u32 = u32::MAX;
        let region = |start, len, unit| EraseRegion {
            start,
            len,
            unit,
            unit_max_ms: 7,
        };
        let layouts: [&[EraseRegion]; 6] = [
            &[region(0, TOP, 0x1000)],
            &[region(0, TOP, 0)],
            &[region(0, TOP, TOP)],
            &[region(TOP - 0x2fff, 0x3000, 0x1000)],
            &[region(TOP - 0xfff, TOP, 0x1000), region(0, 0x1000, 3)],
            &[region(0x8000_0000, 0x8000_0000, 0x3)],
        ];
        let range = |start, len| Some(ByteRange { start, len });
        let ranges = [
            [range(0, TOP), None],
            [range(TOP - 0x1fff, 0x2000), range(0, 5)],
            [range(TOP, 1), None],
            [range(TOP - 2, TOP), range(TOP - 1, TOP)],
            [None, range(0x8000_0001, 0x10)],
            [range(0x7fff_ffff, 2), None],
            [None, None],
        ];
        for regions in layouts {
            for r in ranges {
                let mut plan = Plan {
                    regions,
                    ranges: r,
                    range: 0,
                    at: r[0].map_or(0, |r| r.start.into()),
                };
                let (mut wr, mut wi, mut wa) = (r, 0, plan.at);
                for n in 0..64 {
                    let wide = step_wide(regions, &mut wr, &mut wi, &mut wa);
                    assert_eq!(plan.step(), wide, "{regions:?} {r:?} step {n}");
                    if wide.is_none() {
                        break;
                    }
                }
            }
        }
    }

    #[test]
    fn the_limit_saturates() {
        let regions = [EraseRegion {
            start: 0,
            len: u32::MAX,
            unit: 1,
            unit_max_ms: u32::MAX,
        }];
        let range = Some(ByteRange { start: 0, len: 3 });
        let p = plan_erase(&regions, [range, None]).unwrap();
        assert_eq!(erase_limit_ms(p), u32::MAX);
    }

    #[test]
    fn without_a_layout_the_erase_limit_is_the_fallback() {
        let t: NodeTimings = NodeTimings::FALLBACK;
        assert_eq!(t.erase_limit_ms(1, 0, 512, 10), ERASE_FALLBACK_MS);
        let t = NodeTimings {
            erase: Some(LAYOUT),
            ..t
        };
        assert_eq!(t.erase_limit_ms(1, 0, 512, 10), 100 + ERASE_MARGIN_MS);
        assert_eq!(t.erase_limit_ms(9, 0, 512, 10), ERASE_FALLBACK_MS);
    }

    fn timings(trial_ms: u32, trial_attempts: u8) -> NodeTimings<()> {
        NodeTimings {
            erase: None,
            trial_ms,
            trial_attempts,
        }
    }

    #[test]
    fn the_fallback_passes_the_check() {
        assert_eq!(NodeTimings::<()>::FALLBACK.check(), Ok(()));
    }

    #[test]
    fn each_rule_holds_at_its_limit() {
        let poll = NODE_POLL_MS + 2 * REQUEST_MS;
        let retries: u32 = CONFIRM_RETRY_MS.iter().sum();
        assert_eq!(timings(poll, 1).check(), Err(TimingError::PollMissesTrial));
        assert_eq!(
            timings(poll + 1, 1).check(),
            Err(TimingError::ConfirmMissesTrial)
        );
        assert_eq!(
            timings(retries + poll, 1).check(),
            Err(TimingError::ConfirmMissesTrial)
        );
        assert_eq!(timings(retries + poll + 1, 1).check(), Ok(()));
        // attempts x (trial + 1000) + 2000 = REBOOT_WAIT_MS exactly.
        let trial = (REBOOT_WAIT_MS - 2_000) / 4 - 1_000;
        assert_eq!(timings(trial, 4).check(), Ok(()));
        assert_eq!(
            timings(trial + 1, 4).check(),
            Err(TimingError::RebootWaitMissesRollback)
        );
        assert_eq!(
            timings(trial, 5).check(),
            Err(TimingError::RebootWaitMissesRollback)
        );
    }

    #[test]
    fn the_largest_values_a_definition_accepts_fail_without_overflow() {
        assert_eq!(
            timings(u32::MAX, u8::MAX).check(),
            Err(TimingError::RebootWaitMissesRollback)
        );
        assert_eq!(
            timings(u32::MAX, 1).check(),
            Err(TimingError::RebootWaitMissesRollback)
        );
        assert_eq!(
            timings(8_000, u8::MAX).check(),
            Err(TimingError::RebootWaitMissesRollback)
        );
    }
}
