# seg node firmware (NUCLEO-F401RE)

A sample app, not part of DAGON: the demo's seg node, built on the MCU
package (`../../targets/mcu/stm32f401re`: layout, boot core, watchdog,
I2C target serving) and the board package
(`../../targets/boards/nucleo-f401re`: pins, the Arduino I2C the shield sits
on). The X-NUCLEO-53L0A1's 7-segment display is this sample's own: the text
(`src/seg.rs`), the expander writes (`src/shield.rs`, both host-tested:
`cargo test --no-default-features --target <host triple>`) and the driver
on the board's Arduino I2C (`src/display.rs`).

| Address | Size | Content |
|---|---|---|
| 0x08000000 | 32 KiB | boot core (`../../targets/mcu/stm32f401re/bootcore`), signed header in the last 512 B |
| 0x08008000 | 2 × 16 KiB | APP state log faces A/B |
| 0x08010000 | 64 KiB | reserved |
| 0x08020000 | 128 KiB | APP-A: 512-byte header, body at 0x08020200 |
| 0x08040000 | 128 KiB | APP-B: same, body at 0x08040200 |
| 0x08060000 | 128 KiB | reserved |

The boot core tries a pending slot up to 3 times (signature + SHA-256,
BOOT_ATTEMPT before each start, 4 s watchdog) and then rolls back; otherwise
it starts the confirmed slot (8 s watchdog). Both run at 84 MHz from the HSI.

This app is the DAGON node: it starts DAGON through the board package's
public `start` (an I2C target at 0x42 on I2C3, SCL PA8 = D7, SDA PB4 = D5,
serving the update protocol through the mailbox) and shows its own version on
the stacked X-NUCLEO-53L0A1's 4-digit display. The display runs on the
board's Arduino I2C (I2C1, PB8/PB9) from the app's executor and keeps the shield's VL53L0X sensors in
XSHUT. It feeds the watchdog only while confirmed, so
an image that never gets CONFIRM resets when the 4 s trial watchdog runs out
and is rolled back after three attempts. RTT shows the boot state and every request that takes
20 ms or more (`cmd 0x20: N ms` for SET_PENDING).

Version display (this app's own format, `seg::version`):

- `major.minor.patch` right-aligned on the 4 digits, each `.` set as the
  point of the digit before it. The driver does not light the
  shield's decimal point, so the digits show side by side: ` 110` for 1.1.0, ` 120` for 1.2.0, `1107`
  for 1.10.7.
- `major.minor.` when that does not fit (10.10.x: `1010`), else
  `----`.
- Builds with the public test key show `tESt` for 1.5 s first; an
  unreadable state log shows `----`.

Update display (`src/update.rs`, host-tested): the display task follows
the node's published events (`AppEvents`, which the public `start`
returns; dagon-agent's `AppSnapshot` phase and percent) and shows, while
DAGON updates the node:

| Phase | Display |
|---|---|
| transferring | `d` and the number DAGON told the node, the same as the DAGON Console's % (`d---` when it told none) |
| verifying | `ChEc` |
| written | `donE` |
| pending (next boot) | `PEnd` |
| reboot accepted | `boot` |
| on trial | `tri` and the version, alternating every second |
| confirmed | `ConF` for 5 s, then the version |
| rolled back | `roLL` for 5 s, then the version |
| failed | `FAIL` for 5 s, then the version |
| starting, idle | the version |

Features: `slot-b` links for APP-B; `confirm` confirms a version on trial at start-up, without a 親機 (the public start's `Confirm::Own`); `no-confirm` makes the self-check fail
(the trial always rolls back); `conform` is the conformance build (below).

## Build

The verifying key is embedded at build time. `DAGON_PUBKEY` takes the public
key file of `dagon key gen` (or its 64 hex digits). Unset, the build fails,
unless `DAGON_ALLOW_TEST_KEY=1` asks for the public RFC 8032 test key (the
build warns and the display shows `tESt` at start).

```sh
export DAGON_PUBKEY=/path/outside/the/repo/dagon-signing.pub
(cd targets/mcu/stm32f401re/bootcore && DAGON_TARGET_TAG=seg-f401-7seg cargo build --release)
(cd samples/f401-app && cargo build --release)
```

## First install (ST-LINK)

Point `[provision.seg]` in `dagon.toml` at the two release ELFs:

```toml
[provision.seg]
bootcore = "targets/mcu/stm32f401re/bootcore/target/thumbv7em-none-eabihf/release/dagon-stm32f401re-bootcore"
app = "samples/f401-app/target/thumbv7em-none-eabihf/release/f401-app"
version = "1.1.0"
```

```sh
dagon provision seg --key /path/outside/the/repo/dagon-signing.key
probe-rs erase --chip STM32F401RETx
probe-rs download --chip STM32F401RETx --binary-format bin --base-address 0x08000000 out/provision/seg/flash.bin
probe-rs reset --chip STM32F401RETx
```

`flash.bin` holds the signed boot core, state log face A (APP-A provisioned
and confirmed) and APP-A; the chip erase blanks face B. The display shows
` 110`. RTT: `probe-rs attach --chip STM32F401RETx
samples/f401-app/target/thumbv7em-none-eabihf/release/f401-app` prints
`app 1.1.0 slot 0`.

## Conformance build

`--features conform` adds CONF_RUN in front of the agent: the node-side
tests of dagon-conform (CT-01 flash geometry, CT-02 erase, CT-03 aligned
write, CT-07 Ed25519, CT-08 SHA-256), answered through the same I2C mailbox.

- Declared layout: the board's writable regions (log faces 0x08008000,
  2 × 16 KiB; APP slots 0x08020000, 2 × 128 KiB).
- Scratch: the start of the APP slot that is neither running, nor
  confirmed, nor pending (APP-B while running a confirmed APP-A with no
  update staged). CT-02/CT-03 erase that whole 128 KiB sector, so an old
  image there is gone afterwards. With no such slot (on trial, or an update
  staged) CT-02/CT-03 fail ("outside flash") without erasing.
- Watchdog: 12 s for this build, on trial too (about 8.2 s if the LSI runs
  at its fast end), fed through the tests' feed hook, so a sector erase (up
  to 4 s) cannot reset the node. An unconfirmed trial of this build is
  therefore reset 12 s after it stops feeding instead of 4 s.
- CT-02 and CT-03 each erase the sector twice; the node NACKs its address
  for the whole test, up to about 8 s. The other behaviour is the same as
  the normal app.

Write it with ST-LINK (not through the hub), as a fresh first install:

```sh
export DAGON_PUBKEY=/path/outside/the/repo/dagon-signing.pub
(cd targets/mcu/stm32f401re/bootcore && DAGON_TARGET_TAG=seg-f401-7seg cargo build --release)
(cd samples/f401-app && cargo build --release --features conform --target-dir target/conform)
```

Point `[provision.seg]` `app` at
`samples/f401-app/target/conform/thumbv7em-none-eabihf/release/f401-app`
(boot core as above), then:

```sh
dagon provision seg --key /path/outside/the/repo/dagon-signing.key
probe-rs erase --chip STM32F401RETx
probe-rs download --chip STM32F401RETx --binary-format bin --base-address 0x08000000 out/provision/seg/flash.bin
probe-rs reset --chip STM32F401RETx
```

The display shows the version as usual; connect the hub (I2C3 as above)
and run the conformance sequence from the PC. Switch `app` back to the
normal build and provision again to leave the conformance build.
