//! A node's handler table: one row for each command it serves, and
//! nothing for the rest of the command table. What the node says it serves
//! (HELLO's `cmds`, CAPS) is read from these rows, and a request reaches a
//! row's handler only after the one reader accepted its core and every
//! field, so nothing has happened when a request is refused.

use crate::field::{read, Parsed};
use crate::payload::CmdSet;
use crate::table::CmdSpec;
use crate::{Cmd, Status};

/// What a row runs on a request the reader accepted: it takes the typed
/// request out of `Parsed` (refusing a value outside its range) and hands
/// it to the node's handler.
pub type Run<C, R> = fn(&mut C, &Parsed<'_>) -> Result<R, Status>;

/// One command a node serves, for a node state `C` and an outcome `R`.
pub struct Handler<C, R> {
    pub cmd: Cmd,
    /// The command's row of the command table.
    pub spec: &'static CmdSpec,
    /// Whether bulk follows the request's fields.
    pub bulk: bool,
    pub run: Run<C, R>,
}

impl<C, R> Handler<C, R> {
    pub const fn new(cmd: Cmd, run: Run<C, R>) -> Self {
        let spec = cmd.spec();
        Handler {
            cmd,
            spec,
            bulk: spec.req.bulk,
            run,
        }
    }

    /// Reads `payload` as the command's request, then runs the handler.
    /// `Status::Arg` or `Status::Field` when the reader refuses it, with
    /// the handler not run.
    pub fn call(&self, node: &mut C, payload: &[u8]) -> Result<R, Status> {
        let request = read(payload, &self.spec.req)?;
        (self.run)(node, &request)
    }
}

impl<R> Handler<(), R> {
    /// For a node whose handlers cannot run from a row (they wait on a
    /// link, or hold what a row cannot name): reads `payload` as the
    /// command's request and returns what the row took out of it, with
    /// the request's bulk. The node runs its handler on that.
    pub fn read<'a>(&self, payload: &'a [u8]) -> Result<(R, &'a [u8]), Status> {
        let request = read(payload, &self.spec.req)?;
        Ok(((self.run)(&mut (), &request)?, request.bulk))
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
