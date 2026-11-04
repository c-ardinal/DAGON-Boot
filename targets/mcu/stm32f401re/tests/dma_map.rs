//! The definition's DMA request mapping (`[[peripheral]] dma_requests`)
//! equals ST's chip data for the STM32F401RE as stm32-metapac ships it
//! (the data embassy-stm32's DMA traits come from), for every block the
//! definition declares.

use std::collections::BTreeSet;

type Cell = (String, String, String, u8, u8);

fn definition() -> (BTreeSet<Cell>, BTreeSet<String>) {
    let text = include_str!("../dagon-mcu.toml");
    let t: toml::Table = text.parse().unwrap();
    let mut cells = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for p in t["peripheral"].as_array().unwrap() {
        let id = p["id"].as_str().unwrap().to_string();
        for r in p
            .get("dma_requests")
            .and_then(|r| r.as_array())
            .into_iter()
            .flatten()
        {
            let int = |k: &str| u8::try_from(r[k].as_integer().unwrap()).unwrap();
            cells.insert((
                id.clone(),
                r["signal"].as_str().unwrap().to_string(),
                r["controller"].as_str().unwrap().to_string(),
                int("stream"),
                int("channel"),
            ));
        }
        ids.insert(id);
    }
    (cells, ids)
}

#[test]
fn the_dma_request_mapping_is_st_chip_data() {
    let (cells, ids) = definition();
    let mut st = BTreeSet::new();
    for p in stm32_metapac::metadata::METADATA.peripherals {
        let id = p.name.to_lowercase();
        if !ids.contains(&id) {
            continue;
        }
        for c in p.dma_channels {
            // `DMA1_CH4`: stream 4 of DMA1 (embassy-stm32's naming).
            let (ctl, stream) = c.channel.unwrap().split_once("_CH").unwrap();
            st.insert((
                id.clone(),
                c.signal.to_lowercase(),
                ctl.to_lowercase(),
                stream.parse().unwrap(),
                u8::try_from(c.request.unwrap()).unwrap(),
            ));
        }
    }
    assert!(!st.is_empty());
    let missing: Vec<_> = st.difference(&cells).collect();
    let extra: Vec<_> = cells.difference(&st).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "missing {missing:?}, not in ST's data {extra:?}"
    );
}
