//! What the host reports while it writes, rolls back or recovers nodes.

use dagon_format::Version;

/// Progress of apply / rollback / recover, for the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// Topology node index.
    pub node: u8,
    /// SHOW phase (`payload::phase`).
    pub phase: u8,
    /// The phase's own progress: the bytes of this image during TRANSFER.
    pub percent: u8,
    /// The node's one progress number (`percent.rs`), what the node being
    /// written is told during its transfer.
    pub shown: u8,
    /// This run has written an image to the node: false on the last report
    /// of a node left as it was, which is no confirmation of anything.
    pub wrote: bool,
    /// The host's clock when this number was reckoned: a caller that counts
    /// the trial's time on from it reckons with the same moments.
    pub at_ms: u64,
    pub version: Version,
    /// On the VERIFY event of a written image: how long its phases took.
    pub times: Option<XferTimes>,
    /// About the node's boot core, written before its app: its CONFIRM is
    /// not the node's (the app is confirmed after it).
    pub boot_core: bool,
}

/// How long one image's transfer took, by phase, in ms of the host's clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XferTimes {
    /// The round trip of the request the node erases the slot on: the last
    /// header chunk (XFER_BEGIN for an image without header). The header's
    /// other requests are in no phase.
    pub erase_ms: u32,
    /// The image body (XFER_DATA), with the progress the node is told.
    pub data_ms: u32,
    /// XFER_END: the node's digest over what it wrote.
    pub verify_ms: u32,
}
