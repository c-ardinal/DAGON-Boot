//! Child-link tasks. Each owns its bus (I2C0 for I2C nodes, the PIO UART for
//! the UART node), serves RELAY / I2C_RAW requests from the PC task through
//! a channel, and polls its nodes with a display HELLO every 2 s while the
//! PC is quiet. Every bus operation is bounded by a deadline.

use dagon_proto::payload::link;
use dagon_proto::Status;
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::channel::Channel;
use embassy_time::{with_timeout, Duration, Timer};
use heapless::Vec;

use crate::lcd::Shade;
use rp2040_common::uart::{UartLink, UART_FRAME};

use crate::relay::{I2cLink, PioStream, RpI2c, I2C_FRAME};
use crate::state::{self, Seen};
use crate::wdt::{self, Task};

const POLL_EVERY: Duration = Duration::from_secs(2);
/// Largest request or response body carried through a port.
pub const DATA_MAX: usize = dagon_proto::max_payload::USB - 1;
/// Extra time the PC task waits beyond the request's own timeout.
const PORT_MARGIN: Duration = Duration::from_millis(1_000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Forward a frame, return the node's response frame.
    Relay,
    /// I2C_RAW: write `data`, then read `read_len` bytes.
    Raw,
}

pub struct Req {
    pub id: u32,
    pub op: Op,
    pub addr: u8,
    pub timeout_ms: u16,
    pub read_len: u16,
    pub data: Vec<u8, DATA_MAX>,
}

pub struct Resp {
    pub id: u32,
    pub status: Status,
    pub data: Vec<u8, DATA_MAX>,
}

/// Request/response channels of one link task.
pub struct Port {
    req: Channel<Cs, Req, 1>,
    resp: Channel<Cs, Resp, 1>,
}

pub static I2C_PORT: Port = Port::new();
pub static UART_PORT: Port = Port::new();

impl Port {
    const fn new() -> Self {
        Port {
            req: Channel::new(),
            resp: Channel::new(),
        }
    }

    /// Sends `req` and waits for its response; E_TIMEOUT if the link task
    /// does not answer within the request's timeout plus a margin.
    pub async fn call(&self, req: Req) -> Resp {
        let id = req.id;
        let limit = Duration::from_millis(req.timeout_ms as u64) + PORT_MARGIN;
        let answer = async {
            self.req.send(req).await;
            loop {
                let r = self.resp.receive().await;
                if r.id == id {
                    return r;
                }
            }
        };
        with_timeout(limit, answer).await.unwrap_or(Resp {
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

fn seen(h: Option<dagon_proto::payload::HelloResp>) -> Seen {
    match h {
        Some(h) if h.trial => Seen {
            version: Some(h.active_version),
            shade: Shade::Trial,
        },
        Some(h) => Seen {
            version: Some(h.active_version),
            shade: Shade::Confirmed,
        },
        None => Seen {
            version: None,
            shade: Shade::NoAnswer,
        },
    }
}

fn next_seq(seq: &mut u8) -> u8 {
    *seq = seq.wrapping_add(1) | 0x80;
    *seq
}

#[embassy_executor::task]
pub async fn i2c_task(mut link: I2cLink<RpI2c<'static>>) -> ! {
    let mut seq = 0x80;
    loop {
        wdt::idle(Task::I2c);
        let event = select(I2C_PORT.req.receive(), Timer::after(POLL_EVERY)).await;
        wdt::busy(Task::I2c);
        match event {
            Either::First(req) => {
                wdt::busy_for(Task::I2c, Duration::from_millis(req.timeout_ms as u64));
                let mut out = [0u8; DATA_MAX];
                let (status, n) = match req.op {
                    Op::Relay => {
                        let r = link
                            .exchange(req.addr, &req.data, req.timeout_ms, &mut out[..I2C_FRAME])
                            .await;
                        r.map_or_else(|s| (s, 0), |n| (Status::Ok, n))
                    }
                    Op::Raw => {
                        let n = req.read_len as usize;
                        let r = link.rom(req.addr, &req.data, &mut out[..n]).await;
                        r.map_or_else(|s| (s, 0), |()| (Status::Ok, n))
                    }
                };
                I2C_PORT.answer(Resp {
                    id: req.id,
                    status,
                    data: Vec::from_slice(&out[..n]).unwrap_or_default(),
                });
            }
            Either::Second(()) if state::pc_quiet() => {
                for sub in state::subs().iter().filter(|s| s.link == link::I2C0) {
                    let h = link.hello(sub.addr, next_seq(&mut seq)).await;
                    state::set_seen(sub.index, seen(h));
                }
            }
            Either::Second(()) => {}
        }
    }
}

#[embassy_executor::task]
pub async fn uart_task(mut link: UartLink<PioStream<'static>, fn() -> u64>) -> ! {
    let mut seq = 0x80;
    loop {
        wdt::idle(Task::Uart);
        let event = select(UART_PORT.req.receive(), Timer::after(POLL_EVERY)).await;
        wdt::busy(Task::Uart);
        match event {
            Either::First(req) => {
                wdt::busy_for(Task::Uart, Duration::from_millis(req.timeout_ms as u64));
                let mut out = [0u8; UART_FRAME];
                let r = link.exchange(&req.data, req.timeout_ms, &mut out).await;
                let (status, n) = r.map_or_else(|s| (s, 0), |n| (Status::Ok, n));
                UART_PORT.answer(Resp {
                    id: req.id,
                    status,
                    data: Vec::from_slice(&out[..n]).unwrap_or_default(),
                });
            }
            Either::Second(()) if state::pc_quiet() => {
                for sub in state::subs().iter().filter(|s| s.link == link::PIO_UART) {
                    let h = link.hello(next_seq(&mut seq)).await;
                    state::set_seen(sub.index, seen(h));
                }
            }
            Either::Second(()) => {}
        }
    }
}
