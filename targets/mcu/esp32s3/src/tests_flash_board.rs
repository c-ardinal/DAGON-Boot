//! A board with its own flash part: its `BoardDef` returns the regions its
//! build generated (the loader's effective regions for this MCU and board,
//! checked in dagon-topology's `tests/board_flash.rs`), and the store plans
//! every erase over them.

use crate::board::BoardDef;
use crate::store::EspStore;
use crate::tests::{FakeOta, Feeds, Store};
use dagon_agent::{erase_plan, SlotStore};
use dagon_port::timeout::{EraseOp, EraseRegion};
use dagon_proto::payload::target::APP;

/// What the board package's build writes from its `[flash]`.
mod generated {
    include!("../tests/fixtures/flash_board_def.rs");
}

const REGIONS: [EraseRegion; generated::ERASE_REGIONS.len()] =
    EraseRegion::table(generated::ERASE_REGIONS);

struct SlowBoard;

impl BoardDef for SlowBoard {
    fn erase_regions(&self) -> &'static [EraseRegion] {
        &REGIONS
    }
}

fn store_on(board: &dyn BoardDef) -> Store {
    EspStore::new(
        crate::tests::FakeFlash(vec![0xFF; crate::layout::MIN_FLASH_SIZE as usize]),
        FakeOta::blank(),
        Feeds::default(),
        board.erase_regions(),
    )
}

#[test]
fn the_store_plans_over_the_generated_board_regions() {
    let s = store_on(&SlowBoard);
    let layout = s.erase_layout().unwrap();
    assert_eq!(
        layout.regions,
        [EraseRegion {
            start: 0x0,
            len: 0x100_0000,
            unit: 0x1000,
            unit_max_ms: 900,
        }]
    );
    let plan: Vec<EraseOp> = erase_plan(&layout, APP, 1, 3_000).unwrap().collect();
    let op = |addr| EraseOp {
        addr,
        len: 0x1000,
        max_ms: 900,
    };
    assert_eq!(plan, [op(0x71_1000), op(0x41_0000)]);
}
