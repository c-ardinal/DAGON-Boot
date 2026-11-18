//! What a node's link diagnostics rest on: the store of its records
//! ([`DiagStore`]) and the source of the seeds of a signed clear
//! ([`SeedSource`]). The agent is given one of each; [`NoDiag`] and
//! [`NoSeed`] are a node that keeps none, which answers LINK_DIAG,
//! LINK_DIAG_CLEAR and DIAG_SEED with `Status::Unsupported` and does not
//! name them in its HELLO.
//!
//! Every method answers a refusal as the `Status` the node sends:
//! `Status::Unsupported` for what the implementation cannot do,
//! `Status::Flash` for a write to its store that did not complete.

use dagon_proto::diag::{ClearScope, DiagInfo, Entry, Key, Peer, Seed};
use dagon_proto::Status;

/// One exchange the node started with a peer, as the link worker saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub link: u8,
    pub addr: u8,
    /// A complete response frame came back; `false`: a failure sample.
    pub pass: bool,
}

/// The records of a node's link diagnostics: per peer whether it was ever
/// communicated with and the state of its DTCs, and the seed counter.
pub trait DiagStore {
    /// Whether the node keeps link diagnostics: `false` only for
    /// [`NoDiag`].
    const SERVED: bool = true;

    /// What every LINK_DIAG response says of the store.
    fn info(&mut self) -> DiagInfo;

    /// The DTC record with the lowest key above `after` (from the first
    /// when `None`), in key order (`Entry::key`).
    fn entry_after(&mut self, after: Option<Key>) -> Result<Option<Entry>, Status>;

    /// The ever-communicated record with the lowest key above `after`
    /// (from the first when `None`), in key order (`Peer::key`).
    fn peer_after(&mut self, after: Option<Key>) -> Result<Option<Peer>, Status>;

    /// Clears what `scope` names; returns once the clear is committed. A
    /// DTC scope never touches an ever-communicated record.
    fn clear(&mut self, scope: ClearScope) -> Result<(), Status>;

    /// The seed counter plus 1, committed before it is returned, so a
    /// value once returned is never returned again. `Status::Unsupported`
    /// at `u32::MAX` (it never wraps); `Status::Flash` when the append did
    /// not complete.
    fn next_seed_counter(&mut self) -> Result<u32, Status>;

    /// Takes one sample of an exchange.
    fn record(&mut self, sample: Sample) -> Result<(), Status>;
}

/// The source of the seeds a node issues for a signed clear. A seed needs
/// to be unique among the nodes under one signing key, not secret.
pub trait SeedSource {
    /// Whether the node issues seeds: `false` only for [`NoSeed`].
    const SERVED: bool = true;

    /// A new seed; `store` holds the seed counter.
    fn seed<D: DiagStore>(&mut self, store: &mut D) -> Result<Seed, Status>;
}

/// A node that keeps no link diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoDiag;

impl DiagStore for NoDiag {
    const SERVED: bool = false;

    fn info(&mut self) -> DiagInfo {
        DiagInfo::NONE
    }
    fn entry_after(&mut self, _: Option<Key>) -> Result<Option<Entry>, Status> {
        Err(Status::Unsupported)
    }
    fn peer_after(&mut self, _: Option<Key>) -> Result<Option<Peer>, Status> {
        Err(Status::Unsupported)
    }
    fn clear(&mut self, _: ClearScope) -> Result<(), Status> {
        Err(Status::Unsupported)
    }
    fn next_seed_counter(&mut self) -> Result<u32, Status> {
        Err(Status::Unsupported)
    }
    fn record(&mut self, _: Sample) -> Result<(), Status> {
        Err(Status::Unsupported)
    }
}

/// A node that issues no seeds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoSeed;

impl SeedSource for NoSeed {
    const SERVED: bool = false;

    fn seed<D: DiagStore>(&mut self, _: &mut D) -> Result<Seed, Status> {
        Err(Status::Unsupported)
    }
}

/// A hardware random number driver.
pub trait RandomBytes {
    /// Fills `out`; `Status::Unsupported` when the driver fails.
    fn fill(&mut self, out: &mut [u8; 16]) -> Result<(), Status>;
}

/// Seeds of 16 random bytes. Over `n` seeds of all nodes under one key the
/// chance of two equal ones is at most n(n-1)/2^129, when the driver meets
/// the entropy statement of the vendor document its MCU definition cites;
/// a repeat is not excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomSeed<R>(pub R);

impl<R: RandomBytes> SeedSource for RandomSeed<R> {
    fn seed<D: DiagStore>(&mut self, _: &mut D) -> Result<Seed, Status> {
        let mut b = [0; 16];
        self.0.fill(&mut b)?;
        Seed::new(&b).ok_or(Status::Unsupported)
    }
}

/// The factory-unique ID of an MCU, as its vendor states it.
pub trait FactoryId {
    /// Bytes of the ID, 1 to 16: fixed for the MCU's `mcu_id`, so seeds
    /// of one `mcu_id` have one width.
    const LEN: usize;

    /// Reads the ID into `out[..LEN]`; `Status::Unsupported` when it
    /// cannot be read.
    fn read(&mut self, out: &mut [u8; 16]) -> Result<(), Status>;
}

/// Seeds of the store's next seed counter, the MCU number and the
/// factory-unique ID (`Seed::counter`). One node never repeats a seed;
/// two nodes differ when the vendor's uniqueness statement for the ID
/// holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterSeed<F> {
    /// The image header `mcu_id` of the MCU.
    pub mcu_id: u16,
    pub id: F,
}

impl<F: FactoryId> SeedSource for CounterSeed<F> {
    fn seed<D: DiagStore>(&mut self, store: &mut D) -> Result<Seed, Status> {
        // Every refusal of the ID comes before the counter: none spends a
        // value.
        if !(1..=16).contains(&F::LEN) {
            return Err(Status::Unsupported);
        }
        let mut id = [0; 16];
        self.id.read(&mut id)?;
        let counter = store.next_seed_counter()?;
        let id = id.get(..F::LEN).unwrap_or(&[]);
        Seed::counter(counter, self.mcu_id, id).ok_or(Status::Unsupported)
    }
}
