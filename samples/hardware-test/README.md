# Sample hardware test

Drives the sample through the `dagon` CLI and checks every node after
each step. This tests DAGON together with the sample apps on this one setup
configuration; it is a sample, not part of DAGON. Python 3.11+ standard
library only; Windows (the hub identity check uses PowerShell).

The setup: 親機 Waveshare RP2040-Touch-LCD-1.28 on USB (`hub`), NUCLEO-F401RE
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

## Usage

```sh
# 1. once per code change: refresh the node crates from this repo's samples
#    (seg = samples/f401-app, watch = samples/esp32s3-app, paths to this
#    repo), give the hub app the self-check switch, dagon gen hub.
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
  (Windows PnP id with `hub_usb`, and the hub's MCU id).
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
  out trials (reads only), checks the port is still the same hub (if not,
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
node rolls back by itself; the hub takes about 3.5 min); a transfer
interrupted by killing the CLI at 30 % on each node (old version keeps
running, the retry succeeds); a tampered release (plain, and with the item
CRC recomputed) and a release signed with a throwaway key (refused by
`dagon apply` before any write; every slot unchanged); A/B back and forth
three times; each boot core with a release, for seg and the hub (`core-seg`,
`core-hub`): raised, it goes to the idle slot and is confirmed; raised again,
back to the other slot; a failing one goes back to the confirmed slot and the
node still answers, all read from `dagon status`' versions lines; F401 ROM
write (SKIPPED); a final good deploy.

Test versions are 2.0.N (the format is major.minor.patch only, no
pre-release tags). A node's version is its crate's: before each deploy step
`run` sets `[package] version` in the Cargo.toml of each node it deploys
(the `[build.<id>]` package) to the step's version, and the crates keep the
last one. A crate whose version is inherited from a workspace is refused;
set it in the crate itself.

## Test switches

`no-confirm` makes the node's self-check fail: `samples/f401-app` and
`samples/esp32s3-app` (cargo feature), and the hub app (`sync` patches the
generated `self_check()` in the hub's `src/app.rs` and adds the feature).
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
- Interrupting by resetting the 親機: the CLI has no command that resets the
  hub on its own; the interrupt is the CLI killed mid-transfer.
