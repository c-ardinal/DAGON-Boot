//! A release file as the host reads it before any check of its digests or
//! signature: the head, every item and its bytes, the manifest. Nothing
//! panics on any byte string.
#![no_main]

use dagon_format::bundle::BundleView;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(view) = BundleView::parse(data) else {
        return;
    };
    for i in 0..view.item_count() {
        let _ = view.item(i);
        let _ = view.item_bytes(i);
    }
    let _ = view.manifest();
    let _ = view.topology_item();
});
