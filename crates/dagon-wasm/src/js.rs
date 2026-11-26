//! Plain JS values the exports build.

use js_sys::{Object, Reflect};
use wasm_bindgen::JsValue;

/// Sets property `k` of the plain object `o`.
pub(crate) fn set(o: &Object, k: &str, v: JsValue) {
    Reflect::set(o, &k.into(), &v).expect("plain object");
}
