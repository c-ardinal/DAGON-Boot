//! `dagon.toml`, a project's file: its format number and the one list of
//! its top-level keys. Every reader of the file checks both, takes the keys
//! it reads and leaves the others to their readers, so no key (an override
//! least of all) is dropped by a reader that does not know it.

/// The `dagon.toml` format number this reader reads (its `format` key).
pub const FORMAT: u32 = 1;

/// Every top-level key of `dagon.toml`.
pub const KEYS: [&str; 14] = [
    "format",
    "topology",
    "release",
    "provision_dir",
    "pubkey",
    "signing_key",
    "provision",
    "build",
    "dagon",
    "extensions",
    "board",
    "mcu_files",
    "mcu_override",
    "board_override",
];

/// Refuses a `dagon.toml` text that does not parse, is not of [`FORMAT`] or
/// has a top-level key outside [`KEYS`].
pub fn check(text: &str) -> Result<(), String> {
    let t: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    match t.get("format") {
        Some(toml::Value::Integer(n)) if *n == i64::from(FORMAT) => {}
        None => {
            return Err(format!(
                "no `format`; this dagon reads dagon.toml format {FORMAT}: put `format = {FORMAT}` at its top"
            ))
        }
        Some(v) => {
            return Err(format!(
                "dagon.toml format {v}; this dagon reads dagon.toml format {FORMAT} \
                 (use a dagon release that reads it; site/src/content/docs/explanation/versioning.md)"
            ))
        }
    }
    match t.keys().find(|k| !KEYS.contains(&k.as_str())) {
        Some(k) => Err(format!(
            "unknown key `{k}`; dagon.toml takes {}",
            KEYS.join(", ")
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_of_this_format_with_known_keys_passes_and_any_other_is_refused() {
        check("format = 1\ntopology = \"t.yaml\"\n[mcu_override.x]\nname = \"y\"\n").unwrap();
        assert!(check("topology = \"t.yaml\"\n")
            .unwrap_err()
            .contains("format = 1"));
        assert!(check("format = 2\n").unwrap_err().contains("format 2"));
        assert!(check("format = \"1\"\n").is_err());
        assert!(check("format = 1\n[serve]\nx = 1\n")
            .unwrap_err()
            .contains("`serve`"));
    }
}
