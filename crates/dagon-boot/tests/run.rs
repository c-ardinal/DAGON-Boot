//! A boot core's run with extensions: the events in order, the time
//! limits, and the decision no extension changes.

#[path = "../../dagon-core/tests/bench/mod.rs"]
mod bench;

use bench::*;
use dagon_boot::sequence;
use dagon_boot::BootRun;
use dagon_events::{compose_boot, Ctx, Event, Extension, Nop};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const WDT_MS: u32 = 4_000;

/// Records every event it sees; on `slow_on` it takes `cost_us` of the
/// boot core's clock.
struct Probe {
    name: &'static str,
    seen: Rc<RefCell<Vec<String>>>,
    clock: Rc<Cell<u64>>,
    slow_on: &'static str,
    cost_us: u64,
}

impl Extension for Probe {
    fn id(&self) -> &'static str {
        self.name
    }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        self.seen
            .borrow_mut()
            .push(format!("{}:{}.{}", self.name, e.layer().name(), e.kind()));
        if e.kind() == self.slow_on {
            self.clock.set(self.clock.get() + self.cost_us);
        }
    }
}

struct Bench {
    seen: Rc<RefCell<Vec<String>>>,
    clock: Rc<Cell<u64>>,
}

impl Bench {
    fn new() -> Self {
        Bench {
            seen: Rc::default(),
            clock: Rc::default(),
        }
    }
    fn probe(&self, name: &'static str, slow_on: &'static str, cost_us: u64) -> Probe {
        Probe {
            name,
            seen: self.seen.clone(),
            clock: self.clock.clone(),
            slow_on,
            cost_us,
        }
    }
    fn kinds(&self, name: &str) -> Vec<String> {
        self.seen
            .borrow()
            .iter()
            .filter_map(|s| s.strip_prefix(&format!("{name}:")).map(String::from))
            .collect()
    }
}

fn start(m: &mut Mem, chain: &mut impl dagon_events::Sink) -> BootRun {
    BootRun::start(chain, m, &LOG, &SLOTS, &cfg(), WDT_MS)
}

#[test]
fn a_trusted_extension_observes_the_boot_in_order() {
    let b = Bench::new();
    let clock = b.clock.clone();
    let mut chain = compose_boot(b.probe("show", "", 0), move || clock.get(), &[1_000]);
    let mut m = with_pending_b();
    let mut run = start(&mut m, &mut chain);
    run.before_jump(&mut chain);
    assert_eq!(
        b.kinds("show"),
        [
            "boot.watchdog_armed",
            "boot.log_read",
            "boot.before_decision",
            "boot.slot_chosen",
            "boot.before_jump"
        ]
    );
    assert_eq!(
        run.decision(),
        Decision::Boot {
            slot: B,
            entry: entry(B),
            trial: true
        }
    );
}

#[test]
fn the_payloads_say_what_the_boot_core_saw() {
    struct Fields(Rc<RefCell<Vec<String>>>);
    impl Extension for Fields {
        fn id(&self) -> &'static str {
            "fields"
        }
        fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
            let mut v = self.0.borrow_mut();
            if let Some(p) = e.get::<sequence::WatchdogArmed>() {
                v.push(format!("armed {}", p.period_ms));
            }
            if let Some(p) = e.get::<sequence::BeforeDecision>() {
                v.push(format!(
                    "before {} {} {}",
                    p.pending, p.confirmed, p.attempts
                ));
            }
            if let Some(p) = e.get::<sequence::SlotChosen>() {
                v.push(format!("chosen {} {}", p.slot, p.trial));
            }
        }
    }
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut chain = compose_boot(Fields(seen.clone()), || 0u64, &[1_000]);
    let mut m = with_pending_b();
    start(&mut m, &mut chain);
    assert_eq!(
        *seen.borrow(),
        ["armed 4000", "before true true 0", "chosen 1 true"]
    );
}

#[test]
fn an_overrun_is_reported_and_the_decision_stays_the_same() {
    // Reference: the same device booted without extensions.
    let mut plain = with_pending_b();
    let want = start(&mut plain, &mut compose_boot(Nop, || 0u64, &[])).decision();
    let b = Bench::new();
    let clock = b.clock.clone();
    let mut chain = compose_boot(
        dagon_events::extensions![
            b.probe("slow", "before_decision", 5_000),
            b.probe("next", "", 0)
        ],
        move || clock.get(),
        &[1_000, 1_000],
    );
    let mut m = with_pending_b();
    let mut run = start(&mut m, &mut chain);
    run.before_jump(&mut chain);
    assert_eq!(run.decision(), want);
    assert_eq!(state(&mut m), state(&mut plain), "the same records written");
    // Reported right after the slow call, to every extension.
    for name in ["slow", "next"] {
        let k = b.kinds(name);
        let at = k.iter().position(|x| x == "boot.extension_overran");
        assert!(at.is_some(), "{name}: {k:?}");
        assert_eq!(k.last().unwrap(), "boot.before_jump", "the boot went on");
    }
    let all = b.seen.borrow();
    let slow_call = all.iter().position(|x| x == "slow:boot.before_decision");
    let report = all.iter().position(|x| x == "slow:boot.extension_overran");
    assert_eq!(report, slow_call.map(|i| i + 1));
}

#[test]
fn the_overrun_names_the_extension_and_its_budget() {
    struct Watch(Rc<RefCell<Vec<(u64, u32, u32)>>>);
    impl Extension for Watch {
        fn id(&self) -> &'static str {
            "watch"
        }
        fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
            if let Some(p) = e.get::<dagon_events::boot_extensions::ExtensionOverran>() {
                self.0.borrow_mut().push((p.id.hash, p.budget_us, p.us));
            }
        }
    }
    let b = Bench::new();
    let clock = b.clock.clone();
    let got = Rc::new(RefCell::new(Vec::new()));
    let mut chain = compose_boot(
        dagon_events::extensions![b.probe("slow", "log_read", 2_500), Watch(got.clone())],
        move || clock.get(),
        &[2_000, 2_000],
    );
    let mut m = with_pending_b();
    start(&mut m, &mut chain);
    assert_eq!(
        *got.borrow(),
        [(dagon_events::ExtId::of("slow").hash, 2_000, 2_500)]
    );
}

#[test]
fn a_call_within_its_budget_is_not_reported() {
    let b = Bench::new();
    let clock = b.clock.clone();
    let mut chain = compose_boot(
        b.probe("ok", "before_decision", 1_000),
        move || clock.get(),
        &[1_000],
    );
    let mut m = with_pending_b();
    start(&mut m, &mut chain);
    assert!(!b
        .kinds("ok")
        .contains(&"boot.extension_overran".to_string()));
}

#[test]
fn no_startable_slot_ends_in_the_halt() {
    let b = Bench::new();
    let clock = b.clock.clone();
    let mut chain = compose_boot(b.probe("show", "", 0), move || clock.get(), &[1_000]);
    let mut m = Mem::new();
    let mut run = start(&mut m, &mut chain);
    run.before_jump(&mut chain);
    run.halt(&mut chain);
    assert_eq!(run.decision(), Decision::NoImage);
    assert_eq!(
        b.kinds("show"),
        [
            "boot.watchdog_armed",
            "boot.log_read",
            "boot.before_decision",
            "boot.no_image",
            "boot.halt"
        ]
    );
}

#[test]
fn without_extensions_the_boot_decides_as_before() {
    for make in [with_pending_b as fn() -> Mem, Mem::new] {
        let (mut a, mut b) = (make(), make());
        let run = start(&mut a, &mut compose_boot(Nop, || 0u64, &[]));
        let d = match Log::load(&mut b, &LOG) {
            Ok(mut log) => decide(&mut b, &mut log, &SLOTS, &cfg()),
            Err(_) => Decision::NoImage,
        };
        assert_eq!(run.decision(), d);
        assert_eq!(a.bytes, b.bytes);
    }
}
