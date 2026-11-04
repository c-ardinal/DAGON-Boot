# Desk hardware test

Drives the real desk through the `dagon` CLI and checks every node after
each step. Python 3.11+ standard library only; Windows-friendly.

The desk: 親機 Waveshare RP2040-Touch-LCD-1.28 on USB (`hub`), NUCLEO-F401RE
+ X-NUCLEO-53L0A1 on I2C 0x42 (`seg`), M5StopWatch on UART (`watch`), all
connected; a DAGON project for it (`topology.yaml`, `dagon.toml` with
`[build.hub]`, `[build.seg]`, `[build.watch]`, `nodes/hub`, `nodes/seg`,
`nodes/watch-app`).

## Usage

```sh
# 1. once per code change: the node crates from this repo's samples
#    (seg = samples/f401-app, watch = samples/esp32s3-app, paths to this
#    repo), the hub app's no-confirm switch, dagon gen hub.
#    Point [dagon] path in the project's dagon.toml at this repo first.
python hardware-test/desk/hwt.py sync --project <desk> --repo <this repo> --dagon <dagon.exe>

# 2. every scenario (stops at the first unexpected state)
python hardware-test/desk/hwt.py run --project <desk> --dagon <dagon.exe> --key <secret key file> [--out <log dir>]

# one or some scenarios, or resume from one
python hardware-test/desk/hwt.py run ... --only seg-alone tampered
python hardware-test/desk/hwt.py run ... --from interrupt-seg

# parser self-check (no hardware)
python hardware-test/desk/hwt.py selftest
```

`--key` is passed to `dagon` as a path; the harness never opens it.

## Output

`<out>/<yyyymmdd-hhmmss>/` (default `~/dagon-hwt/`):

- `steps.jsonl`: one record per step: time, scenario, command, exit code,
  the status of every node (version, on trial, active and confirmed slot,
  attempts, last reason, each slot's state and version) and the verdict;
- `NNN-<scenario>-<step>-<kind>.log`: each CLI call's full output;
- `results.json`: PASS / FAIL / SKIPPED per scenario and the final status.

## Scenarios

`scenarios.toml`, in order: status; batch update of all three; seg alone;
watch alone; manual rollback of each node; a failing self-check image on
each MCU (feature `no-confirm`: the deploy fails, the node rolls back by
itself); a transfer interrupted by killing the CLI at 30 % on each node
(old version keeps running, the retry succeeds); a tampered release and a
release signed with a throwaway key (refused by `dagon apply` before any
write; every slot unchanged); A/B back and forth three times; `dagon
recover seg` while it answers; F401 ROM recovery (SKIPPED, see the file);
a final good deploy.

Test versions are 2.0.N (the format is major.minor.patch only, no
pre-release tags). The run ends with every node on the final version,
confirmed; a run stopped early leaves the nodes as they are for
inspection.

## Test switches

`no-confirm` makes the node's self-check fail: `samples/f401-app` and
`samples/esp32s3-app` (cargo feature), and the hub app (`sync` patches the
generated `self_check()` in `nodes/hub/src/app.rs` and adds the feature).
`run` builds failing images through a copy of dagon.toml,
`dagon.hwt-fail.toml`, with `features = ["no-confirm"]` for the node.
Nothing in the DAGON packages changes.

## Not automated

- F401 ROM recovery: `dagon recover` uses the ROM path only for an F401
  that does not answer; making it silent needs an erase outside the normal
  confirmation.
- Interrupting by resetting the 親機: the CLI has no command that resets the
  hub on its own; the interrupt is the CLI killed mid-transfer.
