# Lifecycle events and extensions: one model for boot core, agent, relay and host

This document is the design of DAGON's lifecycle-event and extension
model: how every state transition in every layer is an observable
event; how board packages, libraries and applications extend DAGON in
every layer, the boot stage included, by observing events, answering
the few decision points and replacing parts of DAGON through ports; how
the MCU's protection hardware keeps extensions from touching what makes
an update safe; how board values and boot-time actions come from
definitions; and how the model is versioned. It is the reference for
the implementation and for anyone extending DAGON.

The goal: DAGON and user code fully separated. A user (a board package,
a library, an application) meets every need through DAGON's public API,
events, extension ports and definitions; nothing in DAGON is edited,
copied or forked; DAGON holds no knowledge of any sample, board or
peripheral. The model is written for needs that are not known in advance
(an automotive diagnostic transport, added security processing, a
display, a different storage), not for a fixed list.

Terms: *boot core* is the image DAGON runs before the app (stage0 and
the boot core on a self-boot MCU; the vendor 2nd stage on a
vendor-stage2 MCU); *agent* is `dagon-agent` inside the app; *relay* is
the hub's PC link and sub-node links; *host* is `dagon-host` driven by
the CLI and `dagon serve`; an *extension* is code or data outside DAGON
that DAGON runs at one of its extension points. 親機/子機 are hub/node.

## 1. State machines and their events

Every state change in DAGON happens through a transition of a declared
state machine, and every transition emits its event. The machines are
declared once each with `state_machine!` (1.7). The normative catalog is
the generated one: `docs/events.md`, produced from the machines'
`CATALOG` constants (kind, payload, attributes, observer path) and
checked against them in CI. The event tables in this section are
illustrative examples of what the machines produce; they carry no entry
the generated catalog does not, and when the two differ the generated
catalog is right. 1.8 shows, with the same caveat, how each class of
event is observed.

### 1.1 Image lifecycle (per slot, persistent)

The state of an image slot persists across resets in the slot store
(kind S: the state log; kind V: the vendor selection data such as
otadata). It is one machine with transitions in two layers: the agent
changes it over the wire, the boot core changes it at boot. No public
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
| `erase` | app | XFER_BEGIN passed every guard and `accept_update`; from `Empty`, `Written`, `Pending` or `Reverted` (never from the active or the confirmed slot). From `Pending` because the protocol lets a new image replace one set pending but not yet started; the erased header no longer matches the SLOT_PENDING record, so the slot reads empty | slot body and header place erased | `app.transfer_started` |
| `written` | app | XFER_END: body digest equals the header's, header written and re-verified | header in place | `app.written { slot, version }` |
| `pending_set` | app | SET_PENDING: header present, signature and compatibility, body digest; never the running slot; from `Written`, `Reverted`, `Pending` (a repeated SET_PENDING appends again) or `Confirmed` (the confirmed slot set pending again while the other slot is on trial, which switches back to it) | SLOT_PENDING record / vendor selection | `app.pending_set { slot, version }` |
| `pending_moved` | app | SET_PENDING on the other slot while this one is `Pending` or `Trial`, and this slot is not the confirmed one | the same SLOT_PENDING record (the log names one pending slot) | `app.pending_moved { from, to }` |
| `pending_withdrawn` | app | this slot is the confirmed one, set pending again, and another slot is set pending or its trial is rolled back: it reads `Confirmed` again, as the log's `confirmed` still names it | the same SLOT_PENDING or ROLLBACK record | `app.pending_withdrawn { slot, version }` |
| `attempt` | boot | pending slot checked (signature, digest), `attempts < max` | BOOT_ATTEMPT record / vendor "new" → "pending verify" | `boot.attempt_recorded { slot, attempt, max }` |
| `confirmed` | app | CONFIRM while on trial, `self_check` healthy | CONFIRMED record / vendor "valid" | `app.confirmed { slot, version }` |
| `demoted` | app | the other slot's `confirmed` (the log's `confirmed` now names the other slot) | the same CONFIRMED record | `app.demoted { slot, version }` |
| `rolled_back` | boot | attempts exhausted, or the pending image fails its check | ROLLBACK record / vendor abort | `boot.rollback { from, to, reason }` |
| `chosen` | boot | the slot to start is final (confirmed, or trial) | none | `boot.slot_chosen { slot, trial, version }` |
| `no_image` | boot | no startable slot | none | `boot.no_image { reason }` |

The extra source states of `erase` and `pending_set`, and
`pending_withdrawn`, exist because the protocol already allows those
requests; the machine follows the wire behaviour rather than narrowing
it.

One persisted record can move two slots (`confirmed` and `demoted`,
`pending_set` and `pending_moved` or `pending_withdrawn`): the machine
holds one state per slot, one trigger runs both transitions and both
events are emitted, in that order. A test folds every sequence of what
the agent and the boot core write (transfers, switches, confirms, boot
attempts, rollbacks) from a provisioned device and checks after each
step that every slot's machine state equals STATUS as folded from the
log (`LogState`), so no fold outcome exists without its transition.

The machine wraps the slot store port: `ImageStore<S: SlotStore>` holds
the store, and the port's mutators (`erase`, `write`, `write_header`,
`set_pending`, `confirm`) take a `Transition` that only the machine
makes, inside the transition that emits the change's event. The
`SlotStore` trait remains the port a board implements. Code that holds
the store for the board's other resources (`Agent::store()`) cannot
change image state with it. The agent exposes `images() -> &ImageView`
(read-only: status, each slot's lifecycle state, own version, on
trial). An app that confirms itself at start
(bench use) calls `confirm_own()`, which is the `confirmed` transition:
the `self_check` decision first, the port's confirm, the events always.
The boot-layer transitions are replayed by the app at start (1.2, 5.5)
so app-stage observers see them too (`app.trial_started`,
`app.rolled_back`).

### 1.2 Boot sequence

Self-boot MCUs (stage0 and boot core): `Entry → HalReady →
WatchdogArmed → LogRead → BeforeDecision → Planned → Committed →
Jumping | Halted`. `plan` is pure (reads flash, computes the decision
and the records to append); `commit` performs the image-lifecycle
transitions `attempt`, `rolled_back`, `chosen` or `no_image`. Stage0
has only `Entry`, `CoreChosen` and `Halted`: it picks the boot core slot
from the boot core log and verifies that image under the one
authentication rule of 4.7 (header signature with the project key, body
digest) before `core_slot_chosen`; it runs no extension.
The watchdog rule is in 4.1.

Boot-stage extensions (2) and the boot action program (5.2) run only
from `WatchdogArmed` on: every event before it is recorded and replayed
but has no live observer, so nothing outside DAGON runs before the
watchdog protects the boot. Every boot event is also appended to the
boot record (5.5), retained memory that costs no flash write, and the
app's agent replays them at start as the same `boot.*` events with
`replayed: true`. The record is never an input to `plan`, `commit`,
`self_check` or `confirm`.

| Event | When | Payload | Live at boot |
|---|---|---|---|
| `boot.entry` | first instruction of DAGON code after reset, HAL not initialised | `stage`, `reset_cause` | no (record only) |
| `boot.hal_ready` | after HAL/clock init | `stage` | no |
| `boot.watchdog_armed` | watchdog started (4.1); from here extensions run | `period_ms` | yes |
| `boot.log_read` | the state log read | `ok`, `pending`, `confirmed`, `attempts` | yes |
| `boot.before_decision` | `plan` is about to run; the last event before the decision | `pending`, `confirmed`, `attempts` | yes |
| `boot.decision_planned` | the decision is known, nothing written yet | `Decision` | yes |
| `boot.core_slot_chosen` | stage0 picked the boot core slot | `slot`, `version` | no |
| `boot.slot_chosen` | image lifecycle `chosen` | `slot`, `trial`, `version`, `entry` | yes |
| `boot.attempt_recorded`, `boot.rollback`, `boot.no_image` | image lifecycle (1.1) | | yes |
| `boot.before_jump` | last event before control leaves DAGON | `slot`, `trial` | yes |
| `boot.recovery` | a recovery path is entered instead of an app | `path` | yes |
| `boot.halt` | the core halts for good | `code` | yes (boot core) |
| `boot.extension_ran` / `boot.extension_faulted` / `boot.extension_overran` | an extension's handler returned / faulted (protection, bus, instruction, bad service) / exceeded its time | `id`, `us` / `id`, `fault`, `pc?`, `addr?` / `id`, `budget_us` | yes |
| `boot.extension_disabled` / `boot.extension_irq_storm` | an extension disabled for the rest of the boot (fault limit, or the lockup fuse from the previous boot) / a declared peripheral's interrupt source disabled | `id`, `why` / `id`, `peripheral` | yes |
| `boot.program_step` / `boot.program_skipped` | one boot action ran / the program was not run | `index`, `op`, `ok` / `why` | yes |
| `boot.step` | diag step reached | `n` | no |

Vendor-stage2 MCUs have no DAGON boot stage: the app emits
`boot.slot_chosen`, `boot.rollback` and `boot.recovery` with `replayed:
true` from the selection data at start, and boot-stage extensions do not
exist there (4.5).

### 1.3 Agent session

States `Idle`, `Header { slot, body_len, got }`, `Body { target, slot,
len, next }`; every protocol request is a transition (self-transitions
for HELLO, STATUS, VERSION and progress). The protocol itself is a port
(2.1): the machine's triggers are protocol-neutral (`update_offered`,
`header_chunk`, `data_chunk`, `transfer_end`, `switch`, `reboot`,
`confirm`, `status`), and a protocol plug-in (DAGON wire, UDS, …) maps
its messages onto them.

Generated order at start: `app.starting` → `app.store_opened` (or
`app.store_failed`) → the boot replay (`boot.*` with `replayed: true`) →
`app.boot_replayed` (or `app.boot_record_missing`) → `app.trial_started`
/ `app.rolled_back` → `app.watchdog_taken` → `app.bring_up` →
`app.link_up` …

| Event | When | Payload |
|---|---|---|
| `app.starting` | DAGON's start function entered | `tag`, `role`, `mode` |
| `app.store_opened` / `app.store_failed` | state log or selection data read / unreadable | `active`, `trial`, `version`, `attempts`, `last_reason` / `error` |
| `app.boot_replayed` / `app.boot_record_missing` | the boot record's valid events replayed (one summary per boot found, oldest first) / no valid event at all (5.5) | boot identity, `writers`, `entries`, `invalid_entries` / `why` |
| `app.trial_started` / `app.rolled_back` | replayed from the state log | `slot`, `version`, `attempt`, `max` / `from`, `to`, `reason` |
| `app.watchdog_taken` | the boot core's watchdog taken over | `period_ms`, `trial_policy` |
| `app.bring_up` | board bring-up runs | `board_id` |
| `app.extension_started` / `app.extension_faulted` / `app.extension_overran` | an extension's handler ran / faulted / overran (4.2) | `id`, `point` / `id`, `point` / `id`, `point`, `us` |
| `app.link_up` / `app.link_down` | a DAGON link opened / closed | `link`, `addr` |
| `app.request` / `app.replied` | a request arrived (any protocol) / its reply left | `protocol`, `cmd`, `seq`, `via` / `status`, `ms` |
| `app.hello` / `app.status_asked` / `app.version_asked` | identity / status / VERSION answered | `via` |
| `app.update_offered` | an update request passed every DAGON guard; **decision** `accept_update` | `target`, `slot`, `body_len` |
| `app.update_refused` | refused | `status`, `by: Dagon \| Extension` |
| `app.header_verified` / `app.header_rejected` | header signature and compatibility (through the verifier port) | `slot`, `version`, `key_id` / `status` |
| `app.transfer_started`, `app.transfer_progress`, `app.transfer_aborted` | erase done / each chunk written / idle timeout, store error, new request | `target`, `slot`, `len` / `done`, `total` / `cause` |
| `app.verify_started`, `app.verify_ok`, `app.verify_failed` | body hashing and read-back | `len` / `slot`, `version` / `status` |
| `app.written`, `app.pending_set`, `app.pending_moved`, `app.pending_refused`, `app.confirmed`, `app.demoted`, `app.confirm_refused` | image lifecycle (1.1) | |
| `app.reboot_requested` | reboot accepted; **decision** `allow_reboot_now` | `mode` |
| `app.before_reboot` | reply has left; reset follows | |
| `app.self_check` | confirm while on trial; **decision** `self_check` | `slot`, `version` |
| `app.rom_entry` | recovery entry accepted | `method` |
| `app.conformance` | conformance test served | `test` |
| `app.liveness_overdue` | the guard stopped feeding because a task is overdue | `task` |

### 1.4 Relay

The relay is also an app (its own agent emits `app.*`). Its own machine:
PC link up/down, a quiet window after a relayed request or a display
request, per-child `Unknown → Present | Absent` from polls and relayed
replies, and a progress screen. Child links are ports (a child protocol
plug-in may relay UDS as well as DAGON frames).

| Event | When | Payload |
|---|---|---|
| `relay.pc_up` / `relay.pc_down` / `relay.pc_quiet` | PC link opened / closed / quiet window ended | |
| `relay.relay_request` | a request to relay received | `link`, `addr`, `cmd` |
| `relay.child_seen` | a relayed identity/status reply parsed | `index`, `link`, `addr`, `Seen` |
| `relay.child_found` / `relay.child_lost` / `relay.child_polled` | poll answered after absence / first miss / any poll | `index`, `link`, `addr`, `Seen` |
| `relay.child_update_progress` / `relay.child_update_result` | relayed transfer reply / relayed switch or confirm reply, or a display request with a final phase | `index`, `done`, `total` / `index`, `outcome` |
| `relay.children_set` | the sub-node list loaded (topology copy or the project's fallback) | `Subs` |
| `relay.child_control` | a child's control lines driven / raw bus access | `line`, `level` / `addr`, `len` |
| `relay.show` / `relay.show_expired` | display request received / progress screen dropped | `Progress` / |

### 1.5 Host operations

One machine per operation (`apply`, `rollback`, `recover`, `confirm`,
`provision`, `push_topology`, `build`) with per-node sub-states, plus the
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
| `host.node.result` | | `node`, `Done(v) \| Failed(reason, status?) \| NotStarted` |
| `host.confirm.result` | | `Confirmed \| NotOnTrial \| NeedsChoice` |
| `host.rollback.*`, `host.recover.*` | same shape as deploy | |
| `host.provision.started` / `host.provision.recorded` | first install through the ROM; the installed boot image identity recorded | `node`, `boot_sha256` |
| `host.topology.pushed` | the display copy written to the hub | `sha256` |
| `host.project.opened` / `host.project.closed` | project session | `path` |
| `host.agent.linked` / `host.agent.unlinked` | consent recorded / withdrawn | `agent`, `way` |
| `host.version.read` | VERSION answered or Unsupported; includes the isolation level and the trusted extensions the node reports | `target`, `VersionResp?` |
| `host.extension.loaded` / `host.extension.rejected` | a host extension (data-defined or crate) loaded / refused | `id`, `kind` / `why` |
| `host.job.*`, `host.device.*` | serve job and device messages | |
| `host.action.ran` | a data-defined action finished | `id`, `ok`, `elapsed_ms` |

### 1.6 Persistent state and its machines

Everything that survives a reset or a restart is behind a machine, and
every write to it is atomic in the sense of the last column: a reset or
power loss at any point leaves either the previous state or the new one,
never a half-written one that a reader accepts.

| Persistent state | Where | Machine | Transitions that write it | Atomic how |
|---|---|---|---|---|
| App image slots (headers, bodies) and their selection (state log kind S) | node flash | image lifecycle (1.1) | `erase`, `written`, `pending_set`, `pending_moved`, `attempt`, `confirmed`, `demoted`, `rolled_back` | body first, header last (a slot has an image only once its header reads back and verifies); the state log is append-only records, each with its own CRC, on two faces that alternate: a torn record fails its CRC and is ignored, the previous record stands |
| Vendor selection (kind V) | node flash | image lifecycle (1.1) | `pending_set`, `attempt`, `confirmed`, `rolled_back` | the vendor's two-sector scheme: a whole entry with sequence and CRC written to the other sector, the newest valid sequence wins |
| Boot core slots and the boot core log | node flash (self-boot) | boot core image lifecycle: `Provisioned → Chosen` | host `provision` (`host.provision.recorded`), stage0 `chosen` (`boot.core_slot_chosen`) | same as the app slots and state log |
| Installed boot image identity (vendor-stage2 MCUs) | provisioning record in the project | host `provision` | `host.provision.recorded` | written to a temporary file and renamed over the old one |
| Boot record (retained memory, 5.5) | node RAM or retained registers | boot record: append-only, no state beyond the recorded events | every `boot.*` transition appends one event | required property: a write interrupted at any byte loses at most that event and never makes another one readable as valid; the format that meets it is defined by the executable model (5.5) |
| Lockup fuse (retained memory, 5.5) | node RAM or retained registers | fuse: `Clear → Armed → Clear` (a call), `Armed → Tripped → Clear` (a lockup, then a clear) | armed and cleared around every sandboxed call; tripped by the next start | required property: no reset at any point forges a fuse for a call that returned or loses one for a call that locked up; the format that meets it is defined by the executable model (5.5) |
| Topology display copy | hub flash | topology copy: `Absent → Written → Written` (replacement) | `written` with target TOPOLOGY (`app.written`, `host.topology.pushed`) | required property: a replacement interrupted at any byte leaves the hub reading the previous copy or the new one, never a mixture; the `topology-bin` format and write order that meet it are defined by the executable model (5.5) |
| Agent consent | `~/.dagon/agent.json` | agent link: `Unlinked → Linked → Unlinked` | `host.agent.linked` / `host.agent.unlinked` | temporary file and rename |
| Project session (open project, release key) | serve process | project: `Closed → Open` | `host.project.opened` / `host.project.closed` | process memory under one lock |
| Diag step, extension timing, quiet windows, seen tables | RAM only, process-lifetime | not persistent; events only | | |

No persistent structure in this model has a header, counter or state
field that is rewritten in place; every durable write is a new record
that commits as a unit, or a whole file replaced by rename. The
formats this model introduces for retained or persistent data (the boot
record, the lockup fuse, the compiled topology with MCU identity) are
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
`retired` flags); and a test per machine that drives every transition
from every state and replays its record. Per crate it defines the
`MACHINES` static, which the catalog registry (`dagon-catalog`) lists;
a test there fails while a crate that depends on `dagon-events` is
missing. A state no transition reaches, a transition without `emits` or
an `observe` path, a duplicate code, an oversized payload, a second
`state_machine!` in a crate, or a machine outside it are compile
errors. `dagon events catalog` prints the catalogs; `docs/events.md` is
generated from them and a test keeps the two equal.

Decision points (3) are never `emits` clauses: a new one is a design
change written here first, with its bound, default, answering parties
and safety argument, and is a separate trait method.

Events are emitted by DAGON only. Extensions observe them and never
construct them.

### 1.8 Who observes what

Every event has at least one observer path, and the path is an attribute
of the generated catalog, so the generated `docs/events.md` carries this
table per event; a generated test fails on a kind with no path. *Live*
is an extension in the same image (2); *replay* is the app's agent
re-emitting it at start from the boot record (5.5) or the state log;
*host* is the CLI, the serve stream and actions (2.5), which also see
every device-side event that a reply or VERSION carries. Replay
delivers only kinds the catalog marks `replay` and not `retired`; any
other record is refused, not guessed.

| Events | Layer | Live | Replay | Host |
|---|---|---|---|---|
| `boot.entry`, `hal_ready`, `core_slot_chosen`, `step` | boot | no (before the watchdog, or stage0) | every intact recorded event → `boot.*` `replayed: true`, at least once, with an identity | VERSION record TLVs |
| `boot.watchdog_armed` and every later boot event, including `halt` and the extension/program events | boot | yes: boot-stage extensions (2), in the sandbox of 4 | the same | VERSION |
| `boot.halt`, and the events of a boot that hung before `before_jump` or faulted in an extension | boot | up to the point of the halt or hang | the events stay in the record; the first app that runs replays every boot still present, oldest first | VERSION (readable until overwritten) |
| `boot.slot_chosen`, `attempt_recorded`, `rollback`, `no_image` | boot | yes | boot record, and the state log (`app.trial_started`, `app.rolled_back`) even when the record is missing | VERSION, STATUS |
| `app.*` | app | yes | `boot_replayed`, `trial_started`, `rolled_back` are themselves replays | replies, relay `child_*` |
| `relay.*` | relay | yes (the hub app's chain) | | relayed replies, display results |
| `host.*` | host | `on_event` callback (CLI, serve), host extensions | | stream, `--events`, actions |

## 2. The extension model

One shape in every layer: **ports** DAGON exposes for replacement,
**plug-ins** that provide a port, observe events and answer decisions,
**events** with no-op defaults, and **one composition order** (board
package → libraries → user). The execution context differs (an
unprivileged sandbox in the boot core and, where the MCU allows it, in
the app; embassy tasks in apps; threads and JSON on the host), the shape
does not. A declarative action program (5.2) is the simplest plug-in
kind, for uses that need no code.

### 2.1 Ports

A port is a trait DAGON calls for one replaceable part, with DAGON's own
implementation as the default. A plug-in that provides a port replaces
that part for the image it is linked into. Ports exist in every layer.

The port catalog is one table in code, `dagon_events::PORTS`, one
record per port (`id`, the set of layers it exists in, the default per
layer, the trust policy, the services it needs, a description),
from which this table, the generated `docs/ports.md`, the manifest
validator (`provides` must name a port of the extension's layers), the
service tables of 4.2 and the `dagon status` port columns derive; a
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
| `SlotStore` (where images live) | boot, app | the MCU package's flash layout | trusted only | – |
| `LogStorage` (where the state log lives) | boot, app | the MCU package's flash layout | trusted only | – |
| `Recovery` (an additional recovery path: a service window for a transport, a vendor download mode) | boot, app | BOOTSEL/UF2, factory, ROM entry as the MCU and board define | untrusted, additive (a plug-in adds a path, never removes DAGON's) | return, transport, time, memory |
| `Diagnostics` (extra VERSION TLVs, extra status, a UDS DID table) | boot, app, relay | none | untrusted | return, time, memory |
| `Display` (what a phase or a boot outcome looks like) | boot, app, relay | boot: the `[boot] led` and `halt` of the board definition through the action program; app, relay: none | untrusted (at boot it runs in the sandbox with the declared pins and buses) | return, pins, buses, time, memory |
| `LogSink` (where `Recent` overflow and extension logs go) | app, relay, host | none | untrusted | return, events, time, memory |
| `SelfCheck` (the health answer when a slot on trial is confirmed) | app | healthy | untrusted (a decision point, 3) | return, time, memory |
| `BoardBringUp` (the board's power, pins and buses at app start) | app, relay | the board package's | untrusted | return, pins, buses, time, memory |
| `ChildLink` (how a hub reaches a child) | relay | I2C controller, PIO UART | untrusted | return, buses, transport, dma, time, memory |
| `Signer` (signing images and releases) | host | file key pair | untrusted (the host is trusted by definition; the device verifies) | – |
| `KeyStore` (where the signing key is kept) | host | file key pair | untrusted (the host is trusted by definition; the device verifies) | – |
| `BuildRunner` (building node images) | host | cargo | untrusted | – |
| `Agent` (an external agent the CLI and the page hand requests to) | host | data-defined (agent definitions) | untrusted | – |
| `Action` (a program or request run after a host event) | host | data-defined (action definitions) | untrusted | – |
<!-- end of generated table -->

A port's default is DAGON code; a trusted-only port's replacement runs
privileged and is accepted only under 4.7. A manifest's `provides` must
name ports whose layer sets include every layer in the manifest's
`layers` (the UDS example of 8.4 declares `UpdateTransport`,
`UpdateProtocol` and `Diagnostics` for `boot` and `app`, which the
catalog allows). Every other port's replacement
runs in the layer's sandbox (4) with the capabilities its manifest
declares.

### 2.2 Plug-ins and the manifest

A plug-in is a Rust crate (or, for the simplest kind, an action program
in a definition) with a manifest, `dagon-extension.toml`:

```toml
format = 1
id = "uds-transport"                 # a-z 0-9 -, up to 32
layers = ["boot", "app"]             # where it may be linked
provides = ["UpdateTransport", "UpdateProtocol"]   # ports it replaces (may be empty: observer only)
observes = ["boot.*", "app.update_*", "app.transfer_*"]   # event kinds, validated against CATALOG
decides = ["accept_update"]          # decision points it answers (may be empty)
trusted = false                      # 4.4
[capabilities]                       # what DAGON hands it, nothing else (4.2)
peripherals = ["can0"]               # declared in the board definition's peripheral table
pins = []
buses = []
timer = true
[limits]
max_run_us = 2000                    # per handler call at boot; per call in the app
stack_bytes = 1024
ram_bytes = 4096
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
DAGON's memory in `Ctx`, and in the sandboxed layers the hardware denies
them whatever the code does (4).

The extension interface is the trust boundary: an extension receives
only `&Event`, `&Ctx` and the decision arguments, none of which leads to
a chain, to published state or to replay. Composing a chain, delivering
into it and replaying are layer-start APIs. Code built into the same
image outside the extension interface (DAGON's crates, the application's
own start-up code) is in the trusted base, as for anything in one
address space; isolating untrusted extensions is the sandbox's part (4).

### 2.3 Composition

A typed chain, order fixed at the type level and therefore at build
time:

```rust
pub struct Chain<A, B> { /* private */ }   // Chain::new(a, b)
impl<A: Extension, B: Extension> Extension for Chain<A, B> { /* on_event: both; decisions: each in turn, first Some wins */ }
pub struct Nop;  // no extension
#[macro_export] macro_rules! extensions { ($a:expr $(, $b:expr)*) => { … nested Chain::new … } }
```

Order: **publisher → board package → libraries → user**, always, in
every layer. The layer's start (the boot core's generated `main`, the
MCU package's `start`, the host's `Host::new`) builds the chain itself
with `compose` (layers with published state and decision points: app,
relay) or `compose_unpublished` (boot core, host): the publisher for the
`Published` state (2.4), then the board package's chain, then the
user's `extensions![..]` (libraries first, then the app's own). The
result is an opaque `Composed`: its parts cannot be reached or
reordered, it is not an extension, and only the layer start holds it.
User code cannot put itself first, and `extensions![..]` never contains
the publisher. Notifications reach every extension;
decisions stop at the first non-abstaining answer. A port can be
provided by at most one plug-in per image: `dagon gen` refuses a project
whose chain provides a port twice (a library and the user both bringing
an `UpdateProtocol`), naming both.

Registration per layer:

- **Boot core (self-boot MCUs).** The project's `dagon.toml` lists the
  extensions for the boot core (`[boot] extensions = ["uds-transport",
  …]`); `dagon gen` writes `boot_wiring.rs` with the chain type and
  links the crates into the boot core image in their own sections
  (`.ext.text`, `.ext.data`, `.ext.bss`) that the protection map of 4.4
  isolates. The chain is `const`-constructed (no allocation); handlers
  are called synchronously from the boot machine through the entry gate (4.1), each call under its `max_run_us`. The signed boot core
  image contains the extensions, so an update of an extension is an
  update of the boot core through the ROM path like any boot core
  change.
- **App (embassy).** `node::start` / `board::start` take `exts: impl
  Extension + 'static`. The MCU package creates and keeps the layer's
  `Published`, composes it, and returns a `Reader` to the app. The
  agent's machines deliver every event into the chain, and the chain
  asks the decision methods at the three points. Where the MCU's isolation
  level allows it (4.5), the serving task runs the chain's handlers
  unprivileged under the same sandbox as at boot; otherwise they run as
  ordinary code with the time limit measured and reported.
- **Relay.** The relay's own agent is an app with the same chain; the
  relay machine's events and the `ChildLink` port go through it.
- **Host.** `dagon-host` takes the chain as a `Box<dyn Extension>`
  built by the CLI from the project's `dagon.toml` (`[host] extensions`):
  data-defined kinds (agent definitions, action definitions, transport
  definitions such as a serial or CAN adapter named by program or
  device) and Rust crates implementing `dagon_host::Transport`,
  `Signer` or `BuildRunner`, loaded as workspace dependencies of the
  CLI build. The host runs with the user's permissions; its sandbox is
  the operating system's.

The app's agent takes the composed chain and nothing else:
`Agent::new(config, store, chain)`, where `chain` is `compose`'s result.
There is no separate hooks trait; observing events and answering
`accept_update`, `allow_reboot_now` and `self_check` are `Extension`
methods. The MCU packages' `start` still accepts the app's
`fn() -> bool` self-check and wraps it as a one-method extension
(`SelfCheck`) in the user part of the chain.

### 2.4 Waitable published state

Async display and log tasks never run inside a handler. The publisher at
the head of the chain copies each event into:

- `Latest`: an `embassy_sync::watch::Watch` (multi-receiver, last
  value) holding `Latest { state, last }`: the layer's snapshot, folded
  from every event (the app's `{ phase, progress, version, trial }`, the
  relay's children), and the last event's record.
- `Recent<N>`: a fixed ring of the last N events as `EventRecord`, an
  owned, bounded form generated per event by `state_machine!`:
  `{ layer: u8, kind: u8, flags: u8, dropped: u8, seq: u32, payload:
  [u8; 16] }`. Fields are copied in their compact form the catalog
  states (a length, an id, a digest prefix). No reference is retained
  after the producer returns. Overflow drops the oldest; the records
  after it report the loss in `dropped`, up to 255 each, carrying the
  rest, and the producer never waits. `seq` wraps and compares by serial
  arithmetic.

Both live in one `Published<S, N>` that the layer package (the MCU
package, the host) creates, keeps and hands to `compose` by exclusive
reference, so the chain is its only writer. Display, log and app tasks
get a `Reader` (receivers, the latest state, the recent ring), which
only reads. The relay's display state is the same type with the relay's
snapshot. The boot core has no publisher: its events go to the boot
record and to its extensions' handlers.

### 2.5 Host: event stream, actions, package discovery

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

Actions are the host's data-defined plug-in kind, like agent
definitions: a TOML file naming what to run and when, over transport
kinds DAGON implements. The command transport (environment allow-list,
fresh working folder, process group, time and output limits) is one
shared module of the CLI that agent definitions and actions both use.

```toml
# ~/.dagon/actions/notify.toml  (never a project folder: it names a program)
format = 1
id = "notify-on-result"
on = ["host.node.result", "host.deploy.finished"]   # validated against CATALOG
when = { outcome = "failed" }                        # optional: equality on payload fields
[run]
kind = "command"
program = "notify-send"
args = ["DAGON", "{payload.node} {payload.outcome}"]
timeout_s = 30
```

`kind = "http"` posts the event JSON to a URL (https only). Actions run
after the event is emitted, never inside the operation; their outcome is
`host.action.ran` (an action cannot subscribe to `host.action.*`).
Actions never answer a decision: `confirm_plan` is answered by the TTY,
the page or `--yes`. A command action runs with the user's permissions,
as a command agent does.

Where serve finds packages: `dagon.toml [serve] packages` is a list of
globs relative to the project (`["nodes/*"]` in a generated project);
only directories matched there and holding a `Cargo.toml` are listed and
built. DAGON has no built-in search path.

## 3. Decision points

Everything not in this table is a notification. There is no decision
point in the boot core: nothing outside DAGON influences what the boot
core starts, except a trusted plug-in that provides `DecisionPolicy`
(4.7), which is then that part of DAGON.

| # | Decision | Layer | Who may answer (first non-abstaining in order) | Bound | Default | Never overridable |
|---|---|---|---|---|---|---|
| 3.1 | `accept_update(&UpdateOffer) -> Accept` | app | board, libraries, user | 50 ms; after every DAGON guard passed, before erase | accept | cannot accept what DAGON refused (not called then); refusal answers the protocol's "state" status |
| 3.2 | `allow_reboot_now(&RebootRequest) -> Delay` | app | board, libraries, user | `Now` or `After(ms)`, `ms ≤ REBOOT_DELAY_MAX`, a build-time constant derived from the MCU definition's watchdog periods (`[watchdog] trial_ms`, `confirmed_ms`): half the shorter one. DAGON replies first, keeps feeding under the guard's policy while it waits, then resets; a watchdog reset during the wait is the same reset | now | a reboot cannot be refused; the delay is capped |
| 3.3 | `self_check(&TrialInfo) -> Health` | app | board, libraries, user | 50 ms; `Pending` keeps the "state" status and lets the host retry | healthy | cannot confirm a slot not on trial, cannot bypass the `confirmed` transition |
| 3.4 | `confirm_plan(&Prepared) -> bool` | host | the interactive user (TTY, page) or `--yes`; actions and host extensions cannot | until the user answers | ask | key check and image verification happen before the plan is shown |

Bounds are measured: the composed chain times each extension's call on
its own with the layer's clock (DAGON's, from the MCU package; not a
port); a late answer is ignored, as an abstention, and the chain emits
`app.extension_overran { id, point, us }` (`id` is the extension's id,
its 64-bit FNV-1a hash in the record); in a sandboxed layer the timer
preempts the call at the bound (4.3).

## 4. Isolation and safety contract

Safety is enforced by hardware where the MCU allows it, not by trust.
An untrusted extension runs unprivileged in a sandbox whose only way
out is a privileged service call that DAGON validates on every use; the
MCU's protection unit gives it its own code, data and stack and the
capability windows its manifest declares, and nothing else: not DAGON's
code, not DAGON's memory, not the flash controller, the state and log
storage, the watchdog, any bus master, any clock, reset or interrupt
routing block. DAGON enforces its time limit with a timer that preempts
it and unwinds the call; DAGON continues booting (or serving) when it
overruns or faults. Nothing an untrusted extension does changes the boot
decision, the signature, rollback or watchdog rules, or the confirm
rule.

The model is written for MCUs DAGON does not support yet. Everything
MCU-specific in it is **data in the MCU definition**: the protection
hardware kind and its parameters, the bus masters and privileged
control blocks with their address ranges, the fault and unwinding
mechanism, the watchdog, the retained memory. DAGON has one generator
and one checker per protection-hardware *kind* (4.4), not per MCU.
Adding an MCU of a known kind is a definition; a new kind is one new
generator module behind a trait. The tables for the three MCUs in 4.5
are examples generated from their definitions, not the normative
content; the normative content is the definition schema and the kind's
rules.

### 4.1 The sandbox

| Element | Rule |
|---|---|
| Privilege | Extension handlers run in the architecture's unprivileged mode on their own stack (Cortex-M: Thread mode with `CONTROL.nPRIV = 1`, `SPSEL = 1`; RISC-V: U-mode; Xtensa with PMS: World 1; ARMv8-M with TrustZone: the Non-secure state). DAGON runs privileged (or Secure). Unprivileged code cannot mask interrupts, change its privilege, write the stack pointers of the privileged mode, the protection unit, the interrupt controller or the system control block (architectural in every supported kind). It cannot execute or read DAGON's code or data (4.4). |
| Entry | DAGON calls a handler through the entry gate: it records a `CallFrame` in privileged RAM (the callee-saved registers, the privileged stack pointer, the continuation address, the extension id, the call's budget), loads the extension's protection map (4.4), sets the unprivileged stack pointer to the top of the extension's stack region, arms the sandbox timer (4.3), and drops to unprivileged mode into the handler's entry with the arguments in the argument registers (a borrowed payload is first copied into the extension's data region by DAGON, so the handler receives pointers into its own memory only). |
| Exit | The only ways out are the service call `RETURN` (4.2), the sandbox timer, or a fault; all three unwind through 4.3 to the continuation in the `CallFrame`, never through the extension's stack. |
| Memory | The protection map (4.4) allows exactly: the extension's `.ext.text` (read, execute), its `.ext.data`/`.ext.bss` (read, write, execute never), its stack (read, write, execute never, with a guard at the bottom), and the capability windows its manifest declares. Everything else faults for unprivileged access: DAGON's code and data, the boot record, the ROM, every bus master, every clock/reset/interrupt-routing/pad-mux block, the flash controller, the state and log storage, the watchdog, the other extensions' memory. |
| Capability windows | A peripheral may be a window only if the MCU definition lists it as window-eligible (`[[peripheral]] window_eligible = true`): a self-contained register block that cannot write memory, cannot route interrupts or clocks, and shares nothing with DAGON (4.6, 11). Everything else is reached through a service (4.2) that DAGON performs on the extension's behalf under the same declaration. |
| Time | A per-call budget from the manifest, capped by the layer; the sandbox timer preempts at the budget (4.3). The watchdog is fed by DAGON only, on its own schedule. |
| Interrupts | Extensions have no interrupt handlers and cannot enable, disable or route interrupts. A declared peripheral's interrupt is served by a privileged, bounded DAGON handler that moves data into a per-extension privileged queue and sets a flag the extension polls through a service (4.2); the sandbox timer has the highest configurable priority, every capability interrupt a lower one, and a source that interrupts more often than its declared rate is disabled by its handler (`extension_irq_storm`). |
| DMA | No bus master's controller, channel, descriptor or chaining register is ever inside a window (11). An extension's I/O that needs DMA goes through the `dma_*` services: DAGON validates the extension's buffer, copies through a privileged bounce buffer, programs the controller itself and reports completion through the queue. On an MCU with caches (4.10) the bounce buffer is in non-cacheable memory or cleaned and invalidated by the service. |
| Repeated faults | A handler that faults or overruns `EXT_FAULT_LIMIT` (3) times in one boot or one app run is disabled for the rest of it (`extension_disabled`); the lockup fuse (4.3) disables an extension that locked the core up in the previous boot. |
| Order of arming | The sandbox exists only from `WatchdogArmed`: the watchdog is armed right after HAL init with the MCU definition's `trial_ms`, before the log is read, before any extension or action runs; after `plan` it is set to `confirmed_ms` when the decision is not a trial, and fed once before the jump. |

### 4.2 The service ABI

A service call is the architecture's supervisor-call instruction with
the service id and the arguments in the argument registers and the
result in the first (Cortex-M: `svc #0`, r0–r3; RISC-V: `ecall`,
a0–a3; Xtensa: `syscall`; ARMv8-M with TrustZone: a call to a Secure
gateway veneer, which is the same ABI with the id as the veneer's
argument). The handler runs privileged and is the only intended entry
into DAGON from an extension; DAGON's own code never uses the
instruction, so a service call from privileged mode is itself a fault.
The ABI exposes no service that reads or writes flash, the state log,
the boot record, the watchdog or DAGON's memory; the privileged
implementation of the services is part of DAGON's trusted computing
base like the rest of DAGON's code, and its bugs are DAGON's bugs.

Validation, on every call, before anything is done:

1. **Caller.** The current extension id comes from the `CallFrame`
   DAGON made at entry, never from the caller; a service call outside a
   handler (no frame) is a fault.
2. **Service id.** Must be in the layer's service table (a generated
   `const` from the port catalog, 2.1); an unknown id is a fault,
   recorded as `extension_faulted { fault: BadService }`.
3. **Capability.** A service that acts on a capability takes a handle
   index; it must be below the count of that extension's declared
   capabilities of the service's kind (from `boot_wiring.rs`), and the
   capability's own limits (a device's writable registers, a pin's
   allowed levels, a peripheral's declared interrupt rate) are checked
   by the service.
4. **Pointers and lengths.** Every pointer must lie inside the calling
   extension's data or stack region, every `(pointer, length)` range
   must stay inside it with overflow-checked arithmetic, alignment must
   match the service, and a length must not exceed the service's
   maximum. DAGON copies in or out through privileged buffers **during
   the call that passed the pointer** and never keeps an extension
   pointer after that call returns; a later call that wants the data
   passes a pointer again and is validated again.
5. **Budget.** A service call runs inside the caller's time budget; a
   service whose non-preemptible part would not finish in the time left
   is refused with `Busy` before it starts.

#### Privileged operations and cancellation

A service is a **privileged operation** with explicit state kept by
DAGON per call in the `CallFrame`'s operation slot (`OpState`: the
service, the capability, the resources it holds, its phase). The
sandbox timer never abandons a privileged operation in the middle of a
critical section: a service runs in two parts.

- **Non-preemptible part**, bounded and declared per service (`np_max_us`
  in the catalog, asserted by a test against the implementation's worst
  case): taking or releasing the bus mutex, programming or stopping a
  peripheral or a DMA channel, masking or clearing an interrupt source,
  copying at most `COPY_MAX` bytes. The timer interrupt is masked only
  for this part, with the kind's critical-section primitive (below), and
  a service whose non-preemptible part would overrun the remaining
  budget is refused before it starts (validation 5), so the budget is
  never exceeded by more than `np_max_us`.
- **Preemptible part**: waiting for a transfer to finish. A service
  never spins with the timer masked; waiting is either returned to the
  extension as `Pending` (the asynchronous services) or done with the
  timer unmasked and the `OpState` recording what must be undone.

When a call ends abnormally (timer or fault) the unwind
of 4.3 runs the **cancellation sequence** on the `OpState` before it
resumes DAGON, in this order, each step bounded; nothing is released
until the hardware the operation touched is idle:

1. stop and disable any DMA channel the operation started, and wait for
   the controller to report the channel idle (bounded by the kind
   record's `dma_stop_max_us`; a controller that does not stop in time
   is recorded `extension_faulted { fault: DmaStuck }` and the channel
   is held in reset until step 3 reinitialises its peripheral);
2. mask and clear the capability's interrupt sources in the interrupt
   controller and in the peripheral;
3. quiesce the peripheral through the port's default (abort the
   transfer, flush FIFOs, return the block to idle; for a stuck DMA,
   reset the peripheral and the channel through the reset block);
4. reclaim the bounce buffers and the privileged queue entries of the
   call (they are owned by DAGON and indexed by the extension, never by
   an extension pointer);
5. release the bus mutex if held, now that no transfer of the
   extension's can touch the bus (the bus is left as the port's default
   left it in step 3, so DAGON's next use starts from a known state);
6. clear the `OpState`.

The same sequence runs at the end of every normal call for anything
the extension left pending (a started DMA it never collected), so a
call always ends with no resource of the extension's in flight. An
operation that spans calls (asynchronous I/O) therefore spans only
**within one handler call**: `DMA_RX` started in one handler call is
collected or cancelled before that call returns.

The non-preemptible part masks the sandbox timer with the kind's
critical-section primitive, recorded in the kind's capability record
(`critical_section`, 4.4): on ARMv6-M `PRIMASK` (all interrupts, so the
part's bound is also the interrupt-latency bound of the layer and
`np_max_us` is small); on ARMv7-M and ARMv8-M `BASEPRI` set to a
priority threshold that masks the timer and every capability interrupt
while leaving DAGON's faults enabled; on RISC-V `mstatus.MIE` cleared (or the timer's
bit in `mie`); on Xtensa the interrupt level raised above the timer's.
The bound `np_max_us` of each service is asserted in a test against the
measured worst case of the implementation on that kind.

**Reset-time contract.** A lockup, a watchdog reset or a power loss
ends a call without running the sequence above. Therefore, after every
reset, before anything else uses them, the layer's start reinitialises
every peripheral, DMA channel and interrupt source that any capability
of any extension in its image could have touched: the kind record's
`reset_cleanup` names how (`peripheral-reset` through the reset block
where the MCU has one, else `reinit` through each port's default), and
the list of blocks comes from the image's manifests, so the cleanup is
generated with the protection map and runs at `boot.hal_ready` (boot
core) and before `app.bring_up` (app), ahead of the first service call.
A peripheral DAGON itself uses (its link, its LED) is reinitialised by
DAGON's own bring-up after any reset. The conformance
build triggers a watchdog reset from inside each service's
non-preemptible part and checks that the next boot's first use of the
same peripheral and channel succeeds.

#### Services

The complete list is generated from the port catalog; the kinds are:

| Kind | Services | Non-preemptible part | State in `OpState` | Cancellation |
|---|---|---|---|---|
| Return | `RETURN(result)` | none | none | none |
| Events | `LOG(ptr, len)` | copy ≤ `COPY_MAX` into the sink's buffer | none | none |
| Pins | `GPIO_SET(cap, level)`, `GPIO_GET(cap)` | one register write or read | none | none |
| Buses | `BUS_WRITE(cap, reg, ptr, len)`, `BUS_READ(cap, reg, ptr, len)` | take the mutex, start the transfer; copy ≤ `BUS_MAX` out of or into the extension's buffer **in this call** | mutex held, transfer in flight | steps 2, 3, 5 |
| Transport | `IO_TX(cap, ptr, len)`, `IO_RX(cap, ptr, len) -> n`, `IO_POLL(cap) -> flags` | copy between the extension's buffer and the privileged queue; enable or disable the source | none beyond the queue | step 2 |
| DMA | `DMA_TX_START(cap, ptr, len) -> op`, `DMA_RX_START(cap, len) -> op`, `DMA_STATUS(op) -> Pending \| Done(n) \| Failed`, `DMA_COLLECT(op, ptr, len) -> n`, `DMA_CANCEL(op)` | `START`: copy the extension's buffer into the bounce buffer (TX) and program the channel; `COLLECT`: copy ≤ `n` bytes from the bounce buffer into the **revalidated** extension buffer passed to this call; `CANCEL`: steps 1–4 | channel, bounce buffer, bytes done | steps 1–4; an uncollected RX is discarded |
| Time | `NOW_US()`, `BUDGET_LEFT_US()` | none | none | none |
| Memory | `SCRATCH(len) -> ptr` | none (bump pointer in the extension's own region) | none | the bump pointer resets at the end of the call |

`DMA_RX` therefore never writes into extension memory on completion:
the controller writes DAGON's bounce buffer, the extension learns of
completion through `DMA_STATUS` (or the capability's queue flag), and
`DMA_COLLECT` copies into a buffer it names and DAGON validates at that
moment. No extension pointer outlives the call that passed it.

#### Conformance at the cancellation boundaries

For every service with a non-empty cancellation column, the MCU
package's conformance build runs the call with the timer set to fire at
each boundary (before the mutex, holding the mutex, after the transfer
started, after completion before collect, during collect, after
collect), checks that DAGON's next use of the same bus or channel
succeeds, that no byte lands in extension memory after the call ended,
that the bounce buffer is reclaimed (reuse by the next call sees no old
data), that the capability's interrupt is masked and re-enabled only by
the next call, and that the budget overrun is at most `np_max_us`. The
executable model (5.5) runs the same boundaries in its boot scenarios.

### 4.3 Preemption, faults and unwinding

The sandbox timer (`[isolation] timer`: SysTick on Cortex-M, `mtimecmp`
or a platform timer on RISC-V, a timer-group alarm on Xtensa) is armed
before each call with the call's budget at the highest configurable
priority (above every capability interrupt and every DAGON handler that
is not a fault); a non-preemptible part of a service masks it for at
most `np_max_us` with the kind's critical-section primitive (4.2). The timer handler, the fault handlers and the
`RETURN` service all end a call the same way; the steps are the same
for every architecture and the registers involved come from the kind's
unwinding module (`[isolation] unwind`: `cortex-m-exception`,
`riscv-trap`, `xtensa-world`, `armv8m-tz`):

1. **Decide whether the stacked frame is usable.** Cortex-M ARMv7-M and
   ARMv8-M: not if `MSTKERR`/`STKERR` is set (the entry stacking
   faulted); ARMv6-M: not if the process stack pointer is outside the
   extension's stack region or misaligned; RISC-V: the trap does not
   stack (registers stay in place, `mepc`/`mcause`/`mtval` are CSRs), so
   the frame is the live register file, always usable; Xtensa: not if
   the World-1 stack pointer is outside its region. The procedure never
   reads the stacked frame when it is unusable, and never depends on it
   otherwise: the continuation comes from the `CallFrame`, which lives
   in privileged RAM the extension cannot reach.
2. **Attribute.** The call's extension id is in the `CallFrame`. The
   fault kind and, where the architecture reports it, the faulting data
   address are added (ARMv7-M and ARMv8-M: `CFSR`, and `MMFAR` only when
   `MMARVALID` is set, else no address; RISC-V: `mcause` and `mtval`;
   Xtensa: `EXCCAUSE` and `EXCVADDR`); ARMv6-M has no fault status or
   address registers (every fault is a HardFault), so the record carries
   the stacked `PC` when the frame is usable and the region it falls in,
   else `PC: unknown`; the fault is attributed to the extension whenever
   a `CallFrame` is open.
3. **Cancel.** Run the cancellation sequence of 4.2 on the `OpState`
   (DMA idle, interrupts masked, peripheral quiesced, buffers reclaimed,
   then the mutex released).
4. **Unwind.** On the handler's own (privileged) stack: restore the
   callee-saved registers from the `CallFrame`; set the privileged stack
   pointer to the saved one; load DAGON's privileged protection map
   (the background map, every extension region disabled); restore
   privileged mode for the resumed context; discard any lazily saved
   floating-point context of the abandoned call (Cortex-M4F/M7:
   clear `FPCCR.LSPACT` and resume with a basic frame; the boot core
   runs with the FPU disabled so an extension FP instruction is a
   `NOCP` fault; extensions are compiled soft-float in every layer and
   kind).
5. **Resume.** Return from the exception into DAGON's continuation
   (Cortex-M: a synthetic basic exception frame on the main stack and
   an `EXC_RETURN` for privileged Thread mode; RISC-V: `mepc` set to
   the continuation, `mstatus.MPP` to M-mode, `mret`; Xtensa: return to
   World 0 at the continuation; ARMv8-M TrustZone: the Secure
   continuation through the Secure stack). The continuation is DAGON
   code that reads the call's outcome (`Returned(result) | Overran |
   Faulted(kind) | Disabled`) from the `CallFrame` and goes on; a port
   call that did not return yields the port's error value, a
   notification that did not return is over.
6. **Record.** `extension_ran | extension_overran | extension_faulted`
   with the attribution of step 2.

Simultaneous timer and fault: both handlers go through one unwind
routine guarded by the `CallFrame`'s state; the second one to run finds
the frame closed and returns without unwinding again.

#### Lockups and the fuse

A fault during the entry of the timer or fault exception, when the
extension has moved its stack pointer out of its region (stack
arithmetic is unprivileged; writing the stack pointer registers through
system instructions is privileged-only on every supported kind): on
ARMv7-M/ARMv8-M the processor enters the handler with the stacking
error flagged and the procedure above applies; on RISC-V nothing is
stacked by hardware, so the procedure applies; on ARMv6-M the escalated
HardFault stacks on the same invalid pointer and the core locks up; on
Xtensa a World-switch exception with an invalid stack is handled on the
privileged stack. A lockup is ended by the watchdog (armed before any
extension runs at boot, 4.1; the app's guard in the app), and the
reset-time contract of 4.2 reinitialises what the call touched.

So that a lockup is not repeated at every reset, DAGON keeps a **lockup
fuse** in retained memory: a record that says which extension of which
image a sandboxed call was running when the previous run died, read by
every layer's start, and used to run with that extension disabled. The
fuse is retained-memory state whose correctness depends on byte-level
behaviour under resets at every point, identity collisions and image
and extension upgrades; like the boot record it is therefore not
specified in prose but by requirements (5.5) and an executable model
whose passing shape is its format. What the design fixes here:

- it covers every sandboxed call in every layer (boot core, app,
  relay);
- a confirmed app whose extension locks the core up comes back with
  that extension disabled and keeps serving; a boot-core lockup after
  `commit` costs that boot's attempt only, and the fuse makes it one
  attempt, not three;
- the fuse is written and read by the layers' starts only; it is not
  the boot record and not an input to `plan`, `commit`, `self_check` or
  `confirm`;
- a tripped fuse is reported on VERSION (0x07) and by `dagon status`
  ("extension <id> disabled after a lockup"), and cleared by the host
  through the agent or when the identity it names no longer exists in
  the running images.

A fault inside DAGON's own fault handler is DAGON's halt with the diag
code.

### 4.4 Protection kinds, the definition schema, and the generated map

The MCU definition declares the protection hardware as data:

```toml
[isolation]
level = "mpu"                 # the level DAGON enforces: none | mpu | pms | tz (4.5)
kind = "armv7m-mpu"           # armv6m-mpu | armv7m-mpu | armv8m-mpu | armv8m-tz | riscv-pmp | riscv-epmp | esp-pms | none
[isolation.capabilities]     # the kind's capability record, read back from the hardware and checked (below)
mpu_regions = 8               # armv6m/armv7m/armv8m-mpu: the unit's region count (MPU_TYPE.DREGION)
min_region = 32               # bytes (256 on armv6m-mpu implementations such as the RP2040)
subregions_from = 256         # 0 if none
# armv8m-tz adds: sau_regions, s_mpu_regions, ns_mpu_regions, idau = [{ base, size, attribution }]
# riscv-pmp adds: pmp_entries (0 | 16 | 64), grain (the smallest NAPOT size the implementation honours), na4 = true|false, tor = true|false
# esp-pms adds: worlds, memory_regions, peripheral_slots, gdma_channels
critical_section = "basepri"  # primask | basepri | mstatus-mie | xtensa-level: how the non-preemptible part masks the sandbox timer (4.2)
np_max_us = 50                # bound of a non-preemptible part on this MCU (asserted against the measured worst case)
dma_stop_max_us = 20          # how long a channel may take to report idle after a stop
reset_cleanup = "peripheral-reset"   # peripheral-reset | reinit: how the reset-time contract of 4.2 is met
background_privileged = true  # the unit can give privileged code a default map
unwind = "cortex-m-exception" # cortex-m-exception | riscv-trap | xtensa-world | armv8m-tz
timer = "systick"             # the sandbox timer
fault_address = true          # the unit reports the faulting data address
boot_extensions = true        # the MCU has a DAGON boot stage
ext_ram_max = 24576
root_protection = { kinds = ["rdp", "wrp"], rdp_levels = ["rdp1", "rdp2"] }   # the MCU's root-image protection and its levels (4.7)
cache = "none"                # none | i-cache | i+d-cache (4.10)
tcm = []                      # tightly coupled memories, each { base, size } (4.10)

[[peripheral]]                # every register block the MCU has
id = "can1"
base = 0x4000_6400
size = 0x400
window_eligible = true        # self-contained, no memory writes, no routing, not shared with DAGON

[[peripheral]]
id = "dma1"
base = 0x4002_6000
size = 0x400
window_eligible = false
bus_master = true             # it can write memory
privileged_why = "dma"        # dma | flash | watchdog | clock | reset | irq-routing | pinmux | power | rom | core-control | protection | shared

[[core]]                      # every processor core; DAGON runs on the first
id = "cpu0"
```

The capability record is checked twice: `dagon gen` plans against the
definition's numbers and refuses a definition whose numbers are outside
the kind's allowed set (below); at boot and in the conformance build the
MCU package reads the unit's own registers (`MPU_TYPE` on Cortex-M, the
SAU type register, the count of writable `pmpcfg` entries and the
smallest NAPOT size that reads back on RISC-V, the PMS version
registers) and refuses to enable a level whose record the hardware does
not confirm (`boot.isolation_mismatch`, level reported as `none`).

Required by the checker for every definition whose `kind` is not
`none`: at least one `[[peripheral]]` with `privileged_why` of each of
`flash`, `watchdog`, `clock`, `reset`, `irq-routing`; every entry with
`bus_master = true` has `window_eligible = false`; for an MCU with more
than one `[[core]]`, a `core-control` block; for a kind with a
protection-configuration register block (`esp-pms`, `riscv-epmp`), a
`protection` block. A definition that leaves one out is refused with
the missing class named: an MCU whose definition does not say where its
DMA controllers are cannot be given a level above `none`.

The map generator and checker are generic over `kind`: one module per
kind behind one trait, used by `dagon gen` and by the test:

```rust
pub trait ProtectionKind {
    fn plan(&self, def: &McuDef, exts: &[ExtLayout]) -> Result<Map, Refusal>;   // regions / entries / worlds
    fn check(&self, def: &McuDef, map: &Map) -> Vec<Violation>;                  // alignment, overlap, attributes, privileged blocks outside every allowed range, fixed regions present
    fn unwind(&self) -> UnwindKind;
    fn guarantees(&self) -> Guarantees;                                          // what 4.5 prints for the level
}
```

`plan` places `.ext.text`, `.ext.data`/`.ext.bss` and the stack at
addresses the kind's alignment rule allows (the linker script is
generated from the same result), assigns the declared windows, and
refuses with a clear message when the entries do not fit (`extension
uds-can needs 5 windows; 3 fit after code, data and stack on a
8-region armv6m-mpu`). `check` runs in the MCU package's test against
the generated tables and the definition; the conformance build executes
a write to each privileged block from an extension on the device and
expects the fault event. Adding an MCU of a known kind touches no code;
adding a kind adds one module.

Per kind, what the hardware gives and what the generator does with it:

| Kind | Unit | Capability record and shape | Fault reporting | What it guarantees | What it cannot |
|---|---|---|---|---|---|
| `armv6m-mpu` | ARMv6-M PMSAv6 MPU | `mpu_regions` (an implementation choice, read back from `MPU_TYPE`; 8 on the RP2040), power-of-two sizes, base aligned to size, `min_region` as implemented (256 B on the RP2040), 8 subregions from 256 B, `PRIVDEFENA`; `critical_section = primask` (no `BASEPRI` on this architecture, so a non-preemptible part masks every interrupt and `np_max_us` is the layer's interrupt-latency bound) | every fault is HardFault; no status or address register; attribution by `CallFrame` and stacked PC (4.3) | unprivileged denial of everything outside the enabled regions; XN per region | a fault during exception stacking locks the core up (the lockup fuse, 4.3); windows must be aligned blocks of at least `min_region` |
| `armv7m-mpu` | ARMv7-M PMSAv7 MPU | `mpu_regions` ∈ {8, 16} as implemented (Cortex-M3/M4: 8; Cortex-M7: 8 or 16, read back), power-of-two, base aligned to size, 32 B minimum, subregions from 256 B, `PRIVDEFENA`; `critical_section = basepri` | MemManage with `CFSR`, and `MMFAR` when `MMARVALID`; stacking errors flagged, no lockup | as above, with data-fault addresses | windows are power-of-two blocks; on cached cores the rules of 4.10 apply |
| `armv8m-mpu` | ARMv8-M PMSAv8 MPU | `mpu_regions` ∈ {0, 4, 8, 12, 16} as implemented (read back), base/limit pairs on a 32 B granule (no power-of-two rule, no subregions), `PRIVDEFENA` | MemManage with `MMFAR` when valid | as `armv7m-mpu`, with windows of any 32 B-aligned extent | 0 regions is `none` |
| `armv8m-tz` | ARMv8-M with the Security Extension | three independent records: `sau_regions` ∈ {0, 4, 8}, `s_mpu_regions` and `ns_mpu_regions` each ∈ {0, 4, 8, 12, 16}, all read back; plus the `idau` table of the MCU's fixed attribution and the `mpc`/`ppc` units the definition lists; DAGON is Secure, extensions Non-secure and unprivileged under the NS-MPU; the service ABI is the Secure gateway (`SG` veneers in an NSC region) | Secure and Non-secure faults, `SecureFault` | the strongest: Secure memory is invisible to extensions even when privileged in the Non-secure state; bus masters are attributed Secure/Non-secure by the interconnect and the definition lists each master's attribution | the IDAU/MPC/PPC configuration is MCU-specific data; a master the interconnect cannot attribute must stay Secure-only; attribution may come from the IDAU alone (`sau_regions = 0` with a complete `idau` table is a valid `tz` definition) or from IDAU plus SAU; the level is `none` only when neither can attribute the extension regions |
| `riscv-pmp` | RISC-V PMP | `pmp_entries` ∈ {0, 16, 64} as implemented (read back by probing which `pmpcfg` entries are writable), address modes NA4 (4 B, if `na4`), NAPOT (from 8 B, the implementation's `grain` read back), TOR (any range on the granule, if `tor`); U-mode for extensions, M-mode for DAGON; `critical_section = mstatus-mie` | `mcause`, `mtval` (address), no hardware stacking | denial for U-mode of everything not granted; XN per entry | without ePMP, M-mode is not restricted by PMP; entries are few and TOR consumes an entry per boundary; 0 entries is `none` |
| `riscv-epmp` | RISC-V ePMP (Smepmp) | as PMP, plus `mseccfg` machine-mode lockdown and shared regions | as PMP | as PMP, and DAGON can forbid M-mode execution from extension memory | |
| `esp-pms` | Espressif Permission Control + World Controller | `worlds`, `memory_regions`, `peripheral_slots`, `gdma_channels` as the part implements (read back from the PMS version and region registers); `critical_section = xtensa-level`; per-world (World 0 DAGON, World 1 extensions) permissions on internal memory regions, external memory through the MMU, each peripheral, each GDMA channel; applied per core; lockable | `EXCCAUSE`/`EXCVADDR`, World-switch exception | denial for World 1 of everything not granted, on every core, including GDMA | the PMS/WCL registers must be programmed by DAGON's MCU package (no HAL driver); until then `none` |
| `none` | no usable unit | | | time measured, events recorded | memory, flash, watchdog, hang (4.5) |

The level a definition may claim follows from the kind: `mpu` for the
MPU and PMP kinds, `pms` for `esp-pms`, `tz` for `armv8m-tz`, `none`
otherwise or until the package implements the kind.

### 4.5 Isolation level per MCU, and the three examples

`dagon-mcu.toml` declares the level DAGON enforces on that MCU; VERSION
reports it (5.4) so the host shows it next to the node.

| Level | Guarantees | Unguaranteed (stated in docs, VERSION, UI) |
|---|---|---|
| `mpu` | all of 4.1–4.4: unprivileged execution, DAGON code and data denied, service-call validation, capability windows, DMA and interrupts privileged, timer preemption and unwinding, fault recovery, the lockup fuse | physical effects of declared capabilities (4.6) |
| `pms` | the same through the permission controller and world switching, on every core, with GDMA denied per channel | the same |
| `tz` | the same through the Security Extension; additionally DAGON's memory is invisible to any Non-secure software | the same |
| `none` | time limit measured and reported (`extension_overran`) but not preempted; capability handles passed as declared; events recorded. No hardware denial: an extension can, by bug or intent, write flash, touch the watchdog or DAGON's memory, or hang until the watchdog resets the chip (which, at boot, costs the attempt only if it happens after `commit`) | memory and flash denial, the time limit, the decision/signature/rollback/watchdog rules against a misbehaving extension; `dagon status` and the page show "extensions unguarded" for such a node, and `dagon doctor` warns when the project links any untrusted extension into it |

The three MCUs DAGON ships definitions for, as examples of the schema
(the normative maps are the ones `dagon gen` generates from the
definitions in `targets/mcu/*/dagon-mcu.toml`; the facts below are from
the vendors' reference manuals and are read back from the units' type
registers at bring-up):

**STM32F401RE** — `kind = "armv7m-mpu"`, `mpu_regions = 8` (read back
from `MPU_TYPE`), `min_region = 32`, `subregions_from = 256`, `fault_address = true`, `unwind =
"cortex-m-exception"`, `level = "mpu"` at boot and in the app (ST
PM0214 §4.2 MPU; ST RM0368 §2 memory map for the block addresses).
Generated map for one extension with two windows:

| Region | Base | Size | Attributes | Purpose |
|---|---|---|---|---|
| 0 | `.ext.text` in the image body, aligned to its size | 2 KiB … 32 KiB | unprivileged RO, X | extension code and constants |
| 1 | `.ext.data` + `.ext.bss` in RAM, aligned | 1 KiB … 16 KiB | unprivileged RW, XN | extension data |
| 2 | extension stack, aligned | 1 KiB … 8 KiB | unprivileged RW, XN; lowest subregion disabled | stack with a guard |
| 3–4 | capability windows: one 1 KiB APB/AHB block each | 1 KiB | unprivileged RW, XN | declared peripherals (window-eligible) |
| 5–6 | free for more windows | | | |
| 7 | reserved for a second data region when `ram_bytes` is not a power of two | | | |

Privileged (`window_eligible = false`): flash interface `0x4002_3C00`
(`flash`), IWDG `0x4000_3000` and WWDG (`watchdog`), RTC and backup
`0x4000_2800` (`power`), RCC `0x4002_3800` (`clock`, `reset`), PWR,
SYSCFG and EXTI `0x4001_3800`/`0x4001_3C00` (`irq-routing`, `pinmux`),
DMA1 `0x4002_6000` and DMA2 `0x4002_6400` (`dma`, bus masters), USB OTG
FS `0x5000_0000` (shared with DAGON's link), CRC, the system memory
`0x1FFF_0000` (`rom`), the option bytes, the PPB.

**RP2040** — `kind = "armv6m-mpu"`, `mpu_regions = 8` (read back from
`MPU_TYPE`), `min_region = 256`,
`subregions_from = 256`, `fault_address = false`, `unwind =
"cortex-m-exception"`, two `[[core]]`, `level = "mpu"` at boot and in
the app with HardFault attribution and the lockup fuse (RP2040
datasheet §2.4 Cortex-M0+ features and the MPU type register; Arm
ARMv6-M Architecture Reference Manual, exception model). Generated map:

| Region | Base | Size | Attributes | Purpose |
|---|---|---|---|---|
| 0 | `.ext.text` in the XIP window inside the image's slot body, aligned to its size | 1 KiB … 32 KiB | unprivileged RO, X | extension code and constants |
| 1 | `.ext.data` + `.ext.bss` in SRAM, aligned | 256 B … 16 KiB | unprivileged RW, XN | extension data |
| 2 | extension stack, aligned | 1 KiB … 8 KiB | unprivileged RW, XN; lowest subregion disabled | stack with a guard |
| 3–5 | capability windows: one 16 KiB APB block each (UART, SPI, I2C, PWM, ADC, PIO) | 16 KiB | unprivileged RW, XN | declared peripherals (window-eligible) |
| 6 | reserved for a second data region | | | |
| 7 | reserved for a sub-block deny inside a window | | no access | |

Privileged: the boot ROM `0x0000_0000` (`rom`; its flash routines are
reachable only by executing ROM, which is denied as a whole), XIP
control `0x1400_0000` and SSI `0x1800_0000` (`flash`), DMA `0x5000_0000`
(`dma`, bus master), USB `0x5010_0000` (bus master into its own DPRAM,
and DAGON's link), WATCHDOG `0x4005_8000` (`watchdog`), CLOCKS
`0x4000_8000` (`clock`), RESETS `0x4000_C000` (`reset`), PSM
`0x4001_0000` and SIO `0xD000_0000` (`core-control`: the second core is
never started by DAGON, and the blocks that could start it are
privileged, so a single-core map suffices; GPIO output lives in SIO and
is a service), IO_BANK0/QSPI and PADS `0x4001_4000`–`0x4001_C000`
(`pinmux`), XOSC/PLL/ROSC, VREG (`power`), TIMER, RTC, the PPB.

**ESP32-S3** — `kind = "esp-pms"`, two `[[core]]`, `boot_extensions =
false` (vendor stage 2, no DAGON boot stage), `fault_address = true`,
`unwind = "xtensa-world"`, `level = "none"` until the MCU package
programs the PMS and WCL registers (the HAL exposes no driver), then
`pms` for the app (Espressif ESP32-S3 TRM, "World Controller" and
"Permission Control" chapters). Generated map at `pms`:

| Resource | World 1 (extension) | Notes |
|---|---|---|
| `.ext.text` region of internal SRAM (or IROM through the MMU) | read, execute | |
| `.ext.data`, stack regions | read, write, no execute | guard by a denied region below the stack |
| declared peripherals | read, write | window-eligible list in the definition |
| everything else: DAGON's code and data, the ROM, SPI0/SPI1 (cache and flash, `flash`), the RTC and TIMG watchdogs (`watchdog`), RTC_CNTL (`power`, `reset`), the system and interrupt-matrix registers (`irq-routing`, `clock`), GDMA (`dma`, bus master), USB OTG (bus master), the AES/SHA/RSA engines (bus masters through GDMA), both cores' control registers (`core-control`), the PMS and WCL registers (`protection`) | no access | GDMA permission control denies World 1 memory access by every channel; the map is applied to both cores and locked before World 1 is entered |

### 4.6 What the sandbox cannot guarantee

Not guaranteed by any level: what a declared capability can do in the
physical world. A pin the manifest declares can cut the board's power
if the wiring lets it; a bus write to a declared device's writable
register can change that device's state. The manifest declares; `dagon
gen` shows the declared capabilities of every extension in the project;
the board definition narrows a bus capability to a device and a register
set and a pin to its allowed levels; and the board author owns the
wiring. Whether a peripheral is window-eligible is the MCU definition's
statement that it cannot write memory, route interrupts or clocks, or
share state with DAGON (11); the checker refuses a definition that
marks a bus master eligible.

The guarantee covers DAGON's public API and the sandbox; code that is
linked privileged (DAGON itself, trusted extensions) is outside it by
construction.

### 4.7 Trusted extensions and the authenticated chain

A plug-in that provides a trusted-only port (2.1) replaces a part that
the safety rules are made of. It is accepted only when:

1. its manifest says `trusted = true` and names the trusted ports it
   provides;
2. the project's `dagon.toml` lists it under `[boot] trusted_extensions`
   or `[app] trusted_extensions` by id and by the SHA-256 of its built
   artifact (the linked `.ext.*` bytes, computed by `dagon gen`);
3. the image that contains it is signed with the project key, the
   signature covering the extension's bytes, and the device verifies
   that signature before the image runs (below);
4. `dagon gen`, `dagon status`, `dagon doctor` and the page show the
   trusted extensions of each node by id, and VERSION reports their ids
   and the first bytes of their artifact hashes for display (0x08) and
   the full hashes for identity (0x09).

A trusted extension runs privileged, in DAGON's own regions: it is that
part of DAGON for that project. Nothing in 4.1–4.6 holds against it, and
the documents say so wherever trusted extensions are mentioned.

The one authentication rule, applied by every verifier in the chain
(the `ImageVerifier` and `Digest` ports, whose defaults are the rule):

- **Signed bytes.** The image header's signature covers the header's
  bytes from offset 0 to the start of the signature field, which
  include the format number, the image kind, the MCU identity, the
  target tag, the slot binding, the version, the body length and the
  body's SHA-256; the body is authenticated by that digest, and the
  body includes every `.ext.*` section, so an extension's bytes are
  covered.
- **Key selection.** The verifier accepts exactly the key whose id the
  header names, and only if that id is the project key embedded in the
  verifying image at build (`DAGON_PUBKEY`; the public test key only in
  a build that opts in). There is no key list on the device and no
  fallback key.
- **Who verifies what.** The agent verifies an image before `written`
  and the boot core before `chosen` on a trial boot (and checks the
  digest on every boot). On a self-boot MCU with a stage0, **stage0
  verifies the boot core image under the same rule before
  `core_slot_chosen`**, not its digest alone; the boot core log's
  confirmed record names the slot and the header digest, and stage0
  checks both the record's digest and the signature. On a self-boot MCU
  without a stage0, the boot core is the root and is verified by the
  host before it is written and by the agent when it is written through
  the update path.
- **Stage0 budget.** The MCU definition's `[layout] stage0.size` is the
  whole of stage0 (vector table, the verifier, the flash reader, the
  recovery path, the record writer); `dagon gen` emits a link-time
  assertion (`ASSERT(SIZEOF(.text) + SIZEOF(.rodata) + SIZEOF(.data)
  <= stage0.size - trailer)`) and a `#[cfg]`-checked constant so the
  build fails when the verifier does not fit, and a worst-case
  verification time (`[layout] stage0.max_ms`, the signature check plus
  the digest of the largest boot core slot at the reset clock) that the
  conformance build measures with the cycle counter and compares. When
  a definition's stage0 does not fit the verifier, the layout is
  changed (a larger stage0 region) before the first public release;
  formats before that release carry no compatibility promise (6).
- **The root image** (stage0 where it exists, else the boot core) is
  written through the MCU's ROM loader at provisioning. ROM loaders
  verify nothing of DAGON's: whoever can drive the ROM loader can
  replace the root, and with it every rule. The chain therefore holds
  against every update path DAGON offers (its wire, a replaced transport
  such as UDS, the hub relay) and not against physical reprogramming
  through the ROM. Where the MCU offers protection of the root image,
  the definition lists it with its levels (`[isolation]
  root_protection`: for the STM32, `rdp1` is reversible by a mass erase
  and `rdp2` is irreversible, plus sector write protection `wrp`;
  nothing on the RP2040, whose boot ROM always honours BOOTSEL; secure
  boot and flash encryption eFuses on the ESP32-S3), `dagon provision`
  offers to set a level and says which are reversible, and VERSION
  reports the exact level and the protected sectors (0x07), so the host
  shows a node whose root is replaceable, reversibly protected, or
  irreversibly protected.

### 4.8 App, relay and host

A sandboxed app extension (level `mpu`/`pms`/`tz`) has the guarantees of
4.1–4.4 for its handlers; an app extension at level `none` has the
API-shape guarantees only:

| Rule | Enforced by |
|---|---|
| No store access | Extensions receive read-only payloads copied into their own region; applications get `ImageView` (2.3); the slot store's mutators are reachable only through the image-lifecycle machine. |
| Verify/confirm order | The machines' transitions are the only state changes: `self_check` is consulted only inside the `confirmed` transition after the "on trial" guard; `accept_update` only after the DAGON guards. An extension's answer is a validated `enum` (`RETURN`), never a closure DAGON executes. A protocol plug-in produces triggers; the session machine runs the guards. |
| Time | Decision bounds in 3; notifications have the same bound, preempted at a hardware level, measured at `none`. |
| Watchdog | No service touches it. |
| Reboot | 3.2: capped delay, never refused. |

Relay extensions are app extensions. Host extensions run with the user's
permissions in the user's operating system; data-defined ones (actions,
agents, transports named by program) run out of process under the shared
command transport's limits, and no host extension answers a decision
(3.4).

### 4.9 Conformance cases

The MCU package's conformance build runs, from an extension on the
device, and expects the recorded outcome: a write to each privileged
block of the definition (fault), a read of DAGON code (fault), a jump
into DAGON code (fault), a forged service id (fault), a service pointer
outside the region and a length that overflows (refused), an invalid
unprivileged stack pointer before a service call (the kind's documented
outcome: flagged stacking error and unwind, or lockup, watchdog reset,
fuse set and the extension disabled on the next boot), a loop past the
budget (overran), an interrupt storm from a declared peripheral (source
disabled), a DMA request through a window that is not eligible (refused
at generation), nested faults (one unwind), a timer and a fault in the
same call (one unwind), an attempt to start another core where the MCU
has one (fault), and on a `pms` MCU a GDMA access from World 1
(denied). Added for the services: the cancellation-boundary cases of 4.2 and the
reset-time contract case (a watchdog reset inside each non-preemptible
part, then first use succeeds); for the fuse: the scenarios of its
model requirements (5.5), run on the device. The cases are generated from the
definition (one per privileged block, one per core, one per bus master,
one per service cancellation boundary), so a new MCU gets its cases from
its definition. Each case is also a scenario of the
executable model (5.5), which checks that the boot reaches the same
decision as without the extension.

### 4.10 Other architectures in the same model

The model's MCU-specific parts are all definition data, and each has a
small closed set of mechanisms behind a trait:

| Part | Definition data | Mechanisms (one module each) |
|---|---|---|
| Protection map | `[isolation] kind` and parameters, `[[peripheral]]`, `[[core]]` | `ProtectionKind` (4.4) |
| Unwinding | `[isolation] unwind` | `cortex-m-exception`, `riscv-trap`, `xtensa-world`, `armv8m-tz` |
| Sandbox timer | `[isolation] timer` | `systick`, `riscv-mtime`, `timer-group` |
| Watchdog | `[watchdog] kind, trial_ms, confirmed_ms` | the `WatchdogDriver` port's defaults per kind (`iwdg`, `rp-watchdog`, `rtc-wdt`, …) |
| Boot record placement | `[boot.record] kind, at, survives, rings` | `noinit-ram`, `retained-registers` (5.5) |
| Root protection | `[isolation] root_protection` | per MCU family, applied by `dagon provision` |
| Caches and TCM | `[isolation] cache, tcm` | window attributes and bounce-buffer placement (below) |

Kinds that are not among the first three MCUs but fit the same schema:

- **ARMv8-M with TrustZone-M** (Cortex-M23/M33/M55/M85): the
  `armv8m-tz` row of the kind table (4.4) is the whole statement of its
  capabilities and guarantees; what is specific to this family beyond
  the table is that bus masters are Secure or Non-secure by the
  interconnect's attribution units, which the definition lists per
  master (`bus_master_attribution = "secure" | "configurable"`), and a
  configurable master used by an extension's I/O goes through DAGON's
  DMA service as Secure.
- **RISC-V PMP/ePMP** (ESP32-C/H series, RP2350's Hazard3, many
  others): the `riscv-pmp` and `riscv-epmp` rows of the kind table are
  the statement of entries, address modes and guarantees; what is
  specific beyond the table is that the ESP32-C/H parts also have the
  Espressif PMS for peripherals and GDMA, so their definitions combine
  `riscv-pmp` for memory with the `esp-pms` peripheral and DMA controls
  (a kind `riscv-pmp+esp-pms` whose module composes the two).
- **Cortex-M7 and other cached cores**: the `armv7m-mpu` row of the kind
  table; the definition states `cache = "i+d-cache"` and the TCM
  ranges. The generator marks capability windows Device (non-cacheable)
  and places the DMA bounce buffers in a non-cacheable region (a DTCM
  range, or an MPU region with the non-cacheable attribute); the DMA
  services clean before a transmit and invalidate after a receive when
  the buffer is cacheable; `.ext.text` may live in flash or ITCM, and
  the unwinding is unchanged.
- **A dual-core MCU where DAGON uses one core**: the other core is
  listed under `[[core]]`, its launch and inter-core blocks are
  `core-control`, and the map is applied on every core where the kind
  configures per core (`esp-pms`); where the unit is per core and the
  second core is never started, the checker requires the launch blocks
  to be privileged.

What no kind guarantees: physical effects of declared capabilities
(4.6), DAGON's own bugs in privileged code, and anything a ROM loader
can rewrite (4.7).

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
writes `boot_wiring.rs` for the boot core build (`LED`, `HALT`,
`PROGRAM`, `RECORD`, the extension chain type and its protection map); a
board package ships its definition and the boot core build reads it the
same way. The MCU crates take these constants and hold no pin, pattern
or environment-variable default of their own.

`dagon-mcu.toml` carries what the MCU, not the board, determines:
`[watchdog] trial_ms, confirmed_ms`, `[isolation]` (4.4),
`boot_capabilities` (5.2) and `[boot.record]` (5.5).

### 5.2 Boot action program: the simplest extension kind

For a board that needs an LED pattern or a power-hold line and no code,
the board definition holds a declarative, bounded program that DAGON
interprets. It is an extension like any other in the chain (the board
package position), observes action-capable events, and runs under the
same rules: only from `WatchdogArmed` on, only on declared pins, buses
and devices.

```toml
[boot]
led = "GPIO25"
led_active = "high"
halt = { pattern = "blink", period_ms = 100 }
power_hold = { pin = "GPIO14", level = "high" }

[[boot.action]]
on = "watchdog_armed"              # any boot event marked action-capable in CATALOG (never entry or hal_ready)
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
access, clock or watchdog control. A boot core that does not know an op
treats the program as invalid as a whole (`boot.program_skipped`).

### 5.3 Shared resources

A board resource several parties need is shared, not owned.

- A bus the board package needs for bring-up and recovery and the app
  needs for its own devices is exported as `pub static SYS_I2C:
  Shared<I2c<'static, Blocking>>` (`Shared<T> = embassy_sync::
  blocking_mutex::Mutex<CriticalSectionRawMutex, RefCell<Option<T>>>`,
  used through `with(|bus| ..)`). DAGON's bring-up and recovery take it
  through `with` (bounded, synchronous, inside the lease); the app does
  the same at any time; a sandboxed extension reaches it through the
  bus port call with its declared device. The package exports its
  device constants and register-level functions, so an app copies
  nothing.
- Board bring-up is the board package's extension on `app.bring_up`
  (the `BoardBringUp` port); a user step is an extension later in the
  chain on the same event, or a task waiting for `Latest.phase ==
  Serving`.
- A board package holds the board, not what is stacked on it; a stacked
  peripheral's driver lives with the app that uses it, and the package
  hands out the bus pins as plain peripherals.

### 5.4 VERSION query

Command `Cmd::Version`, part of the wire contract. A node that does not
implement it (a recovery agent, a minimal image) answers `E_UNSUPPORTED`
through the unknown-command rule, which the host shows as "versions
unknown". Request: empty for the summary set, or one selector `[0x21,
offset u16 LE, len u8]` for a page of the boot record. Response: TLV,
`u8 count` then `u8 id, u8 len, bytes`; a reader ignores ids it does not
know; a value the device cannot determine is omitted, never a sentinel.
Every TLV value is at most 255 bytes and the whole response fits the
smallest link payload (10).

| id | Value |
|---|---|
| 0x01 | boot core version (`Version`, LE u32) |
| 0x02 | installed boot image identity: first 8 bytes of its SHA-256 |
| 0x03 | agent (`dagon-agent`) version |
| 0x04 | MCU package version |
| 0x05 | board package version |
| 0x06 | MCU identity (5.5): the value the image header and the compiled topology carry |
| 0x07 | isolation (4.5): `level u8`, `kind u8`, `boot_extensions u8`, `root_protection` (kind and exact level as the definition enumerates them, e.g. `rdp1`/`rdp2`, the write-protected sectors, and whether the level is reversible), `fuse` (Clear, or Tripped with the disabled extension's identity as the model encodes it, 5.5) |
| 0x08 | extensions, for display: per linked extension `id` (up to 32 bytes), `trusted u8`, first 8 bytes of its artifact hash; repeated TLVs, one per extension |
| 0x09 | extensions, for identity: per trusted extension the full 32-byte SHA-256 of its linked artifact; repeated TLVs |
| 0x10 | formats: pairs `(format id u8, number u8)`: wire, image-header, state-log, event, definitions, boot-program, boot-record, extension-manifest |
| 0x20 | boot record summary (5.5): which boots the record holds and the decision and reason of the newest; layout by the model |
| 0x21 | one page of the raw boot record region: `offset u16`, then `len` bytes (`len ≤ RECORD_PAGE`, 10); the host reads the region page by page and decodes it with the model's reader |

The running boot core is identified from flash, never from what the app
was compiled against:

- **Self-boot with boot core slots and an inline header**: the app reads
  the boot core log to learn which boot core slot was started, then that
  slot's header (0x01) and hashes the image (0x02).
- **Self-boot with a trailer header**: the app reads the trailer (0x01)
  and hashes the body (0x02).
- **Vendor-stage2**: the installed bootloader has no DAGON header. The
  app reads it from flash at the vendor's bootloader offset, determines
  its length by walking the vendor image's segment table, hashes it
  (0x02), and omits 0x01. The host matches 0x02 against the identity
  recorded at provisioning (`host.provision.recorded`, kept in the
  project) and against the MCU package's vendor table, and shows the
  version or "unknown vendor stage (hash …)".

`dagon status` prints one line per node with these values, the isolation
level ("guarded" / "unguarded extensions"), the trusted extensions, and
the last boot summary; `dagon doctor` compares the format numbers with
`compat.toml`, warns on a number the CLI does not read, and warns on an
untrusted extension linked into a node at level `none`.

### 5.5 Boot record, MCU identity and compiled topology: requirements and the executable model

Four formats of this model touch retained or persistent data whose
correctness depends on byte-level behaviour under resets, power loss,
partial writes and version skew: the boot record in retained memory, the
lockup fuse next to it, the MCU identity carried by the image header,
the identity reply and VERSION, and the compiled topology's node layout
and replacement. Prose
cannot show that such a format survives every interleaving of faults;
an executable model can. These formats are therefore not specified in
this document. They are designed as a host-side Rust model with
randomized fault injection, and the shape that passes its properties
becomes the written specification of each format, published with the
model's tests as its proof. The model also exercises the sandbox's
observable contract (an extension that overruns, faults or misuses a
capability at every boot step) against the boot machine's continuation.

This section states what those formats must satisfy; the model is
accepted only when every property below holds under its fault injection.

#### Boot record

- **Observability.** Every boot transition of every boot stage (stage0,
  boot core) is recorded, and the app's agent replays every intact
  recorded event at start as the catalog event with `replayed: true` and
  an identity that distinguishes boots, writers and positions. A boot
  that halts, hangs, or never reaches an app is replayed by the first
  app that runs afterwards. Loss of events (capacity) is itself recorded
  and replayed.
- **Torn-write safety.** A write interrupted at any byte, including an
  overwrite of a previously valid slot, loses at most the event being
  written and never yields a different event that a reader accepts as
  intact. No field is ever updated in place; nothing a reader accepts is
  the product of two writes.
- **Writer independence.** A writer derives its write position only from
  constants of the MCU definition and from data it has validated; it
  never interprets another writer's content, so a stage may be upgraded
  before or after the other in any order and malformed retained memory
  cannot direct a write outside the record. Identities stay unique
  across stage upgrades in either order, including a first stage that
  wrote nothing before its upgrade.
- **Ordering.** Boots are replayed oldest first across sequence wrap,
  including when one writer has been silent for more than half the
  sequence space; the reader never orders a newer boot before an older
  one it still holds, and an identity never repeats while both its
  events are still held.
- **No decision input.** The record is never read by `plan`, `commit`,
  `self_check` or `confirm` (4.1): the model's API gives the boot stages
  only `append` and the agent only `events`. Extensions cannot reach it
  (denied region).
- **Versioning.** The retained layout carries what a reader needs to
  refuse a layout it does not read (6); retained bytes of an older or
  newer layout are never accepted as valid events of the reader's
  layout.
- **Size.** The region is at most `RECORD_MAX` bytes and is read in
  pages of `RECORD_PAGE(link)` bytes (10); its location and size per
  writer are declared in the MCU definition's `[boot.record]` and reach
  every image's `memory.x` through one path (the MCU crate's build
  script), with a test that the images agree.
- **Replay semantics.** At least once: nothing in the record says what
  was replayed; consumers de-duplicate on the identity, and the agent
  does not replay one identity twice within a run.
- **Independence from the state log.** `app.trial_started` and
  `app.rolled_back` come from the state log regardless of the record.

#### Lockup fuse

- **Owner.** The fuse is written only by the layer that is about to run
  a sandboxed call (boot core, app or relay) and read by every layer's
  start; no other code reads or writes it, and the boot decision, the
  confirm rule and the self-check never take it as input.
- **Identity.** A fuse names the image (slot and the image's header
  digest) and the extension (its position in the image's chain and its
  full artifact hash) and the layer, completely enough that no other
  build of any extension in any image can match it; the model tests
  collisions by construction, and no count of extensions or slots is
  capped by the encoding.
- **False-trip-free.** A reset at any point of arming, of the call, of
  the return, or of clearing never produces a fuse that names a call
  that returned; a reset during a call that locked up always produces a
  fuse that names that call (reset survival as the MCU definition
  declares; power loss may lose a fuse but never forge one).
- **Hand-off.** A fuse armed by an app or relay call survives the boot
  core's start unchanged (the boot core records it and does not clear
  or act on another layer's fuse) and is read by the app's start, which
  disables the named extension for its run; a fuse armed by a boot-core
  call is acted on by the boot core's next start.
- **Stale and upgrade.** A fuse whose identity matches no extension of
  the images now installed (the image or the extension was replaced) is
  recorded as stale and cleared by the layer that would have acted on
  it, never by another layer; an extension whose artifact hash changed
  is a new identity and runs again.
- **Clearing.** The host clears a tripped fuse through the agent
  (`dagon extension reset <node>`); nothing else clears a fuse whose
  identity still exists.
- **Versioned.** The fuse carries what a reader needs to refuse a layout
  it does not read (6); bytes of another layout are never acted on.
- **Size and placement.** Declared in the MCU definition next to the
  boot record (`[boot.record] fuse`), reaching every image through the
  same linker path (one test that the images agree).

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

- **Machine.** `Absent → Written` (first copy) and `Written → Written`
  (replacement), both through the image lifecycle's `written` with
  target TOPOLOGY, both emitting `app.written` and `relay.children_set`.
- **Round trip.** A copy the host pushed reads back on the hub as the
  same topology (node ids, links, addresses, MCU identities), and
  `host.topology.pushed` carries its digest, which the hub's
  `relay.children_set` repeats.
- **Bounds.** A topology larger than the hub's TOPOLOGY region is
  refused by the host before any byte is sent (`E_RANGE` is never
  reached) and by the hub if sent anyway.
- **Malformed or unsupported.** A copy whose layout number the hub does
  not read, or that fails its own integrity check, reads as absent, is
  reported by `relay.children_set { source: Fallback }`, and the project's
  fallback list is used; it is never partially applied.
- **Interrupted replacement.** A replacement interrupted at any byte,
  once or repeatedly, leaves the hub reading exactly the previous copy
  or exactly the new one; the model states, for every interruption
  point, which of the two it is, and the outcome is the same on every
  subsequent read until the next write.
- **Exact old-copy outcome.** After an interrupted replacement that
  leaves the previous copy, the previous copy is byte-identical to what
  it was; after one that completes, the previous copy is unreachable.

#### What stays in prose

Every other byte-level structure in this document is either generated at
build time and validated before it reaches a device (the boot action
program table, `boot_wiring.rs` constants, the extension protection map), or a
request/response encoding with no persistence (VERSION's TLV framing,
the serve JSON messages), or an existing contract this model does not
change (the image header's signature and digest fields, the state log,
the vendor selection data, the release bundle). None of them is written
twice or read back after a fault, so none needs the model; the model's
harness may still exercise VERSION paging against every link size.

## 6. Versioning rules

Numbers and components follow `docs/versioning.md` and are registered in
`compat.toml`. The rules specific to this model:

| Contract | Number moves when | Reader rule |
|---|---|---|
| `event` (`EVENT_FORMAT`) | a variant is renamed or removed, or a payload's meaning changes; adding a variant or a payload field keeps it | a reader ignores kinds and fields it does not know; refuses a number it does not read |
| `extension-manifest` | a manifest key's meaning changes or a key the reader refuses is added | `dagon gen` refuses a manifest with a number it does not read, naming both numbers |
| `boot-program` | an op is added, removed or its fields change | a boot core that meets an op it does not know treats the whole program as invalid and emits `boot.program_skipped`; `dagon gen` refuses to generate a program for a boot core whose number does not read it |
| `boot-record` (and the lockup fuse beside it) | the retained layout changes in a way an older reader would misread (the model's compatibility tests decide, 5.5); a writer's event encoding moves with its `event` number, independently per writer | a reader that does not read the number reports the record as unreadable and still serves raw pages on VERSION 0x21; a reader that does not read one writer's `event` format counts that writer's events and skips their content |
| `image-header`, `topology-bin` | the MCU identity field or the node layout changes (5.5) | a boot core, an agent and a hub accept the numbers `compat.toml` lists for them and refuse others by number |
| `action-defs` | the action file's keys change incompatibly | an action file with a number the CLI does not read is reported as not loaded, like an agent definition |
| `definitions` | a key is added that the reader refuses (the reader rejects unknown keys), or a key's meaning changes | a definition states the number it needs; the CLI reads every number it supports and names both numbers when it refuses one |
| `wire` | a command's request or reply changes meaning; adding a command keeps it (the unknown-command rule answers `E_UNSUPPORTED`), and no device sends unsolicited frames (the wire is request/response through the relay) | a node that does not implement a command answers `E_UNSUPPORTED`, which the host shows as unknown |
| `serve-api` | unchanged by the `event` message, an addition | consumers ignore unknown message types |

Formats before the first public release carry no compatibility promise:
the formats that ship in the first public release are each contract's
number 1, and nothing reads or writes a development format that preceded
them. From that release on, the rules above apply.

Component kinds: a change in a crate's public API that is incompatible
is breaking for that component; a new event, port, op, TLV id or
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

The boot core expects a slot with a signed header for this MCU (by its
identity, 5.5), target tag and slot, a body whose digest matches, a
vector table at the body start, and an app that feeds the watchdog the
boot core armed (on trial: confirm within the trial period or be rolled
back). An app that does not link DAGON can still be installed through
the ROM path and runs after signing; it never confirms, so it rolls back
after `max_attempts` unless installed as confirmed by `dagon provision`.
To attach DAGON: add the MCU and board packages, call `start` with an
extension chain (`Nop` is valid), read the published events through the
`Reader` it returns, keep the guard fed. To detach: stop calling `start`; the boot core still
verifies and counts attempts, still runs its own extensions and still
writes the boot record; the host sees no identity reply and `dagon
status` says so. No extension, event, definition or boot action changes
what the boot core checks, except a trusted extension that is declared
to (4.7).

## 8. Examples

### 8.1 Splash and update progress on a display (app)

```rust
#[embassy_executor::task]
async fn display_task(mut lcd: Lcd, events: Reader<AppSnapshot, 16>) {
    let mut rx = events.receiver().unwrap();
    loop {
        let s = rx.changed().await.state;
        match s.phase {
            Phase::Starting | Phase::Serving => lcd.splash(s.version, s.trial),
            Phase::Transferring => lcd.bar(s.progress),
            Phase::Verifying => lcd.text("verify"),
            Phase::RebootPending => lcd.text("reboot"),
            Phase::Confirmed => lcd.text("ok"),
            _ => {}
        }
    }
}
// main: let events = node::start::<Board, N>(spawner, dagon, &GUARD, tag, extensions![MyApp], UPLINK);
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

### 8.3 Logging every event, boot included (app or relay)

```rust
pub struct DefmtLog;
impl Extension for DefmtLog {
    fn id(&self) -> &'static str { "defmt-log" }
    fn on_event(&mut self, e: &Event<'_>, _: &Ctx) { defmt::info!("{}", defmt::Debug2Format(e)); }
}
// … extensions![DefmtLog, MyApp]
// The log shows the generated order of 1.3: app.starting, app.store_opened,
// the replayed boot (boot.entry … boot.before_jump, replayed: true),
// app.boot_replayed, app.trial_started, app.watchdog_taken, app.bring_up, app.link_up …
```

Or asynchronously, without touching the serving task's time: a task that
drains `events.recent()` from the `Reader` (owned `EventRecord`s, 2.4).

### 8.4 A replaced update transport and protocol: UDS over CAN

An automotive node is updated by a diagnostic tester speaking UDS
(ISO 14229) over CAN with ISO-TP (ISO 15765-2), not by DAGON's wire
over a serial link. One plug-in provides two ports and runs at boot and
in the app.

Manifest:

```toml
format = 1
id = "uds-can"
layers = ["boot", "app"]
provides = ["UpdateTransport", "UpdateProtocol", "Diagnostics"]
observes = ["app.transfer_*", "app.verify_*", "boot.slot_chosen"]
decides = []
trusted = false
[capabilities]
peripherals = ["can1"]       # declared in the board definition's peripheral table
timer = true
[limits]
max_run_us = 500
stack_bytes = 2048
ram_bytes = 8192
```

Boot stage. The boot core links the plug-in into `.ext.*`, and the
generated map (4.4) grants it the CAN1 register block, window-eligible in
the MCU definition, and its own memory; the flash
controller, the state log, the watchdog and DAGON's RAM are denied.
From `WatchdogArmed` the boot machine polls the `UpdateTransport` port
between its steps for a bounded window (`[boot] service_window_ms` in
the project, capped by the trial period): a tester that sends
`DiagnosticSessionControl(programmingSession)` keeps the boot core in
its recovery path (`boot.recovery { path: Service }`) and the plug-in's
`UpdateProtocol` maps `RequestDownload` → `update_offered`,
`TransferData` → `header_chunk`/`data_chunk`, `RequestTransferExit` →
`transfer_end`, `RoutineControl(switchSlot)` → `switch`, `ECUReset` →
`reboot`; the session machine runs every DAGON guard and the verifier
port on each trigger exactly as for the DAGON wire, so a tester cannot
skip the signature or write the active slot. The plug-in's `Diagnostics`
port answers `ReadDataByIdentifier` for the identifiers it declares
(the versions and the isolation level from the same values VERSION
carries). Each handler call runs under `max_run_us`; a stuck CAN
controller is an overrun event and the boot goes on to its decision
without the window.

App stage. The same crate, in the app's chain, provides the same ports
to the agent: the serving task reads frames from the `UpdateTransport`
(ISO-TP reassembly inside the plug-in), the `UpdateProtocol` turns UDS
services into session triggers, and the agent's replies go back the
same way. `accept_update` and `self_check` stay with the board package,
libraries or the app (`decides = []` here). At level `mpu` the handlers
run unprivileged with the CAN window as their only peripheral; at level
`none` the same code runs, and `dagon status` shows the node as
"extensions unguarded".

What the plug-in cannot do, at any level, through the API: reach the
slot store or the log (no handle), confirm a slot (not a decision it
declares), or change what the boot core starts. What it cannot do at
`mpu`/`pms` whatever its code does: write flash, touch the watchdog,
read DAGON's memory, or hang the boot.

### 8.5 A host action on a deploy result

`~/.dagon/actions/on-result.toml` as in 2.5; `dagon apply` then emits
`host.node.result` per node and `host.action.ran` after the program
finished. The same event appears on `/v1/events` and on stdout with
`--events`.

### 8.6 Confirming at start (bench use)

```rust
struct Bench;
impl Extension for Bench {
    fn id(&self) -> &'static str { "bench" }
    fn self_check(&mut self, _: &TrialInfo, _: &Ctx) -> Option<Health> { Some(Health::Healthy) }
}
// after start: if agent.images().on_trial() { agent.confirm_own(); }   // emits app.self_check, app.confirmed (and app.demoted for the other slot)
```

## 9. Alternatives rejected

| Alternative | Why rejected |
|---|---|
| A closed, DAGON-interpreted op set as the only boot-stage extension | Needs are open-ended (a diagnostic transport such as UDS, added security processing, a display, a different storage); every new need would be a DAGON change. Kept as the simplest extension kind for no-code uses; general needs are code behind ports in a hardware-enforced sandbox. |
| Arbitrary code at boot with lints, dependency allow-lists and conformance scans as the barrier | Those inspect source, not what runs; a callback could loop until the watchdog resets the chip, write any register or flash. The barrier is the MCU's protection unit plus a preempting timer (4.1), and where an MCU has neither, the definition says `none` and the product says so. |
| A boot-time veto answered by an extension | Any untrusted answer that turns a `Boot` into a rollback lets a faulty extension keep a valid image from ever running. Boot policy is DAGON's `DecisionPolicy` port; replacing it is a trusted extension (4.7), declared, signed and shown. |
| Public slot store mutators with the agent as one user among others | Persistent image state could then change without its event and without the confirm rule; the image-lifecycle machine is the only writer. |
| A fixed hook trait extended with a method per new timing | No composition, no payload stream for displays, no replacement of parts, and the list must be fixed again at every timing not foreseen. Kept only as a deprecated facade. |
| A hand-maintained event enum checked against a hand-maintained catalog | Two lists can agree while a transition emits nothing; only a primitive that cannot change state without emitting makes the catalog derived. |
| Linker-section registries (`linkme`) | Entry order undefined; hidden registration by libraries; `dyn` dispatch cost and size. |
| `dyn Extension` list with `alloc` on the device | No alloc below the host; a static slice reintroduces the registry problem. |
| Channels for everything | Decisions and ports need a synchronous answer; a queue is the right tool only for the published state. |
| Retaining borrowed payloads in the waitable ring | A static ring cannot hold references past the producer; the generated owned compact form (2.4) is bounded and complete enough for display and log tasks. |
| Unsolicited event frames from devices to the host | The wire is request/response relayed hop by hop by the hub; a frame nobody asked for has no route, no sequence to retry on and no place in the relay's quiet-window model. The `EVENT` frame flag is reserved and unused; the host derives every device transition from replies, VERSION and the boot record. |
| Board values through build-time environment variables | MCU knowledge in disguise, in no definition; replaced by `[boot]` in the board definition. |
| Reporting the boot core version the app was compiled against | Does not identify what is installed; VERSION reads the installed image from flash. |
| A fixed reboot-delay cap | A number chosen once is wrong for an MCU with a shorter trial watchdog; the cap derives from the MCU definition's periods. |
| A boot record with CRC-covered headers updated in place | A count, state or CRC updated in place cannot be made atomic against a reset: a torn update invalidates every event the header covers; a hand-off that locates another writer's section by its header couples the writers' formats. |
| Specifying the retained-memory and identity formats in prose | A format whose correctness depends on every interleaving of resets, partial writes and version skew is shown correct by an executable model with randomized fault injection, not by prose (5.5). |
| A built-in example topology and `demo` command in the CLI | Trying DAGON without hardware is the simulator's job (`dagon sim` runs any topology with its definitions); a browser try page is a separate published page. DAGON holds no board or topology of its own. |

## 10. Size limits and where they come from

Every limit has one source of truth; every place that must fit it is
listed, and a test asserts each pair.

| Limit | Value | Source of truth | Must fit it |
|---|---|---|---|
| Link payload per kind | USB 1088, UART 1024, I2C 256 bytes | `dagon_proto::max_payload` | every request and reply; `RECORD_PAGE`; VERSION response; display text; conformance detail |
| Smallest link payload | `max_payload::MIN` = the minimum over the kinds above (derived, not written) | `dagon-proto` | VERSION response as a whole (every TLV set the device may answer, asserted with the largest 0x08 set, 0x20 and a full 0x21 page) |
| TLV value | ≤ 255 bytes (`len u8`) | VERSION encoding (`dagon-proto` payload) | every id in 5.4; 0x21 pages |
| VERSION 0x21 fixed overhead | `VERSION_PAGE_OVERHEAD = 6` = status (1) + TLV count (1) + id (1) + len (1) + offset (2) | `dagon-proto` constant with a `const` assertion that it equals the sum of the field widths | `RECORD_PAGE` |
| `RECORD_PAGE` per link | `RECORD_PAGE(link) = min(255 − 2, max_payload(link) − VERSION_PAGE_OVERHEAD)`: I2C 250, UART 253, USB 253; `RECORD_PAGE_MIN = 250` | `dagon-proto` `const fn` with one `const` assertion per link kind | VERSION 0x21; the host's paged reader asks at most `RECORD_PAGE_MIN` |
| Boot record region | `bytes ≤ RECORD_MAX = 4096` (so `offset u16` of 0x21 addresses all of it); internal bounds are the model's constants (5.5) | `dagon_format::bootrecord::RECORD_MAX`; the MCU definition's `[boot.record]` | the definition validator; every image's `memory.x`; the agent's reader |
| Service non-preemptible part | `np_max_us` per service and kind, `COPY_MAX`, `BUS_MAX`, `dma_stop_max_us` | the port catalog and the kind capability records (4.4), asserted against the implementation's measured worst case | the service implementations; the timer mask; validation 5 of 4.2 |
| Extension time budget | `max_run_us ≤ BOOT_EXT_MAX_US`, `≤ APP_EXT_MAX_US`; `Σ (calls × budget)` over a boot ≤ `trial_ms / 4` | `dagon-events` `const fn` from the MCU definition's `[watchdog]`; `boot_wiring.rs` | `dagon gen` validator; the sandbox timer; the manifest |
| Extension memory | `stack_bytes + ram_bytes ≤` the MCU definition's `[isolation] ext_ram_max`; entries ≤ the kind's budget after code, data and stack | the MCU definition; `boot_wiring.rs` protection map | `dagon gen` validator; `memory.x` |
| Extension count and ids | ids ≤ 32 bytes; 0x08 repeats per extension and the whole VERSION reply must still fit the smallest link | the manifest format; `max_payload::MIN` | `dagon gen` refuses a project whose VERSION reply would not fit |
| Boot action program | `ops ≤ 32` per event, `delay total ≤ 50 ms`, `bytes ≤ 8` per `bus_write`, `delay ≤ 20 ms` | `dagon_format::bootprogram` constants | `dagon gen` validator; the boot core interpreter; the `boot_wiring.rs` table type |
| Live event payload | ≤ 16 bytes for boot events (by value); larger app/host payloads by reference | `dagon-events` size assertions | `Published` snapshot; the compact form |
| `EventRecord` | 24 bytes: `layer`, `kind`, `flags`, `dropped`, `seq u32`, `payload [u8; 16]` | `state_machine!` `compact` attribute (a generated test asserts every kind has one) | `Recent<N>`; the boot record's compact form reuses the first 3 payload bytes |
| Catalog | no numeric cap; `kind` is `u8` per layer | `state_machine!` | the record `kind u8` (a generated test fails at 256 kinds in a layer); serve stream |
| Recent ring | `N` chosen by the layer package | `Published<S, N>` | the layer package's `Published`; overflow counts `dropped` |
| Decision bounds | 50 ms; `REBOOT_DELAY_MAX = min(trial_ms, confirmed_ms) / 2` | `dagon-events` constants; the MCU definition's `[watchdog]` | the serving task's timer; `boot_wiring.rs` |

## 11. Bus masters and privileged control blocks: the definition schema

Every MCU definition lists every register block the MCU has
(`[[peripheral]]`, 4.4) with `window_eligible`, `bus_master` and
`privileged_why`; the checker refuses a definition whose kind is not
`none` and that lacks a block of each required class, or that marks a
bus master window-eligible. The generated map (4.4), the conformance
cases (4.9) and the table `dagon mcu show <id>` prints all derive from
that list; nothing below is maintained by hand elsewhere. The three
tables here are the lists of the three shipped definitions, as examples
of the judgement the schema asks a definition author to make: can the
block write memory, and how does the design keep it privileged.

**STM32F401RE**

| Block | Writes memory | Kept privileged by |
|---|---|---|
| DMA1, DMA2 (`0x4002_6000`, `0x4002_6400`) | yes (bus masters) | `bus_master = true`, `privileged_why = "dma"`; never a window; extension DMA only through `DMA_*` services with bounce buffers |
| USB OTG FS (`0x5000_0000`) | no system-RAM DMA on this part (FIFO-based), but it is DAGON's link | `shared`; never a window; the agent owns it |
| Flash interface (`0x4002_3C00`) | no (programs flash) | `flash` |
| IWDG, WWDG | no | `watchdog` |
| RCC, PWR, RTC/backup | no | `clock`, `reset`, `power` |
| SYSCFG, EXTI, NVIC/SCB (PPB) | no | `irq-routing`, `pinmux`; the PPB is privileged by the architecture |
| CRC | no | `shared` (DAGON uses it) |
| System memory (ROM loader) | no | `rom`: not executable from unprivileged mode (background map) |
| USART/SPI/I2C/CAN/TIM/ADC/GPIO blocks not used by DAGON on the board | no | `window_eligible = true` where self-contained; GPIO is a service (shared bank with DAGON's LED) |

**RP2040**

| Block | Writes memory | Kept privileged by |
|---|---|---|
| DMA (`0x5000_0000`) | yes | `dma`, bus master; services only |
| USB (`0x5010_0000`) | into its own DPRAM only, not system RAM; DAGON's link | `shared`; never a window |
| XIP control, SSI (`0x1400_0000`, `0x1800_0000`) | no (programs flash through the SSI) | `flash` |
| Boot ROM (`0x0000_0000`) | its flash routines drive the SSI and DMA | `rom`: ROM execution is denied as a whole from unprivileged mode; the routines' addresses are irrelevant to the barrier |
| WATCHDOG | no | `watchdog` |
| CLOCKS, RESETS, XOSC, PLLs, ROSC, VREG | no | `clock`, `reset`, `power` |
| PSM, SIO FIFO and spinlocks | core-1 launch and inter-core | `core-control`; the second core is never started |
| SIO GPIO out/oe | no | `shared` (DAGON's LED); GPIO is a service |
| IO_BANK0, IO_QSPI, PADS | no | `pinmux` |
| PIO0, PIO1 | no (FIFOs; a PIO program cannot access system memory) | `window_eligible = true` when the board does not use them for DAGON's links; else `shared` |
| UART0/1, SPI0/1, I2C0/1, PWM, ADC, TIMER | no | eligible where not DAGON's; TIMER is `shared` |
| Core 1 | a second CPU, unconstrained by core 0's MPU | `[[core]]`; never started; its launch blocks privileged |

**ESP32-S3**

| Block | Writes memory | Kept privileged by |
|---|---|---|
| GDMA | yes, on behalf of SPI2/3, I2S, UHCI, LCD_CAM, ADC, AES, SHA | `dma`, bus master; GDMA permission control denies World 1 per channel; services only |
| USB OTG | yes (its own DMA) | `shared`, bus master; never a window |
| USB Serial/JTAG | no DMA; DAGON's link | `shared` |
| SPI0/SPI1 (cache and flash), the MMU | no (programs flash; maps memory) | `flash`, `protection` |
| AES, SHA, RSA, HMAC, Digital Signature engines | through GDMA (AES/SHA) | `shared`/bus masters; a `CryptoProvider` is a trusted port (2.1) |
| RTC WDT, TIMG WDTs, super watchdog | no | `watchdog` |
| RTC_CNTL, system registers, interrupt matrix, IO MUX, GPIO matrix | no | `power`, `reset`, `clock`, `irq-routing`, `pinmux` |
| PMS and WCL registers | no | `protection`; locked before World 1 |
| Wi-Fi and Bluetooth MACs | yes (bus masters) | bus masters; never enabled by DAGON; privileged |
| Core 1 | a second CPU | `[[core]]`; the map is applied on both cores |
| UART1/2, SPI2/3 without DMA, I2C, TWAI, PWM, RMT, PCNT not used by DAGON | no | eligible where self-contained; TWAI (CAN) is the natural window for a UDS transport |
