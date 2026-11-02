---
name: fix-a-failing-build
description: Fix a node whose `dagon build` fails, by changing only the developer's code in nodes/<id>/src. Use when the developer says a build of a DAGON node fails or shows a build error.
argument-hint: <node id>
---

# Fix a failing DAGON build

Node: `$ARGUMENTS` (ask for the node id if none was given; the ids are in
`topology.yaml`).

## Steps

1. **Reproduce.** Run `dagon build <id>` and keep the first error with its
   file and line. A build that fails before compiling (no `[build.<id>]`,
   missing `nodes/<id>`) is fixed with `dagon gen <id>`, not by hand.
2. **Find the cause.** Read the error and the code it points at.
   - Error in `src/app.rs` or a file the developer added: fix it there.
   - Error in `src/wiring.rs` or a value from it (pin, address, link):
     the 構成 is the cause. Explain which `topology.yaml` value is wrong and
     run `dagon gen <id>` after the developer agrees to the change.
   - Error in `src/main.rs`: if the developer edited it, show the
     difference; `dagon gen <id> --force-main` writes the start-up again
     (app.rs is never touched).
   - Error inside a `dagon-board-*` / `dagon-mcu-*` crate, a missing
     toolchain or target: do not edit those crates. Report what is missing
     (for example a Rust target from `rustup target add`, or the ESP32-S3
     toolchain from `espup`).
3. **Fix and build again.** Change the smallest thing, run
   `dagon build <id>` again. Stop after 5 attempts and report what you
   tried.
4. **Keep the node alive.** Do not remove the watchdog lease calls or make
   `self_check` always fail; a version that cannot be confirmed is rolled
   back on the device.
5. **Report** the cause, the files changed and the last build result.

Never touch the signing key, `pubkey` in `dagon.toml`, `src/wiring.rs` or
the DAGON package lines of `Cargo.toml`.
