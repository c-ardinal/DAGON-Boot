---
name: dagon-port
description: Autonomously port DAGON Boot to a new MCU and board, iterating until the conformance suite passes through the hub or a time / iteration limit is reached.
argument-hint: <mcu> <board> <S|V> [--max-hours N] [--max-iterations N] [--rerun]
arguments: [mcu, board, kind]
disable-model-invocation: true
---

# Port DAGON Boot to $mcu on $board (port kind $kind)

Full arguments: `$ARGUMENTS`

## Inputs

| Input | Meaning |
|---|---|
| `mcu` | MCU name, for example `esp32s3`. Used as the `targets/<mcu>-*` prefix. |
| `board` | Board name, for example `m5stopwatch`. |
| `kind` | Port kind, `S` or `V`, as defined in `crates/dagon-port`. Stop and ask if it is neither. |
| `--max-hours N` | Wall-clock limit. Default 6, or 3 with `--rerun`. |
| `--max-iterations N` | Iteration limit. Default 30. |
| `--rerun` | Reproduction run of an earlier successful port (shorter default limit). |

## Ground rules

- `crates/dagon-conform`, `crates/dagon-sim` and `crates/dagon-core` are read-only for the
  whole run; a guard hook enforces it. Never weaken or skip a test. If a test itself looks
  wrong, stop and report to a human.
- Shared crates (`crates/*`) are not edited. If one needs a change, stop and hand a change
  request to a human.
- A pass is judged only from `dagon conform` measured through the hub, as reported by the
  `conformance-judge` agent. A simulator pass or your own reading of the code is not a pass.
- Branches and tags of the run stay local. Never `git push`.

## Setup

1. Record the start time. Let `RUN=<mcu>-<board>-<YYYYMMDD-HHMM>`.
2. Create a dedicated worktree and branch for this run, and work only inside it:
   `git worktree add ../dagon-port-$RUN -b port/$RUN`
   (the `port/` prefix marks a port session for the guard hook).
3. If this is an ESP32-S3 target, delegate implementation work to the `port-engineer` agent;
   otherwise implement under `targets/<mcu>-<board>/` yourself under the same rules.

## Steps

1. **Layout table with sources.** From primary sources only (datasheet, technical reference
   manual, vendor SDK docs), write a table of flash layout (bank / slot addresses and sizes,
   erase granularity, write alignment), boot method (reset vector or ROM boot flow, boot
   mode pins) and link layout (memory regions the linker script needs). Every row cites
   its source (document, section or page, URL). If a value cannot be found, ask the human.
2. **Create the target.** Create `targets/<mcu>-<board>/` as a crate in the workspace.
3. **Implement the six traits** from `crates/dagon-port` for kind `$kind`, plus the linker
   script and boot glue the table requires. Build until clean.
4. **Simulator.** Ask `conformance-judge` to run `dagon sim` scenarios S1 and S2 with this
   layout. Fix failures before going to hardware.
5. **Hardware conformance.** Ask `conformance-judge` to run `dagon conform` through the hub
   (all 12 normal-path tests). Hub access only inside the slot a human has granted.
6. **Fix loop.** For each failing test, read the judge's reason, fix the target, commit on
   the run branch (`fix(<mcu>): <what> for <test id>`), and go back to step 4. Each pass
   through steps 4-6 is one iteration.
7. After every iteration, write `targets/<mcu>-<board>/port-progress.json`:
   `{"run": "<RUN>", "iteration": N, "elapsed_s": S, "passed": P, "total": 12}` and keep the
   result file `dagon conform` wrote for that iteration.

## Stop conditions

Stop when the first of these holds, then report to the human:

- all 12 tests pass through the hub (success),
- elapsed time reaches `--max-hours` (default 6 h, 3 h with `--rerun`),
- iteration count reaches `--max-iterations` (default 30),
- a required value cannot be determined, a shared crate needs a change, or a test looks wrong.

## Finish

1. Tag the final commit locally: `git tag port-$RUN-pass` on success, `port-$RUN-stop` otherwise.
   Do not push the branch or the tag.
2. Write `targets/<mcu>-<board>/PORT_REPORT.md`: inputs, the layout table with sources,
   iteration count, total time, per-iteration fix history (test id, judge reason, change,
   commit), final judge totals, and open issues. Commit it on the run branch.
3. Report to the human: outcome, iterations, elapsed time, tag name, report path.
