//! The node-side conformance tests over a fake flash with the board's
//! declared layout and the definition's scratch.

use dagon_conform::node::{run, NodeSetup};
use dagon_mcu_stm32f401re::{conform_scratch, layout, CONFORM_SCRATCH};
use dagon_port::{Flash, FlashError, FlashRegion};
use dagon_proto::payload::ConfRunReq;

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

/// The scratch is the definition's, a declared region of its own, apart
/// from both APP slots and the log faces.
#[test]
fn the_scratch_is_the_definitions_and_holds_no_image() {
    assert_eq!(
        CONFORM_SCRATCH,
        Some((layout::SCRATCH_SECTOR, layout::SCRATCH_SECTOR_LEN))
    );
    let at = conform_scratch();
    assert!(layout::FLASH_REGIONS
        .iter()
        .any(|r| r.start == at && r.len == layout::SCRATCH_SECTOR_LEN));
    for s in layout::SLOTS.slots {
        assert!(at + layout::SCRATCH_SECTOR_LEN <= s || at >= s + layout::SLOTS.slot_len);
    }
    for f in layout::LOG.faces {
        assert!(at + layout::SCRATCH_SECTOR_LEN <= f || at >= f + layout::LOG.face_len);
    }
}

/// Every node-side test passes on the scratch and erases nothing else: both
/// APP slots (whatever images they hold) and the log stay as they were.
#[test]
fn node_tests_pass_and_only_touch_the_scratch() {
    let mut m = Mem {
        bytes: vec![0x11; 512 * 1024],
        erased: Vec::new(),
    };
    let setup = NodeSetup {
        declared: &layout::FLASH_REGIONS,
        scratch: conform_scratch(),
    };
    let mut feeds = 0;
    for test_id in [1, 2, 3, 7, 8] {
        let req = ConfRunReq { test_id, param: 0 };
        let o = run(&mut m, &setup, req, &mut || feeds += 1).unwrap();
        assert!(o.pass, "CT-{test_id:02}: {:?}", o.detail);
    }
    assert!(feeds > 0);
    assert!(m.erased.iter().all(|&a| a == layout::SCRATCH_SECTOR));
    for s in layout::SLOTS.slots {
        let a = (s - BASE) as usize;
        assert!(m.bytes[a..a + 0x2_0000].iter().all(|&b| b == 0x11));
    }
    let log = (layout::LOG.faces[0] - BASE) as usize;
    assert!(m.bytes[log..log + 0x8000].iter().all(|&b| b == 0x11));
}
