---
title: "Retained and persistent formats"
description: "The boot record, the lockup fuse, the MCU identity and the compiled topology's layout."
---

This document is the normative specification of the formats that
[lifecycle-events.md section 5.5](/en/explanation/lifecycle-events/#55-boot-record-mcu-identity-and-compiled-topology-requirements-and-the-executable-model)
leaves to an executable model: the boot record, the lockup fuse, the MCU
identity and the compiled topology's node layout and replacement. Every
shape here is the one `crates/dagon-retained-model` proves; a change to a
shape is a change to the model first, and the model's tests decide.

The model is host-only and not released. Each format's codec is a trait in
the model (`RecordCodec`, `FuseCodec`, `BodyCodec`); a production codec
implements the same trait and runs under the same harness, so the shipped
codec and the proof cannot drift apart.

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
(at most 2^-40 per slot); the topology store also requires a SHA-256
match. A new boot-record sequence repeats an earlier one's numbers and
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

0x20 (boot record summary): `layout u8, boots u8, flags u8` (bit 0: some
boot is uncertain; bit 1: a loss is stated, i.e. some writer has a gap or
a window whose oldest entry is not an `epoch` start), then for the newest
boot `opener u8, boot u32 LE, boot tag u32 LE, count u8` and per writer
of that boot its last entry as `writer u8, format u8, kind u8,
payload[3]`. 0x21 serves the region raw, `offset u16` plus at most
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

### 5.1 Body (`topology-bin` 1)

Head, 16 bytes: magic `DGTP`, layout u16 LE (1), node count (1 to `MAX_NODES`), link
count (≤ `MAX_LINKS`), constraint count (≤ `MAX_CONSTRAINTS`), 7 zero bytes. Then:

- nodes, 40 bytes each: index, role, port kind, max attempts, listen
  window ms u16 LE, map id, order rank, recovery tiers [3], 5 zero bytes,
  MCU identity u64 LE at offset 16, target tag [16] at offset 24;
- links, 16 bytes each, in the image tools' link layout;
- constraints, 16 bytes each, in the manifest's constraint layout.

Canonical: node `index` equals its position, links sorted by `to`,
reserved bytes zero; a decoder re-encodes and compares. The host's
`host.topology.pushed` and the hub's `relay.children_set` carry the body's
SHA-256.

### 5.2 Store on the hub

The TOPOLOGY region is two banks of equal size, each erasable on its own.
Each bank starts with a 64-byte header:

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | commit: `0xC1` (store layout 1) |
| 1 | 3 | zero |
| 4 | 4 | sequence u32 LE |
| 8 | 4 | body length u32 LE |
| 12 | 32 | SHA-256 of the body |
| 44 | 4 | CRC-32 over `[1]` followed by bytes 1 to 43 |
| 48 | 16 | zero |

Bytes 1 to 3 and 48 to 63 are programmed as zero and not checked by a
reader (bytes 1 to 3 are under the CRC).

The body follows the header. A bank counts when its commit byte is `0xC1`, its
length fits, its CRC matches and its body's SHA-256 equals the header's.
The copy is the counted bank with the newer sequence (serial order);
there is none when no bank counts.

**Write** (image lifecycle `written`, target TOPOLOGY): the host refuses a
body larger than a bank's capacity before sending any byte, and the hub
refuses it again before erasing. Otherwise the hub writes the bank that is
not the copy (bank 0 when there is none) with sequence = copy's + 1 (1
when none): erase the bank, program the body, program header bytes 1 to
63, program the commit byte. Number the steps of a write from 0, one per
erased or programmed byte; the commit byte is step `bank size + body
length + 63`. A write that stops before that step has completed leaves
the previous copy, byte for byte; once it has completed, the new one.
Every later read gives the same answer until the next write (1). A completed write makes
the previous copy unreachable. The transition is `Absent → Written` when
no copy existed, `Written → Written` otherwise.

**Read** at the relay's start: a copy whose body does not decode (a layout
number the hub does not read, or malformed) reads as absent and the
project's fallback list is used (`relay.children_set { source: Fallback }`);
a body is applied whole or not at all.

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
| P10 | Region ≤ `RECORD_MAX`; paged reads at every link size reassemble the region; the 0x20 summary fits one page. |
| P11 | A consumer de-duplicating `(layout, writer, tag, eseq)` against its previous replay never meets one identity with two contents (outside 2.7). |
| P12 | Identity is a pure function of the id; collisions refused at every carrier width; unknown never matched. |
| P13 | Topology: stated outcome at every interruption point, previous copy unchanged, round trip with identity, bounds, unsupported or malformed reads as absent, stable reads. |
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
new start, never fewer (the model found that the previous shape, a word
and its complement, read such a pair as 0 and lost the start). A random
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
a reset inside the check never leaves the cell counting fewer starts (the
model found the earlier check, which probed over a running count, letting
a failing core start without end).

Limit of the claims: that each place keeps its value across the resets
named is the vendor's documented behaviour.

