//! The built-in MCU definitions the simulator models its nodes on.

/// The built-in definition of a simulated MCU (for a simulation given no
/// definitions), as a node on its default board has it ([`on_board`]): a
/// host times such a node on that board.
pub(crate) fn simulated_mcu(mcu: &str) -> &'static dagon_topology::defs::McuDef {
    use dagon_topology::defs::McuDef;
    use std::sync::{Mutex, PoisonError};
    static MADE: Mutex<Vec<&'static McuDef>> = Mutex::new(Vec::new());
    let mut made = MADE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(m) = made.iter().find(|m| m.id == mcu) {
        return m;
    }
    let reg = dagon_topology::defs::Registry::builtin();
    let def = reg
        .mcu(mcu)
        .unwrap_or_else(|| panic!("no MCU definition {mcu}"));
    let def = match on_board(def, reg.default_board(mcu)) {
        Some(on) => &*Box::leak(Box::new(on)),
        None => def,
    };
    made.push(def);
    def
}

/// `def` as a node on `board` has it: the board's `[flash]` erase regions
/// in place of the MCU's (the regions a host's time limits for that node
/// come from, `McuDef::erase_regions_on`); `None` when the board states
/// none, so `def` stands as it is.
pub(crate) fn on_board(
    def: &dagon_topology::defs::McuDef,
    board: Option<&dagon_topology::defs::BoardDef>,
) -> Option<dagon_topology::defs::McuDef> {
    let flash = board?.flash.as_ref()?;
    let mut on = def.clone();
    on.execution.as_mut()?.erase_regions = flash.erase_regions.clone();
    Some(on)
}
