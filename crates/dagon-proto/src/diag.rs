//! Typed payloads of the link diagnostics a node keeps of the peers it
//! starts exchanges with: LINK_DIAG (after UDS ReadDTCInformation, 0x19),
//! LINK_DIAG_CLEAR (after ClearDiagnosticInformation, 0x14) and DIAG_SEED
//! (after SecurityAccess, 0x27). The service ideas are UDS's; the bytes
//! are the field grammar of [`crate::field`] and the rows of
//! [`crate::table`], not UDS's. A DTC, its statusOfDTC byte,
//! statusAvailabilityMask and the fault detection counter keep their UDS
//! encodings.

use crate::field::{FieldSpec, FieldWriter, Parsed};
use crate::payload::{at, byte, need, rd, val, PolicySet};
use crate::table::{diag_seed, dtc, link_diag, link_diag_clear};
use crate::{Cmd, FrameError, Status};

use link_diag::{report, store};
use link_diag_clear::scope;

/// One end of a link the node starts exchanges over: the link's id
/// (RELAY's `link`) and the peer's address on it (RELAY's `addr`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Endpoint {
    pub link: u8,
    pub addr: u8,
}

/// The key of an element: what CURSOR and NEXT carry, and the order the
/// elements of a report come in (bytes compared from the first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    bytes: [u8; link_diag::ENTRY_KEY_LEN as usize],
    len: u8,
}

impl Key {
    /// The key in `b`: [`link_diag::ENTRY_KEY_LEN`] or
    /// [`link_diag::PEER_KEY_LEN`] bytes.
    pub fn new(b: &[u8]) -> Option<Key> {
        let len = u8::try_from(b.len()).ok()?;
        if len != link_diag::ENTRY_KEY_LEN && len != link_diag::PEER_KEY_LEN {
            return None;
        }
        let mut bytes = [0; link_diag::ENTRY_KEY_LEN as usize];
        bytes.get_mut(..b.len())?.copy_from_slice(b);
        Some(Key { bytes, len })
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or(&[])
    }
}

/// A DTC as UDS writes it: 3 bytes, high byte first.
pub const fn dtc_bytes(dtc: u32) -> [u8; 3] {
    let b = dtc.to_be_bytes();
    [b[1], b[2], b[3]]
}

/// The DTC of 3 bytes, high byte first; `Status::Arg` for one that names
/// no single DTC (the reserved number, or the group of all).
fn one_dtc([a, b, c]: [u8; 3]) -> Result<u32, Status> {
    let v = u32::from_be_bytes([0, a, b, c]);
    if v == dtc::RESERVED || v == dtc::ALL {
        return Err(Status::Arg);
    }
    Ok(v)
}

/// One element of report 1: a DTC of one endpoint and its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub dtc: u32,
    pub link: u8,
    pub addr: u8,
    /// statusOfDTC (UDS bit meanings).
    pub status: u8,
    /// Fault detection counter, -128 (passed) to +127 (failed).
    pub fdc: i8,
}

impl Entry {
    pub const LEN: usize = link_diag::ENTRY_LEN as usize;

    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let [a, b, c] = dtc_bytes(self.dtc);
        let [fdc] = self.fdc.to_le_bytes();
        [a, b, c, self.link, self.addr, self.status, fdc]
    }

    pub fn from_bytes(b: [u8; Self::LEN]) -> Self {
        let [a, b0, c, link, addr, status, fdc] = b;
        Entry {
            dtc: u32::from_be_bytes([0, a, b0, c]),
            link,
            addr,
            status,
            fdc: i8::from_le_bytes([fdc]),
        }
    }

    /// DTC, link, addr.
    pub fn key(&self) -> Key {
        let [a, b, c] = dtc_bytes(self.dtc);
        Key {
            bytes: [a, b, c, self.link, self.addr],
            len: link_diag::ENTRY_KEY_LEN,
        }
    }

    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            link: self.link,
            addr: self.addr,
        }
    }
}

/// The instance identifier of a peer whose identifier is not known. No
/// identifier a peer is given is this one.
pub const NO_INSTANCE: [u8; 16] = [0; 16];

/// One element of report 2: a peer the node has communicated with since
/// its last clear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    pub link: u8,
    pub addr: u8,
    /// [`NO_INSTANCE`] when not known.
    pub instance: [u8; 16],
}

impl Peer {
    pub const LEN: usize = link_diag::PEER_LEN as usize;

    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut b = [0; Self::LEN];
        b[0] = self.link;
        b[1] = self.addr;
        b[2..].copy_from_slice(&self.instance);
        b
    }

    pub fn from_bytes(b: [u8; Self::LEN]) -> Self {
        let mut instance = NO_INSTANCE;
        instance.copy_from_slice(&b[2..]);
        Peer {
            link: b[0],
            addr: b[1],
            instance,
        }
    }

    /// Link, addr.
    pub fn key(&self) -> Key {
        Key {
            bytes: [self.link, self.addr, 0, 0, 0],
            len: link_diag::PEER_KEY_LEN,
        }
    }

    pub fn endpoint(&self) -> Endpoint {
        Endpoint {
            link: self.link,
            addr: self.addr,
        }
    }

    /// The instance identifier, when known.
    pub fn instance_id(&self) -> Option<[u8; 16]> {
        (self.instance != NO_INSTANCE).then_some(self.instance)
    }
}

/// What a LINK_DIAG request asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Report {
    /// Report 1: the DTCs whose statusOfDTC has a bit of `mask` (under
    /// AVAILABILITY), of one DTC when `dtc` is given.
    Dtcs { mask: u8, dtc: Option<u32> },
    /// Report 2: the peers the node has communicated with.
    Peers,
}

impl Report {
    pub const fn number(&self) -> u8 {
        match self {
            Report::Dtcs { .. } => report::DTC_BY_STATUS_MASK,
            Report::Peers => report::EVER_COMMUNICATED,
        }
    }

    /// Bytes of one element.
    pub const fn elem_len(&self) -> usize {
        match self {
            Report::Dtcs { .. } => Entry::LEN,
            Report::Peers => Peer::LEN,
        }
    }

    /// Bytes of an element's key.
    pub const fn key_len(&self) -> usize {
        match self {
            Report::Dtcs { .. } => link_diag::ENTRY_KEY_LEN as usize,
            Report::Peers => link_diag::PEER_KEY_LEN as usize,
        }
    }

    /// The response field of its elements.
    pub const fn field(&self) -> &'static FieldSpec {
        match self {
            Report::Dtcs { .. } => &link_diag::ENTRIES,
            Report::Peers => &link_diag::PEERS,
        }
    }

    /// [`min_resp`] of this report.
    pub const fn min_resp(&self) -> usize {
        min_resp(self.number())
    }
}

/// Bytes of a LEB128 value in its shortest form.
const fn shortest_len(v: usize) -> usize {
    if v < 1 << 7 {
        1
    } else if v < 1 << 14 {
        2
    } else if v < 1 << 21 {
        3
    } else {
        4
    }
}

/// Bytes of field `f` with a value of `len` bytes.
const fn field_len(f: &FieldSpec, len: usize) -> usize {
    shortest_len(f.num as usize * 2 + f.must as usize) + shortest_len(len) + len
}

/// Bytes of a response, status included, before its elements and NEXT:
/// the status, FORMAT and the four fixed fields at their longest.
const FIXED_MAX: usize = 1
    + 1
    + field_len(
        &link_diag::AVAILABILITY,
        link_diag::AVAILABILITY.max as usize,
    )
    + field_len(&link_diag::STORE, link_diag::STORE.max as usize)
    + field_len(&link_diag::POLICY, link_diag::POLICY.max as usize)
    + field_len(&link_diag::PARAMS, link_diag::PARAMS.max as usize);

/// The least MAX_RESP a request of `report` may name: the payload of a
/// response, status included, with the fixed fields at their longest, one
/// element and NEXT. 0 for a report this table does not have.
pub const fn min_resp(report: u8) -> usize {
    let r = match report {
        report::DTC_BY_STATUS_MASK => Report::Dtcs { mask: 0, dtc: None },
        report::EVER_COMMUNICATED => Report::Peers,
        _ => return 0,
    };
    FIXED_MAX + field_len(r.field(), r.elem_len()) + field_len(&link_diag::NEXT, r.key_len())
}

// The least answer of each report crosses one relay over links of the
// smallest payload: a node behind it can always make progress.
const _: () = {
    let smallest = crate::max_payload::SMALLEST;
    let inner = crate::payload::Relay::tunnel_mtu(smallest, smallest) - crate::frame_len(0);
    assert!(min_resp(report::DTC_BY_STATUS_MASK) <= inner);
    assert!(min_resp(report::EVER_COMMUNICATED) <= inner);
};

/// The endpoint of a message's LINK and ADDR: both or neither.
fn endpoint(
    p: &Parsed<'_>,
    link: &FieldSpec,
    addr: &FieldSpec,
) -> Result<Option<Endpoint>, Status> {
    match (val::<1>(p, link)?, val::<1>(p, addr)?) {
        (Some([link]), Some([addr])) => Ok(Some(Endpoint { link, addr })),
        (None, None) => Ok(None),
        _ => Err(Status::Arg),
    }
}

/// LINK_DIAG request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkDiagReq {
    pub report: Report,
    /// Only this endpoint's elements; `None`: every endpoint's.
    pub endpoint: Option<Endpoint>,
    /// Only elements above this key; `None`: from the first.
    pub cursor: Option<Key>,
    /// The most payload bytes of the response, status included.
    pub max_resp: u16,
}

impl LinkDiagReq {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        Self::of(&rd(b, &Cmd::LinkDiag.spec().req)?)
    }

    /// From a request the reader accepted: `Status::Unsupported` for a
    /// report this table does not have; `Status::Arg` for a field the
    /// report does not take or needs and lacks, LINK without ADDR, a
    /// CURSOR of another length than the report's keys, a DTC that names
    /// no single DTC; `Status::Range` for MAX_RESP below the report's
    /// [`min_resp`].
    pub fn of(p: &Parsed<'_>) -> Result<Self, Status> {
        let [number] = at(p.core, 0)?;
        let mask = val::<1>(p, &link_diag::STATUS_MASK)?;
        let dtc = val::<3>(p, &link_diag::DTC)?;
        let report = match number {
            report::DTC_BY_STATUS_MASK => Report::Dtcs {
                mask: mask.ok_or(Status::Arg)?[0],
                dtc: dtc.map(one_dtc).transpose()?,
            },
            report::EVER_COMMUNICATED if mask.is_none() && dtc.is_none() => Report::Peers,
            report::EVER_COMMUNICATED => return Err(Status::Arg),
            _ => return Err(Status::Unsupported),
        };
        let endpoint = endpoint(p, &link_diag::LINK, &link_diag::ADDR)?;
        let cursor = match p.field(&link_diag::CURSOR) {
            Some(c) if c.len() == report.key_len() => Some(Key::new(c).ok_or(Status::Arg)?),
            Some(_) => return Err(Status::Arg),
            None => None,
        };
        let max_resp = u16::from_le_bytes(need(p, &link_diag::MAX_RESP)?);
        if usize::from(max_resp) < report.min_resp() {
            return Err(Status::Range);
        }
        Ok(LinkDiagReq {
            report,
            endpoint,
            cursor,
            max_resp,
        })
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.report.number()])?;
        if let Report::Dtcs { mask, dtc } = self.report {
            w.field(&link_diag::STATUS_MASK, &[mask])?;
            if let Some(d) = dtc {
                w.field(&link_diag::DTC, &dtc_bytes(d))?;
            }
        }
        if let Some(e) = self.endpoint {
            w.field(&link_diag::LINK, &[e.link])?
                .field(&link_diag::ADDR, &[e.addr])?;
        }
        if let Some(c) = &self.cursor {
            w.field(&link_diag::CURSOR, c.as_bytes())?;
        }
        w.field(&link_diag::MAX_RESP, &self.max_resp.to_le_bytes())?;
        Ok(w.len())
    }

    /// Whether `e` is an element this request asks for, its key aside:
    /// for report 1, a DTC of the request's under `availability` with a
    /// status bit of the mask (UDS 0x19 0x02); the request's endpoint.
    pub fn wants_entry(&self, e: &Entry, availability: u8) -> bool {
        let Report::Dtcs { mask, dtc } = self.report else {
            return false;
        };
        dtc.is_none_or(|d| d == e.dtc)
            && e.status & mask & availability != 0
            && self.endpoint.is_none_or(|p| p == e.endpoint())
    }

    /// Whether `p` is an element this request asks for, its key aside.
    pub fn wants_peer(&self, p: &Peer) -> bool {
        self.report == Report::Peers && self.endpoint.is_none_or(|e| e == p.endpoint())
    }
}

/// FDC step, confirmation threshold and aging threshold of a node's link
/// diagnostics (LINK_DIAG's PARAMS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Params {
    pub fdc_step: u8,
    pub confirmation: u8,
    pub aging: u8,
}

/// What a node says of its link diagnostics in every LINK_DIAG response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagInfo {
    /// statusAvailabilityMask.
    pub availability: u8,
    /// [`link_diag::store`] bits.
    pub store: u8,
    pub policy: PolicySet,
    pub params: Params,
}

impl DiagInfo {
    /// A node that keeps nothing and supports no status bit.
    pub const NONE: DiagInfo = DiagInfo {
        availability: 0,
        store: store::RETENTION_NONE,
        policy: PolicySet::EMPTY,
        params: Params {
            fdc_step: 0,
            confirmation: 0,
            aging: 0,
        },
    };
}

/// The elements of a LINK_DIAG response, packed as on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elements<'a> {
    None,
    Entries(&'a [u8]),
    Peers(&'a [u8]),
}

impl<'a> Elements<'a> {
    /// The report 1 elements, in order.
    pub fn entries(&self) -> impl Iterator<Item = Entry> + 'a {
        let b: &'a [u8] = match *self {
            Elements::Entries(b) => b,
            _ => &[],
        };
        b.chunks_exact(Entry::LEN)
            .filter_map(|c| c.try_into().ok().map(Entry::from_bytes))
    }

    /// The report 2 elements, in order.
    pub fn peers(&self) -> impl Iterator<Item = Peer> + 'a {
        let b: &'a [u8] = match *self {
            Elements::Peers(b) => b,
            _ => &[],
        };
        b.chunks_exact(Peer::LEN)
            .filter_map(|c| c.try_into().ok().map(Peer::from_bytes))
    }
}

/// LINK_DIAG response body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkDiagResp<'a> {
    /// [`link_diag::FORMAT`]; a response of another format carries
    /// nothing this crate reads.
    pub format: u8,
    pub info: DiagInfo,
    pub elements: Elements<'a>,
    /// The key of the last element, when one beyond it exists.
    pub next: Option<Key>,
}

/// The longest response, status included: the fixed fields at their
/// longest, the longest packed field and NEXT.
pub const RESP_MAX: usize = FIXED_MAX
    + if field_len(&link_diag::ENTRIES, link_diag::ENTRIES.max as usize)
        > field_len(&link_diag::PEERS, link_diag::PEERS.max as usize)
    {
        field_len(&link_diag::ENTRIES, link_diag::ENTRIES.max as usize)
    } else {
        field_len(&link_diag::PEERS, link_diag::PEERS.max as usize)
    }
    + field_len(&link_diag::NEXT, link_diag::NEXT.max as usize);

impl<'a> LinkDiagResp<'a> {
    /// The longest body (after the status byte).
    pub const MAX_LEN: usize = RESP_MAX - 1;

    /// Bytes of a response, status included, with `info`, `n` elements of
    /// `report` and NEXT when `next`.
    pub fn len_of(info: &DiagInfo, report: &Report, n: usize, next: bool) -> usize {
        let policy = info.policy.as_bytes().len();
        let fixed = 1
            + 1
            + field_len(&link_diag::AVAILABILITY, 1)
            + field_len(&link_diag::STORE, 1)
            + if policy == 0 {
                0
            } else {
                field_len(&link_diag::POLICY, policy)
            }
            + field_len(&link_diag::PARAMS, 3);
        let elements = match n {
            0 => 0,
            n => field_len(report.field(), n * report.elem_len()),
        };
        let next = if next {
            field_len(&link_diag::NEXT, report.key_len())
        } else {
            0
        };
        fixed + elements + next
    }

    /// Whether a host may read the records of this response: its format
    /// is this crate's, no reserved STORE bit is set and its retention is
    /// one STORE defines. A response that is not is unknown history.
    pub fn readable(&self) -> bool {
        self.format == link_diag::FORMAT
            && self.info.store & store::RESERVED == 0
            && self.info.store & store::RETENTION <= store::RETENTION_POWER_CUT
    }

    /// Whether this is a well-formed page of the answer to `q`: readable,
    /// its elements of `q`'s report and all asked for, their keys rising
    /// and above `q`'s cursor, and NEXT, when present, the key of its last
    /// element. `Status::Arg` for anything else, so a reader never takes a
    /// malformed page for the end of the records.
    pub fn page_of(&self, q: &LinkDiagReq) -> Result<(), Status> {
        if !self.readable() {
            return Err(Status::Arg);
        }
        let (mut last, mut n) = (q.cursor, 0);
        let mut step = |k: Key, wanted: bool| {
            let rising = last.is_none_or(|l| k > l);
            last = Some(k);
            n += 1;
            wanted && rising
        };
        let avail = self.info.availability;
        let elements = match (self.elements, q.report) {
            (Elements::None, _) => true,
            (Elements::Entries(_), Report::Dtcs { .. }) => {
                (self.elements.entries()).all(|e| step(e.key(), q.wants_entry(&e, avail)))
            }
            (Elements::Peers(_), Report::Peers) => {
                (self.elements.peers()).all(|p| step(p.key(), q.wants_peer(&p)))
            }
            _ => false,
        };
        let next = self.next.is_none_or(|k| n > 0 && Some(k) == last);
        if elements && next {
            Ok(())
        } else {
            Err(Status::Arg)
        }
    }

    pub fn parse(b: &'a [u8]) -> Result<Self, Status> {
        let format = *b.first().ok_or(Status::Arg)?;
        if format != link_diag::FORMAT {
            return Ok(LinkDiagResp {
                format,
                info: DiagInfo::NONE,
                elements: Elements::None,
                next: None,
            });
        }
        let p = rd(b, &Cmd::LinkDiag.spec().resp)?;
        let params = val::<3>(&p, &link_diag::PARAMS)?.unwrap_or_default();
        let elements = match (p.field(&link_diag::ENTRIES), p.field(&link_diag::PEERS)) {
            (None, None) => Elements::None,
            (Some(e), None) => Elements::Entries(e),
            (None, Some(e)) => Elements::Peers(e),
            (Some(_), Some(_)) => return Err(Status::Arg),
        };
        let next = match p.field(&link_diag::NEXT) {
            Some(k) => Some(Key::new(k).ok_or(Status::Arg)?),
            None => None,
        };
        Ok(LinkDiagResp {
            format,
            info: DiagInfo {
                availability: byte(&p, &link_diag::AVAILABILITY, 0)?,
                store: byte(&p, &link_diag::STORE, store::RETENTION_NONE)?,
                policy: PolicySet::of(&p, &link_diag::POLICY)?,
                params: Params {
                    fdc_step: params[0],
                    confirmation: params[1],
                    aging: params[2],
                },
            },
            elements,
            next,
        })
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let i = &self.info;
        let p = &i.params;
        let mut w = FieldWriter::new(out);
        w.put(&[self.format])?
            .field(&link_diag::AVAILABILITY, &[i.availability])?
            .field(&link_diag::STORE, &[i.store])?;
        if !i.policy.is_empty() {
            w.field(&link_diag::POLICY, i.policy.as_bytes())?;
        }
        w.field(&link_diag::PARAMS, &[p.fdc_step, p.confirmation, p.aging])?;
        match self.elements {
            Elements::Entries(e) if !e.is_empty() => {
                w.field(&link_diag::ENTRIES, e)?;
            }
            Elements::Peers(e) if !e.is_empty() => {
                w.field(&link_diag::PEERS, e)?;
            }
            _ => {}
        }
        if let Some(k) = &self.next {
            w.field(&link_diag::NEXT, k.as_bytes())?;
        }
        Ok(w.len())
    }
}

/// What a LINK_DIAG_CLEAR clears, after UDS groupOfDTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearScope {
    /// One DTC of one endpoint.
    OneDtc { dtc: u32, at: Endpoint },
    /// Every DTC of one endpoint.
    EndpointDtcs(Endpoint),
    /// Every DTC of every endpoint.
    AllDtcs,
    /// The ever-communicated record and every DTC of one endpoint; signed.
    EndpointAll(Endpoint),
    /// Every ever-communicated record and every DTC; signed.
    All,
}

impl ClearScope {
    /// Its `scope` number.
    pub const fn number(&self) -> u8 {
        match self {
            ClearScope::OneDtc { .. } => scope::ONE_DTC,
            ClearScope::EndpointDtcs(_) => scope::ENDPOINT_DTCS,
            ClearScope::AllDtcs => scope::ALL_DTCS,
            ClearScope::EndpointAll(_) => scope::ENDPOINT_ALL,
            ClearScope::All => scope::ALL,
        }
    }

    /// The endpoint it clears, when one.
    pub const fn endpoint(&self) -> Option<Endpoint> {
        match *self {
            ClearScope::OneDtc { at, .. }
            | ClearScope::EndpointDtcs(at)
            | ClearScope::EndpointAll(at) => Some(at),
            ClearScope::AllDtcs | ClearScope::All => None,
        }
    }

    /// Whether it clears ever-communicated records, and so takes a KEY.
    pub const fn signed(&self) -> bool {
        matches!(self, ClearScope::EndpointAll(_) | ClearScope::All)
    }
}

/// LINK_DIAG_CLEAR request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkDiagClear {
    pub scope: ClearScope,
    /// The signature of a signed scope ([`ClearMessage`]); `None` for the
    /// others.
    pub key: Option<[u8; 64]>,
}

impl LinkDiagClear {
    /// The longest request: a signed scope of one endpoint.
    pub const MAX_LEN: usize = 1
        + field_len(&link_diag_clear::LINK, 1)
        + field_len(&link_diag_clear::ADDR, 1)
        + field_len(&link_diag_clear::KEY, 64);

    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        Self::of(&rd(b, &Cmd::LinkDiagClear.spec().req)?)
    }

    /// From a request the reader accepted: `Status::Unsupported` for a
    /// scope this table does not have; `Status::Arg` for a field its scope
    /// does not take, or one it needs left out, and for a DTC that names
    /// no single DTC.
    pub fn of(p: &Parsed<'_>) -> Result<Self, Status> {
        let [number] = at(p.core, 0)?;
        let dtc = val::<3>(p, &link_diag_clear::DTC)?;
        let at = endpoint(p, &link_diag_clear::LINK, &link_diag_clear::ADDR)?;
        let key = val::<64>(p, &link_diag_clear::KEY)?;
        let scope = match (number, dtc, at) {
            (scope::ONE_DTC, Some(d), Some(at)) => ClearScope::OneDtc {
                dtc: one_dtc(d)?,
                at,
            },
            (scope::ENDPOINT_DTCS, None, Some(at)) => ClearScope::EndpointDtcs(at),
            (scope::ALL_DTCS, None, None) => ClearScope::AllDtcs,
            (scope::ENDPOINT_ALL, None, Some(at)) => ClearScope::EndpointAll(at),
            (scope::ALL, None, None) => ClearScope::All,
            (
                scope::ONE_DTC
                | scope::ENDPOINT_DTCS
                | scope::ALL_DTCS
                | scope::ENDPOINT_ALL
                | scope::ALL,
                _,
                _,
            ) => return Err(Status::Arg),
            _ => return Err(Status::Unsupported),
        };
        if scope.signed() != key.is_some() {
            return Err(Status::Arg);
        }
        Ok(LinkDiagClear { scope, key })
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.scope.number()])?;
        if let ClearScope::OneDtc { dtc, .. } = self.scope {
            w.field(&link_diag_clear::DTC, &dtc_bytes(dtc))?;
        }
        if let Some(e) = self.scope.endpoint() {
            w.field(&link_diag_clear::LINK, &[e.link])?
                .field(&link_diag_clear::ADDR, &[e.addr])?;
        }
        if let Some(k) = &self.key {
            w.field(&link_diag_clear::KEY, k)?;
        }
        Ok(w.len())
    }
}

const SEED_MAX: usize = diag_seed::SEED_MAX as usize;

/// A seed a node issued for one signed clear: 7 to 22 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seed {
    bytes: [u8; SEED_MAX],
    len: u8,
}

impl Seed {
    /// Bytes of a DIAG_SEED response body at most.
    pub const FIELD_LEN: usize = field_len(&diag_seed::SEED, SEED_MAX);

    /// A seed of `b`; `None` outside 7 to 22 bytes.
    pub fn new(b: &[u8]) -> Option<Seed> {
        let len = u8::try_from(b.len()).ok()?;
        if !(diag_seed::SEED_MIN..=diag_seed::SEED_MAX).contains(&len) {
            return None;
        }
        let mut bytes = [0; SEED_MAX];
        bytes.get_mut(..b.len())?.copy_from_slice(b);
        Some(Seed { bytes, len })
    }

    /// The seed of a counter source: `counter` (u32 LE), `mcu_id` (u16
    /// LE), then the MCU's factory-unique ID (1 to 16 bytes, its width
    /// fixed for its `mcu_id`). Equal seeds of equal width have equal
    /// counter, `mcu_id` and ID. `None` for an ID outside 1 to 16 bytes.
    pub fn counter(counter: u32, mcu_id: u16, factory_id: &[u8]) -> Option<Seed> {
        if !(1..=16).contains(&factory_id.len()) {
            return None;
        }
        let mut b = [0; SEED_MAX];
        b[..4].copy_from_slice(&counter.to_le_bytes());
        b[4..6].copy_from_slice(&mcu_id.to_le_bytes());
        b.get_mut(6..6 + factory_id.len())?
            .copy_from_slice(factory_id);
        Seed::new(b.get(..6 + factory_id.len())?)
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or(&[])
    }

    /// From a DIAG_SEED response body.
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        let p = rd(b, &Cmd::DiagSeed.spec().resp)?;
        let seed = p.field(&diag_seed::SEED).ok_or(Status::Arg)?;
        Seed::new(seed).ok_or(Status::Arg)
    }

    /// As a DIAG_SEED response body.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.field(&diag_seed::SEED, self.as_bytes())?;
        Ok(w.len())
    }
}

/// DIAG_SEED request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagSeedReq {
    /// [`diag_seed::level`].
    pub level: u8,
}

impl DiagSeedReq {
    pub fn parse(b: &[u8]) -> Result<Self, Status> {
        Self::of(&rd(b, &Cmd::DiagSeed.spec().req)?)
    }

    /// From a request the reader accepted: `Status::Unsupported` for a
    /// level this table does not have.
    pub fn of(p: &Parsed<'_>) -> Result<Self, Status> {
        let [level] = at(p.core, 0)?;
        if level != diag_seed::level::EVER_COMMUNICATED_CLEAR {
            return Err(Status::Unsupported);
        }
        Ok(DiagSeedReq { level })
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<usize, FrameError> {
        let mut w = FieldWriter::new(out);
        w.put(&[self.level])?;
        Ok(w.len())
    }
}

/// What a signed clear's KEY signs after
/// [`dagon_format::DIAG_CLEAR_DOMAIN`]: the seed's length (one byte, so
/// seeds of different lengths never give the same message), the seed,
/// the scope, and the endpoint's link and address when the scope has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClearMessage {
    bytes: [u8; 1 + SEED_MAX + 3],
    len: usize,
}

impl ClearMessage {
    pub fn new(seed: &Seed, scope: &ClearScope) -> ClearMessage {
        let s = seed.as_bytes();
        let mut bytes = [0; 1 + SEED_MAX + 3];
        let mut len = 0;
        let mut put = |b: &[u8]| {
            if let Some(to) = bytes.get_mut(len..len + b.len()) {
                to.copy_from_slice(b);
                len += b.len();
            }
        };
        put(&[seed.len]);
        put(s);
        put(&[scope.number()]);
        if let Some(e) = scope.endpoint() {
            put(&[e.link, e.addr]);
        }
        ClearMessage { bytes, len }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }
}
