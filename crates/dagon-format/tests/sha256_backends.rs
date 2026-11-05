//! SHA-256 against digests of an independent implementation (Python's
//! hashlib) over the block edges and a full 128 KiB boot core slot body,
//! hashed whole and in the 256-byte reads stage0 makes.
//!
//! The digests do not depend on the sha2 backend, so the same test checks
//! whichever one the build selects. CI runs it a second time with
//! `--cfg sha2_backend="soft" --cfg sha2_backend_soft="compact"`: the
//! compact rounds the stage0s are built with (their `.cargo/config.toml`),
//! the same Rust code their targets compile, which have no other backend.

use sha2::{Digest, Sha256};

/// (length, SHA-256 of the first `length` bytes of [`data`]).
const VECTORS: &[(usize, &str)] = &[
    (
        0,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        1,
        "6922e93e3827642ce4b883c756b31abf80036649d3614bf5fcb3adda43b8ea32",
    ),
    (
        3,
        "c70406763cd0f149b4caf62c4e1ffdb8c0c3402ac386569e09e701eb2779b6a7",
    ),
    (
        55,
        "e8d303869d23b3892ab75195a242f5203144ded112a87c4fb9079b355e87ffe4",
    ),
    (
        56,
        "ab8727d3bae8b8c39f394b2a53411d0e8d2020a1574bfcfcb9a8d535009d06c5",
    ),
    (
        57,
        "23953a96203914356f4a331ec89593a979ce96c4ccae998b10910dbe0d9ce2c5",
    ),
    (
        63,
        "8c7cdde97c6969437751b2282f88b747a0c99671df710d83d3e621ebaddf1e65",
    ),
    (
        64,
        "148abbe3aab04cb38056c13ab3434aebd4c7fe5fc59259d9cf0290832bc3bae6",
    ),
    (
        65,
        "a6fa1ba90e0982054f11bf4fec71ee17e99a993eba065726d856066450ec8d94",
    ),
    (
        119,
        "91de87bd101f56a5830e8ec0654686be4ce3d26394841e2f4c4c46881903fd85",
    ),
    (
        120,
        "4f47393eb8899df4e71ba0589f97a23cd8c9e1b523b953c5499f33ccab1e84bd",
    ),
    (
        127,
        "dbdb339057889f4849ce91779a7ddc9414e8488bc8841593445d682df2c71d06",
    ),
    (
        128,
        "72065c80ac80470479aa05485be4d420311818802403f89f5d164d5e4716b8d1",
    ),
    (
        129,
        "ac2da602af427486ed7dca8bb1661a0672fd9bf6ff7ad57ac02a60bdb3dba1e2",
    ),
    (
        255,
        "439cc6819cadf1571f1263a0fd26b1310c1a3705313e7cceda7663f1a6f9dd4d",
    ),
    (
        256,
        "c1f7ae93af4c1686c882edf61ce40cc06774a3ebdc7581aa19d1966f4ad7a33d",
    ),
    (
        257,
        "6f613822d77f21c96b1d7261ac9f20509b4a540ebabd084fb0a6d17e74c0dbdd",
    ),
    (
        511,
        "fdc1cacdda89a73aa998fa9171663deaac10076b1b32479a0ace6346c7bd57b4",
    ),
    (
        512,
        "056213237abc5479df5296d9d4a2c2a38f35675b8a01711c5dca7ced70e5606a",
    ),
    (
        513,
        "92e1372e5acf4245eeda21e2198f1a6401f4e12d531712fe04dc68b600af6f26",
    ),
    (
        1000,
        "cbb19333ff7c92a306ea837d06ea7f3feb9f5efa8820b4a9af123770092a0b0a",
    ),
    // A boot core filling the RP2040's 128 KiB slot after its 512-byte header.
    (
        130560,
        "d32e001ec5d1829902c5bb343965f4235ad9d0900fecbeda68ecdc45607d239c",
    ),
];

/// The bytes hashed: an xorshift32 stream (seed 0x1234_5678, low byte).
fn data(n: usize) -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn sha256_matches_an_independent_implementation_whole_and_in_stage0_reads() {
    let all = data(VECTORS.last().unwrap().0);
    for &(n, want) in VECTORS {
        let bytes = &all[..n];
        assert_eq!(hex(&dagon_format::sha256(bytes)), want, "{n} bytes whole");
        let mut h = Sha256::new();
        for chunk in bytes.chunks(256) {
            h.update(chunk);
        }
        assert_eq!(hex(&h.finalize()), want, "{n} bytes in 256-byte reads");
    }
}
