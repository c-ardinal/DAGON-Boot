# Desk sample hardware test

Drives the desk sample through the `dagon` CLI and checks every node after
each step. This tests DAGON together with the sample apps on this one desk
configuration; it is a sample, not part of DAGON. Python 3.11+ standard
library only; Windows (the hub identity check uses PowerShell).

The desk: 親機 Waveshare RP2040-Touch-LCD-1.28 on USB (`hub`), NUCLEO-F401RE
+ X-NUCLEO-53L0A1 on I2C 0x42 (`seg`), M5StopWatch on UART (`watch`), all
connected; a DAGON project for it (`topology.yaml` as
`topology/desk-demo.yaml`, `dagon.toml` with `[build.hub]`, `[build.seg]`,
`[build.watch]`).

## Files

- `hwt.py`: the runner (`sync`, `run`, `selftest`).
- `hwt.toml`: the desk's data (`[desk]`: nodes, coordinator, hub USB id,
  self-check switch name, settle timing, which sample feeds which node) and
  the scenarios, in order.

## Usage

```sh
# 1. once per code change: refresh the node crates from this repo's samples
#    (seg = samples/f401-app, watch = samples/esp32s3-app, paths to this
#    repo), give the hub app the self-check switch, dagon gen hub.
#    Point [dagon] path in the project's dagon.toml at this repo first.
python samples/desk-test/hwt.py sync --project <desk> --repo <this repo> --dagon <dagon.exe>

# 2. every scenario
python samples/desk-test/hwt.py run --project <desk> --dagon <dagon.exe> --key <secret key file> --port <COMn> [--out <log dir>]

# some scenarios, or resume from one (unknown or missing ids are an error)
python samples/desk-test/hwt.py run ... --only status batch final
python samples/desk-test/hwt.py run ... --from interrupt-seg

# no hardware: status parsing, plan check, --only, tampering, key cleanup
python samples/desk-test/hwt.py selftest
```

`--key` is passed to `dagon` as a path; the harness never opens it.

## Safety

- `--port` is required and passed to every CLI call that talks to the
  hub. At the start of each scenario the port must still be the same device
  (Windows PnP id with `hub_usb`, and the hub's MCU id).
- `dagon deploy` is never answered in advance: its plan (version, nodes,
  release file) is read first and must be exactly the step's; otherwise
  the harness answers `n` and fails.
- `dagon rollback`, `apply` and `recover` ask nothing; the harness checks
  what they will do before calling them (rollback: the other slot holds a
  confirmed version; apply: only the deliberately bad releases, which must
  be refused; recover: the named release exists).
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
  A failed restore exits with code 2 and prints the exact state.

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
2 the restore failed.

## Scenarios

status; batch update of all three; seg alone; watch alone; manual rollback
of each node; a failing self-check image on each MCU (the deploy fails, the
node rolls back by itself; the hub takes about 3.5 min); a transfer
interrupted by killing the CLI at 30 % on each node (old version keeps
running, the retry succeeds); a tampered release (plain, and with the item
CRC recomputed) and a release signed with a throwaway key (refused by
`dagon apply` before any write; every slot unchanged); A/B back and forth
three times; `dagon recover seg` while it answers; F401 ROM recovery
(SKIPPED); a final good deploy.

Test versions are 2.0.N (the format is major.minor.patch only, no
pre-release tags).

## Test switches

`no-confirm` makes the node's self-check fail: `samples/f401-app` and
`samples/esp32s3-app` (cargo feature), and the hub app (`sync` patches the
generated `self_check()` in the hub's `src/app.rs` and adds the feature).
`run` builds failing images through a copy of dagon.toml,
`dagon.hwt-fail.toml`, with `features = ["no-confirm"]` for the node.
Nothing in the DAGON packages changes.

## Not automated

- F401 ROM recovery: `dagon recover` uses the ROM path only for an F401
  that does not answer; making it silent needs an erase outside the normal
  confirmation.
- Interrupting by resetting the 親機: the CLI has no command that resets the
  hub on its own; the interrupt is the CLI killed mid-transfer.
