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
    /// To the hub, for its display. To a node being written, the write's
    /// one number for the node's own display.
    Show = 0x41, idempotent: true, req: msg(1, &show::REQ, false), resp: NONE;
    Relay = 0x50, idempotent: true, req: msg(1, &relay::REQ, true), resp: msg(0, &[], true);
    /// Not resent: the hub keeps no seq memory for its line controls.
    SubCtl = 0x60, idempotent: false, req: msg(0, &subctl::REQ, false), resp: NONE;
    /// Not resent: a raw transaction acts on the ROM loader every time.
    I2cRaw = 0x61, idempotent: false, req: msg(0, &i2c_raw::REQ, true), resp: msg(0, &[], true);
    /// Starts a conformance session: the node forgets the results it
    /// holds and names the tests it runs (`payload::ConfListResp`).
    ConfList = 0x70, idempotent: true, req: NONE, resp: msg(0, &conf_list::RESP, false);
    /// Not resent: a conformance step acts on the node every time it runs.
    ConfRun = 0x71, idempotent: false, req: msg(1, &conf_run::REQ, false), resp: msg(1, &conf_run::RESP, false);
    /// The state and result of the node's last run of one test
    /// (`payload::ConfResultResp`).
    ConfResult = 0x72, idempotent: true, req: msg(1, &[], false), resp: msg(1, &conf_result::RESP, false);
}

/// HELLO. Core of both: `proto_ver u32`, `max_payload u16`.
pub mod hello {
    use super::FieldSpec;
    pub const ROLE: FieldSpec = FieldSpec::opt(1, 1, 1);
    /// Image header `port_kind`.
    pub const PORT_KIND: FieldSpec = FieldSpec::opt(2, 1, 1);
    /// The low 16 bits of the MCU identity.
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
    /// The cause of the node's last reset (`payload::reset_cause`). Left
    /// out: not reported.
    pub const RESET_CAUSE: FieldSpec = FieldSpec::opt(7, 1, 1);
    pub const RESP: [FieldSpec; 7] = [
        SLOTS,
        ACTIVE,
        CONFIRMED,
        ATTEMPTS,
        LAST_REASON,
        LOG_GENERATION,
        RESET_CAUSE,
    ];
}

/// VERSION response: the values this crate reads. The numbers are
/// `payload::version_id`'s, which lists the whole set.
pub mod version {
    use super::FieldSpec;
    pub const BOOTCORE: FieldSpec = FieldSpec::opt(1, 4, 4);
    pub const MCU: FieldSpec = FieldSpec::opt(6, 8, 8);
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
    pub const REQ: [FieldSpec; 2] = [ADDR, TIMEOUT_MS];
}

/// SUBCTL request. No core.
///
/// A relay answers a SUBCTL it cannot carry out the same way wherever it
/// runs (a node's firmware or the simulator):
///
/// | Status | When |
/// |---|---|
/// | `E_UNSUPPORTED` | The relay has no recovery target. |
/// | `E_ARG` | The request names a target (a link and a loader address) the relay does not have, or names only one of the two. |
/// | `E_TIMEOUT` | The target's lines could not be set ([`LINES_FAILED`]); nothing is reported as set. |
pub mod subctl {
    use super::FieldSpec;
    use crate::Status;
    /// The answer when the recovery lines could not be set.
    pub const LINES_FAILED: Status = Status::Timeout;
    pub const NRST: FieldSpec = FieldSpec::must(1, 1, 1);
    pub const BOOT0: FieldSpec = FieldSpec::must(2, 1, 1);
    /// The recovery target whose lines are set: the link its node is on
    /// (a `[[relay_link]]` id), with [`ROM_ADDR`]. Both left out: the
    /// relay's first target.
    pub const LINK: FieldSpec = FieldSpec::must(3, 1, 1);
    /// That target's ROM loader address (7-bit), with [`LINK`].
    pub const ROM_ADDR: FieldSpec = FieldSpec::must(4, 1, 1);
    pub const REQ: [FieldSpec; 4] = [NRST, BOOT0, LINK, ROM_ADDR];
}

/// I2C_RAW request. No core; bulk: the bytes to write. The response's
/// bulk is the bytes read.
pub mod i2c_raw {
    use super::FieldSpec;
    /// Left out: nothing is read.
    pub const READ_LEN: FieldSpec = FieldSpec::must(1, 2, 2);
    /// The recovery target written to: the link its node is on (a
    /// `[[relay_link]]` id), with [`ROM_ADDR`]. Both left out: the relay's
    /// first target.
    pub const LINK: FieldSpec = FieldSpec::must(2, 1, 1);
    /// That target's ROM loader address (7-bit), with [`LINK`].
    pub const ROM_ADDR: FieldSpec = FieldSpec::must(3, 1, 1);
    pub const REQ: [FieldSpec; 3] = [READ_LEN, LINK, ROM_ADDR];
}

/// CONF_LIST. No core.
pub mod conf_list {
    use super::FieldSpec;
    /// The `test_id` of every test the node runs, one byte each.
    pub const TESTS: FieldSpec = FieldSpec::opt(1, 0, 32);
    pub const RESP: [FieldSpec; 1] = [TESTS];
}

/// CONF_RUN. Request core: `test_id u8`; response core: `verdict u8`
/// (`payload::verdict::PENDING` for a test the node took and runs after
/// this answer).
pub mod conf_run {
    use super::FieldSpec;
    /// Left out: 0.
    pub const PARAM: FieldSpec = FieldSpec::must(1, 4, 4);
    pub const REQ: [FieldSpec; 1] = [PARAM];
    pub const MEASURED: FieldSpec = FieldSpec::opt(1, 4, 4);
    pub const DETAIL: FieldSpec = FieldSpec::opt(2, 0, 64);
    /// The number of this run, which CONF_RESULT repeats.
    pub const RUN_ID: FieldSpec = FieldSpec::opt(3, 2, 2);
    pub const RESP: [FieldSpec; 3] = [MEASURED, DETAIL, RUN_ID];
}

/// CONF_RESULT. Request core: `test_id u8`; response core: `state u8`
/// (`payload::conf_state`).
pub mod conf_result {
    use super::FieldSpec;
    pub const RUN_ID: FieldSpec = FieldSpec::opt(1, 2, 2);
    /// With state DONE.
    pub const VERDICT: FieldSpec = FieldSpec::opt(2, 1, 1);
    pub const MEASURED: FieldSpec = FieldSpec::opt(3, 4, 4);
    pub const DETAIL: FieldSpec = FieldSpec::opt(4, 0, 64);
    pub const RESP: [FieldSpec; 4] = [RUN_ID, VERDICT, MEASURED, DETAIL];
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
