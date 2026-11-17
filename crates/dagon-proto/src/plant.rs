//! Requests with one fault planted, for a node's own check that it reads a
//! whole request before it acts on any of it: sent to every command the
//! node serves, each must be refused with nothing done.

use crate::field::{FieldSpec, FieldWriter, MsgSpec};
use crate::payload::CmdSet;
use crate::{Cmd, FrameError, Status};

/// A field number no command has (a two-byte key).
pub const UNKNOWN: u16 = 300;

/// The fault a planted request carries, after fields that are all well
/// formed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plant {
    /// A must field the receiver does not know ([`UNKNOWN`]).
    UnknownMust,
    /// A field sent twice: the request's last one, or an optional one the
    /// receiver does not know when the request has none.
    Duplicate,
    /// A length the table does not give: the request's last field one byte
    /// too long, or its core one byte short, or a field longer than what
    /// is left.
    BadLength,
}

impl Plant {
    pub const ALL: [Plant; 3] = [Plant::UnknownMust, Plant::Duplicate, Plant::BadLength];

    /// What a node answers it with.
    pub const fn status(self) -> Status {
        match self {
            Plant::UnknownMust => Status::Field,
            Plant::Duplicate | Plant::BadLength => Status::Arg,
        }
    }
}

/// `f` with any length taken.
const fn free(f: &FieldSpec) -> FieldSpec {
    FieldSpec {
        num: f.num,
        must: f.must,
        min: 0,
        max: u8::MAX,
        elem: 1,
    }
}

/// One zero field of `len` bytes written at `out[at..]` whatever came
/// before it; returns the offset after it.
fn put(out: &mut [u8], at: usize, f: &FieldSpec, len: usize) -> Result<usize, FrameError> {
    let zeros = [0u8; 256];
    let value = zeros.get(..len).ok_or(FrameError::Field)?;
    let mut w = FieldWriter::new(out.get_mut(at..).ok_or(FrameError::BufferTooSmall)?);
    w.field(f, value)?;
    Ok(at + w.len())
}

/// A request of `spec` with `plant` in it: a zero core, every field of the
/// table at its shortest and zero, the planted fault, then an empty bulk
/// where the request has one. Returns its length.
pub fn request(spec: &MsgSpec, plant: Plant, out: &mut [u8]) -> Result<usize, FrameError> {
    let core = usize::from(spec.core);
    let last = spec.fields.last();
    let cut_core = plant == Plant::BadLength && last.is_none() && core > 0;
    let mut at = core - usize::from(cut_core);
    out.get_mut(..at).ok_or(FrameError::BufferTooSmall)?.fill(0);
    // Where the last field of the table starts.
    let mut last_at = at;
    for f in spec.fields {
        last_at = at;
        at = put(out, at, f, usize::from(f.min))?;
    }
    let unknown = FieldSpec::opt(UNKNOWN, 0, u8::MAX);
    match (plant, last) {
        (Plant::UnknownMust, _) => at = put(out, at, &FieldSpec::must(UNKNOWN, 0, 0), 0)?,
        (Plant::Duplicate, Some(f)) => at = put(out, at, f, usize::from(f.min))?,
        (Plant::Duplicate, None) => {
            at = put(out, at, &unknown, 0)?;
            at = put(out, at, &unknown, 0)?;
        }
        (Plant::BadLength, Some(f)) if f.max < u8::MAX => {
            at = put(out, last_at, &free(f), usize::from(f.max) + 1)?;
        }
        (Plant::BadLength, _) if cut_core => {}
        (Plant::BadLength, _) => {
            // Its length says one byte more than follows.
            at = put(out, at, &unknown, 2)? - 1;
        }
    }
    if spec.bulk {
        *out.get_mut(at).ok_or(FrameError::BufferTooSmall)? = 0;
        at += 1;
    }
    Ok(at)
}

/// Room for any planted request.
pub const REQUEST_MAX: usize = 512;

/// Calls `f` with every planted request (command number, plant, request
/// payload) for the commands of `cmds`, and returns how many commands that
/// was. A node answers each with [`Plant::status`], and has done nothing.
/// A command counts once every plant went to it; a number this crate's
/// table has no command for is passed over, so the count falls short of
/// the set.
pub fn each(cmds: &CmdSet, mut f: impl FnMut(u8, Plant, &[u8])) -> usize {
    let mut buf = [0u8; REQUEST_MAX];
    let mut commands = 0;
    for cmd in cmds.iter().filter_map(Cmd::from_u8) {
        let mut planted = 0;
        for plant in Plant::ALL {
            if let Ok(len) = request(&cmd.spec().req, plant, &mut buf) {
                f(cmd as u8, plant, buf.get(..len).unwrap_or(&[]));
                planted += 1;
            }
        }
        commands += usize::from(planted == Plant::ALL.len());
    }
    commands
}

/// Calls `f` with a probe (command number, request payload) for every
/// command number: a request with an unknown must field, which a node
/// refuses with `Status::Field` where it serves the command and with
/// `Status::Unsupported` where it does not, doing nothing either way. The
/// numbers answered with `Status::Field` are what the node serves.
pub fn each_probe(mut f: impl FnMut(u8, &[u8])) {
    // A number this crate's table has no command for: no core, no field.
    const BARE: MsgSpec = MsgSpec {
        core: 0,
        fields: &[],
        bulk: false,
    };
    let mut buf = [0u8; REQUEST_MAX];
    for n in 0..=u8::MAX {
        let spec = Cmd::from_u8(n).map_or(&BARE, |c| &c.spec().req);
        if let Ok(len) = request(spec, Plant::UnknownMust, &mut buf) {
            f(n, buf.get(..len).unwrap_or(&[]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{read, Refusal};

    /// Each plant is refused as it says in every command, and by the
    /// plant alone: without the unknown must field the request reads.
    #[test]
    fn every_plant_is_refused_in_every_command() {
        for cmd in Cmd::ALL {
            let spec = &cmd.spec().req;
            let mut buf = [0u8; 512];
            for plant in Plant::ALL {
                let n = request(spec, plant, &mut buf).unwrap();
                let want = match plant {
                    Plant::UnknownMust => Refusal::Field(u32::from(UNKNOWN)),
                    _ => Refusal::Arg,
                };
                assert_eq!(read(&buf[..n], spec).err(), Some(want), "{cmd:?} {plant:?}");
                assert_eq!(Status::from(want), plant.status());
            }
            // The unknown must field is the three bytes before the bulk.
            let n = request(spec, Plant::UnknownMust, &mut buf).unwrap();
            let end = n - usize::from(spec.bulk);
            buf.copy_within(end..n, end - 3);
            assert!(read(&buf[..n - 3], spec).is_ok(), "{cmd:?}");
        }
    }
}
