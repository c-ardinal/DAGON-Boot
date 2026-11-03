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
slot-b = ["dagon-mcu-stm32f401re/slot-b"]

[dependencies]
# DAGON MCU support (the package line is rewritten by dagon gen).
{{dep:dagon-mcu-stm32f401re}}
embassy-stm32 = { version = "0.6.0", features = ["stm32f401re", "time-driver-any"] }
embassy-executor = { version = "0.10", features = ["platform-cortex-m", "executor-thread"] }
embassy-time = "0.5"
cortex-m = { version = "0.7", features = ["critical-section-single-core"] }
cortex-m-rt = "0.7"
defmt-rtt = "1"
panic-probe = "1"

[profile.release]
opt-level = "z"
lto = "fat"
codegen-units = 1
panic = "abort"
debug = true
