//! A node's handler table: one row for each command it serves, and
//! nothing for the rest of the command table. What the node says it serves
//! (HELLO's `cmds`, CAPS) is read from these rows.
//!
//! A row is in two parts that cannot be mixed. Its decoder is the
//! command's own ([`Req::decoder`]): it makes the typed request out of a
//! message the one reader accepted, refusing a value outside its range.
//! Its action takes the node and that typed request and nothing else: no
//! payload bytes and no parsed message reach it. Between the two the
//! answer the request asks for is held against what the requester takes
//! ([`Ceiling`]). So when a request is refused, for its form, a value or
//! the size of its answer, no action has run.

pub use crate::ceiling::Ceiling;
use crate::field::read;
pub use crate::payload::Decode;
use crate::payload::{CmdSet, Req};
use crate::table::CmdSpec;
use crate::{Cmd, Status};

/// What a row does with the node and a typed request.
pub type Act<C, R> = for<'a> fn(&mut C, Req<'a>) -> Result<R, Status>;

/// One command a node serves, for a node state `C` and an outcome `R`.
pub struct Handler<C, R> {
    pub cmd: Cmd,
    /// The command's row of the command table.
    pub spec: &'static CmdSpec,
    /// Whether bulk follows the request's fields.
    pub bulk: bool,
    decode: Decode,
    act: Act<C, R>,
}

impl<C, R> Handler<C, R> {
    /// The row of `cmd`: the command's decoder, and `act` for what it
    /// decodes.
    pub const fn new(cmd: Cmd, act: Act<C, R>) -> Self {
        let spec = cmd.spec();
        Handler {
            cmd,
            spec,
            bulk: spec.req.bulk,
            decode: Req::decoder(cmd),
            act,
        }
    }

    /// The typed request in `payload`, with nothing done: `Status::Arg`
    /// or `Status::Field` when the reader refuses it, `Status::Arg` for a
    /// value outside its range and for an answer that would not fit
    /// `ceiling`.
    pub fn request<'a>(&self, payload: &'a [u8], ceiling: Ceiling) -> Result<Req<'a>, Status> {
        let message = read(payload, &self.spec.req)?;
        let request = (self.decode)(&message)?;
        if !request.fits(ceiling) {
            return Err(Status::Arg);
        }
        Ok(request)
    }

    /// Runs the row's action on a request [`Handler::request`] gave.
    pub fn act(&self, node: &mut C, request: Req<'_>) -> Result<R, Status> {
        (self.act)(node, request)
    }

    /// [`Handler::request`], then the action.
    pub fn call(&self, node: &mut C, payload: &[u8], ceiling: Ceiling) -> Result<R, Status> {
        let request = self.request(payload, ceiling)?;
        self.act(node, request)
    }
}

impl Handler<(), ()> {
    /// A row whose handler cannot run from the table (it waits on a link,
    /// or holds what a row cannot name): the node takes the typed request
    /// from [`Handler::request`] and runs its handler on that.
    pub const fn deferred(cmd: Cmd) -> Self {
        Handler::new(cmd, |_, _| Ok(()))
    }
}

/// The row of command number `cmd`; `None`: the node does not serve it
/// (`Status::Unsupported`).
pub fn find<C, R>(table: &[Handler<C, R>], cmd: u8) -> Option<&Handler<C, R>> {
    table.iter().find(|h| h.cmd as u8 == cmd)
}

/// The commands of `table`, as HELLO's `cmds` says them.
pub const fn cmds<C, R>(table: &[Handler<C, R>]) -> CmdSet {
    let mut set = CmdSet::EMPTY;
    let mut i = 0;
    while i < table.len() {
        set = set.with(table[i].cmd as u8);
        i += 1;
    }
    set
}
