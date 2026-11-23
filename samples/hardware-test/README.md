# Sample hardware test

Drives the sample through the `dagon` CLI and checks every node after
each step. This tests DAGON together with the sample apps on this one setup
configuration; it is a sample, not part of DAGON. Python 3.11+ standard
library only; Windows (the coordinator identity check uses PowerShell).

The setup: a Waveshare RP2040-Touch-LCD-1.28 on USB (`hub`), NUCLEO-F401RE
+ X-NUCLEO-53L0A1 on I2C 0x42 (`seg`), M5StopWatch on UART (`watch`), all
connected; a DAGON project for it (`topology.yaml` as
`topology/sample.yaml`, `dagon.toml` with `[build.hub]`, `[build.seg]`,
`[build.watch]`).

## Files

- `hwt.py`: the runner (`sync`, `run`, `selftest`).
- `hwt.toml`: the setup's data (`[setup]`: nodes, coordinator, hub USB id,
  self-check switch name, settle timing, which sample feeds which node) and
  the scenarios, in order.
- `deploy-plan.txt`: the plan `dagon deploy` prints before it asks, as the
  CLI printed it (two nodes on 2.0.7, the package path shown as
  `<release>`); `selftest` checks the plan reader against it. DAGON's own
  test of that plan is `the_plan_names_the_package_and_each_node_s_old_and_new_version`
  (`cargo test -p dagon-cli -- the_plan_names --nocapture` prints it).
- `measure.py`: the measurements the firmware has a measuring build for
  (see Measurements); `test_measure.py` its tests without hardware.

## Usage

```sh
# 1. once per code change: refresh the node crates from this repo's samples
#    (seg = samples/f401-app, watch = samples/esp32s3-app, paths to this
#    repo), give the coordinator app the self-check switch, dagon gen hub.
#    Point [dagon] path in the project's dagon.toml at this repo first.
python samples/hardware-test/hwt.py sync --project <project> --repo <this repo> --dagon <dagon.exe>

# 2. every scenario
python samples/hardware-test/hwt.py run --project <project> --dagon <dagon.exe> --key <secret key file> --port <COMn> [--out <log dir>]

# some scenarios, or resume from one (unknown or missing ids are an error)
python samples/hardware-test/hwt.py run ... --only status batch final
python samples/hardware-test/hwt.py run ... --from interrupt-seg

# no hardware: status parsing, plan check (deploy-plan.txt), crate versions,
# --only, tampering, key cleanup, restore
python samples/hardware-test/hwt.py selftest
```

`--key` is passed to `dagon` as a path; the harness never opens it.

## Safety

- `--port` is required and passed to every CLI call that talks to the
  hub. At the start of each scenario the port must still be the same device
  (Windows PnP id with `hub_usb`, and the coordinator's MCU id).
- `dagon deploy` is never answered in advance: its plan (each node and the
  version it goes to, and the package file) is read first and must be
  exactly the step's; otherwise the harness answers `n` and fails.
- `dagon rollback` and `apply` ask nothing; the harness checks
  what they will do before calling them (rollback: the other slot holds a
  confirmed version; apply: only the deliberately bad releases, which must
  be refused).
- The throwaway key of the foreign-key scenario is made in a temporary
  folder and deleted as soon as its release is built; only its key_id is
  recorded.
- The status of every node at the start of the run is recorded
  (`start_state` in results.json).
- On any failure (Ctrl-C included) the run stops and restores: it waits
  out trials (reads only), checks the port is still the same coordinator (if not,
  it writes nothing), then
  - applies the last release that reached every node in this run (only
    the recorded copy `known-good-v<version>.dgb`, unchanged since: its
    SHA-256 is checked) and checks every node is confirmed on it; or,
  - with no such release yet, rolls each changed node back to its start
    version with `dagon rollback` (only when its other slot holds that
    version, all checked before the first write) and checks every node is
    on its start version, confirmed.
  A failed or unverified restore (no start state could be read) exits
  with code 2 and prints the exact state.

## Output

`<out>/<yyyymmdd-hhmmss>/` (default `~/dagon-hwt/`):

- `steps.jsonl`: one record per step: time, scenario, command, exit code,
  the status of every node (version, on trial, MCU, active and confirmed
  slot, attempts, last reason, each slot's state and version), the verdict;
- `NNN-<scenario>-<step>-<kind>.log`: each CLI call's full output,
  including the harness's answer to the prompt;
- `results.json`: port and hub identity, PASS / FAIL / SKIPPED per
  scenario, the restore record and the final status.

Exit code: 0 all passed, 1 a scenario failed and the restore succeeded,
2 the restore failed or could not be verified.

## Scenarios

status; batch update of all three; seg alone; watch alone; manual rollback
of each node; a failing self-check image on each MCU (the deploy fails, the
node rolls back by itself; the coordinator takes about 3.5 min); a transfer
interrupted by killing the CLI at 30 % on each node (old version keeps
running, the retry succeeds); a tampered release (plain, and with the item
CRC recomputed) and a release signed with a throwaway key (refused by
`dagon apply` before any write; every slot unchanged); A/B back and forth
three times; each boot core with a release, for seg and the coordinator (`core-seg`,
`core-hub`): raised, it goes to the idle slot and is confirmed; raised again,
back to the other slot; a failing one goes back to the confirmed slot and the
node still answers, all read from `dagon status`' versions lines; F401 ROM
write (SKIPPED); the hub updating itself ten times (`hub-x10`); a final
good deploy.

Test versions are 2.0.N (the format is major.minor.patch only, no
pre-release tags). A node's version is its crate's: before each deploy step
`run` sets `[package] version` in the Cargo.toml of each node it deploys
(the `[build.<id>]` package) to the step's version, and the crates keep the
last one. A crate whose version is inherited from a workspace is refused;
set it in the crate itself.

## Test switches

`no-confirm` makes the node's self-check fail: `samples/f401-app` and
`samples/esp32s3-app` (cargo feature), and the coordinator app (`sync` patches the
generated `self_check()` in the coordinator's `src/app.rs` and adds the feature).
`run` builds failing images through a copy of dagon.toml,
`dagon.hwt-fail.toml`, with `features = ["no-confirm"]` for the node.

The same feature in a boot crate makes the core fail its own check, so stage0
falls back to the confirmed core; `dagon gen` writes it into `boot/<id>/` and
the core is built with it through `boot_features = ["no-confirm"]` in the same
copy. A first write never builds a core with it. The `core` steps need boot
crates from this DAGON: run `dagon gen` first.

## Not automated

- F401 ROM write: `dagon deploy` uses the ROM path only for an F401
  that does not answer; making it silent needs an erase outside the normal
  confirmation.
- Interrupting by resetting the coordinator: the CLI has no command that resets the
  coordinator on its own; the interrupt is the CLI killed mid-transfer.

## The hub seen again after it updates itself

An RP2040 hub reboots by its watchdog after each update; the PC must see it
again, without replugging, for the update to reach confirm. `hub-x10`
updates the hub alone ten times; each step passes only when the hub is back
and confirmed on the new version. Run it twice: with the hub behind a USB
hub, then plugged directly into the PC.

```sh
python samples/hardware-test/hwt.py run ... --only hub-x10 --out <log dir>   # through a USB hub
python samples/hardware-test/hwt.py run ... --only hub-x10 --out <log dir>   # directly
```

Both `results.json` show `hub-x10` PASS (20 of 20 updates confirmed). Turn
off Windows' USB selective suspend for the run, so a suspended port is not
mistaken for the node's fault. If a step stays unseen, before replugging
note what USBView shows for the port, then try Device Manager's "Scan for
hardware changes", and record which one brought the hub back.

To tell where a lost reconnect comes from, the same three restarts can be
tried by hand through the USB hub on a build from before the node
re-attaches by itself (the first parent of merge 337eeaaf): a reset by the
RUN pin, and the restart after `dagon rollback hub`; for each, whether the
PC sees the hub again. (A REBOOT without an update has no CLI command.)

## Measurements

`measure.py` takes the values the firmware has a measuring build for. It
writes the node with that build through `dagon flash` (its plan and
question come up as usual), reads the value, checks it against its limit,
then writes the node's normal build back through `dagon flash` again. The
node is erased by both writes: it comes back on its first-install state, so
run `hwt.py` after measuring, not in between.

```sh
# stage0's signature check of the boot core (each built-in MCU with a stage0)
python samples/hardware-test/measure.py stage0-verify --project <project> --dagon <dagon.exe> --node seg --key <secret key file> [--probe <sel>]
python samples/hardware-test/measure.py stage0-verify --project <project> --dagon <dagon.exe> --node hub --key <secret key file> --drive <UF2 drive>
python samples/hardware-test/measure.py stage0-verify ... --node hub --drive <UF2 drive> --probe <sel> --chip RP2040

# the STM32F401 boot core's listen window
python samples/hardware-test/measure.py listen-time --project <project> --dagon <dagon.exe> --node seg --key <secret key file> [--probe <sel>]

# a run stopped before it put a Cargo.toml back
python samples/hardware-test/measure.py restore --project <project>

# no hardware
python samples/hardware-test/measure.py selftest
```

- `stage0-verify`: stage0 with `verify-time` times its check of the boot
  core and starts no core. Through a debug probe (the node's writer is
  probe-rs, or `--probe` with `--chip`, e.g. `--chip RP2040`, on a UF2
  node: the write still goes to `--drive`, the probe only reads) it reads
  `DAGON_VERIFY_CYCLES` and
  `DAGON_VERIFY_COUNT` and gives the seconds at the clock the build chose
  (`clock.rs` of the build). Without one it asks for the blink code on the
  board's diagnostic output: the fast blinks between two dark pauses, in
  tenths of a second, rounded up. The project must build from a DAGON clone
  (`[dagon] path`): the feature goes into that clone's stage0 Cargo.toml.
- `listen-time`: the boot core with `listen-time` keeps the microseconds
  from its start to the end of the listen window in `DAGON_LISTEN_US` and
  stays there. PASS when it is at most the node's `listen_window_ms` + 10 ms
  (`boot/<id>/mcu.toml`, 50 when left out). The feature goes into the
  node's boot crate (`boot/<id>/Cargo.toml`).

The feature is named in the package's `default` features for the one
write; the file is put back byte for byte afterwards, also on a failure or
Ctrl-C (`Cargo.toml.measure-backup` holds the original until then).
A measurement that fails part way still writes the normal build back.
Records go to `<out>/<yyyymmdd-hhmmss>/` (default `~/dagon-measure/`):
`steps.json` (each command, the ELF and the symbol addresses, the words
read) and `result.json` (the value, its limit, PASS / FAIL, whether the
normal build went back). Exit code: 0 PASS, 1 FAIL with the normal build
back, 2 the normal build could not be written back.
