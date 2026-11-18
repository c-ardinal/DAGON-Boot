---
title: "Retained and persistent formats"
description: "The boot record, the lockup fuse, the MCU identity, the compiled topology's layout and the keyed records of the state log."
---

For DAGON contributors. This page is the design's norm; where the code differs from it is in [Limitations](/en/explanation/limitations/).

This document is the normative specification of the formats that
[lifecycle-events.md section 5.5](/en/explanation/lifecycle-events/#55-boot-record-mcu-identity-and-compiled-topology-requirements-and-the-executable-model)
leaves to an executable model: the boot record, the lockup fuse, the MCU
identity, the compiled topology's node layout, and the keyed records of the
state log with the link diagnostics records (section 8). Every
shape here is the one `crates/dagon-retained-model` proves; a change to a
shape is a change to the model first, and the model's tests decide.

The model is host-only and not released. Each format's codec is a trait in
the model (`RecordCodec`, `FuseCodec`, `BodyCodec`), proven on the host. By design,
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
- **Reset** keeps retained bytes. **Power loss** does what the definition's
  `[boot.record] survives` declares: `reset-random` (bytes random, SRAM),
  `reset-cleared` (bytes zero, registers) or `power` (bytes kept,
  battery-backed).
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

Two guarantees are probabilistic. Random bytes are accepted as a valid
entry or record only by coincidence of an 8-bit commit value and a CRC-32
(at most 2^-40 per slot). A new boot-record sequence repeats an earlier one's numbers and
tag only with probability about 2^-k for k bits of min-entropy in the
entropy word (2.3), and this matters only against the identities a
consumer saw in its previous replay. Every other guarantee below is
structural: it holds for every interruption point, not with a
probability.

## 2. Boot record (`boot-record` 1)

Byte ranges in this document are inclusive: "bytes 1 to 23" are the 23
bytes at offsets 1, 2, …, 23.

### 2.1 Placement

The MCU definition's `[boot.record]` declares the region (at most
`RECORD_MAX` = 4096 bytes), one ring per writer as a byte range, in boot
order, and the entropy word (2.3). Ring 0 belongs to the first DAGON
stage (stage0, the small program that runs first after a reset, where the MCU has one), ring 1 to the next stage, and so
on, at most 16 writers. A ring holds `bytes / 28` entry slots, from 2 to
256. Rings do not overlap and lie inside the region; reserved addresses
of a register file lie outside every ring. The same definition reaches
every image's `memory.x`.

### 2.2 Entry

28 bytes, little-endian:

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | commit: `0xB1` committed; any other value: not an entry of layout 1 |
| 1 | 1 | flags: bit 0 `start` (first entry of the writer's run), bit 1 `epoch` (first entry of a new sequence), bits 2-3 zero, bits 4-7 `opener` |
| 2 | 1 | the writer's `event` format number |
| 3 | 1 | event kind |
| 4 | 4 | `eseq`: the writer's sequence number |
| 8 | 4 | `boot`: the opener's `eseq` of its first entry in this boot |
| 12 | 4 | `boot tag`: the opener's sequence tag |
| 16 | 4 | `tag`: this writer's sequence tag |
| 20 | 3 | payload (the event's compact form) |
| 23 | 1 | zero |
| 24 | 4 | CRC-32 (ISO-HDLC) over `[1, writer index]` followed by bytes 1 to 23 |

An entry is valid when the commit byte is `0xB1`, the reserved bits and
byte are zero and the CRC matches. The CRC binds the layout number and the
writer index, so an entry of another layout or in another writer's ring is
never valid.

**Append**, in this order: store byte 0 = `0x00`; store bytes 1 to 27;
store byte 0 = `0xB1`. Only the commit byte is stored twice, and only by
its own append: the first store makes the slot invalid before any other
byte changes, the last one makes it valid after every byte is in place. A
write interrupted anywhere loses at most that entry, also when it
overwrites a valid one, and nothing a reader accepts is made of two
writes. No other field is ever rewritten.

### 2.3 Writer

A writer's numbers form **sequences**: a sequence starts on an empty ring
and carries one tag in every entry. A stage opens its writer once per run
and reads **only its own ring**:

1. Decode every slot once (one CRC per slot). The newest entry is the
   valid one with the most valid entries at most 2^24 sequence numbers
   behind it (ties: the higher slot); in a ring the writer filled this is
   the largest `eseq`, across wrap.
2. **Intact ring.** The ring is intact when it shows exactly what the
   writer's own appends leave: every slot valid with numbers falling slot
   by slot backwards from the newest; or, before the first wrap, slots 0
   to *p* (the newest) valid and falling back to an `epoch` entry at slot
   0, and every later slot all zeros.
3. **Continue.** With a newest entry, the next append goes to the
   following slot (mod `slots`) with the newest entry's tag and `eseq` =
   newest + 1 when the ring is intact, newest + `slots` when it is not
   (bytes the writer did not write destroyed entries, which may have been
   newer than the newest it holds; the jump passes them and the reader
   states it as a gap).
4. **New sequence.** Without a valid entry, the writer stores zero into
   every byte of its ring, then appends at slot 0 with `epoch`, `eseq` =
   FNV-1a-32 of the whole region's raw bytes from basis `0x811C9DC5` XOR
   the entropy word, and tag = FNV-1a-32 of the same bytes from basis
   `0x050C5D1F` XOR the entropy word rotated left by 16.
5. **Boot.** The hand-off from the previous stage (2.4) when it is valid
   for this layout and names an earlier writer; otherwise this stage opens
   the boot itself (`opener` = its own index, `boot` = the `eseq` of its
   first entry, `boot tag` = its tag). A stage that opens a boot appends
   its first entry (the `entry` event) before it hands off.
6. The run's first entry carries `start`.

Write positions come only from definition constants and from entries the
writer validated in its own ring, so foreign bytes cannot direct a store
outside the ring. The start values are not a CRC on purpose: CRC-32 over
a region of CRC-32-protected entries barely depends on their contents.

**Entropy word.** The MCU definition declares where the entropy word
comes from (`[boot.record] entropy`). The region alone can return to an
earlier state (all zeros after power-on where `survives = reset-cleared`,
or after another layout's writer of the same stage cleared the ring), so
the word must carry at least 16 bits of min-entropy per new sequence.
Any source that meets this is valid: a hardware random generator, a ring
oscillator's random bit sampled repeatedly, low-speed against high-speed
oscillator period captures, or the low bits of repeated ADC conversions
of an internal sensor. An MCU without a source has no boot record.

**Stage0 budget.** The writer runs in every stage that records, stage0
included, so its time counts in that stage's time limit
(`stage0.max_ms` for stage0): one CRC per slot at open, a clear of the
ring on a new sequence, one append per event. Stage0 records only a few
events per boot; a stage0 ring of 2 to 4 slots is enough.

### 2.4 Hand-off

A stage passes the boot identity to the next stage in four 32-bit
registers at the jump, never through retained memory (a hung earlier boot
cannot leave a stale value):

| Word | Value |
|---|---|
| 0 | `0x52424744` (`DGBR`) |
| 1 | `boot` |
| 2 | `boot tag` |
| 3 | bits 0-7 `opener`, bits 8-15 record layout, bits 16-31 the low 16 bits of CRC-32 over words 0 to 2 and bits 0-15 of word 3 (little-endian) |

The receiving stage uses it only when the magic, the check and the layout
match and `opener` is below its own index. A stage that receives a
well-formed hand-off of another layout passes it on unchanged (so one
stage of another layout in the middle does not cut the chain); otherwise
it passes its own boot. A stage that predates the record leaves the next
stage to open the boot itself.

### 2.5 Reader

The agent reads the region once at start with one layout (it never
writes it). Per writer: the valid entries at most 2^24 behind the newest,
oldest first; a run starts at each `start` entry or change of boot. Per
writer it states a **window** (`from` = the oldest held `eseq`, and
whether that entry carries `epoch`: nothing earlier of this sequence
existed), and a **gap** for each missing range of sequence numbers
(numbers inside a gap may never have been used, after a jump). A writer
with no valid entry is stated **empty**.

Boots are ordered by every writer's run order, merged, and boots of one
opener by its numbers:

- a boot of the opener's current sequence (the tag of its newest held
  entry) is newer than a boot of any other sequence;
- within the current sequence, by distance back from the newest held
  entry (a boot up to 2^24 ahead of it is newer: its entries were
  destroyed);
- within another sequence, by serial order when less than 2^30 apart
  (only pairs no writer's run order relates are ordered by numbers);
- boots of two other sequences, or 2^30 or more apart in another
  sequence, are not ordered by numbers.

The reader never reports a newer boot before an older one; pairs it
cannot order are marked uncertain (`uncertain` on both boots). Such pairs
are boots of different openers that no writer's order relates (2.7), or
boots of one opener from sequences that are no longer current.

Replay: boots oldest first; within a boot, writer order, then sequence.
Each event is replayed as its catalog event with `replayed: true` and the
identity `(writer, tag, eseq)`; its boot is `(opener, boot, boot tag)`.
The loss statements are replayed with them. The record is not read by
`plan`, `commit`, `self_check` or `confirm`, and `app.trial_started` and
`app.rolled_back` come from the state log.

### 2.6 Consumers

Replay is at least once: every held entry is replayed at every start. An
identity absent from one replay is never replayed again, so a consumer
de-duplicates `(layout, writer, tag, eseq)` against the previous replay it
read and needs to keep nothing older. A writer never appends an identity
it appended before (2.3 step 3 jumps past destroyed entries; a new
sequence has a new tag), so a new event is never taken for one already
seen, within the limits of 2.7. A window whose `from` follows the last
identity the consumer saw from that writer, a gap, or an `epoch` start
is a loss of the entries in between.

### 2.7 Limits of the claims

- **2^32 wrap.** An identity repeats only after 2^32 appends of its
  writer in one sequence; a consumer that reads two replays more than
  2^32 − `slots` appends of one writer apart cannot tell a new entry from
  an old one with the same number. A later stage's link to a boot more
  than 2^32 − `slots` of the opener's appends old may likewise be placed
  as a recent boot, and two boots of a sequence the opener no longer holds
  that are about 3·2^30 or more of its appends apart may be misordered
  when no writer's run order relates them (serial order across 2^32 makes
  such a pair look less than 2^30 apart the other way, 2.5).
- **Repeated destruction.** When bytes the writer did not write destroy
  its newest entries again before it has appended `slots` entries since
  the previous time, the jump of 2.3 may fall short and the writer may
  reuse numbers of the destroyed entries (and their loss is not stated).
  A destruction that leaves a slot all zeros during the first lap is not
  detected.
- **Opener changes.** When one writer stops recording in the reader's
  layout and starts again (an old or other-layout stage0 in between),
  boots it opened and boots the next stage opened itself are not ordered
  against each other; the reader marks them uncertain. Ordering them would
  need a counter both stages write, which writer independence excludes.
- **Split boots.** A stage that predates the record in the middle of the
  chain, or stages of two record layouts in one boot, may make a later
  stage open the boot again: that boot is replayed as two.

### 2.8 VERSION

0x11 (boot record summary): `layout u8, boots u8, flags u8` (bit 0: some
boot is uncertain; bit 1: a loss is stated, i.e. some writer has a gap or
a window whose oldest entry is not an `epoch` start), then for the newest
boot `opener u8, boot u32 LE, boot tag u32 LE, count u8` and per writer
of that boot its last entry as `writer u8, format u8, kind u8,
payload[3]`. 0x12 serves the region raw, `offset u16` plus at most
`RECORD_PAGE(link)` bytes; the host joins the pages and decodes them with
this reader.

### 2.9 Versioning

Layout *n* (1 to 15) commits with `0xB0 | n`, and its CRC is seeded with
*n*. A layout beyond 15 takes a commit value that is neither `0x00` nor
the value of any earlier layout, and is listed here. A reader counts the
slots whose first byte is another layout's commit (where strides differ
the count is approximate) so it can report a record of another layout,
and never decodes them. A writer's event encoding moves with that
writer's `event` number, carried in each entry.

## 3. Lockup fuse (`boot-record` 1)

### 3.1 Cells

One cell per image kind that runs sandboxed calls: the boot loader's cell
and the app's cell (the app and relay layers share it; at most one
sandboxed call is open per core). A cell is an arm slot followed by
record slots (`[boot.record] fuse` declares both cells and their slot
counts next to the record). Only the cell's own layer stores into it;
every layer's start reads every cell.

### 3.2 Fuse id

`SHA-256("dagon-fuse/1" ‖ layer u8 ‖ slot u32 LE ‖ image header digest
[32] ‖ chain index u32 LE ‖ artifact SHA-256 [32])`. The fields have fixed
widths, so two different call sites never give the same input; no count
of slots or extensions is capped. A new build of an extension or a new
image is a new id.

### 3.3 Record

40 bytes:

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | mark: `0xA1` armed, `0xA2` tripped, `0xA3` clear request, `0xA4` lockup verdict, `0xA5` clean verdict, `0xA6` clear done; any other value: free |
| 1 | 1 | layer |
| 2 | 2 | zero |
| 4 | 32 | fuse id |
| 36 | 4 | CRC-32 over `[1]` followed by bytes 1 to 35 |

A record is written like a record entry (mark `0x00`, bytes 1 to 39,
mark last) into a free slot; freeing a slot is one store of `0x00` to its mark.
The mark is outside the CRC, so turning a trip into a clear-done record is
one byte.

### 3.4 Calls

Before a sandboxed call the layer feeds the watchdog and writes an armed
record into its arm slot; the first store of the return path, also after
an overrun or a fault, frees the arm slot (disarm). A call has returned
exactly when its disarm is stored. The budget of every call is far below
the watchdog period (lifecycle-events.md section 10), so a watchdog reset with a call armed means that
call locked up. A layer whose start reports `can_arm = false` arms
nothing in that run.

The fuse needs a **watchdog flag**: set by a watchdog reset, kept
across any later reset other than power-on, cleared only when a start
acknowledges it. A start acknowledges it when it stored everything it
had to (`complete`). The MCU definition declares whether the MCU has
such a flag and its source (`[watchdog] flag`: a hardware register whose
reset flags persist until cleared, or a flag the MCU definition provides,
e.g. in a retained register it sets on a watchdog reset). An MCU without
one has no fuse: the definition loader reports the fuse as unavailable,
and its layers run sandboxed calls without a lockup fuse.

### 3.5 Start

Each layer's start, before it runs any extension, frees what is done
(step 3) first, so that a verdict or trip finds room, then:

1. **Classify.** For every other cell whose arm slot is armed with no
   verdict in any cell, store a verdict in the own cell: *lockup* if the
   watchdog flag is set, else *clean*. No room: `complete` and `can_arm`
   are false.
2. **Own call.** If the own arm slot is armed: it locked up when a verdict
   says so, or, without a verdict, when the watchdog flag is set. A locked
   call whose id is installed gets a tripped record (no duplicate), then
   the arm slot is freed; no room: the arm slot stays armed and `can_arm`
   and `complete` are false. A locked call whose id is not installed is
   recorded as stale and freed. A clean call is just freed.
3. **Own records** (done first, see above). A trip whose id is not
   installed is stale: freed by this layer. A trip with a clear request
   in another cell and no
   clear-done of its own becomes clear-done (one byte). A clear request
   is freed when another cell holds its clear-done, or no other cell
   holds its trip. A clear-done is freed when no other cell holds its
   request. A verdict is freed when no other cell's arm slot holds its id.
4. **Result.** The installed trips are disabled for the run; ids with a
   verdict in another cell are held (not armed this run); other cells'
   trips are reported, never acted on.

So an app or relay lockup survives the boot loader's start unchanged (the
boot loader only stores its verdict in its own cell) and is tripped by the
app's start; a boot-loader lockup is tripped by the boot loader's next start.
A reset at any byte of arming, of the call, of the return or of the
disarm never trips a call that returned; a reset during the start keeps
the flag and the armed record, so the next start repeats the decision.

### 3.6 Clearing

DAGON has no request that clears a trip: neither the protocol nor the CLI
has one. This is what the format's model holds for one: the agent frees an app
trip in its own cell, or stores a clear request for a boot-loader trip; the boot
loader's next start turns the trip into clear-done, and each side frees its
record when it sees the other's. One request clears one trip. Nothing else
clears a trip whose id is installed.

### 3.7 Limits of the claims

Power-on clears the watchdog flag: an unclassified lockup is lost,
never forged. Foreign bytes may destroy records the same way; a destroyed
clean verdict lets a later watchdog reset classify that interrupted call
(one that never returned) as a lockup, and a destroyed clear-done record
lets a clear request still pending clear the same extension's next trip
once more. A full cell makes its layer arm nothing
until one of its records is freed; a trip whose id is installed stays, since no
clear request exists (3.6).

### 3.8 Versioning

Each fuse layout has its own six mark values, distinct from every other
layout's and from `0x00`; layout 1 uses `0xA1` to `0xA6`, and a later
layout lists its values here. The CRC is seeded with the layout number.
Records of another layout are free slots to this reader and are never
acted on.

## 4. MCU identity

`identity(id)` = the first 8 bytes of `SHA-256("dagon-mcu/" ‖ id)` as a
little-endian u64, where `id` is the MCU definition's `id`. Every carrier
holds all 64 bits. Carriers: `image-header`, `identity-reply`,
`version-0x06`, `topology-node` (the model's `identity::CARRIERS`; a test
checks this line against it). The definition loader refuses two
definitions whose identities agree in the width of any carrier, naming
both ids, and refuses an identity that is all zeros or all ones in a
carrier's width. A device whose identity matches no loaded definition
exactly is shown as unknown. No numeric MCU table exists.

## 5. Compiled topology

### 5.1 Body (`topology-bin` 2)

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

## 6. Properties

The model checks these after every operation of every scenario; the
fixed cases in `tests/fixed.rs` name the property they guard.

| Id | Property |
|---|---|
| P1 | Every held entry is replayed by every app of its layout. |
| P2 | Every replayed event is a held append of the reader's layout; no torn write is accepted. |
| P3 | Every store of a writer lies in its own ring, whatever bytes the region holds. |
| P4 | A writer never appends an identity it appended before in its sequence, and continues ahead of every number it used (outside 2.7); a boot number is held for one boot. |
| P5 | The reader never reports a newer boot before an older one; pairs it cannot order are marked uncertain; an unordered pair has different openers, or one opener in two sequences, or one opener's numbers 2^30 or more apart. |
| P6 | Every replayed boot holds the events of one boot, and one boot is one replayed boot (outside the split boots of 2.7). |
| P7 | An append lost before any replay is covered by a window or gap: at once when its own writer overwrote it, when other bytes destroyed it, once an entry the writer appended later is held. |
| P8 | `plan`, `commit` and `confirm` take only the state log (compile-time). |
| P9 | A reader never accepts entries of another layout (in P2's lookup). |
| P10 | Region ≤ `RECORD_MAX`; paged reads at every link size reassemble the region; the 0x11 summary fits one page. |
| P11 | A consumer de-duplicating `(layout, writer, tag, eseq)` against its previous replay never meets one identity with two contents (outside 2.7). |
| P12 | Identity is a pure function of the id; collisions refused at every carrier width; unknown never matched. |
| P14 | A call past its budget is abandoned; the boot reaches the decision the state log gives. |
| P15 | An access to a denied region faults without effect. |
| P16 | A capability misuse (undeclared window or bus target) faults or is refused. |
| P17 | A handler is not called after `EXT_FAULT_LIMIT` faults or overruns in a run. |
| P18 | Boot time with the service window stays within half the trial period. |
| P19 | A verifier replacement decides only when listed, hash-matched and in a signed image. |
| P20 | A layer stores only into its own fuse cell. |
| P21 | Distinct call sites have distinct fuse ids. |
| P22 | A trip names only a call that locked up; a lockup is tripped by its layer's next completed start. |
| P23 | The boot loader's run leaves the app's cell unchanged; the app's start acts on it. |
| P24 | A trip whose id is no longer installed is freed by its layer's start; a new build runs again. |
| P25 | Records of another fuse layout are never acted on. |
| P26 | A trip of an installed id disappears only by a clear request, which exists only in the model (3.6). |

## 7. Boot loader trial cell

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

A cell that does not hold what is
written to it at all is found by `bootcore::cell_holds`, run by the
started boot loader and by the app: such a node takes no boot loader, and a
pending core that finds it so rolls itself back.

A cell with a word that
holds a count was written by stage0, so it holds, and the check writes
nothing over it; only a cell counting nothing gets a probe (a value no
cell word takes) and its old value back, word by word, each read back, so
a reset inside the check never leaves the cell counting fewer starts.

Limit of the claims: that each place keeps its value across the resets
named is the vendor's documented behaviour.

## 8. Keyed records of the state log and link diagnostics (`state-log` 1)

Beside the records of the image slots, the state log holds, as keyed records, the link diagnostics a node keeps about its peers. This section is the norm of the keyed record, of the five link diagnostics records and of the rules that append them. The model is `journal`, `diag` and `world_diag` of `dagon-retained-model`. The faces, the face header and the fold of the slot records are those of `dagon_format::statelog` and `dagon_core::log`, and the model compares every operation on the slot records with `dagon_core::log::Log` started from the same bytes.

Each rule names its class: [ISO] follows ISO 14229-1, [AUTOSAR] follows AUTOSAR Dem, [DAGON] is a rule of DAGON.

### 8.1 Faces and records

[DAGON] The log is two faces used in turn. A face is a 32-byte face header followed by 32-byte records; erased bytes are `0xFF`. The valid face with the larger generation is the active one. Its records are folded in order up to the first erased record or the first record that fails its CRC.

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | Kind: `0x01` to `0x7F` a slot record, `0x80` to `0xFE` a keyed record, `0xFF` erased |
| 1 | 3 | Fields of the kind |
| 4 | 4 | `seq`: sequence number, little-endian. No fold uses it |
| 8 | 20 | Fields of the kind |
| 28 | 4 | CRC-32 (ISO-HDLC) of bytes 0 to 27 |

[DAGON] A record is appended into the first erased record of the active face, after a check that all its 32 bytes are erased. When the face is full, ends in a record that fails its CRC, or fails that check, the append compacts first: it erases the other face, writes the fold there and writes that face's header with the generation plus 1 last. Until that header is whole the old face stays the valid one. A reader skips a record whose kind it does not know.

### 8.2 Keyed record

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | Kind: bit 7 is 1; bit 6 is 0 for a put and 1 for an erase; bits 0 to 5 are the class (0 to 62) |
| 1 | 1 | `n`: for a put the length of its key, for an erase the length of the key prefix it names (0 to 20) |
| 2 | 1 | For an erase the last class it covers (its own class to 62); for a put zero |
| 3 | 1 | Zero |
| 4 | 4 | `seq` |
| 8 | 20 | For a put the key (`n` bytes), then the value; for an erase the key prefix (`n` bytes), then zero |
| 28 | 4 | CRC-32 |

[DAGON] The fold needs no knowledge of what a class means:

- The key of a put is its class and its `n` key bytes. The latest put of a key is the value of that key; it removes the earlier put of the same key.
- An erase removes every earlier put whose class lies between the class of the erase and its last class and whose key starts with its key prefix. With `n` 0 it removes every key of those classes. The erase itself leaves nothing in the fold.
- A record of kind `0x80` to `0xFE` that breaks the table (`n` above 20, a last class below its class or above 62, a byte that must be zero) is no keyed record: the fold skips it and compaction does not copy it.

[DAGON] Compaction writes the slot records as before, then copies, in the order of the log and byte for byte (sequence number and CRC as they are), every put that no later record of the face removes. It does not copy erases or removed puts. It reads the face record by record and holds no table. Code that knows no class therefore carries every class: a boot loader carries the records of an app newer than itself. When the other face cannot take all the records, the compaction fails and the old face stays the valid one.

| Class | Record |
|---|---|
| 0 | Ever-communicated |
| 1 | DTC state |
| 2 | Seed counter |

### 8.3 Link diagnostics records

[DAGON] A peer is `link` and `addr` of the link the node reaches it on. The five records:

| Record | Kind | `n` | Bytes 8 to 27 |
|---|---|---|---|
| Ever-communicated | `0x80` | 2 | `link`, `addr`, instance identifier (16), zero (2) |
| DTC state | `0x81` | 5 | `link`, `addr`, DTC number (3, high byte first), status, age, failed-cycle count, zero (12) |
| Seed counter | `0x82` | 0 | Counter (u32, little-endian), zero (16) |
| DTC clear | `0xC1` | 5, 2 or 0 | `link`, `addr`, DTC number (3); or `link`, `addr`; or nothing. The rest zero. Byte 2 is 1 |
| Ever-communicated clear | `0xC0` | 2 or 0 | `link`, `addr`; or nothing. The rest zero. Byte 2 is 1 |

- [DAGON] The instance identifier is 16 bytes in every record. 16 zero bytes mean that there is none. How an identifier is supplied is outside this format; a writer with no supply writes 16 zero bytes, and no supply gives 16 zero bytes as a value.
- [ISO] The status is the statusOfDTC byte. The record holds bits 1 to 6 only; bits 0 and 7 are zero in it. Bit 0 lives in RAM and bit 7 is always 0.
- [DAGON] A DTC number from `0x000001` to `0xFFFFFE` names a fault. `0x000000` and `0xFFFFFF` are never recorded.
- [DAGON] The age counts the passed cycles since the DTC was confirmed; the failed-cycle count counts the failed cycles in a row. Each is one byte and stops at 255.
- [DAGON] A DTC clear with `n` 5 clears one DTC of one peer, with `n` 2 all DTCs of one peer, with `n` 0 all DTCs. It covers class 1 only, so it never removes an ever-communicated record.
- [DAGON] An ever-communicated clear with `n` 2 clears one peer, with `n` 0 all peers. It covers classes 0 to 1, so it removes the DTC states of those peers with it.
- [DAGON] No clear covers class 2: the seed counter outlives every clear.
- [DAGON] A record of class 0, 1 or 2 in any other form (another `n`, a DTC number that names no fault, a status bit outside 1 to 6, a byte that must be zero) is a record of an unknown format: the store does not use it, and compaction carries it.

The bytes of each record with sequence number 1, for the peer with `link` 255 and `addr` 255 (DTC number `0xFFFFFE`, status `0x7E`, age 255, failed-cycle count 255, counter 4294967295):

```
ever-communicated, no identifier         80 02 00 00 01 00 00 00 FF FF 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 51 2E 5F 7F
ever-communicated, identifier 01..10     80 02 00 00 01 00 00 00 FF FF 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F 10 00 00 73 A9 82 B9
DTC state                                81 05 00 00 01 00 00 00 FF FF FF FF FE 7E FF FF 00 00 00 00 00 00 00 00 00 00 00 00 4D 7B A9 6F
DTC clear, one DTC of one peer           C1 05 01 00 01 00 00 00 FF FF FF FF FE 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 B4 3C 82 0C
DTC clear, all DTCs of one peer          C1 02 01 00 01 00 00 00 FF FF 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 9B 32 88 F4
DTC clear, all DTCs                      C1 00 01 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 84 80 7C 50
ever-communicated clear, one peer        C0 02 01 00 01 00 00 00 FF FF 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 86 CF 3D F5
ever-communicated clear, all peers       C0 00 01 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 99 7D C9 51
seed counter                             82 00 00 00 01 00 00 00 FF FF FF FF 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 06 02 EA 28
```

### 8.4 DTC state transitions

[ISO] A DTC with no record has the status `0x50`, age 0 and failed-cycle count 0: the state after a clear.

[AUTOSAR] The fault detection counter (FDC) of a DTC lives in RAM and starts each operation cycle at 0. A fail sample adds the FDC step, up to +127; an FDC of +127 is a failed test. [DAGON] A pass sample sets the FDC to -128 at once.

| Event | Change of the record | Class |
|---|---|---|
| Pass sample | Bit 4 becomes 0. Bit 6 becomes 0 when bit 2 or bit 3 is 1 | [ISO] bit 4, [DAGON] bit 6 |
| Failed test, bit 1 is 0 | Bits 1, 2 and 5 become 1; bits 4 and 6 become 0; the failed-cycle count rises by 1; bit 3 becomes 1 when the count reaches the confirmation threshold; the age becomes 0 | [AUTOSAR] |
| Failed test, bit 1 is 1 | None | [AUTOSAR] |
| Cycle-end evaluation, bit 6 is 1 | None | [DAGON] |
| Cycle-end evaluation, bit 6 is 0 and bit 1 is 1 | Bit 1 becomes 0 and bit 6 becomes 1 | [AUTOSAR] |
| Cycle-end evaluation, bit 6 is 0 and bit 1 is 0 | Bit 2 becomes 0; the failed-cycle count becomes 0; when bit 3 is 1 the age rises by 1, and an age that reaches the aging threshold makes bit 3 and the age 0; bit 6 becomes 1 | [AUTOSAR] |

- [DAGON] An operation cycle is one run of the node's app, from reset to reset. A power cut ends a cycle with no chance to write, so the cycle that ended is evaluated at the next start of the app, before the first sample, from bits 1 and 6 of the record.
- [DAGON] The result of the evaluation has bit 6 at 1, and the evaluation changes nothing in a record whose bit 6 is 1. An evaluation cut at any point and run again therefore ends where one run ends.
- [DAGON] A pass sample of a DTC that is neither pending nor confirmed leaves bit 6 of the record at 1, because the evaluation has nothing to decide for it. A peer with no fault appends no record in a cycle. Bit 6 of the running cycle, like bit 0, is read from RAM.
- [AUTOSAR] A failed test after a pass sample in the same cycle makes the age 0: the failure cancels the aging of that cycle.

### 8.5 Appends

[DAGON] Every change is one appended record. No field of a record is written twice, and a record counts from the moment all its 32 bytes with the CRC are there.

- A pass sample of a peer whose ever-communicated record the fold does not hold appends that record first, not only the first time: a record cut by a power loss is appended again by the next pass sample. When the fold holds `diag_peers` peers, the new peer is not recorded and the store reports that it is full.
- A pass sample or a failed test then appends the DTC state when the table of 8.4 changes it.
- A timeout of a peer with no ever-communicated record is no sample.
- A clear appends one erase. A clear that names nothing the fold holds appends nothing.
- The seed counter starts at 0 with no record. A request appends the counter plus 1 and returns that value only after the append is complete. At 4294967295 it returns no value; the counter never wraps.
- The cycle-end evaluation appends one DTC state for each DTC it changes.

### 8.6 Properties

The model injects a power cut after every byte programmed or erased in every operation (the appends of a sample, of a clear and of a seed counter, the cycle-end evaluation, compaction by the app and by the boot loader), foreign bytes anywhere in the flash, and keyed records of a class that nothing decodes. After every operation it checks that the fold is the expected one, and after a power cut that the fold is the one before the record being appended or the one after it, and these properties, which are claims under power cuts (8.7 has what holds under foreign bytes):

| Id | Property |
|---|---|
| L1 | A peer read as ever communicated had a pass sample since the last ever-communicated clear, and the identifier read is that sample's. |
| L2 | An ever-communicated record whose append is complete is read after any later power cut, compaction by the app or by the boot loader, and DTC clear. |
| L3 | What a completed clear removed is not read again before a new sample of that peer. |
| L4 | The link diagnostics records appended in one cycle are at most 3 × `diag_peers` + the clears + the seed requests + 3 × the peers those clears cover. A peer takes at most three in a cycle: the evaluation or the ever-communicated record, the first pass and the first failure. A clear lets each peer it covers take three more. |
| L5 | Every write is inside the two faces. |
| L6 | The slot records fold to what `dagon_core::log::Log` folds from the same bytes. An operation on them leaves the records that `Log` leaves, keyed records and sequence numbers aside. An append of a keyed record changes that fold only through compaction. A compacted face holds every record of the fold, or the old face stays the valid one. |
| L7 | The cycle-end evaluation, cut at any point and run again, ends where one run ends. |
| L8 | pendingDTC, confirmedDTC, the age and the failed-cycle count follow the outcomes of the cycles as 8.4 states, a failure after a pass in the same cycle included. |
| L9 | A seed counter value that was returned is never returned again, across any power cut. |

### 8.7 Limits of the claims

- **Foreign bytes.** L1 to L9 are claims under power cuts. Foreign bytes over a record end the fold at that record, and foreign bytes over the header of the active face make the older face the valid one. The log then reads as it did at an earlier time, or as a part of it: a put that an erase had removed is read again, and the seed counter reads an earlier value. Under foreign bytes only these weaker claims hold: no peer is read as ever communicated that had no pass sample at any time (the coincidence of a CRC-32 aside), L5 holds, and L1 to L9 hold again from the next completed record on, counted from the fold that is read.
- **A face full of the fold.** When the records of the fold fill a face, compaction frees no room: every append fails, and each one erases a face first. The MCU definition's `diag_peers` keeps the records of the fold below one face.
- **A cut record and the boot path.** A record cut by a power loss ends its face until the next compaction. The boot path never erases, so it appends nothing to such a face, and no trial starts until an append of the running app has compacted.
- **Erasing both faces.** A log whose two faces are erased holds no record: every peer reads as never communicated with, and the seed counter starts at 0 again.

### 8.8 Versioning

The keyed record belongs to `state-log` 1: every reader of format 1 folds the kinds `0x80` to `0xFE` by 8.2 and its compaction carries them. A new record takes a class number that no record has; readers need no change for it. A record whose layout changes takes a new class. A change of the tables of 8.1 or 8.2 takes a new `state-log` number.
