#![forbid(unsafe_code)]
//! Browser build of the shared Rust rules: board table, topology check and
//! compile, release verification, HELLO payloads and version text. Each
//! export is a thin wrapper so the page and the CLI apply the same code.

extern crate alloc;

mod define;
mod frame;
mod host;
mod release_topo;
#[cfg(feature = "sim")]
mod sim;
mod watch;

pub use define::{board_template, release_topology, supported_mcus, topo_parse};
pub use frame::{frame_encode, FrameCutter};
pub use host::{
    check_bootcore_for, check_release_for, require_release_key, set_release_key, watch_interval_ms,
    WasmHost,
};
#[cfg(feature = "sim")]
pub use sim::SimDesk;

use dagon_format::{BundleView, Version};
use dagon_proto::payload::{HelloReq, HelloResp};
use dagon_topology::boards::BOARDS;
use wasm_bindgen::prelude::*;

/// One entry of the supported-board table.
#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardInfo {
    /// Key used in `nodes[].board`.
    pub id: String,
    pub product: String,
    pub mcu_name: String,
    /// Topology MCU id (`rp2040`, `stm32f401re`, ...).
    pub mcu: String,
    pub flash_bytes: u32,
    pub i2c_target: bool,
    /// Which line drawing the UI uses.
    pub drawing: String,
}

#[wasm_bindgen]
pub fn supported_boards() -> Vec<BoardInfo> {
    BOARDS
        .iter()
        .map(|b| BoardInfo {
            id: b.id.into(),
            product: b.product.into(),
            mcu_name: b.mcu_name.into(),
            mcu: b.mcu.into(),
            flash_bytes: b.flash_bytes,
            i2c_target: b.i2c_target,
            drawing: b.drawing.into(),
        })
        .collect()
}

/// `topo check` report as JSON: `{ok, errors, warnings, issues: [{rule, severity, path, message}]}`.
#[wasm_bindgen]
pub fn topo_check(yaml: &str) -> String {
    dagon_topology::check_str(yaml).to_json()
}

/// Canonical topology binary; throws when the YAML does not parse or check.
#[wasm_bindgen]
pub fn topo_compile(yaml: &str) -> Result<Vec<u8>, JsError> {
    dagon_topology::compile_str(yaml).map_err(|e| JsError::new(&e.to_string()))
}

/// Full host-side check of a `.dgb` file under a 32-byte Ed25519 public
/// key; throws with the failure name (`BadSignature`, `DigestMismatch`, ...).
#[wasm_bindgen]
pub fn verify_release(bundle: &[u8], pubkey: &[u8]) -> Result<(), JsError> {
    verify(bundle, pubkey).map_err(|e| JsError::new(&e))
}

fn verify(bundle: &[u8], pubkey: &[u8]) -> Result<(), String> {
    let key: &[u8; 32] = pubkey.try_into().map_err(|_| "BadKeyLength".to_string())?;
    BundleView::parse(bundle)
        .and_then(|b| b.verify(key))
        .map_err(|e| format!("{e:?}"))
}

/// The version `bundle` gives each node, as `[node_index, raw_version]`
/// pairs from its manifest (a node with A/B images appears once per image).
/// Empty when the bundle does not parse; read only to show "1.0.0 → 1.1.0"
/// before an update, never to decide anything.
#[wasm_bindgen]
pub fn release_versions(bundle: &[u8]) -> Vec<u32> {
    let Ok(b) = BundleView::parse(bundle) else {
        return Vec::new();
    };
    let m = b.manifest();
    (0..m.entry_count())
        .map(|i| m.entry(i))
        .flat_map(|e| [u32::from(e.node_index), e.version.raw()])
        .collect()
}

/// Decoded HELLO response body.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hello {
    pub proto_ver: u32,
    pub role: u8,
    pub port_kind: u8,
    pub mcu_id: u16,
    pub mode: u8,
    /// Raw `major << 24 | minor << 16 | patch`; show it with `version_text`.
    pub active_version: u32,
    pub trial: bool,
    key_id: [u8; 8],
    pub max_payload: u16,
}

#[wasm_bindgen]
impl Hello {
    #[wasm_bindgen(getter)]
    pub fn key_id(&self) -> Vec<u8> {
        self.key_id.to_vec()
    }
}

/// HELLO request body.
#[wasm_bindgen]
pub fn hello_request(proto_ver: u32, max_payload: u16) -> Vec<u8> {
    let mut out = [0u8; 6];
    let n = HelloReq {
        proto_ver,
        max_payload,
    }
    .encode(&mut out)
    .expect("6-byte buffer fits HelloReq");
    out[..n].to_vec()
}

/// HELLO response body (after the status byte); throws on a malformed body.
#[wasm_bindgen]
pub fn hello_decode(body: &[u8]) -> Result<Hello, JsError> {
    decode(body).ok_or_else(|| JsError::new("malformed HELLO response"))
}

fn decode(body: &[u8]) -> Option<Hello> {
    let h = HelloResp::parse(body).ok()?;
    Some(Hello {
        proto_ver: h.proto_ver,
        role: h.role,
        port_kind: h.port_kind,
        mcu_id: h.mcu_id,
        mode: h.mode,
        active_version: h.active_version.raw(),
        trial: h.trial,
        key_id: h.key_id,
        max_payload: h.max_payload,
    })
}

/// Whether raw version `a` is older than `b` (dagon-format's ordering).
#[wasm_bindgen]
pub fn version_older(a: u32, b: u32) -> bool {
    Version::from_raw(a) < Version::from_raw(b)
}

/// The page verifies releases with the public RFC 8032 test key.
#[wasm_bindgen]
pub fn release_key_is_test() -> bool {
    host::release_key() == Some(TEST_PUBKEY_BYTES)
}

const TEST_PUBKEY_BYTES: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

/// `major.minor.patch` of a raw version.
#[wasm_bindgen]
pub fn version_text(raw: u32) -> String {
    Version::from_raw(raw).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const TEST_PUBKEY: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

    fn root(rel: &str) -> PathBuf {
        [env!("CARGO_MANIFEST_DIR"), "..", "..", rel]
            .iter()
            .collect()
    }

    fn golden(name: &str) -> Vec<u8> {
        std::fs::read(root(&format!("crates/dagon-format/golden/{name}"))).unwrap()
    }

    fn pubkey() -> Vec<u8> {
        (0..32)
            .map(|i| u8::from_str_radix(&TEST_PUBKEY[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn boards_mirror_the_table() {
        let b = supported_boards();
        assert_eq!(b.len(), BOARDS.len());
        assert!(b.iter().any(|x| x.mcu == "stm32f401re"));
    }

    #[test]
    fn desk_demo_checks_and_compiles_to_the_golden_binary() {
        let yaml = std::fs::read_to_string(root("topology/desk-demo.yaml")).unwrap();
        assert!(topo_check(&yaml).starts_with("{\"ok\":true"));
        assert_eq!(
            dagon_topology::compile_str(&yaml).unwrap(),
            golden("topology_desk_demo.bin")
        );
        assert!(topo_check("nodes: [").starts_with("{\"ok\":false"));
    }

    #[test]
    fn golden_release_verifies_and_tampering_fails() {
        let mut dgb = golden("release.dgb");
        assert_eq!(verify(&dgb, &pubkey()), Ok(()));
        assert_eq!(verify(&dgb, &[0; 31]), Err("BadKeyLength".into()));
        let last = dgb.len() - 1;
        dgb[last] ^= 1;
        assert!(verify(&dgb, &pubkey()).is_err());
    }

    #[test]
    fn release_verdict_names_why_nothing_is_sent() {
        use crate::host::{release_refusal, PUBKEY};
        let topo = golden("topology_desk_demo.bin");
        let mut dgb = golden("release.dgb");
        // The default build embeds the test key the golden release is signed with.
        assert_eq!(PUBKEY.to_vec(), pubkey());
        // A run-time key replaces it; a required one refuses until it is set.
        let other = "ab".repeat(32);
        crate::host::set_release_key(&other).unwrap();
        assert_eq!(release_refusal(&dgb, &topo), Some("key"));
        crate::host::set_release_key(TEST_PUBKEY).unwrap();
        assert_eq!(release_refusal(&dgb, &topo), None);
        assert!(release_key_is_test());
        assert!(version_older(0x0100_0000, 0x0101_0000));
        assert!(!version_older(0x0101_0000, 0x0101_0000));
        assert_eq!(release_refusal(&dgb, &topo), None);
        let yaml = std::fs::read_to_string(root("topology/sim-pair.yaml")).unwrap();
        let other = dagon_topology::compile_str(&yaml).unwrap();
        assert_eq!(release_refusal(&dgb, &other), Some("topology"));
        assert_eq!(release_refusal(b"not a release", &topo), Some("format"));
        let last = dgb.len() - 1;
        dgb[last] ^= 1;
        assert_eq!(release_refusal(&dgb, &topo), Some("source"));
    }

    #[test]
    fn hello_round_trip_against_golden_frames() {
        // Frame = 8-byte header + payload + 4-byte CRC; a response payload starts with the status byte.
        let req = golden("frame_hello_req.bin");
        assert_eq!(hello_request(1, 1088), req[8..req.len() - 4]);
        let resp = golden("frame_hello_resp.bin");
        let h = decode(&resp[9..resp.len() - 4]).unwrap();
        assert_eq!(version_text(h.active_version), "1.1.0");
        assert_eq!((h.trial, h.max_payload), (true, 256));
        assert_eq!(decode(&resp[9..resp.len() - 5]), None);
    }
}
