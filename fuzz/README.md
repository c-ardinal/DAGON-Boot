# Fuzz targets

Coverage-guided fuzzing ([cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz),
libFuzzer) of the code that reads bytes from outside. The pull request
checks run the same readers over generated input with a fixed seed
(`crates/dagon-proto/tests/random_frames.rs`); these targets keep looking.

| Target | Reads | Holds |
| -- | -- | -- |
| `frame` | a frame off a link at every link's payload limit, and the request of its command as a node's table decodes it | no panic; a decoded frame encodes back to its bytes; a frame built from the input decodes to what it was built from |
| `release_file` | a release file before any digest or signature check: head, items, manifest | no panic |
| `flash_records` | an image header, a state log face header and a state log record | no panic; what parses writes back to bytes that parse to the same value |

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked
cd fuzz
cargo +nightly fuzz run frame -- -max_total_time=300
```

A failing input is kept in `fuzz/artifacts/<target>/`; `cargo +nightly fuzz
run <target> <file>` runs it again. The nightly workflow
(`.github/workflows/nightly.yml`) runs each target for five minutes.
