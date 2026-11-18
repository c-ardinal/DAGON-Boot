//! The link diagnostics a node keeps of its peers, as the host reads and
//! clears them: what one answer says of a peer's history, and the clear
//! of a scope, signed with the project's key when it removes
//! ever-communicated records (after UDS SecurityAccess, 0x27).

use alloc::vec::Vec;

use dagon_format::{ClearSigner, DIAG_CLEAR_DOMAIN};
use dagon_port::timeout::REQUEST_MS;
use dagon_proto::diag::{
    ClearMessage, ClearScope, DiagSeedReq, Endpoint, Key, LinkDiagClear, LinkDiagReq, LinkDiagResp,
    Peer, Report, Seed,
};
use dagon_proto::table::diag_seed::level;
use dagon_proto::table::link_diag::store;
use dagon_proto::{payload, Cmd, Status};

use crate::{hello_request, request, Error, Host, HostClock, Result, Target, Transport};

/// What a node's records say of a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum History {
    /// The peer answered since its last clear: silent now, it is lost
    /// (途絶).
    Communicated,
    /// The node would have recorded an answer and has none: no
    /// communication record (通信実績なし).
    Never,
    /// The records cannot tell (履歴不明): another format, a reserved
    /// STORE bit, or a store that may have lost or never kept one.
    Unknown,
}

/// What the report 2 answer `r`, asked about `peer` from the first key,
/// says of it. A page that is not a well-formed answer to that request
/// (`LinkDiagResp::page_of`) is unknown history. The instance identifier
/// never changes the class.
pub fn history(r: &LinkDiagResp<'_>, peer: Endpoint) -> History {
    let asked = LinkDiagReq {
        report: Report::Peers,
        endpoint: Some(peer),
        cursor: None,
        max_resp: 0,
    };
    if r.page_of(&asked).is_err() {
        return History::Unknown;
    }
    if r.elements.peers().any(|p| p.endpoint() == peer) {
        return History::Communicated;
    }
    let s = r.info.store;
    let kept = s & store::RETENTION == store::RETENTION_POWER_CUT;
    let whole = s & (store::FULL | store::LOST | store::UNKNOWN_FORMAT) == 0;
    if kept && whole {
        History::Never
    } else {
        History::Unknown
    }
}

/// What a clear came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearOutcome {
    Cleared,
    /// The node answered that it held no seed for the clear, and its
    /// records still name what the scope covers.
    NotCleared,
}

/// Every key a peer can have (link, addr).
const PEER_KEYS: usize = 1 << 16;

/// An answer to LINK_DIAG_CLEAR carries nothing.
fn cleared<E>(body: &[u8]) -> Result<(), E> {
    payload::parse_empty(&Cmd::LinkDiagClear.spec().resp, body).map_err(|_| Error::BadResponse)
}

impl<T: Transport, C: HostClock> Host<T, C> {
    /// The MAX_RESP of a LINK_DIAG to `to`: the payload the route carries
    /// back, the PC's link for the node it is connected to and the inner
    /// payload of the relay's tunnel for a linked one.
    pub fn max_resp(&self, to: Target) -> u16 {
        u16::try_from(self.link_max(to)).unwrap_or(u16::MAX)
    }

    /// The ever-communicated records of `to` (of `endpoint` only, when
    /// given), page by page. A page that is not a well-formed answer to
    /// its request is `Error::BadResponse`: no page is taken for the end
    /// of the records unless it says so. Keys rise from page to page, so
    /// there are no more pages than peer keys.
    async fn ever_communicated(
        &mut self,
        to: Target,
        endpoint: Option<Endpoint>,
    ) -> Result<Vec<Peer>, T::Error> {
        let mut peers = Vec::new();
        let mut cursor: Option<Key> = None;
        for _ in 0..PEER_KEYS {
            let q = LinkDiagReq {
                report: Report::Peers,
                endpoint,
                cursor,
                max_resp: self.max_resp(to),
            };
            let body = self
                .call(to, Cmd::LinkDiag, &request(|b| q.encode(b)), REQUEST_MS)
                .await?;
            let r = LinkDiagResp::parse(&body).map_err(|_| Error::BadResponse)?;
            r.page_of(&q).map_err(|_| Error::BadResponse)?;
            peers.extend(r.elements.peers());
            match r.next {
                Some(k) => cursor = Some(k),
                None => return Ok(peers),
            }
        }
        Err(Error::BadResponse)
    }

    /// Clears the link diagnostics records of `to` that `scope` names. A
    /// scope of ever-communicated records is signed by `signer` over a
    /// seed the node issues. Refused before any frame: without a signer
    /// ([`Error::NoSigner`]), and when the request does not fit the route
    /// ([`Error::ConnectDirectly`] for a linked node, [`Error::TooLarge`]
    /// for the one the PC is connected to). Refused before a seed is
    /// spent when the node's HELLO names another key
    /// ([`Error::KeyMismatch`]). When the node answers that it holds no
    /// seed (a resend after its reset), its records are read back.
    pub async fn link_diag_clear(
        &mut self,
        to: Target,
        scope: ClearScope,
        signer: Option<&dyn ClearSigner>,
    ) -> Result<ClearOutcome, T::Error> {
        if !scope.signed() {
            let req = request(|b| LinkDiagClear { scope, key: None }.encode(b));
            let body = self.call(to, Cmd::LinkDiagClear, &req, REQUEST_MS).await?;
            cleared(&body)?;
            return Ok(ClearOutcome::Cleared);
        }
        let signer = signer.ok_or(Error::NoSigner)?;
        let longest = LinkDiagClear {
            scope,
            key: Some([0; 64]),
        };
        let (need, room) = (request(|b| longest.encode(b)).len(), self.link_max(to));
        if need > room {
            return Err(match to {
                Target::Local => Error::TooLarge {
                    request: need,
                    room,
                },
                Target::Linked { .. } => Error::ConnectDirectly {
                    request: need,
                    room,
                },
            });
        }
        let hello = hello_request(room);
        let body = self.call(to, Cmd::Hello, &hello, REQUEST_MS).await?;
        let node = payload::hello_key_id(&body).map_err(|_| Error::BadResponse)?;
        if let Some(node) = node.filter(|n| *n != signer.key_id()) {
            let signer = signer.key_id();
            return Err(Error::KeyMismatch { signer, node });
        }
        let ask = DiagSeedReq {
            level: level::EVER_COMMUNICATED_CLEAR,
        };
        let body = self
            .call(to, Cmd::DiagSeed, &request(|b| ask.encode(b)), REQUEST_MS)
            .await?;
        let seed = Seed::parse(&body).map_err(|_| Error::BadResponse)?;
        let msg = ClearMessage::new(&seed, &scope);
        let key = signer
            .sign(DIAG_CLEAR_DOMAIN, &[msg.as_bytes()])
            .map_err(Error::Sign)?;
        let req = request(|b| {
            LinkDiagClear {
                scope,
                key: Some(key),
            }
            .encode(b)
        });
        match self.call(to, Cmd::LinkDiagClear, &req, REQUEST_MS).await {
            Ok(body) => cleared(&body).map(|()| ClearOutcome::Cleared),
            Err(Error::Rejected {
                status: Status::State,
                ..
            }) => {
                let left = self.ever_communicated(to, scope.endpoint()).await?;
                Ok(if left.is_empty() {
                    ClearOutcome::Cleared
                } else {
                    ClearOutcome::NotCleared
                })
            }
            Err(e) => Err(e),
        }
    }
}
