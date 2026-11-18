//! The command table: the single source of every command's number, of the
//! core, fields and bulk of its request and response, and of whether a
//! resend can harm. Everything else reads it; a command added here without
//! one of its columns does not build.
//!
//! A member is in a core only when every message of the command carries it
//! for ever and its layout never changes; everything else is a field.
//! Request fields that change what the receiver does are must fields;
//! response fields are optional (a reader that does not know one loses
//! only what it says). A retired field keeps its row.

use crate::field::{FieldSpec, MsgSpec, MAX_FIELDS};

/// Never a command: kept as the escape to a wider command number.
pub const RESERVED_CMD: u8 = 0xFF;

/// One command of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmdSpec {
    /// A second run answers as the first did, so a lost reply may be
    /// asked for again. Not so for a command that acts every time it runs
    /// or whose receiver has forgotten the seq by then.
    pub idempotent: bool,
    pub req: MsgSpec,
    pub resp: MsgSpec,
}

const fn msg(core: u8, fields: &'static [FieldSpec], bulk: bool) -> MsgSpec {
    MsgSpec { core, fields, bulk }
}

const NONE: MsgSpec = msg(0, &[], false);

macro_rules! commands {
    ($(
        $(#[$doc:meta])*
        $name:ident = $num:literal, idempotent: $idem:literal, req: $req:expr, resp: $resp:expr;
    )*) => {
        /// Commands of this protocol version.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub enum Cmd {
            $( $(#[$doc])* $name = $num, )*
        }

        impl Cmd {
            pub const ALL: [Cmd; [$(Cmd::$name),*].len()] = [$(Cmd::$name),*];

            /// The command's row of the table.
            pub const fn spec(self) -> &'static CmdSpec {
                match self {
                    $( Cmd::$name => {
                        const ROW: CmdSpec = CmdSpec { idempotent: $idem, req: $req, resp: $resp };
                        &ROW
                    } )*
                }
            }

            pub fn from_u8(v: u8) -> Option<Cmd> {
                Cmd::ALL.into_iter().find(|c| *c as u8 == v)
            }
        }
    };
}

commands! {
    Hello = 0x01, idempotent: true, req: msg(6, &[], false), resp: msg(6, &hello::RESP, false);
    Status = 0x02, idempotent: true, req: msg(0, &status::REQ, false), resp: msg(0, &status::RESP, false);
    /// What the node runs: stage0, boot loader and app versions, wire
    /// version, MCU id (`payload::VersionResp`).
    Version = 0x03, idempotent: true, req: NONE, resp: msg(0, &version::RESP, false);
    /// What the node knows of one command it serves: the fields of its
    /// request and of its response (`payload::CapsResp`). A command the
    /// node does not serve answers `Status::Unsupported`.
    Caps = 0x04, idempotent: true, req: msg(1, &caps::REQ, false), resp: msg(0, &caps::RESP, false);
    /// To a node's boot core while its listen window is open: stay in
    /// update mode instead of starting the app. A boot core already in
    /// update mode answers OK; an app answers `Status::State`.
    Stay = 0x05, idempotent: true, req: NONE, resp: NONE;
    XferBegin = 0x10, idempotent: true, req: msg(5, &xfer_begin::REQ, false), resp: NONE;
    XferData = 0x11, idempotent: true, req: msg(4, &[], true), resp: msg(4, &[], false);
    XferEnd = 0x12, idempotent: true, req: NONE, resp: msg(0, &xfer_end::RESP, false);
    XferHdr = 0x15, idempotent: true, req: msg(4, &[], true), resp: msg(4, &[], false);
    SetPending = 0x20, idempotent: true, req: msg(1, &set_pending::REQ, false), resp: NONE;
    /// Not resent: a node that already reset has no memory of the seq.
    Reboot = 0x21, idempotent: false, req: msg(0, &reboot::REQ, false), resp: NONE;
    Confirm = 0x22, idempotent: true, req: msg(0, &confirm::REQ, false), resp: NONE;
    /// The node replies OK, then enters its ROM loader. Not resent: it has
    /// left by then.
    EnterRom = 0x24, idempotent: false, req: msg(1, &[], false), resp: NONE;
    /// The link diagnostics a node keeps of the peers it starts exchanges
    /// with, one page at a time (`diag::LinkDiagReq`); after UDS
    /// ReadDTCInformation (0x19).
    LinkDiag = 0x30, idempotent: true, req: msg(1, &link_diag::REQ, false), resp: msg(1, &link_diag::RESP, false);
    /// Clears link diagnostics records by scope (`diag::LinkDiagClear`);
    /// after UDS ClearDiagnosticInformation (0x14). A resend of an
    /// authenticated clear is answered as the first was, with no second
    /// clear.
    LinkDiagClear = 0x31, idempotent: true, req: msg(1, &link_diag_clear::REQ, false), resp: NONE;
    /// A new seed for an authenticated clear (`diag::DiagSeedReq`); after
    /// UDS SecurityAccess (0x27). Not resent: each run issues a new seed
    /// and voids the last.
    DiagSeed = 0x32, idempotent: false, req: msg(1, &[], false), resp: msg(0, &diag_seed::RESP, false);
    /// To the hub, for its display. To a node being written, the write's
    /// one number for the node's own display.
    Show = 0x41, idempotent: true, req: msg(1, &show::REQ, false), resp: NONE;
    Relay = 0x50, idempotent: true, req: msg(1, &relay::REQ, true), resp: msg(0, &[], true);
    /// Not resent: the hub keeps no seq memory for its line controls.
    SubCtl = 0x60, idempotent: false, req: msg(0, &subctl::REQ, false), resp: NONE;
    /// Not resent: a raw transaction acts on the ROM loader every time.
    I2cRaw = 0x61, idempotent: false, req: msg(0, &i2c_raw::REQ, true), resp: msg(0, &[], true);
    /// Not resent: a conformance step acts on the node every time it runs.
    ConfRun = 0x71, idempotent: false, req: msg(1, &conf_run::REQ, false), resp: msg(1, &conf_run::RESP, false);
}

/// HELLO. Core of both: `proto_ver u32`, `max_payload u16`.
pub mod hello {
    use super::FieldSpec;
    pub const ROLE: FieldSpec = FieldSpec::opt(1, 1, 1);
    /// Image header `port_kind`.
    pub const PORT_KIND: FieldSpec = FieldSpec::opt(2, 1, 1);
    pub const MCU_ID: FieldSpec = FieldSpec::opt(3, 2, 2);
    pub const MODE: FieldSpec = FieldSpec::opt(4, 1, 1);
    pub const ACTIVE_VERSION: FieldSpec = FieldSpec::opt(5, 4, 4);
    /// 1: running on trial.
    pub const TRIAL: FieldSpec = FieldSpec::opt(6, 1, 1);
    pub const KEY_ID: FieldSpec = FieldSpec::opt(7, 8, 8);
    /// The commands the node serves, as the set of their numbers
    /// (`payload::CmdSet`). A reader takes a set as long as any command
    /// number needs; a node writes one as long as the highest command of
    /// its table needs ([`super::CMDS_LEN`]). Left out: the node does not
    /// say.
    pub const CMDS: FieldSpec = FieldSpec::opt(8, 1, 32);
    pub const RESP: [FieldSpec; 8] = [
        ROLE,
        PORT_KIND,
        MCU_ID,
        MODE,
        ACTIVE_VERSION,
        TRIAL,
        KEY_ID,
        CMDS,
    ];
}

/// CAPS. Request core: `cmd u8`, the command asked about. The response
/// has no core.
pub mod caps {
    use super::{FieldSpec, FIELD_NUM_MAX};

    /// About RELAY, to a node that relays: the link whose tunnel is asked
    /// for. Left out: no tunnel is asked for.
    pub const LINK: FieldSpec = FieldSpec::must(1, 1, 1);
    pub const REQ: [FieldSpec; 1] = [LINK];
    /// Bytes of a set of field numbers (`payload::FieldSet`).
    pub const FIELD_SET_LEN: u8 = (FIELD_NUM_MAX / 8 + 1) as u8;
    /// The fields the node reads in the command's request.
    pub const REQ_FIELDS: FieldSpec = FieldSpec::opt(1, 1, FIELD_SET_LEN);
    /// The fields the node may send in the command's response.
    pub const RESP_FIELDS: FieldSpec = FieldSpec::opt(2, 1, FIELD_SET_LEN);
    /// The longest inner frame the node forwards for this requester over
    /// the link the request names (`u16`).
    pub const TUNNEL_MTU: FieldSpec = FieldSpec::opt(3, 2, 2);
    pub const RESP: [FieldSpec; 3] = [REQ_FIELDS, RESP_FIELDS, TUNNEL_MTU];
}

/// STATUS. No core in either direction.
pub mod status {
    use super::FieldSpec;
    /// Left out: the APP slots.
    pub const TARGET: FieldSpec = FieldSpec::must(1, 1, 1);
    pub const REQ: [FieldSpec; 1] = [TARGET];
    /// Bytes of one slot: `state u8`, `version u32`, `digest16`.
    pub const SLOT_LEN: u8 = 21;
    /// Slots A, B, factory, packed.
    pub const SLOTS: FieldSpec = FieldSpec::opt(1, 0, 3 * SLOT_LEN).packed(SLOT_LEN);
    pub const ACTIVE: FieldSpec = FieldSpec::opt(2, 1, 1);
    pub const CONFIRMED: FieldSpec = FieldSpec::opt(3, 1, 1);
    pub const ATTEMPTS: FieldSpec = FieldSpec::opt(4, 1, 1);
    pub const LAST_REASON: FieldSpec = FieldSpec::opt(5, 1, 1);
    pub const LOG_GENERATION: FieldSpec = FieldSpec::opt(6, 4, 4);
    pub const RESP: [FieldSpec; 6] = [
        SLOTS,
        ACTIVE,
        CONFIRMED,
        ATTEMPTS,
        LAST_REASON,
        LOG_GENERATION,
    ];
}

/// VERSION response: the values this crate reads. The numbers are
/// `payload::version_id`'s, which lists the whole set.
pub mod version {
    use super::FieldSpec;
    pub const BOOTCORE: FieldSpec = FieldSpec::opt(1, 4, 4);
    pub const MCU: FieldSpec = FieldSpec::opt(6, 2, 2);
    pub const STAGE0: FieldSpec = FieldSpec::opt(10, 4, 4);
    /// `active u8`, then per slot A, B `state u8`, `version u32`.
    pub const BOOTCORE_SLOTS: FieldSpec = FieldSpec::opt(11, 11, 11);
    /// The same for slots A, B, factory.
    pub const APP_SLOTS: FieldSpec = FieldSpec::opt(12, 16, 16);
    pub const WIRE: FieldSpec = FieldSpec::opt(13, 1, 1);
    pub const RESP: [FieldSpec; 6] = [BOOTCORE, MCU, STAGE0, BOOTCORE_SLOTS, APP_SLOTS, WIRE];
}

/// XFER_BEGIN request. Core: `target u8`, `total_len u32`.
pub mod xfer_begin {
    use super::FieldSpec;
    pub const SLOT: FieldSpec = FieldSpec::must(1, 1, 1);
    /// Left out: no header.
    pub const HDR_LEN: FieldSpec = FieldSpec::must(2, 2, 2);
    pub const REQ: [FieldSpec; 2] = [SLOT, HDR_LEN];
}

/// XFER_END response.
pub mod xfer_end {
    use super::FieldSpec;
    /// SHA-256 the receiver computed over the body.
    pub const SHA256: FieldSpec = FieldSpec::opt(1, 32, 32);
    pub const RESP: [FieldSpec; 1] = [SHA256];
}

/// SET_PENDING request. Core: `target u8`.
pub mod set_pending {
    use super::FieldSpec;
    pub const SLOT: FieldSpec = FieldSpec::must(1, 1, 1);
    pub const REQ: [FieldSpec; 1] = [SLOT];
}

/// REBOOT request. No core.
pub mod reboot {
    use super::FieldSpec;
    /// Left out: a normal restart.
    pub const MODE: FieldSpec = FieldSpec::must(1, 1, 1);
    pub const REQ: [FieldSpec; 1] = [MODE];
}

/// CONFIRM request. No core.
pub mod confirm {
    use super::FieldSpec;
    /// Left out: the app.
    pub const TARGET: FieldSpec = FieldSpec::must(1, 1, 1);
    pub const REQ: [FieldSpec; 1] = [TARGET];
}

/// SHOW request. Core: `phase u8`. What a display may leave out is
/// optional.
pub mod show {
    use super::FieldSpec;
    pub const NODE: FieldSpec = FieldSpec::opt(1, 1, 1);
    pub const PERCENT: FieldSpec = FieldSpec::opt(2, 1, 1);
    pub const VERSION: FieldSpec = FieldSpec::opt(3, 4, 4);
    /// UTF-8.
    pub const TEXT: FieldSpec = FieldSpec::opt(4, 0, 32);
    pub const REQ: [FieldSpec; 4] = [NODE, PERCENT, VERSION, TEXT];
}

/// RELAY request. Core: `link u8`; bulk: the inner frame. The response's
/// bulk is the node's response frame.
pub mod relay {
    use super::FieldSpec;
    /// Left out: a link without addresses.
    pub const ADDR: FieldSpec = FieldSpec::must(1, 1, 1);
    /// Left out: the relay's own wait for that link.
    pub const TIMEOUT_MS: FieldSpec = FieldSpec::opt(2, 2, 2);
    /// 1: a timeout of this one exchange is no sample of the link
    /// diagnostics ([`super::policy::PER_REQUEST_INHIBIT`]); a complete
    /// answer still is. Left out: 0. Not counted in
    /// `payload::Relay::OVERHEAD_MAX`: a requester sets it only on an
    /// inner frame at least its 3 bytes below the tunnel.
    pub const INHIBIT: FieldSpec = FieldSpec::opt(3, 1, 1);
    pub const REQ: [FieldSpec; 3] = [ADDR, TIMEOUT_MS, INHIBIT];
}

/// LINK_DIAG. Request core: `report u8` ([`link_diag::report`]);
/// response core: `format u8` ([`link_diag::FORMAT`]).
pub mod link_diag {
    use super::{policy, FieldSpec};

    /// statusOfDTC mask, after UDS 0x19 0x02: the DTCs whose status has a
    /// bit of it (under AVAILABILITY). Report 1 only, which needs it.
    pub const STATUS_MASK: FieldSpec = FieldSpec::must(1, 1, 1);
    /// One DTC (3 bytes, high byte first). Report 1 only. Left out: every
    /// DTC.
    pub const DTC: FieldSpec = FieldSpec::must(2, 3, 3);
    /// One endpoint: its link, and its address on it. Both or neither.
    /// Left out: every endpoint.
    pub const LINK: FieldSpec = FieldSpec::must(3, 1, 1);
    pub const ADDR: FieldSpec = FieldSpec::must(4, 1, 1);
    /// The key of an element ([`ENTRY_KEY_LEN`] or [`PEER_KEY_LEN`]
    /// bytes): only elements above it are returned. Left out: from the
    /// first.
    pub const CURSOR: FieldSpec = FieldSpec::must(5, 2, 5);
    /// The most payload bytes of the response, status included (u16 LE):
    /// what the route back to the requester carries.
    pub const MAX_RESP: FieldSpec = FieldSpec::must(6, 2, 2);
    pub const REQ: [FieldSpec; 6] = [STATUS_MASK, DTC, LINK, ADDR, CURSOR, MAX_RESP];

    /// statusAvailabilityMask: the statusOfDTC bits the node supports.
    pub const AVAILABILITY: FieldSpec = FieldSpec::opt(1, 1, 1);
    /// What the node's store keeps ([`store`]).
    pub const STORE: FieldSpec = FieldSpec::opt(2, 1, 1);
    /// The policies the node applies, as a set of their numbers
    /// ([`super::policy`]; `payload::PolicySet`).
    pub const POLICY: FieldSpec = FieldSpec::opt(3, 1, policy::LEN);
    /// FDC step, confirmation threshold, aging threshold, one byte each.
    pub const PARAMS: FieldSpec = FieldSpec::opt(4, 3, 3);
    /// Bytes of an ENTRIES element: DTC (3, high byte first), link, addr,
    /// statusOfDTC, FDC (i8).
    pub const ENTRY_LEN: u8 = 7;
    /// Bytes of a PEERS element: link, addr, instance identifier (16).
    pub const PEER_LEN: u8 = 18;
    /// Bytes of the key of an ENTRIES element: DTC, link, addr.
    pub const ENTRY_KEY_LEN: u8 = 5;
    /// Bytes of the key of a PEERS element: link, addr.
    pub const PEER_KEY_LEN: u8 = 2;
    /// Report 1's elements, rising by key.
    pub const ENTRIES: FieldSpec = FieldSpec::opt(5, ENTRY_LEN, whole(ENTRY_LEN)).packed(ENTRY_LEN);
    /// Report 2's elements, rising by key.
    pub const PEERS: FieldSpec = FieldSpec::opt(6, PEER_LEN, whole(PEER_LEN)).packed(PEER_LEN);
    /// The key of the last element returned, when one beyond it exists:
    /// the CURSOR of the next page.
    pub const NEXT: FieldSpec = FieldSpec::opt(7, PEER_KEY_LEN, ENTRY_KEY_LEN);
    pub const RESP: [FieldSpec; 7] = [AVAILABILITY, STORE, POLICY, PARAMS, ENTRIES, PEERS, NEXT];

    /// The longest packed field of `elem`-byte elements: a field length
    /// is one byte.
    const fn whole(elem: u8) -> u8 {
        u8::MAX / elem * elem
    }

    /// The format of the response this table writes. A reader that gets
    /// another reads none of its fields.
    pub const FORMAT: u8 = 1;

    /// Request `report`.
    pub mod report {
        /// The DTCs whose status matches a mask, after UDS 0x19 0x02.
        pub const DTC_BY_STATUS_MASK: u8 = 1;
        /// The peers the node has communicated with since their last
        /// clear.
        pub const EVER_COMMUNICATED: u8 = 2;
    }

    /// Bits of STORE.
    pub mod store {
        /// Bits 0-1: what survives of the records.
        pub const RETENTION: u8 = 0b11;
        /// Kept in RAM only.
        pub const RETENTION_NONE: u8 = 0;
        /// Kept across a reset, not a power cut.
        pub const RETENTION_RESET: u8 = 1;
        /// Kept across a power cut.
        pub const RETENTION_POWER_CUT: u8 = 2;
        /// No room for another peer: a new one is not recorded.
        pub const FULL: u8 = 1 << 2;
        /// Samples or records were dropped.
        pub const LOST: u8 = 1 << 3;
        /// The store holds records of a format the node does not read.
        pub const UNKNOWN_FORMAT: u8 = 1 << 4;
        /// Bits 5-7, zero in format 1.
        pub const RESERVED: u8 = 0b1110_0000;
    }
}

/// DTC numbers of the link diagnostics: the UDS width (3 bytes), numbered
/// by DAGON (no SAE J2012 number, no failure type byte). The endpoint is
/// a key of its own, never a part of the number.
pub mod dtc {
    /// Never a DTC.
    pub const RESERVED: u32 = 0x00_0000;
    /// The group of every DTC (UDS groupOfDTC 0xFFFFFF).
    pub const ALL: u32 = 0xFF_FFFF;
    /// The peer does not answer an exchange the node started.
    pub const PEER_NOT_ANSWERING: u32 = 0x00_0001;
    /// Every DTC a fault is given.
    pub const ASSIGNED: [u32; 1] = [PEER_NOT_ANSWERING];
}

/// Numbers of the DAGON policies a node applies to its link diagnostics
/// (LINK_DIAG's POLICY).
pub mod policy {
    /// A timeout is a failure sample only for a peer that answered once.
    pub const FIRST_PASS_BEFORE_SAMPLING: u8 = 1;
    /// RELAY's INHIBIT keeps the timeout of that exchange out of the
    /// samples.
    pub const PER_REQUEST_INHIBIT: u8 = 2;
    /// A resend of a request is no sample of its own.
    pub const RETRIES_NOT_SAMPLES: u8 = 3;
    /// Every policy number.
    pub const ALL: [u8; 3] = [
        FIRST_PASS_BEFORE_SAMPLING,
        PER_REQUEST_INHIBIT,
        RETRIES_NOT_SAMPLES,
    ];
    /// Bytes of a set of policy numbers: up to the byte of the highest.
    pub const LEN: u8 = {
        let mut highest = 0;
        let mut i = 0;
        while i < ALL.len() {
            if ALL[i] > highest {
                highest = ALL[i];
            }
            i += 1;
        }
        highest / 8 + 1
    };
}

/// LINK_DIAG_CLEAR request. Core: `scope u8` ([`link_diag_clear::scope`]).
/// A field its scope does not take, or one it needs left out, is E_ARG.
pub mod link_diag_clear {
    use super::FieldSpec;

    /// Scope 1: the DTC.
    pub const DTC: FieldSpec = FieldSpec::must(1, 3, 3);
    /// Scopes 1, 2 and 16: the endpoint.
    pub const LINK: FieldSpec = FieldSpec::must(2, 1, 1);
    pub const ADDR: FieldSpec = FieldSpec::must(3, 1, 1);
    /// Scopes 16 and 17: the Ed25519 signature of the clear under the
    /// node's outstanding seed (`diag::ClearMessage`).
    pub const KEY: FieldSpec = FieldSpec::must(4, 64, 64);
    pub const REQ: [FieldSpec; 4] = [DTC, LINK, ADDR, KEY];

    /// `scope`, after UDS groupOfDTC.
    pub mod scope {
        /// One DTC of one endpoint.
        pub const ONE_DTC: u8 = 1;
        /// Every DTC of one endpoint.
        pub const ENDPOINT_DTCS: u8 = 2;
        /// Every DTC (groupOfDTC 0xFFFFFF).
        pub const ALL_DTCS: u8 = 3;
        /// Ever-communicated and every DTC of one endpoint; signed.
        pub const ENDPOINT_ALL: u8 = 16;
        /// Ever-communicated and every DTC of every endpoint; signed.
        pub const ALL: u8 = 17;
    }
}

/// DIAG_SEED. Request core: `level u8` ([`diag_seed::level`]); the
/// response has no core.
pub mod diag_seed {
    use super::FieldSpec;

    /// Bytes of the shortest seed: a counter seed with a 1-byte factory ID.
    pub const SEED_MIN: u8 = 7;
    /// Bytes of the longest seed: a counter seed with a 16-byte factory
    /// ID.
    pub const SEED_MAX: u8 = 22;
    pub const SEED: FieldSpec = FieldSpec::opt(1, SEED_MIN, SEED_MAX);
    pub const RESP: [FieldSpec; 1] = [SEED];

    /// `level`, after UDS securityAccessType.
    pub mod level {
        /// For a clear of ever-communicated records (scopes 16 and 17).
        pub const EVER_COMMUNICATED_CLEAR: u8 = 1;
    }
}

/// SUBCTL request. No core.
pub mod subctl {
    use super::FieldSpec;
    pub const NRST: FieldSpec = FieldSpec::must(1, 1, 1);
    pub const BOOT0: FieldSpec = FieldSpec::must(2, 1, 1);
    pub const REQ: [FieldSpec; 2] = [NRST, BOOT0];
}

/// I2C_RAW request. No core; bulk: the bytes to write. The response's
/// bulk is the bytes read.
pub mod i2c_raw {
    use super::FieldSpec;
    /// Left out: nothing is read.
    pub const READ_LEN: FieldSpec = FieldSpec::must(1, 2, 2);
    pub const REQ: [FieldSpec; 1] = [READ_LEN];
}

/// CONF_RUN. Request core: `test_id u8`; response core: `verdict u8`.
pub mod conf_run {
    use super::FieldSpec;
    /// Left out: 0.
    pub const PARAM: FieldSpec = FieldSpec::must(1, 4, 4);
    pub const REQ: [FieldSpec; 1] = [PARAM];
    pub const MEASURED: FieldSpec = FieldSpec::opt(1, 4, 4);
    pub const DETAIL: FieldSpec = FieldSpec::opt(2, 0, 64);
    pub const RESP: [FieldSpec; 2] = [MEASURED, DETAIL];
}

/// Fields of a non-OK response (`status || field*`).
pub mod error {
    use super::FieldSpec;
    /// With `Status::Field`: the number of the must field the receiver
    /// does not know, as a varint in its shortest form.
    pub const BAD_FIELD: FieldSpec = FieldSpec::opt(32, 1, 4);
    /// With `Status::Frame` for a frame of another wire version: the
    /// version the receiver reads.
    pub const WIRE: FieldSpec = FieldSpec::opt(33, 1, 1);
}

/// The fields that mean the same in every command (32 to 63).
pub const COMMON: [FieldSpec; 2] = [error::BAD_FIELD, error::WIRE];

/// The body of every non-OK response.
pub const ERROR: MsgSpec = msg(0, &COMMON, false);

/// Bytes of the set of this table's commands (`payload::CmdSet`): up to
/// the byte of the highest command number.
pub const CMDS_LEN: usize = {
    let mut highest = 0;
    let mut i = 0;
    while i < Cmd::ALL.len() {
        if Cmd::ALL[i] as usize > highest {
            highest = Cmd::ALL[i] as usize;
        }
        i += 1;
    }
    highest / 8 + 1
};

/// The highest number a field of the table has.
pub const FIELD_NUM_MAX: u16 = 63;

/// What the table promises, checked when it is built: no command at the
/// reserved number or at another's, fields rising by number within 1 to
/// [`FIELD_NUM_MAX`] and no more of them than the reader holds.
const fn well_formed() -> bool {
    let mut i = 0;
    while i < Cmd::ALL.len() {
        let c = Cmd::ALL[i];
        if c as u8 == RESERVED_CMD {
            return false;
        }
        let mut j = i + 1;
        while j < Cmd::ALL.len() {
            if Cmd::ALL[j] as u8 == c as u8 {
                return false;
            }
            j += 1;
        }
        if !(fields_ok(c.spec().req.fields) && fields_ok(c.spec().resp.fields)) {
            return false;
        }
        i += 1;
    }
    fields_ok(ERROR.fields)
}

const fn fields_ok(f: &[FieldSpec]) -> bool {
    if f.len() > MAX_FIELDS {
        return false;
    }
    let mut i = 0;
    while i < f.len() {
        let s = f[i];
        let rising = i == 0 || f[i - 1].num < s.num;
        if !rising || s.num == 0 || s.num > FIELD_NUM_MAX || s.min > s.max || s.elem == 0 {
            return false;
        }
        i += 1;
    }
    true
}

const _: () = assert!(well_formed());
