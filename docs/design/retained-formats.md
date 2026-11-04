# Retained and persistent formats

This document is the normative specification of the formats that
[lifecycle-events.md §5.5](lifecycle-events.md#55-boot-record-mcu-identity-and-compiled-topology-requirements-and-the-executable-model)
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
  interrupted program leaves a prefix programmed. A programmed byte reads
  the same on every read.
- **Foreign bytes.** Any byte may also be overwritten by something that is
  not the format's writer: a writer of another layout of the same stage,
  or a fault. The model injects random bytes into every region.

Random bytes are accepted as a valid entry or record only by coincidence
of an 8-bit commit value and a CRC-32 (at most 2^-40 per slot); the
topology store also requires a SHA-256 match. Every other guarantee below
is structural: it holds for every interruption point, not with a
probability.

## 2. Boot record (`boot-record` 1)

### 2.1 Placement

The MCU definition's `[boot.record]` declares the region (at most
`RECORD_MAX` = 4096 bytes) and one ring per writer as a byte range, in
boot order: ring 0 belongs to the first DAGON stage (stage0 where the MCU
has one), ring 1 to the next stage, and so on, at most 16 writers. A ring
holds `bytes / 20` entry slots, at least 2. Rings do not overlap and lie
inside the region; reserved addresses of a register file lie outside
every ring. The same definition reaches every image's `memory.x`.

### 2.2 Entry

20 bytes, little-endian:

| Offset | Size | Field |
|---|---|---|
| 0 | 1 | commit: `0xB1` committed; any other value: not an entry of layout 1 |
| 1 | 1 | flags: bit 0 `start` (first entry of the writer's run), bit 1 `epoch` (first entry after the writer found no valid entry), bits 2-3 zero, bits 4-7 `opener` |
| 2 | 1 | the writer's `event` format number |
| 3 | 1 | event kind |
| 4 | 4 | `eseq`: the writer's sequence number |
| 8 | 4 | `boot`: the opener's `eseq` of its first entry in this boot |
| 12 | 3 | payload (the event's compact form) |
| 15 | 1 | zero |
| 16 | 4 | CRC-32 (ISO-HDLC) over `[1, writer index]` followed by bytes 1..16 |

An entry is valid when the commit byte is `0xB1`, the reserved bits and
byte are zero and the CRC matches. The CRC binds the layout number and the
writer index, so an entry of another layout or in another writer's ring is
never valid.

**Append**, in this order: store byte 0 = `0x00`; store bytes 1..19;
store byte 0 = `0xB1`. Only the commit byte is stored twice, and only by
its own append: the first store makes the slot invalid before any other
byte changes, the last one makes it valid after every byte is in place. A
write interrupted anywhere loses at most that entry, also when it
overwrites a valid one, and nothing a reader accepts is made of two
writes. No other field is ever rewritten.

### 2.3 Writer

A stage opens its writer once per run and reads **only its own ring**:

1. Decode every slot. The newest entry is the valid one with the most
   valid entries less than `slots` sequence numbers behind it (ties: the
   higher slot); in a ring the writer filled this is the largest `eseq`,
   across wrap.
2. With a newest entry, the next append goes to the following slot (mod
   `slots`) with `eseq` = newest + 1 (mod 2^32). Without one, the next
   append goes to slot 0 with `eseq` = FNV-1a-32 of the whole region's raw
   bytes XOR the MCU package's entropy word, and carries `epoch`.
3. The run's boot: the hand-off from the previous stage (2.4) when it is
   valid for this layout and names an earlier writer; otherwise this stage
   opens the boot itself (`opener` = its own index, `boot` = the `eseq` of
   its first entry).
4. The run's first entry carries `start`.

Write positions come only from definition constants and from entries the
writer validated in its own ring, so foreign bytes cannot direct a store
outside the ring. The fresh start is not a CRC on purpose: CRC-32 over a
region of CRC-32-protected entries barely depends on their contents, and
a writer restarting after its entries were overwritten would reuse its old
numbers. Where `survives = reset-cleared` the region is all zeros after
power-on, so the MCU package must supply an entropy word there (a hardware
random source or uninitialised RAM); with `reset-random` and `power` the
word may be 0.

### 2.4 Hand-off

A stage passes the boot identity to the next stage in three 32-bit
registers at the jump, never through retained memory (a hung earlier boot
cannot leave a stale value):

| Word | Value |
|---|---|
| 0 | `0x52424744` (`DGBR`) |
| 1 | `boot` |
| 2 | bits 0-7 `opener`, bits 8-15 record layout, bits 16-31 the low 16 bits of CRC-32 over word 0, word 1 and bits 0-15 of word 2 (little-endian) |

The receiving stage uses it only when the magic, the check and the layout
match and `opener` is below its own index. A stage that predates the
record, or writes another layout, leaves the next stage to open the boot
itself.

### 2.5 Reader

The agent reads the region once at start with one layout (it never
writes it). Per writer: the valid entries less than `slots` behind the
newest, oldest first; a run starts at each `start` entry or change of
boot. Per writer it states a **window** (`from` = the oldest held `eseq`,
and whether that entry carries `epoch`: nothing earlier of this
sequence existed), and a **gap** for each missing range of sequence
numbers. A writer with no valid entry is stated **empty**.

Boots are ordered by every writer's own run order, merged. A boot whose
opener's ring no longer holds it is placed by its number against the
opener's newest held entry: up to `slots` ahead, it is newer than every
boot the opener holds (the opener's newest entries were destroyed by
foreign bytes); within the held range, by number; otherwise older. The
reader never reports a newer boot before an older one; pairs it cannot
order are marked uncertain (`uncertain` on both boots). This happens
only between boots opened by different writers.

Replay: boots oldest first; within a boot, writer order, then sequence.
Each event is replayed as its catalog event with `replayed: true` and the
identity `(writer, eseq)`; its boot is `(opener, boot)`. The loss
statements are replayed with them. The record is not read by `plan`,
`commit`, `self_check` or `confirm`, and `app.trial_started` and
`app.rolled_back` come from the state log.

### 2.6 Consumers

Replay is at least once: every held entry is replayed at every start.
An identity absent from one replay is never replayed again, so a consumer
de-duplicates `(layout, writer, eseq)` against the previous replay it
read and needs to keep nothing older. A window whose `from` follows the
last identity the consumer saw from that writer, or a gap, is a loss of
the entries in between.

### 2.7 Limits of the claims

- An identity repeats only after 2^32 appends of its writer; a consumer
  that reads two replays more than 2^32 − `slots` appends of one writer
  apart cannot tell a new entry from an old one with the same number. A
  later stage's link to a boot more than 2^32 − `slots` of the opener's
  appends old may likewise be placed as a recent boot.
- When one writer stops recording in the reader's layout and starts again
  (an old or other-layout stage0 in between), boots it opened and boots
  the next stage opened itself are not ordered against each other; the
  reader marks them `uncertain`. Ordering them would need a counter both
  stages write, which writer independence excludes.
- When foreign bytes destroy a writer's newest entries, the writer
  continues below them and may reuse a boot number that a later stage's
  held entry still names; event identities stay unique, but boot grouping
  and order are not claimed until those entries have been overwritten.

### 2.8 VERSION

0x20 (boot record summary): `layout u8, boots u8, flags u8` (bit 0 some
boot uncertain, bit 1 a loss stated), then for the newest boot
`opener u8, boot u32 LE, count u8` and per writer of that boot its last
entry as `writer u8, format u8, kind u8, payload[3]`. 0x21 serves the
region raw, `offset u16` plus at most `RECORD_PAGE(link)` bytes; the host
joins the pages and decodes them with this reader.

### 2.9 Versioning

A committed entry's first byte is `0xB0 | layout`; the CRC is seeded with
the layout number. A reader counts the slots whose first byte is
`0xB0 | other` (so it can report a record of another layout) and never
decodes them. A writer's event encoding moves
with that writer's `event` number, carried in each entry.

## 3. Lockup fuse (`boot-record` 1)

### 3.1 Cells

One cell per image kind that runs sandboxed calls: the boot core's cell
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
| 36 | 4 | CRC-32 over `[1]` followed by bytes 1..36 |

A record is written like a record entry (mark `0x00`, bytes 1..39, mark
last) into a free slot; freeing a slot is one store of `0x00` to its mark.
The mark is outside the CRC, so turning a trip into a clear-done record is
one byte.

### 3.4 Calls

Before a sandboxed call the layer feeds the watchdog and writes an armed
record into its arm slot; the first store of the return path, also after
an overrun or a fault, frees the arm slot (disarm). A call has returned
exactly when its disarm is stored. The budget of every call is far below
the watchdog period (lifecycle-events.md §10), so a watchdog reset with a call armed means that
call locked up. A layer whose start reports `can_arm = false` arms
nothing in that run.

The fuse needs a **watchdog flag**: set by a watchdog reset, kept
across any later reset other than power-on, cleared only when a start
acknowledges it. A start acknowledges it when it stored everything it
had to (`complete`). The MCU definition declares whether the MCU has
such a flag and its source (`[watchdog] flag`: a hardware register whose
reset flags persist until cleared, or a flag the MCU package provides,
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

So an app or relay lockup survives the boot core's start unchanged (the
boot core only stores its verdict in its own cell) and is tripped by the
app's start; a boot-core lockup is tripped by the boot core's next start.
A reset at any byte of arming, of the call, of the return or of the
disarm never trips a call that returned; a reset during the start keeps
the flag and the armed record, so the next start repeats the decision.

### 3.6 Clearing

`dagon extension reset` reaches the agent, which frees an app trip in its
own cell, or stores a clear request for a boot-core trip; the boot core's
next start turns the trip into clear-done, and each side frees its record
when it sees the other's. One request clears one trip. Nothing else clears
a trip whose id is installed.

### 3.7 Limits of the claims

Power-on clears the watchdog flag: a lockup not yet classified is lost,
never forged. Foreign bytes may destroy records the same way; a destroyed
clear verdict lets a later watchdog reset classify that interrupted call
(one that never returned) as a lockup, and a destroyed clear-done record
lets a clear request still pending clear the same extension's next trip
once more. A full cell makes its layer arm nothing
until the host clears a trip.

### 3.8 Versioning

Marks of layout *n* are distinct from those of every other layout, and the
CRC is seeded with the layout number; records of another layout are free
slots to this reader and are never acted on.

## 4. MCU identity

`identity(id)` = the first 8 bytes of `SHA-256("dagon-mcu/" ‖ id)` as a
little-endian u64, where `id` is the MCU definition's `id`. Every carrier
holds all 64 bits: the image header, the identity reply, VERSION 0x06 and
the compiled topology's node. The definition loader refuses two
definitions whose identities agree in the width of any carrier, naming
both ids, and refuses an identity that is all zeros or all ones in a
carrier's width. A device whose identity matches no loaded definition
exactly is shown as unknown. No numeric MCU table exists.

## 5. Compiled topology

### 5.1 Body (`topology-bin` 1)

Head, 16 bytes: magic `DGTP`, layout u16 LE (1), node count (1..8), link
count (≤ 7), constraint count (≤ 16), 7 zero bytes. Then:

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
| 44 | 4 | CRC-32 over `[1]` followed by bytes 1..44 |
| 48 | 16 | zero |

followed by the body. A bank counts when its commit byte is `0xC1`, its
length fits, its CRC matches and its body's SHA-256 equals the header's.
The copy is the counted bank with the newer sequence (serial order);
there is none when no bank counts.

**Write** (image lifecycle `written`, target TOPOLOGY): the host refuses a
body larger than a bank's capacity before sending any byte, and the hub
refuses it again before erasing. Otherwise the hub writes the bank that is
not the copy (bank 0 when there is none) with sequence = copy's + 1 (1
when none): erase the bank, program the body, program header bytes 1..63,
program the commit byte. Counting erased and programmed bytes from 0, a
write interrupted before step `bank size + body length + 63` leaves the
previous copy, byte for byte, and at or after it the new one; every later
read gives the same answer until the next write. A completed write makes
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
| P4 | No identity is appended twice while held; a writer continues its own sequence; a boot number held for two boots only after foreign bytes destroyed its first use. |
| P5 | The reader never reports a newer boot before an older one; pairs it cannot order are marked uncertain (on both boots), and only boots of different openers are. |
| P6 | Every replayed boot holds the events of one boot, and one boot is one replayed boot. |
| P7 | An append overwritten by its own writer before any replay is covered by a window or gap. |
| P8 | `plan`, `commit` and `confirm` take only the state log (compile-time). |
| P9 | A reader never accepts entries of another layout (in P2's lookup). |
| P10 | Region ≤ `RECORD_MAX`; paged reads at every link size reassemble the region; the 0x20 summary fits one page. |
| P11 | A consumer de-duplicating against its previous replay never meets one identity with two contents. |
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
| P23 | The boot core's run leaves the app's cell unchanged; the app's start acts on it. |
| P24 | A trip whose id is no longer installed is freed by its layer's start; a new build runs again. |
| P25 | Records of another fuse layout are never acted on. |
| P26 | A trip of an installed id disappears only by the host's clear. |
