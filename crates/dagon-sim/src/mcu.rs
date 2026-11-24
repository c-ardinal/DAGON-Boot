//! The built-in MCU definitions the simulator models its nodes on.

/// The built-in definition of a simulated MCU (for a simulation given no
/// definitions).
pub(crate) fn simulated_mcu(mcu: &str) -> &'static dagon_topology::defs::McuDef {
    dagon_topology::defs::Registry::builtin()
        .mcu(mcu)
        .unwrap_or_else(|| panic!("no MCU definition {mcu}"))
}
