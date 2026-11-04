//! `state_machine!`: one declaration per machine, from which its state,
//! its transition methods, its event payloads and its catalog are made.

/// Declares a state machine whose transitions are the only way to change
/// its state, each emitting its event.
///
/// ```
/// use dagon_events::state_machine;
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
/// let mut m = transfer::Machine::new();
/// let ev = m.offered(1, 4096).unwrap();
/// assert_eq!(ev.body_len, 4096);
/// assert_eq!(transfer::UpdateOffered::ENTRY.kind, "update_offered");
/// assert!(m.offered(1, 1).is_err()); // not from Body
/// ```
///
/// What it generates, in the module `name`:
///
/// - `State`, the states; `Machine`, which holds one in a private field
///   and changes it only in the transition methods. A transition called
///   from a state it does not leave from returns [`crate::Refused`] and
///   changes nothing. Guards beyond the state (a digest, a signature) are
///   the caller's, before it calls the transition.
/// - One payload struct per transition, `#[non_exhaustive]` (only the
///   declaring crate constructs it) and `#[must_use]`; the transition
///   method returns it. It implements [`crate::Payload`], and its owned
///   form is its fields' [`crate::Compact`] forms in order.
/// - `CATALOG`, the machine's [`crate::Catalog`], and `ENTRY` on each
///   payload type.
/// - A test, `every_transition`, that calls every transition from every
///   state (fields and arguments at their `Default`) and checks it moves
///   exactly from its `from` states to its `to` state.
///
/// Rules, each a compile error when broken:
///
/// - Every transition `emits` one event with a code (`u8`, unique within
///   the machine; the layer's codes are kept unique by the catalog's
///   registry) and at least one `observe` path (`live`, `replay`, `host`).
///   Further flags after it: `action`, `retired`.
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
/// A state set outside the transitions:
///
/// ```compile_fail,E0451
/// dagon_events::state_machine! {
///     pub machine m in App {
///         initial A;
///         states { A, B }
///         transitions { go(): A => B emits Went = 1 {} observe(live); }
///     }
/// }
/// let forged = m::Machine { state: m::State::B };
/// ```
///
/// State fields and payload fields are `Copy`; `from` patterns bind state
/// fields (`Body { len, .. }`) and do not filter on their values. The
/// event kind is the payload type's name in snake case.
#[macro_export]
macro_rules! state_machine {
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
                    $( $from:tt $({ $($fb:tt)* })? )|+
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
            /// methods, each of which returns its event.
            #[derive(Debug)]
            pub struct Machine {
                state: State,
            }

            impl Default for Machine {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl Machine {
                pub const fn new() -> Self {
                    Machine { state: State::$initial }
                }

                pub fn state(&self) -> State {
                    self.state
                }

                $(
                    $(#[doc = $tdoc])*
                    #[must_use = "a transition's event must be handed on"]
                    pub fn $trig(&mut self, $($arg: $argty),*) -> ::core::result::Result<$ev, $crate::Refused> {
                        match self.state {
                            $( $crate::__sm_from!($from $({ $($fb)* })?) )|+ => {
                                self.state = State::$to $({ $($tf $(: $te)?),* })?;
                                ::core::result::Result::Ok($ev { $($pf $(: $pe)?),* })
                            }
                            #[allow(unreachable_patterns)]
                            _ => ::core::result::Result::Err($crate::Refused {
                                machine: stringify!($name),
                                trigger: stringify!($trig),
                                state: self.state.name(),
                            }),
                        }
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
            };

            $(
                $(#[doc = $tdoc])*
                #[derive(Clone, Copy, Debug, PartialEq)]
                #[non_exhaustive]
                #[must_use = "an event must be handed on"]
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
                let all: &[State] = &[
                    $( State::$S $({ $($sf: ::core::default::Default::default()),* })? ),+
                ];
                $(
                    for s in all {
                        let mut m = Machine { state: *s };
                        let from: &[&str] = &[$(stringify!($from)),+];
                        let leaves = from.contains(&"*") || from.contains(&s.name());
                        let r = m.$trig($(<$argty as ::core::default::Default>::default()),*);
                        if leaves {
                            let ev = r.unwrap_or_else(|e| panic!("{e}"));
                            assert_eq!(m.state().name(), stringify!($to), "{}", stringify!($trig));
                            assert_eq!($crate::Payload::entry(&ev).trigger, stringify!($trig));
                        } else {
                            assert!(r.is_err(), "{} from {}", stringify!($trig), s.name());
                            assert_eq!(m.state(), *s, "a refused {} changed the state", stringify!($trig));
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
    ($s:ident { $($b:tt)* }) => { State::$s { $($b)* } };
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
