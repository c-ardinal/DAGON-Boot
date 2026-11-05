---
title: Ports
description: Every port DAGON's extensions use, from the catalogs.
editUrl: false
---

:::note
Generated from `docs/ports.md`; do not edit. Regenerate with `python site/scripts/gen.py`.
:::

A port is a part of DAGON that a plug-in replaces, in the layers listed, for
the image it is linked into; DAGON's own implementation is the default. A
plug-in's manifest may provide a port only in layers the port exists in, and
one image has at most one provider per port.

Trust:

- *untrusted*: any plug-in; it runs in the layer's sandbox with the
  capabilities its manifest declares and calls DAGON only through the
  services listed.
- *untrusted, additive*: as untrusted, and it only adds; DAGON's own
  implementation stays.
- *trusted only*: only a trusted extension, declared and signed; it runs
  privileged and calls no services.

| Port | What it does | Layers | Default | Trust | Services |
|---|---|---|---|---|---|
| `UpdateTransport` | bytes in and out of a link | boot, app, relay, host | boot, app, relay: USB CDC, UART, I2C target, PIO UART; host: serial | untrusted | return, transport, dma, time, memory |
| `UpdateProtocol` | messages ↔ session triggers: identity, status, transfer, switch, reboot, confirm | boot, app, relay, host | the DAGON wire | untrusted (it cannot skip a guard: the session machine runs the guards, not the protocol) | return, time, memory |
| `ImageVerifier` | header signature, key lookup | boot, app | Ed25519 over the project key | trusted only | – |
| `Digest` | body hash | boot, app | SHA-256 | trusted only | – |
| `CryptoProvider` | the primitives the verifier and digest use; hardware acceleration, HSM | boot, app, host | software | trusted only | – |
| `DecisionPolicy` | attempt limit, which slot wins | boot | `max_attempts` from the definition, pending before confirmed | trusted only | – |
| `WatchdogDriver` | the watchdog that guards boot and trial | boot, app | the MCU's | trusted only | – |
| `SlotStore` | where images live | boot, app | the MCU package's flash layout | trusted only | – |
| `LogStorage` | where the state log lives | boot, app | the MCU package's flash layout | trusted only | – |
| `Recovery` | an additional recovery path: a service window for a transport, a vendor download mode | boot, app | BOOTSEL/UF2, factory, ROM entry as the MCU and board define | untrusted, additive (a plug-in adds a path, never removes DAGON's) | return, transport, time, memory |
| `Diagnostics` | extra VERSION TLVs, extra status, a UDS DID table | boot, app, relay | none | untrusted | return, time, memory |
| `Display` | what a phase or a boot outcome looks like | boot, app, relay | boot: the `[boot] led` and `halt` of the board definition through the action program; app, relay: none | untrusted (at boot it runs in the sandbox with the declared pins and buses) | return, pins, buses, time, memory |
| `LogSink` | where `Recent` overflow and extension logs go | app, relay, host | none | untrusted | return, events, time, memory |
| `SelfCheck` | the health answer when a slot on trial is confirmed | app | healthy | untrusted (a decision point, 3) | return, time, memory |
| `BoardBringUp` | the board's power, pins and buses at app start | app, relay | the board package's | untrusted | return, pins, buses, time, memory |
| `ChildLink` | how a hub reaches a child | relay | I2C controller, PIO UART | untrusted | return, buses, transport, dma, time, memory |
| `Signer` | signing images and releases | host | file key pair | untrusted (the host is trusted by definition; the device verifies) | – |
| `KeyStore` | where the signing key is kept | host | file key pair | untrusted (the host is trusted by definition; the device verifies) | – |
| `BuildRunner` | building node images | host | cargo | untrusted | – |
| `Agent` | an external agent the CLI and the page hand requests to | host | data-defined (agent definitions) | untrusted | – |
| `Action` | a program or request run after a host event | host | data-defined (action definitions) | untrusted | – |

## Parts that are not ports

No plug-in replaces these, trusted or not.

| Part | What it is | Layers | Why not a port |
|---|---|---|---|
| `ImageLifecycle` | the image-lifecycle machine over the slot store and the state log | boot, app | the only writer of persistent image state; every change emits its event and keeps the confirm rule |
| `SessionGuards` | the update session's guards: target, slot, header signature and compatibility, body digest | boot, app | every protocol's triggers pass the same guards; replacing them would let a transport write what DAGON refuses |
| `BootRecord` | the retained record of boot events the app replays | boot, app | its format is DAGON's retained contract and it is never an input to a boot or confirm decision |
| `Composition` | the order of a layer's chain (publisher, board package, libraries, user) and the decision bounds | boot, app, relay, host | it is what makes every extension see every event and keeps answers bounded |
| `Sandbox` | the entry gate, the service calls, the protection map and the lockup fuse | boot, app, relay | it is what keeps extensions away from DAGON's state |
