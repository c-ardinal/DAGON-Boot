# Hub firmware (RP2040-Touch-LCD-1.28)

Three images share the 4 MiB flash:

| Offset | Size | Content |
|---|---|---|
| 0x000000 | 16 KiB | boot2 (generic 03h) + stage0 (`../rp2040-stage0`), never updated |
| 0x004000 | 2 × 4 KiB | boot core state log A/B |
| 0x006000 | 2 × 4 KiB | app state log A/B |
| 0x008000 | 128 KiB | boot core slot A (`../rp2040-bootcore`); slot B at 0x028000 is reserved |
| 0x048000 | 1 MiB | hub app slot A (this package); slot B at 0x148000 |
| 0x248000 | 4 KiB | topology display copy (XFER target 3) |

Each slot starts with the 512-byte signed image header; the body (vector
table) follows at +0x200. The app is linked per slot: default A,
`--features slot-b` for B.

stage0 picks the boot core slot from its log (slot A while empty) and checks
its SHA-256. The boot core picks the app slot: a pending slot is tried up to
3 times (signature + SHA-256, BOOT_ATTEMPT before the jump) and then rolled
back; otherwise the confirmed slot is started after a SHA-256 check. The
watchdog runs at 8 s. The app feeds it while confirmed, and on trial only for
60 s after boot, so an unconfirmed app is reset and counted.

## Build

The verification key is embedded at build time from the public key file of
`dagon key gen`:

```sh
export DAGON_PUBKEY=/path/outside/the/repo/dagon-signing.pub
(cd targets/rp2040-stage0 && cargo build --release)
(cd targets/rp2040-bootcore && cargo build --release)
(cd targets/rp2040-hub && cargo build --release)
(cd targets/rp2040-hub && cargo build --release --features slot-b --target-dir target/slot-b)
```

## Initial install UF2

```sh
elf2uf2-rs targets/rp2040-stage0/target/thumbv6m-none-eabi/release/rp2040-stage0 out/stage0.uf2
elf2uf2-rs targets/rp2040-bootcore/target/thumbv6m-none-eabi/release/rp2040-bootcore out/bootcore.uf2
elf2uf2-rs targets/rp2040-hub/target/thumbv6m-none-eabi/release/rp2040-hub out/hub-a.uf2
(cd targets/rp2040-hub/pack && cargo run --release --target <host triple> -- \
    --stage0 ../../../out/stage0.uf2 --bootcore ../../../out/bootcore.uf2 \
    --hub ../../../out/hub-a.uf2 --key /path/outside/the/repo/dagon-signing.key \
    --version 1.0.0 --out ../../../out/hub-install.uf2)
```

`pack` signs both headers, writes both logs (PROVISION + CONFIRMED for slot
A) and reads the result back the way stage0 and the boot core will before
writing the UF2. Hold BOOT, plug in USB (or press RESET), and copy
`hub-install.uf2` to the `RPI-RP2` drive.

## On the device

- USB: CDC ACM device, product "DAGON hub", manufacturer "DAGON", serial
  number = flash unique ID (16 hex digits), VID:PID c0de:cafe.
- LCD: the hub in the centre, sub nodes below (from the topology display
  copy, else seg on I2C0 0x42 and watch on the PIO UART), each with its
  version; ring colour green = confirmed, blue = on trial, red = no answer,
  grey = not polled yet.
- Frames on USB: RELAY (link 1 = I2C0, 2 = PIO UART) is forwarded; HELLO,
  STATUS, XFER_*, SET_PENDING, REBOOT and CONFIRM are answered by the hub's
  own agent. Other commands answer E_UNSUPPORTED.
