//! The boot core's listen window, as the simulator times it: when the
//! hub's first STAY after it releases a node's reset arrives on a link of
//! each kind, and whether a STAY arriving then keeps the boot core in
//! update mode under the node's MCU definition (`[boot_receive]`).

use dagon_topology::defs::McuDef;
use dagon_topology::schema::LinkKind;

use crate::timing::kind_bytes_per_ms;

/// The hub sends its first STAY this long after it releases the node's
/// reset.
pub const STAY_FIRST_MS: f64 = 2.0;

/// Ms after the node's reset at which the first STAY (an empty request)
/// has fully arrived on a link of `kind`; `None` for a kind the simulator
/// has no rate for.
pub fn first_stay_ms(kind: LinkKind) -> Option<f64> {
    let stay = dagon_proto::frame_len(0) as f64;
    Some(STAY_FIRST_MS + stay / kind_bytes_per_ms(kind)?)
}

/// Whether a STAY that has arrived `at_ms` after the reset on a link of
/// `kind` keeps the boot core of an `mcu` node in update mode: it came
/// within the window the definition opens on that kind.
pub fn stays(mcu: &McuDef, kind: LinkKind, at_ms: f64) -> bool {
    at_ms < f64::from(mcu.listen_window_ms(kind))
}
