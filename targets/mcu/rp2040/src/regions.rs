//! Where each XFER target and slot the slot store receives into lies.

use crate::layout::{
    body_capacity, APP_SLOT, APP_SLOT_SIZE, BL_SLOT, BL_SLOT_SIZE, HEADER_LEN, TOPOLOGY,
    TOPOLOGY_SIZE,
};
use dagon_proto::payload::target;

/// Where `t`'s region of `slot` starts, where its body starts, and its body
/// capacity; `None` for a target and slot this package does not receive.
pub fn region(t: u8, slot: u8) -> Option<(u32, u32, u32)> {
    match (t, slot) {
        (target::APP, 0 | 1) => {
            let base = APP_SLOT[slot as usize];
            Some((base, base + HEADER_LEN, body_capacity(APP_SLOT_SIZE)))
        }
        (target::BOOTCORE, 0 | 1) => {
            let base = BL_SLOT[slot as usize];
            Some((base, base + HEADER_LEN, body_capacity(BL_SLOT_SIZE)))
        }
        (target::TOPOLOGY, 0) => Some((TOPOLOGY, TOPOLOGY, TOPOLOGY_SIZE)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::region;

    /// Every target and slot the store receives into has the definition's
    /// one planner layout, with the store's capacity and region start.
    #[test]
    fn every_slot_the_store_takes_has_one_planner_layout() {
        for t in 0..=u8::MAX {
            for slot in 0..=u8::MAX {
                let mut layouts = crate::SLOT_LAYOUTS
                    .iter()
                    .filter(|l| (l.target, l.slot) == (t, slot));
                let (first, second) = (layouts.next(), layouts.next());
                assert!(second.is_none(), "{t}/{slot}: two layouts");
                match (region(t, slot), first) {
                    (None, None) => {}
                    (Some((base, _, cap)), Some(l)) => {
                        assert_eq!((l.start, l.capacity), (base, cap), "{t}/{slot}")
                    }
                    (r, l) => panic!("{t}/{slot}: region {r:?}, layout {l:?}"),
                }
            }
        }
    }
}
