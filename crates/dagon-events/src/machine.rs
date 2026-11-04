//! `state_machine!`: one declaration per machine, from which its state,
//! its transition methods, its event payloads and its catalog are made.

/// Declares a crate's state machines, whose transitions are the only way
/// to change their state, each delivering its event.
///
/// A crate invokes it once, at its root, with all its machines (none is
/// allowed); it defines `MACHINES`, the crate's [`crate::Machines`], which
/// the catalog registry lists. A second invocation in the same crate does
/// not compile, so no machine is left out of `MACHINES`; a machine under
/// `#[cfg]` that is not compiled is not in it either.
///
/// ```compile_fail,E0428
/// dagon_events::state_machine! {}
/// mod more {
///     dagon_events::state_machine! {}
/// }
/// ```
///
/// ```
/// use dagon_events::{compose_unpublished, state_machine, Layer, Nop};
///
/// state_machine! {
///     /// A transfer into one slot.
///     pub machine transfer in App {
///         initial Idle;
///         states {
///             Idle,
///             Body { slot: u8, len: u32 },
///         }
///         transitions {
///             /// An update was offered and passed every guard.
///             offered(slot: u8, len: u32): Idle => Body { slot, len }
///                 emits UpdateOffered = 0x10 { slot: u8, body_len: u32 = len }
///                 observe(live, host);
///             /// A chunk was written.
///             written(done: u32): Body { slot, len } => Body { slot, len }
///                 emits TransferProgress = 0x11 { done: u32, total: u32 = len }
///                 observe(live, host);
///             /// The transfer was dropped.
///             aborted(): * => Idle
///                 emits TransferAborted = 0x12 {}
///                 observe(live, host);
///         }
///     }
/// }
///
/// let mut chain = compose_unpublished(Layer::App, Nop, Nop);
/// let mut m = transfer::Machine::new();
/// m.offered(&mut chain, 1, 4096).unwrap();
/// assert_eq!(m.state(), transfer::State::Body { slot: 1, len: 4096 });
/// assert_eq!(transfer::UpdateOffered::ENTRY.kind, "update_offered");
/// assert!(m.offered(&mut chain, 1, 1).is_err()); // not from Body
/// ```
///
/// What it generates for each machine, in the module `name`:
///
/// - `State`, the states, and `Machine`, which holds one in a private
///   field. `Machine::new()` (initial state) is visible only in the crate
///   that declares the machine; `Machine` has no `Default` and no `Clone`.
/// - One transition method per transition. It takes the layer's composed
///   chain ([`crate::Sink`]); from a state it leaves from it builds the
///   event and the next state, delivers the event to the chain, then sets
///   the state. From any other state it returns [`crate::Refused`] and
///   changes and delivers nothing. Guards beyond the state (a digest, a
///   signature) are the caller's, before it calls the transition.
/// - One payload struct per transition: `#[non_exhaustive]` (only the
///   declaring crate builds one), neither `Copy` nor `Clone`, implementing
///   [`crate::Payload`]; its owned form is its fields' [`crate::Compact`]
///   forms in order.
/// - `CATALOG`, the machine's [`crate::Catalog`], and `ENTRY` on each
///   payload type.
/// - Decoding of the machine's [`crate::EventRecord`]s back into its
///   payloads, which the layer's replay ([`crate::Composed::replay`])
///   uses.
/// - A test, `every_transition`, that calls every transition from every
///   state (fields and arguments at their `Default`) and checks it moves
///   exactly from its `from` states to its `to` state and delivers exactly
///   one event when it does.
///
/// A `from` state is a state name, optionally with the names of fields it
/// binds (`Body { len }`); it never filters on values. `*` is any state.
/// The event kind is the payload type's name in snake case.
///
/// Rules, each a compile error when broken:
///
/// - Every transition `emits` one event with a code (`u8`, unique within
///   the machine; across a layer the registry's test keeps codes unique)
///   and at least one `observe` path (`live`, `replay`, `host`). Further
///   flags after it: `action`, `retired`.
/// - Every state other than `initial` is reached by some transition.
/// - A payload's owned form fits [`crate::PAYLOAD_BYTES`].
///
/// A transition without `emits`:
///
/// ```compile_fail
/// dagon_events::state_machine! {
///     machine m in App {
///         initial A;
///         states { A, B }
///         transitions { go(): A => B; }
///     }
/// }
/// ```
///
/// A state no transition reaches:
///
/// ```compile_fail,E0080
/// dagon_events::state_machine! {
///     machine m in App {
///         initial A;
///         states { A, B, C }
///         transitions { go(): A => B emits Went = 1 {} observe(live); }
///     }
/// }
/// ```
///
/// Two transitions with one code:
///
/// ```compile_fail,E0080
/// dagon_events::state_machine! {
///     machine m in App {
///         initial A;
///         states { A, B }
///         transitions {
///             go(): A => B emits Went = 1 {} observe(live);
///             back(): B => A emits Back = 1 {} observe(live);
///         }
///     }
/// }
/// ```
///
/// A payload larger than its record:
///
/// ```compile_fail,E0080
/// dagon_events::state_machine! {
///     machine m in App {
///         initial A;
///         states { A, B }
///         transitions { go(d: [u8; 17]): A => B emits Went = 1 { d: [u8; 17] } observe(live); }
///     }
/// }
/// ```
///
/// An event with no observer path, or an unknown one:
///
/// ```compile_fail
/// dagon_events::state_machine! {
///     machine m in App {
///         initial A;
///         states { A, B }
///         transitions { go(): A => B emits Went = 1 {} observe(); }
///     }
/// }
/// ```
///
/// ```compile_fail,E0560
/// dagon_events::state_machine! {
///     machine m in App {
///         initial A;
///         states { A, B }
///         transitions { go(): A => B emits Went = 1 {} observe(radio); }
///     }
/// }
/// ```
///
/// A `from` that filters on a value:
///
/// ```compile_fail
/// dagon_events::state_machine! {
///     machine m in App {
///         initial A;
///         states { A { n: u8 }, B }
///         transitions { go(): A { n: 0 } => B emits Went = 1 {} observe(live); }
///     }
/// }
/// ```
///
/// A state set outside the transitions, from another crate: a machine
/// cannot be built, rebuilt, reset or cloned there.
///
/// ```compile_fail,E0451
/// use dagon_events::decisions::{Machine, State};
/// let forged = Machine { state: State::Ready };
/// ```
///
/// ```compile_fail,E0624
/// let fresh = dagon_events::decisions::Machine::new();
/// ```
///
/// ```compile_fail,E0277
/// fn reset(m: &mut dagon_events::decisions::Machine) {
///     let _old = core::mem::take(m);
/// }
/// ```
///
/// ```compile_fail,E0308
/// fn copy(m: &dagon_events::decisions::Machine) -> dagon_events::decisions::Machine {
///     m.clone()
/// }
/// ```
///
/// An event built outside the crate that declares it:
///
/// ```compile_fail,E0639
/// use dagon_events::decisions::ExtensionOverran;
/// use dagon_events::{DecisionId, Party};
/// let forged = ExtensionOverran { point: DecisionId::SelfCheck, party: Party::User, us: 1 };
/// ```
#[macro_export]
macro_rules! state_machine {
    (
        $(
            $(#[doc = $d:literal])*
            $vis:vis machine $name:ident in $layer:ident { $($body:tt)* }
        )*
    ) => {
        $(
            $crate::__machine! {
                $(#[doc = $d])*
                $vis machine $name in $layer { $($body)* }
            }
        )*

        /// The machines this crate declares, for the catalog registry.
        pub const MACHINES: $crate::Machines = $crate::Machines {
            krate: env!("CARGO_PKG_NAME"),
            catalogs: &[$($name::CATALOG),*],
        };

        // A second `state_machine!` in the crate defines this again.
        #[doc(hidden)]
        #[macro_export]
        macro_rules! __dagon_events_state_machine_once_per_crate {
            () => {};
        }
    };
}

/// One machine of [`state_machine!`].
#[doc(hidden)]
#[macro_export]
macro_rules! __machine {
    (
        $(#[doc = $mdoc:literal])*
        $vis:vis machine $name:ident in $layer:ident {
            initial $initial:ident;
            states {
                $( $(#[$sdoc:meta])* $S:ident $({ $($sf:ident : $st:ty),* $(,)? })? ),+ $(,)?
            }
            transitions {
                $(
                    $(#[doc = $tdoc:literal])*
                    $trig:ident ( $($arg:ident : $argty:ty),* $(,)? ) :
                    $( $from:tt $({ $($fb:ident),* $(,)? })? )|+
                    => $to:ident $({ $($tf:ident $(: $te:expr)?),* $(,)? })?
                    emits $ev:ident = $code:literal { $($pf:ident : $pt:ty $(= $pe:expr)?),* $(,)? }
                    observe ( $($obs:ident),+ $(,)? ) $($flag:ident)* ;
                )+
            }
        }
    ) => {
        $(#[doc = $mdoc])*
        $vis mod $name {
            #[allow(unused_imports)]
            use super::*;

            /// The machine's states.
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum State {
                $( $(#[$sdoc])* $S $({ $($sf: $st),* })? ),+
            }

            impl State {
                pub fn name(&self) -> &'static str {
                    match self {
                        $( State::$S { .. } => stringify!($S), )+
                    }
                }
            }

            /// The machine. Its state changes only through the transition
            /// methods, each of which delivers its event first.
            #[derive(Debug)]
            pub struct Machine {
                state: State,
            }

            impl Machine {
                /// A machine in its initial state; only the declaring
                /// crate makes one.
                #[allow(dead_code)]
                pub(crate) const fn new() -> Self {
                    Machine { state: State::$initial }
                }

                pub fn state(&self) -> State {
                    self.state
                }

                $(
                    $(#[doc = $tdoc])*
                    pub fn $trig(
                        &mut self,
                        out: &mut impl $crate::Sink,
                        $($arg: $argty),*
                    ) -> ::core::result::Result<(), $crate::Refused> {
                        let (next, event) = match self.state {
                            $( $crate::__sm_from!($from $({ $($fb),* })?) )|+ => (
                                State::$to $({ $($tf $(: $te)?),* })?,
                                $ev { $($pf $(: $pe)?),* },
                            ),
                            #[allow(unreachable_patterns)]
                            _ => {
                                return ::core::result::Result::Err($crate::Refused {
                                    machine: stringify!($name),
                                    trigger: stringify!($trig),
                                    state: self.state.name(),
                                })
                            }
                        };
                        $crate::Sink::deliver(out, event);
                        self.state = next;
                        ::core::result::Result::Ok(())
                    }
                )+
            }

            /// The machine's catalog: its states and every transition.
            pub const CATALOG: $crate::Catalog = $crate::Catalog {
                machine: stringify!($name),
                layer: $crate::Layer::$layer,
                doc: concat!("" $(, $mdoc, "\n")*),
                initial: stringify!($initial),
                states: &[$(stringify!($S)),+],
                entries: &[$($ev::ENTRY),+],
                __replay,
            };

            fn __replay(
                rec: &$crate::EventRecord,
                deliver: &mut dyn FnMut(&dyn $crate::Payload),
            ) -> bool {
                if rec.layer != $crate::Layer::$layer.code() {
                    return false;
                }
                $(
                    if rec.kind == $code {
                        return match $ev::__decode(&rec.payload) {
                            ::core::option::Option::Some(p) => {
                                deliver(&p);
                                true
                            }
                            ::core::option::Option::None => false,
                        };
                    }
                )+
                false
            }

            $(
                $(#[doc = $tdoc])*
                #[derive(Debug, PartialEq)]
                #[non_exhaustive]
                pub struct $ev {
                    $(pub $pf: $pt),*
                }

                // `observe(live, replay, host)` names every field.
                #[allow(clippy::needless_update)]
                impl $ev {
                    pub const ENTRY: $crate::Entry = $crate::Entry {
                        layer: $crate::Layer::$layer,
                        machine: stringify!($name),
                        kind: {
                            const NAME: &str = stringify!($ev);
                            const KIND: [u8; $crate::__private::snake_len(NAME)] =
                                $crate::__private::snake(NAME);
                            match ::core::str::from_utf8(&KIND) {
                                ::core::result::Result::Ok(s) => s,
                                ::core::result::Result::Err(_) => panic!("kind is ASCII"),
                            }
                        },
                        code: $code,
                        trigger: stringify!($trig),
                        from: &[$(stringify!($from)),+],
                        to: stringify!($to),
                        payload: &[$($crate::Field {
                            name: stringify!($pf),
                            ty: stringify!($pt),
                            bytes: <$pt as $crate::Compact>::SIZE,
                        }),*],
                        observe: $crate::Observe { $($obs: true,)+ ..$crate::Observe::NONE },
                        flags: $crate::Flags { $($flag: true,)* ..$crate::Flags::NONE },
                        doc: concat!("" $(, $tdoc, "\n")*),
                    };
                }

                impl $crate::__private::Generated for $ev {}

                impl $ev {
                    #[allow(unused_mut, unused_variables, unused_assignments)]
                    fn __decode(b: &[u8; $crate::PAYLOAD_BYTES]) -> ::core::option::Option<Self> {
                        let mut at = 0usize;
                        $(
                            let n = <$pt as $crate::Compact>::SIZE;
                            let $pf = <$pt as $crate::Compact>::get(&b[at..at + n])?;
                            at += n;
                        )*
                        ::core::option::Option::Some($ev { $($pf),* })
                    }
                }

                impl $crate::Payload for $ev {
                    fn entry(&self) -> &'static $crate::Entry {
                        &Self::ENTRY
                    }

                    #[allow(unused_mut, unused_variables, unused_assignments)]
                    fn compact(&self) -> [u8; $crate::PAYLOAD_BYTES] {
                        let mut out = [0u8; $crate::PAYLOAD_BYTES];
                        let mut at = 0usize;
                        $(
                            let n = <$pt as $crate::Compact>::SIZE;
                            $crate::Compact::put(&self.$pf, &mut out[at..at + n]);
                            at += n;
                        )*
                        out
                    }
                }

                const _: () = assert!(
                    $crate::PAYLOAD_BYTES
                        .checked_sub(0 $(+ <$pt as $crate::Compact>::SIZE)*)
                        .is_some(),
                    concat!(stringify!($ev), ": the payload's owned form exceeds PAYLOAD_BYTES")
                );
            )+

            const _: () = assert!(
                $crate::__private::unique_codes(&[$($code),+]),
                concat!(stringify!($name), ": two transitions share an event code")
            );

            const _: () = {
                let reached: &[&str] = &[$(stringify!($to)),+];
                $(
                    assert!(
                        $crate::__private::contains(&[stringify!($initial)], stringify!($S))
                            || $crate::__private::contains(reached, stringify!($S)),
                        concat!(stringify!($name), ": no transition reaches state ", stringify!($S))
                    );
                )+
            };

            #[cfg(test)]
            #[test]
            fn every_transition() {
                /// Events seen, the last one's record, and how many were replays.
                struct Count(usize, $crate::EventRecord, usize);
                impl $crate::Extension for Count {
                    fn id(&self) -> &'static str {
                        "count"
                    }
                    fn on_event(&mut self, e: &$crate::Event<'_>, _: &$crate::Ctx<'_>) {
                        self.0 += 1;
                        self.1 = e.record(0);
                        self.2 += usize::from(e.is_replayed());
                    }
                }
                let all: &[State] = &[
                    $( State::$S $({ $($sf: ::core::default::Default::default()),* })? ),+
                ];
                $(
                    for s in all {
                        let mut seen = Count(0, ::core::default::Default::default(), 0);
                        let mut m = Machine { state: *s };
                        let from: &[&str] = &[$(stringify!($from)),+];
                        let leaves = from.contains(&"*") || from.contains(&s.name());
                        let r = {
                            let mut out = $crate::compose_unpublished(
                                $crate::Layer::$layer, $crate::Nop, &mut seen,
                            );
                            m.$trig(&mut out, $(<$argty as ::core::default::Default>::default()),*)
                        };
                        if leaves {
                            r.unwrap_or_else(|e| panic!("{e}"));
                            assert_eq!(m.state().name(), stringify!($to), "{}", stringify!($trig));
                            assert_eq!(seen.0, 1, "{} delivers one event", stringify!($trig));
                            let rec = seen.1;
                            assert_eq!(rec.kind, $code);
                            let replayed = {
                                let mut out = $crate::compose_unpublished(
                                    $crate::Layer::$layer, $crate::Nop, &mut seen,
                                );
                                out.replay(&[CATALOG], &rec)
                            };
                            assert!(replayed.is_ok(), "{} replays", stringify!($trig));
                            assert_eq!((seen.0, seen.2), (2, 1), "{} replays once", stringify!($trig));
                            assert_eq!(seen.1.payload, rec.payload, "{} decodes", stringify!($trig));
                        } else {
                            assert!(r.is_err(), "{} from {}", stringify!($trig), s.name());
                            assert_eq!(m.state(), *s, "a refused {} changed the state", stringify!($trig));
                            assert_eq!(seen.0, 0, "a refused {} delivered an event", stringify!($trig));
                        }
                    }
                )+
            }
        }
    };
}

/// A `from` state of [`state_machine!`] as a pattern on `State`.
#[doc(hidden)]
#[macro_export]
macro_rules! __sm_from {
    (*) => { _ };
    ($s:ident { $($b:ident),* }) => { State::$s { $($b,)* .. } };
    ($s:ident) => { State::$s { .. } };
}

/// Length of `name` (CamelCase ASCII) in snake case.
pub const fn snake_len(name: &str) -> usize {
    let b = name.as_bytes();
    let mut n = b.len();
    let mut i = 1;
    while i < b.len() {
        if b[i].is_ascii_uppercase() {
            n += 1;
        }
        i += 1;
    }
    n
}

/// `name` (CamelCase ASCII) in snake case; `N` is [`snake_len`].
pub const fn snake<const N: usize>(name: &str) -> [u8; N] {
    let b = name.as_bytes();
    let mut out = [0u8; N];
    let (mut i, mut o) = (0, 0);
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_uppercase() {
            if i > 0 {
                out[o] = b'_';
                o += 1;
            }
            out[o] = c.to_ascii_lowercase();
        } else {
            out[o] = c;
        }
        o += 1;
        i += 1;
    }
    out
}

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

pub const fn contains(set: &[&str], s: &str) -> bool {
    let mut i = 0;
    while i < set.len() {
        if str_eq(set[i], s) {
            return true;
        }
        i += 1;
    }
    false
}

pub const fn unique_codes(codes: &[u8]) -> bool {
    let mut i = 0;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_the_type_name_in_snake_case() {
        const K: [u8; snake_len("TransferProgress")] = snake("TransferProgress");
        assert_eq!(&K, b"transfer_progress");
        const L: [u8; snake_len("Entry")] = snake("Entry");
        assert_eq!(&L, b"entry");
    }

    #[test]
    fn code_and_name_checks() {
        assert!(unique_codes(&[1, 2, 3]));
        assert!(!unique_codes(&[1, 2, 1]));
        assert!(contains(&["Idle", "Body"], "Body"));
        assert!(!contains(&["Idle"], "Idl"));
    }
}
