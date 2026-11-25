//! Which nodes go back to their previous version after a node of an apply
//! stayed off the release's version, so that the release's compatibility
//! constraints hold again.
//!
//! The rule is a closure: a node that runs the release's version and whose
//! constraint no longer holds goes back, which may break a constraint of
//! another node, until nothing changes. A node no constraint ties to the
//! one that stayed keeps the release's version.

use alloc::vec::Vec;

use dagon_format::manifest::{constraint_kind, Constraint};
use dagon_format::Version;

/// One node of the apply that answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// Topology node index.
    pub index: u8,
    /// The app version the release gives it.
    pub release: Version,
    /// The app version it ran before the apply.
    pub before: Version,
    /// The app version it runs (and keeps) now.
    pub running: Version,
}

impl Node {
    /// This apply moved it to the release's version: it can go back.
    fn moved(&self) -> bool {
        self.running == self.release && self.before != self.release
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Realign {
    /// The nodes that go back to `before`, in the order of `nodes`.
    pub back: Vec<u8>,
    /// Nodes on the release's version a constraint still fails for then:
    /// they ran it before this apply already, so there is no version of
    /// this apply to take them back to.
    pub stuck: Vec<u8>,
}

/// The nodes of `nodes` that go back so that `constraints` hold. A node a
/// constraint names and `nodes` leaves out (it does not answer) is not
/// judged.
///
/// - `LOCKSTEP_PROTO`: the named nodes all run the release's version, or
///   none of them does. The protocol version of an image outside the
///   release is not known to the host, so any mix counts as a difference.
/// - `REQUIRES`: `node_a` on the release's version needs `node_b` at
///   `min_version` or later.
/// - `MIN_BOOTCORE`: not judged here. An apply writes a node's boot core
///   before its app and stops when the core fails, so the release's app
///   never runs over an older core.
pub fn realign(constraints: &[Constraint], nodes: &[Node]) -> Realign {
    let mut back: Vec<u8> = Vec::new();
    loop {
        let more: Vec<u8> = constraints
            .iter()
            .flat_map(|c| leaving(c, nodes, &back))
            .filter(|n| n.moved())
            .map(|n| n.index)
            .collect();
        let before = back.len();
        for n in more {
            if !back.contains(&n) {
                back.push(n);
            }
        }
        if back.len() == before {
            break;
        }
    }
    let mut stuck: Vec<u8> = constraints
        .iter()
        .flat_map(|c| leaving(c, nodes, &back))
        .map(|n| n.index)
        .collect();
    let place = |i: &u8| nodes.iter().position(|n| n.index == *i);
    stuck.sort_by_key(place);
    stuck.dedup();
    back.sort_by_key(place);
    Realign { back, stuck }
}

/// The version `n` runs once the nodes of `back` went back.
fn version(n: &Node, back: &[u8]) -> Version {
    if back.contains(&n.index) {
        n.before
    } else {
        n.running
    }
}

/// The nodes `c` does not hold for while they run the release's version,
/// with `back` gone back.
fn leaving<'n>(c: &Constraint, nodes: &'n [Node], back: &[u8]) -> Vec<&'n Node> {
    let on_release = |n: &Node| version(n, back) == n.release;
    let named = |n: &Node| match c.kind {
        constraint_kind::LOCKSTEP_PROTO => n.index < 32 && c.node_mask & (1 << n.index) != 0,
        constraint_kind::REQUIRES => n.index == c.node_a,
        _ => false,
    };
    let broken = match c.kind {
        constraint_kind::LOCKSTEP_PROTO => nodes.iter().any(|n| named(n) && !on_release(n)),
        constraint_kind::REQUIRES => nodes
            .iter()
            .any(|n| n.index == c.node_b && version(n, back) < c.min_version),
        _ => false,
    };
    nodes
        .iter()
        .filter(|n| broken && named(n) && on_release(n))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const P: Version = Version::new(1, 0, 0);
    const N: Version = Version::new(1, 1, 0);

    fn lockstep(mask: u32) -> Constraint {
        Constraint {
            kind: constraint_kind::LOCKSTEP_PROTO,
            node_mask: mask,
            ..Default::default()
        }
    }

    fn requires(a: u8, b: u8, min: Version) -> Constraint {
        Constraint {
            kind: constraint_kind::REQUIRES,
            node_a: a,
            node_b: b,
            min_version: min,
            ..Default::default()
        }
    }

    /// Node `index`: `before` → `running`, the release giving N.
    fn node(index: u8, before: Version, running: Version) -> Node {
        Node {
            index,
            release: N,
            before,
            running,
        }
    }

    #[test]
    fn lockstep_takes_every_named_node_back_with_the_one_that_stayed() {
        // hub 0 and seg 1 on N, watch 2 rolled back.
        let nodes = [node(0, P, N), node(1, P, N), node(2, P, P)];
        let r = realign(&[lockstep(0b111)], &nodes);
        assert_eq!((r.back, r.stuck), (vec![0, 1], vec![]));
    }

    #[test]
    fn a_node_no_constraint_ties_to_the_one_that_stayed_keeps_the_release() {
        let nodes = [node(0, P, N), node(1, P, N), node(2, P, P)];
        assert_eq!(realign(&[], &nodes), Realign::default());
        let r = realign(&[lockstep(0b110)], &nodes);
        assert_eq!((r.back, r.stuck), (vec![1], vec![]));
    }

    #[test]
    fn requires_takes_the_dependent_node_back_and_not_the_one_it_needs() {
        // 0 needs 1 at N. 1 stayed: 0 goes back. 0 stayed: 1 keeps N.
        let c = [requires(0, 1, N)];
        let r = realign(&c, &[node(0, P, N), node(1, P, P)]);
        assert_eq!((r.back, r.stuck), (vec![0], vec![]));
        assert_eq!(
            realign(&c, &[node(0, P, P), node(1, P, N)]),
            Realign::default()
        );
    }

    #[test]
    fn the_range_grows_until_it_is_closed() {
        // 2 stayed; 1 needs 2 at N; 0 is in lockstep with 1.
        let c = [lockstep(0b011), requires(1, 2, N)];
        let nodes = [node(0, P, N), node(1, P, N), node(2, P, P)];
        let r = realign(&c, &nodes);
        assert_eq!((r.back, r.stuck), (vec![0, 1], vec![]));
    }

    #[test]
    fn a_node_on_the_release_before_the_apply_is_stuck() {
        // 0 ran N before this apply (an earlier partial one): nothing to
        // take it back to.
        let nodes = [node(0, N, N), node(1, P, P)];
        let r = realign(&[lockstep(0b011)], &nodes);
        assert_eq!((r.back, r.stuck), (vec![], vec![0]));
    }

    #[test]
    fn a_node_that_does_not_answer_is_not_judged() {
        let r = realign(&[lockstep(0b111)], &[node(0, P, N), node(1, P, N)]);
        assert_eq!(r, Realign::default());
    }

    /// Every state of three nodes under every set of constraints drawn from
    /// the lockstep masks and the requires pairs: the result is closed (no
    /// constraint fails for a node that could still go back), only nodes
    /// this apply moved go back, each node that goes back is forced by a
    /// constraint, and `stuck` names exactly the nodes a constraint is left failing for.
    #[test]
    fn every_small_case_is_closed_and_minimal() {
        let states = [(P, P), (P, N), (N, N)];
        let mut pool: Vec<Constraint> = (0b011..=0b111u32)
            .filter(|m| m.count_ones() >= 2)
            .map(lockstep)
            .collect();
        for a in 0..3u8 {
            for b in (0..3u8).filter(|b| *b != a) {
                pool.push(requires(a, b, N));
            }
        }
        let mut cases = 0u32;
        for set in 0u32..1 << pool.len() {
            if set.count_ones() > 3 {
                continue;
            }
            let cs: Vec<Constraint> = pool
                .iter()
                .enumerate()
                .filter(|(i, _)| set & (1 << i) != 0)
                .map(|(_, c)| *c)
                .collect();
            for s in 0..27usize {
                let nodes: Vec<Node> = (0..3)
                    .map(|i| {
                        let (before, running) = states[s / 3usize.pow(i) % 3];
                        node(i as u8, before, running)
                    })
                    .collect();
                let r = realign(&cs, &nodes);
                cases += 1;
                for i in &r.back {
                    assert!(nodes[*i as usize].moved(), "{cs:?} {nodes:?} {r:?}");
                }
                let mut left: Vec<u8> = cs
                    .iter()
                    .flat_map(|c| leaving(c, &nodes, &r.back))
                    .inspect(|n| assert!(!n.moved(), "{cs:?} {nodes:?} {r:?}"))
                    .map(|n| n.index)
                    .collect();
                left.sort_unstable();
                left.dedup();
                assert_eq!(left, r.stuck, "{cs:?} {nodes:?}");
                // Minimal: without any one of them, a constraint fails for it.
                for skip in &r.back {
                    let rest: Vec<u8> = r.back.iter().copied().filter(|n| n != skip).collect();
                    let forced = cs
                        .iter()
                        .any(|c| leaving(c, &nodes, &rest).iter().any(|n| n.index == *skip));
                    assert!(forced, "{skip} not forced: {cs:?} {nodes:?} {r:?}");
                }
            }
        }
        assert_eq!(cases, 176 * 27);
    }
}
