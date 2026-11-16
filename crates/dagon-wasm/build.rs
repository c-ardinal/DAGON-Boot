//! Embeds the release verification key from `DAGON_PUBKEY` through the MCU
//! packages' shared helper: the path to the public key file from
//! `dagon key gen`, or the 64 hex digits themselves. Unset, the page build
//! (wasm32) fails unless `DAGON_ALLOW_TEST_KEY=1` opts in to the public RFC
//! 8032 test key (then it warns). Native builds only run this crate's tests,
//! which check the golden release signed with that test key, so they use it
//! with a warning.
use std::env;

fn main() {
    let page = env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32");
    if !page && env::var_os("DAGON_PUBKEY").is_none() {
        env::set_var("DAGON_ALLOW_TEST_KEY", "1");
    }
    dagon_mcu_build::embed_pubkey();
}
