//! Reading and validating definitions: builtin (embedded strings), a DAGON
//! source tree, and project definitions.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use super::{
    page_safe, BoardDef, McuDef, Origin, Part, Partition, Placement, Registry, Slot, Writer,
    FORMAT, HEADER, PROJECT_TEXT,
};

pub const MCU_FILE: &str = "dagon-mcu.toml";
pub const BOARD_FILE: &str = "dagon-board.toml";

/// Where definitions come from besides the builtin set.
#[derive(Debug, Clone, Copy)]
pub enum Source<'a> {
    /// Root of a DAGON source tree: `targets/mcu/*/dagon-mcu.toml` and
    /// `targets/boards/*/dagon-board.toml` replace the builtin set.
    Tree(&'a Path),
    /// A project's dagon.toml: `[board.<id>]` tables (the board schema
    /// without `id`) and `mcu_files = ["<path>/dagon-mcu.toml", ...]`
    /// inside the project.
    Project(&'a Path),
}

/// A definition that does not load: where, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefError {
    pub at: String,
    pub msg: String,
}

impl fmt::Display for DefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.msg)
    }
}

impl std::error::Error for DefError {}

fn err<T>(at: impl fmt::Display, msg: impl Into<String>) -> Result<T, DefError> {
    Err(DefError {
        at: at.to_string(),
        msg: msg.into(),
    })
}

/// The package a definition sits in; referenced files resolve inside it.
enum Pkg<'a> {
    /// Builtin: text files embedded by name. Other referenced files are
    /// only shape-checked here; a test loads the source tree, which checks
    /// they exist.
    Embedded {
        label: &'a str,
        files: &'a [(&'a str, &'a str)],
    },
    Dir(PathBuf),
}

impl Pkg<'_> {
    fn label(&self) -> String {
        match self {
            Pkg::Embedded { label, .. } => (*label).to_string(),
            Pkg::Dir(d) => d.display().to_string(),
        }
    }

    /// Checks the file `rel` stays inside the package (and exists, on disk).
    fn check(&self, rel: &str) -> Result<(), DefError> {
        self.contained(rel, true)
    }

    /// Checks the folder `rel` stays inside the package (and exists, on
    /// disk); "" is the package itself.
    fn check_dir(&self, rel: &str) -> Result<(), DefError> {
        if rel.is_empty() {
            return Ok(());
        }
        self.contained(rel, false)
    }

    /// Relative, plain components only (no `..`, root, drive or `\`); on
    /// disk the resolved path (symlinks followed) must stay in the package.
    fn contained(&self, rel: &str, file: bool) -> Result<(), DefError> {
        let at = || self.label();
        let p = Path::new(rel);
        let plain = !rel.is_empty()
            && !rel.contains(['\\', ':'])
            && p.components().all(|c| matches!(c, Component::Normal(_)));
        if !plain {
            return err(
                at(),
                format!("'{rel}' must be a relative path inside the package"),
            );
        }
        let Pkg::Dir(dir) = self else { return Ok(()) };
        let inside = std::fs::canonicalize(dir.join(p))
            .ok()
            .zip(std::fs::canonicalize(dir).ok())
            .is_some_and(|(f, d)| f.starts_with(d) && if file { f.is_file() } else { f.is_dir() });
        if !inside {
            let what = if file { "file" } else { "folder" };
            return err(at(), format!("'{rel}' is not a {what} inside the package"));
        }
        Ok(())
    }

    /// Part folders stay in the package; names and binary targets are
    /// plain names.
    fn check_parts(&self, parts: &[Part]) -> Result<(), DefError> {
        for p in parts {
            for name in std::iter::once(&p.name).chain(&p.bin) {
                let plain = !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
                if !plain {
                    return err(
                        self.label(),
                        format!("part name/bin '{name}' must be letters, digits, '-' or '_'"),
                    );
                }
            }
            self.check_dir(&p.sub)?;
        }
        Ok(())
    }

    fn text(&self, rel: &str) -> Result<String, DefError> {
        self.check(rel)?;
        match self {
            Pkg::Embedded { label, files } => files
                .iter()
                .find(|(n, _)| *n == rel)
                .map(|(_, t)| (*t).to_string())
                .map_or_else(|| err(label, format!("'{rel}' is not embedded")), Ok),
            Pkg::Dir(d) => std::fs::read_to_string(d.join(rel))
                .or_else(|e| err(self.label(), format!("{rel}: {e}"))),
        }
    }
}

type Embedded = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

const BUILTIN_MCUS: [Embedded; 3] = [
    (
        "targets/mcu/rp2040",
        include_str!("../../../../targets/mcu/rp2040/dagon-mcu.toml"),
        &[],
    ),
    (
        "targets/mcu/stm32f401re",
        include_str!("../../../../targets/mcu/stm32f401re/dagon-mcu.toml"),
        &[],
    ),
    (
        "targets/mcu/esp32s3",
        include_str!("../../../../targets/mcu/esp32s3/dagon-mcu.toml"),
        &[(
            "partitions.csv",
            include_str!("../../../../targets/mcu/esp32s3/partitions.csv"),
        )],
    ),
];

const BUILTIN_BOARDS: [(&str, &str); 3] = [
    (
        "targets/boards/waveshare-rp2040-touch-lcd-1.28",
        include_str!("../../../../targets/boards/waveshare-rp2040-touch-lcd-1.28/dagon-board.toml"),
    ),
    (
        "targets/boards/nucleo-f401re",
        include_str!("../../../../targets/boards/nucleo-f401re/dagon-board.toml"),
    ),
    (
        "targets/boards/m5stopwatch",
        include_str!("../../../../targets/boards/m5stopwatch/dagon-board.toml"),
    ),
];

/// What a definition was read from: its TOML and the text files it
/// references (by package-relative name).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefSource {
    pub label: String,
    pub toml: String,
    #[serde(default)]
    pub files: Vec<(String, String)>,
}

/// [`Registry::to_json`]'s form.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JsonForm {
    pub mcus: Vec<DefSource>,
    pub boards: Vec<DefSource>,
}

fn empty() -> Registry {
    Registry {
        mcus: Vec::new(),
        boards: Vec::new(),
        mcu_sources: Vec::new(),
        board_sources: Vec::new(),
        mcu_origins: Vec::new(),
        board_origins: Vec::new(),
    }
}

impl Registry {
    /// `own`: a project's own definition (read from its files, or labelled
    /// as the project's in JSON), made safe here (project_text).
    fn push_mcu(
        &mut self,
        text: &str,
        pkg: &Pkg<'_>,
        origin: Origin,
        own: bool,
    ) -> Result<(), DefError> {
        let mut m = parse_mcu(text, pkg)?;
        if own {
            let at = format!("{} (MCU '{}')", pkg.label(), m.id);
            super::project_text::mcu(&mut m).or_else(|e| err(&at, e))?;
            if let Pkg::Dir(dir) = pkg {
                m.package = Some(dir.clone());
            }
        }
        let files = match &m.partitions {
            Some(rel) => vec![(rel.clone(), pkg.text(rel)?)],
            None => Vec::new(),
        };
        self.mcu_sources.push(DefSource {
            label: pkg.label(),
            toml: text.to_string(),
            files,
        });
        self.mcus.push(m);
        self.mcu_origins.push(origin);
        Ok(())
    }

    fn push_board(
        &mut self,
        text: &str,
        pkg: &Pkg<'_>,
        origin: Origin,
        own: bool,
    ) -> Result<(), DefError> {
        let mut b = parse_board(text, pkg)?;
        if own {
            let at = format!("{} (board '{}')", pkg.label(), b.id);
            super::project_text::board(&mut b).or_else(|e| err(&at, e))?;
            if let Pkg::Dir(dir) = pkg {
                b.package = Some(dir.clone());
            }
        }
        self.boards.push(b);
        self.board_origins.push(origin);
        self.board_sources.push(DefSource {
            label: pkg.label(),
            toml: text.to_string(),
            files: Vec::new(),
        });
        Ok(())
    }
}

/// Embedded-file view of a source's owned files.
fn file_refs(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect()
}

/// The JSON form with nothing local in it: each definition is labelled by
/// its origin, kind and id (no path), its TOML is re-written without
/// comments and without `[[doctor]]` (commands stay on this machine), and
/// referenced text files lose their comment lines.
pub(super) fn public_json(reg: &Registry) -> String {
    let public = |kind: &str, id: &str, origin: Origin, s: &DefSource| {
        let scrub = origin == Origin::Project;
        let id = if !scrub || page_safe(id) {
            id
        } else {
            PROJECT_TEXT
        };
        // A project's file names are its own words too: each becomes
        // `file-<n>`, in the files and wherever the TOML names it.
        let renames: Vec<(String, String)> = s
            .files
            .iter()
            .enumerate()
            .map(|(i, (n, _))| {
                let to = if scrub {
                    format!("file-{}", i + 1)
                } else {
                    n.clone()
                };
                (n.clone(), to)
            })
            .collect();
        DefSource {
            label: format!("{} {kind} {id}", origin.as_str()),
            toml: public_toml(&s.toml, scrub, &renames),
            files: (s.files.iter().zip(&renames))
                .map(|((_, t), (_, to))| (to.clone(), public_text_file(t, scrub)))
                .collect(),
        }
    };
    let all = JsonForm {
        mcus: (reg.mcus.iter().zip(&reg.mcu_origins).zip(&reg.mcu_sources))
            .map(|((m, o), s)| public("MCU", &m.id, *o, s))
            .collect(),
        boards: (reg
            .boards
            .iter()
            .zip(&reg.board_origins)
            .zip(&reg.board_sources))
        .map(|((b, o), s)| public("board", &b.id, *o, s))
        .collect(),
    };
    serde_json::to_string(&all).expect("definitions serialise")
}

/// A loaded definition's TOML, re-written: values only, no `doctor`.
/// With `scrub` (a project's definition), a string naming a referenced
/// file takes its new name from `renames`, and every other string that is
/// not [`page_safe`] becomes [`PROJECT_TEXT`].
fn public_toml(text: &str, scrub: bool, renames: &[(String, String)]) -> String {
    let mut t: toml::Table = toml::from_str(text).expect("a loaded definition parses");
    t.remove("doctor");
    let mut v = toml::Value::Table(t);
    if scrub {
        scrub_value(&mut v, renames);
    }
    toml::to_string(&v).expect("a table serialises")
}

fn scrub_value(v: &mut toml::Value, renames: &[(String, String)]) {
    match v {
        toml::Value::String(s) => {
            if let Some((_, to)) = renames.iter().find(|(from, _)| from == s) {
                *s = to.clone();
            } else if !page_safe(s) {
                *s = PROJECT_TEXT.into();
            }
        }
        toml::Value::Array(a) => a.iter_mut().for_each(|v| scrub_value(v, renames)),
        toml::Value::Table(t) => t.iter_mut().for_each(|(_, v)| scrub_value(v, renames)),
        _ => {}
    }
}

/// `text` without its `#` comment lines; with `scrub`, each comma-separated
/// field is sent trimmed when it is [`page_safe`] and as [`PROJECT_TEXT`]
/// otherwise (the same replacement as in the TOML, so references still
/// meet); nothing of the raw field (padding included) goes out.
fn public_text_file(text: &str, scrub: bool) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .map(|l| {
            if !scrub {
                return format!("{l}\n");
            }
            let fields: Vec<&str> = l
                .split(',')
                .map(str::trim)
                .map(|f| if page_safe(f) { f } else { PROJECT_TEXT })
                .collect();
            format!("{}\n", fields.join(","))
        })
        .collect()
}

pub(super) fn from_json(json: &str) -> Result<Registry, DefError> {
    let form: JsonForm =
        serde_json::from_str(json).or_else(|e| err("definitions JSON", e.to_string()))?;
    // A project's own entries are made safe again here, whatever the
    // sender did (public_json labels each by its origin).
    let from_project = |label: &str| label.starts_with(&format!("{} ", Origin::Project.as_str()));
    let mut reg = empty();
    for s in &form.mcus {
        let files = file_refs(&s.files);
        reg.push_mcu(
            &s.toml,
            &Pkg::Embedded {
                label: &s.label,
                files: &files,
            },
            // JSON comes from outside: never trusted to run anything.
            Origin::Project,
            from_project(&s.label),
        )?;
    }
    for s in &form.boards {
        reg.push_board(
            &s.toml,
            &Pkg::Embedded {
                label: &s.label,
                files: &[],
            },
            Origin::Project,
            from_project(&s.label),
        )?;
    }
    validate(&reg)?;
    Ok(reg)
}

pub(super) fn builtin() -> Result<Registry, DefError> {
    let mut reg = empty();
    for (label, text, files) in BUILTIN_MCUS {
        reg.push_mcu(
            text,
            &Pkg::Embedded { label, files },
            Origin::Builtin,
            false,
        )?;
    }
    for (label, text) in BUILTIN_BOARDS {
        reg.push_board(
            text,
            &Pkg::Embedded { label, files: &[] },
            Origin::Builtin,
            false,
        )?;
    }
    validate(&reg)?;
    Ok(reg)
}

pub(super) fn load(sources: &[Source<'_>]) -> Result<Registry, DefError> {
    let trees: Vec<&Path> = sources
        .iter()
        .filter_map(|s| match s {
            Source::Tree(p) => Some(*p),
            Source::Project(_) => None,
        })
        .collect();
    let mut reg = match trees[..] {
        [] => builtin()?,
        [root] => tree(root)?,
        _ => return err("sources", "more than one DAGON source tree"),
    };
    for s in sources {
        if let Source::Project(toml) = s {
            project(&mut reg, toml)?;
        }
    }
    validate(&reg)?;
    Ok(reg)
}

/// Package folders under `dir` holding `file`, sorted by name.
fn packages(dir: &Path, file: &str) -> Result<Vec<PathBuf>, DefError> {
    let entries = std::fs::read_dir(dir).or_else(|e| err(dir.display(), e.to_string()))?;
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join(file).is_file())
        .collect();
    found.sort();
    Ok(found)
}

fn read(path: &Path) -> Result<String, DefError> {
    std::fs::read_to_string(path).or_else(|e| err(path.display(), e.to_string()))
}

fn tree(root: &Path) -> Result<Registry, DefError> {
    let targets = root.join("targets");
    let mut reg = empty();
    for dir in packages(&targets.join("mcu"), MCU_FILE)? {
        let text = read(&dir.join(MCU_FILE))?;
        reg.push_mcu(&text, &Pkg::Dir(dir), Origin::Tree, false)?;
    }
    for dir in packages(&targets.join("boards"), BOARD_FILE)? {
        let text = read(&dir.join(BOARD_FILE))?;
        reg.push_board(&text, &Pkg::Dir(dir), Origin::Tree, false)?;
    }
    Ok(reg)
}

#[derive(serde::Deserialize)]
struct ProjectDefs {
    #[serde(default)]
    board: toml::Table,
    #[serde(default)]
    mcu_files: Vec<String>,
}

/// Adds a project's definitions; an id already known is an error.
fn project(reg: &mut Registry, dagon_toml: &Path) -> Result<(), DefError> {
    let at = dagon_toml.display();
    let defs: ProjectDefs =
        toml::from_str(&read(dagon_toml)?).or_else(|e| err(&at, e.to_string()))?;
    let base = Pkg::Dir(dagon_toml.parent().unwrap_or(Path::new(".")).to_path_buf());
    for rel in &defs.mcu_files {
        base.check(rel)?;
        let Pkg::Dir(dir) = &base else { unreachable!() };
        let path = dir.join(rel);
        let pkg = Pkg::Dir(path.parent().unwrap_or(dir).to_path_buf());
        let text = read(&path)?;
        let id = parse_mcu(&text, &pkg)?.id;
        if reg.mcu(&id).is_some() {
            return err(path.display(), format!("MCU '{id}' is already defined"));
        }
        reg.push_mcu(&text, &pkg, Origin::Project, true)?;
    }
    for (id, table) in defs.board {
        let toml::Value::Table(mut t) = table else {
            return err(&at, format!("board.{id} must be a table"));
        };
        if t.contains_key("id") {
            return err(
                &at,
                format!("board.{id}: the id is the table name, drop 'id'"),
            );
        }
        t.insert("id".into(), toml::Value::String(id.clone()));
        let text = toml::to_string(&t).or_else(|e| err(&at, e.to_string()))?;
        let label = format!("{at} [board.{id}]");
        check_format(&text, &label)?;
        let b: BoardDef = toml::from_str(&text).or_else(|e| err(&label, e.to_string()))?;
        if !b.parts.is_empty() {
            return err(
                &at,
                format!("board.{id}: an inline board has no package, so no parts"),
            );
        }
        if reg.board(&b.id).is_some() {
            return err(&at, format!("board '{id}' is already defined"));
        }
        reg.push_board(
            &text,
            &Pkg::Embedded {
                label: &label,
                files: &[],
            },
            Origin::Project,
            true,
        )?;
    }
    Ok(())
}

/// Refuses a definition in a format this reader does not know, before its
/// fields are read (a newer format may have fields this one rejects).
fn check_format(text: &str, at: impl fmt::Display) -> Result<(), DefError> {
    let n = toml::from_str::<toml::Table>(text)
        .ok()
        .and_then(|t| t.get("format").cloned());
    match n {
        None => Ok(()),
        Some(toml::Value::Integer(n)) if n == i64::from(FORMAT) => Ok(()),
        Some(v) => err(
            at,
            format!(
                "definition format {v}; this dagon reads definition format {FORMAT}                  (use a dagon release that reads it; docs/versioning.md)"
            ),
        ),
    }
}

fn parse_board(text: &str, pkg: &Pkg<'_>) -> Result<BoardDef, DefError> {
    check_format(text, pkg.label())?;
    let b: BoardDef = toml::from_str(text).or_else(|e| err(pkg.label(), e.to_string()))?;
    pkg.check_parts(&b.parts)?;
    Ok(b)
}

fn parse_mcu(text: &str, pkg: &Pkg<'_>) -> Result<McuDef, DefError> {
    check_format(text, pkg.label())?;
    let mut m: McuDef = toml::from_str(text).or_else(|e| err(pkg.label(), e.to_string()))?;
    let at = format!("{} (MCU '{}')", pkg.label(), m.id);
    if let Some(rel) = &m.partitions {
        m.partition_table = partitions(&pkg.text(rel)?).or_else(|e| err(&at, e))?;
    }
    if let Writer::Espflash {
        bootloader,
        checksums,
        ..
    } = &m.writer
    {
        pkg.check(bootloader)?;
        pkg.check(checksums)?;
    }
    pkg.check_parts(&m.parts)?;
    m.slots = m
        .slot_defs
        .iter()
        .map(|s| resolve(&m.partition_table, s))
        .collect::<Result<_, _>>()
        .or_else(|e| err(&at, e))?;
    super::requirements::check_mcu(&m).or_else(|e| err(&at, e))?;
    Ok(m)
}

fn number(s: &str) -> Option<u32> {
    let s = s.trim();
    let (digits, mul) = match s.as_bytes().last() {
        Some(b'K' | b'k') => (&s[..s.len() - 1], 1024),
        Some(b'M' | b'm') => (&s[..s.len() - 1], 1024 * 1024),
        _ => (s, 1),
    };
    let v = match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(h) => u32::from_str_radix(h, 16).ok()?,
        None => digits.parse().ok()?,
    };
    v.checked_mul(mul)
}

/// `# Name, Type, SubType, Offset, Size` rows; offsets and sizes explicit.
fn partitions(csv: &str) -> Result<Vec<Partition>, String> {
    csv.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            match (
                f.first(),
                f.get(3).and_then(|o| number(o)),
                f.get(4).and_then(|s| number(s)),
            ) {
                (Some(name), Some(offset), Some(size)) => Ok(Partition {
                    name: (*name).to_string(),
                    offset,
                    size,
                }),
                _ => Err(format!("partition row '{l}' needs a name, offset and size")),
            }
        })
        .collect()
}

fn partition<'p>(table: &'p [Partition], name: &str) -> Result<&'p Partition, String> {
    table
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("no partition '{name}' in the partition table"))
}

fn resolve(table: &[Partition], s: &super::SlotDef) -> Result<Slot, String> {
    let what = format!("slot {:?}/{:?}", s.image, s.binding);
    let (start, size) = match (s.at, s.size, &s.partition) {
        (Some(at), Some(size), None) => (at, size),
        (None, None, Some(name)) => {
            let p = partition(table, name)?;
            (p.offset, p.size)
        }
        _ => return Err(format!("{what}: give either at + size or partition")),
    };
    // Saturating: sizes and bounds are the requirements' to report
    // (requirements::slots), resolution only must not wrap.
    let slot = |header_addr, load_addr, max_payload| Slot {
        image: s.image,
        binding: s.binding,
        placement: s.header,
        start,
        size,
        header_addr,
        load_addr,
        max_payload,
    };
    let payload = size.saturating_sub(HEADER);
    match (s.header_at, &s.header_partition, s.header) {
        (None, None, Placement::Inline) => Ok(slot(start, start.saturating_add(HEADER), payload)),
        (None, None, Placement::Trailer) => Ok(slot(
            start.saturating_add(size).saturating_sub(HEADER),
            start,
            payload,
        )),
        (Some(at), None, Placement::Detached) => Ok(slot(at, start, size)),
        (None, Some(name), Placement::Detached) => {
            let p = partition(table, name)?;
            Ok(slot(p.offset.saturating_add(s.header_offset), start, size))
        }
        (_, _, Placement::Detached) => Err(format!(
            "{what}: a detached header needs header_at or header_partition"
        )),
        _ => Err(format!(
            "{what}: header_at/header_partition are for detached headers"
        )),
    }
}

/// Registry-wide rules: unique ids, wire ids and flash map names; every
/// board's MCU exists and its flash holds one of the MCU's flash maps.
fn validate(reg: &Registry) -> Result<(), DefError> {
    for (i, m) in reg.mcus.iter().enumerate() {
        let at = format!("MCU '{}'", m.id);
        if m.id.is_empty() {
            return err("MCU", "id is empty");
        }
        if let Some(o) = reg.mcus[..i].iter().find(|o| o.id == m.id) {
            return err(at, format!("defined twice ({})", o.name));
        }
        if let Some(o) = reg.mcus[..i].iter().find(|o| o.wire_id == m.wire_id) {
            return err(
                at,
                format!("wire_id {:#06x} is MCU '{}''s too", m.wire_id, o.id),
            );
        }
        for f in &m.flash_maps {
            let twice = reg.mcus[..=i]
                .iter()
                .flat_map(|o| &o.flash_maps)
                .filter(|g| g.name == f.name)
                .count()
                > 1;
            if twice {
                return err(at, format!("flash map '{}' is defined twice", f.name));
            }
        }
    }
    for (i, b) in reg.boards.iter().enumerate() {
        let at = format!("board '{}'", b.id);
        if b.id.is_empty() {
            return err("board", "id is empty");
        }
        if reg.boards[..i].iter().any(|o| o.id == b.id) {
            return err(at, "defined twice");
        }
        let Some(m) = reg.mcu(&b.mcu) else {
            return err(at, format!("MCU '{}' is not defined", b.mcu));
        };
        super::requirements::check_board(b, m).or_else(|e| err(&at, e))?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn parse_mcu_text(text: &str, files: &[(&str, &str)]) -> Result<McuDef, DefError> {
    parse_mcu(
        text,
        &Pkg::Embedded {
            label: "test",
            files,
        },
    )
}

#[cfg(test)]
pub(super) fn parse_board_dir(dir: &Path) -> Result<BoardDef, DefError> {
    parse_board(&read(&dir.join(BOARD_FILE))?, &Pkg::Dir(dir.to_path_buf()))
}

#[cfg(test)]
pub(super) fn parse_mcu_dir(dir: &Path) -> Result<McuDef, DefError> {
    parse_mcu(&read(&dir.join(MCU_FILE))?, &Pkg::Dir(dir.to_path_buf()))
}
