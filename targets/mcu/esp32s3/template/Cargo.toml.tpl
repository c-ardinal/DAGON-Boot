[package]
name = "{{node}}"
version = "0.1.0"
edition = "2021"
publish = false

# One crate per node: each builds for its own target (espup Xtensa toolchain).
[workspace]

[[bin]]
name = "{{node}}"
path = "src/main.rs"
test = false

[dependencies]
# DAGON board support (the package lines are rewritten by dagon gen).
{{dep:dagon-mcu-esp32s3}}
esp-hal = { version = "1.2.2", features = ["esp32s3", "unstable"] }
esp-bootloader-esp-idf = { version = "0.6.0", features = ["esp32s3"] }
esp-rtos = { version = "0.4.0", features = ["esp32s3", "embassy"] }
embassy-executor = "0.10"
embassy-time = "0.5"

[profile.release]
opt-level = "s"
lto = "fat"
codegen-units = 1
debug = 2

# Size-tuned Ed25519: at "s" the field arithmetic is inlined into one
# function of several hundred KiB.
[profile.release.package.ed25519-compact]
opt-level = "z"

[profile.dev]
opt-level = "s"

[profile.dev.package.esp-bootloader-esp-idf]
overflow-checks = false
