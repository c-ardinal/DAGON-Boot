//! The link kinds as the page reads them: codes to decide on, names only to
//! show a kind and to read and write a 構成. All of it is generated from
//! `dagon_format::with_link_kinds!`, so a new kind reaches the page with no
//! edit here.

use crate::define::set;
use dagon_topology::schema::LinkKind as Kind;
use js_sys::{Array, Object};
use wasm_bindgen::prelude::*;

macro_rules! link_kind_exports {
    (() $( $v:ident = $c:literal, $n:literal; )*) => {
        /// A link kind by its code (the code events carry): what the page
        /// decides on.
        #[wasm_bindgen]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum LinkKind {
            $( $v = $c, )*
        }

        /// Every kind's code and 構成 name, in code order.
        pub(crate) const KINDS: &[(u8, &str)] = &[$( ($c, $n), )*];

        /// The code of kind `k`.
        pub(crate) fn code(k: Kind) -> u8 {
            match k {
                $( Kind::$v => $c, )*
            }
        }

        /// The kind with code `c`, if any.
        pub(crate) fn of_code(c: u8) -> Option<Kind> {
            match c {
                $( $c => Some(Kind::$v), )*
                _ => None,
            }
        }
    };
}

dagon_format::with_link_kinds!([link_kind_exports]());

/// The link kind table: `[{code, name}]` in code order. `name` is what a 構成
/// writes; the page decides on `code` and shows its own words for it.
#[wasm_bindgen]
pub fn link_kind_table() -> JsValue {
    KINDS
        .iter()
        .map(|&(c, n)| {
            let o = Object::new();
            set(&o, "code", c.into());
            set(&o, "name", n.into());
            JsValue::from(o)
        })
        .collect::<Array>()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_every_kind_in_code_order() {
        assert_eq!(KINDS.len(), Kind::ALL.len());
        let names: Vec<&str> = KINDS.iter().map(|&(_, n)| n).collect();
        let all: Vec<&str> = Kind::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(names, all);
        assert!(KINDS.windows(2).all(|w| w[0].0 < w[1].0), "{KINDS:?}");
    }

    #[test]
    fn codes_go_both_ways() {
        for &k in Kind::ALL {
            let c = code(k);
            assert_eq!(of_code(c), Some(k));
            assert!(KINDS.contains(&(c, k.name())), "{k:?}");
        }
        assert_eq!(of_code(0), None);
    }
}
