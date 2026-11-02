//! The node-side conformance tests over a fake flash with the board's
//! declared layout and the scratch slot the conformance build picks.

use dagon_conform::node::{run, NodeSetup};
use dagon_core::log::{LogState, SlotRef};
use dagon_format::Version;
use dagon_port::{Flash, FlashError, FlashRegion};
use dagon_proto::payload::ConfRunReq;
use dagon_mcu_stm32f401re::{scratch_addr, scratch_slot, NO_SCRATCH};

use dagon_mcu_stm32f401re::layout;

const BASE: u32 = 0x0800_0000;

/// Flash with the board geometry; counts erases per address.
struct Mem {
    bytes: Vec<u8>,
    erased: Vec<u32>,
}

impl Flash for Mem {
    fn geometry(&self) -> &[FlashRegion] {
        &layout::FLASH_REGIONS
    }
    fn read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), FlashError> {
        let at = (addr - BASE) as usize;
        buf.copy_from_slice(&self.bytes[at..at + buf.len()]);
        Ok(())
    }
    fn erase(&mut self, addr: u32, len: u32) -> Result<(), FlashError> {
        let at = (addr - BASE) as usize;
        self.bytes[at..at + len as usize].fill(0xFF);
        self.erased.push(addr);
        Ok(())
    }
    fn write(&mut self, addr: u32, data: &[u8]) -> Result<(), FlashError> {
        if !addr.is_multiple_of(4) || !data.len().is_multiple_of(4) {
            return Err(FlashError::Align);
        }
        let at = (addr - BASE) as usize;
        self.bytes[at..at + data.len()].copy_from_slice(data);
        Ok(())
    }
}

fn confirmed(slot: u8) -> LogState {
    LogState {
        confirmed: Some(SlotRef {
            slot,
            version: Version::default(),
            digest16: [0; 16],
        }),
        ..LogState::default()
    }
}

#[test]
fn node_tests_pass_and_only_touch_the_spare_slot() {
    let mut m = Mem {
        bytes: vec![0x11; 512 * 1024],
        erased: Vec::new(),
    };
    let spare = scratch_slot(&confirmed(0), 0).unwrap();
    let setup = NodeSetup {
        declared: &layout::FLASH_REGIONS,
        scratch: layout::SLOTS.slots[spare as usize],
    };
    let mut feeds = 0;
    for test_id in [1, 2, 3, 7, 8] {
        let req = ConfRunReq { test_id, param: 0 };
        let o = run(&mut m, &setup, req, &mut || feeds += 1).unwrap();
        assert!(o.pass, "CT-{test_id:02}: {:?}", o.detail);
    }
    assert!(feeds > 0);
    assert!(m.erased.iter().all(|&a| a == layout::SLOTS.slots[1]));
    // Running slot A and the log are untouched.
    let a = (layout::SLOTS.slots[0] - BASE) as usize;
    assert!(m.bytes[a..a + 0x2_0000].iter().all(|&b| b == 0x11));
    let log = (layout::LOG.faces[0] - BASE) as usize;
    assert!(m.bytes[log..log + 0x8000].iter().all(|&b| b == 0x11));
}

#[test]
fn on_trial_erase_tests_fail_without_erasing() {
    let mut m = Mem {
        bytes: vec![0x11; 512 * 1024],
        erased: Vec::new(),
    };
    let mut trial = confirmed(0);
    trial.pending = trial.confirmed.map(|c| SlotRef { slot: 1, ..c });
    trial.attempts = 1;
    let scratch = scratch_addr(&trial, 1, &layout::SLOTS);
    assert_eq!(scratch, NO_SCRATCH);
    let setup = NodeSetup {
        declared: &layout::FLASH_REGIONS,
        scratch,
    };
    for test_id in [2, 3] {
        let req = ConfRunReq { test_id, param: 0 };
        let o = run(&mut m, &setup, req, &mut || {}).unwrap();
        assert!(!o.pass, "CT-{test_id:02} passed");
        assert!(o.detail.as_bytes().ends_with(b"outside flash"));
    }
    assert!(m.erased.is_empty());
    assert!(m.bytes.iter().all(|&b| b == 0x11));
}
