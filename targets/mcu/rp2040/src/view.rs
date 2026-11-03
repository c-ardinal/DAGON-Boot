//! What a home screen shows about the 親機 and its 子機, independent of
//! the HAL so the rules (and a display's layout) run in host tests.

use dagon_format::Version;
use dagon_proto::payload::{HelloResp, Show, SlotState, StatusResp, SHOW_TEXT_MAX};
use heapless::String;

use crate::nodes::{SubNode, MAX_SUBS};

/// How a node is doing, as far as the 親機 knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shade {
    #[default]
    Unknown,
    Confirmed,
    Trial,
    NoAnswer,
    /// Tried a new version and went back (yellow).
    RolledBack,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub node: SubNode,
    pub version: Option<Version>,
    pub shade: Shade,
}

/// The last SHOW.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub node: u8,
    pub phase: u8,
    pub percent: u8,
    pub version: Version,
    /// Printable ASCII of the SHOW text.
    pub text: String<SHOW_TEXT_MAX>,
}

impl From<&Show<'_>> for Progress {
    fn from(s: &Show<'_>) -> Self {
        let mut text = String::new();
        if s.text.iter().all(|b| (0x20..0x7F).contains(b)) {
            s.text.iter().for_each(|&b| {
                let _ = text.push(b as char);
            });
        }
        Progress {
            node: s.node,
            phase: s.phase,
            percent: s.percent.min(100),
            version: s.version,
            text,
        }
    }
}

/// Everything a home screen shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Home {
    pub hub: Node,
    pub subs: heapless::Vec<Node, MAX_SUBS>,
    pub progress: Option<Progress>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    pub version: Option<Version>,
    pub shade: Shade,
}

impl Seen {
    /// From a HELLO answer (`None`: no answer).
    pub fn from_hello(h: Option<&HelloResp>) -> Seen {
        match h {
            Some(h) => Seen {
                version: Some(h.active_version),
                shade: if h.trial {
                    Shade::Trial
                } else {
                    Shade::Confirmed
                },
            },
            None => Seen {
                version: None,
                shade: Shade::NoAnswer,
            },
        }
    }

    /// From a STATUS answer: on trial, rolled back (a slot reverted and
    /// nothing newer staged), or confirmed.
    pub fn from_status(st: &StatusResp) -> Seen {
        let active = st.slots.get(st.active as usize);
        let shade = match active.map(|s| s.state) {
            Some(SlotState::Trying) => Shade::Trial,
            _ if st.slots.iter().any(|s| s.state == SlotState::Reverted) => Shade::RolledBack,
            Some(_) => Shade::Confirmed,
            None => Shade::Unknown,
        };
        Seen {
            version: active.map(|s| s.version),
            shade,
        }
    }
}

/// What to show after `new` was observed where `old` was shown. A HELLO
/// cannot tell a rolled-back node from a confirmed one, so the shade stays
/// rolled back while the version stays the same, and a version on trial
/// that is replaced by another one without trial was rolled back.
pub fn merge(old: Seen, new: Seen) -> Seen {
    let confirmed = new.shade == Shade::Confirmed && new.version.is_some();
    let rolled = match old.shade {
        Shade::Trial => confirmed && old.version != new.version,
        Shade::RolledBack => confirmed && old.version == new.version,
        _ => false,
    };
    if rolled {
        Seen {
            shade: Shade::RolledBack,
            ..new
        }
    } else {
        new
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_proto::payload::SlotInfo;

    fn seen(shade: Shade, minor: u8) -> Seen {
        Seen {
            version: Some(Version::new(1, minor, 0)),
            shade,
        }
    }

    #[test]
    fn trial_replaced_by_the_old_version_is_rolled_back() {
        let r = merge(seen(Shade::Trial, 2), seen(Shade::Confirmed, 1));
        assert_eq!(r, seen(Shade::RolledBack, 1));
        // Polling again keeps it yellow; a new trial or version clears it.
        assert_eq!(merge(r, seen(Shade::Confirmed, 1)), r);
        assert_eq!(merge(r, seen(Shade::Trial, 3)), seen(Shade::Trial, 3));
        assert_eq!(
            merge(r, seen(Shade::Confirmed, 3)),
            seen(Shade::Confirmed, 3)
        );
    }

    #[test]
    fn confirmed_trial_stays_confirmed() {
        let r = merge(seen(Shade::Trial, 2), seen(Shade::Confirmed, 2));
        assert_eq!(r, seen(Shade::Confirmed, 2));
        let silent = Seen::from_hello(None);
        assert_eq!(merge(r, silent).shade, Shade::NoAnswer);
    }

    #[test]
    fn status_with_a_reverted_slot_is_rolled_back() {
        let slot = |state, minor| SlotInfo {
            state,
            version: Version::new(1, minor, 0),
            digest16: [0; 16],
        };
        let mut st = StatusResp {
            slots: [
                slot(SlotState::Confirmed, 1),
                slot(SlotState::Reverted, 2),
                SlotInfo::default(),
            ],
            active: 0,
            confirmed: 0,
            ..StatusResp::default()
        };
        assert_eq!(Seen::from_status(&st), seen(Shade::RolledBack, 1));
        st.slots[1].state = SlotState::Pending;
        assert_eq!(Seen::from_status(&st), seen(Shade::Confirmed, 1));
        st.slots[1].state = SlotState::Trying;
        st.active = 1;
        assert_eq!(Seen::from_status(&st), seen(Shade::Trial, 2));
    }
}
