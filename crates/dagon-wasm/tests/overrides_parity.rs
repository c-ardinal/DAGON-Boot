//! The page's queries answer from a project's overridden definitions, as
//! `dagon serve` sends them (`Registry::to_json`), for a built-in MCU and
//! board and for a synthetic pair (dagon-topology's tests/parity).

#[path = "../../dagon-topology/tests/parity/mod.rs"]
mod parity;

use dagon_topology::defs::{Registry, Source};
use parity::*;

#[test]
fn the_page_queries_see_the_projects_overrides() {
    let p = project("wasm");
    let reg = Registry::load(&[Source::Tree(&p.tree), Source::Project(&p.toml)]).unwrap();
    // Before the page loads them: the built-in set.
    assert_ne!(dagon_wasm::mcu_trial_ms(BUILTIN.0), Some(TRIAL_MS));
    assert_eq!(dagon_wasm::mcu_trial_ms(SYNTHETIC.0), None);
    dagon_wasm::load_definitions(&reg.to_json()).unwrap();

    for (mcu, board, _) in OVERRIDDEN {
        assert_eq!(dagon_wasm::mcu_trial_ms(mcu), Some(TRIAL_MS), "{mcu}");
        let b = dagon_wasm::supported_boards()
            .into_iter()
            .find(|b| b.id == board)
            .unwrap();
        assert_eq!((b.product.as_str(), b.mcu.as_str()), (PRODUCT, mcu));
    }
    assert!(dagon_wasm::rom_i2c_addrs().contains(&ROM_ADDR));
    let report = dagon_wasm::topo_check(TOPOLOGY);
    assert!(report.starts_with("{\"ok\":true"), "{report}");
    assert!(!dagon_wasm::topo_compile(TOPOLOGY).unwrap().is_empty());
}
