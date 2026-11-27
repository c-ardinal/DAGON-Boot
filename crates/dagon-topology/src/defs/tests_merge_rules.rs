//! The merge-key table against the definition types: a walk of their serde
//! schema finds every array of tables and every variant table, and each
//! needs a row; then every row is merged by its rule on TOML built from the
//! row alone, so a new row is tested without new test code.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;

use serde::de::{self, Deserialize, DeserializeSeed, IntoDeserializer, Visitor};
use toml::{Table, Value};

use super::merge::{check_keys, merge, rule, Def, Rule, ROWS};
use super::{BoardDef, McuDef};

// ------------------------------------------------------------ schema walk

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    /// A struct field.
    Field,
    /// An array whose entries are tables.
    Tables,
    /// A value of an untagged, internally tagged or data-carrying enum:
    /// a table whose shape depends on its variant.
    Variant,
    /// A table of keys the type does not name (a map): merged key by key,
    /// as every table without a row.
    Map,
}

#[derive(Debug)]
struct Stop(String);

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Stop {}

impl de::Error for Stop {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Stop(msg.to_string())
    }
}

/// What a walk found, and the fields it leaves out on its next pass (each
/// left out once it failed, so the fields after it are reached).
#[derive(Default)]
struct Walk {
    found: RefCell<BTreeSet<(String, Kind)>>,
    skip: RefCell<BTreeSet<String>>,
    variant: usize,
}

impl Walk {
    fn note(&self, path: &str, kind: Kind) {
        self.found.borrow_mut().insert((path.to_string(), kind));
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// A deserializer that answers every request with a placeholder of the
/// asked shape, noting the shape at its path.
struct Tracer<'w> {
    walk: &'w Walk,
    path: String,
    in_array: bool,
}

impl<'w> Tracer<'w> {
    fn at(&self, path: String, in_array: bool) -> Tracer<'w> {
        Tracer {
            walk: self.walk,
            path,
            in_array,
        }
    }
}

macro_rules! number {
    ($($f:ident)*) => {$(
        fn $f<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
            v.visit_u64(0)
        }
    )*};
}

impl<'de, 'w> de::Deserializer<'de> for Tracer<'w> {
    type Error = Stop;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Stop> {
        self.walk.note(&self.path, Kind::Variant);
        Err(Stop("variant".into()))
    }

    number!(deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64
            deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64);

    fn deserialize_bool<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_bool(false)
    }
    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_f64(0.0)
    }
    fn deserialize_f64<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_f64(0.0)
    }
    fn deserialize_char<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_char('a')
    }
    fn deserialize_str<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_str("")
    }
    fn deserialize_string<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_str("")
    }
    fn deserialize_bytes<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_bytes(&[])
    }
    fn deserialize_byte_buf<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_bytes(&[])
    }
    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_some(self)
    }
    fn deserialize_unit<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_unit()
    }
    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        v: V,
    ) -> Result<V::Value, Stop> {
        v.visit_unit()
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        v: V,
    ) -> Result<V::Value, Stop> {
        v.visit_newtype_struct(self)
    }
    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        let one = self.at(self.path.clone(), true);
        v.visit_seq(Items(vec![one]))
    }
    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, v: V) -> Result<V::Value, Stop> {
        let items = (0..len)
            .map(|_| self.at(self.path.clone(), false))
            .collect();
        v.visit_seq(Items(items))
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        len: usize,
        v: V,
    ) -> Result<V::Value, Stop> {
        self.deserialize_tuple(len, v)
    }
    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        self.walk.note(&self.path, Kind::Map);
        v.visit_map(Fields {
            tracer: self,
            fields: &[],
            next: 0,
            current: None,
        })
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Stop> {
        if self.in_array {
            self.walk.note(&self.path, Kind::Tables);
        }
        for f in fields {
            self.walk.note(&join(&self.path, f), Kind::Field);
        }
        v.visit_map(Fields {
            tracer: self,
            fields,
            next: 0,
            current: None,
        })
    }
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Stop> {
        let name = variants[self.walk.variant % variants.len()];
        v.visit_enum(OneVariant { tracer: self, name })
    }
    fn deserialize_identifier<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_str("")
    }
    fn deserialize_ignored_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Stop> {
        v.visit_unit()
    }
}

struct Items<'w>(Vec<Tracer<'w>>);

impl<'de, 'w> de::SeqAccess<'de> for Items<'w> {
    type Error = Stop;
    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Stop> {
        if self.0.is_empty() {
            return Ok(None);
        }
        seed.deserialize(self.0.remove(0)).map(Some)
    }
}

struct Fields<'w> {
    tracer: Tracer<'w>,
    fields: &'static [&'static str],
    next: usize,
    current: Option<String>,
}

impl<'de, 'w> de::MapAccess<'de> for Fields<'w> {
    type Error = Stop;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Stop> {
        while let Some(f) = self.fields.get(self.next) {
            self.next += 1;
            let path = join(&self.tracer.path, f);
            if self.tracer.walk.skip.borrow().contains(&path) {
                continue;
            }
            self.current = Some(path);
            return seed.deserialize(f.into_deserializer()).map(Some);
        }
        Ok(None)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Stop> {
        let path = self.current.take().expect("a key first");
        let walk = self.tracer.walk;
        let before = walk.skip.borrow().len();
        let r = seed.deserialize(self.tracer.at(path.clone(), false));
        // Left out next time only where the failure started (not a field
        // whose own field failed: that one is left out instead).
        if r.is_err() && walk.skip.borrow().len() == before {
            walk.skip.borrow_mut().insert(path);
        }
        r
    }
}

struct OneVariant<'w> {
    tracer: Tracer<'w>,
    name: &'static str,
}

impl<'de, 'w> de::EnumAccess<'de> for OneVariant<'w> {
    type Error = Stop;
    type Variant = Tracer<'w>;
    fn variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<(T::Value, Tracer<'w>), Stop> {
        let name: de::value::StrDeserializer<'_, Stop> = self.name.into_deserializer();
        Ok((seed.deserialize(name)?, self.tracer))
    }
}

impl<'de, 'w> de::VariantAccess<'de> for Tracer<'w> {
    type Error = Stop;
    fn unit_variant(self) -> Result<(), Stop> {
        Ok(())
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Stop> {
        self.walk.note(&self.path, Kind::Variant);
        let inner = self.at(self.path.clone(), false);
        seed.deserialize(inner)
    }
    fn tuple_variant<V: Visitor<'de>>(self, len: usize, v: V) -> Result<V::Value, Stop> {
        self.walk.note(&self.path, Kind::Variant);
        de::Deserializer::deserialize_tuple(self.at(self.path.clone(), false), len, v)
    }
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value, Stop> {
        self.walk.note(&self.path, Kind::Variant);
        de::Deserializer::deserialize_struct(self.at(self.path.clone(), false), "", fields, v)
    }
}

/// Every (path, kind) the definition type `T` has.
fn schema<T: for<'de> Deserialize<'de>>() -> BTreeSet<(String, Kind)> {
    let mut all = BTreeSet::new();
    // Each pick of an externally tagged enum's variant, to a fixed point.
    for variant in 0..8 {
        let walk = Walk {
            variant,
            ..Walk::default()
        };
        loop {
            let before = walk.skip.borrow().len();
            let _ = T::deserialize(Tracer {
                walk: &walk,
                path: String::new(),
                in_array: false,
            });
            if walk.skip.borrow().len() == before {
                break;
            }
        }
        all.extend(walk.found.into_inner());
    }
    all
}

fn whole_below(def: Def, path: &str) -> bool {
    let segs: Vec<&str> = path.split('.').collect();
    (1..segs.len()).any(|n| {
        matches!(
            rule(def, &segs[..n].join(".")),
            Some(Rule::Whole | Rule::AddOrDelete(_) | Rule::Fixed)
        )
    })
}

/// What the table lacks for `T`'s schema, or holds that `T` does not have.
fn table_gaps<T: for<'de> Deserialize<'de>>(def: Def) -> Vec<String> {
    let found = schema::<T>();
    let variants: Vec<&str> = found
        .iter()
        .filter(|(_, k)| *k == Kind::Variant)
        .map(|(p, _)| p.as_str())
        .collect();
    let mut gaps = Vec::new();
    for (path, kind) in &found {
        if matches!(kind, Kind::Field | Kind::Map) || whole_below(def, path) {
            continue;
        }
        let ok = matches!(
            (kind, rule(def, path)),
            (
                Kind::Tables,
                Some(Rule::Keyed(_) | Rule::AddOrDelete(_) | Rule::Whole | Rule::Fixed),
            ) | (Kind::Variant, Some(Rule::Whole | Rule::Fixed))
        );
        if !ok {
            let what = match kind {
                Kind::Tables => "array of tables",
                _ => "table whose shape follows its variant (tagged enum or fact)",
            };
            gaps.push(format!(
                "{def:?} definition: {what} `{path}` needs its row in the merge-key table \
                 (defs/merge.rs ROWS): {:?}",
                rule(def, path)
            ));
        }
    }
    for row in ROWS.iter().filter(|r| r.def == def) {
        let known = found.iter().any(|(p, _)| p == row.path)
            || variants
                .iter()
                .any(|v| row.path.starts_with(&format!("{v}.")));
        if !known {
            gaps.push(format!(
                "{def:?} row `{}` names no field of the definition",
                row.path
            ));
        }
    }
    gaps
}

#[test]
fn every_array_of_tables_and_variant_table_of_the_definitions_has_a_row() {
    let mut gaps = table_gaps::<McuDef>(Def::Mcu);
    gaps.extend(table_gaps::<BoardDef>(Def::Board));
    assert!(gaps.is_empty(), "{}", gaps.join("\n"));
}

#[test]
fn the_schema_walk_finds_arrays_and_variants_through_failures() {
    let mcu = schema::<McuDef>();
    for (path, kind) in [
        ("slot", Kind::Tables),
        ("peripheral.dma_requests", Kind::Tables),
        ("execution.erase_regions", Kind::Tables),
        ("writer", Kind::Variant),
        ("execution.masked_max_us", Kind::Variant),
        ("execution.request_max_ms", Kind::Variant),
        ("connection", Kind::Field),
        ("writers", Kind::Map),
    ] {
        assert!(mcu.contains(&(path.to_string(), kind)), "{path} {kind:?}");
    }
    let board = schema::<BoardDef>();
    assert!(board.contains(&("boot.action.ops".to_string(), Kind::Variant)));
    assert!(board.contains(&("device.regs".to_string(), Kind::Tables)));
}

// ------------------------------------------------------------ per row

fn s(v: &str) -> Value {
    Value::String(v.into())
}

fn table(pairs: &[(&str, Value)]) -> Table {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

/// An entry of a keyed array: every key field `key`, plus `v` and `w`.
fn entry(keys: &[&str], key: &str, extra: &[(&str, Value)]) -> Value {
    let mut t: Table = keys.iter().map(|k| ((*k).to_string(), s(key))).collect();
    t.extend(extra.iter().map(|(k, v)| ((*k).to_string(), v.clone())));
    Value::Table(t)
}

fn keys_of(def: Def, path: &str) -> Option<&'static [&'static str]> {
    match rule(def, path)? {
        Rule::Keyed(k) | Rule::AddOrDelete(k) => Some(k),
        _ => None,
    }
}

/// A definition holding `leaf` at `path`; arrays on the way get one entry.
fn at_path(def: Def, path: &str, leaf: Value) -> Table {
    let segs: Vec<&str> = path.split('.').collect();
    let mut v = leaf;
    for n in (1..segs.len()).rev() {
        let holder = Table::from_iter([(segs[n].to_string(), v)]);
        let q = segs[..n].join(".");
        v = match keys_of(def, &q) {
            Some(keys) => {
                let mut e = holder;
                e.extend(keys.iter().map(|k| ((*k).to_string(), s("p"))));
                Value::Array(vec![Value::Table(e)])
            }
            None => Value::Table(holder),
        };
    }
    Table::from_iter([(segs[0].to_string(), v)])
}

/// The value at `path` of a table built by [`at_path`].
fn get(def: Def, t: &Table, path: &str) -> Value {
    let mut v = Value::Table(t.clone());
    let segs: Vec<&str> = path.split('.').collect();
    for (n, seg) in segs.iter().enumerate() {
        v = v.as_table().unwrap()[*seg].clone();
        if n + 1 < segs.len() && keys_of(def, &segs[..=n].join(".")).is_some() {
            v = v.as_array().unwrap()[0].clone();
        }
    }
    v
}

/// A kept field `path` of the entries of an add-or-delete array (its
/// parent): an entry the definition has keeps its value, even removed and
/// added again; an added entry names only a value an entry already has.
fn same_in_entries(def: Def, path: &str) {
    let (array, field) = path
        .rsplit_once('.')
        .expect("a field of an array's entries");
    let Some(Rule::AddOrDelete(k)) = rule(def, array) else {
        panic!("{path}: a kept field below an array sits in an add-or-delete array's entries");
    };
    let one = |v: Vec<Value>| at_path(def, array, Value::Array(v));
    let base = one(vec![
        entry(k, "p", &[(field, s("a"))]),
        entry(k, "q", &[(field, s("b"))]),
    ]);
    let again = |name: &str, value: &str| {
        one(vec![
            entry(k, name, &[("dagon-remove", true.into())]),
            entry(k, name, &[(field, s(value))]),
        ])
    };
    assert!(merge(def, &base, &again("p", "a")).is_ok(), "{path}");
    // Each value is the definition's, yet `p` would change its own.
    let e = merge(def, &base, &again("p", "b")).unwrap_err();
    assert!(e.contains(path), "{path}: {e}");
    assert!(
        merge(def, &base, &one(vec![entry(k, "n", &[(field, s("b"))])])).is_ok(),
        "{path}"
    );
    let e = merge(def, &base, &one(vec![entry(k, "n", &[(field, s("c"))])])).unwrap_err();
    assert!(e.contains(path), "{path}: {e}");
}

#[test]
fn every_row_merges_by_its_rule() {
    for row in ROWS {
        let (def, path) = (row.def, row.path);
        let parent = path.rsplit_once('.').map(|(a, _)| a);
        if row.rule == Rule::Same && parent.and_then(|a| keys_of(def, a)).is_some() {
            same_in_entries(def, path);
            continue;
        }
        if whole_below(def, path) && row.rule != Rule::Same {
            // Inside an entry replaced whole: nothing merges below it.
            assert_eq!(row.rule, Rule::Whole, "{path}");
            continue;
        }
        let one = |v: Vec<Value>| at_path(def, path, Value::Array(v));
        match row.rule {
            Rule::Keyed(k) => {
                let base = one(vec![
                    entry(k, "a", &[("v", 1.into()), ("w", 1.into())]),
                    entry(k, "b", &[("v", 1.into())]),
                ]);
                let over = one(vec![
                    entry(k, "a", &[("v", 2.into())]),
                    entry(k, "b", &[("dagon-remove", true.into())]),
                    entry(k, "c", &[("v", 3.into())]),
                ]);
                let got = merge(def, &base, &over).unwrap();
                let want = Value::Array(vec![
                    entry(k, "a", &[("v", 2.into()), ("w", 1.into())]),
                    entry(k, "c", &[("v", 3.into())]),
                ]);
                assert_eq!(get(def, &got, path), want, "{path}");
                // An entry's optional field is unset; a key field is not.
                let unset = one(vec![entry(
                    k,
                    "a",
                    &[("dagon-unset", Value::Array(vec![s("w")]))],
                )]);
                let got = merge(def, &base, &unset).unwrap();
                assert_eq!(
                    get(def, &got, path).as_array().unwrap()[0],
                    entry(k, "a", &[("v", 1.into())]),
                    "{path}"
                );
                let key = one(vec![entry(
                    k,
                    "a",
                    &[("dagon-unset", Value::Array(vec![s(k[0])]))],
                )]);
                assert!(merge(def, &base, &key).is_err(), "{path}");
            }
            Rule::AddOrDelete(k) => {
                let base = one(vec![entry(k, "a", &[("v", 1.into())]), entry(k, "b", &[])]);
                let over = one(vec![
                    entry(k, "b", &[("dagon-remove", true.into())]),
                    entry(k, "c", &[("v", 3.into())]),
                ]);
                let got = merge(def, &base, &over).unwrap();
                let want = Value::Array(vec![
                    entry(k, "a", &[("v", 1.into())]),
                    entry(k, "c", &[("v", 3.into())]),
                ]);
                assert_eq!(get(def, &got, path), want, "{path}");
                let partial = one(vec![entry(k, "a", &[("v", 2.into())])]);
                assert!(merge(def, &base, &partial).is_err(), "{path}");
                // Removed and added again: a whole new entry.
                let again = one(vec![
                    entry(k, "a", &[("dagon-remove", true.into())]),
                    entry(k, "a", &[("v", 2.into())]),
                ]);
                let got = merge(def, &base, &again).unwrap();
                assert_eq!(
                    get(def, &got, path),
                    Value::Array(vec![entry(k, "b", &[]), entry(k, "a", &[("v", 2.into())])]),
                    "{path}"
                );
            }
            Rule::Whole => {
                let base = one(vec![Value::Table(table(&[
                    ("x", 1.into()),
                    ("y", 1.into()),
                ]))]);
                let new = Value::Array(vec![Value::Table(table(&[("x", 2.into())]))]);
                let got = merge(def, &base, &at_path(def, path, new.clone())).unwrap();
                assert_eq!(get(def, &got, path), new, "{path}");
                // A table (a variant or a fact) is replaced, not merged.
                let base = at_path(
                    def,
                    path,
                    Value::Table(table(&[("kind", s("a")), ("x", 1.into())])),
                );
                let new = Value::Table(table(&[("kind", s("b")), ("y", 2.into())]));
                let got = merge(def, &base, &at_path(def, path, new.clone())).unwrap();
                assert_eq!(get(def, &got, path), new, "{path}");
            }
            Rule::Fixed => {
                let base = one(vec![Value::Table(table(&[("x", 1.into())]))]);
                let e = merge(def, &base, &one(vec![])).unwrap_err();
                assert!(e.contains(path), "{path}: {e}");
            }
            Rule::Same => {
                let base = at_path(def, path, s("a"));
                assert!(
                    merge(def, &base, &at_path(def, path, s("a"))).is_ok(),
                    "{path}"
                );
                let e = merge(def, &base, &at_path(def, path, s("b"))).unwrap_err();
                assert!(e.contains(path), "{path}: {e}");
            }
            Rule::WholeWhenChanged(key) => {
                let base = at_path(
                    def,
                    path,
                    Value::Table(table(&[(key, s("a")), ("x", 1.into())])),
                );
                let got = merge(
                    def,
                    &base,
                    &at_path(def, path, Value::Table(table(&[("x", 2.into())]))),
                )
                .unwrap();
                assert_eq!(
                    get(def, &got, path),
                    Value::Table(table(&[(key, s("a")), ("x", 2.into())])),
                    "{path}"
                );
                let half = at_path(def, path, Value::Table(table(&[(key, s("b"))])));
                let e = merge(def, &base, &half).unwrap_err();
                assert!(e.contains(path) && e.contains(key), "{path}: {e}");
                let whole = Value::Table(table(&[(key, s("b")), ("x", 3.into())]));
                let got = merge(def, &base, &at_path(def, path, whole.clone())).unwrap();
                assert_eq!(get(def, &got, path), whole, "{path}");
            }
        }
    }
}

#[test]
fn every_ordered_row_takes_a_full_order_and_refuses_any_other() {
    for row in ROWS.iter().filter(|r| r.ordered) {
        let (def, path) = (row.def, row.path);
        let k = keys_of(def, path).expect("an ordered row has a key");
        assert_eq!(k.len(), 1, "{path}: an order names one key field");
        assert!(
            !(1..path.split('.').count()).any(|n| keys_of(
                def,
                &path.split('.').take(n).collect::<Vec<_>>().join(".")
            )
            .is_some()),
            "{path}: an ordered array sits in tables only"
        );
        let base = at_path(
            def,
            path,
            Value::Array(vec![entry(k, "a", &[]), entry(k, "b", &[])]),
        );
        let order = |names: &[&str]| {
            let mut over = Table::new();
            over.insert(
                "dagon-order".into(),
                Value::Table(at_path(
                    def,
                    path,
                    Value::Array(names.iter().map(|n| s(n)).collect()),
                )),
            );
            over
        };
        let got = merge(def, &base, &order(&["b", "a"])).unwrap();
        assert_eq!(
            get(def, &got, path),
            Value::Array(vec![entry(k, "b", &[]), entry(k, "a", &[])]),
            "{path}"
        );
        for bad in [&["a"][..], &["a", "b", "a"], &["a", "c"]] {
            assert!(merge(def, &base, &order(bad)).is_err(), "{path}: {bad:?}");
        }
    }
}

#[test]
fn every_keyed_row_refuses_a_key_given_twice_and_the_builtins_pass() {
    for row in ROWS {
        let Some(k) = keys_of(row.def, row.path) else {
            continue;
        };
        if whole_below(row.def, row.path) {
            continue;
        }
        let twice = at_path(
            row.def,
            row.path,
            Value::Array(vec![entry(k, "a", &[]), entry(k, "a", &[])]),
        );
        let e = check_keys(row.def, &twice).unwrap_err();
        assert!(e.contains(row.path) && e.contains("twice"), "{e}");
        let once = at_path(
            row.def,
            row.path,
            Value::Array(vec![entry(k, "a", &[]), entry(k, "b", &[])]),
        );
        check_keys(row.def, &once).unwrap();
    }
    let (mcus, boards) = super::load::builtin_texts();
    for t in mcus {
        check_keys(Def::Mcu, &t.parse().unwrap()).unwrap();
    }
    for t in boards {
        check_keys(Def::Board, &t.parse().unwrap()).unwrap();
    }
}
