//! `release build`: release checks, signed manifest and `.dgb` bundle with
//! the topology canonical binary as the `dest = TOPOLOGY` item.

use std::fmt;

use dagon_format::bundle::{self, dest, BundleItem, NO_ENTRY};
use dagon_format::header::{image_kind, target_tag, ImageHeader, HEADER_SIZE};
use dagon_format::manifest::{self, entry_flags, ManifestEntry, ManifestFields};
use dagon_format::{crc32, header, key_id, sha256};
use dagon_topology::schema::{Constraint, Role, Topology};
use ed25519_compact::KeyPair;

use crate::image::{self, capabilities_ok, mcu_id_of, parse_version, port_kind_of, sign};

/// Size of one hub bundle store face (4 MiB flash layout).
pub const SUB_BUNDLE_LIMIT: usize = 884_736;

/// An image placed in the bundle, in manifest entry order.
#[derive(Debug, Clone, Copy)]
pub struct ReleaseItem<'a> {
    /// Signed header + payload.
    pub image: &'a [u8],
    pub node_index: u8,
    pub dest: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub manifest: Vec<u8>,
    pub bundle: Vec<u8>,
}

/// A failed release check: check ID, error code and what failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseError {
    pub check: &'static str,
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.check, self.code, self.message)
    }
}

impl std::error::Error for ReleaseError {}

fn fail(check: &'static str, code: &'static str, message: impl Into<String>) -> ReleaseError {
    ReleaseError {
        check,
        code,
        message: message.into(),
    }
}

fn header_of(image: &[u8]) -> Result<([u8; HEADER_SIZE], ImageHeader), ReleaseError> {
    let raw: [u8; HEADER_SIZE] = image
        .get(..HEADER_SIZE)
        .and_then(|h| h.try_into().ok())
        .ok_or_else(|| fail("RB-01", "R-SIG", "image shorter than its header"))?;
    let h = ImageHeader::parse(&raw).map_err(|e| fail("RB-01", "R-SIG", format!("{e:?}")))?;
    Ok((raw, h))
}

/// Signed manifest (no constraints; entries in `items` order, `order_rank` =
/// position) and the bundle: head, manifest, images, topology item last.
/// Performs no release checks; see [`build`].
pub fn assemble(
    name: &str,
    release_seq: u32,
    items: &[ReleaseItem<'_>],
    topology: &[u8],
    key: &KeyPair,
) -> Result<Release, ReleaseError> {
    let entries = items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let (raw, h) = header_of(it.image)?;
            Ok(ManifestEntry {
                node_index: it.node_index,
                image_kind: h.image_kind,
                slot_binding: h.slot_binding,
                flags: entry_flags::REQUIRED,
                version: h.version,
                payload_size: h.payload_size,
                proto_version: h.proto_version,
                header_sha256: header::header_digest(&raw),
                order_rank: i as u8,
            })
        })
        .collect::<Result<Vec<_>, ReleaseError>>()?;
    let fields = ManifestFields {
        release_seq,
        topology_sha256: sha256(topology),
        release_name: target_tag(name),
        key_id: key_id(&key.pk),
    };
    let mut man = vec![0u8; manifest::total_size(entries.len(), 0)];
    manifest::encode(&fields, &entries, &[], &mut man)
        .map_err(|e| fail("RB-02", "R-NODE", format!("manifest: {e:?}")))?;
    let body = man.len() - manifest::SIG_SIZE;
    let sig = sign(key, manifest::SIGN_DOMAIN, &man[..body]);
    man[body..].copy_from_slice(&sig);

    let parts: Vec<(&[u8], u8, u8)> = items
        .iter()
        .enumerate()
        .map(|(i, it)| (it.image, i as u8, it.dest))
        .chain([(topology, NO_ENTRY, dest::TOPOLOGY)])
        .collect();
    let mut off = bundle::head_size(parts.len());
    let man_off = off;
    off += man.len().next_multiple_of(4);
    let mut table = Vec::with_capacity(parts.len());
    for (bytes, entry_index, d) in &parts {
        table.push(BundleItem {
            entry_index: *entry_index,
            dest: *d,
            offset: off as u32,
            length: bytes.len() as u32,
            crc32: crc32(bytes),
        });
        off += bytes.len().next_multiple_of(4);
    }
    let mut out = vec![0u8; off];
    bundle::encode_head(&table, man_off as u32, man.len() as u32, &mut out)
        .map_err(|e| fail("RB-03", "R-BUNDLE", format!("bundle head: {e:?}")))?;
    out[man_off..man_off + man.len()].copy_from_slice(&man);
    for (it, (bytes, _, _)) in table.iter().zip(&parts) {
        out[it.offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
    Ok(Release {
        manifest: man,
        bundle: out,
    })
}

struct Placed<'a> {
    image: &'a [u8],
    header: ImageHeader,
    node: usize,
}

/// Runs the release checks against topology `t`, then signs and assembles.
/// `topology_bin` is the canonical binary of `t`; it is carried as is.
pub fn build(
    t: &Topology,
    topology_bin: &[u8],
    images: &[Vec<u8>],
    name: &str,
    key: &KeyPair,
) -> Result<Release, ReleaseError> {
    if !dagon_topology::check(t).is_pushable() {
        return Err(fail(
            "RB-02",
            "R-NODE",
            "topology has errors; run `dagon topo check`",
        ));
    }
    let placed = images
        .iter()
        .enumerate()
        .map(|(i, img)| place(t, i, img, &key.pk))
        .collect::<Result<Vec<_>, _>>()?;
    let release_seq = check_set(t, &placed)?;
    check_constraints(t, &placed)?;

    let rank = |node: usize| {
        t.update
            .order
            .iter()
            .position(|id| *id == t.nodes[node].id)
            .unwrap_or(usize::MAX)
    };
    let mut placed = placed;
    placed.sort_by_key(|p| {
        (
            rank(p.node),
            p.node,
            p.header.image_kind,
            p.header.slot_binding,
        )
    });
    let items: Vec<ReleaseItem> = placed
        .iter()
        .map(|p| ReleaseItem {
            image: p.image,
            node_index: p.node as u8,
            dest: if t.nodes[p.node].role == Role::Coordinator {
                dest::HUB_DIRECT
            } else {
                dest::SUB_STORE
            },
        })
        .collect();
    check_sub_bundle_size(&items)?;
    assemble(name, release_seq, &items, topology_bin, key)
}

/// RB-01 (signature, key), RB-02 (node match), RB-03 (slot budget), RB-04.
fn place<'a>(
    t: &Topology,
    i: usize,
    img: &'a [u8],
    pubkey: &[u8; 32],
) -> Result<Placed<'a>, ReleaseError> {
    let h = image::verify_image(img, pubkey)
        .map_err(|e| fail("RB-01", "R-SIG", format!("image {i}: {e}")))?;
    let node = t
        .nodes
        .iter()
        .position(|n| target_tag(&n.tag) == h.target_tag)
        .ok_or_else(|| {
            fail(
                "RB-02",
                "R-NODE",
                format!("image {i}: target_tag matches no topology node"),
            )
        })?;
    let n = &t.nodes[node];
    if mcu_id_of(&n.mcu) != Some(h.mcu_id) || port_kind_of(&n.port) != Some(h.port_kind) {
        return Err(fail(
            "RB-02",
            "R-NODE",
            format!(
                "image {i}: mcu_id or port_kind differs from node '{}'",
                n.id
            ),
        ));
    }
    let slot = image::slot_of(&n.mcu, &h).ok_or_else(|| {
        fail(
            "RB-02",
            "R-NODE",
            format!("image {i}: no such slot on node '{}'", n.id),
        )
    })?;
    if h.payload_size > slot.max_payload {
        return Err(fail(
            "RB-03",
            "R-BUNDLE",
            format!("image {i}: payload exceeds the slot of node '{}'", n.id),
        ));
    }
    if !capabilities_ok(h.image_kind, h.build_flags, h.capabilities) {
        return Err(fail(
            "RB-04",
            "R-CAP",
            format!("image {i}: boot core or agent without verification"),
        ));
    }
    Ok(Placed {
        image: img,
        header: h,
        node,
    })
}

/// RB-02: 1..=16 entries, one release_seq, no duplicate slot, A/B pairs whole.
fn check_set(t: &Topology, placed: &[Placed<'_>]) -> Result<u32, ReleaseError> {
    if !(1..=manifest::MAX_ENTRIES).contains(&placed.len()) {
        return Err(fail("RB-02", "R-NODE", "a release holds 1 to 16 images"));
    }
    let seq = placed[0].header.release_seq;
    if placed.iter().any(|p| p.header.release_seq != seq) {
        return Err(fail(
            "RB-02",
            "R-NODE",
            "images carry different release_seq",
        ));
    }
    let key = |p: &Placed| (p.node, p.header.image_kind, p.header.slot_binding);
    for (i, a) in placed.iter().enumerate() {
        if placed[i + 1..].iter().any(|b| key(a) == key(b)) {
            return Err(fail(
                "RB-02",
                "R-NODE",
                format!(
                    "two images for the same slot of node '{}'",
                    t.nodes[a.node].id
                ),
            ));
        }
        let n = &t.nodes[a.node];
        let has_pair = image::slot(&n.mcu, a.header.image_kind, 1).is_some();
        let other = 1 - a.header.slot_binding.min(1);
        let paired = placed.iter().any(|b| {
            (b.node, b.header.image_kind, b.header.slot_binding)
                == (a.node, a.header.image_kind, other)
        });
        if has_pair && !paired {
            return Err(fail(
                "RB-02",
                "R-NODE",
                format!("node '{}' needs both slot A and slot B images", n.id),
            ));
        }
    }
    Ok(seq)
}

/// RB-05: the topology's constraints hold for the images present.
fn check_constraints(t: &Topology, placed: &[Placed<'_>]) -> Result<(), ReleaseError> {
    fn of<'p, 'q>(
        t: &'p Topology,
        placed: &'p [Placed<'q>],
        id: &'p str,
    ) -> impl Iterator<Item = &'p Placed<'q>> {
        placed.iter().filter(move |p| t.nodes[p.node].id == id)
    }
    for c in &t.constraints {
        match c {
            Constraint::Lockstep(ids) => {
                let mut protos = ids
                    .iter()
                    .flat_map(|id| of(t, placed, id))
                    .map(|p| p.header.proto_version);
                if let Some(first) = protos.next() {
                    if protos.any(|v| v != first) {
                        return Err(fail(
                            "RB-05",
                            "R-COMPAT",
                            format!("lockstep {ids:?}: proto_version differs"),
                        ));
                    }
                }
            }
            Constraint::Requires { node, needs, min } => {
                let min = version(min)?;
                if of(t, placed, node).next().is_some()
                    && of(t, placed, needs)
                        .any(|p| p.header.image_kind == image_kind::APP && p.header.version < min)
                {
                    return Err(fail(
                        "RB-05",
                        "R-COMPAT",
                        format!("'{node}' requires '{needs}' {min} or later"),
                    ));
                }
            }
            Constraint::MinBootcore { node, min } => {
                let min = version(min)?;
                if of(t, placed, node)
                    .any(|p| p.header.image_kind == image_kind::BOOTCORE && p.header.version < min)
                {
                    return Err(fail(
                        "RB-05",
                        "R-COMPAT",
                        format!("'{node}' requires boot core {min} or later"),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn version(s: &str) -> Result<dagon_format::Version, ReleaseError> {
    parse_version(s).ok_or_else(|| fail("RB-05", "R-COMPAT", format!("bad version '{s}'")))
}

/// RB-03: head + manifest + every `SUB_STORE` image fit one hub store face.
fn check_sub_bundle_size(items: &[ReleaseItem<'_>]) -> Result<(), ReleaseError> {
    let subs: Vec<_> = items.iter().filter(|i| i.dest == dest::SUB_STORE).collect();
    let size = bundle::head_size(subs.len())
        + manifest::total_size(items.len(), 0).next_multiple_of(4)
        + subs
            .iter()
            .map(|i| i.image.len().next_multiple_of(4))
            .sum::<usize>();
    if size > SUB_BUNDLE_LIMIT {
        return Err(fail(
            "RB-03",
            "R-BUNDLE",
            format!("sub-node bundle is {size} bytes, limit {SUB_BUNDLE_LIMIT}"),
        ));
    }
    Ok(())
}
