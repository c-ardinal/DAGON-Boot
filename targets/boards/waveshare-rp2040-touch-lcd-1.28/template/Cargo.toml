[package]
name = "{{node}}"
version = "0.1.0"
edition = "2021"
publish = false

# One crate per node: each builds for its own target.
[workspace]

[[bin]]
name = "{{node}}"
path = "src/main.rs"
test = false

[features]
# Links the same app for slot B (dagon.toml: slot_b_feature = "slot-b").
slot-b = ["dagon-board-waveshare-rp2040-touch-lcd-1-28/slot-b"]

[dependencies]
# DAGON board support (the package lines are rewritten by dagon gen).
{{dep:dagon-board-waveshare-rp2040-touch-lcd-1-28}}
{{dep:dagon-mcu-rp2040}}
embassy-rp = { version = "0.10.0", features = ["rp2040", "time-driver", "critical-section-impl"] }
embassy-executor = { version = "0.10", features = ["platform-cortex-m", "executor-thread"] }
embassy-time = "0.5"
cortex-m-rt = "0.7"
panic-halt = "1"

[profile.release]
opt-level = "z"
lto = "fat"
codegen-units = 1
panic = "abort"
debug = true
