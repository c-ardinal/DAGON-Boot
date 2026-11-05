//! The serving loop of a node's agent over any [`Link`]: wait for a request
//! frame, answer it, and after the reply to REBOOT let the reply leave,
//! wait the delay `allow_reboot_now` asked for and hand the reset to the
//! caller. What differs between MCUs (watchdog reports, a conformance build
//! answering first, logging, the timer) comes through [`ServeHost`]. The
//! loop reports the link and every request to the agent's chain
//! (`app.link_up`, `app.link_down`, `app.request`, `app.replied`).

use dagon_agent::{reply_status, Agent, AppChain, SlotStore, IDLE_TIMEOUT_MS};

use crate::link::{Link, LinkError};

/// What the serving loop asks of its MCU.
#[allow(async_fn_in_trait)]
pub trait ServeHost<S: SlotStore, X: AppChain> {
    /// Milliseconds on a monotonic clock.
    fn now_ms(&self) -> u64;
    /// Waiting for a request (no deadline of its own).
    fn idle(&mut self);
    /// A request is being handled.
    fn busy(&mut self);
    /// A gated feed of the watchdog: every request starts with one, so a
    /// flash erase it runs has a full period (execution model, section 12,
    /// rule 4).
    fn feed(&mut self);
    /// A request arrived, before it is handled (e.g. a gated feed so an
    /// erase starts with a full watchdog period).
    fn on_request(&mut self) {}
    /// Answers `request` before the agent does (a conformance build);
    /// `None` leaves it to the agent.
    fn intercept(&mut self, _agent: &mut Agent<S, X>, _request: &[u8]) -> Option<&[u8]> {
        None
    }
    /// The request (command `cmd`) was answered (e.g. trial state re-read,
    /// timing log).
    fn after(&mut self, _agent: &mut Agent<S, X>, _cmd: u8) {}
    /// A read failed (logging); the loop goes on.
    fn on_error(&mut self, _e: LinkError) {}
    /// Waits `ms` before a reset. The loop has reported itself idle, so
    /// the watchdog guard keeps feeding meanwhile (the delay is capped
    /// below the watchdog's period).
    async fn wait_ms(&mut self, ms: u32);
}

/// Why [`serve`] returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The reply to REBOOT has left: reset now.
    Reboot,
    /// The link is gone.
    Closed,
}

/// Serves `agent` over `link` until a reboot is due or the link closes.
/// A transfer the PC abandoned is dropped after `IDLE_TIMEOUT_MS` without
/// requests, checked whenever the link's read times out.
///
/// The link is up from the first frame that arrives (`app.link_up`) until
/// a reply cannot be written or the link closes (`app.link_down`). A reboot
/// follows only a reply that was written and flushed; otherwise the node
/// keeps serving and the peer's retry gets the same reply again.
pub async fn serve<L, S, X, H>(link: &mut L, agent: &mut Agent<S, X>, host: &mut H) -> Exit
where
    L: Link,
    S: SlotStore,
    X: AppChain,
    H: ServeHost<S, X>,
{
    let (via, addr) = (link.kind().code(), link.addr());
    let mut up = false;
    let mut last_request: Option<u64> = None;
    loop {
        host.idle();
        let read = link.read_frame().await;
        host.busy();
        let request = match read {
            Ok(r) => r,
            Err(LinkError::Timeout) => {
                let idle = IDLE_TIMEOUT_MS as u64;
                if last_request.is_some_and(|t| host.now_ms().saturating_sub(t) >= idle) {
                    agent.abort();
                    last_request = None;
                }
                continue;
            }
            Err(LinkError::Closed) => {
                if up {
                    agent.link_down(via, addr);
                }
                return Exit::Closed;
            }
            Err(e) => {
                host.on_error(e);
                continue;
            }
        };
        host.feed();
        if !up {
            up = true;
            agent.link_up(via, addr);
        }
        let arrived = host.now_ms();
        last_request = Some(arrived);
        // Byte 5 of a frame is the command.
        let cmd = request.get(5).copied().unwrap_or(0);
        agent.request(request, via);
        host.on_request();
        let mut reboot = None;
        let mut status = None;
        let wrote = match host.intercept(agent, request) {
            Some(answer) => {
                status = reply_status(answer);
                link.write_frame(answer).await
            }
            None => match agent.handle(request) {
                Some(reply) => {
                    reboot = reply.reboot.then_some(reply.reboot_delay_ms);
                    status = reply_status(reply.frame);
                    link.write_frame(reply.frame).await
                }
                // Not a valid request, or not for this node: no answer, but
                // the link must take the next request.
                None => link.no_reply().await,
            },
        };
        host.after(agent, cmd);
        match (wrote, status) {
            (Err(e), _) => {
                host.on_error(e);
                up = false;
                agent.link_down(via, addr);
                continue;
            }
            (Ok(()), Some(s)) => {
                let ms = host.now_ms().saturating_sub(arrived);
                agent.replied(s, u32::try_from(ms).unwrap_or(u32::MAX));
            }
            (Ok(()), None) => {}
        }
        // Reset only once the reply has left; a link that cannot tell (a
        // new request came first) keeps serving instead.
        if let Some(delay_ms) = reboot {
            if link.flush().await.is_ok() {
                host.idle();
                host.wait_ms(delay_ms).await;
                agent.before_reboot();
                return Exit::Reboot;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::vec::Vec;

    use crate::hooks::{always, node_chain, NodeChain};
    use dagon_events::{
        compose, Bounds, Composed, Ctx, Delay, Event, Extension, Layer, Nop, Published,
        RebootRequest, Watchdog,
    };
    use dagon_proto::payload::{reboot_mode, HelloResp};
    use dagon_proto::{max_payload, split_status, Cmd, Frame, Status};
    use dagon_sim::store::SimStore;

    use super::*;
    use crate::link::{Kind, Reconnect};
    use crate::uart::hello_request;

    /// A link fed from a queue: `None` entries are read timeouts; when the
    /// queue is empty the link is closed.
    /// Like an I2C target: a request is handed out with the link closed
    /// for new ones (NACK) until it is answered or dropped.
    #[derive(Default)]
    struct Mock {
        incoming: VecDeque<Option<Vec<u8>>>,
        current: Vec<u8>,
        written: Vec<Vec<u8>>,
        flushed: usize,
        dropped: usize,
        closed_for_requests: bool,
        /// Writes and flushes that fail.
        fail_write: bool,
        fail_flush: bool,
    }

    impl Link for Mock {
        fn max_payload(&self) -> usize {
            max_payload::UART
        }
        fn reconnect(&self) -> Reconnect {
            Reconnect::Persistent
        }
        fn kind(&self) -> Kind {
            Kind::Uart
        }
        async fn read_frame(&mut self) -> Result<&[u8], LinkError> {
            assert!(
                !self.closed_for_requests,
                "the last request was neither answered nor dropped"
            );
            match self.incoming.pop_front() {
                Some(Some(f)) => {
                    self.current = f;
                    self.closed_for_requests = true;
                    Ok(&self.current)
                }
                Some(None) => Err(LinkError::Timeout),
                None => Err(LinkError::Closed),
            }
        }
        async fn write_frame(&mut self, frame: &[u8]) -> Result<(), LinkError> {
            self.closed_for_requests = false;
            if self.fail_write {
                return Err(LinkError::Closed);
            }
            self.written.push(frame.to_vec());
            Ok(())
        }
        async fn no_reply(&mut self) -> Result<(), LinkError> {
            self.dropped += 1;
            self.closed_for_requests = false;
            Ok(())
        }
        async fn flush(&mut self) -> Result<(), LinkError> {
            self.flushed += 1;
            if self.fail_flush {
                return Err(LinkError::Closed);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct Host {
        busy: usize,
        requests: usize,
        cmds: Vec<u8>,
        answer: Option<Vec<u8>>,
        /// `idle`, `busy` and `wait <ms>`, in order.
        calls: Vec<std::string::String>,
    }

    impl<S: SlotStore, X: AppChain> ServeHost<S, X> for Host {
        fn now_ms(&self) -> u64 {
            0
        }
        fn idle(&mut self) {
            self.calls.push("idle".into());
        }
        fn busy(&mut self) {
            self.busy += 1;
            self.calls.push("busy".into());
        }
        fn feed(&mut self) {
            self.calls.push("feed".into());
        }
        fn on_request(&mut self) {
            self.requests += 1;
        }
        fn intercept(&mut self, _: &mut Agent<S, X>, request: &[u8]) -> Option<&[u8]> {
            (request.get(5) == Some(&0x7F)).then_some(())?;
            self.answer.as_deref()
        }
        fn after(&mut self, _: &mut Agent<S, X>, cmd: u8) {
            self.cmds.push(cmd);
            self.calls.push("answered".into());
        }
        async fn wait_ms(&mut self, ms: u32) {
            self.calls.push(std::format!("wait {ms}"));
        }
    }

    /// An app extension that records the events it sees and asks for a
    /// reboot delay.
    #[derive(Clone, Default)]
    struct Probe {
        seen: Rc<RefCell<Vec<&'static str>>>,
        delay_ms: u32,
    }

    impl Extension for Probe {
        fn id(&self) -> &'static str {
            "probe"
        }
        fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
            self.seen.borrow_mut().push(e.kind());
        }
        fn allow_reboot_now(&mut self, _: &RebootRequest, _: &Ctx<'_>) -> Option<Delay> {
            Some(Delay::After(self.delay_ms))
        }
    }

    type ProbeChain = Composed<&'static Published<(), 8>, Nop, Probe, fn() -> u64>;

    fn probed(probe: Probe) -> Agent<SimStore, ProbeChain> {
        let events = std::boxed::Box::leak(std::boxed::Box::new(Published::new()));
        let now: fn() -> u64 = || 0;
        let chain = compose(
            Layer::App,
            events,
            Nop,
            probe,
            Bounds::new(now, Watchdog::new(4000, 4000)),
        );
        Agent::new(dagon_sim::f401_config([0; 32]), SimStore::default(), chain)
    }

    fn agent() -> Agent<SimStore, NodeChain> {
        let events = std::boxed::Box::leak(std::boxed::Box::new(Published::new()));
        let (chain, _) = node_chain(events, None, None, always, || 0, Watchdog::new(4000, 4000));
        Agent::new(dagon_sim::f401_config([0; 32]), SimStore::default(), chain)
    }

    fn hello(seq: u8) -> Vec<u8> {
        let mut b = [0u8; 32];
        let n = hello_request(max_payload::UART, seq, &mut b).unwrap();
        b[..n].to_vec()
    }

    fn request(seq: u8, cmd: u8, payload: &[u8]) -> Vec<u8> {
        let mut b = [0u8; 64];
        b[8..8 + payload.len()].copy_from_slice(payload);
        let n = dagon_proto::finish(&mut b, 0, seq, cmd, payload.len()).unwrap();
        b[..n].to_vec()
    }

    #[test]
    fn answers_requests_and_skips_timeouts_until_closed() {
        let mut link = Mock::default();
        link.incoming.extend([Some(hello(1)), None, Some(hello(2))]);
        let (mut a, mut h) = (agent(), Host::default());
        let exit = dagon_sim::block_on(serve(&mut link, &mut a, &mut h));
        assert_eq!(exit, Exit::Closed);
        assert_eq!(link.written.len(), 2);
        for (w, seq) in link.written.iter().zip([1u8, 2]) {
            let (f, _) = Frame::decode(w, max_payload::UART).unwrap();
            assert_eq!(f.seq, seq);
            let (st, body) = split_status(f.payload).unwrap();
            assert_eq!(st, Status::Ok);
            assert!(HelloResp::parse(body).is_ok());
        }
        assert_eq!(h.requests, 2);
        assert_eq!(h.cmds, [Cmd::Hello as u8, Cmd::Hello as u8]);
        assert_eq!(link.flushed, 0);
    }

    #[test]
    fn reboot_reply_is_flushed_before_the_exit() {
        let mut link = Mock::default();
        let reboot = request(3, Cmd::Reboot as u8, &[reboot_mode::NORMAL]);
        link.incoming.extend([Some(reboot), Some(hello(4))]);
        let (mut a, mut h) = (agent(), Host::default());
        let exit = dagon_sim::block_on(serve(&mut link, &mut a, &mut h));
        assert_eq!(exit, Exit::Reboot);
        assert_eq!(link.written.len(), 1, "the reply, then nothing more");
        assert_eq!(link.flushed, 1);
        assert_eq!(h.calls.last().unwrap(), "wait 0");
    }

    /// The delay `allow_reboot_now` asked for is waited after the reply
    /// left, with the loop idle (the guard keeps feeding); the reset
    /// follows `app.before_reboot`.
    #[test]
    fn reboot_waits_the_asked_delay_while_idle() {
        let probe = Probe {
            delay_ms: 300,
            ..Probe::default()
        };
        let mut a = probed(probe.clone());
        let mut link = Mock::default();
        let reboot = request(3, Cmd::Reboot as u8, &[reboot_mode::NORMAL]);
        link.incoming.extend([Some(reboot), Some(hello(4))]);
        let mut h = Host::default();
        let exit = dagon_sim::block_on(serve(&mut link, &mut a, &mut h));
        assert_eq!(exit, Exit::Reboot);
        assert_eq!(h.calls[h.calls.len() - 2..], ["idle", "wait 300"]);
        let seen = probe.seen.borrow();
        let at = seen.iter().position(|k| *k == "request").unwrap();
        assert_eq!(
            seen[at..],
            ["request", "reboot_requested", "replied", "before_reboot"]
        );
    }

    /// Every request starts with a gated feed, before it is handled; a
    /// read timeout feeds nothing.
    #[test]
    fn every_request_starts_with_a_gated_feed() {
        let mut link = Mock::default();
        link.incoming.extend([Some(hello(1)), None, Some(hello(2))]);
        let (mut a, mut h) = (agent(), Host::default());
        dagon_sim::block_on(serve(&mut link, &mut a, &mut h));
        let order: Vec<&str> = h
            .calls
            .iter()
            .map(std::string::String::as_str)
            .filter(|c| matches!(*c, "feed" | "answered"))
            .collect();
        assert_eq!(order, ["feed", "answered", "feed", "answered"]);
    }

    /// The link is up from its first frame and down once it closes; each
    /// request is reported with its reply.
    #[test]
    fn link_and_requests_are_reported() {
        let probe = Probe::default();
        let mut a = probed(probe.clone());
        let mut link = Mock::default();
        link.incoming.extend([None, Some(hello(1))]);
        dagon_sim::block_on(serve(&mut link, &mut a, &mut Host::default()));
        let seen = probe.seen.borrow();
        let start = seen.iter().position(|k| *k == "store_opened").unwrap() + 1;
        assert_eq!(
            seen[start..],
            ["link_up", "request", "hello", "replied", "link_down"]
        );
        assert_eq!(seen[0], "starting");
    }

    /// A REBOOT whose reply could not be written resets nothing: the link
    /// is down, and the next frame brings it up and is answered.
    #[test]
    fn no_reboot_when_the_reply_was_not_written() {
        let probe = Probe::default();
        let mut a = probed(probe.clone());
        let mut link = Mock {
            fail_write: true,
            ..Mock::default()
        };
        let reboot = request(3, Cmd::Reboot as u8, &[reboot_mode::NORMAL]);
        link.incoming.extend([Some(reboot)]);
        let mut h = Host::default();
        let exit = dagon_sim::block_on(serve(&mut link, &mut a, &mut h));
        assert_eq!(exit, Exit::Closed);
        assert_eq!(link.flushed, 0);
        assert!(!h.calls.iter().any(|c| c.starts_with("wait")));
        {
            let seen = probe.seen.borrow();
            assert!(!seen.contains(&"before_reboot"), "{seen:?}");
            let at = seen.iter().position(|k| *k == "reboot_requested").unwrap();
            assert_eq!(seen[at + 1..], ["link_down"]);
        }
        // The peer's retry (same seq) on a working link gets the reply.
        link.fail_write = false;
        link.incoming
            .extend([Some(request(3, Cmd::Reboot as u8, &[reboot_mode::NORMAL]))]);
        let exit = dagon_sim::block_on(serve(&mut link, &mut a, &mut h));
        assert_eq!((exit, link.written.len()), (Exit::Reboot, 1));
    }

    /// A REBOOT whose reply could not be flushed resets nothing either.
    #[test]
    fn no_reboot_when_the_reply_was_not_flushed() {
        let probe = Probe::default();
        let mut a = probed(probe.clone());
        let mut link = Mock {
            fail_flush: true,
            ..Mock::default()
        };
        let reboot = request(3, Cmd::Reboot as u8, &[reboot_mode::NORMAL]);
        link.incoming.extend([Some(reboot), Some(hello(4))]);
        let mut h = Host::default();
        let exit = dagon_sim::block_on(serve(&mut link, &mut a, &mut h));
        assert_eq!(exit, Exit::Closed);
        assert_eq!((link.written.len(), link.flushed), (2, 1));
        assert!(!h.calls.iter().any(|c| c.starts_with("wait")));
        assert!(!probe.seen.borrow().contains(&"before_reboot"));
    }
}
