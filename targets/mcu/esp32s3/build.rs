//! The constants of `dagon-mcu.toml` and the image verification key
//! (`DAGON_PUBKEY`, as for the RP2040 and F401 builds).
fn main() {
    dagon_mcu_build::definition();
    dagon_mcu_build::embed_pubkey();
}
