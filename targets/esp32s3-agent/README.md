# esp32s3-agent (factory firmware)

DAGON's own factory firmware for the M5StopWatch: the recovery agent that
goes into the factory partition. It is the same agent as in an app, started
through the board package with `NodeConfig::factory()` (HELLO mode 3), and
nothing else. The 2nd stage boots it when otadata selects nothing bootable;
from there an image is received into ota_0 or ota_1 and handed over with
SET_PENDING, REBOOT and CONFIRM. Details: `targets/mcu/esp32s3/README.md`.

```sh
# from this directory (espup toolchain; DAGON_PUBKEY set to the release key)
cargo build --release          # binary `factory`
```

First install and the conformance runs: `samples/esp32s3-app/README.md`.
Feature `pmic-download` forwards to the board package (ENTER_ROM method 2,
unconfirmed values).
