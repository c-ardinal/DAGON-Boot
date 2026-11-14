//! A project's override of a definition (`[mcu_override.<id>]` and
//! `[board_override.<id>]` in dagon.toml) merged into the definition's TOML
//! before it is parsed, so the merged text is checked by the same rules as
//! any definition.
//!
//! Tables merge key by key. A value that is not a table on either side is
//! replaced. Arrays of tables and tables whose shape follows a variant go
//! by their row of [`ROWS`]; an array of plain values is replaced whole.
//! The merge-only keys never reach the definition types:
//! - `dagon-remove = true` with an entry's key fields (or alone in an
//!   optional table) deletes it;
//! - `dagon-unset = ["field", ...]` removes optional fields of an entry or
//!   table before the override's own fields apply;
//! - `dagon-order.<array> = [key, ...]`, at the top of the override, gives
//!   the order of an ordered array, every key exactly once.

use std::collections::BTreeSet;

use toml::{Table, Value};

/// Which definition an override changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Def {
    Mcu,
    Board,
}

/// How the override's value at a row's path meets the definition's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// An array of tables merged entry by entry by these key fields: an
    /// override entry changes the fields it writes; a new key is appended.
    Keyed(&'static [&'static str]),
    /// An array of tables whose entries are added or deleted whole, by
    /// these key fields.
    AddOrDelete(&'static [&'static str]),
    /// Replaced whole when given: an array whose order or entries carry no
    /// key, or a table whose shape follows its variant (a tagged enum, a
    /// fact).
    Whole,
    /// A table merged key by key, given whole when the field named here
    /// changes (none of the old table's other values carry over).
    WholeWhenChanged(&'static str),
    /// Never written by an override.
    Fixed,
    /// Kept as the definition has it: its identity, or a file of its
    /// package (which the project does not own). In an array, an entry the
    /// definition has keeps its value (removed and added again too); an
    /// entry added names only a value one of the definition's entries names.
    Same,
}

/// One row of the merge-key table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub def: Def,
    /// Field names from the definition's top, `.`-separated; an array's
    /// entries share the array's path.
    pub path: &'static str,
    pub rule: Rule,
    /// `dagon-order` may give the order (its first entry carries meaning).
    pub ordered: bool,
}

const fn row(def: Def, path: &'static str, rule: Rule) -> Row {
    Row {
        def,
        path,
        rule,
        ordered: false,
    }
}

const fn ordered(def: Def, path: &'static str, rule: Rule) -> Row {
    Row {
        def,
        path,
        rule,
        ordered: true,
    }
}

use Def::{Board, Mcu};
use Rule::{AddOrDelete, Fixed, Keyed, Same, Whole, WholeWhenChanged};

/// The merge-key table: every array of tables and every variant table of
/// the definition types has its row (a test walks their serde schema), and
/// the fields an override must keep.
pub const ROWS: &[Row] = &[
    // The first flash map is the default.
    ordered(Mcu, "flash_map", Keyed(&["name"])),
    row(Mcu, "slot", Keyed(&["image", "binding"])),
    row(Mcu, "part", AddOrDelete(&["name"])),
    row(Mcu, "doctor", Fixed),
    row(Mcu, "peripheral", Keyed(&["id"])),
    // The order is a priority within the peripheral.
    row(Mcu, "peripheral.dma_requests", Whole),
    // One value or a list.
    row(Mcu, "peripheral.privileged_why", Whole),
    // DAGON runs on the first core.
    ordered(Mcu, "core", AddOrDelete(&["id"])),
    row(Mcu, "rom_i2c.pairs", Keyed(&["sda", "scl"])),
    // A topology link end names a port by `name`.
    row(Mcu, "link_port", Keyed(&["name"])),
    // One relay link per port; the id may change.
    row(Mcu, "relay_link", Keyed(&["port"])),
    // "seven-bit" or { fixed = N }.
    row(Mcu, "relay_link.addressing", Whole),
    // In boot order.
    ordered(Mcu, "boot.record.rings", Keyed(&["writer"])),
    row(Mcu, "boot.record.fuse.cells", Keyed(&["image"])),
    row(Mcu, "boot.record.entropy.min_bits", Whole),
    row(Mcu, "isolation.capabilities.idau", Whole),
    row(Mcu, "isolation.tcm", Whole),
    row(Mcu, "slot_layout", Keyed(&["target", "slot"])),
    row(Mcu, "slot_layout.header", Whole),
    row(Mcu, "execution.erase_regions", Keyed(&["start"])),
    row(Mcu, "execution.erase_regions.unit_max_ms", Whole),
    row(Mcu, "execution.masked_max_us", Whole),
    row(Mcu, "watchdog.flag", Whole),
    row(Mcu, "writer", Whole),
    // The setups are checked as a set (one internal, no two alike).
    row(Mcu, "stage0.clock", Whole),
    row(Mcu, "bootcore.clock", Whole),
    row(Mcu, "app.clock", Whole),
    row(Mcu, "id", Same),
    row(Mcu, "wire_id", Same),
    row(Mcu, "format", Same),
    row(Mcu, "port", Same),
    row(Mcu, "partitions", Same),
    row(Mcu, "writer.bootloader", Same),
    row(Mcu, "writer.checksums", Same),
    row(Mcu, "part.sub", Same),
    row(Board, "header", Keyed(&["pin"])),
    row(Board, "i2c", Keyed(&["sda", "scl"])),
    row(Board, "part", AddOrDelete(&["name"])),
    row(Board, "pin", Keyed(&["pin"])),
    row(Board, "bus", Keyed(&["id"])),
    row(Board, "peripheral", Keyed(&["id"])),
    row(Board, "device", Keyed(&["id"])),
    row(Board, "device.regs", Keyed(&["name"])),
    // Register numbers or names.
    row(Board, "device.writable", Whole),
    row(Board, "boot.action", Whole),
    row(Board, "boot.action.ops", Whole),
    // A new part's erase timings are the part's, never the old part's.
    row(Board, "flash", WholeWhenChanged("part")),
    row(Board, "flash.erase_regions", Whole),
    // "internal", { crystal_hz = N } or { external_hz = N }.
    row(Board, "clock.source", Whole),
    row(Board, "id", Same),
    row(Board, "mcu", Same),
    row(Board, "format", Same),
    row(Board, "part.sub", Same),
];

const REMOVE: &str = "dagon-remove";
const UNSET: &str = "dagon-unset";
const ORDER: &str = "dagon-order";

/// The row at `path`, if any.
pub fn rule(def: Def, path: &str) -> Option<Rule> {
    ROWS.iter()
        .find(|r| r.def == def && r.path == path)
        .map(|r| r.rule)
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn shown(path: &str) -> String {
    if path.is_empty() {
        "the definition".into()
    } else {
        format!("`{path}`")
    }
}

/// `over` merged into `base`, both a whole definition's table.
pub fn merge(def: Def, base: &Table, over: &Table) -> Result<Table, String> {
    if over.contains_key(REMOVE) {
        return Err(format!(
            "{REMOVE} removes an entry or an optional table, not a whole definition"
        ));
    }
    let mut over = over.clone();
    let order = over.remove(ORDER);
    let mut merged = table(def, "", base, &over, &[])?;
    if let Some(order) = order {
        reorder(def, &mut merged, &order, "")?;
    }
    same(def, base, &merged)?;
    Ok(merged)
}

/// `over` merged into the table `base` at `path`; `keys` are the key
/// fields when it is an array entry.
fn table(def: Def, path: &str, base: &Table, over: &Table, keys: &[&str]) -> Result<Table, String> {
    let mut out = base.clone();
    if let Some(list) = over.get(UNSET) {
        unset(def, path, &mut out, list, keys)?;
    }
    for (k, v) in over {
        if k == UNSET || (k == REMOVE && !keys.is_empty()) {
            continue;
        }
        if k.starts_with("dagon-") {
            return Err(format!(
                "{}: `{k}` is no merge key here ({REMOVE}, {UNSET}; {ORDER} at the top)",
                shown(path)
            ));
        }
        let p = join(path, k);
        match value(def, &p, out.get(k), v)? {
            Some(v) => out.insert(k.clone(), v),
            None => out.remove(k),
        };
    }
    Ok(out)
}

fn unset(def: Def, path: &str, out: &mut Table, list: &Value, keys: &[&str]) -> Result<(), String> {
    let names = list
        .as_array()
        .filter(|a| a.iter().all(Value::is_str))
        .ok_or_else(|| format!("{}: {UNSET} takes a list of field names", shown(path)))?;
    for name in names.iter().filter_map(Value::as_str) {
        let p = join(path, name);
        if keys.contains(&name) {
            return Err(format!("`{p}` is a key: an entry keeps its key fields"));
        }
        if rule(def, &p) == Some(Fixed) {
            return Err(format!("`{p}` may not be overridden"));
        }
        if out.remove(name).is_none() {
            return Err(format!(
                "{UNSET} names `{p}`, which the definition does not have"
            ));
        }
    }
    Ok(())
}

/// Whether `t` asks for its own removal (only `true` does).
fn removal(t: &Table, path: &str) -> Result<bool, String> {
    match t.get(REMOVE) {
        None => Ok(false),
        Some(Value::Boolean(true)) => Ok(true),
        Some(_) => Err(format!("{}: {REMOVE} takes only true", shown(path))),
    }
}

/// The merged value at `path`; `None` removes it.
fn value(def: Def, p: &str, base: Option<&Value>, over: &Value) -> Result<Option<Value>, String> {
    let r = rule(def, p);
    if r == Some(Fixed) {
        return Err(format!("`{p}` may not be overridden"));
    }
    if let Value::Table(t) = over {
        if removal(t, p)? {
            if t.len() > 1 {
                return Err(format!("`{p}`: {REMOVE} = true stands alone"));
            }
            return match base {
                Some(Value::Table(_)) => Ok(None),
                _ => Err(format!(
                    "there is no `{p}` to remove: the definition does not have it"
                )),
            };
        }
    }
    match (r, base, over) {
        (Some(Keyed(keys)), _, _) => array(def, p, keys, false, base, over).map(Some),
        (Some(AddOrDelete(keys)), _, _) => array(def, p, keys, true, base, over).map(Some),
        (Some(Whole), _, _) => Ok(Some(over.clone())),
        (Some(WholeWhenChanged(key)), Some(Value::Table(b)), Value::Table(o))
            if o.get(key).is_some_and(|v| Some(v) != b.get(key)) =>
        {
            if let Some(missing) = b.keys().find(|k| !o.contains_key(*k)) {
                return Err(format!(
                    "`{p}`: a new `{key}` brings the whole `{p}` table; `{missing}` is not given"
                ));
            }
            table(def, p, &Table::new(), o, &[]).map(|t| Some(Value::Table(t)))
        }
        (_, Some(Value::Table(b)), Value::Table(o)) => {
            table(def, p, b, o, &[]).map(|t| Some(Value::Table(t)))
        }
        (_, _, Value::Table(o)) => {
            table(def, p, &Table::new(), o, &[]).map(|t| Some(Value::Table(t)))
        }
        _ => Ok(Some(over.clone())),
    }
}

fn key_text(keys: &[&str], vals: &[Value]) -> String {
    keys.iter()
        .zip(vals)
        .map(|(k, v)| format!("{k} = {v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The values of `keys` in entry `t`, every one given.
fn key_of(t: &Table, keys: &[&str], p: &str) -> Result<Vec<Value>, String> {
    keys.iter()
        .map(|k| {
            t.get(*k)
                .cloned()
                .ok_or_else(|| format!("`{p}`: each entry names its key ({})", keys.join(", ")))
        })
        .collect()
}

fn entries<'a>(v: &'a Value, p: &str) -> Result<Vec<&'a Table>, String> {
    v.as_array()
        .and_then(|a| a.iter().map(Value::as_table).collect::<Option<Vec<_>>>())
        .ok_or_else(|| format!("`{p}` takes a list of tables"))
}

/// An array of tables merged by `keys`; `whole`: entries are added or
/// deleted only.
fn array(
    def: Def,
    p: &str,
    keys: &[&str],
    whole: bool,
    base: Option<&Value>,
    over: &Value,
) -> Result<Value, String> {
    let mut out: Vec<Table> = match base {
        Some(b) => entries(b, p)?.into_iter().cloned().collect(),
        None => Vec::new(),
    };
    let find = |out: &[Table], key: &[Value]| {
        out.iter()
            .position(|e| keys.iter().zip(key).all(|(k, v)| e.get(*k) == Some(v)))
    };
    let over = entries(over, p)?;
    let mut removed = Vec::new();
    for e in &over {
        if !removal(e, p)? {
            continue;
        }
        let key = key_of(e, keys, p)?;
        if e.keys().any(|k| k != REMOVE && !keys.contains(&k.as_str())) {
            return Err(format!(
                "`{p}` entry {}: {REMOVE} = true goes with the key fields only",
                key_text(keys, &key)
            ));
        }
        let i = find(&out, &key)
            .ok_or_else(|| format!("`{p}` has no entry {} to remove", key_text(keys, &key)))?;
        out.remove(i);
        removed.push(key);
    }
    let mut written: Vec<Vec<Value>> = Vec::new();
    for e in over {
        if removal(e, p)? {
            continue;
        }
        let key = key_of(e, keys, p)?;
        if written.contains(&key) {
            return Err(format!(
                "`{p}` entry {} is given twice",
                key_text(keys, &key)
            ));
        }
        match find(&out, &key) {
            Some(_) if whole => {
                return Err(format!(
                    "`{p}` entry {} is added or deleted whole: remove it ({REMOVE} = true) \
                     and give the whole new entry",
                    key_text(keys, &key)
                ))
            }
            Some(i) => out[i] = table(def, p, &out[i], e, keys)?,
            None => out.push(table(def, p, &Table::new(), e, keys)?),
        }
        written.push(key);
    }
    Ok(Value::Array(out.into_iter().map(Value::Table).collect()))
}

/// Applies each `dagon-order` list under `order` (nested by path).
fn reorder(def: Def, merged: &mut Table, order: &Value, path: &str) -> Result<(), String> {
    let Value::Table(t) = order else {
        return ordered_array(def, merged, order, path);
    };
    for (k, v) in t {
        reorder(def, merged, v, &join(path, k))?;
    }
    Ok(())
}

fn ordered_array(def: Def, merged: &mut Table, list: &Value, p: &str) -> Result<(), String> {
    let row = ROWS
        .iter()
        .find(|r| r.def == def && r.path == p && r.ordered);
    let (
        Some(Row {
            rule: Keyed(keys) | AddOrDelete(keys),
            ..
        }),
        Some(names),
    ) = (row, list.as_array())
    else {
        return Err(format!(
            "{ORDER}.{p}: `{p}` is no array whose order an override gives"
        ));
    };
    let segs: Vec<&str> = p.split('.').collect();
    let mut at = &mut *merged;
    for s in &segs[..segs.len() - 1] {
        at = at
            .get_mut(*s)
            .and_then(Value::as_table_mut)
            .ok_or_else(|| format!("{ORDER}.{p}: the definition has no `{p}`"))?;
    }
    let arr = at
        .get_mut(segs[segs.len() - 1])
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("{ORDER}.{p}: the definition has no `{p}`"))?;
    let key = keys[0];
    let mut rest: Vec<Value> = std::mem::take(arr);
    let mut sorted = Vec::new();
    for n in names {
        let i = rest
            .iter()
            .position(|e| e.get(key) == Some(n))
            .ok_or_else(|| {
                if sorted.iter().any(|e: &Value| e.get(key) == Some(n)) {
                    format!("{ORDER}.{p} names {n} twice")
                } else {
                    format!("{ORDER}.{p} names {n}, which `{p}` does not have")
                }
            })?;
        sorted.push(rest.remove(i));
    }
    if let Some(e) = rest.first() {
        let missing = e.get(key).map_or_else(String::new, Value::to_string);
        return Err(format!(
            "{ORDER}.{p} leaves out {missing}: it lists every entry exactly once"
        ));
    }
    *arr = sorted;
    Ok(())
}

/// A value at a path, with the keys of the array entries it lies in (from
/// the outermost); `None` where the field is left out.
type AtEntry = (Vec<Value>, Option<Value>);

/// The values at `segs` below `v`, which sits at `path` (through arrays, one
/// per entry, each with its entry's key values).
fn values_at(def: Def, v: &Value, path: &str, segs: &[&str]) -> Vec<AtEntry> {
    match (segs.split_first(), v) {
        (None, _) => vec![(Vec::new(), Some(v.clone()))],
        (Some(_), Value::Array(a)) => {
            let keys = match rule(def, path) {
                Some(Keyed(k) | AddOrDelete(k)) => k,
                _ => &[][..],
            };
            a.iter()
                .flat_map(|e| {
                    let own: Vec<Value> = keys.iter().filter_map(|k| e.get(*k).cloned()).collect();
                    values_at(def, e, path, segs)
                        .into_iter()
                        .map(move |(mut chain, x)| {
                            chain.splice(0..0, own.iter().cloned());
                            (chain, x)
                        })
                })
                .collect()
        }
        (Some((s, rest)), Value::Table(t)) => match t.get(*s) {
            Some(inner) => values_at(def, inner, &join(path, s), rest),
            None if rest.is_empty() => vec![(Vec::new(), None)],
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Every [`Rule::Same`] field of `merged` holds what `base` has there: in an
/// entry `base` has (by its keys), the same value; in an entry `base` does
/// not have, a value one of `base`'s entries holds.
fn same(def: Def, base: &Table, merged: &Table) -> Result<(), String> {
    let base = Value::Table(base.clone());
    let merged = Value::Table(merged.clone());
    for r in ROWS.iter().filter(|r| r.def == def && r.rule == Same) {
        let segs: Vec<&str> = r.path.split('.').collect();
        let had = values_at(def, &base, "", &segs);
        let changed = values_at(def, &merged, "", &segs)
            .into_iter()
            .find(|(chain, v)| match had.iter().find(|(c, _)| c == chain) {
                Some((_, old)) => old != v,
                None => !had.iter().any(|(_, old)| old == v),
            });
        if let Some((_, v)) = changed {
            let v = v
                .as_ref()
                .map_or_else(|| "left out".into(), Value::to_string);
            return Err(format!(
                "`{}` may not be changed by an override ({v}): it is the definition's                  identity or names a file of its package; define a new one under its own id instead",
                r.path
            ));
        }
    }
    Ok(())
}

/// The arrays at `path` (one per enclosing entry).
fn arrays_at<'a>(v: &'a Value, segs: &[&str], out: &mut Vec<&'a Vec<Value>>) {
    match (segs.split_first(), v) {
        (None, Value::Array(a)) => out.push(a),
        (Some(_), Value::Array(a)) => a.iter().for_each(|e| arrays_at(e, segs, out)),
        (Some((s, rest)), Value::Table(t)) => {
            if let Some(inner) = t.get(*s) {
                arrays_at(inner, rest, out);
            }
        }
        _ => {}
    }
}

/// Every keyed array of a definition names each key once.
pub fn check_keys(def: Def, t: &Table) -> Result<(), String> {
    let root = Value::Table(t.clone());
    for r in ROWS.iter().filter(|r| r.def == def) {
        let (Keyed(keys) | AddOrDelete(keys)) = r.rule else {
            continue;
        };
        let segs: Vec<&str> = r.path.split('.').collect();
        let mut found = Vec::new();
        arrays_at(&root, &segs, &mut found);
        for a in found {
            let mut seen = BTreeSet::new();
            for e in a.iter().filter_map(Value::as_table) {
                let key: Vec<String> = keys
                    .iter()
                    .map(|k| e.get(*k).map_or_else(String::new, Value::to_string))
                    .collect();
                if !seen.insert(key.clone()) {
                    let vals: Vec<Value> = keys.iter().filter_map(|k| e.get(*k).cloned()).collect();
                    return Err(format!(
                        "{} {} is given twice",
                        r.path,
                        key_text(keys, &vals)
                    ));
                }
            }
        }
    }
    Ok(())
}
