//! `dagon new` and `dagon gen`: a build-ready project with one Rust crate
//! per topology node, on top of the board support packages
//! (`dagon-board-<board>` when the node names a board, `dagon-mcu-<mcu>`
//! when it names only an MCU).
//!
//! ```text
//! <project>/
//!   dagon.toml          topology, [dagon] crate source, [build.<id>], board
//!                       definitions ([board.<board id>])
//!   topology.yaml
//!   nodes/<id>/Cargo.toml, .cargo/config.toml, (ESP) rust-toolchain.toml
//!   nodes/<id>/src/main.rs   start-up, written once (--force-main rewrites)
//!   nodes/<id>/src/app.rs    the developer's code, never touched again
//!   nodes/<id>/src/wiring.rs constants from the topology, rewritten by gen
//! ```
//!
//! The files of a node come from its package's project template: a
//! `template/` folder shipped inside the board (or MCU) package, so the
//! package owns its start-up code. Placeholders: `{{node}}`,
//! `{{tag}}`, `{{dep:<package>}}` (the dependency line for that package).
//!
//! `dagon gen` rewrites only wiring.rs and the DAGON package entries of
//! Cargo.toml, and adds missing [build.<id>] tables; everything the
//! developer wrote stays.

use std::path::{Path, PathBuf};

use dagon_topology::defs::{BoardDef, Origin, Registry};
use dagon_topology::schema::{LinkKind, Node, Role, Topology, DEFAULT_I2C_HZ};
use dagon_topology::SCHEMA_VERSION;
use serde::Deserialize;

use crate::cli::{GenArgs, NewArgs};
use crate::config::{Config, DagonSource};
use crate::defs::read_topology;
use crate::genfs;
use crate::paths::contained;
use crate::templates::{manifests, package_name, render_deps, template_for, write, Written};

pub const SAMPLE: &str = include_str!("../../../topology/sample.yaml");

/// The nodes the links of node `id` reach, in the topology's link order:
/// one `Reached` entry of wiring.rs each (its index, its id, this node's
/// port the link runs on, and the address this node reaches it at).
fn reached(t: &Topology, id: &str) -> String {
    t.links
        .iter()
        .filter_map(|l| Some((l.end(id)?, l.other(id)?)))
        .filter_map(|(near, far)| {
            let index = t.nodes.iter().position(|n| n.id == far.node)?;
            let addr = far
                .addr
                .map_or("None".into(), |a| format!("Some({a:#04x})"));
            Some(format!(
                "    Reached {{ index: {index}, name: {:?}, link: {:?}, addr: {addr} }},\n",
                far.node, near.port
            ))
        })
        .collect()
}

/// The I2C target address node `id` answers at: its end's `addr` on the
/// link it is reached over (`None`: the topology gives none, or the link
/// is not I2C; the board's `[default_link] addr` applies then).
pub(crate) fn link_addr(t: &Topology, id: &str) -> Option<u8> {
    t.reached_over(id)
        .filter(|l| l.kind == LinkKind::I2c)?
        .end(id)?
        .addr
}

/// wiring.rs for the node at `index` in `t.nodes`: constants only, from
/// the topology, `flash_bytes` for a node whose crate comes from its MCU
/// package, the node's erase regions (`regions`: its board's `[flash]`
/// when present, else its MCU's), which its `BoardDef` returns, and for an
/// MCU with a DAGON boot loader `clock`, the `[[app.clock]]` setup the
/// board the node names picks, which its start-up hands its HAL.
pub fn wiring_rs(
    t: &Topology,
    index: usize,
    flash_bytes: Option<u32>,
    regions: &[dagon_topology::defs::EraseRegionDef],
    clock: Option<&dagon_topology::defs::ClockSetup>,
) -> String {
    let n = &t.nodes[index];
    let hub = n.role == Role::Coordinator;
    let link = if hub {
        "Link::Hub".to_string()
    } else {
        match t.reached_over(&n.id) {
            Some(l) if l.kind == LinkKind::I2c => {
                let speed = l
                    .bus
                    .as_ref()
                    .and_then(|b| t.buses.iter().find(|x| &x.id == b))
                    .map_or(DEFAULT_I2C_HZ, |b| b.i2c_hz());
                let at = l.end(&n.id);
                let pins = at.and_then(|e| e.node_pins.as_ref());
                let s = |v: Option<&String>| v.map_or("None".into(), |p| format!("Some({p:?})"));
                format!(
                    "Link::I2c {{ addr: {:#04x}, speed_hz: {speed}, sda: {}, scl: {} }}",
                    at.and_then(|e| e.addr).unwrap_or(0),
                    s(pins.map(|p| &p.sda)),
                    s(pins.map(|p| &p.scl))
                )
            }
            Some(l) => format!("Link::Uart {{ baud: {} }}", l.uart_baud()),
            None => "Link::Hub".to_string(),
        }
    };
    let flash = flash_bytes.map_or("None".into(), |f| format!("Some({f})"));
    let clock = clock.map_or(String::new(), |c| {
        format!(
            "\n/// The clock this node's app runs on.\npub mod clock {{\n{}}}\n",
            dagon_topology::defs::codegen::clock_rs(dagon_topology::defs::ClockStage::App, c)
        )
    });
    format!(
        "//! Generated by `dagon gen` from the topology {topo:?}. Do not edit:\n\
         //! dagon gen rewrites this file; change the topology instead.\n\n\
         #![allow(dead_code)]\n\n\
         /// How this node reaches the hub (the hub itself: `Link::Hub`).\n\
         #[derive(Clone, Copy, Debug)]\npub enum Link {{\n    Hub,\n    \
         I2c {{ addr: u8, speed_hz: u32, sda: Option<&'static str>, scl: Option<&'static str> }},\n    \
         Uart {{ baud: u32 }},\n}}\n\n\
         /// A node one of this node's links reaches.\n\
         #[derive(Clone, Copy, Debug)]\npub struct Reached {{\n    \
         /// Its index in the compiled topology.\n    pub index: u8,\n    \
         /// Its id in the topology.\n    pub name: &'static str,\n    \
         /// This node's port the link runs on (a `[[link_port]]` name of its MCU).\n    \
         pub link: &'static str,\n    \
         /// The address this node reaches it at on that link (`None`: the link gives it none).\n    \
         pub addr: Option<u8>,\n}}\n\n\
         #[derive(Clone, Copy, Debug)]\npub struct Node {{\n    pub id: &'static str,\n    \
         /// Its index in the compiled topology, which identifies it there (nodes may share a tag).\n    \
         pub index: u8,\n    \
         pub tag: &'static str,\n    pub hub: bool,\n    pub max_attempts: u8,\n    pub link: Link,\n    \
         /// Flash size given for a board chosen by MCU (else the board package knows it).\n    \
         pub flash_bytes: Option<u32>,\n}}\n\n\
         pub const NODE: Node = Node {{\n    id: {id:?},\n    index: {index},\n    tag: {tag:?},\n    hub: {hub},\n    \
         max_attempts: {att},\n    link: {link},\n    flash_bytes: {flash},\n}};\n\n\
         {addr}\n\
         /// The nodes this node's links reach, in the topology's link order.\n\
         pub const REACHED: &[Reached] = &[\n{reached}];\n\n{regions}{clock}",
        topo = t.name,
        id = n.id,
        tag = n.tag,
        att = n.max_attempts.unwrap_or(3),
        reached = reached(t, &n.id),
        addr = dagon_extension::gen::link_addr_rs(link_addr(t, &n.id)),
        regions = dagon_topology::defs::codegen::erase_regions_rs(regions),
    )
}

/// What a node needs: its board package, or its MCU package when the node
/// names no board or a board defined in its dagon.toml (which has no
/// package).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Want {
    Board(String),
    Mcu(String),
}

pub fn want_of(reg: &Registry, n: &Node) -> Want {
    match &n.board {
        Some(b) if project_board(reg, n).is_none() => Want::Board(b.clone()),
        _ => Want::Mcu(n.mcu.clone()),
    }
}

/// The board definition node `n` names when the project wrote it in its
/// dagon.toml (`[board.<board id>]`): a board with no package, so the MCU
/// package's crate takes the board's flash size from wiring.rs. A built-in
/// board the project overrides keeps its package (`Origin::Overridden`).
fn project_board<'a>(reg: &'a Registry, n: &Node) -> Option<&'a BoardDef> {
    let id = n.board.as_deref()?;
    (reg.board_origin(id) == Some(Origin::Project))
        .then(|| reg.board(id))
        .flatten()
}

/// The conventional package name: `dagon-board-<board id>` or
/// `dagon-mcu-<mcu id>`, dots and underscores turned into dashes. A package
/// may also declare its id instead (`[package.metadata.dagon] board = ...`
/// or `mcu = ...`), which a local clone is searched for.
pub fn conventional_name(w: &Want) -> String {
    let clean = |s: &str| s.replace(['.', '_'], "-").to_ascii_lowercase();
    match w {
        Want::Board(b) => format!("dagon-board-{}", clean(b)),
        Want::Mcu(m) => format!("dagon-mcu-{}", clean(m)),
    }
}

/// A package found in a local clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub dir: PathBuf,
}

/// The package for `w` in the clone at `root`: by its declared id, else by
/// the conventional name.
pub fn find_for(root: &Path, w: &Want) -> Option<Found> {
    let all = manifests(root);
    let (key, id) = match w {
        Want::Board(b) => ("board", b),
        Want::Mcu(m) => ("mcu", m),
    };
    let declared = all.iter().find(|(_, v)| {
        v.get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("dagon"))
            .and_then(|d| d.get(key))
            .and_then(|x| x.as_str())
            == Some(id.as_str())
    });
    let name = conventional_name(w);
    let by_name = || all.iter().find(|(_, v)| package_name(v) == Some(&name));
    let (dir, v) = declared.or_else(by_name)?;
    Some(Found {
        name: package_name(v)?.to_string(),
        dir: std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone()),
    })
}

/// Fills `{{node}}`, `{{tag}}` and `{{dep:<package>}}` in a template file.
/// The variant of `dagon_mcu_<mcu>::links::Kind` for a link kind (the MCU
/// packages' generator contract, "Links").
fn kind_variant(k: LinkKind) -> &'static str {
    k.variant()
}

/// `{{uplink}}` and `{{downlinks}}` of node `n`: its uplink kind (a coordinator's
/// declared one, else USB CDC; for `role: node`, its link from the node nearer the PC) and its
/// downlink kinds, as Rust paths into its MCU package. A hop its firmware
/// does not run is refused here, by name.
pub fn hop_placeholders(
    reg: &Registry,
    t: &Topology,
    n: &Node,
) -> Result<(String, String), String> {
    use dagon_topology::links::{available_in, known_in, Dir};
    let krate = format!("dagon_mcu_{}", n.mcu.replace('-', "_"));
    let path = |k: LinkKind| format!("{krate}::links::Kind::{}", kind_variant(k));
    let refuse = |hop: String, k: LinkKind| {
        format!(
            "{hop} over {}: the firmware of {} does not run this link kind (choose another kind in the 構成)",
            k.name(),
            n.mcu
        )
    };
    let can =
        |k: LinkKind, d: Dir| !known_in(reg, &n.mcu) || available_in(reg, &n.mcu, d).contains(&k);
    let (up, hop) = match n.role {
        Role::Coordinator => (Topology::uplink(n).kind, format!("uplink of {}", n.id)),
        Role::Node => {
            let l = t
                .reached_over(&n.id)
                .ok_or_else(|| format!("no link reaches {}", n.id))?;
            (l.kind, format!("link {}", l.named()))
        }
    };
    if !can(up, Dir::Up) {
        return Err(refuse(hop, up));
    }
    let mut down: Vec<LinkKind> = Vec::new();
    for l in t.relayed_from(&n.id) {
        if !can(l.kind, Dir::Down) {
            return Err(refuse(format!("link {}", l.named()), l.kind));
        }
        if !down.contains(&l.kind) {
            down.push(l.kind);
        }
    }
    let downs: Vec<String> = down.into_iter().map(path).collect();
    Ok((path(up), downs.join(", ")))
}

/// The link wiring of node `n` for an MCU package's template (a node
/// without a board package), from the 構成 and the MCU definition, so the
/// template names no board's pins:
///
/// - `{{link_i2c}}`, `{{link_sda}}`, `{{link_scl}}`: a node reached over
///   I2C: the pins its link names (`node_pins`) and the controller the
///   MCU's ROM loader pair of those pins names (upper case; the loader
///   refuses a pin pair listed for two controllers);
/// - `{{link_tx_dma_ctl}}`, `{{link_tx_dma_stream}}` and the `rx` pair:
///   the DMA controller (upper case) and stream of that I2C controller's
///   tx and rx, from the MCU definition's request mapping
///   (`defs::dma::assign`), empty when the MCU lists none for it;
/// - `{{down_i2c}}`, `{{down_i2c_sda}}`, `{{down_i2c_scl}}`,
///   `{{down_i2c_hz}}` and `{{down_uart_tx}}`, `{{down_uart_rx}}`,
///   `{{down_uart_baud}}`: a coordinator's I2C bus and UART link to the nodes at their other ends
///   (GPIO numbers; speeds default as in wiring.rs), with `{{if_down_i2c}}`
///   / `{{if_down_uart}}` `all()` when it has one, `any()` (the code is
///   left out; its names then render as placeholders that still parse)
///   when not.
///
/// The template serves one I2C bus and one UART link: a coordinator with more, or
/// an I2C node whose link names no pins the MCU knows, is refused, naming
/// the links, rather than wiring one of them.
pub fn wiring_placeholders(
    reg: &Registry,
    t: &Topology,
    n: &Node,
) -> Result<Vec<(String, String)>, String> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut put = |k: &str, v: String| out.push((format!("{{{{{k}}}}}"), v));
    let pairs = reg
        .mcu(&n.mcu)
        .and_then(|m| m.rom_i2c.as_ref())
        .map_or(&[][..], |r| &r.pairs[..]);
    let (ctl, sda, scl) = match t.reached_over(&n.id).filter(|l| l.kind == LinkKind::I2c) {
        Some(l) => {
            let at = format!("link {}", l.named());
            let p = l
                .end(&n.id)
                .and_then(|e| e.node_pins.as_ref())
                .ok_or_else(|| {
                    format!("{at}: name the node's pins (node_pins) for its MCU template")
                })?;
            let q = pairs
                .iter()
                .find(|q| q.sda == p.sda && q.scl == p.scl)
                .ok_or_else(|| {
                    format!(
                        "{at}: {}/{} is no I2C pair of {} this template knows",
                        p.sda, p.scl, n.mcu
                    )
                })?;
            (q.controller.clone(), p.sda.clone(), p.scl.clone())
        }
        None => Default::default(),
    };
    let mcu = reg.mcu(&n.mcu);
    let has_dma = mcu
        .and_then(|m| m.peripheral(&ctl))
        .is_some_and(|p| !p.dma_requests.is_empty());
    let streams = match mcu.filter(|_| has_dma) {
        Some(m) => dagon_topology::defs::dma::assign(m, &[(&ctl, "tx"), (&ctl, "rx")])
            .map_err(|e| format!("{}: {e}", n.id))?,
        None => Vec::new(),
    };
    for (i, dir) in ["tx", "rx"].into_iter().enumerate() {
        let s = streams.get(i);
        put(
            &format!("link_{dir}_dma_ctl"),
            s.map_or(String::new(), |s| s.controller.to_uppercase()),
        );
        put(
            &format!("link_{dir}_dma_stream"),
            s.map_or(String::new(), |s| s.stream.to_string()),
        );
    }
    put("link_i2c", ctl.to_uppercase());
    put("link_sda", sda);
    put("link_scl", scl);
    let downs: Vec<_> = t.relayed_from(&n.id).collect();
    let names = |ls: &[&&dagon_topology::schema::Link]| {
        ls.iter().map(|l| l.named()).collect::<Vec<_>>().join(", ")
    };
    let i2c: Vec<_> = downs.iter().filter(|l| l.kind == LinkKind::I2c).collect();
    let mut buses: Vec<&str> = i2c.iter().filter_map(|l| l.bus.as_deref()).collect();
    buses.sort_unstable();
    buses.dedup();
    if buses.len() > 1 {
        return Err(format!(
            "{}: its MCU template serves one I2C bus; the 構成 has {} ({}): give the node a board package or one bus",
            n.id,
            buses.join(", "),
            names(&i2c)
        ));
    }
    let bus = buses
        .first()
        .and_then(|b| t.buses.iter().find(|x| x.id == *b));
    put(
        "if_down_i2c",
        if bus.is_some() { "all()" } else { "any()" }.into(),
    );
    put(
        "down_i2c",
        bus.map_or("NoI2c".into(), |b| b.controller.to_uppercase()),
    );
    put(
        "down_i2c_sda",
        bus.map_or("none".into(), |b| b.sda.to_string()),
    );
    put(
        "down_i2c_scl",
        bus.map_or("none".into(), |b| b.scl.to_string()),
    );
    put(
        "down_i2c_hz",
        bus.map_or("0".into(), |b| b.i2c_hz().to_string()),
    );
    let uarts: Vec<_> = downs.iter().filter(|l| l.kind == LinkKind::Uart).collect();
    if uarts.len() > 1 {
        return Err(format!(
            "{}: its MCU template serves one UART link; the 構成 has {} ({}): give the node a board package or one UART link",
            n.id,
            uarts.len(),
            names(&uarts)
        ));
    }
    let uart = uarts.first();
    let pins = match uart {
        Some(l) => Some(l.end(&n.id).and_then(|e| e.pins.as_ref()).ok_or_else(|| {
            format!(
                "link {}: name {}'s UART pins on its end for the MCU template",
                l.named(),
                n.id
            )
        })?),
        None => None,
    };
    put(
        "if_down_uart",
        if uart.is_some() { "all()" } else { "any()" }.into(),
    );
    put(
        "down_uart_tx",
        pins.map_or("none".into(), |p| p.tx.to_string()),
    );
    put(
        "down_uart_rx",
        pins.map_or("none".into(), |p| p.rx.to_string()),
    );
    put(
        "down_uart_baud",
        uart.map_or("0".into(), |l| l.uart_baud().to_string()),
    );
    Ok(out)
}

/// Whether a template uses [`wiring_placeholders`].
fn wires(text: &str) -> bool {
    ["{{link_", "{{down_", "{{if_down_"]
        .iter()
        .any(|k| text.contains(k))
}

fn render(
    text: &str,
    n: &Node,
    hops: &(String, String),
    wiring: &Result<Vec<(String, String)>, String>,
    src: &DagonSource,
    base: &Path,
) -> Result<String, String> {
    // Package lines first, so a tag cannot add placeholders; the tag goes
    // into string literals and comments, escaped as in a Rust string.
    let out = render_deps(text, src, base)?;
    let tag = n.tag.escape_debug().to_string();
    let out = if wires(text) {
        wiring
            .clone()?
            .iter()
            .fold(out, |o, (k, v)| o.replace(k.as_str(), v))
    } else {
        out
    };
    Ok(out
        .replace("{{node}}", &n.id)
        .replace("{{tag}}", &tag)
        .replace("{{uplink}}", &hops.0)
        .replace("{{downlinks}}", &hops.1))
}

/// Whether a dependency is a DAGON board or MCU package, by its effective
/// package name: `package = "..."` when renamed, else the key.
fn dagon_package(k: &str, item: &toml_edit::Item) -> bool {
    let name = item.get("package").and_then(|v| v.as_str()).unwrap_or(k);
    name.starts_with("dagon-board-") || name.starts_with("dagon-mcu-")
}

/// Why the node crate at `manifest` (relative to `base`) does not take its
/// DAGON packages from `src` (`[dagon]`), or `None` when every DAGON
/// dependency matches: release mode wants that git and tag, path mode a
/// path inside the clone.
pub fn source_mismatch(
    base: &Path,
    manifest: &Path,
    src: &DagonSource,
) -> Result<Option<String>, String> {
    let path = contained(base, manifest)?;
    let text =
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|_| format!("{} does not parse", manifest.display()))?;
    let clone =
        crate::dagonsrc::clone_root(src, base).map(|r| std::fs::canonicalize(&r).unwrap_or(r));
    for (_, t) in dep_tables(&mut doc) {
        for (name, item) in t.iter().filter(|(k, v)| dagon_package(k, v)) {
            let field = |k: &str| item.get(k).and_then(|v| v.as_str()).map(String::from);
            let (git, tag, dep_path) = (field("git"), field("tag"), field("path"));
            let ok = match &clone {
                None => git.is_some() && git == src.git && tag == src.tag,
                Some(root) => dep_path.as_deref().is_some_and(|p| {
                    let p = Path::new(&p);
                    let abs = if p.is_absolute() {
                        p.to_path_buf()
                    } else {
                        path.parent().unwrap_or(base).join(p)
                    };
                    std::fs::canonicalize(&abs).is_ok_and(|a| a.starts_with(root))
                }),
            };
            if !ok {
                let has = match (&git, &tag, &dep_path) {
                    (Some(g), t, _) => format!("{g} {}", t.as_deref().unwrap_or("(no tag)")),
                    (None, _, Some(p)) => format!("path {p}"),
                    _ => "an unknown source".into(),
                };
                let want = match (&clone, &src.git, &src.tag) {
                    (Some(r), _, _) => format!("the clone {}", r.display()),
                    (None, Some(g), Some(t)) => format!("{g} {t}"),
                    _ => "?".into(),
                };
                return Ok(Some(format!(
                    "{name} comes from {has}, [dagon] says {want}"
                )));
            }
        }
    }
    Ok(None)
}

/// Every dependency table of a manifest (`dependencies` and
/// `target.<cfg>.dependencies`), with its key path.
fn dep_tables(doc: &mut toml_edit::DocumentMut) -> Vec<(Vec<String>, &mut toml_edit::Table)> {
    let mut out = Vec::new();
    for (k, item) in doc.as_table_mut().iter_mut() {
        if k == "dependencies" {
            if let Some(t) = item.as_table_mut() {
                out.push((vec!["dependencies".to_string()], t));
            }
        } else if k == "target" {
            if let Some(targets) = item.as_table_mut() {
                for (cfg, c) in targets.iter_mut() {
                    if let Some(d) = c
                        .as_table_mut()
                        .and_then(|c| c.get_mut("dependencies"))
                        .and_then(|d| d.as_table_mut())
                    {
                        out.push((
                            vec![
                                "target".into(),
                                cfg.get().to_string(),
                                "dependencies".into(),
                            ],
                            d,
                        ));
                    }
                }
            }
        }
    }
    out
}

type DagonDeps = std::collections::BTreeMap<Vec<String>, Vec<(String, toml_edit::Item)>>;

/// Brings the DAGON package entries of an existing Cargo.toml in line with
/// the freshly rendered template: replaced, added where missing (also a
/// package the template newly needs) and removed where the template has
/// none; everything else stays.
fn refresh_manifest(
    base: &Path,
    rel: &str,
    rendered: &str,
    out: &mut Vec<Written>,
) -> Result<(), String> {
    let rel_path = genfs::Rel::new(rel)?;
    let path = genfs::checked(base, &rel_path)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{rel}: {e}"))?;
    let mut doc: toml_edit::DocumentMut =
        text.parse().map_err(|_| format!("{rel} does not parse"))?;
    let mut want: toml_edit::DocumentMut = rendered
        .parse()
        .map_err(|_| format!("template for {rel} does not parse"))?;
    let mut fresh = DagonDeps::new();
    for (id, t) in dep_tables(&mut want) {
        let deps: Vec<(String, toml_edit::Item)> = t
            .iter()
            .filter(|(k, v)| dagon_package(k, v))
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        if !deps.is_empty() {
            fresh.insert(id, deps);
        }
    }
    for (id, t) in dep_tables(&mut doc) {
        let keep: Vec<&str> = fresh
            .get(&id)
            .map(|d| d.iter().map(|(k, _)| k.as_str()).collect())
            .unwrap_or_default();
        let stale: Vec<String> = t
            .iter()
            .filter(|(k, v)| dagon_package(k, v) && !keep.contains(k))
            .map(|(k, _)| k.to_string())
            .collect();
        for k in stale {
            t.remove(&k);
        }
    }
    for (id, deps) in fresh {
        let mut t = doc.as_table_mut();
        for seg in &id {
            t = t
                .entry(seg)
                .or_insert_with(|| {
                    let mut n = toml_edit::Table::new();
                    n.set_implicit(true);
                    toml_edit::Item::Table(n)
                })
                .as_table_mut()
                .ok_or_else(|| format!("{rel}: [{}] is not a table", id.join(".")))?;
        }
        for (k, v) in deps {
            t.insert(&k, v);
        }
    }
    let new = doc.to_string();
    let changed = new != text;
    if changed {
        genfs::write(base, &rel_path, new.as_bytes())?;
        // The lock file pins the old source; the next build writes a new
        // one (a crate without Cargo.lock is resolved once).
        let lock = rel.replace("Cargo.toml", "Cargo.lock");
        if genfs::remove(base, &genfs::Rel::new(&lock)?)? {
            out.push(Written {
                path: lock,
                action: "removed",
            });
        }
    }
    out.push(Written {
        path: rel.into(),
        action: if changed { "rewritten" } else { "unchanged" },
    });
    Ok(())
}

/// The template files of node `n` (rendered), or why there are none.
fn node_files(
    reg: &Registry,
    t: &Topology,
    n: &Node,
    src: &DagonSource,
    base: &Path,
) -> Result<Vec<(String, String)>, String> {
    let want = want_of(reg, n);
    // The release's checkout (fetched by cargo) or the local clone: the
    // templates come from the same version the crates depend on.
    let root = crate::dagonsrc::root(src, base)?;
    let f = find_for(&root, &want).ok_or_else(|| {
        format!(
            "no board support package for {want:?} in {} (looked for {} or a \
             package declaring it)",
            root.display(),
            conventional_name(&want)
        )
    })?;
    let name = f.name;
    let hops = hop_placeholders(reg, t, n)?;
    let wiring = wiring_placeholders(reg, t, n);
    let files = template_for(&f.dir).ok_or_else(|| format!("{name} has no project template"))?;
    files
        .into_iter()
        .map(|(rel, text)| Ok((rel, render(&text, n, &hops, &wiring, src, base)?)))
        .collect()
}

/// What `gen` did, and the nodes it could not generate (with the reason).
#[derive(Debug, Default)]
pub struct GenReport {
    pub written: Vec<Written>,
    pub skipped: Vec<(String, String)>,
}

/// Generates (or refreshes) the crates of `only` (all nodes when empty).
/// A node whose package or template is missing is skipped with the reason;
/// the others are still generated.
pub fn gen(
    reg: &Registry,
    base: &Path,
    t: &Topology,
    src: &DagonSource,
    only: &[String],
    force_main: bool,
) -> Result<GenReport, String> {
    let mut r = GenReport::default();
    for (index, n) in t.nodes.iter().enumerate() {
        if !only.is_empty() && !only.contains(&n.id) {
            continue;
        }
        // A hop its firmware does not run stops gen, by name.
        hop_placeholders(reg, t, n)?;
        // A node folder reached through a link is the user's: refused, as
        // for the generated protection files (the node is reported skipped).
        if let Some(link) = genfs::link_in(base, &genfs::Rel::new(format!("nodes/{}", n.id))?) {
            r.skipped.push((n.id.clone(), genfs::link_reason(&link)));
            continue;
        }
        let files = match node_files(reg, t, n, src, base) {
            Ok(f) => f,
            Err(e) => {
                r.skipped.push((n.id.clone(), e));
                continue;
            }
        };
        let dir = format!("nodes/{}", n.id);
        for (rel, text) in files {
            let path = format!("{dir}/{rel}");
            match rel.as_str() {
                "Cargo.toml" if contained(base, Path::new(&path))?.exists() => {
                    refresh_manifest(base, &path, &text, &mut r.written)?
                }
                "src/app.rs" => {
                    write(base, &path, &text, false, &mut r.written)?;
                    app_template(base, &path, &text, &mut r.written)?;
                }
                "src/main.rs" => write(base, &path, &text, force_main, &mut r.written)?,
                "src/wiring.rs" => {}
                _ => write(base, &path, &text, false, &mut r.written)?,
            }
        }
        let flash = project_board(reg, n).map(|b| b.flash_bytes);
        let regions = reg
            .mcu(&n.mcu)
            .and_then(|m| m.erase_regions_on(reg.node_board(n)))
            .unwrap_or(&[]);
        // The board the node names: a node naming none runs on the internal
        // oscillator, as its boot stages do.
        let board = n.board.as_deref().and_then(|b| reg.board(b));
        let clock = reg
            .mcu(&n.mcu)
            .and_then(|m| m.clock_for(dagon_topology::defs::ClockStage::App, board));
        let wiring = wiring_rs(t, index, flash, regions, clock);
        write(
            base,
            &format!("{dir}/src/wiring.rs"),
            &wiring,
            true,
            &mut r.written,
        )?;
    }
    Ok(r)
}

/// Suffix of the template gen writes beside a file of the developer's that
/// differs from it.
pub(crate) const NEW_SUFFIX: &str = ".new";

/// The template of the developer's file `rel`, written beside it as
/// `<rel>.new` while the two differ (line ends aside), so that a template
/// that changed can be compared and taken over by hand; the file itself is
/// never touched. Once they are the same, the copy gen wrote is removed.
fn app_template(base: &Path, rel: &str, text: &str, out: &mut Vec<Written>) -> Result<(), String> {
    let new = format!("{rel}{NEW_SUFFIX}");
    let read = |rel: &str| -> Result<Option<String>, String> {
        let path = genfs::checked(base, &genfs::Rel::new(rel)?)?;
        Ok(std::fs::read_to_string(path)
            .ok()
            .map(|t| t.replace("\r\n", "\n")))
    };
    let template = text.replace("\r\n", "\n");
    if read(rel)?.is_some_and(|own| own != template) {
        return write(base, &new, text, true, out);
    }
    // Only gen's own copy is removed: one the developer changed stays.
    if read(&new)?.is_some_and(|copy| copy == template)
        && genfs::remove(base, &genfs::Rel::new(&new)?)?
    {
        out.push(Written {
            path: new,
            action: "removed",
        });
    }
    Ok(())
}

/// Adds `[build.<id>]` for nodes that have none (comments kept).
pub fn add_build_tables(
    reg: &Registry,
    config: &Path,
    t: &Topology,
) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(config).map_err(|e| format!("{}: {e}", config.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|_| "dagon.toml does not parse".to_string())?;
    let builds = doc
        .entry("build")
        .or_insert(toml_edit::table())
        .as_table_mut()
        .ok_or("build in dagon.toml is not a table")?;
    builds.set_implicit(true);
    let mut added = Vec::new();
    for n in &t.nodes {
        if builds.contains_key(&n.id) {
            continue;
        }
        let mut tbl = toml_edit::Table::new();
        tbl.insert("package", toml_edit::value(format!("nodes/{}", n.id)));
        // An MCU without its own slot B link (ESP32-S3: one image runs
        // from either OTA slot) builds once.
        if crate::values::has_slot_b_link(reg, &n.mcu) {
            tbl.insert("slot_b_feature", toml_edit::value("slot-b"));
        }
        builds.insert(&n.id, toml_edit::Item::Table(tbl));
        added.push(n.id.clone());
    }
    if !added.is_empty() {
        let (dir, name) = split(config)?;
        genfs::write(dir, &genfs::Rel::new(&name)?, doc.to_string().as_bytes())?;
    }
    Ok(added)
}

/// The `[dagon]` and `[extensions]` tables of dagon.toml (its board
/// definitions are read with the other definitions, `defs::registry`).
#[derive(Debug, Default, Deserialize)]
struct ProjectTables {
    #[serde(default)]
    dagon: DagonSource,
    #[serde(default)]
    extensions: crate::extgen::Extensions,
}

fn project_tables(config: &Path) -> Result<ProjectTables, String> {
    let text = std::fs::read_to_string(config).map_err(|e| format!("{}: {e}", config.display()))?;
    dagon_topology::project::check(&text).map_err(|e| format!("{}: {e}", config.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", config.display()))
}

/// `dagon gen`
pub fn gen_command(a: &GenArgs) -> Result<(), String> {
    let config = PathBuf::from(a.config.as_deref().unwrap_or(crate::config::CONFIG_FILE));
    let lines = gen_project(&config, &a.nodes, a.force_main)?;
    for line in &lines {
        println!("{line}");
    }
    outcome(&lines)
}

/// How `dagon gen` ends: an error naming every node (or generated folder)
/// it did not generate, with the reason, so the exit status is non-zero.
pub(crate) fn outcome(lines: &[String]) -> Result<(), String> {
    let skipped: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("skipped"))
        .map(str::trim)
        .collect();
    if skipped.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} not generated: {}",
        skipped.len(),
        skipped.join("; ")
    ))
}

/// What gen tells of a template it wrote, or wrote anew, beside the
/// developer's file.
fn template_note(w: &Written) -> Option<String> {
    let own = w.path.strip_suffix(NEW_SUFFIX)?;
    matches!(w.action, "created" | "rewritten").then(|| {
        format!(
            "note: {own} differs from its template, which is in {}: compare the two and take over what you need ({own} is never rewritten)",
            w.path
        )
    })
}

/// `dagon gen` for the project whose dagon.toml is `config`: the lines it
/// reports (written, skipped, added, notes).
pub fn gen_project(config: &Path, only: &[String], force: bool) -> Result<Vec<String>, String> {
    let cfg = Config::load(Some(&config.to_string_lossy()))?;
    let t = read_topology(&cfg, &contained(&cfg.base, &cfg.configured_topology()?)?)?;
    let tables = project_tables(config)?;
    if let Some(bad) = only.iter().find(|id| t.node(id).is_none()) {
        return Err(format!("no updatable node '{bad}' in the topology"));
    }
    let reg = crate::defs::registry(&cfg)?;
    let report = gen(&reg, &cfg.base, &t, &tables.dagon, only, force)?;
    let mut out: Vec<String> = report
        .written
        .iter()
        .map(|w| format!("{:<10} {}", w.action, w.path))
        .collect();
    for (id, why) in &report.skipped {
        out.push(format!("skipped    nodes/{id}: {why}"));
    }
    out.extend(report.written.iter().filter_map(template_note));
    // Every selected node's generated folder holds exactly this run's
    // files; a node whose crate was skipped keeps none.
    let failed: Vec<String> = report.skipped.iter().map(|(id, _)| id.clone()).collect();
    let config_name = config
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| crate::config::CONFIG_FILE.to_string());
    let project = crate::extgen::ProjectRef {
        base: &cfg.base,
        config: &config_name,
        src: &tables.dagon,
        check_boot_graph: true,
        link_addr: &|id| link_addr(&t, id),
        recovery: &|id| dagon_topology::recovery_targets(&reg, &t, id),
    };
    out.extend(crate::extgen::write_nodes(
        &reg,
        &project,
        &t.nodes,
        only,
        &failed,
        &tables.extensions,
    )?);
    for id in add_build_tables(&reg, config, &t)? {
        out.push(format!("added      [build.{id}] to dagon.toml"));
    }
    for n in t.nodes.iter().filter(|n| n.board.is_none()) {
        out.push(format!(
            "note: {} is chosen by MCU; for its flash size, define its board in \
             [board.<board id>] of dagon.toml and name it in the node's board:",
            n.id
        ));
    }
    out.push(match cfg.pubkey {
        Some(_) => "next: dagon build (the firmware embeds pubkey from dagon.toml as DAGON_PUBKEY)"
            .to_string(),
        None => "note: set pubkey in dagon.toml; dagon build embeds it as DAGON_PUBKEY".to_string(),
    });
    Ok(out)
}

/// A 構成 with no nodes yet, for a project started from scratch.
pub fn empty_topology(name: &str) -> String {
    format!(
        "dagon: {SCHEMA_VERSION}
name: {name:?}
nodes: []
update:
  order: []
"
    )
}

/// `dagon new <dir>`
pub fn new_command(a: &NewArgs) -> Result<(), String> {
    let dir = PathBuf::from(&a.dir);
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .ok_or("new needs a folder name")?;
    let yaml = match a.from.as_deref() {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("cannot read {p}: {e}"))?,
        None => SAMPLE.to_string(),
    };
    let keys = match a.keys.as_deref() {
        Some(k) => PathBuf::from(k),
        None => home()?.join(".dagon").join("keys").join(&name),
    };
    let src = crate::dagonsrc::NewSource::from_options(
        a.dagon_path.as_deref(),
        a.dagon_git.as_deref(),
        a.dagon_tag.as_deref(),
    )?;
    let agent = if a.no_agent {
        Vec::new()
    } else {
        let home = crate::consent::home().ok();
        crate::agents::Agents::load(home.as_deref())
            .project_files(a.agent.as_deref(), home.as_deref())?
    };
    let made = new_project_with(&dir, &name, &yaml, &keys, &src, &agent)?;
    for line in &made {
        println!("{line}");
    }
    println!(
        "next: cd {} && dagon gen, dagon build (firmware embeds pubkey from dagon.toml as \
         DAGON_PUBKEY), then dagon serve (signing: set DAGON_SIGNING_KEY={})",
        dir.display(),
        keys.join(dagon_sign::SECRET_FILE).display()
    );
    Ok(())
}

fn home() -> Result<PathBuf, String> {
    crate::consent::home().map_err(|_| "cannot find the home folder; pass --keys <dir>".into())
}

/// The documentation's words page: the one list of DAGON's words, which
/// an agent's project file (the builtin Claude's CLAUDE.md) carries in
/// place of `WORDS_AT`.
const WORDS_PAGE: &str = include_str!("../../../site/src/content/docs/explanation/words.md");
const WORDS_AT: &str = "<!-- words -->";

/// The bullet items of the words page (continuation lines included).
fn word_list() -> String {
    let mut fences = 0;
    let mut in_item = false;
    let mut out = Vec::new();
    for line in WORDS_PAGE.lines() {
        if fences < 2 {
            fences += usize::from(line.trim_end() == "---");
            continue;
        }
        in_item = line.starts_with("- ") || (in_item && line.starts_with("  "));
        if in_item {
            out.push(line);
        }
    }
    out.join("\n")
}

/// An agent's project file as `dagon new` writes it: with the word list
/// in place of `WORDS_AT`.
fn project_text(text: &str) -> String {
    if text.contains(WORDS_AT) {
        text.replace(WORDS_AT, &word_list())
    } else {
        text.to_string()
    }
}

/// The project files of the first builtin agent (`dagon new` without
/// `--agent`).
pub(crate) fn default_agent_files() -> Vec<(String, String)> {
    crate::agents::Agents::load(None)
        .project_files(None, None)
        .expect("the builtin agents' project files are built in")
}

/// `path` as its folder and its last name, for the genfs calls.
fn split(path: &Path) -> Result<(&Path, String), String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?
        .to_string_lossy()
        .to_string();
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok((dir, name))
}

/// [`new_project_with`] the first builtin agent's project files.
pub fn new_project(
    dir: &Path,
    name: &str,
    yaml: &str,
    keys: &Path,
    src: &crate::dagonsrc::NewSource,
) -> Result<Vec<String>, String> {
    new_project_with(dir, name, yaml, keys, src, &default_agent_files())
}

/// Creates the project folder: dagon.toml, topology.yaml, .gitignore, the
/// chosen agent's project files `agent` ((path, text), from its
/// definition) and, when `keys` holds none, a signing key pair there
/// (outside any git work tree). Refuses a non-empty folder.
pub fn new_project_with(
    dir: &Path,
    name: &str,
    yaml: &str,
    keys: &Path,
    src: &crate::dagonsrc::NewSource,
    agent: &[(String, String)],
) -> Result<Vec<String>, String> {
    if dir.exists() && std::fs::read_dir(dir).map_or(true, |mut d| d.next().is_some()) {
        return Err(format!("{} exists and is not empty", dir.display()));
    }
    dagon_topology::parse_str(yaml).map_err(|e| format!("topology: {e}"))?;
    let existed = dir.exists();
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let own = [("topology.yaml", yaml), (".gitignore", "/out/\ntarget/\n")];
    let files: Vec<(&str, String)> = own
        .into_iter()
        .map(|(f, t)| (f, t.to_string()))
        .chain(agent.iter().map(|(f, t)| (f.as_str(), project_text(t))))
        .collect();
    let written = files
        .iter()
        .try_for_each(|(f, text)| genfs::write(dir, &genfs::Rel::new(f)?, text.as_bytes()));
    let made = written.and_then(|_| init_project(dir, name, "topology.yaml", keys, src));
    let mut made = match made {
        Ok(m) => m,
        Err(e) => {
            // The folder was empty (or not there): what is in it now is this
            // call's, so taking it away lets the next try start clean.
            undo_new(dir, existed, files.iter().map(|(f, _)| *f));
            return Err(e);
        }
    };
    for (f, _) in &files {
        made.push(format!("created    {f}"));
    }
    Ok(made)
}

/// Removes what a failed `new_project` wrote into `dir` (empty before):
/// its files `files` and dagon.toml and the folders they made, and `dir`
/// itself when it was new.
fn undo_new<'a>(dir: &Path, existed: bool, files: impl Iterator<Item = &'a str>) {
    for f in files.chain(["dagon.toml"]) {
        if let Ok(r) = genfs::Rel::new(f) {
            let _ = genfs::remove(dir, &r);
        }
        // Folders the file needed, innermost first; only while empty.
        let mut up = Path::new(f).parent();
        while let Some(d) = up.filter(|d| !d.as_os_str().is_empty()) {
            let gone = genfs::Rel::new(d.to_string_lossy())
                .is_ok_and(|r| genfs::remove_empty_dir(dir, &r));
            if !gone {
                break;
            }
            up = d.parent();
        }
    }
    if !existed {
        if let Ok((parent, name)) = split(dir) {
            if let Ok(r) = genfs::Rel::new(&name) {
                genfs::remove_empty_dir(parent, &r);
            }
        }
    }
}

/// dagon.toml for the 構成 `topology` (a file name in `dir`) and, when
/// `keys` holds none, a signing key pair there. Refuses a folder that
/// already has a dagon.toml.
pub fn init_project(
    dir: &Path,
    name: &str,
    topology: &str,
    keys: &Path,
    src: &crate::dagonsrc::NewSource,
) -> Result<Vec<String>, String> {
    let config = dir.join(crate::config::CONFIG_FILE);
    if config.exists() {
        return Err("the folder already has a dagon.toml".into());
    }
    let mut made = Vec::new();
    let secret = keys.join(dagon_sign::SECRET_FILE);
    if !secret.exists() {
        let k = dagon_sign::generate_to_dir(keys).map_err(|e| e.to_string())?;
        made.push(format!(
            "key: {} (key id {})",
            k.public_path.display(),
            dagon_sign::to_hex(&k.key_id)
        ));
    }
    let pubkey = keys
        .join(dagon_sign::PUBLIC_FILE)
        .to_string_lossy()
        .replace('\\', "/");
    let toml = format!(
        "# DAGON project {name:?}.\nformat = {}\ntopology = {topology:?}\n\
         # Public key the firmware and the page verify images with (the secret\n\
         # key stays outside the project: DAGON_SIGNING_KEY).\npubkey = {pubkey:?}\n\n\
         # Where the generated crates and the boot images take DAGON from:\n\
         # a release (git and tag) or, for development, a local clone (path).\n\
         [dagon]\n{}",
        dagon_topology::project::FORMAT,
        src.toml()
    );
    genfs::write(
        dir,
        &genfs::Rel::new(crate::config::CONFIG_FILE)?,
        toml.as_bytes(),
    )?;
    made.push("created    dagon.toml".into());
    Ok(made)
}

#[cfg(test)]
// Tests set up and inspect project folders directly.
#[allow(clippy::disallowed_methods)]
mod tests;
#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests_slots;
#[cfg(test)]
mod tests_wiring;
