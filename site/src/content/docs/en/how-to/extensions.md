---
title: "Writing an extension"
description: "Observe update events and answer decision points in the app, and replace a port."
---

An extension is your own Rust code that DAGON calls.
It runs in two places: in the app of a node, and on the host (the PC that builds, signs and writes).

DAGON does not isolate an extension.
It runs with the rights of the code around it: in the app it reaches everything the app reaches, and on the host it runs with your rights.
Reviewing an extension's content, the crates it depends on and the source it takes in is your responsibility (see [Limitations](/en/explanation/limitations/)).

## In the app

An extension in the app does two things.

- It observes every `app.*` event of the node ([Events](/en/reference/events/)).
- It answers the decision points: whether an update may start, whether a reboot may happen now, and whether a version on trial is healthy.

### 1. Add the dependencies

In the node's `Cargo.toml`, add `dagon-events`, `dagon-agent` and `static_cell`.
Take the two DAGON packages from the same source as the `dagon-mcu-...` line of that file (the same `git` and `tag`, or a `path` into the same DAGON source):

```toml nocheck
dagon-events = { git = "<as the dagon-mcu line>", tag = "<as the dagon-mcu line>" }
dagon-agent = { git = "<as the dagon-mcu line>", tag = "<as the dagon-mcu line>" }
static_cell = "2"
```

`dagon-events` holds the `Extension` trait.
`dagon-agent` holds the types of the event payloads (`dagon_agent::image::Confirmed` and so on).

### 2. Write the extension

Only `id` is required.
Every other method has a default: `on_event` does nothing, and a decision method returns `None`, which abstains.

```rust
use dagon_agent::image;
use dagon_events::{Accept, Ctx, Delay, Event, Extension, Health, RebootRequest, TrialInfo, UpdateOffer};

/// Keeps an update away while the motor runs, and refuses to confirm a
/// new version whose sensor does not answer.
pub struct Machine {
    pub motor_running: fn() -> bool,
    pub sensor_ok: fn() -> bool,
}

impl Extension for Machine {
    fn id(&self) -> &'static str {
        "machine"
    }

    fn on_event(&mut self, e: &Event<'_>, _: &Ctx<'_>) {
        if let Some(c) = e.get::<image::Confirmed>() {
            defmt::info!("slot {} confirmed", c.slot);
        }
    }

    fn accept_update(&mut self, _: &UpdateOffer, _: &Ctx<'_>) -> Option<Accept> {
        (self.motor_running)().then_some(Accept::No)
    }

    fn allow_reboot_now(&mut self, _: &RebootRequest, _: &Ctx<'_>) -> Option<Delay> {
        (self.motor_running)().then_some(Delay::After(2_000))
    }

    fn self_check(&mut self, _: &TrialInfo, _: &Ctx<'_>) -> Option<Health> {
        (!(self.sensor_ok)()).then_some(Health::Unhealthy)
    }
}
```

`e.get::<T>()` gives the payload when the event is of kind `T`.
The kinds and their fields are in [Events](/en/reference/events/).

### 3. Hand it to DAGON

`src/app.rs` of a project made by `dagon new` has a function `dagon()` that returns what the app hands DAGON's start.
Put the extensions in a `static` and name them in `exts`:

```rust
use dagon_events::{extensions, Chain, Nop};
use static_cell::StaticCell;

static EXTS: StaticCell<Chain<Machine, Nop>> = StaticCell::new();

pub fn dagon() -> App {
    let exts = EXTS.init(extensions![Machine { motor_running, sensor_ok }]);
    App { exts: Some(exts), ..App::new(self_check) }
}
```

`extensions![a, b, c]` makes one chain of several extensions: its type is `Chain<A, Chain<B, Chain<C, Nop>>>`.
`dagon gen` never rewrites `src/app.rs`.

### The decision points

| Method | When DAGON asks | Answers | Without an answer |
|---|---|---|---|
| `accept_update` | An update request passed every guard of DAGON, before the slot is erased | `Accept::Yes`, `Accept::No` | The update starts |
| `allow_reboot_now` | DAGON accepted a reboot | `Delay::Now`, `Delay::After(ms)` | The reboot happens now |
| `self_check` | The host asks to confirm a version on trial | `Health::Healthy`, `Health::Unhealthy`, `Health::Pending` | `self_check()` of `src/app.rs` answers |

- DAGON asks the board's extension first, then yours in the order of `extensions![..]`. The first answer stands.
- `self_check()` of `src/app.rs` is asked last and always answers, so an extension that returns `None` leaves the decision to it.
- An answer has to come within `DECISION_BOUND_MS` (50 ms). A later answer is ignored, and `app.extension_overran` names the extension. Do not wait or block in a method: keep the state ready and return.
- `Accept::No` cannot be overruled by a later extension, and no answer makes DAGON accept what its own guards refused: an image with a wrong signature, target or digest never reaches `accept_update`.
- `Delay::After` is capped by the watchdog period of the node.
- `Health::Pending` keeps the version on trial, and the host asks again. A version on trial that is never confirmed is rolled back.

### Drawing and logging from a task

An extension's methods run in DAGON's serving task, so they have to return fast.
For work that takes time (a display, a log over a slow link), use the events the start returns (`AppEvents`) from a task of your own: it waits for the latest state and reads the recent events.
[Lifecycle events and extensions](/en/explanation/lifecycle-events/) shows a display task in section 8.1.

## Replacing a port

A port is a part of DAGON behind a Rust trait, with DAGON's own implementation as the default.
Two ports are replaced without code. The others are replaced in a program of your own that uses DAGON's crates: the `dagon` command uses the defaults.

| Port | Trait | DAGON's default | How to replace it |
|---|---|---|---|
| `KeyStore` | `dagon_sign::KeyStore` | A key file | No code: set `DAGON_SIGNING_KEY_COMMAND` to a command that prints the key (see [From a new project to a new version](/en/getting-started/first-project/)). In your own program: implement `key()`. |
| `Agent` | `dagon_host::ports::Agent` | The linked agent of the agent definitions | No code: add an agent definition file ([Agent definition format](/en/reference/agent-definition/)). In your own program: any `FnMut(&Prompt) -> Result<String, String>` is one. |
| `Signer` | `dagon_sign::Signer` | An Ed25519 key pair in memory | Implement `public_key()` and `sign()`, and pass it to `dagon_sign::image::sign_image` and `dagon_sign::release::assemble`. |
| `BuildRunner` | `dagon_host::ports::BuildRunner` | `cargo build` | Implement `build()`: it builds one node image and returns the executable. |
| `CryptoProvider`, `Digest` | `dagon_format::CryptoProvider`, `dagon_format::Digest` | SHA-256 and SHA-512 in software (`dagon_format::Software`) | Implement the two hashes and pass the provider to `dagon_agent::Agent::with_crypto` and `dagon_ed25519::Ed25519Small::with_crypto`. |
| `DecisionPolicy` | `dagon_core::boot::DecisionPolicy` | The pending slot before the confirmed one, at most `max_attempts` times | Implement `trial()` and pass it to `dagon_core::boot::decide_with`. |

A signer whose key stays in an HSM:

```rust
use dagon_sign::Signer;

pub struct Hsm {
    pub public_key: [u8; 32],
}

impl Signer for Hsm {
    fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    /// Ed25519 over `domain` followed by `message`.
    fn sign(&self, domain: &[u8], message: &[u8]) -> [u8; 64] {
        hsm_sign_ed25519(&[domain, message].concat())
    }
}
```

`hsm_sign_ed25519` stands for the call of your HSM.

A replaced port has to keep what the default does: a `Signer` signs `domain` followed by `message` with Ed25519 under the key `public_key()` returns, and a `CryptoProvider` returns the same digests as the software one.
DAGON does not check a replacement against its default. The node checks every image's signature and digest with its own code, so a host port that signs or hashes wrongly makes images the node refuses.

## Before you use an extension

- Read its source and the crates it depends on. It runs with the rights of the app or of your account on the host.
- In the app, check that each method returns within the bound, without waiting on a bus, a lock or a timer.
- Check what `accept_update` and `self_check` answer when the extension's own input is missing: `None` leaves the decision to the next one, and `Accept::No` or `Health::Unhealthy` stops the update or the confirm.
- On a board, run it through one update that confirms and one that rolls back.
