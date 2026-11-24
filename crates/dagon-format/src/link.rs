//! Link kinds: the one list of the media a DAGON hop runs over, with the
//! name a 構成 uses and the code events carry (`app.link_up` `link`,
//! `app.request` `via`). Codes never change; a new kind takes the next one.
//!
//! The list is data handed to a generator: [`with_link_kinds!`] calls the
//! given macro with every entry, so the topology's kind, the firmware's kind
//! and its compile-time requirement messages all come from it, and what a
//! kind is (addressed, enumerated, carried by the compiled topology) is
//! the entry's traits. Adding a transport is one entry here.

/// Calls `$($gen)*! { ($($args)*) Variant = code, "name", [traits]; … }`
/// with every link kind, in code order. A kind's traits are what the
/// medium is, whatever MCU drives it ([`link_kind_has!`] reads them):
///
/// - `compiled`: the compiled topology can carry it between a coordinator
///   and a node linked to it;
/// - `addressed`: its nodes are reached at a 7-bit address an end gives;
/// - `enumerated`: the other end enumerates it before it carries a frame,
///   so it cannot take part in a boot stage's listen window.
#[macro_export]
macro_rules! with_link_kinds {
    ([$($gen:tt)*] ($($args:tt)*)) => {
        $($gen)*! {
            ($($args)*)
            UsbCdc = 1, "usb-cdc", [enumerated];
            Uart = 2, "uart", [compiled];
            I2c = 3, "i2c", [compiled addressed];
            Ble = 4, "ble", [];
            WifiTcp = 5, "wifi-tcp", [];
            Ethernet = 6, "ethernet", [];
            Can = 7, "can", [];
        }
    };
}

/// Whether the trait list of a [`with_link_kinds!`] entry holds a trait:
/// `link_kind_has!(addressed; compiled addressed)` is `true`. A trait the
/// list does not know stops the build.
#[macro_export]
macro_rules! link_kind_has {
    ($want:ident; $($t:ident)*) => {
        false $(|| $crate::__link_trait_is!($want $t))*
    };
}

/// `link_kind_has!`: one wanted trait against one held.
#[doc(hidden)]
#[macro_export]
macro_rules! __link_trait_is {
    (compiled compiled) => {
        true
    };
    (addressed addressed) => {
        true
    };
    (enumerated enumerated) => {
        true
    };
    ($want:ident compiled) => {
        false
    };
    ($want:ident addressed) => {
        false
    };
    ($want:ident enumerated) => {
        false
    };
    ($want:ident $t:ident) => {
        compile_error!(concat!("unknown link kind trait: ", stringify!($t)))
    };
}

#[cfg(test)]
mod tests {
    /// Each kind's name with its traits, as a generator sees them.
    macro_rules! traits {
        (() $( $v:ident = $c:literal, $n:literal, [$($t:ident)*]; )*) => {
            &[$((
                $n,
                $crate::link_kind_has!(compiled; $($t)*),
                $crate::link_kind_has!(addressed; $($t)*),
                $crate::link_kind_has!(enumerated; $($t)*),
            )),*]
        };
    }

    /// The traits a generator reads are the ones the list gives: the
    /// compiled topology carries UART and I2C, only I2C is addressed, only
    /// USB CDC is enumerated, and a kind with no trait has none.
    #[test]
    #[allow(clippy::nonminimal_bool, clippy::overly_complex_bool_expr)]
    fn a_kinds_traits_are_the_ones_its_entry_lists() {
        let all: &[(&str, bool, bool, bool)] = crate::with_link_kinds!([traits]());
        let of = |name: &str| *all.iter().find(|k| k.0 == name).unwrap();
        assert_eq!(of("usb-cdc"), ("usb-cdc", false, false, true));
        assert_eq!(of("uart"), ("uart", true, false, false));
        assert_eq!(of("i2c"), ("i2c", true, true, false));
        for name in ["ble", "wifi-tcp", "ethernet", "can"] {
            assert_eq!(of(name), (name, false, false, false));
        }
        assert_eq!(all.len(), 7);
    }
}
