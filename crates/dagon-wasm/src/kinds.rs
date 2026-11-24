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

/// Every rule id the topology check gives (`dagon_topology::RULES`): the
/// page shows its own words for each.
#[wasm_bindgen]
pub fn rule_ids() -> Vec<String> {
    dagon_topology::RULES
        .iter()
        .map(|r| r.to_string())
        .collect()
}

/// Every reason dagon-host gives a node that did not get through
/// (`dagon_host::REASONS`), then every kind of refused release
/// ([`crate::host::REFUSALS`]): the page shows its own words for each.
#[wasm_bindgen]
pub fn failure_reasons() -> Vec<String> {
    dagon_host::REASONS
        .iter()
        .chain(crate::host::REFUSALS)
        .map(|r| r.to_string())
        .collect()
}

/// Every job kind `dagon serve` runs (`dagon_host::jobs::KINDS`).
#[wasm_bindgen]
pub fn job_kinds() -> Vec<String> {
    dagon_host::jobs::KINDS
        .iter()
        .map(|k| k.to_string())
        .collect()
}

/// Every job state `dagon serve` gives (`dagon_host::jobs::STATES`).
#[wasm_bindgen]
pub fn job_states() -> Vec<String> {
    dagon_host::jobs::STATES
        .iter()
        .map(|k| k.to_string())
        .collect()
}

/// How long dagon-host waits for a rebooted hub's new HELLO: the Console's
/// reconnect countdown reads it here instead of keeping a copy.
#[wasm_bindgen]
pub fn host_reconnect_ms() -> u32 {
    dagon_host::HUB_RECONNECT_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_refusals_are_every_kind_refusal_gives() {
        let src = include_str!("host.rs");
        let start = src.find("pub fn refusal<E>").unwrap();
        let body = &src[start..];
        let body = &body[..body.find("\n}\n").unwrap()];
        let mut given: Vec<&str> = body.split('"').skip(1).step_by(2).collect();
        given.sort();
        let mut listed = crate::host::REFUSALS.to_vec();
        listed.sort();
        assert_eq!(listed, given);
        let reasons = failure_reasons();
        assert_eq!(reasons.len(), dagon_host::REASONS.len() + listed.len());
        assert_eq!(rule_ids().len(), dagon_topology::RULES.len());
    }

    #[test]
    fn the_reconnect_wait_is_the_hosts() {
        assert_eq!(host_reconnect_ms(), dagon_host::HUB_RECONNECT_MS);
    }

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
