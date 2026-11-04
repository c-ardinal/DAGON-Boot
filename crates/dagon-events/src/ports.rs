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

const DEVICE_LINKS: &str = "the links the MCU package provides";
const WIRE: &str = "the DAGON wire";
const NONE: &str = "none";
const DAGON: &str = "DAGON's";

use Layer::{App, Boot, Host, Relay};
use Service as S;

/// Every port and every part that is not one.
pub const PORTS: &[Port] = &[
    Port {
        id: "UpdateTransport",
        what: "bytes in and out of a link",
        layers: &[
            on(Boot, DEVICE_LINKS),
            on(App, DEVICE_LINKS),
            on(Relay, DEVICE_LINKS),
            on(Host, "serial"),
        ],
        trust: Trust::Untrusted,
        services: &[S::Return, S::Transport, S::Dma, S::Time, S::Memory],
    },
    Port {
        id: "UpdateProtocol",
        what: "messages to and from session triggers: identity, status, transfer, switch, reboot, confirm; the session machine runs every guard, so a protocol cannot skip one",
        layers: &[on(Boot, WIRE), on(App, WIRE), on(Relay, WIRE), on(Host, WIRE)],
        trust: Trust::Untrusted,
        services: &[S::Return, S::Time, S::Memory],
    },
    Port {
        id: "ImageVerifier",
        what: "header signature and key lookup",
        layers: &[
            on(Boot, "Ed25519 over the project key"),
            on(App, "Ed25519 over the project key"),
        ],
        trust: Trust::TrustedOnly,
        services: &[],
    },
    Port {
        id: "Digest",
        what: "image body hash",
        layers: &[on(Boot, "SHA-256"), on(App, "SHA-256")],
        trust: Trust::TrustedOnly,
        services: &[],
    },
    Port {
        id: "CryptoProvider",
        what: "the primitives the verifier and the digest use (hardware acceleration, a secure element)",
        layers: &[on(Boot, "software"), on(App, "software"), on(Host, "software")],
        trust: Trust::TrustedOnly,
        services: &[],
    },
    Port {
        id: "DecisionPolicy",
        what: "the attempt limit and which slot boots",
        layers: &[on(
            Boot,
            "`max_attempts` from the definition, pending before confirmed",
        )],
        trust: Trust::TrustedOnly,
        services: &[],
    },
    Port {
        id: "WatchdogDriver",
        what: "the watchdog that guards boot and trial",
        layers: &[on(Boot, "the MCU package's"), on(App, "the MCU package's")],
        trust: Trust::TrustedOnly,
        services: &[],
    },
    Port {
        id: "SlotStore",
        what: "where images live",
        layers: &[
            on(Boot, "the MCU package's flash layout"),
            on(App, "the MCU package's flash layout"),
        ],
        trust: Trust::TrustedOnly,
        services: &[],
    },
    Port {
        id: "LogStorage",
        what: "where the state log lives",
        layers: &[
            on(Boot, "the MCU package's flash layout"),
            on(App, "the MCU package's flash layout"),
        ],
        trust: Trust::TrustedOnly,
        services: &[],
    },
    Port {
        id: "Recovery",
        what: "an additional recovery path: a service window for a transport, a vendor download mode",
        layers: &[
            on(Boot, "the recovery paths the MCU and board definitions declare"),
            on(App, "the recovery paths the MCU and board definitions declare"),
        ],
        trust: Trust::Additive,
        services: &[S::Return, S::Transport, S::Time, S::Memory],
    },
    Port {
        id: "Diagnostics",
        what: "extra VERSION entries, extra status, a diagnostic identifier table",
        layers: &[on(Boot, NONE), on(App, NONE), on(Relay, NONE)],
        trust: Trust::Untrusted,
        services: &[S::Return, S::Time, S::Memory],
    },
    Port {
        id: "Display",
        what: "what a phase or a boot outcome looks like",
        layers: &[
            on(
                Boot,
                "the board definition's `[boot]` LED and halt pattern, through the action program",
            ),
            on(App, NONE),
            on(Relay, NONE),
        ],
        trust: Trust::Untrusted,
        services: &[S::Return, S::Pins, S::Buses, S::Time, S::Memory],
    },
    Port {
        id: "LogSink",
        what: "where overflow of the recent ring and extension logs go",
        layers: &[on(App, NONE), on(Relay, NONE), on(Host, NONE)],
        trust: Trust::Untrusted,
        services: &[S::Return, S::Events, S::Time, S::Memory],
    },
    Port {
        id: "SelfCheck",
        what: "the health answer when a slot on trial is confirmed",
        layers: &[on(App, "healthy")],
        trust: Trust::Untrusted,
        services: &[S::Return, S::Time, S::Memory],
    },
    Port {
        id: "BoardBringUp",
        what: "the board's power, pins and buses at app start",
        layers: &[on(App, "the board package's"), on(Relay, "the board package's")],
        trust: Trust::Untrusted,
        services: &[S::Return, S::Pins, S::Buses, S::Time, S::Memory],
    },
    Port {
        id: "ChildLink",
        what: "how a hub reaches a child",
        layers: &[on(Relay, DEVICE_LINKS)],
        trust: Trust::Untrusted,
        services: &[S::Return, S::Buses, S::Transport, S::Dma, S::Time, S::Memory],
    },
    Port {
        id: "Signer",
        what: "signing images and releases",
        layers: &[on(Host, "the project's key pair file")],
        trust: Trust::Untrusted,
        services: &[],
    },
    Port {
        id: "KeyStore",
        what: "where the signing key is kept",
        layers: &[on(Host, "the project's key pair file")],
        trust: Trust::Untrusted,
        services: &[],
    },
    Port {
        id: "BuildRunner",
        what: "building node images",
        layers: &[on(Host, "cargo")],
        trust: Trust::Untrusted,
        services: &[],
    },
    Port {
        id: "Agent",
        what: "an external agent the CLI and the page hand requests to",
        layers: &[on(Host, "agent definitions")],
        trust: Trust::Untrusted,
        services: &[],
    },
    Port {
        id: "Action",
        what: "a program or request run after a host event",
        layers: &[on(Host, "action definitions")],
        trust: Trust::Untrusted,
        services: &[],
    },
    Port {
        id: "ImageLifecycle",
        what: "the image-lifecycle machine over the slot store and the state log",
        layers: &[on(Boot, DAGON), on(App, DAGON)],
        trust: Trust::Never(
            "the only writer of persistent image state; every change emits its event and keeps the confirm rule",
        ),
        services: &[],
    },
    Port {
        id: "SessionGuards",
        what: "the update session's guards: target, slot, header signature and compatibility, body digest",
        layers: &[on(Boot, DAGON), on(App, DAGON)],
        trust: Trust::Never(
            "every protocol's triggers pass the same guards; replacing them would let a transport write what DAGON refuses",
        ),
        services: &[],
    },
    Port {
        id: "BootRecord",
        what: "the retained record of boot events the app replays",
        layers: &[on(Boot, DAGON), on(App, DAGON)],
        trust: Trust::Never(
            "its format is DAGON's retained contract and it is never an input to a boot or confirm decision",
        ),
        services: &[],
    },
    Port {
        id: "Sandbox",
        what: "the entry gate, the service calls, the protection map and the lockup fuse",
        layers: &[on(Boot, DAGON), on(App, DAGON), on(Relay, DAGON)],
        trust: Trust::Never("it is what keeps extensions away from DAGON's state"),
        services: &[],
    },
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
