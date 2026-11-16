//! The link workers. Each owns its link, serves the RELAY and I2C_RAW
//! requests [`serve`](crate::serve) hands it through its [`Port`]. Every
//! link operation is bounded by a deadline (the link's own).

use dagon_port::NodeLink;
use dagon_proto::{frame_len, Status};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::channel::Channel;
use heapless::Vec;

use crate::{until, Liveness, RelayClock, RelayLinkDef};

/// Largest request or response body carried through a port.
pub(crate) const DATA_MAX: usize = dagon_proto::max_payload::USB - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    /// Forward a frame, return the node's response frame.
    Relay,
    /// I2C_RAW: write `data`, then read `read_len` bytes.
    Raw,
}

pub(crate) struct Req {
    pub(crate) id: u32,
    pub(crate) op: Op,
    pub(crate) addr: u8,
    pub(crate) timeout_ms: u16,
    pub(crate) read_len: u16,
    pub(crate) data: Vec<u8, DATA_MAX>,
}

pub(crate) struct Resp {
    id: u32,
    pub(crate) status: Status,
    pub(crate) data: Vec<u8, DATA_MAX>,
}

/// The request and response channels between [`serve`](crate::serve) and
/// one link worker. The caller owns one per link.
pub struct Port {
    req: Channel<Cs, Req, 1>,
    resp: Channel<Cs, Resp, 1>,
}

impl Default for Port {
    fn default() -> Self {
        Port::new()
    }
}

impl Port {
    pub const fn new() -> Self {
        Port {
            req: Channel::new(),
            resp: Channel::new(),
        }
    }

    /// Sends `req` and waits for its response; E_TIMEOUT if the link
    /// worker does not answer within the request's timeout plus
    /// `margin_ms`.
    pub(crate) async fn call<T: RelayClock>(&self, req: Req, clock: &T, margin_ms: u32) -> Resp {
        let id = req.id;
        let deadline = clock.now_ms() + u64::from(req.timeout_ms) + u64::from(margin_ms);
        let answer = async {
            self.req.send(req).await;
            loop {
                let r = self.resp.receive().await;
                if r.id == id {
                    return r;
                }
            }
        };
        until(clock, deadline, answer).await.unwrap_or(Resp {
            id,
            status: Status::Timeout,
            data: Vec::new(),
        })
    }

    /// Hands a response back, dropping a stale one nobody waited for.
    fn answer(&self, resp: Resp) {
        while self.resp.try_receive().is_ok() {}
        let _ = self.resp.try_send(resp);
    }
}

/// One link of this node, as one entry of the one list
/// [`serve`](crate::serve) takes: its descriptor (id, largest payload,
/// addressing) and its worker's port, `None` when the MCU defines the link
/// but this board does not wire it.
#[derive(Clone, Copy)]
pub struct LinkRoute<'a> {
    pub link: RelayLinkDef,
    pub port: Option<&'a Port>,
}

/// Serves `link` for good (the caller keeps it, so an embassy task holds
/// one copy): the requests arriving on `port`. `task` is its guard task
/// ([`task::link`](crate::task::link)).
pub async fn run_link<L: NodeLink, W: Liveness>(
    link: &mut L,
    port: &Port,
    watch: W,
    task: usize,
) -> ! {
    let frame_max = frame_len(link.max_payload()).min(DATA_MAX);
    loop {
        watch.idle(task);
        let req = port.req.receive().await;
        watch.busy(task);
        watch.busy_for(task, req.timeout_ms.into());
        let mut out = [0u8; DATA_MAX];
        let (status, n) = match req.op {
            Op::Relay => {
                let r = link
                    .exchange(req.addr, &req.data, req.timeout_ms, &mut out[..frame_max])
                    .await;
                r.map_or_else(|s| (s, 0), |n| (Status::Ok, n))
            }
            Op::Raw => {
                let n = usize::from(req.read_len).min(DATA_MAX);
                let r = link.raw(req.addr, &req.data, &mut out[..n]).await;
                r.map_or_else(|s| (s, 0), |()| (Status::Ok, n))
            }
        };
        port.answer(Resp {
            id: req.id,
            status,
            data: Vec::from_slice(&out[..n]).unwrap_or_default(),
        });
    }
}
