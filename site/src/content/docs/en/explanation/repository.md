---
title: "The repository"
description: "What the repository holds, where, and how to work on DAGON itself."
---

For DAGON contributors.

## Repository layout

| Path | Contents |
|---|---|
| `crates/` | DAGON's crates, listed in [Rust API](/en/reference/rust-api/) |
| `targets/` | Definitions per MCU (`targets/mcu/*`) and per board (`targets/boards/*`). The relay itself is MCU-independent, in `crates/dagon-relay`; an MCU definition's `[coordinator]` section decides whether that MCU may relay, and a node relays over the links its definition lists in `[[relay_link]]`, routed by the topology |
| `samples/` | Sample apps built on the definitions (not part of DAGON) |
| `web/` | DAGON Console (Svelte, Vite, TypeScript); `web/README.md` lists what it does |
| `topology/` | Example topologies |
| `plugin/` | Claude Code plugin (agents, skills, hooks) used to port DAGON to new MCUs |
| `scripts/licences.py` | Generates the dependency tables in [Licenses](/en/reference/licenses/) |

## Working on DAGON itself

### What you need

- All the tools in [Install](/en/getting-started/install/)
- The `wasm32-unknown-unknown` target (all targets are in `rust-toolchain.toml`)
- `wasm-pack` 0.15.1 for `crates/dagon-wasm` (it fetches the matching `wasm-bindgen` and `wasm-opt` itself)
- Node.js and npm for `web/`
- ESP-IDF v6.1 to rebuild the ESP32-S3 2nd stage from `targets/esp32s3-2ndstage`. An unmodified prebuilt one is bundled as `targets/mcu/esp32s3/vendor/bootloader.bin` (see `vendor/PROVENANCE.md`).

### Using a clone

`python scripts/page.py build` builds the DAGON Console page into `crates/dagon-cli/page`; then `cargo install --path crates/dagon-cli` installs the `dagon` command from your clone, with that page built in. The page is build output and is not kept in git; rebuild it after changing `web/`.

Projects use the clone with `dagon new --dagon-path <clone>` (or by setting `DAGON_HOME`), which writes `[dagon] path` in `dagon.toml`.

### Formal verification

Two checks reach further than the tests:

- **Kani proofs of stage0's decision** (`crates/dagon-core/src/proofs.rs`). Stage0 is never updated in the field, so a panic in it leaves a node only the ROM loader recovers. The proofs check, for every flash and retained content and every answer of the signature check, that reading and folding the state log and the assembly of stage0's decision (`bootcore::start`) do not panic, and that the trial cell and the choice keep their rules. For the state log's compaction, they check that, for the fold of any four records of any kind, slot, reason and image, the copied records fold to the state they were copied from. Assumed: every flash read returns anything, the state log's CRC-32 returns anything (stubbed), a log face holds two records; the image header and body checks take the solver too long over a symbolic 512-byte header, so the decision's proof replaces them with any result they can give, and the tests and the node search cover them; the loop bodies are the same at every iteration, so the bound hides no path. The signature check itself is the Ed25519 vector and differential tests'.
- **Exhaustive search of a whole node** (`targets/mcu/stm32f401re/tests/explore.rs`). Over the F401's production code (stage0's decision, the boot loader's app decision and settle, the agent, the store) it runs every sequence of app and boot loader writes, CONFIRM, boots and power losses up to a bound, with a reset or a power loss after any flash operation, for two boot loader slots and for one. States are compared by their flash and retained contents, and each is expanded once. Properties: stage0 always starts something (N1), no broken image is confirmed (N2), an app stays confirmed (N3), and with no further fault a few boots end at a good app that runs (N4). A violation prints the shortest sequence.

```
cargo kani -Z stubbing -p dagon-core          # Kani 0.68 or later
cd targets/mcu/stm32f401re
cargo test --no-default-features --target <host triple> --release --test explore
DAGON_EXPLORE_STEPS=8 DAGON_EXPLORE_FAULTS=2 ...  # a wider bound (default: 6, 2 writes, 1 fault)
```

The workflow `.github/workflows/ci.yml` runs both: its `kani` job, and the F401 package's host tests.

### Building the release archives

`python scripts/release.py [--target <triple>]` builds the page from the tree, then `out/release/dagon-cli-<version>-<target>.*` with that page built in. The build stops when there is no page to embed.

`.github/workflows/release.yml` does the same for each platform when a `cli-v*` tag is pushed:

1. `scripts/bump_check.py` confirms the version is at least what the commits require.
2. It builds each system's archive and its `.sha256`.
3. Once every build has passed, it publishes a GitHub release carrying each system's archive and its `.sha256`.

A failed build publishes nothing, and neither does a manual run.

The versioning rules are in [Versions and releases](/en/explanation/versioning/).
