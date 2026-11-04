//! Compiled-topology scenarios: pushes interrupted at every byte (erase and
//! program), repeated interruption, corrupted banks, bodies of another
//! layout, topologies too large for the bank, erased or random flash at the
//! start. Property P13 in its parts: interrupted replacement (previous or
//! new, as `commit_step` states), exact old copy, round trip with identity,
//! bounds, malformed or unsupported reads as absent, stable reads.

use crate::harness::{Violation, World};
use crate::identity::identity;
use crate::mem::{Cut, Mem};
use crate::topology::{
    self, host_plan, BodyCodec, BodyV1, Node, Refused, Source, Store, Topology, Written,
};
use dagon_format::manifest::Constraint;
use dagon_format::topology::TopoLink;
use dagon_sim::SimRng;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// The host pushes the topology generated from `topo`.
    Push { topo: u64, cut: Option<u32> },
    /// A body whose layout number the hub does not read.
    PushForeign { cut: Option<u32> },
    /// One flipped byte at `at` within the two banks.
    Corrupt { at: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub bank: usize,
    /// Flash starts random instead of erased.
    pub random_start: bool,
}

pub fn topology(seed: u64) -> Topology {
    let mut r = SimRng::new(seed);
    let n = 1 + r.below(8) as usize;
    let nodes = (0..n)
        .map(|i| Node {
            index: i as u8,
            role: (i > 0) as u8,
            port_kind: r.byte(),
            max_attempts: r.byte(),
            listen_window_ms: r.next_u64() as u16,
            map_id: r.byte() & 1,
            order_rank: r.byte(),
            recovery_tiers: [r.byte() % 5, r.byte() % 5, 0],
            mcu: identity(&format!("mcu-{}", r.below(5))),
            target_tag: [r.byte(); 16],
        })
        .collect();
    let mut tos: Vec<u8> = (1..n as u8).filter(|_| r.chance(0.7)).collect();
    tos.sort_unstable();
    let links = tos
        .into_iter()
        .map(|to| TopoLink {
            from: 0,
            to,
            iface: 1 + r.below(3) as u8,
            control: 1,
            tx_pin: r.byte(),
            rx_pin: r.byte(),
            nrst_pin: r.byte(),
            boot_pin: r.byte(),
            baud: r.next_u64() as u32,
            rom_entry: r.byte() % 4,
            i2c_addr: r.byte() & 0x7F,
            bus_id: r.byte(),
        })
        .collect();
    let constraints = (0..r.below(17))
        .map(|_| Constraint {
            kind: r.byte(),
            node_a: r.byte(),
            node_b: r.byte(),
            node_mask: r.next_u64() as u32,
            min_version: dagon_format::Version::from_raw(r.next_u64() as u32),
        })
        .collect();
    Topology {
        nodes,
        links,
        constraints,
    }
}

struct Sim {
    store: Store,
    mem: Mem,
    /// Every body written to completion, for "never partially applied".
    complete: Vec<Vec<u8>>,
    step: usize,
}

impl Sim {
    fn fail(&self, p: &'static str, d: impl Into<String>) -> Violation {
        Violation::new(p, self.step, d)
    }

    fn banks(&self) -> std::ops::Range<usize> {
        self.store.base..self.store.base + 2 * self.store.bank
    }

    /// The same answer on every read, and only ever a complete body.
    fn check_read(&self) -> Result<Option<topology::Copy>, Violation> {
        let a = topology::current(self.mem.bytes(), &self.store);
        if a != topology::current(self.mem.bytes(), &self.store) {
            return Err(self.fail("P13", "two reads differ"));
        }
        if let Some(c) = &a {
            if !self.complete.contains(&c.body) {
                return Err(self.fail("P13", "a copy no write completed"));
            }
        }
        Ok(a)
    }

    fn push(&mut self, body: &[u8], cut: Option<u32>) -> Result<(), Violation> {
        let before = self.check_read()?;
        let old_bank = before.as_ref().map(|c| {
            let at = self.store.base + c.bank * self.store.bank;
            (at, self.mem.bytes()[at..at + self.store.bank].to_vec())
        });
        if host_plan(&self.store, body).is_err() {
            let steps = self.mem.steps();
            let r = topology::write(&mut self.mem, &self.store, body);
            if r != Ok(Err(Refused::TooLarge)) || self.mem.steps() != steps {
                return Err(self.fail("P13", "an oversize body reached flash"));
            }
            return Ok(());
        }
        self.mem.cut_after(cut.map(u64::from));
        let r = topology::write(&mut self.mem, &self.store, body);
        self.mem.cut_after(None);
        let done = match r {
            Ok(Ok(w)) => {
                let want = if before.is_some() {
                    Written::Replaced
                } else {
                    Written::First
                };
                if w != want {
                    return Err(self.fail("P13", format!("transition {w:?}, want {want:?}")));
                }
                self.complete.push(body.to_vec());
                true
            }
            Ok(Err(e)) => return Err(self.fail("P13", format!("hub refused {e:?}"))),
            Err(Cut) => {
                let k = cut.unwrap_or(0) as u64;
                if k > self.store.commit_step(body.len()) {
                    self.complete.push(body.to_vec());
                }
                k > self.store.commit_step(body.len())
            }
        };
        let after = self.check_read()?;
        let outcome_ok = if done {
            after.as_ref().is_some_and(|c| c.body == body)
        } else {
            after.as_ref().map(|c| &c.body) == before.as_ref().map(|c| &c.body)
        };
        if !outcome_ok {
            return Err(self.fail("P13", format!("cut {cut:?}: not the stated outcome")));
        }
        if let Some((at, bytes)) = old_bank {
            if !done && self.mem.bytes()[at..at + bytes.len()] != bytes[..] {
                return Err(self.fail("P13", "previous copy changed"));
            }
        }
        Ok(())
    }

    fn round_trip(&self, t: &Topology, body: &[u8]) -> Result<(), Violation> {
        let (got, src) = topology::load(&BodyV1, self.mem.bytes(), &self.store);
        let digest = host_plan(&self.store, body).unwrap();
        if got.as_ref() != Some(t) || src != (Source::Copy { digest }) {
            return Err(self.fail("P13", "pushed topology does not read back"));
        }
        Ok(())
    }
}

pub struct TopologyWorld;

impl World for TopologyWorld {
    type Config = Config;
    type Op = Op;

    fn generate(rng: &mut SimRng) -> (Config, Vec<Op>) {
        let cfg = Config {
            bank: [512, 1024][rng.below(2) as usize],
            random_start: rng.chance(0.3),
        };
        let total = (cfg.bank * 2 + topology::BODY_MAX) as u64;
        let ops = (0..1 + rng.below(12))
            .map(|_| match rng.below(10) {
                0 => Op::PushForeign {
                    cut: rng.chance(0.3).then(|| rng.below(total) as u32),
                },
                1 => Op::Corrupt {
                    at: rng.below(cfg.bank as u64 * 2) as u16,
                },
                _ => Op::Push {
                    topo: rng.next_u64(),
                    cut: rng.chance(0.5).then(|| rng.below(total) as u32),
                },
            })
            .collect();
        (cfg, ops)
    }

    fn run(cfg: &Config, ops: &[Op], seed: u64) -> Result<(), Violation> {
        let store = Store {
            base: 16,
            bank: cfg.bank,
        };
        let len = store.base + 2 * store.bank + 16;
        let mem = if cfg.random_start {
            Mem::random(len, seed)
        } else {
            Mem::new(len, 0xFF, seed)
        };
        let mut sim = Sim {
            store,
            mem,
            complete: Vec::new(),
            step: 0,
        };
        sim.mem.allow(sim.banks());
        for (i, op) in ops.iter().enumerate() {
            sim.step = i;
            match *op {
                Op::Push { topo, cut } => {
                    let t = topology(topo);
                    let body = BodyV1
                        .encode(&t)
                        .map_err(|e| sim.fail("P13", format!("{e:?}")))?;
                    sim.push(&body, cut)?;
                    let fits = body.len() <= store.capacity();
                    let landed = cut.is_none_or(|k| k as u64 > store.commit_step(body.len()));
                    if fits && landed {
                        sim.round_trip(&t, &body)?;
                    }
                }
                Op::PushForeign { cut } => {
                    let mut body = BodyV1.encode(&topology(1)).unwrap();
                    body[4] = 2;
                    sim.push(&body, cut)?;
                    let (t, src) = topology::load(&BodyV1, sim.mem.bytes(), &store);
                    let newest = topology::current(sim.mem.bytes(), &store);
                    if newest.is_some_and(|c| c.body == body)
                        && (t.is_some() || src != Source::Fallback(Some(Refused::Unsupported(2))))
                    {
                        return Err(sim.fail("P13", "unsupported layout applied"));
                    }
                }
                Op::Corrupt { at } => {
                    let a = store.base + at as usize % (2 * store.bank);
                    let v = sim.mem.bytes()[a] ^ 0x5A;
                    sim.mem.poke(a, &[v]);
                    sim.check_read()?;
                }
            }
            if let Some(a) = sim.mem.stray().first() {
                return Err(sim.fail("P13", format!("store outside the banks at {a}")));
            }
        }
        Ok(())
    }

    fn simpler(op: &Op) -> Vec<Op> {
        match *op {
            Op::Push { topo, cut: Some(_) } => vec![Op::Push { topo, cut: None }],
            Op::PushForeign { cut: Some(_) } => vec![Op::PushForeign { cut: None }],
            _ => Vec::new(),
        }
    }
}
