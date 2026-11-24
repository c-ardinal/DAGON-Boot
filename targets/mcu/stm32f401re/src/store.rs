//! Kind S receiver: APP slots and boot core slots with the header inline at
//! the slot start, boot target switches and confirms appended to the state
//! log (one log holds the APP and the boot core records). The store is
//! the shared one of the MCUs with a state log
//! (`dagon_node::log_store`), under this MCU's definition.

use dagon_node::log_store::{LogMcu, LogStore};

/// The STM32F401RE as the log store takes it.
pub enum F401 {}

impl LogMcu for F401 {
    const WRITE_UNIT: usize = crate::WRITE_UNIT as usize;
    const ERASED: u8 = crate::ERASED_VALUE;
    const TRIAL_ATTEMPTS: u8 = crate::TRIAL_ATTEMPTS;
    const SLOT_LAYOUTS: &'static [dagon_port::timeout::SlotLayout] = &crate::SLOT_LAYOUTS;
    const STAGE0: Option<(u32, u32)> = Some((crate::layout::STAGE0, crate::layout::STAGE0_SIZE));
}

pub type KindSStore<F> = LogStore<F, F401>;
