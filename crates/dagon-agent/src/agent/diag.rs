//! The link diagnostics of the app's session: LINK_DIAG pages read from
//! the store, clears by scope, and the seed and key of a signed clear,
//! after UDS SecurityAccess (0x27): the node holds one seed at a time in
//! RAM, a signature checks against it once, and an accepted clear is
//! remembered so a resend of it is answered as the first was.

use dagon_format::{Sha256, SigVerifier, DIAG_CLEAR_DOMAIN};
use dagon_port::diag::{DiagStore, SeedSource};
use dagon_proto::diag::{
    ClearMessage, DiagInfo, DiagSeedReq, Elements, Entry, Key, LinkDiagClear, LinkDiagReq,
    LinkDiagResp, Peer, Report, Seed,
};
use dagon_proto::table::link_diag::FORMAT;
use dagon_proto::{flags, frame_len, payload, Cmd, Frame, Status, HEADER_LEN};

use super::{Agent, AppChain, Handled};
use crate::{SlotStore, Writer};

/// The most element bytes of one response: a field's length is one byte.
const PACKED_MAX: usize = 255;

/// What the session holds for a signed clear; gone at a reset.
#[derive(Clone, Copy)]
pub(super) struct Auth {
    /// The one outstanding seed: voided by its use, by a bad signature,
    /// and by the next DIAG_SEED.
    seed: Option<Seed>,
    /// The signed clear accepted under the last seed: the SHA-256 of its
    /// request and its outcome, until the next DIAG_SEED.
    used: Option<([u8; 32], Result<(), Status>)>,
    /// The SHA-256 of the LINK_DIAG_CLEAR request being handled, as its
    /// bytes came: a resend answers as the first only when it is the same
    /// bytes, which its typed request does not keep.
    received: [u8; 32],
}

impl Auth {
    pub(super) const NONE: Auth = Auth {
        seed: None,
        used: None,
        received: [0; 32],
    };

    /// The LINK_DIAG_CLEAR request `payload` is being handled.
    pub(super) fn received(&mut self, payload: &[u8]) {
        let mut h = Sha256::new();
        h.update(payload);
        self.received = h.finalize();
    }
}

/// One element of either report.
enum Element {
    Entry(Entry),
    Peer(Peer),
}

impl Element {
    fn key(&self) -> Key {
        match self {
            Element::Entry(e) => e.key(),
            Element::Peer(p) => p.key(),
        }
    }

    /// Writes its bytes at the start of `out`.
    fn put(&self, out: &mut [u8]) {
        match self {
            Element::Entry(e) => put(out, &e.to_bytes()),
            Element::Peer(p) => put(out, &p.to_bytes()),
        }
    }
}

fn put(out: &mut [u8], b: &[u8]) {
    if let Some(to) = out.get_mut(..b.len()) {
        to.copy_from_slice(b);
    }
}

impl<S, X, V, W, const L: usize, Q, D> Agent<S, X, V, W, L, Q, D>
where
    S: SlotStore,
    X: AppChain,
    V: SigVerifier,
    W: Writer,
    Q: SeedSource,
    D: DiagStore,
{
    /// The first element `q` asks for with a key above `after`.
    fn wanted_after(
        &mut self,
        q: &LinkDiagReq,
        info: &DiagInfo,
        after: Option<Key>,
    ) -> Result<Option<Element>, Status> {
        let mut at = after;
        loop {
            let found = match q.report {
                Report::Dtcs { .. } => self.diag.entry_after(at)?.map(Element::Entry),
                Report::Peers => self.diag.peer_after(at)?.map(Element::Peer),
            };
            let Some(e) = found else {
                return Ok(None);
            };
            let wanted = match &e {
                Element::Entry(x) => q.wants_entry(x, info.availability),
                Element::Peer(p) => q.wants_peer(p),
            };
            if wanted {
                return Ok(Some(e));
            }
            at = Some(e.key());
        }
    }

    /// LINK_DIAG: the elements above the cursor, in key order, while the
    /// answer stays within the request's MAX_RESP and what the requester
    /// takes; NEXT when one beyond the last returned exists. The node
    /// keeps nothing of the read.
    pub(super) fn link_diag(&mut self, q: LinkDiagReq) -> Handled {
        let room = usize::from(q.max_resp)
            .min(self.ceiling_on(self.on).bytes())
            .min(self.room.as_ref().len().saturating_sub(frame_len(0)));
        let info = self.diag.info();
        let width = q.report.elem_len();
        let mut packed = [0u8; PACKED_MAX];
        let (mut n, mut last) = (0, q.cursor);
        let mut next = None;
        let mut pending = self.wanted_after(&q, &info, last)?;
        while let Some(e) = pending {
            let after = self.wanted_after(&q, &info, Some(e.key()))?;
            let len = LinkDiagResp::len_of(&info, &q.report, n + 1, after.is_some());
            if (n + 1) * width > PACKED_MAX || len > room {
                if n == 0 {
                    // The request was held against the least answer.
                    return Err(Status::Range);
                }
                next = last;
                break;
            }
            e.put(packed.get_mut(n * width..).unwrap_or(&mut []));
            n += 1;
            last = Some(e.key());
            pending = after;
        }
        let bytes = packed.get(..n * width).unwrap_or(&[]);
        let elements = match q.report {
            _ if n == 0 => Elements::None,
            Report::Dtcs { .. } => Elements::Entries(bytes),
            Report::Peers => Elements::Peers(bytes),
        };
        let resp = LinkDiagResp {
            format: FORMAT,
            info,
            elements,
            next,
        };
        let body = self.room.as_mut().get_mut(HEADER_LEN + 1..);
        let len = resp
            .encode(body.unwrap_or(&mut []))
            .map_err(|_| Status::Range)?;
        Ok((len, None))
    }

    /// The LINK_DIAG request `f`, answered in the store's room; the
    /// answer frame's length.
    pub(super) fn answer_link_diag(&mut self, f: &Frame<'_>) -> Option<usize> {
        let handled = self.dispatch(f.cmd, f.payload);
        let out = self.room.as_mut();
        let len = match handled {
            Ok((body, _)) => {
                *out.get_mut(HEADER_LEN)? = Status::Ok as u8;
                1 + body
            }
            Err(s) => payload::put_refusal(out.get_mut(HEADER_LEN..)?, s, f.cmd, f.payload),
        };
        dagon_proto::finish(out, flags::RESP, f.seq, f.cmd, len).ok()
    }

    /// LINK_DIAG_CLEAR: a DTC scope at once; a signed scope against the
    /// seed held, which it uses up. A resend of the accepted one gets its
    /// answer again and clears nothing; anything else under that seed is
    /// E_STATE.
    pub(super) fn link_diag_clear(&mut self, c: LinkDiagClear) -> Handled {
        let Some(key) = c.key else {
            self.diag.clear(c.scope)?;
            return Ok((0, None));
        };
        let request = self.auth.received;
        if let Some((done, outcome)) = self.auth.used {
            if done != request {
                return Err(Status::State);
            }
            return outcome.map(|()| (0, None));
        }
        let seed = self.auth.seed.take().ok_or(Status::State)?;
        let msg = ClearMessage::new(&seed, &c.scope);
        if !self
            .verifier
            .verify(DIAG_CLEAR_DOMAIN, &[msg.as_bytes()], &key)
        {
            return Err(Status::Signature);
        }
        let outcome = self.diag.clear(c.scope);
        self.auth.used = Some((request, outcome));
        outcome.map(|()| (0, None))
    }

    /// DIAG_SEED: voids what the last seed held, then issues a new one;
    /// after a refusal the node holds none. The answers to LINK_DIAG_CLEAR
    /// a link keeps for a resend end with it, on every link: a resend is
    /// then checked against the new seed, and a signature made for an
    /// earlier seed is E_SIGNATURE and voids it.
    pub(super) fn diag_seed(&mut self, _: DiagSeedReq) -> Handled {
        self.auth = Auth::NONE;
        let clear = Cmd::LinkDiagClear as u8;
        for link in self.links.iter_mut() {
            if link.last.is_some_and(|(_, cmd)| cmd == clear) {
                link.last = None;
            }
        }
        let seed = self.seeds.seed(&mut self.diag)?;
        self.auth.seed = Some(seed);
        let len = seed.encode(self.body()).map_err(|_| Status::Range)?;
        Ok((len, None))
    }
}
