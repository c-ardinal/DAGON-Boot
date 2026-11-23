---
title: "Retained and persistent formats"
description: "The MCU identity, the compiled topology's layout, the boot loader trial cell and the state log."
---

For DAGON contributors. This page is the design's norm; where the code differs from it is in [Limitations](/en/explanation/limitations/).

This document is the normative specification of the formats that
[lifecycle-events.md section 5.5](/en/explanation/lifecycle-events/#55-mcu-identity-and-compiled-topology-requirements-and-the-executable-model)
leaves to an executable model: the MCU identity and the compiled
topology's node layout. It also specifies the boot loader trial cell
(section 5) and the state log's faces and records (section 6). Every
shape here is the one `crates/dagon-retained-model` proves; a change to a
shape is a change to the model first, and the model's tests decide.

The model is host-only and not released. Each format's codec is a trait in
the model (`BodyCodec`), proven on the host. By design,
a firmware codec implements the same trait and is tested under the same
harness.

```
cargo test -p dagon-retained-model                       # 10^4 scenarios per world, fixed cases
cargo test -p dagon-retained-model --release -- --ignored # 10^6 scenarios per world
DAGON_MODEL_SCENARIOS=<n> DAGON_MODEL_SEED=<seed>         # override count and base seed
```

A failing scenario prints its seed, its configuration and its operation
list shrunk to the smallest list that still fails the same property; the
case is added to `tests/fixed.rs` as a regression.

## 1. Fault model

What the proofs assume of the hardware and what they inject:

- **Stores.** Retained memory is written by byte stores in program order
  (volatile stores with a data memory barrier between the steps of one
  write). A wider store is allowed when it does not combine bytes of two
  different steps; a 32-bit-only register file rewrites a step's
  neighbouring bytes with their current values. A run may stop after any
  byte (a reset or a power cut); the model injects that stop after every
  byte of every write.
- **Reset** keeps retained bytes. What **power loss** keeps depends on
  the place (5).
- **NOR flash** erases to `0xFF` and programs bits from 1 to 0. An
  interrupted erase leaves a prefix erased and the rest old or random; an
  interrupted program leaves a prefix programmed. Every byte, including a
  commit byte whose program was interrupted, reads the same on every read
  until the next erase or program (no weak bits); flash that can return
  different values for a partly programmed byte needs its MCU definition to
  read the commit byte back until two reads agree.
- **Foreign bytes.** Any byte may also be overwritten by something that is
  not the format's writer: a writer of another layout of the same stage,
  or a fault. The model injects random bytes into every region.

Every guarantee below is
structural: it holds for every interruption point, not with a
probability.

## 2. MCU identity

`identity(id)` = the first 8 bytes of `SHA-256("dagon-mcu/" ‖ id)` as a
little-endian u64, where `id` is the MCU definition's `id`. Every carrier
holds all 64 bits. Carriers: `image-header`, `identity-reply`,
`version-0x06`, `topology-node` (the model's `identity::CARRIERS`; a test
checks this line against it). The definition loader refuses two
definitions whose identities agree in the width of any carrier, naming
both ids, and refuses an identity that is all zeros or all ones in a
carrier's width. A device whose identity matches no loaded definition
exactly is shown as unknown. No numeric MCU table exists.

## 3. Compiled topology

### 3.1 Body (`topology-bin` 2)

Head, 16 bytes: magic `DGTP`, layout u16 LE (2), node count (1 to `MAX_NODES`), link
count (≤ `MAX_LINKS`), constraint count (≤ `MAX_CONSTRAINTS`), 7 zero bytes. Then:

- nodes, 40 bytes each: index, role, port kind, max attempts, listen
  window ms u16 LE, map id, order rank, recovery tiers [3], 5 zero bytes,
  MCU identity u64 LE at offset 16, target tag [16] at offset 24;
- links, 20 bytes each, in the image tools' link layout: no direction, two
  ends, each with its node, port index and address;
- constraints, 16 bytes each, in the manifest's constraint layout.

Canonical: node `index` equals its position, a link's ends in node order,
links sorted by their ends' nodes, reserved bytes zero; layout 1 (16-byte
links) is not read; a decoder re-encodes and compares.

## 4. Properties

The model checks these after every operation of every scenario; the
fixed cases in `tests/fixed.rs` name the property they guard.

| Id | Property |
|---|---|
| P12 | Identity is a pure function of the id; collisions refused at every carrier width; unknown never matched. |

## 5. Boot loader trial cell

Stage0 counts the starts of a pending boot loader in two retained words,
not in flash: stage0 then has no flash writer (it is never written again, so it
holds as little code as it can), and a writer bug cannot turn into a
stage0 that never starts anything. `dagon_core::bootcore` is the codec and
its tests are the proof.

| MCU | Place | Kept through | Lost with |
|---|---|---|---|
| RP2040 | watchdog SCRATCH0, SCRATCH1 (the boot ROM uses SCRATCH4-7) | every reset but power-on and the RUN pin | power |
| STM32F401RE | RTC backup registers BKP0R, BKP1R | every system reset, the watchdog's included | the backup domain's supply (VDD where VBAT is tied to it) |

Value for start `n` (at most 15) of the pending image `p`: one word,
`0xB7 << 24 | p.digest16[0] << 16 | p.digest16[1] << 8 | n << 4 | (!n &
0xF)`, stored in both places. A word counts for `p` only when its tag,
its digest bytes and its count check (the low nibble is the count's
complement) hold; the cell counts the larger of its two words' counts,
and `[0, 0]` is the cleared cell. Each word is one store, so a reset can
land between the two: one new word and one old word then still count the
new start, never fewer. A random
word matches with probability 2^-28, and a match only makes stage0 give
the core up sooner.

The executable model is `dagon-retained-model`'s `world_trial`: the
production codec and stage0's and the boot loader's rules
(`bootcore::choose`, `settle`) over the two words, new cores set pending
good or broken (a broken core passes stage0's check and fails its own),
a reset or a power loss (the cell then cleared or random) after any store
to the cell (the cell check's included, in the MCUs' order: word 0, then
word 1), seeded scenarios in `cargo test` and many more in
the full profile. Its properties, checked after every operation:

| Id | Property |
|---|---|
| C1 | Between two power losses a pending core is started on trial at most `max` times. |
| C2 | With no further fault, at most `max + 1` boots end the trial: no core is pending and a core runs. |
| C3 | A broken core is never confirmed. |

The MCU definitions' sweeps (`a_power_cut_at_every_flash_step_leaves_a_bootable_core`)
add the flash side: a cut at every flash operation of a write,
compaction of the state log included.

Kani proofs (`crates/dagon-core/src/proofs.rs`) check stage0's decision
for every input: the cell's properties (at most 15 counted, an update cut
between its two stores never lowers the count, a cell for another image
counts nothing), the plan and settle rules, and that reading the state
log and the assembly of the decision (`bootcore::start`) do not panic on
any flash and retained content (the image checks replaced by any result). The F401 package's exhaustive search
(`targets/mcu/stm32f401re/tests/explore.rs`) checks the whole node's
combinations. [Working on DAGON itself: formal verification](/en/explanation/repository/#formal-verification)
says how to run them.

A cell that does not hold what is
written to it at all is found by `bootcore::cell_holds`, run by the
started boot loader and by the app: such a node takes no boot loader, and a
pending core that finds it so rolls itself back.

A cell with a word that
holds a count was written by stage0, so it holds, and the check writes
nothing over it; only a cell counting nothing gets a probe (a value no
cell word takes) and its old value back, word by word, each read back, so
a reset inside the check never leaves the cell counting fewer starts.

### 5.1 App hand-off

An executing ESP image cannot map the app over itself, so on ESP32-S3
stage0 starts the app and the boot loader asks for it by reset. The request
and the result use the trial cell's two words; the cell holds one tagged
value at a time.

| Tag | Value | Written by | Counts when |
|---|---|---|---|
| `0xB7` | trial count (above) | stage0 | either word holds it |
| `0xB8` | `START_APP`: the fixed word `0xB8C33C5A` twice | boot loader | both words hold it |
| `0xB9` | `app_result`: `0xB9 << 24 \| 0x5A << 16 \| 0xA5 << 8 \| c << 4 \| (!c & 0xF)` twice (`c` 1: `NoImage`, 2: `Rejected`) | stage0 | either word holds it |

The boot loader settles its own trial, writes `START_APP` (word 0, then
word 1) and resets. Stage0 takes the request before it chooses a boot
loader (`take_request`): it clears the cell, word 0 first, and runs the app
decision once, only when the cell then reads cleared. When the decision
starts no app, stage0 writes the result and starts only the confirmed
boot loader, which reads it (`hand_off`), clears the cell and stays in
recovery mode until an image or the log's choice changes.

A request counts only when both words hold it, so a reset inside its
write or its take leaves none: one request runs the decision at most
once, and an unfinished write runs none (the boot loader asks again at its
next start). A result only keeps the boot loader from asking, so either
word counts it; a result lost to a reset costs one more decision.
Requests and results count no trial start and never read as each other
(Kani proof `the_cell_values_never_read_as_each_other`).

The executable model is `dagon-retained-model`'s `world_hand_off`: the
production rules (`take_request`, `app_result`, `hand_off`,
`cell_holds`, and `choose`'s cell for the confirmed core) with a reset or
a power loss after every store of the request, the take, the result, the
confirmed core's start, the cell checks and the clearing of a result, and
the app made good, absent or broken. Its properties, checked after every
operation:

| Id | Property |
|---|---|
| H1 | Stage0 runs the app decision only on a request the boot loader wrote whole, at most once for each. |
| H2 | With no further fault, one boot ends with the app running when it is good, and otherwise with the boot loader in recovery mode, after at most two decisions. |
| H3 | The boot loader stays in recovery only on a result stage0 wrote for the app as it is. |

Limit of the claims: that each place keeps its value across the resets
named is the vendor's documented behaviour.

## 6. Faces and records of the state log (`state-log` 1)

The state log holds the records of the image slots. This section is the norm of its faces and records. The faces, the face header and the fold of the slot records are those of `dagon_format::statelog` and `dagon_core::log`.

The log is two faces used in turn. A face is a 32-byte face header followed by 32-byte records; erased bytes are `0xFF`. The valid face with the larger generation is the active one. Its records are folded in order up to the first erased record or the first record that fails its CRC.

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | Kind: `0x01` to `0x7F` a slot record, `0x80` to `0xFE` reserved, `0xFF` erased |
| 1 | 3 | Fields of the kind |
| 4 | 4 | `seq`: sequence number, little-endian. No fold uses it |
| 8 | 20 | Fields of the kind |
| 28 | 4 | CRC-32 (ISO-HDLC) of bytes 0 to 27 |

A record is appended into the first erased record of the active face, after a check that all its 32 bytes are erased. When the face is full, ends in a record that fails its CRC, or fails that check, the append compacts first: it erases the other face, writes the fold there and writes that face's header with the generation plus 1 last. Until that header is whole the old face stays the valid one. A reader skips a record whose kind it does not know.
