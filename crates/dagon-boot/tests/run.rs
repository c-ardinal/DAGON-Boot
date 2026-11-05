//! A boot core's run with extensions: the events in order, each delivered
//! only to the extensions that observe it, the time limits, and the
//! decision and flash writes no extension changes.

#[path = "../../dagon-core/tests/bench/mod.rs"]
mod bench;

use bench::*;
use dagon_boot::sequence;
use dagon_boot::BootRun;
use dagon_events::{compose_boot, BootExtension, Ctx, Event, Extension, Nop};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const WDT_MS: u32 = 4_000;

/// Codes of single boot kinds the tests name (`dagon gen` resolves
/// `observes` to codes).
const ARMED: u8 = sequence::WatchdogArmed::ENTRY.code;
const BEFORE: u8 = sequence::BeforeDecision::ENTRY.code;
const JUMP: u8 = sequence::BeforeJump::ENTRY.code;
const OVERRAN: u8 = dagon_events::boot_extensions::ExtensionOverran::ENTRY.code;
/// The boot-layer entries of the catalogs the boot core delivers from
/// (its sequence, and the extension-call reports): a transition added to
/// either machine is in every list below.
fn boot_entries() -> Vec<&'static dagon_events::Entry> {
    [&dagon_boot::MACHINES, &dagon_events::MACHINES]
        .iter()
        .flat_map(|m| m.catalogs())
        .filter(|c| c.layer == dagon_events::Layer::Boot)
        .flat_map(|c| c.entries)
        .filter(|e| e.observe.live && !e.flags.retired)
        .collect()
}

/// `boot.*`: every live boot code, as `dagon gen` resolves it.
fn every() -> &'static [u8] {
    Box::leak(boot_entries().iter().map(|e| e.code).collect())
}

/// The sequence's kinds: every transition an extension can be slow on.
fn sequence_kinds() -> Vec<&'static str> {
    dagon_boot::MACHINES
        .catalogs()
        .iter()
        .flat_map(|c| c.entries)
        .map(|e| e.kind)
        .collect()
}

/// `n` extensions observing every boot event, 1 ms per call each.
fn all_1ms(n: usize) -> &'static [BootExtension] {
    Box::leak(vec![BootExtension::new(1_000, every()); n].into_boxed_slice())
}

#[test]
fn the_derived_lists_hold_every_boot_kind() {
    let kinds: Vec<_> = boot_entries().iter().map(|e| e.kind).collect();
    for k in sequence_kinds() {
        assert!(kinds.contains(&k), "{k}");
    }
    assert!(kinds.contains(&"extension_overran"));
    assert_eq!(every().len(), kinds.len());
}

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
    fn clock(&self) -> impl FnMut() -> u64 {
        let c = self.clock.clone();
        move || c.get()
    }
}

fn start(m: &mut Mem, chain: &mut impl dagon_events::Sink) -> BootRun {
    BootRun::start(chain, m, &LOG, &SLOTS, &cfg(), WDT_MS)
}

/// A whole boot: up to the decision, then the jump or the halt.
fn boot_all(m: &mut Mem, chain: &mut impl dagon_events::Sink) -> Decision {
    let mut run = start(m, chain);
    run.before_jump(chain);
    run.halt(chain);
    run.decision()
}

#[test]
fn a_trusted_extension_observes_the_boot_in_order() {
    let b = Bench::new();
    let mut chain = compose_boot(b.probe("show", "", 0), b.clock(), all_1ms(1));
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
fn an_extension_is_called_only_for_the_events_it_observes() {
    const ONE: &[BootExtension] = &[BootExtension::new(1_000, &[BEFORE])];
    // `boot.before_*`.
    const PREFIX: &[BootExtension] = &[BootExtension::new(1_000, &[BEFORE, JUMP])];
    let b = Bench::new();
    let mut chain = compose_boot(b.probe("one", "", 0), b.clock(), ONE);
    boot_all(&mut with_pending_b(), &mut chain);
    assert_eq!(b.kinds("one"), ["boot.before_decision"]);
    let b = Bench::new();
    let mut chain = compose_boot(b.probe("pre", "", 0), b.clock(), PREFIX);
    boot_all(&mut with_pending_b(), &mut chain);
    assert_eq!(b.kinds("pre"), ["boot.before_decision", "boot.before_jump"]);
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
    let mut chain = compose_boot(Fields(seen.clone()), || 0u64, all_1ms(1));
    let mut m = with_pending_b();
    start(&mut m, &mut chain);
    assert_eq!(
        *seen.borrow(),
        ["armed 4000", "before true true 0", "chosen 1 true"]
    );
}

#[test]
fn an_overrun_is_reported_to_its_observers_and_the_decision_stays_the_same() {
    let slow_and_deaf: &'static [BootExtension] = Box::leak(Box::new([
        BootExtension::new(1_000, every()),
        BootExtension::new(1_000, &[ARMED, JUMP]),
        BootExtension::new(1_000, &[OVERRAN]),
    ]));
    let mut plain = with_pending_b();
    let want = boot_all(&mut plain, &mut compose_boot(Nop, || 0u64, &[]));
    let b = Bench::new();
    let mut chain = compose_boot(
        dagon_events::extensions![
            b.probe("slow", "before_decision", 5_000),
            b.probe("deaf", "", 0),
            b.probe("watch", "", 0)
        ],
        b.clock(),
        slow_and_deaf,
    );
    let mut m = with_pending_b();
    assert_eq!(boot_all(&mut m, &mut chain), want);
    assert_eq!(m.bytes, plain.bytes, "the same records written");
    // Reported right after the slow call, to the extensions observing it.
    let all = b.seen.borrow().clone();
    let slow_call = all.iter().position(|x| x == "slow:boot.before_decision");
    let report = all.iter().position(|x| x == "slow:boot.extension_overran");
    assert_eq!(report, slow_call.map(|i| i + 1), "{all:?}");
    assert_eq!(b.kinds("watch"), ["boot.extension_overran"]);
    assert_eq!(b.kinds("deaf"), ["boot.watchdog_armed", "boot.before_jump"]);
    assert_eq!(b.kinds("slow").last().unwrap(), "boot.before_jump");
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
    let two_2ms: &'static [BootExtension] = Box::leak(Box::new([
        BootExtension::new(2_000, every()),
        BootExtension::new(2_000, every()),
    ]));
    let b = Bench::new();
    let got = Rc::new(RefCell::new(Vec::new()));
    let mut chain = compose_boot(
        dagon_events::extensions![b.probe("slow", "log_read", 2_500), Watch(got.clone())],
        b.clock(),
        two_2ms,
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
    let mut chain = compose_boot(
        b.probe("ok", "before_decision", 1_000),
        b.clock(),
        all_1ms(1),
    );
    let mut m = with_pending_b();
    start(&mut m, &mut chain);
    assert!(!b
        .kinds("ok")
        .contains(&"boot.extension_overran".to_string()));
}

#[test]
#[should_panic(expected = "one BootExtension per extension")]
fn a_limit_list_that_does_not_match_the_chain_is_refused() {
    let b = Bench::new();
    compose_boot(
        dagon_events::extensions![b.probe("a", "", 0), b.probe("b", "", 0)],
        b.clock(),
        all_1ms(1),
    );
}

#[test]
fn no_startable_slot_ends_in_the_halt() {
    let b = Bench::new();
    let mut chain = compose_boot(b.probe("show", "", 0), b.clock(), all_1ms(1));
    let mut m = Mem::new();
    assert_eq!(boot_all(&mut m, &mut chain), Decision::NoImage);
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

// The equivalence matrix: for every device state, a boot with extensions
// (each event slow once, or none) decides and writes exactly what the
// boot without extensions does, and what dagon-core's `decide` does.

/// A confirmed, B pending: a trial of B.
fn pending_trial() -> Mem {
    with_pending_b()
}

/// A confirmed, B pending with a body that does not match its header:
/// the boot falls back to A.
fn confirmed_fallback() -> Mem {
    let mut m = with_pending_b();
    let body = SLOTS.slots[B as usize] + HEADER_SIZE as u32;
    m.put(body, &[0x5A; 16]);
    m
}

/// A confirmed, B pending, its attempts used up: rollback to A.
fn exhausted() -> Mem {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    let b = install(&mut m, B, &image(B, 1));
    write_log(
        &mut m,
        0,
        1,
        &[
            (kind::PROVISION, a),
            (kind::CONFIRMED, a),
            (kind::SLOT_PENDING, b),
            (kind::BOOT_ATTEMPT, b),
            (kind::BOOT_ATTEMPT, b),
            (kind::BOOT_ATTEMPT, b),
        ],
    );
    m
}

/// The confirmed image's body corrupted: nothing to start.
fn corrupt_image() -> Mem {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    write_log(&mut m, 0, 1, &[(kind::PROVISION, a), (kind::CONFIRMED, a)]);
    let body = SLOTS.slots[A as usize] + HEADER_SIZE as u32;
    m.put(body, &[0xA5; 32]);
    m
}

/// Both log faces garbage: no valid face, nothing to start.
fn corrupt_log() -> Mem {
    let mut m = with_pending_b();
    for face in LOG.faces {
        m.erase(face, LOG.face_len).unwrap();
        m.put(face, &[0x13; 64]);
    }
    m
}

/// A confirmed only.
fn confirmed_only() -> Mem {
    let mut m = Mem::new();
    let a = install(&mut m, A, &image(A, 0));
    write_log(&mut m, 0, 1, &[(kind::PROVISION, a), (kind::CONFIRMED, a)]);
    m
}

/// A named device state the matrix boots.
type State = (&'static str, fn() -> Mem);

const STATES: &[State] = &[
    ("pending trial", pending_trial),
    ("confirmed fallback", confirmed_fallback),
    ("exhausted attempts", exhausted),
    ("corrupt image", corrupt_image),
    ("corrupt log", corrupt_log),
    ("confirmed only", confirmed_only),
    ("erased", Mem::new),
];

#[test]
fn extensions_change_no_decision_and_no_write_in_any_state() {
    // None slow, then slow on each transition of the sequence.
    let slow_on: Vec<&str> = std::iter::once("").chain(sequence_kinds()).collect();
    let mut overran_at = std::collections::BTreeSet::new();
    for (name, make) in STATES {
        // dagon-core alone.
        let mut core = make();
        let want = match Log::load(&mut core, &LOG) {
            Ok(mut log) => decide(&mut core, &mut log, &SLOTS, &cfg()),
            Err(_) => Decision::NoImage,
        };
        // The boot core without extensions.
        let mut plain = make();
        assert_eq!(
            boot_all(&mut plain, &mut compose_boot(Nop, || 0u64, &[])),
            want,
            "{name}"
        );
        assert_eq!(plain.bytes, core.bytes, "{name}");
        for slow in &slow_on {
            let b = Bench::new();
            let mut chain = compose_boot(
                dagon_events::extensions![b.probe("slow", slow, 5_000), b.probe("next", "", 0)],
                b.clock(),
                all_1ms(2),
            );
            let mut m = make();
            assert_eq!(boot_all(&mut m, &mut chain), want, "{name}, slow on {slow}");
            assert_eq!(m.bytes, core.bytes, "{name}, slow on {slow}");
            let overran = b.kinds("next").contains(&"boot.extension_overran".into());
            let reached = b.kinds("slow").iter().any(|k| k == &format!("boot.{slow}"));
            assert_eq!(overran, reached, "{name}, slow on {slow}");
            if overran {
                overran_at.insert(*slow);
            }
        }
    }
    // Every transition was overrun in some state: a new one the states
    // never reach fails here until a state reaching it is added.
    for k in sequence_kinds() {
        assert!(overran_at.contains(k), "no state reaches boot.{k}");
    }
}
