//! The port catalog: every part of DAGON a plug-in can replace, and the
//! parts it cannot, one record each. `docs/ports.md`, the manifest
//! validator and the sandbox service tables derive from it.

use crate::catalog::Layer;

/// Who may replace a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    /// Any plug-in; it runs in the layer's sandbox with the capabilities
    /// its manifest declares.
    Untrusted,
    /// Any plug-in, and it only adds: DAGON's own implementation stays.
    Additive,
    /// Only a trusted extension, declared and signed; it runs privileged.
    TrustedOnly,
    /// Not a port: no plug-in replaces it, for the reason given.
    Never(&'static str),
}

impl Trust {
    pub const fn is_port(self) -> bool {
        !matches!(self, Trust::Never(_))
    }
}

/// The kinds of service a sandboxed handler calls (4.2 of the design).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Return,
    Events,
    Pins,
    Buses,
    Transport,
    Dma,
    Time,
    Memory,
}

impl Service {
    pub const fn name(self) -> &'static str {
        match self {
            Service::Return => "return",
            Service::Events => "events",
            Service::Pins => "pins",
            Service::Buses => "buses",
            Service::Transport => "transport",
            Service::Dma => "dma",
            Service::Time => "time",
            Service::Memory => "memory",
        }
    }
}

/// A layer the port exists in, and DAGON's implementation there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortDefault {
    pub layer: Layer,
    pub default: &'static str,
}

/// One port (or one part that is not a port).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Port {
    pub id: &'static str,
    pub what: &'static str,
    pub layers: &'static [PortDefault],
    pub trust: Trust,
    /// Why the trust policy holds, when it needs saying.
    pub note: &'static str,
    /// The services a sandboxed implementation calls. Privileged
    /// (trusted-only) and host implementations call none.
    pub services: &'static [Service],
}

impl Port {
    pub fn in_layer(&self, layer: Layer) -> bool {
        self.layers.iter().any(|d| d.layer == layer)
    }

    /// The port named `id` (parts that are not ports are not found).
    pub fn find(id: &str) -> Option<&'static Port> {
        PORTS.iter().find(|p| p.id == id && p.trust.is_port())
    }
}

const fn on(layer: Layer, default: &'static str) -> PortDefault {
    PortDefault { layer, default }
}

const fn port(
    id: &'static str,
    what: &'static str,
    layers: &'static [PortDefault],
    trust: Trust,
    note: &'static str,
    services: &'static [Service],
) -> Port {
    Port {
        id,
        what,
        layers,
        trust,
        note,
        services,
    }
}

const LINKS: &str = "USB CDC, UART, I2C target, PIO UART";
const WIRE: &str = "the DAGON wire";
const NONE: &str = "none";
const DAGON: &str = "DAGON's";
const FLASH: &str = "the MCU package's flash layout";
const RECOVERY: &str = "BOOTSEL/UF2, factory, ROM entry as the MCU and board define";
const BOARD: &str = "the board package's";
const KEY: &str = "file key pair";

use Layer::{App, Boot, Host, Relay};
use Service as S;
use Trust::{Additive, Never, TrustedOnly, Untrusted};

const SANDBOXED: &[Service] = &[S::Return, S::Time, S::Memory];

/// Every port and every part that is not one.
pub const PORTS: &[Port] = &[
    port(
        "UpdateTransport",
        "bytes in and out of a link",
        &[on(Boot, LINKS), on(App, LINKS), on(Relay, LINKS), on(Host, "serial")],
        Untrusted,
        "",
        &[S::Return, S::Transport, S::Dma, S::Time, S::Memory],
    ),
    port(
        "UpdateProtocol",
        "messages ↔ session triggers: identity, status, transfer, switch, reboot, confirm",
        &[on(Boot, WIRE), on(App, WIRE), on(Relay, WIRE), on(Host, WIRE)],
        Untrusted,
        "it cannot skip a guard: the session machine runs the guards, not the protocol",
        SANDBOXED,
    ),
    port(
        "ImageVerifier",
        "header signature, key lookup",
        &[
            on(Boot, "Ed25519 over the project key"),
            on(App, "Ed25519 over the project key"),
        ],
        TrustedOnly,
        "",
        &[],
    ),
    port(
        "Digest",
        "body hash",
        &[on(Boot, "SHA-256"), on(App, "SHA-256")],
        TrustedOnly,
        "",
        &[],
    ),
    port(
        "CryptoProvider",
        "the primitives the verifier and digest use; hardware acceleration, HSM",
        &[on(Boot, "software"), on(App, "software"), on(Host, "software")],
        TrustedOnly,
        "",
        &[],
    ),
    port(
        "DecisionPolicy",
        "attempt limit, which slot wins",
        &[on(
            Boot,
            "`max_attempts` from the definition, pending before confirmed",
        )],
        TrustedOnly,
        "",
        &[],
    ),
    port(
        "WatchdogDriver",
        "the watchdog that guards boot and trial",
        &[on(Boot, "the MCU's"), on(App, "the MCU's")],
        TrustedOnly,
        "",
        &[],
    ),
    port(
        "SlotStore",
        "where images live",
        &[on(Boot, FLASH), on(App, FLASH)],
        TrustedOnly,
        "",
        &[],
    ),
    port(
        "LogStorage",
        "where the state log lives",
        &[on(Boot, FLASH), on(App, FLASH)],
        TrustedOnly,
        "",
        &[],
    ),
    port(
        "Recovery",
        "an additional recovery path: a service window for a transport, a vendor download mode",
        &[on(Boot, RECOVERY), on(App, RECOVERY)],
        Additive,
        "a plug-in adds a path, never removes DAGON's",
        &[S::Return, S::Transport, S::Time, S::Memory],
    ),
    port(
        "Diagnostics",
        "extra VERSION TLVs, extra status, a UDS DID table",
        &[on(Boot, NONE), on(App, NONE), on(Relay, NONE)],
        Untrusted,
        "",
        SANDBOXED,
    ),
    port(
        "Display",
        "what a phase or a boot outcome looks like",
        &[
            on(
                Boot,
                "the `[boot] led` and `halt` of the board definition through the action program",
            ),
            on(App, NONE),
            on(Relay, NONE),
        ],
        Untrusted,
        "at boot it runs in the sandbox with the declared pins and buses",
        &[S::Return, S::Pins, S::Buses, S::Time, S::Memory],
    ),
    port(
        "LogSink",
        "where `Recent` overflow and extension logs go",
        &[on(App, NONE), on(Relay, NONE), on(Host, NONE)],
        Untrusted,
        "",
        &[S::Return, S::Events, S::Time, S::Memory],
    ),
    port(
        "SelfCheck",
        "the health answer when a slot on trial is confirmed",
        &[on(App, "healthy")],
        Untrusted,
        "a decision point, 3",
        SANDBOXED,
    ),
    port(
        "BoardBringUp",
        "the board's power, pins and buses at app start",
        &[on(App, BOARD), on(Relay, BOARD)],
        Untrusted,
        "",
        &[S::Return, S::Pins, S::Buses, S::Time, S::Memory],
    ),
    port(
        "ChildLink",
        "how a hub reaches a child",
        &[on(Relay, "I2C controller, PIO UART")],
        Untrusted,
        "",
        &[S::Return, S::Buses, S::Transport, S::Dma, S::Time, S::Memory],
    ),
    port(
        "Signer",
        "signing images and releases",
        &[on(Host, KEY)],
        Untrusted,
        "the host is trusted by definition; the device verifies",
        &[],
    ),
    port(
        "KeyStore",
        "where the signing key is kept",
        &[on(Host, KEY)],
        Untrusted,
        "the host is trusted by definition; the device verifies",
        &[],
    ),
    port(
        "BuildRunner",
        "building node images",
        &[on(Host, "cargo")],
        Untrusted,
        "",
        &[],
    ),
    port(
        "Agent",
        "an external agent the CLI and the page hand requests to",
        &[on(Host, "data-defined (agent definitions)")],
        Untrusted,
        "",
        &[],
    ),
    port(
        "Action",
        "a program or request run after a host event",
        &[on(Host, "data-defined (action definitions)")],
        Untrusted,
        "",
        &[],
    ),
    port(
        "ImageLifecycle",
        "the image-lifecycle machine over the slot store and the state log",
        &[on(Boot, DAGON), on(App, DAGON)],
        Never("the only writer of persistent image state; every change emits its event and keeps the confirm rule"),
        "",
        &[],
    ),
    port(
        "SessionGuards",
        "the update session's guards: target, slot, header signature and compatibility, body digest",
        &[on(Boot, DAGON), on(App, DAGON)],
        Never("every protocol's triggers pass the same guards; replacing them would let a transport write what DAGON refuses"),
        "",
        &[],
    ),
    port(
        "BootRecord",
        "the retained record of boot events the app replays",
        &[on(Boot, DAGON), on(App, DAGON)],
        Never("its format is DAGON's retained contract and it is never an input to a boot or confirm decision"),
        "",
        &[],
    ),
    port(
        "Composition",
        "the order of a layer's chain (publisher, board package, libraries, user) and the decision bounds",
        &[on(Boot, DAGON), on(App, DAGON), on(Relay, DAGON), on(Host, DAGON)],
        Never("it is what makes every extension see every event and keeps answers bounded"),
        "",
        &[],
    ),
    port(
        "Sandbox",
        "the entry gate, the service calls, the protection map and the lockup fuse",
        &[on(Boot, DAGON), on(App, DAGON), on(Relay, DAGON)],
        Never("it is what keeps extensions away from DAGON's state"),
        "",
        &[],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_record_per_id_each_in_a_layer() {
        for (i, p) in PORTS.iter().enumerate() {
            assert!(PORTS[i + 1..].iter().all(|q| q.id != p.id), "{}", p.id);
            assert!(!p.layers.is_empty(), "{}", p.id);
            for (j, d) in p.layers.iter().enumerate() {
                assert!(
                    p.layers[j + 1..].iter().all(|e| e.layer != d.layer),
                    "{}",
                    p.id
                );
            }
        }
    }

    #[test]
    fn only_sandboxed_device_ports_call_services() {
        for p in PORTS {
            let sandboxed = matches!(p.trust, Trust::Untrusted | Trust::Additive)
                && p.layers.iter().any(|d| d.layer != Layer::Host);
            assert_eq!(!p.services.is_empty(), sandboxed, "{}", p.id);
            if sandboxed {
                assert!(p.services.contains(&Service::Return), "{}", p.id);
            }
        }
    }

    #[test]
    fn find_names_ports_only() {
        assert!(Port::find("UpdateTransport").unwrap().in_layer(Layer::Boot));
        assert!(Port::find("ImageLifecycle").is_none());
        assert!(Port::find("Nope").is_none());
    }
}
