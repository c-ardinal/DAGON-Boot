# Events

Generated from the state machines' catalogs; do not edit. Regenerate with
`DAGON_BLESS=1 cargo test -p dagon-catalog --test docs`.

Every state change in DAGON is a transition of a state machine, and every
transition emits one event. This page is generated from the machines'
declarations, so it lists every event there is.

Event format: 1 (`dagon_events::EVENT_FORMAT`). Adding a kind or a payload
field keeps the number; renaming or removing one, or changing its meaning, moves
it. A reader ignores kinds and fields it does not know and refuses a format
number it does not read.

## Reading the tables

- **Kind**: `layer.kind`, the payload type's name in snake case.
- **Code**: the kind's number within its layer (`EventRecord.kind`).
- **Transition**: the method that emits it, and the states it moves between
  (`*` is any state).
- **Payload**: the fields, each with its bytes in the owned record
  (`EventRecord.payload`, in order, 16 bytes at most).
- **Observed**: *live* by extensions in the same image as it happens; *replay*
  re-emitted by the app's agent at start with `replayed: true`; *host* by the
  CLI, the serve event stream, `--events` and actions.
- **Notes**: *action*: a boot action program may run on it; *retired*: no longer
  emitted, kept until the next event format.

## Decision points

Everything not listed here is a notification. Each answer is the first one in
the chain's order (publisher, board package, libraries, user); when every
extension abstains, the default applies.

| Decision | Layer | Question | Answered by | Bound | Default | Never |
|---|---|---|---|---|---|---|
| `accept_update` | app | `UpdateOffer` -> `Accept` | board package, libraries, user (first answer wins) | each call within `DECISION_BOUND_MS` (a later answer is ignored); asked after every DAGON guard passed, before erase | accept | accepting what DAGON refused (not asked then) |
| `allow_reboot_now` | app | `RebootRequest` -> `Delay` | board package, libraries, user (first answer wins) | each call within `DECISION_BOUND_MS`; `Now` or `After(ms)`, ms capped at half the shorter watchdog period of the MCU definition; DAGON replies first and keeps feeding while it waits | now | refusing a reboot; a delay beyond the cap |
| `self_check` | app | `TrialInfo` -> `Health` | board package, libraries, user (first answer wins) | each call within `DECISION_BOUND_MS` (a later answer is ignored); `Pending` keeps the "state" status and the host retries | healthy | confirming a slot not on trial; confirming outside the `confirmed` transition |
| `confirm_plan` | host | the deploy listing -> yes or no | the interactive user (terminal, page) or `--yes`; never actions or host extensions | until the user answers | ask | skipping the key check and image verification, done before the listing is shown |

## Layer `app`

### Machine `decisions`

Decision calls to extensions and their bound.

States: `Ready` (initial).

| Kind | Code | Transition | Payload | Observed | Notes | Description |
|---|---|---|---|---|---|---|
| `app.extension_overran` | 0xf0 | `overran`: Ready → Ready | `id: ExtId` (4 B), `point: DecisionId` (1 B), `us: u32` (4 B) | live |  | An extension answered a decision point after `DECISION_BOUND_MS`; its answer was ignored. |
