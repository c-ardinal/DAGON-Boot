//! The link workers. Each owns its link, serves the RELAY, I2C_RAW and UART_RAW
//! requests [`serve`](crate::serve) hands it through its [`Port`]. Every
//! link operation is bounded by a deadline (the link's own).
//!
//! A port's bytes are one frame of its link's largest payload: the request
//! goes in, the worker moves it into its own buffer of the same size, and
//! the response comes back in the port's. Data longer than a buffer is
//! refused with E_ARG, never cut.

use dagon_port::timeout::link_limits;
use dagon_port::{NodeLink, RelayLinkDef};
use dagon_proto::{frame_len, Status};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;

use crate::{until, Liveness, RelayClock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Op {
    /// Forward a frame, return the node's response frame.
    Relay,
    /// I2C_RAW: write the data, then read `read_len` bytes.
    Raw,
    /// UART_RAW: send the data, then read up to `read_len` bytes, the
    /// first within `timeout_ms`.
    Stream,
}

pub(crate) struct Req {
    pub(crate) id: u32,
    pub(crate) op: Op,
    pub(crate) addr: u8,
    pub(crate) timeout_ms: u16,
    pub(crate) read_len: u16,
}

struct Resp {
    id: u32,
    status: Status,
}

/// The bytes of the request `id`: its data on the way in, its answer on
/// the way out.
struct Data<B: ?Sized> {
    id: u32,
    len: usize,
    bytes: B,
}

/// The request and response channels between [`serve`](crate::serve) and
/// one link worker, and the bytes they carry: `B` is `[u8; N]`, one frame
/// of the link's largest payload ([`Port::new`]). The caller owns one per
/// link; `&Port<[u8; N]>` is a `&Port` (the size left out), as the link
/// list takes it.
pub struct Port<B: ?Sized = [u8]> {
    req: Channel<Cs, Req, 1>,
    resp: Channel<Cs, Resp, 1>,
    data: Mutex<Cs, Data<B>>,
}

/// Whether a port of `n` bytes is the one `link` takes: one frame of its
/// largest payload, and at least one frame of its kind's maximum
/// ([`link_limits`]), the frames a driver of that kind receives.
pub const fn fits_link(link: &RelayLinkDef, n: usize) -> bool {
    let own = n == link.buffer_len();
    match link_limits(link.kind) {
        Some(l) => own && n >= frame_len(l.max_payload),
        None => own,
    }
}

impl<const N: usize> Port<[u8; N]> {
    /// The port of `link`. Built in a `static`, a size that is not the
    /// link's ([`fits_link`]) stops the build.
    pub const fn new(link: &RelayLinkDef) -> Self {
        assert!(
            fits_link(link, N),
            "a relay link's port holds one frame of its max_payload, and its kind's largest frame"
        );
        Port {
            req: Channel::new(),
            resp: Channel::new(),
            data: Mutex::new(Data {
                id: 0,
                len: 0,
                bytes: [0; N],
            }),
        }
    }
}

impl Port {
    /// Sends `req` with `data` and waits for its answer, written to `out`;
    /// returns its status and length. E_ARG when `data` is longer than the
    /// port or the answer longer than `out`; E_TIMEOUT if the link worker
    /// does not answer within the request's timeout plus `margin_ms`.
    pub(crate) async fn call<T: RelayClock>(
        &self,
        req: Req,
        data: &[u8],
        out: &mut [u8],
        clock: &T,
        margin_ms: u32,
    ) -> (Status, usize) {
        let id = req.id;
        let deadline = clock.now_ms() + u64::from(req.timeout_ms) + u64::from(margin_ms);
        let answer = async {
            {
                let mut d = self.data.lock().await;
                let Some(to) = d.bytes.get_mut(..data.len()) else {
                    return (Status::Arg, 0);
                };
                to.copy_from_slice(data);
                d.id = id;
                d.len = data.len();
            }
            self.req.send(req).await;
            let status = loop {
                let r = self.resp.receive().await;
                if r.id == id {
                    break r.status;
                }
            };
            if status != Status::Ok {
                return (status, 0);
            }
            let d = self.data.lock().await;
            match out.get_mut(..d.len) {
                Some(to) => {
                    to.copy_from_slice(&d.bytes[..d.len]);
                    (Status::Ok, d.len)
                }
                None => (Status::Arg, 0),
            }
        };
        until(clock, deadline, answer)
            .await
            .unwrap_or((Status::Timeout, 0))
    }
}

impl<B: ?Sized> Port<B> {
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
/// one copy): the requests arriving on `port`, with a buffer of the
/// port's size. `task` is its guard task ([`task::link`](crate::task::link)).
pub async fn run_link<L: NodeLink, W: Liveness, const N: usize>(
    link: &mut L,
    port: &Port<[u8; N]>,
    watch: W,
    task: usize,
) -> ! {
    let mut sent = [0u8; N];
    loop {
        watch.idle(task);
        let req = port.req.receive().await;
        watch.busy(task);
        watch.busy_for(task, req.timeout_ms.into());
        let mut d = port.data.lock().await;
        if d.id != req.id {
            // Its caller gave up, and a later request holds the bytes.
            continue;
        }
        let sent = &mut sent[..d.len];
        sent.copy_from_slice(&d.bytes[..d.len]);
        let (status, n) = match req.op {
            Op::Relay => link
                .exchange(req.addr, sent, req.timeout_ms, &mut d.bytes)
                .await
                .map_or_else(|s| (s, 0), |n| (Status::Ok, n)),
            Op::Raw => match d.bytes.get_mut(..usize::from(req.read_len)) {
                Some(read) => {
                    let n = read.len();
                    let r = link.raw(req.addr, sent, read).await;
                    r.map_or_else(|s| (s, 0), |()| (Status::Ok, n))
                }
                None => (Status::Arg, 0),
            },
            Op::Stream => match d.bytes.get_mut(..usize::from(req.read_len)) {
                Some(read) => link
                    .stream(sent, read, req.timeout_ms)
                    .await
                    .map_or_else(|s| (s, 0), |n| (Status::Ok, n)),
                None => (Status::Arg, 0),
            },
        };
        d.len = n;
        drop(d);
        port.answer(Resp { id: req.id, status });
    }
}
