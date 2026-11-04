//! The app layer's published snapshot: the state a display or log task
//! waits on (`dagon_events::Reader<AppSnapshot, N>`), folded from the
//! agent's events by the layer's publisher.

use dagon_events::{Event, Snapshot};
use dagon_format::Version;

use crate::{image, requests, session};

/// Where the node is in an update and its trial.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Phase {
    /// The start has not opened the slot store yet.
    #[default]
    Starting,
    /// Running, nothing in progress.
    Idle,
    /// An image is arriving ([`AppSnapshot::progress`]).
    Transferring,
    /// The received body is being checked.
    Verifying,
    /// A new image is written and verified.
    Written,
    /// A written image is marked for the next boot.
    Pending,
    /// A reboot was accepted; the reset follows the reply.
    RebootPending,
    /// The running image is on trial.
    Trial,
    /// The running image's trial was confirmed.
    Confirmed,
    /// A trial ran out or failed; the previous image runs again.
    RolledBack,
    /// A transfer was refused (by DAGON or an extension), failed its
    /// checks or was dropped, or a SET_PENDING was refused. A refused
    /// confirm is not a failure here: the trial goes on (`Trial`).
    Failed,
}

/// The app layer's state for display and log tasks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AppSnapshot {
    pub phase: Phase,
    /// The running image's version.
    pub version: Version,
    /// Bytes of the body received and expected, while transferring.
    pub progress: Option<(u32, u32)>,
    /// The running image is on trial.
    pub trial: bool,
}

impl Snapshot for AppSnapshot {
    fn apply(&mut self, e: &Event<'_>) {
        if let Some(s) = e.get::<requests::StoreOpened>() {
            self.version = s.version;
            self.trial = s.trial;
            self.phase = if s.trial { Phase::Trial } else { Phase::Idle };
        } else if let Some(t) = e.get::<image::TrialStarted>() {
            (self.version, self.trial, self.phase) = (t.version, true, Phase::Trial);
        } else if let Some(c) = e.get::<image::Confirmed>() {
            (self.version, self.trial, self.phase) = (c.version, false, Phase::Confirmed);
        } else if e.get::<image::RolledBack>().is_some() {
            (self.trial, self.phase) = (false, Phase::RolledBack);
        } else if let Some(t) = e.get::<image::TransferStarted>() {
            (self.phase, self.progress) = (Phase::Transferring, Some((0, t.len)));
        } else if let Some(p) = e.get::<session::TransferProgress>() {
            (self.phase, self.progress) = (Phase::Transferring, Some((p.done, p.total)));
        } else if e.get::<session::VerifyStarted>().is_some() {
            self.phase = Phase::Verifying;
        } else if e.get::<image::Written>().is_some() {
            (self.phase, self.progress) = (Phase::Written, None);
        } else if e.get::<image::PendingSet>().is_some() {
            self.phase = Phase::Pending;
        } else if e.get::<requests::RebootRequested>().is_some() {
            self.phase = Phase::RebootPending;
        } else if e.get::<session::TransferAborted>().is_some()
            || e.get::<requests::UpdateRefused>().is_some()
            || e.get::<requests::HeaderRejected>().is_some()
            || e.get::<requests::VerifyFailed>().is_some()
            || e.get::<requests::PendingRefused>().is_some()
        {
            (self.phase, self.progress) = (Phase::Failed, None);
        } else if e.get::<requests::ConfirmRefused>().is_some() && self.trial {
            self.phase = Phase::Trial;
        }
    }
}
