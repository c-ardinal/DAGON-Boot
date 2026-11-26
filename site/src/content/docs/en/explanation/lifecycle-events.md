---
title: "Lifecycle events and extensions"
description: "One model for boot loader, agent, relay and host: state machines, events and ports."
---

This document is the design of DAGON's lifecycle-event and extension model. It is the reference for the implementation and for anyone extending DAGON. It shows:

- How every state transition in every layer is an observable event
- How board definitions, libraries and applications extend DAGON in the app and the host by observing events, answering the few decision points and replacing parts of DAGON through ports
- How board values and boot-time actions come from definitions
- How the model is versioned

The goal: DAGON and user code fully separated. A user (a board definition,
a library, an application) meets every need through DAGON's public API,
events, extension ports and definitions; nothing in DAGON is edited,
copied or forked; DAGON holds no knowledge of any sample, board or
peripheral. The model is written for needs that are not known in advance
(an automotive diagnostic transport, added security processing, a
display, a different storage), not for a fixed list.

The words of this document:

- *boot loader*: the image DAGON runs before the app (stage0 and the boot loader on a self-boot MCU; the vendor 2nd stage on a vendor-stage2 MCU).
- *stage0*: the small program that runs first after a reset. It picks the boot loader slot to start, and starts the confirmed app when no boot loader may start.
- *agent*: `dagon-agent` inside the app.
- *relay*: the hub's PC link and sub-node links.
- *host*: `dagon-host` driven by the CLI and `dagon serve`.
- *extension*: code or data outside DAGON that DAGON runs at one of its extension points.
- コーディネータ/ノード: hub/node.

## 1. State machines and their events

Every state change in DAGON happens through a transition of a declared
state machine, and every transition emits its event. The machines are
declared once each with `state_machine!` (1.7). The normative catalog is
the generated one: [Events](/en/reference/events/), produced from the machines'
`CATALOG` constants (kind, payload, attributes, observer path) and
checked against them by a test. The event tables in this section are
illustrative examples of what the machines produce; they carry no entry
the generated catalog does not, and when the two differ the generated
catalog is right. 1.8 shows, with the same caveat, how each class of
event is observed.

### 1.1 Image lifecycle (per slot, persistent)

The state of an image slot persists across resets in the slot store
(kind S: the state log; kind V: the vendor selection data such as
otadata). It is one machine with transitions in two layers: the agent
changes it over the wire, the boot loader changes it at boot. No public
path changes persistent image state outside this machine.

```
                 erase                  written                 pending_set
  Empty <────────────────── Written ───────────────> ... Written ────────────> Pending
    ^                          ^   ^                                              │
    │ erase                    │   │ pending_moved (another slot set pending)     │ attempt (boot)
    │                          │   └──────────────────────────────────────────────┤
    │                          │ demoted (the other slot confirmed)               v
  Reverted <── rolled_back ──  │                                                Trial ──confirmed──> Confirmed
    │        (boot)            │                                                  │                    │
    └──── erase ───> Empty     └──────────────────────────────────────────────────┘ attempt (boot), until max
```

| Transition | Layer | Guard (status on refusal) | Persisted as | Event |
|---|---|---|---|---|
| `erase` | app | XFER_BEGIN passed every guard and `accept_update`; from `Empty`, `Written`, `Pending`, `Trial`, `Confirmed` or `Reverted`, never from the slot the writer runs from (the session's `Writer` type: the app or the boot loader), and in the app never from its confirmed slot either. From `Pending` because the protocol lets a new image replace one set pending but not started; the erased header no longer matches the SLOT_PENDING record, so the slot reads empty | slot body and header place erased | `app.transfer_started` |
| `written` | app | XFER_END: body digest equals the header's, header written and re-verified | header in place | `app.written { slot, version }` |
| `pending_set` | app | SET_PENDING: header present, signature and compatibility, body digest; never the running slot; from `Written`, `Reverted`, `Pending` (a repeated SET_PENDING appends again) or `Confirmed` (the confirmed slot set pending again while the other slot is on trial, which switches back to it) | SLOT_PENDING record / vendor selection | `app.pending_set { slot, version }` |
| `pending_moved` | app | SET_PENDING on the other slot while this one is `Pending` or `Trial`, and this slot is not the confirmed one | the same SLOT_PENDING record (the log names one pending slot) | `app.pending_moved { from, to }` |
| `pending_withdrawn` | app | this slot is the confirmed one, set pending again, and another slot is set pending or its trial is rolled back: it reads `Confirmed` again, as the log's `confirmed` still names it | the same SLOT_PENDING or ROLLBACK record | `app.pending_withdrawn { slot, version }` |
| `attempt` | boot | pending slot checked (signature, digest), `attempts < max` | BOOT_ATTEMPT record / vendor "new" → "pending verify" | `boot.attempt_recorded { slot, attempt, max }` |
| `confirmed` | app | CONFIRM while on trial, `self_check` healthy | CONFIRMED record / vendor "valid" | `app.confirmed { slot, version }` |
| `demoted` | app | the other slot's `confirmed` (the log's `confirmed` now names the other slot) | the same CONFIRMED record | `app.demoted { slot, version }` |
| `rolled_back` | boot | attempts exhausted, or the pending image fails its check | ROLLBACK record / vendor abort | `boot.rollback { from, to, reason }` |
| `chosen` | boot | the slot to start is final (confirmed, or trial), its signature and digest checked | none | `boot.slot_chosen { slot, trial, version }` |
| `no_image` | boot | no startable slot | none | `boot.no_image { reason }` |

The extra source states of `erase` and `pending_set`, and
`pending_withdrawn`, exist because the protocol already allows those
requests; the machine follows the wire behaviour rather than narrowing
it.

One persisted record can move two slots (`confirmed` and `demoted`,
`pending_set` and `pending_moved` or `pending_withdrawn`): the machine
holds one state per slot, one trigger runs both transitions and both
events are emitted, in that order. A test drives the agent's
`ImageStore` over a store folded from the log (`LogState`) through every
sequence of what the agent and the boot loader write (transfers,
switches, confirms, boot attempts, rollbacks) from a provisioned device
and checks after each step that every slot's machine state equals
STATUS, so no fold outcome exists without its transition and the agent
takes them, the second slot's included.

The machine wraps the slot store port: `ImageStore<S: SlotStore>` holds
the store, and the port's mutators (`erase`, `write`, `write_header`,
`set_pending`, `confirm`) take a `Transition` that only the machine
makes, inside the transition that emits the change's event. The
`SlotStore` trait remains the port a board implements. The agent owns the
store and hands it to no one; the board's other resources (a flash
reader, the watchdog) are handles the MCU definition keeps for itself. What
those handles reach (a raw flash driver, a simulator's provisioning
helpers) is outside this guarantee: it is trusted code of the board or
bench, like anything with flash access in the same image. When a header write does not read back, the slot is
erased again, so a fault leaves no image the machine has not recorded.
The agent exposes `images() -> &ImageView`
(read-only: status, each slot's lifecycle state, own version, on
trial). An app that confirms itself at start
(bench use) asks its start for it (`App { confirm: Confirm::Own, .. }`), which
calls `confirm_own()` after the board's bring-up; that is the `confirmed` transition:
the `self_check` decision first, the port's confirm, the events always.
The app reports the boot-layer transitions at start from the state log
(`app.trial_started`, `app.rolled_back`).

### 1.2 Boot sequence

Self-boot MCUs (stage0 and boot loader): `Entry → HalReady →
WatchdogArmed → LogRead → BeforeDecision → Planned → Committed →
Jumping | Halted`. `plan` is pure (reads flash, computes the decision
and the records to append); `commit` performs the image-lifecycle
transitions `attempt`, `rolled_back`, `chosen` or `no_image`. Stage0
has only `Entry`, `CoreChosen` and `Halted`: it picks the boot loader slot
from the boot loader records of the state log and the trial cell
(`dagon_core::bootcore::start`; retained-formats.md 5): the pending core
while fewer than `trial_attempts` starts are counted for it, else the
confirmed one. A core starts only as a record names it: neither a core no
record names nor a pending core whose starts ran out starts. It checks that the core's header signature verifies, that
the header is the one the record names and that its body matches the
header's digest (4.3) before it picks the core, starts the watchdog before a core on trial, and runs
no extension. Stage0 never writes flash. When no core may start (on an MCU
with one boot loader slot, a new core cut by a power loss or one that
failed its trial), stage0 starts the app the state log's confirmed APP
record names, after checking the record's header digest, the signature
(4.3), the image kind, MCU, target tag, slot binding, placement and load
range, the body digest and that the entry point lies inside the body. It
starts no app while an APP trial is open, and no pending app; it leaves
the trial cell as it is and records nothing, and that app writes the boot
loader again. Only when there is no such app does stage0 start nothing
and wait for recovery (4.3). The started boot loader settles
the boot loader trial before its own boot decision is acted on
(`settle_log`): the pending core appends CONFIRMED (target BOOTCORE) only
after its own check (an app is startable and that app's header verifies
under the core's key) and resets otherwise; the old core, started while
another core is pending, appends the ROLLBACK. The watchdog rule is in
4.1.

The boot loader image lifecycle (the agent's `core_image` machine) follows
the app image lifecycle of 1.1 with the transitions the boot loader path
has: `erase`, `written` and `pending_set` by the agent (XFER and
SET_PENDING with target BOOTCORE into the idle boot loader slot, never the
running or the confirmed core's; when the MCU definition gives one boot
loader slot, into that one, and the agent refuses that transfer and its
pending setting while an app is pending or on trial, and an app's pending
setting while that core is not confirmed (pending, or its slot left
empty by a cut write), so the host asks to confirm or roll
back the app first), `confirmed` and `demoted` by the new
core's own CONFIRMED, `rolled_back` by the old core's ROLLBACK (replayed
at the app's start). There is no `Trial` state the app can see: a core
settles its trial before any app runs. STATUS with the target byte
`BOOTCORE` reports the boot loader slots (version, header digest, running
and confirmed core); a board whose MCU definition declares no boot loader
slots answers `Unsupported`. The number and placement of the boot loader
slots come from the MCU definition's `[[slot]] image = "bootcore"`
entries (one or two; what one slot cannot do is in
[Limitations](/en/explanation/limitations/)).

Update mode and recovery mode. The boot loader also receives: it runs the
same update session as the app (1.3), as the session's boot-loader writer,
and only in update mode or recovery mode, answering HELLO with `mode` 1
(update) or 2 (recovery); an app answers 0 and a recovery agent 3. It
enters update mode by a STAY (0x05) that arrives within its listen window,
or by an UPDATE_STAY record no UPDATE_DONE has consumed; it enters
recovery mode when no app may start. A log with no valid face is first
rebuilt from the slots (`dagon_core::boot::rebuild`): with two apps that
pass the boot checks, PROVISION for the older one and SLOT_PENDING for the
newer one, which starts on trial; with one, SLOT_PENDING alone, and a
failed trial ends in recovery mode; two of the same version give
PROVISION for slot A alone; with none nothing is written, and recovery
mode creates an empty log as it receives. The listen
window counts from the boot loader's start and is the MCU definition's
`[boot_receive] listen_window_ms` (0 to 200 ms, 50 when left out, 0 for
none; a project changes it with `[mcu_override]`). It opens on each link
kind of `[links]` that `[boot_receive] kinds` marks as taking part; a kind
the other end enumerates first (USB) cannot, and the boot loader receives
on it in update or recovery mode only. The boot loader receives on every
link its definition gives, none above another. An app hands over with
REBOOT mode 1: its store makes room for the trials, the UPDATE_STAY (0x06)
and the UPDATE_DONE (0x09) (`Log::request_stay`, which compacts while the
app runs), appends UPDATE_STAY and the app resets. The boot loader appends
UPDATE_DONE into that room as it enters update mode (`Log::take_stay`,
which never erases), so a stay is used once; a compaction keeps an
unconsumed stay. In update mode the boot loader erases and writes only the
slots a write names, makes the log's room before a write starts with the
shared reservation (`Log::reserve`: it compacts into the other face and
writes that face's header last, never erasing the only valid face), and
never erases the log before a REBOOT. STAY and REBOOT mode 1 keep it in
update mode; REBOOT mode 0 leaves it, and so do 60 s without a request
when STAY or UPDATE_STAY brought it there. Recovery mode has no app to
start, so the boot loader stays in it. While the app runs it refuses STAY
(`E_STATE`). Outside update mode the boot stages never erase, except to
rebuild a log with no valid face: no valid record is there to lose, so
face 0 is erased, the records go in and the face header goes last (a cut
leaves no valid face, and the next start rebuilds it again). The boot
loader's session writes only the image kinds the MCU definition's
`[writers]` lists for it (the app); a transfer of any other kind is
refused with `E_UNSUPPORTED` before anything is erased. Readers
that predate these records skip them and an agent that predates STAY
answers `E_UNSUPPORTED`, so `wire` and `state-log` keep their numbers.
The boot loader does not relay ([Limitations](/en/explanation/limitations/)).

The boot action program (5.2) runs only from `WatchdogArmed` on, so no
action runs before the watchdog protects the boot. There is no extension
code in the boot stage.

| Event | When | Payload | Action program |
|---|---|---|---|
| `boot.watchdog_armed` | watchdog started (4.1); from here the action program runs | `period_ms` | yes |
| `boot.log_read` | the state log read | `ok`, `pending`, `confirmed`, `attempts` | yes |
| `boot.before_decision` | `plan` is about to run; the last event before the decision | `pending`, `confirmed`, `attempts` | yes |
| `boot.decision_planned` | the decision is known, nothing written | `Decision` | yes |
| `boot.slot_chosen` | image lifecycle `chosen` | `slot`, `trial`, `version`, `entry` | yes |
| `boot.attempt_recorded`, `boot.rollback`, `boot.no_image` | image lifecycle (1.1) | | yes |
| `boot.before_jump` | last event before control leaves DAGON | `slot`, `trial` | yes |
| `boot.recovery` | a recovery path is entered instead of an app | `path` | yes |
| `boot.halt` | the core halts for good | `code` | yes (boot loader) |
| `boot.program_step` / `boot.program_skipped` | one boot action ran / the program was not run | `index`, `op`, `ok` / `why` | yes |

Vendor-stage2 MCUs have no DAGON boot stage: the app reports
`app.trial_started` and `app.rolled_back` from the selection data at
start.

### 1.3 Agent session

States `Idle`, `Header { slot, body_len, got }`, `Body { target, slot,
len, next }`; every protocol request is a transition (self-transitions
for HELLO, STATUS, VERSION and progress). The protocol itself is a port
(2.1): the machine's triggers are protocol-neutral (`update_offered`,
`header_chunk`, `data_chunk`, `transfer_end`, `switch`, `reboot`,
`confirm`, `status`), and a protocol extension (DAGON wire, UDS, …) maps
its messages onto them.

Generated order at start: `app.starting` → `app.store_opened` (or
`app.store_failed`) → `app.trial_started`
/ `app.rolled_back` → `app.watchdog_taken` → `app.bring_up` →
`app.link_up` …

| Event | When | Payload |
|---|---|---|
| `app.starting` | DAGON's start function entered | `tag` (as a name digest, 2.4), `role`, `mode` |
| `app.store_opened` / `app.store_failed` | state log or selection data read / unreadable | `active`, `trial`, `version`, `attempts`, `last_reason` / `error` |
| `app.trial_started` / `app.rolled_back` | replayed from the state log | `slot`, `version`, `attempt`, `max` / `from`, `to`, `reason` |
| `app.watchdog_taken` | the boot loader's watchdog taken over | `period_ms`, `trial_policy` |
| `app.bring_up` | board bring-up ran | `board_id` (as a name digest, 2.4) |
| `app.extension_started` / `app.extension_overran` | an extension's handler ran / overran (3) | `id`, `point` / `id`, `point`, `us` |
| `app.link_up` / `app.link_down` | a DAGON link carries frames (its first frame, at start or after it went down) / stopped (a reply could not be sent, or the link closed) | `link`, `addr` |
| `app.request` / `app.replied` | a request arrived (any protocol) / its reply left | `protocol`, `cmd`, `seq`, `via` / `status`, `ms` |
| `app.hello` / `app.status_asked` / `app.version_asked` | identity / status / VERSION answered | `via` |
| `app.update_offered` | a write request passed every DAGON guard; **decision** `accept_update` | `target`, `slot`, `body_len` |
| `app.update_refused` | refused | `status`, `by: Dagon \| Extension` |
| `app.header_verified` / `app.header_rejected` | header signature and compatibility (through the verifier port) | `slot`, `version`, `key_id` / `status` |
| `app.transfer_started`, `app.transfer_progress`, `app.transfer_aborted` | erase done / each chunk written / idle timeout, store error, new request | `target`, `slot`, `len` / `done`, `total` / `cause` |
| `app.verify_started`, `app.verify_ok`, `app.verify_failed` | body hashing and read-back | `len` / `slot`, `version` / `status` |
| `app.written`, `app.pending_set`, `app.pending_moved`, `app.pending_refused`, `app.confirmed`, `app.demoted`, `app.confirm_refused` | image lifecycle (1.1) | |
| `app.percent_told` | the host told the write's one progress number (the same as the DAGON Console's %; during a transfer, only when the node can take it; display tasks read `AppSnapshot::percent`) | `percent` |
| `app.reboot_requested` | reboot accepted; **decision** `allow_reboot_now` | `mode` |
| `app.before_reboot` | reply has left; reset follows | |
| `app.self_check` | confirm while on trial; **decision** `self_check` | `slot`, `version` |
| `app.rom_entry` | recovery entry accepted | `method` |
| `app.conformance` | conformance test served | `test` |
| `app.liveness_overdue` | the guard stopped feeding because a task is overdue | `task` |

### 1.4 Relay

The relay is also an app (its own agent emits `app.*`). Its own machine (`dagon_agent`'s `relay`):
PC link up/down and its relaying and ROM recovery work. Child links are ports (a child protocol
extension may relay UDS as well as DAGON frames).

| Event | When | Payload |
|---|---|---|
| `relay.pc_up` / `relay.pc_down` | first frame on the PC link / a reply to the PC could not be sent | `link`, `addr` |
| `relay.relay_request` | a request to relay admitted | `link`, `addr`, `cmd` |
| `relay.child_control` / `relay.child_raw` | a child's control lines driven / raw bus access to its ROM loader | `link`, `nrst`, `boot0` / `addr`, `len`, `read_len` |

The relay records in its own state log which of the nodes it relays to have answered ([section 6.1 of Retained and persistent formats](/en/explanation/retained-formats/#61-records-of-peers-that-answered)). When such a node first answers with a frame that passes its checks, the relay appends one record for it. PEERS reads the records, a page at a time: the request names the page, and the answer holds whether the records are complete, how many there are, and the `(link, addr)` pairs of that page (8 a page). Only a relay answers PEERS; the HELLO of any other node does not name it.

### 1.5 Host operations

The host notifies the progress of each operation (`apply`, `rollback`,
`recover`, `confirm`, `provision`, `build`) as events, plus the
project session and the agent link. The host's transport, signer and
build runner are ports (2.1).

| Event | When | Payload |
|---|---|---|
| `host.build.started` / `host.build.finished` | per node and slot | `node`, `slot`, `ok`, `elapsed_ms`, `image_sha256` |
| `host.release.prepared` | bundle made | `version`, `nodes[]`, `path` |
| `host.deploy.planned` | the listing shown; **decision** `confirm_plan` | listing |
| `host.deploy.started` / `host.deploy.finished` | | `release`, `nodes[]` / `ok`, `results[]` |
| `host.node.discovered` | discovery | `target`, identity |
| `host.node.step` | every phase for a node (TRANSFER, VERIFY, REBOOT, CONFIRM, REJECTED, FAILED, ROM) | `node`, `target`, `phase`, `percent`, `version` |
| `host.node.reconnecting` | waiting for the version after reboot | `node`, `elapsed_ms`, `other_version?` |
| `host.node.result` | | `node`, `Done(v) \| Failed(reason, status?) \| NotStarted \| NeedsFirstWrite \| Realigned(v) \| Incompatible(v)` (`NeedsFirstWrite`: the node does not answer and was never reached; nothing was sent to it and the other nodes are written; it takes its first write directly. `Realigned`: written and confirmed, then taken back to `v` for a compatibility constraint with a node that stayed off the package's version. `Incompatible`: such a constraint does not hold and the node could not go back; see [Keeping node versions compatible](/en/how-to/constraints/)) |
| `host.confirm.result` | | `Confirmed \| NotOnTrial \| NeedsChoice` |
| `host.rollback.*`, `host.recover.*` | same shape as deploy | |
| `host.provision.started` / `host.provision.recorded` | first install through the ROM; the installed boot image identity recorded | `node`, `boot_sha256` |
| `host.project.opened` / `host.project.closed` | project session | `path` |
| `host.agent.linked` / `host.agent.unlinked` | consent recorded / withdrawn | `agent`, `way` |
| `host.version.read` | VERSION answered or Unsupported | `target`, `VersionResp?` |
| `host.job.*`, `host.device.*` | serve job and device messages | |

#### A node that does not answer

`apply` tells a node that does not answer a relayed HELLO apart by the records of peers read from the node on the PC. The CLI, wasm and DAGON Console name the three cases the same way.

| Case | When | The write | `host.node.result` |
|---|---|---|---|
| Lost | A record of the node exists (it answered before) | Stops | `Failed(LOST)` |
| Never reached | The records are complete and none is of the node (a node added to the topology that was never written) | Skips the node, goes on, and points to its first, direct write | `NeedsFirstWrite` |
| History unknown | The records cannot be checked: the node on the PC does not serve PEERS, the read failed, or the records are not complete (no PEERS_KEPT, or the limit reached) | Stops | `Failed(HISTORY UNKNOWN)` |

- Only a node the node on the PC reported as not answering (RELAY `E_TIMEOUT`) is told apart. A node on the PC that does not answer is a failed connection: the host starts that exchange, and the host keeps no records.
- Only the records on the node decide. Whether the node answered earlier in the session does not.
- Once a node with unknown history answers again, nothing has to be told apart. A node never written takes its first write directly. Only a first write of the node on the PC makes its records complete.

### 1.6 Persistent state and its machines

Everything that survives a reset or a restart is behind a machine, and
every write to it is atomic in the sense of the last column: a reset or
power loss at any point leaves either the previous state or the new one,
never a half-written one that a reader accepts.

| Persistent state | Where | Machine | Transitions that write it | Atomic how |
|---|---|---|---|---|
| App image slots (headers, bodies) and their selection (state log kind S) | node flash | image lifecycle (1.1) | `erase`, `written`, `pending_set`, `pending_moved`, `attempt`, `confirmed`, `demoted`, `rolled_back` | body first, header last (a slot has an image only once its header reads back and verifies); the state log is append-only records, each with its own CRC, on two faces that alternate: a torn record fails its CRC and is ignored, the previous record stands |
| Vendor selection (kind V) | node flash | image lifecycle (1.1) | `pending_set`, `attempt`, `confirmed`, `rolled_back` | the vendor's two-sector scheme: a whole entry with sequence and CRC written to the other sector, the newest valid sequence wins |
| Boot loader slots and their records (the boot loader log, or the state log shared with the APP records where the MCU definition gives both logs the same faces) | node flash (self-boot with a stage0) | boot loader image lifecycle (`core_image`, 1.2) | agent `erase`, `written`, `pending_set`; the new core's `confirmed` / `demoted`; the old core's `rolled_back`; host `provision` (`host.provision.recorded`); stage0 `chosen` (no write) | same as the app slots and state log; stage0's start count lives in the trial cell (retained-formats.md 5), not in flash |
| Factory recovery agent (vendor-stage2 MCUs) | node flash, factory partition, header detached | image lifecycle (1.1), `erase` and `written` with target AGENT | agent, only while a confirmed app runs from an OTA slot | body first, header last; while it is written the running app (with its agent) stays the way back |
| Installed boot image identity (vendor-stage2 MCUs) | provisioning record in the project | host `provision` | `host.provision.recorded` | written to a temporary file and renamed over the old one |
| Agent consent | `~/.dagon/agent.json` | agent link: `Unlinked → Linked → Unlinked` | `host.agent.linked` / `host.agent.unlinked` | temporary file and rename |
| Project session (open project, release key) | serve process | project: `Closed → Open` | `host.project.opened` / `host.project.closed` | process memory under one lock |
| Diag step, extension timing, quiet windows, seen tables | RAM only, process-lifetime | not persistent; events only | | |

No persistent structure in this model has a header, counter or state
field that is rewritten in place; every durable write is a new record
that commits as a unit, or a whole file replaced by rename. The
formats this model introduces for persistent data (the compiled topology
with MCU identity) are
not specified in prose: they are designed as an executable model with randomized fault
injection, and the format that passes is the specification (5.5).

### 1.7 The transition primitive

A transition cannot exist without its event, because the only way to
change a machine's state is the generated transition method that
delivers it. A crate declares all its machines in one
`state_machine!`, invoked once, at the crate root:

```rust
dagon_events::state_machine! {
    /// The update session.
    pub machine session in App {
        initial Idle;
        states { Idle, Header { slot: u8, body_len: u32 }, Body { target: u8, slot: u8, len: u32, next: u32 } }
        transitions {
            update_offered(target: u8, slot: u8, body_len: u32): Idle => Header { slot, body_len }
                emits UpdateOffered = 0x10 { target: u8, slot: u8, body_len: u32 } observe(live, host);
            data_written(end: u32): Body { target, slot, len } => Body { target, slot, len, next: end }
                emits TransferProgress = 0x12 { done: u32 = end, total: u32 = len } observe(live, host);
            aborted(cause: AbortCause): * => Idle
                emits TransferAborted = 0x13 { cause: AbortCause } observe(live, host);
            // …
        }
    }
}
```

The macro generates, per machine: the state enum behind a private field
of a generated `Machine` type, built only in the declaring crate, with no
`Default` or `Clone`, whose only mutators are the transition methods;
each takes the layer's composed chain (2.3), builds the event and the
next state, delivers the event, then sets the state, and from a state it
does not leave from changes and delivers nothing. A `from` state is a
name, optionally binding fields, never a value pattern. It also
generates one payload struct per transition, `#[non_exhaustive]` and
neither `Copy` nor `Clone`, so only the declaring crate builds one; the
owned compact form of every payload and its decoding (2.4); `CATALOG`
(machine, states, and per transition: from, to, trigger, kind, code,
payload fields with their bytes, the `observe` paths, the `action` and
`retired` flags); the transition table generated from the declaration
(`StateId`, `Trigger` and `State::leaves(Trigger)`), which code that must
know whether a transition will follow asks instead of comparing names;
and a test per machine that drives every transition from every state
and checks the table against the catalog. An event carries only
its layer, its kind's code (`Event::kind()`, a `u8`) and its payload, so
an image that delivers events links no catalog name; names are looked up
from the code only where a person reads them (a log display,
`dagon_catalog::name`, and the generated pages). Per crate it defines the
`MACHINES` static, which the catalog registry (`dagon-catalog`) lists;
a test there fails while a crate that depends on `dagon-events` is
missing. A state no transition reaches, a transition without `emits` or
an `observe` path, a duplicate code, an oversized payload, a second
`state_machine!` in a crate, or a machine outside it are compile
errors. [Events](/en/reference/events/) is generated from the catalogs and a
test keeps the two equal.

Decision points (3) are never `emits` clauses: a new one is a design
change written here first, with its bound, default, answering parties
and safety argument, and is a separate trait method.

Events are emitted by DAGON only. Extensions observe them and never
construct them.

### 1.8 Who observes what

Every event has at least one observer path, and the path is an attribute
of the generated catalog, so the generated [Events](/en/reference/events/) carries this
table per event; a generated test fails on a kind with no path. *Live*
is an extension in the same image (2), and in the boot stage the boot
action program (5.2); *replay* is the app's agent re-emitting it at
start from the state log; *host* is the CLI and the serve stream, which also see
every device-side event that a reply or VERSION carries. Replay
delivers only kinds the catalog marks `replay` and not `retired`; any
other kind is refused.

| Events | Layer | Live | Replay | Host |
|---|---|---|---|---|
| `boot.watchdog_armed` and every later boot event, including `halt` and the program events | boot | yes: the boot action program (5.2) | | |
| `boot.slot_chosen`, `attempt_recorded`, `rollback`, `no_image` | boot | yes | the state log (`app.trial_started`, `app.rolled_back`) | STATUS |
| `app.*` | app | yes | `trial_started`, `rolled_back` are themselves replays | replies, relay `child_*` |
| `relay.*` | relay | yes (the hub app's chain) | | relayed requests |
| `host.*` | host | `on_event` callback (CLI, serve) | | stream, `--events` |

## 2. The extension model

One shape in every layer: **ports** DAGON exposes for replacement,
**extensions** that provide a port, observe events and answer decisions,
**events** with no-op defaults, and **one composition order** (board
definition → libraries → user). Extensions run in the app and the host
only. The execution context differs (embassy tasks in apps; threads and
JSON on the host), the shape does not. A declarative action program
(5.2) is boot data, not extension code.

### 2.1 Ports

A port is a trait DAGON calls for one replaceable part, with DAGON's own
implementation as the default. An extension that provides a port replaces
that part for the image it is linked into. Ports exist in every layer.

The port catalog is one table in code, `dagon_events::PORTS`, one
record per port (`id`, the set of layers it exists in, the default per
layer, the trust policy, the services it needs, a description),
from which this table, the generated [Ports](/en/reference/ports/), the manifest
validator (`provides` must name a port of the extension's layers) and
the `dagon status` port columns derive; a
test keeps this excerpt equal to the catalog. The parts of DAGON that
are *not* ports are listed in the same catalog with `port = false` and
a reason, so one source answers "can this be replaced".

<!-- generated from dagon_events::PORTS -->
| Port | Layers | Default | Trust policy | Services |
|---|---|---|---|---|
| `UpdateTransport` (bytes in and out of a link) | boot, app, relay, host | boot, app, relay: USB CDC, UART, I2C target, PIO UART; host: serial | untrusted | return, transport, dma, time, memory |
| `UpdateProtocol` (messages ↔ session triggers: identity, status, transfer, switch, reboot, confirm) | boot, app, relay, host | the DAGON wire | untrusted (it cannot skip a guard: the session machine runs the guards, not the protocol) | return, time, memory |
| `ImageVerifier` (header signature, key lookup) | boot, app | Ed25519 over the project key | trusted only | – |
| `Digest` (body hash) | boot, app | SHA-256 | trusted only | – |
| `CryptoProvider` (the primitives the verifier and digest use; hardware acceleration, HSM) | boot, app, host | software | trusted only | – |
| `DecisionPolicy` (attempt limit, which slot wins) | boot | `max_attempts` from the definition, pending before confirmed | trusted only | – |
| `WatchdogDriver` (the watchdog that guards boot and trial) | boot, app | the MCU's | trusted only | – |
| `SlotStore` (where images live) | boot, app | the MCU definition's flash layout | trusted only | – |
| `LogStorage` (where the state log lives) | boot, app | the MCU definition's flash layout | trusted only | – |
| `Recovery` (an additional recovery path: a service window for a transport, a vendor download mode) | boot, app | BOOTSEL/UF2, factory, ROM entry as the MCU and board define | untrusted, additive (an extension adds a path, never removes DAGON's) | return, transport, time, memory |
| `Diagnostics` (extra VERSION TLVs, extra status, a UDS DID table) | boot, app, relay | none | untrusted | return, time, memory |
| `Display` (what a phase or a boot outcome looks like) | boot, app, relay | boot: the `[boot] led` and `halt` of the board definition through the action program; app, relay: none | untrusted (at boot it runs in the sandbox with the declared pins and buses) | return, pins, buses, time, memory |
| `LogSink` (where `Recent` overflow and extension logs go) | app, relay, host | none | untrusted | return, events, time, memory |
| `SelfCheck` (the health answer when a slot on trial is confirmed) | app | healthy | untrusted (a decision point, 3) | return, time, memory |
| `BoardBringUp` (the board's power, pins and buses at app start) | app, relay | the board definition's | untrusted | return, pins, buses, time, memory |
| `NodeLink` (how a node reaches another node over a link, through `dagon_port::NodeLink`) | relay | I2C controller, PIO UART | untrusted | return, buses, transport, dma, time, memory |
| `Signer` (signing images and releases) | host | file key pair | untrusted (the host is trusted by definition; the device verifies) | – |
| `KeyStore` (receives the signing key from where the user chose to keep it (an environment variable, the OS key store, 1Password and so on)) | host | a key file, whose path `--key` or `DAGON_SIGNING_KEY` names | untrusted (the host is trusted by definition; the device verifies) | – |
| `BuildRunner` (building node images) | host | cargo | untrusted | – |
| `Agent` (an external agent the CLI and the page hand requests to) | host | data-defined (agent definitions) | untrusted | – |
| `Action` (a program or request run after a host event) | host | data-defined (action definitions) | untrusted | – |
<!-- end of generated table -->

A port's default is DAGON code. A manifest's `provides` must
name ports whose layer sets include every layer in the manifest's
`layers` (the UDS example of 8.4 declares `UpdateTransport`,
`UpdateProtocol` and `Diagnostics` for `app`).

### 2.2 Extensions and the manifest

An extension is a Rust crate (or, for the simplest kind, an action program
in a definition) with a manifest, `dagon-extension.toml`:

```toml
format = 1
id = "uds-transport"                 # a-z 0-9 -, up to 32
layers = ["app"]                     # where it may be linked
provides = ["UpdateTransport", "UpdateProtocol"]   # ports it replaces (may be empty: observer only)
observes = ["app.update_*", "app.transfer_*"]   # event kinds, validated against CATALOG
decides = ["accept_update"]          # decision points it answers (may be empty)
[capabilities]                       # what DAGON hands it, nothing else
peripherals = ["can0"]               # declared in the board definition's peripheral table
pins = []
buses = []
timer = true
[limits]
max_run_us = 2000                    # per call, measured (3)
```

Code shape, the same in every layer:

```rust
pub trait Extension {
    /// Its identity in events: the manifest id.
    fn id(&self) -> &'static str;

    /// Every event this extension observes, in order. Default: nothing.
    fn on_event(&mut self, _e: &Event<'_>, _ctx: &Ctx) {}

    // Decision points (3). `None` = abstain. Defaults abstain.
    fn accept_update(&mut self, _o: &UpdateOffer, _ctx: &Ctx) -> Option<Accept> { None }
    fn allow_reboot_now(&mut self, _r: &RebootRequest, _ctx: &Ctx) -> Option<Delay> { None }
    fn self_check(&mut self, _t: &TrialInfo, _ctx: &Ctx) -> Option<Health> { None }
}
```

`Ctx` holds the capability handles the manifest declared (a GPIO, a bus,
a timer, a peripheral window) and nothing else; a port implementation is
a separate `impl UpdateTransport for MyExt` on the same type. There is
no handle to the flash, the state log, the watchdog, the slot store or
DAGON's memory in `Ctx`.

The extension interface is the API boundary: an extension receives
only `&Event`, `&Ctx` and the decision arguments, none of which leads to
a chain, to published state or to replay. Composing a chain, delivering
into it and replaying are layer-start APIs. Code built into the same
image outside the extension interface (DAGON's crates, the application's
own start-up code) is in the trusted base, as for anything in one
address space.

### 2.3 Composition

A typed chain, order fixed at the type level and therefore at build
time:

```rust
pub struct Chain<A, B> { /* private */ }   // Chain::new(a, b)
impl<A: Extension, B: Extension> Extension for Chain<A, B> { /* on_event: both; decisions: each in turn, first Some wins */ }
pub struct Nop;  // no extension
#[macro_export] macro_rules! extensions { ($a:expr $(, $b:expr)*) => { … nested Chain::new … } }
```

Order: **publisher → board definition → libraries → user**, always, in
every layer. The layer's start (the
MCU definition's `start`) builds the chain itself
with `compose`: the publisher for the
`Published` state (2.4), then the board definition's chain, then the
user's `extensions![..]` (libraries first, then the app's own). The
result is an opaque `Composed`: its parts cannot be reached or
reordered, it is not an extension, and only the layer start holds it.
User code cannot put itself first, and `extensions![..]` never contains
the publisher. Notifications reach every extension;
decisions stop at the first non-abstaining answer. A port can be
provided by at most one extension per image: `dagon gen` refuses a project
whose chain provides a port twice (a library and the user both bringing
an `UpdateProtocol`), naming both.

Registration per layer:

- **App (embassy).** `node::start` / `board::start` take an `App`:
  the self-check function, `exts: Option<&'static mut (dyn Extension +
  Send)>` (the app's `extensions![..]` in a `static`; a reference, not a
  generic, because an embassy task cannot be generic and the serving
  task owns the chain) and `confirm` (`ByHost`, or `Own`: the start
  confirms the app's trial). The start runs the board's `BoardBringUp`
  port (`app.bring_up`; a board without one emits nothing) and then the
  own confirm before the link serves. The board definition's extension
  (the board definition's `extension()`; on ESP32-S3 `NodePeripherals::
  board_extension`) takes the chain's board place. The agent stays the
  start's and its serving task's: no public function hands it out. The MCU definition creates and keeps the layer's
  `Published`, composes it, and returns a `Reader` to the app. The
  agent's machines deliver every event into the chain, and the chain
  asks the decision methods at the three points. The serving task runs
  the chain's handlers as ordinary code with the time limit measured and
  reported.
- **Relay.** The relay's own agent is an app with the same chain; the
  relay machine's events and the `NodeLink` port go through it.
- **Host.** The host's extensions are data-defined kinds (agent
  definitions, transport definitions such as a serial or CAN adapter
  named by program or device) and Rust crates implementing
  `dagon_host::Transport`, `Signer` or `BuildRunner`, loaded as workspace
  dependencies of the CLI build. The host has no extension chain. The
  host runs with the user's permissions.

The app's agent takes the composed chain and nothing else:
`Agent::new(config, verifier, store, chain)`, where `chain` is
`compose`'s result and `verifier` the MCU definition's signature
verifier.
There is no separate hooks trait; observing events and answering
`accept_update`, `allow_reboot_now` and `self_check` are `Extension`
methods. The MCU definitions' `start` still accepts the app's
`fn() -> bool` self-check and wraps it as a one-method extension
(`SelfCheck`) in the user part of the chain, after the app's extensions:
it never abstains, so placed first it would hide their answers.

### 2.4 Waitable published state

Async display and log tasks never run inside a handler. The publisher at
the head of the chain copies each event into:

- `Latest`: an `embassy_sync::watch::Watch` (multi-receiver, last
  value) holding `Latest { state, last }`: the layer's snapshot, folded
  from every event (the app's `{ phase, progress, percent, version, trial }`, the
  relay's children), and the last event's record.
- `Recent<N>`: a fixed ring of the last N events as `EventRecord`, an
  owned, bounded form generated per event by `state_machine!`:
  `{ layer: u8, kind: u8, flags: u8, dropped: u8, seq: u32, payload:
  [u8; 16] }`. Fields are copied in their compact form the catalog
  states (a length, an id, a digest prefix: a name such as a node's tag or a board id is recorded as the first 8 bytes of the SHA-256 of a domain prefix and the name, which a host maps back through the names it knows). No reference is retained
  after the producer returns. Overflow drops the oldest; the records
  after it report the loss in `dropped`, up to 255 each, carrying the
  rest, and the producer never waits. `seq` wraps and compares by serial
  arithmetic.

Both live in one `Published<S, N>` that the layer crate (the MCU
definition, the host) creates, keeps and hands to `compose` by exclusive
reference, so the chain is its only writer. Display, log and app tasks
get a `Reader` (receivers, the latest state, the recent ring), which
only reads. The boot loader has no publisher: its events go to the boot
action program.

### 2.5 Host: event stream and package discovery

`dagon serve` emits every host event on `/v1/events` as

```json
{"type":"event","format":1,"layer":"host","kind":"node.result","at":"<RFC 3339 UTC>",
 "job":17,"payload":{"node":"n1","outcome":"done","version":"1.1.0"}}
```

Existing `job.*` and `device.*` messages are unchanged (serve-api 1; the
`event` message is an addition). Consumer contract: a consumer of the
stream or of `--events` output ignores message types, event kinds and
payload fields it does not know, and refuses only a `format` it does not
read; this is what lets variants be added without advancing
`EVENT_FORMAT`. The CLI prints the same events with `--events` (one JSON
line each).

Where serve finds packages: `dagon.toml [serve] packages` is a list of
globs relative to the project (`["nodes/*"]` in a generated project);
only directories matched there and holding a `Cargo.toml` are listed and
built. DAGON has no built-in search path.

## 3. Decision points

Everything not in this table is a notification. There is no decision
point in the boot loader: nothing outside DAGON influences what the boot
loader starts.

| # | Decision | Layer | Who may answer (first non-abstaining in order) | Bound | Default | Never overridable |
|---|---|---|---|---|---|---|
| 3.1 | `accept_update(&UpdateOffer) -> Accept` | app | board, libraries, user | 50 ms; after every DAGON guard passed, before erase | accept | cannot accept what DAGON refused (not called then); refusal answers the protocol's "state" status |
| 3.2 | `allow_reboot_now(&RebootRequest) -> Delay` | app | board, libraries, user | `Now` or `After(ms)`, `ms ≤ REBOOT_DELAY_MAX`, a build-time constant derived from the MCU definition's watchdog periods (`[watchdog] trial_ms`, `confirmed_ms`): half the shorter one. DAGON replies first, keeps feeding under the guard's policy while it waits, then resets; a watchdog reset during the wait is the same reset | now | a reboot cannot be refused; the delay is capped |
| 3.3 | `self_check(&TrialInfo) -> Health` | app | board, libraries, user | 50 ms; `Pending` keeps the "state" status and lets the host retry | healthy | cannot confirm a slot not on trial, cannot bypass the `confirmed` transition |
| 3.4 | `confirm_plan(&Prepared) -> bool` | host | the interactive user (TTY, page) or `--yes` | until the user answers | ask | key check and image verification happen before the plan is shown |

Bounds are measured: the composed chain times each extension's call on
its own with the layer's clock (DAGON's, from the MCU definition; not a
port); a late answer is ignored, as an abstention, and the chain emits
`app.extension_overran { id, point, us }` (`id` is the extension's id,
its 64-bit FNV-1a hash in the record).

## 4. Safety contract

### 4.1 The boot watchdog

The watchdog is armed right after HAL init with the MCU definition's
`trial_ms`, before the log is read and before any action runs; after
`plan` it is set to `confirmed_ms` when the decision is not a trial, and
fed once before the jump. The watchdog is fed by DAGON only, on its own
schedule.

### 4.2 Extensions

An extension has the API-shape guarantees only:

| Rule | Enforced by |
|---|---|
| No store access | Extensions receive read-only payloads; applications get `ImageView` (2.3); the slot store's mutators are reachable only through the image-lifecycle machine. |
| Verify/confirm order | The machines' transitions are the only state changes: `self_check` is consulted only inside the `confirmed` transition after the "on trial" guard; `accept_update` only after the DAGON guards. An extension's answer is a validated `enum`, never a closure DAGON executes. A protocol extension produces triggers; the session machine runs the guards. |
| Time | Decision bounds in 3; notifications have the same bound, measured. |
| Watchdog | `Ctx` has no handle to it. |
| Reboot | 3.2: capped delay, never refused. |

Relay extensions are app extensions. Host extensions run with the user's
permissions in the user's operating system; data-defined ones (agents,
transports named by program) run out of process under the shared
command transport's limits, and no host extension answers a decision
(3.4).

### 4.3 The authentication rule

The one authentication rule, applied by every verifier
(the `ImageVerifier` and `Digest` ports, whose defaults are the rule):

- **Signed bytes.** The image header's signature covers the header's
  bytes from offset 0 to the start of the signature field, which
  include the format number, the image kind, the MCU identity, the
  target tag, the slot binding, the version, the body length and the
  body's SHA-256; the body is authenticated by that digest.
- **Key selection.** The verifier accepts exactly the key whose id the
  header names, and only if that id is the project key embedded in the
  verifying image at build (`DAGON_PUBKEY`; the public test key only in
  a build that opts in). There is no key list on the device and no
  fallback key.
- **Who verifies what.** The agent verifies an image before `written`
  and the boot loader before `chosen` on every boot, for the pending and
  the confirmed slot alike: the state log carries no key, so a record
  alone never vouches for an image. Each boot runs one signature check
  for the slot it starts (two when a pending slot is refused and the
  confirmed one is checked next). A slot that does not verify never
  starts, and the boot loader starts no slot the log does not name. A boot loader image is verified under the rule
  by the agent before `written` and again before `pending_set` (the
  SLOT_PENDING record names its header digest).

  On a self-boot MCU with a stage0, stage0 checks the boot loader
  header's signature, under the project key at the place its MCU
  definition names (`[stage0] key`: `flash`, a constant built into
  stage0's own flash, is the one place), as well as the digests, on
  every path that may start a core: the pending core and the confirmed
  core. The confirmed app it starts when no core may start is checked
  under the same key (1.2). A core whose signature does not verify never starts, so
  code that can write flash without the signing key cannot make stage0
  start a core of its own. Stage0's verifier (`Ed25519Small`) accepts
  exactly what the host's and the agent's accept. The key is part of
  stage0, which is never updated in the field: replacing the project's
  signing key means rebuilding stage0 with the new key and writing it
  through the MCU's ROM loader. The boot loader built with the new key
  starts no app signed with the old one: such an app does not boot
  until it is written again, signed with the new key.

  Stage0 starts only a core the log names: a pending core with starts
  left, else the confirmed core. When the pending core does not check
  out (a corrupt slot, a record naming a slot without a core, or a
  signature that does not verify), it takes the confirmed core; when that
  does not check out either, or the log names no core that may start, it
  starts the confirmed app (1.2). It never tries a slot no record names.
  When there is no such app either, it starts nothing and waits for recovery through the ROM loader, and it
  tells why to the node's diagnostic output: on a board whose
  definition's `[boot]` has an LED and a blinking halt pattern, the LED
  blinks the reason's code (1 header, 2 header digest, 3 size, 4 image
  kind, MCU or slot, 5 load address, 6 body digest, 7 signature, 8 the
  log names no core that may start) five
  times faster than that pattern, then stays dark for one period, and
  again (such a pattern blinks every 10 ms or slower; a board with a
  shorter one is refused when it loads). A node without a board, or on a board without that LED or with
  its halt pattern off, shows nothing; stage0 decides the same either
  way.

  On a self-boot MCU without a stage0, the boot loader is the root
  and is verified by the host before it is written and by the agent when
  it is written through the write path.
- **Stage0 budget.** The MCU definition's `[stage0] region_bytes`
  (the size of `[layout] stage0`) is the whole of stage0 (vector table,
  the log reader, the checks with the verifier, the trial cell, the
  diagnostic output). Its linker script fails the link when the image
  does not leave 1/16 of the region free. RP2040: 16 KiB with boot2 (the
  boot ROM's 256-byte second stage). STM32F401RE: 16 KiB, all of sector
  0 where the core fetches its reset vector (the smallest erase unit
  holding it, so no write ever erases stage0). `scripts/stage0_size.py`
  builds the board-less build and one per board whose definition names
  the MCU, since a board puts its diagnostic output into stage0, and
  checks each part of the image (the verifier, SHA-2, dagon-core,
  dagon-format, the MCU package, and the rest) against the most bytes
  the package's `stage0/budget.toml` gives it, from the linker map, and
  its static RAM against that file's `[ram]`. `scripts/bootcore_size.py`
  checks each core the same way against its `bootcore/budget.toml`,
  its slot as the hard limit and no margin, since a core is written
  in the field. Stage0 is never written
  in the field but is rebuilt for every new node with the toolchain of
  that build, so an image without spare room becomes a first write that
  cannot be built. Both stage0s use the compact SHA-256 rounds, since
  the unrolled ones do not fit the RP2040's region. A 12 KiB region was
  measured and not taken: the builds are 13,392 to 13,852 bytes, and a
  12 KiB region allows 11,520 with its 1/16 left free. Byte-loop memory
  routines (the boot loaders' `small-mem`) take about 1.3 to 1.5 KiB off
  but raise a verify on a Cortex-M0+ from 49 to 73 million cycles, and an
  interrupt table of one entry takes 956 bytes off the STM32F401RE's image
  only. With both, by estimate, the STM32F401RE's builds fit (about 11,300
  bytes) and the RP2040's do not (about 12,000 to 12,450). After the first
  public release the stage0 region is fixed: a larger stage0 is a new format number,
  and existing nodes move to it by a first write through the ROM loader (a
  direct write), since the normal update path never rewrites stage0 (power-cut
  safety).
- **Stage0's clock and its verify time.** Stage0 runs its checks on the
  clock the definitions choose for the node: the MCU definition's
  `[[stage0.clock]]` setup that matches the board's `[clock] source`
  (the one with the highest `out_hz`), else the MCU's internal
  oscillator (a node without a board, a board without `[clock]`, or a
  source no setup matches). The build of a node's stage0 writes the
  chosen setup into it. Before it starts a core, stage0 puts the clocks
  back the way the chip starts them, so every boot loader starts from
  the same clocks on every board; its diagnostic output and the trial
  watchdog are timed on the clock each runs on. A signature check with
  `Ed25519Small` takes about 50 million core cycles on a Cortex-M0+
  (`crates/dagon-ed25519/tests/m0_cycles.rs` runs it on an ARMv6-M
  interpreter, without flash wait states, and fails above 55 million):
  on the RP2040 about 7.7 s on its ring oscillator at the nominal
  6.5 MHz (4.2 s to 28 s across its 1.8 to 12 MHz), and about 0.4 s at
  125 MHz from a board's 12 MHz crystal. The boot loader checks
  signatures with the same `Ed25519Small` and SHA-2 with compact rounds,
  so it leaves room for the update session; each boot it checks the
  header of the app it starts, which adds one such check to every boot,
  and feeds its watchdog at every step of the check.
  A stage0 built with the
  `verify-time` feature measures it on the node: it times each check in
  core clock cycles (SysTick), keeps the longest in `DAGON_VERIFY_CYCLES`
  and their number in `DAGON_VERIFY_COUNT` for a debug probe to read,
  shows the longest in tenths of a second, rounded up, as a blink code
  on the diagnostic output, and starts no core; the node gets its normal
  stage0 written back afterwards. The boot loader and the app take their
  clocks the same way, from the MCU definition's `[[bootcore.clock]]` and
  `[[app.clock]]` setups for the board's `[clock] source`, and their HAL
  sets the chosen one up; no stage assumes a crystal the board does not
  state. On the RP2040 only a crystal gives the app and the boot loader
  their USB clock, so a node whose uplink is USB does not build on a board
  without one, and there the boot loader receives only on the board's
  DAGON I2C and UART buses; the
  topology check (V-26) works out each node's UART speeds from the clock
  its app runs on, refusing a UART link on an internal oscillator whose
  tolerance the definition does not state.
- **No erase under a trial watchdog.** Stage0 never writes flash, and the
  boot loader writes its records (BOOT_ATTEMPT, ROLLBACK, the boot loader's
  CONFIRMED or ROLLBACK) only into room the log already has
  (`Log::append_in_place`): the agent reserves room for the trials an
  image starts when it sets it pending (`Log::reserve`, `trial_room`),
  and the app's agent restores it at start (`SlotStore::keep_room`)
  after a power cut tore a record. Until then the boot loader cannot append
  to the torn log, so it starts no trial it cannot record and starts the
  confirmed slot, the log still naming an open trial. The running slot is
  therefore never inferred from the log: STATUS's `active` is the slot the
  app itself knows it was started from (its link slot; the running
  partition on the ESP32-S3), `Trying` is reported for that slot only, and
  CONFIRM, which confirms only the running slot, never confirms an image
  that did not start. Erases run only in the app, under a
  watchdog period the definition check proves long enough: both
  `[watchdog]` periods are at least twice each `[execution]
  erase_regions` entry's `unit_max_ms`.
- **Trial cell.** A node whose trial cell does not hold what is written
  to it (`bootcore::cell_holds`, checked by the app's store and by the
  boot loader) takes no boot loader (XFER refused as unsupported), and a
  pending core that finds it broken records its own rollback, so a
  broken core is never retried without a count.

- **The root image** (stage0 where it exists, else the boot loader) is
  written through the MCU's ROM loader at provisioning. ROM loaders
  verify nothing of DAGON's: whoever can drive the ROM loader can
  replace the root, and with it every rule. The rule therefore holds
  against every write path DAGON offers (its wire, a replaced transport
  such as UDS, the hub relay) and not against physical reprogramming
  through the ROM. Where the MCU offers protection of the root image,
  the definition lists it with its levels (`root_protection`: for the STM32, `rdp1` is reversible by a mass erase
  and `rdp2` is irreversible, plus sector write protection `wrp`;
  nothing on the RP2040, whose boot ROM always honours BOOTSEL; secure
  boot and flash encryption eFuses on the ESP32-S3), `dagon provision`
  offers to set a level and says which are reversible, and VERSION
  reports the exact level and the protected sectors (0x07), so the host
  shows a node whose root is replaceable, reversibly protected, or
  irreversibly protected.

## 5. Board values, action programs, shared resources, VERSION, modelled formats

### 5.1 Board values from definitions, never MCU code

`dagon-board.toml` carries a `[boot]` table: `led` (a pin name or GPIO
number), `led_active = "high" | "low"`, `halt = { pattern = "blink" |
"code" | "off", period_ms }`, optional `power_hold = { pin, level }`,
and `[[boot.action]]` entries (5.2); and the tables extensions'
capabilities are validated against: `[[pin]]`, `[[bus]]`,
`[[peripheral]]`, `[[device]] { bus, addr, writable = [regs] }`. The
definition reader refuses unknown keys, so a definition that uses these
tables states the `definitions` format that has them (6). `dagon gen`
writes `boot_wiring.rs` for the boot loader build (`LED`, `HALT`,
`PROGRAM`); a
board definition ships its definition and the boot loader build reads it the
same way. The MCU crates take these constants and hold no pin, pattern
or environment-variable default of their own.

`dagon-mcu.toml` carries what the MCU, not the board, determines:
`[watchdog] trial_ms, confirmed_ms` and `boot_capabilities` (5.2).

### 5.2 Boot action program: board facts as data

For a board that needs an LED pattern or a power-hold line and no code,
the board definition holds a declarative, bounded program that DAGON
interprets. The program is data, not extension code; it runs on
action-capable events: only from `WatchdogArmed` on, only on declared pins, buses
and devices.

```toml
[boot]
led = "GPIO25"
led_active = "high"
halt = { pattern = "blink", period_ms = 100 }
power_hold = { pin = "GPIO14", level = "high" }

[[boot.action]]
on = "watchdog_armed"              # any boot event marked action-capable in CATALOG
ops = [
  { op = "gpio", pin = "GPIO14", level = "high" },   # the power hold line
  { op = "delay", ms = 2 },
  { op = "led", state = "on" },
]

[[boot.action]]
on = "before_jump"
ops = [{ op = "led", state = "off" }]
```

Op set, `boot-program` format 1:

| Op | Fields | Cost | Notes |
|---|---|---|---|
| `led` | `state: on \| off \| toggle` | ~0 | the `[boot] led` pin |
| `gpio` | `pin`, `level: high \| low \| input` | ~0 | pin must be in the board's pin table and marked `boot = true`; set and left; on the declared `power_hold` pin only its hold level is accepted |
| `delay` | `ms ≤ 20` | `ms` | total per event ≤ 50 ms |
| `bus_write` | `device` (a `[[device]]` entry), `reg` (in its `writable` list), `bytes ≤ 8` | timeout 5 ms | only if the MCU definition declares `boot_capabilities = ["i2c"]`; arbitrary addresses or registers are refused by the validator |

Limits (validated by `dagon gen`, re-checked by the interpreter): `ops ≤
32` per event, `delay total ≤ 50 ms` per event with bus ops at their
timeout. No loops, branches, reads, waits on input, flash or state
access, clock or watchdog control. A boot loader that does not know an op
treats the program as invalid as a whole (`boot.program_skipped`).

### 5.3 Shared resources

A board resource several parties need is shared, not owned.

- A bus the board definition needs for bring-up and recovery and the app
  needs for its own devices is exported as `pub static SYS_I2C:
  Shared<I2c<'static, Blocking>>` (`dagon_events::Shared<T>`, an
  `embassy_sync::mutex::Mutex` taken with `try_lock`), used through
  `with(|bus| ..)`: a lease, not a critical section. Interrupts stay
  enabled while the lease is held, because no operation of unbounded
  length runs with interrupts masked (the execution model), and a bus
  driver may wait for its own interrupts; a nested or concurrent `with`
  answers busy instead of waiting. DAGON's bring-up and recovery take it
  through `with`; the app does the same at any time. The
  definition exports its device constants and register-level functions, so
  an app copies nothing.
- Board bring-up is the board definition's extension on `app.bring_up`
  (the `BoardBringUp` port); a user step is an extension later in the
  chain on the same event, or a task waiting for `Latest.phase ==
  Serving`.
- A board definition holds the board, not what is stacked on it; a stacked
  peripheral's driver lives with the app that uses it, and the definition
  hands out the bus pins as plain peripherals.

### 5.4 VERSION query

Command `Cmd::Version`, part of the wire contract. A node that does not
implement it (a recovery agent, a minimal image) answers `E_UNSUPPORTED`
through the unknown-command rule, which the host shows as "versions
unknown". Request: no field. Response: one field per value, in the field form every payload
uses (`key, len, bytes`; the key is the number below times two, and the
numbers rise through the response); a reader skips the numbers it does not
know; a value the device cannot determine is omitted, never a sentinel.
Every value is at most 255 bytes and the whole response fits the smallest
link payload (10).

| number | Value |
|---|---|
| 0x01 | boot loader version (`Version`, LE u32) |
| 0x02 | installed boot image identity: first 8 bytes of its SHA-256 |
| 0x03 | agent (`dagon-agent`) version |
| 0x04 | MCU definition version |
| 0x05 | board definition version |
| 0x06 | MCU identity (5.5): the image header's `mcu_id`, 8 bytes |
| 0x07 | isolation (4.5): `level u8`, `kind u8`, `boot_extensions u8`, `root_protection` (kind and exact level as the definition enumerates them, e.g. `rdp1`/`rdp2`, the write-protected sectors, and whether the level is reversible), `fuse` (Clear, or Tripped with the disabled extension's identity as the model encodes it, 5.5) |
| 0x08 | extensions, for display: a packed array, per linked extension `id` (32 bytes, zero padded), `trusted u8`, first 8 bytes of its source hash (4.7) |
| 0x09 | extensions, for identity: a packed array, per trusted extension its full 32-byte source hash (4.7) |
| 0x0A | stage0 version: the version mark stage0 carries in its region (`dagon_format::stage0`; `Version`, LE u32); omitted for a stage0 without the mark and on a vendor-stage2 MCU |
| 0x0B | boot loader slots A, B: `active u8` (the running slot), then per slot `state u8` (STATUS slot state) and `version` (LE u32) |
| 0x0C | app slots A, B, factory, in the shape of 0x0B |
| 0x0D | wire version the agent speaks (`PROTO_VERSION`, u8) |
| 0x10 | formats: pairs `(format id u8, number u8)`: wire, image-header, state-log, event, definitions, boot-program, boot-record, extension-manifest |
| 0x11 | boot record summary (5.5): which boots the record holds and the decision and reason of the newest; layout by the model |
| 0x12 | one page of the raw boot record region: `offset u16`, then `len` bytes (`len ≤ RECORD_PAGE`, 10); the host reads the region page by page and decodes it with the model's reader |

The running boot loader is identified from flash, never from what the app
was compiled against:

- **Self-boot with boot loader slots and an inline header**: the app reads
  the boot loader log to learn which boot loader slot was started, then that
  slot's header (0x01) and hashes the image (0x02).
- **Self-boot with a trailer header**: the app reads the trailer (0x01)
  and hashes the body (0x02).
- **Vendor-stage2**: the installed boot loader has no DAGON header. The
  app reads it from flash at the vendor's boot loader offset, determines
  its length by walking the vendor image's segment table, hashes it
  (0x02), and omits 0x01. The host matches 0x02 against the identity
  recorded at provisioning (`host.provision.recorded`, kept in the
  project) and against the MCU definition's vendor table, and shows the
  version or "unknown vendor stage (hash …)".

`dagon status` prints one line per node with these values; `dagon doctor`
compares the format numbers with `compat.toml` and warns on a number the
CLI does not read.

### 5.5 MCU identity and compiled topology: requirements and the executable model

The MCU identity of this model is data whose correctness depends on
byte-level behaviour under resets, power loss, partial writes and version
skew: the image header, the identity reply, VERSION and the compiled
topology's node layout carry it.
Prose
cannot show that such a format survives every interleaving of faults;
an executable model can. These formats are therefore not specified in
this document. They are designed as a host-side Rust model with
randomized fault injection, and the shape that passes its properties
becomes the written specification of each format, published with the
model's tests as its proof.

This section states what those formats must satisfy; the model is
accepted only when every property below holds under its fault injection.
The formats that pass are specified in
[retained-formats.md](/en/explanation/retained-formats/), with the model's property list
and the limits of each claim.

#### MCU identity

- The MCU is identified by a value derived from its definition's `id`,
  not by a hand-assigned number; adding an MCU is a definition, never a
  table edit, and no numeric MCU table exists anywhere (a test scans the
  sources).
- The identity is unique: the definition loader refuses two definitions
  whose identities collide in any field width the identity is carried
  in, naming both ids; a device whose identity no loaded definition
  matches is shown as unknown, never matched to another.
- The image header, the identity reply and VERSION carry it in a way
  the model shows to be unambiguous for the host and the hub; the width
  per carrier and the hash function are the model's output.

#### Compiled topology

- The body format (`topology-bin`) goes into the release package and the host reads it. A node keeps no copy of the topology. The format is in [retained-formats.md](/en/explanation/retained-formats/) section 3.

#### What stays in prose

Every other byte-level structure in this document is either generated at
build time and validated before it reaches a device (the boot action
program table, `boot_wiring.rs` constants), or a
request/response encoding with no persistence (VERSION's fields,
the serve JSON messages), or an existing contract this model does not
change (the image header's signature and digest fields, the state log's slot
records, the vendor selection data, the release bundle). None of them is written
twice or read back after a fault, so none needs the model.

## 6. Versioning rules

Numbers and components follow [Versions and releases](/en/explanation/versioning/) and are registered in
`compat.toml`. The rules specific to this model:

| Contract | Number moves when | Reader rule |
|---|---|---|
| `event` (`EVENT_FORMAT`) | a variant is renamed or removed, or a payload's meaning changes; adding a variant or a payload field keeps it | a reader ignores kinds and fields it does not know; refuses a number it does not read |
| `extension-manifest` | a manifest key's meaning changes or a key the reader refuses is added | `dagon gen` refuses a manifest with a number it does not read, naming both numbers |
| `boot-program` | an op is added, removed or its fields change | a boot loader that meets an op it does not know treats the whole program as invalid and emits `boot.program_skipped`; `dagon gen` refuses to generate a program for a boot loader whose number does not read it |
| `image-header`, `topology-bin` | the MCU identity field or the node layout changes (5.5) | a boot loader and an agent accept the `image-header` numbers `compat.toml` lists for them, and the host the `topology-bin` numbers; each refuses others by number. No node reads `topology-bin` |
| `definitions` | a key is added that the reader refuses (the reader rejects unknown keys), or a key's meaning changes | a definition states the number it needs; the CLI reads every number it supports and names both numbers when it refuses one |
| `wire` | a command's request or response changes meaning; adding a field or a command keeps it, and no device sends unsolicited frames (the wire is request/response through the relay) | a receiver answers an unknown must field with `E_FIELD` naming it and skips an unknown optional field (every response field is optional); it answers a command it does not know with `E_UNSUPPORTED`, which the host shows as unknown |
| `serve-api` | unchanged by the `event` message, an addition | consumers ignore unknown message types |

A number rises only when a reader of the old number would misread the
new data or must refuse it ([Versions and releases](/en/explanation/versioning/));
the table above applies that rule to each contract.

Component kinds: a change in a crate's public API that is incompatible
is breaking for that component; a new event, port, op, VERSION field or
definition key with a default is a feature; everything else follows the
general rules. A port trait gains methods only with defaults; removing
or changing a port method is breaking for the layer's component.

Deprecation: a public trait or function this model replaces (a store
accessor, a start parameter) stays available for one
minor release of its component, marked `#[deprecated]` with the
replacement named, and is removed in the next breaking release. A
`CATALOG` entry is never removed within a format number; a kind that is
no longer emitted stays in the catalog, marked `retired`, until the next
`event` number.

## 7. Attaching and detaching a DAGON-enabled app

The boot loader expects a slot with a signed header for this MCU (by its identity, 5.5), target tag and slot, a body whose digest matches, a vector table at the body start, and an app that feeds the watchdog the boot loader armed (on trial: confirm within the trial period or be rolled back).

An app that does not link DAGON can still be installed through the ROM path and runs after signing. It never confirms, so it rolls back after `max_attempts` unless installed as confirmed by `dagon provision`.

To attach DAGON: add the MCU and board definitions, call `start` with an extension chain (`Nop` is valid), read the published events through the `Reader` it returns, keep the guard fed.

To detach: stop calling `start`. The boot loader still verifies and counts attempts. The host sees no identity reply and `dagon status` says so.

No extension, event, definition or boot action changes what the boot loader checks.

## 8. Examples

### 8.1 Splash and write progress on a display (app)

DAGON draws no screen. It hands an app only the data to draw from: the node's events and the latest `AppSnapshot` (the `phase`, the running `version`, the bytes received `progress`, the write's `percent` as the host told it, and whether the image is on `trial`), and, from the `wiring.rs` that `dagon gen` writes, the node's own entry `NODE` and the list `REACHED` of the nodes its links reach (index, name, link, address). The example below is an app that draws from them.

```rust
#[embassy_executor::task]
async fn display_task(mut lcd: Lcd, events: AppEvents) {
    // AppEvents = Reader<dagon_agent::snapshot::AppSnapshot, RECENT>
    let mut rx = events.receiver().unwrap();
    loop {
        let s = rx.changed().await.state;
        match s.phase {
            Phase::Starting | Phase::Idle | Phase::Trial => lcd.splash(s.version, s.trial),
            Phase::Transferring => lcd.bar(s.percent), // Option<u8>
            Phase::Verifying => lcd.text("verify"),
            Phase::Written | Phase::Pending => lcd.text("written"),
            Phase::RebootPending => lcd.text("reboot"),
            Phase::Confirmed => lcd.text("ok"),
            Phase::RolledBack | Phase::Failed => lcd.text("back"),
            _ => {}
        }
    }
}
// main: static EXTS: StaticCell<MyApp> = StaticCell::new();
//       let app = App { exts: Some(EXTS.init(MyApp)), ..App::new(self_check) };
//       // I2C_ADDR: the node's I2C address from its wiring (LINK_ADDR)
//       let events = node::start::<Board, N>(spawner, dagon, &GUARD, tag, app, UPLINK, I2C_ADDR)?;
//       spawner.spawn(display_task(lcd, events))
```

### 8.2 Power hold at boot (board definition, no code)

```toml
[boot]
led = "GPIO25"
led_active = "high"
halt = { pattern = "code", period_ms = 250 }
power_hold = { pin = "GPIO14", level = "high" }

[[boot.action]]
on = "watchdog_armed"
ops = [{ op = "gpio", pin = "GPIO14", level = "high" }, { op = "led", state = "on" }]

[[boot.action]]
on = "no_image"
ops = [{ op = "led", state = "off" }]
```

A power manager behind a declared I2C device, on an MCU whose definition
declares the `i2c` boot capability and a board that declares the device:

```toml
[[device]]
id = "pmic"
bus = "i2c0"
addr = 0x6E
writable = [0x07]

[[boot.action]]
on = "watchdog_armed"
ops = [{ op = "bus_write", device = "pmic", reg = 0x07, bytes = [0x20] }]
```

### 8.3 Logging every event (app or relay)

```rust
pub struct DefmtLog;
impl Extension for DefmtLog {
    fn id(&self) -> &'static str { "defmt-log" }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx) { defmt::info!("{}", defmt::Debug2Format(e)); }
}
// … extensions![DefmtLog, MyApp]
// The log shows the generated order of 1.3: app.starting, app.store_opened,
// app.trial_started, app.watchdog_taken, app.bring_up, app.link_up …
```

Or asynchronously, without touching the serving task's time: a task that
drains `events.recent()` from the `Reader` (owned `EventRecord`s, 2.4).

### 8.4 A replaced write transport and protocol: UDS over CAN

An automotive node is written by a diagnostic tester speaking UDS
(ISO 14229) over CAN with ISO-TP (ISO 15765-2), not by DAGON's wire
over a serial link. One extension provides the ports and runs in the
app.

Manifest:

```toml
format = 1
id = "uds-can"
layers = ["app"]
provides = ["UpdateTransport", "UpdateProtocol", "Diagnostics"]
observes = ["app.transfer_*", "app.verify_*"]
decides = []
[capabilities]
peripherals = ["can1"]       # declared in the board definition's peripheral table
timer = true
[limits]
max_run_us = 500
```

The extension, in the app's chain, provides the ports to the agent: the
serving task reads frames from the `UpdateTransport` (ISO-TP reassembly
inside the extension), the `UpdateProtocol` turns UDS services into
session triggers, and the agent's replies go back the same way. The
mapping is `RequestDownload` → `update_offered`, `TransferData` →
`header_chunk`/`data_chunk`, `RequestTransferExit` → `transfer_end`,
`RoutineControl(switchSlot)` → `switch`, `ECUReset` → `reboot`; the
session machine runs every DAGON guard and the verifier port on each
trigger exactly as for the DAGON wire, so a tester cannot skip the
signature or write the active slot. The extension's `Diagnostics` port
answers `ReadDataByIdentifier` for the identifiers it declares (the
versions from the same values VERSION carries). `accept_update` and
`self_check` stay with the board definition, libraries or the app
(`decides = []` here).

What the extension cannot do through the API: reach the
slot store or the log (no handle), confirm a slot (not a decision it
declares), or change what the boot loader starts.

### 8.5 Confirming at start (bench use)

```rust
struct Bench;
impl Extension for Bench {
    fn id(&self) -> &'static str { "bench" }
    fn self_check(&mut self, _: &TrialInfo, _: &Ctx) -> Option<Health> { Some(Health::Healthy) }
}
// main: let app = App { exts: Some(EXTS.init(Bench)), confirm: Confirm::Own, ..App::new(always) };
// The start, after the board's bring-up and while the image is on trial, calls
// agent.confirm_own(): app.self_check, app.confirmed (and app.demoted for the other slot).
```

## 9. Alternatives rejected

| Alternative | Why rejected |
|---|---|
| A boot-time veto answered by an extension | Any untrusted answer that turns a `Boot` into a rollback lets a faulty extension keep a valid image from ever running. Boot policy is DAGON's. |
| Public slot store mutators with the agent as one user among others | Persistent image state could then change without its event and without the confirm rule; the image-lifecycle machine is the only writer. |
| A fixed hook trait extended with a method per new timing | No composition, no payload stream for displays, no replacement of parts, and the list must be fixed again at every timing not foreseen. |
| A hand-maintained event enum checked against a hand-maintained catalog | Two lists can agree while a transition emits nothing; only a primitive that cannot change state without emitting makes the catalog derived. |
| Linker-section registries (`linkme`) | Entry order undefined; hidden registration by libraries; `dyn` dispatch cost and size. |
| `dyn Extension` list with `alloc` on the device | No alloc below the host; a static slice reintroduces the registry problem. |
| Channels for everything | Decisions and ports need a synchronous answer; a queue is the right tool only for the published state. |
| Retaining borrowed payloads in the waitable ring | A static ring cannot hold references past the producer; the generated owned compact form (2.4) is bounded and complete enough for display and log tasks. |
| Unsolicited event frames from devices to the host | The wire is request/response relayed hop by hop by the hub; a frame nobody asked for has no route, no sequence to retry on and no place in the relay's quiet-window model. The `EVENT` frame flag is reserved and unused; the host derives the device transitions from replies and VERSION. |
| Board values through build-time environment variables | MCU knowledge in disguise, in no definition; replaced by `[boot]` in the board definition. |
| Reporting the boot loader version the app was compiled against | Does not identify what is installed; VERSION reads the installed image from flash. |
| A fixed reboot-delay cap | A number chosen once is wrong for an MCU with a shorter trial watchdog; the cap derives from the MCU definition's periods. |
| Specifying the retained-memory and identity formats in prose | A format whose correctness depends on every interleaving of resets, partial writes and version skew is shown correct by an executable model with randomized fault injection, not by prose (5.5). |

## 10. Size limits and where they come from

Every limit has one source of truth; every place that must fit it is
listed, and a test asserts each pair.

| Limit | Value | Source of truth | Must fit it |
|---|---|---|---|
| Link payload per kind | USB 1088, UART 1024, I2C 256 bytes | `dagon_proto::max_payload` | every request and reply; VERSION response; display text; conformance detail |
| Smallest link payload | `max_payload::MIN` = the minimum over the kinds above (derived, not written) | `dagon-proto` | VERSION response as a whole (every field set the device may answer) |
| VERSION field value | ≤ 255 bytes | VERSION encoding (`dagon-proto` command table) | every number in 5.4 |
| Extension time budget | `max_run_us ≤ APP_EXT_MAX_US` | `dagon-events` `const fn` from the MCU definition's `[watchdog]` | `dagon gen` validator; the serving task's measurement; the manifest |
| Boot action program | `ops ≤ 32` per event, `delay total ≤ 50 ms`, `bytes ≤ 8` per `bus_write`, `delay ≤ 20 ms` | `dagon_format::bootprogram` constants | `dagon gen` validator; the boot loader interpreter; the `boot_wiring.rs` table type |
| Live event payload | ≤ 16 bytes for boot events (by value); larger app/host payloads by reference | `dagon-events` size assertions | `Published` snapshot; the compact form |
| `EventRecord` | 24 bytes: `layer`, `kind`, `flags`, `dropped`, `seq u32`, `payload [u8; 16]` | `state_machine!` `compact` attribute (a generated test asserts every kind has one) | `Recent<N>` |
| Catalog | no numeric cap; `kind` is `u8` per layer | `state_machine!` | the record `kind u8` (a generated test fails at 256 kinds in a layer); serve stream |
| Recent ring | `N` chosen by the layer crate | `Published<S, N>` | the layer crate's `Published`; overflow counts `dropped` |
| Decision bounds | 50 ms; `REBOOT_DELAY_MAX = min(trial_ms, confirmed_ms) / 2` | `dagon-events` constants; the MCU definition's `[watchdog]` | the serving task's timer; `boot_wiring.rs` |

## 11. Execution model

What runs where in DAGON's own code, how long DAGON's code may mask
interrupts, and what a flash operation stops, per MCU. This section is
normative: the MCU definitions are written to it, the MCU definitions carry
its hardware facts (`[execution]`, `[watchdog]`), and the checks named
below keep code and data to it.

### 11.1 Rules

1. **Work in tasks, not handlers.** DAGON's serving loops, its relay and
   its watchdog feeding run as tasks on executors; interrupt handlers of
   DAGON's own only wake tasks or move bytes (link drivers, an
   executor's own interrupt). Nothing that waits, erases or writes flash
   runs in a handler.
2. **Masking budget.** A critical section in DAGON's own code (the
   watchdog guard's state, a lock's state) masks interrupts for at most
   the MCU definition's `[execution] masked_max_us`. No flash erase or
   program, and no other operation without a bound in microseconds, runs
   inside one. A flash driver with several users is shared through a
   lock that is *taken* without masking for the operation
   (`dagon_mcu_common::shared::SharedFlash`, a `try_lock`; a handle that
   finds it taken answers an error at once, it never waits). Enforced by
   clippy: `targets/mcu/clippy.toml` refuses every masking primitive
   (`critical_section::with` and `acquire`, `cortex_m::interrupt::free`
   and `disable`, a critical-section `blocking_mutex::Mutex::lock`) in the
   MCU definitions and their boot images (stage0, boot loaders), by resolved
   path, so an alias or a wrapper does not hide
   one. The functions that mask (the watchdog guard's state update) carry `#[allow(clippy::disallowed_methods)]`
   with their reason, and a test keeps the list and the exemptions as they
   are reviewed. The same list and test hold for the code the MCU
   definitions run from other crates (`crates/dagon-relay`, and
   `crates/dagon-events` for the published events). Beside the guard,
   the `Channel` and `Watch` of embassy-sync that carry published events
   and the relay port's requests mask for each of their operations, to
   copy one value of a fixed size in or out; the operations that would run
   a closure there (`send_modify`, `try_get_and`) are refused by the same
   list, so a snapshot is folded outside them. These sections are bounded
   by the size of the value, not timed by a `masked-time` build.
3. **Flash operations are hardware facts.** What an erase or program
   stops is the definition's `[execution] flash_op`:
   - `fetch-stall`: single-bank flash the code runs from; instruction
     fetch and flash reads stall until the operation ends. Interrupts
     stay enabled and wait; nothing runs (DAGON keeps no handler in RAM).
   - `xip-off-masked`: the operation runs from RAM with execute-in-place
     off; the flash driver masks interrupts (and stops the other core)
     for that one operation, as the hardware requires.
   - `cache-off-masked`: the flash cache is off during the operation;
     the flash driver masks interrupts for that one operation (handlers
     outside internal RAM cannot run).

   Either way DAGON issues one erase unit of the region it erases in
   (`[execution] erase_regions`) per operation, and the time it may take
   is that region's datasheet maximum `unit_max_ms`. The agent plans every
   erase from the definition's regions and `[[slot_layout]]`, and the
   slot store performs exactly the planned operations; the host waits for
   the same plan ([erase and trial waits](/en/reference/definitions/#erase-and-trial-waits)).
4. **The watchdog outlasts one erase.** Twice each region's `unit_max_ms`
   is at most `[watchdog] trial_ms` and `confirmed_ms` (checked when a
   definition is loaded); every request starts with a gated feed through
   the serving loop's `ServeHost::feed` (the RP2040 relay's PC loop through
   the guard's `progress`). An erase of several units feeds, or reports
   progress, between them. A pending image's trial
   is a window of its own: the guard's trial policy decides whether
   anything is fed while it lasts (`app.watchdog_taken` reports it).
5. **Links during a stall.** A request is handled with the link closed
   for new ones (an I2C target NACKs its own address; a stream link's
   peer waits for the reply). What arrives while the CPU is stalled is
   lost or held by hardware; the requester retries with the same
   sequence number, and the node answers a repeated request with the
   reply it already made.
6. **A reset only after the reply left.** A reboot waits until its reply
   was written and flushed, then for the delay `allow_reboot_now` asked
   for with the serving task idle (so the guard keeps feeding); a reply
   that was not sent resets nothing, and the requester's retry gets the
   same reply.
7. **Facts say how far they are established.** A definition's hardware
   fact is a number checked against its source (the definition's comment
   cites the document and table), `{ unverified = N }` (a value not
   checked against its source) or `"unmeasured"` (no value). Tools and
   documents report the last two as not established; a check that needs a
   value uses an unverified one; none is relied on in a release.

### 11.2 Per MCU

The tables are what the MCU definitions do; the budgets are each MCU
definition's.

**STM32F401RE** (`flash_op = "fetch-stall"`, erase unit one sector: 16, 64 or 128 KiB by region)

| Work | Runs in | Priority |
|---|---|---|
| Serving the I2C target link (the agent, the conformance session), flash erase and program | DAGON's interrupt executor (the USART6 vector) | P6, above thread mode |
| The link's I2C and DMA transfers | the I2C driver's interrupts | the HAL's default |
| Timers (`embassy-time`) | its timer interrupt | the HAL's default |
| Watchdog feeding | the app's feeder task, gated by the guard | thread mode |

During an erase nothing executes until the sector is done; the IWDG counts
from the request's feed. The own
I2C address is NACKed for the whole request, so the parent's polls are
NACKed by the peripheral without the CPU, and the parent retries until
its own deadline.

**RP2040** (`flash_op = "xip-off-masked"`, erase unit one 4 KiB sector unless the board's `[flash]` says otherwise)

| Work | Runs in | Priority |
|---|---|---|
| The PC link and the agent; the sub-node links (relay) | DAGON's interrupt executor (SWI_IRQ_0) | P1 |
| The USB device | the USB interrupt | the HAL's default |
| Flash erase and program | the PC task, through the slot store's `SharedFlash` handle | one sector or page per operation, masked by the driver |
| Watchdog feeding | the feeder task, gated by the guard | thread mode |

Each flash operation masks interrupts for one sector erase or one page program; the slot store reports progress to the
guard between sectors. The USB controller NAKs the host meanwhile and
the host retries.

**ESP32-S3** (`flash_op = "cache-off-masked"`, erase unit one 4 KiB sector unless the board's `[flash]` says otherwise)

| Work | Runs in | Priority |
|---|---|---|
| The UART and USB Serial/JTAG uplinks, the agent, flash erase and program | DAGON's interrupt executor (FROM_CPU_INTR1) | Priority2, above thread mode |
| Watchdog feeding (once confirmed, gated by liveness) | the link task | Priority2 |
| The app's tasks | thread mode | lowest |

Each flash operation masks interrupts for one erase unit; the slot store feeds the watchdog between units. The UART receive
FIFO keeps what fits; a frame cut by the stall is dropped by the 20 ms
gap rule and the requester retries. USB Serial/JTAG NAKs the host
meanwhile.

### 11.3 How the budgets are set

For each MCU, the budgets are set from these values on the boards: the longest critical section
of DAGON's own code (`masked_max_us`: the worst value plus a margin),
the worst-case erase time of the board's flash part (and its datasheet
value, cited in the definition), the watchdog margin left at the end of
one erase, and the link's recovery after it (the parent's retry
answered).
