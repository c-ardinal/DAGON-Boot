//! Re-attach on a fake clock and a fake pull-up: a node whose host never
//! resets the bus drops off and comes back, a bounded number of times; a
//! node whose host resets the bus is left alone.

use dagon_mcu_common::reattach::{Pullup, Reattach, DETACH_MS, SPAN_MS, TRIES, WAIT_MS};

#[derive(Debug, PartialEq)]
enum Op {
    Attach(u64),
    Detach(u64),
}

/// Records each hook call with the fake clock's time.
struct FakePullup<'a> {
    now: &'a std::cell::Cell<u64>,
    ops: Vec<Op>,
}

impl Pullup for FakePullup<'_> {
    fn attach(&mut self) {
        self.ops.push(Op::Attach(self.now.get()));
    }
    fn detach(&mut self) {
        self.ops.push(Op::Detach(self.now.get()));
    }
}

/// Runs the decision from an attach at 0 ms until it stops, jumping the
/// clock to each time it asks for; `reset_at` is when the host resets the
/// bus (bus resets counted from then on). Returns the hook calls and when
/// it stopped.
fn run(reset_at: Option<u64>) -> (Vec<Op>, u64) {
    let now = std::cell::Cell::new(0);
    let mut pullup = FakePullup {
        now: &now,
        ops: Vec::new(),
    };
    let resets = |t: u64| u32::from(reset_at.is_some_and(|r| t >= r));
    let mut r = Reattach::new(0, resets(0));
    let mut steps = 0;
    while let Some(next) = r.step(now.get(), resets(now.get()), &mut pullup) {
        assert!(next >= now.get(), "asks to be called back in the past");
        now.set(next);
        steps += 1;
        assert!(steps < 100, "never stops");
    }
    (pullup.ops, now.get())
}

#[test]
fn reattaches_when_no_bus_reset_arrives() {
    let (ops, _) = run(None);
    assert_eq!(ops[0], Op::Detach(WAIT_MS));
    assert_eq!(ops[1], Op::Attach(WAIT_MS + DETACH_MS));
}

#[test]
fn leaves_the_bus_alone_when_the_host_resets_it() {
    let (ops, end) = run(Some(100));
    assert!(ops.is_empty(), "{ops:?}");
    assert!(end <= WAIT_MS);
}

#[test]
fn stops_after_a_reset_that_follows_a_reattach() {
    let (ops, _) = run(Some(WAIT_MS + DETACH_MS + 100));
    assert_eq!(ops, [Op::Detach(WAIT_MS), Op::Attach(WAIT_MS + DETACH_MS)]);
}

#[test]
fn tries_stop_at_the_bound_attached_and_within_the_trial_window() {
    let (ops, end) = run(None);
    let detaches = ops.iter().filter(|o| matches!(o, Op::Detach(_))).count();
    let attaches = ops.iter().filter(|o| matches!(o, Op::Attach(_))).count();
    assert_eq!((detaches, attaches), (TRIES as usize, TRIES as usize));
    assert!(matches!(ops.last(), Some(Op::Attach(_))), "ends detached");
    assert_eq!(end, SPAN_MS);
    // The RP2040's `[watchdog] trial_ms`; each MCU package checks its own.
    assert!(SPAN_MS <= 8_000);
}

#[test]
fn constants() {
    assert_eq!((WAIT_MS, TRIES), (1_000, 3));
}

#[test]
fn a_reset_seen_before_the_start_does_not_count() {
    let now = std::cell::Cell::new(0);
    let mut pullup = FakePullup {
        now: &now,
        ops: Vec::new(),
    };
    let mut r = Reattach::new(0, 5);
    assert_eq!(r.step(0, 5, &mut pullup), Some(WAIT_MS));
    now.set(WAIT_MS);
    assert_eq!(r.step(WAIT_MS, 5, &mut pullup), Some(WAIT_MS + DETACH_MS));
    assert_eq!(pullup.ops, [Op::Detach(WAIT_MS)]);
}

#[test]
fn a_reset_while_detached_does_not_count() {
    let now = std::cell::Cell::new(0);
    let mut pullup = FakePullup {
        now: &now,
        ops: Vec::new(),
    };
    let mut r = Reattach::new(0, 0);
    r.step(WAIT_MS, 0, &mut pullup);
    // SE0 with the pull-up off can read as a bus reset on the device.
    now.set(WAIT_MS + DETACH_MS);
    assert_eq!(
        r.step(WAIT_MS + DETACH_MS, 1, &mut pullup),
        Some(2 * WAIT_MS + DETACH_MS)
    );
    now.set(2 * WAIT_MS + DETACH_MS);
    r.step(2 * WAIT_MS + DETACH_MS, 1, &mut pullup);
    assert_eq!(pullup.ops.len(), 3, "{:?}", pullup.ops);
}
