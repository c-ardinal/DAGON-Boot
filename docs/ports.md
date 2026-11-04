# Ports

Generated from `dagon_events::PORTS`; do not edit. Regenerate with
`DAGON_BLESS=1 cargo test -p dagon-catalog --test docs`.

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

| Port | What it does | Layers and default | Trust | Services |
|---|---|---|---|---|
| `UpdateTransport` | bytes in and out of a link | boot: the links the MCU package provides; app: the links the MCU package provides; relay: the links the MCU package provides; host: serial | untrusted | return, transport, dma, time, memory |
| `UpdateProtocol` | messages to and from session triggers: identity, status, transfer, switch, reboot, confirm; the session machine runs every guard, so a protocol cannot skip one | boot: the DAGON wire; app: the DAGON wire; relay: the DAGON wire; host: the DAGON wire | untrusted | return, time, memory |
| `ImageVerifier` | header signature and key lookup | boot: Ed25519 over the project key; app: Ed25519 over the project key | trusted only | – |
| `Digest` | image body hash | boot: SHA-256; app: SHA-256 | trusted only | – |
| `CryptoProvider` | the primitives the verifier and the digest use (hardware acceleration, a secure element) | boot: software; app: software; host: software | trusted only | – |
| `DecisionPolicy` | the attempt limit and which slot boots | boot: `max_attempts` from the definition, pending before confirmed | trusted only | – |
| `WatchdogDriver` | the watchdog that guards boot and trial | boot: the MCU package's; app: the MCU package's | trusted only | – |
| `SlotStore` | where images live | boot: the MCU package's flash layout; app: the MCU package's flash layout | trusted only | – |
| `LogStorage` | where the state log lives | boot: the MCU package's flash layout; app: the MCU package's flash layout | trusted only | – |
| `Recovery` | an additional recovery path: a service window for a transport, a vendor download mode | boot: the recovery paths the MCU and board definitions declare; app: the recovery paths the MCU and board definitions declare | untrusted, additive | return, transport, time, memory |
| `Diagnostics` | extra VERSION entries, extra status, a diagnostic identifier table | boot: none; app: none; relay: none | untrusted | return, time, memory |
| `Display` | what a phase or a boot outcome looks like | boot: the board definition's `[boot]` LED and halt pattern, through the action program; app: none; relay: none | untrusted | return, pins, buses, time, memory |
| `LogSink` | where overflow of the recent ring and extension logs go | app: none; relay: none; host: none | untrusted | return, events, time, memory |
| `SelfCheck` | the health answer when a slot on trial is confirmed | app: healthy | untrusted | return, time, memory |
| `BoardBringUp` | the board's power, pins and buses at app start | app: the board package's; relay: the board package's | untrusted | return, pins, buses, time, memory |
| `ChildLink` | how a hub reaches a child | relay: the links the MCU package provides | untrusted | return, buses, transport, dma, time, memory |
| `Signer` | signing images and releases | host: the project's key pair file | untrusted | – |
| `KeyStore` | where the signing key is kept | host: the project's key pair file | untrusted | – |
| `BuildRunner` | building node images | host: cargo | untrusted | – |
| `Agent` | an external agent the CLI and the page hand requests to | host: agent definitions | untrusted | – |
| `Action` | a program or request run after a host event | host: action definitions | untrusted | – |

## Parts that are not ports

No plug-in replaces these, trusted or not.

| Part | What it is | Layers | Why not a port |
|---|---|---|---|
| `ImageLifecycle` | the image-lifecycle machine over the slot store and the state log | boot, app | the only writer of persistent image state; every change emits its event and keeps the confirm rule |
| `SessionGuards` | the update session's guards: target, slot, header signature and compatibility, body digest | boot, app | every protocol's triggers pass the same guards; replacing them would let a transport write what DAGON refuses |
| `BootRecord` | the retained record of boot events the app replays | boot, app | its format is DAGON's retained contract and it is never an input to a boot or confirm decision |
| `Sandbox` | the entry gate, the service calls, the protection map and the lockup fuse | boot, app, relay | it is what keeps extensions away from DAGON's state |
