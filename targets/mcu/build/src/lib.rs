//! Build-script helpers shared by the DAGON MCU packages: the verifying key
//! embedded at build time, the constants the package's MCU definition
//! states (the one the node build is given), and the linker memory map of
//! an image slot.

use std::path::{Path, PathBuf};
use std::{env, fs};

/// RFC 8032 section 7.1 TEST 1 public key (public test data).
pub const TEST_PUBKEY_HEX: &str =
    "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

fn out_dir() -> PathBuf {
    PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"))
}

/// Writes `$OUT_DIR/pubkey.rs` with `PUBKEY: [u8; 32]` and `TEST_KEY: bool`.
///
/// `DAGON_PUBKEY` is the public key file from `dagon key gen` or its 64 hex
/// digits. Unset, the build fails unless `DAGON_ALLOW_TEST_KEY=1`, which
/// selects the public RFC 8032 test key (with a build warning).
pub fn embed_pubkey() {
    println!("cargo:rerun-if-env-changed=DAGON_PUBKEY");
    println!("cargo:rerun-if-env-changed=DAGON_ALLOW_TEST_KEY");
    let hex = match env::var("DAGON_PUBKEY") {
        Ok(v) if is_key_hex(v.trim()) => v.trim().to_string(),
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("DAGON_PUBKEY: cannot read key file {path}: {e}"))
                .trim()
                .to_string()
        }
        Err(_) => {
            if env::var("DAGON_ALLOW_TEST_KEY").as_deref() != Ok("1") {
                panic!(
                    "DAGON_PUBKEY is unset: set it to the public key file from `dagon key gen` \
                     (or its 64 hex digits), or set DAGON_ALLOW_TEST_KEY=1 to build with the \
                     public RFC 8032 test key (TEST KEY)"
                );
            }
            println!(
                "cargo:warning=DAGON_PUBKEY is unset: built with the public RFC 8032 test key \
                 (TEST KEY), which accepts images signed with a published secret"
            );
            TEST_PUBKEY_HEX.to_string()
        }
    };
    assert!(
        is_key_hex(&hex),
        "DAGON_PUBKEY: expected 64 hex digits (or a file holding them)"
    );
    fs::write(out_dir().join("pubkey.rs"), pubkey_rs(&hex)).unwrap();
}

/// The text of `pubkey.rs` for a key of 64 hex digits.
fn pubkey_rs(hex: &str) -> String {
    let bytes: Vec<String> = (0..32)
        .map(|i| {
            format!(
                "{:#04x}",
                u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()
            )
        })
        .collect();
    format!(
        "/// Verifying key embedded at build time (`DAGON_PUBKEY`).\n\
         pub const PUBKEY: [u8; 32] = [{}];\n\
         /// Built with the public test key: anyone can sign images for it.\n\
         pub const TEST_KEY: bool = {};\n",
        bytes.join(", "),
        hex.eq_ignore_ascii_case(TEST_PUBKEY_HEX)
    )
}

fn is_key_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Writes `$OUT_DIR/memory.x` for an image whose vector table starts at
/// `body` (flash) with `len` bytes, RAM at `ram`/`ram_len`.
pub fn memory_x(body: u32, len: u32, ram: u32, ram_len: u32) {
    write_memory_x(&format!(
        "MEMORY
{{
  FLASH : ORIGIN = {body:#010x}, LENGTH = {len:#x}
           RAM   : ORIGIN = {ram:#010x}, LENGTH = {ram_len:#x}
}}
"
    ));
}

/// Writes `text` as `$OUT_DIR/memory.x` and puts it on the linker search
/// path of the final binary (cortex-m-rt's link.x includes it).
pub fn write_memory_x(text: &str) {
    let out = out_dir();
    fs::write(out.join("memory.x"), text).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
}

/// The variable naming a node's boot folder (`boot/<id>/` of a project).
pub const BOOT_FOLDER_ENV: &str = "DAGON_BOOT_CRATE";
/// The node's MCU definition in its boot folder, as `dagon gen` writes it:
/// the built-in definition with the project's overrides applied.
pub const NODE_DEFINITION: &str = "mcu.toml";

/// Writes `$OUT_DIR/definition.rs`, the constants of the MCU definition the
/// node build is given, and returns that definition (for the build
/// script's memory.x). Include it with
/// `include!(concat!(env!("OUT_DIR"), "/definition.rs"))`.
///
/// The definition is `$DAGON_BOOT_CRATE/mcu.toml` when the variable is set
/// (an absolute path), so the app, boot core and stage0 of one node build
/// from one file; else the package's own `dagon-mcu.toml`. A node
/// definition of another MCU, or in another definition format, is refused.
pub fn definition() -> Definition {
    let dir = manifest_dir();
    let def = Definition::parse(&node_definition_text(), |rel| read(&dir.join(rel)));
    let recovery = env::var_os(BOOT_FOLDER_ENV).and_then(|f| recovery_text(Path::new(&f)));
    let rs = def.rs() + &recovery_rom_rs(recovery.as_deref());
    fs::write(out_dir().join("definition.rs"), rs).unwrap();
    def
}

/// The MCU definition of the node on this node's recovery lines (the node
/// its relay resets and reaches through its ROM loader), with the
/// project's overrides applied, as `dagon gen` writes it beside
/// [`NODE_DEFINITION`]; absent when no node is there.
pub const RECOVERY_DEFINITION: &str = "recovery-mcu.toml";

/// The text of the boot folder's [`RECOVERY_DEFINITION`], if it holds one
/// ([`definition`] has checked the folder's hashes first).
fn recovery_text(folder: &Path) -> Option<String> {
    let path = folder.join(RECOVERY_DEFINITION);
    println!("cargo:rerun-if-changed={}", path.display());
    path.is_file().then(|| read(&path))
}

/// `RECOVERY_ROM_ADDR` of the node whose boot folder is `folder`, as
/// [`definition`] generates it.
pub fn recovery_rom_addr_rs(folder: &Path) -> String {
    recovery_rom_rs(recovery_text(folder).as_deref())
}

/// `RECOVERY_ROM_ADDR`, the I2C address of the ROM loader the relay's
/// recovery reaches: `[rom_i2c] addr` of the recovery node's definition
/// (`recovery`); `None` without one or for an MCU with no ROM loader.
fn recovery_rom_rs(recovery: Option<&str>) -> String {
    let addr = recovery.and_then(|text| {
        let t: toml::Table = text.parse().expect("the recovery node's MCU definition");
        let a = t.get("rom_i2c")?.get("addr")?;
        Some(u8::try_from(int(a, "[rom_i2c] addr")).expect("[rom_i2c] addr fits a u8"))
    });
    format!(
        "/// The I2C address of the ROM loader on this node's recovery lines: the\n\
         /// `[rom_i2c] addr` of that node's MCU definition (`None`: no node there,\n\
         /// or its MCU has no ROM loader).\n\
         pub const RECOVERY_ROM_ADDR: Option<u8> = {};\n",
        addr.map_or("None".into(), |a| format!("Some({a:#04x})"))
    )
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
}

/// The text of the MCU definition the node build is given ([`definition`]).
fn node_definition_text() -> String {
    println!("cargo:rerun-if-env-changed={BOOT_FOLDER_ENV}");
    let own = read(&manifest_dir().join("dagon-mcu.toml"));
    let Some(folder) = env::var_os(BOOT_FOLDER_ENV) else {
        return own;
    };
    let folder = PathBuf::from(folder);
    assert!(
        folder.is_absolute(),
        "{BOOT_FOLDER_ENV}={}: give the boot folder as an absolute path",
        folder.display()
    );
    let path = folder.join(NODE_DEFINITION);
    println!("cargo:rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (`dagon gen` writes the node's MCU definition there)",
            path.display()
        )
    });
    check_defs(&folder, &mut |f| {
        println!("cargo:rerun-if-changed={}", f.display())
    })
    .unwrap_or_else(|e| panic!("{e}"));
    same_mcu(&own, &text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text
}

/// The node's board definition in its boot folder, for a node that names
/// a board (`dagon gen` writes it beside [`NODE_DEFINITION`]).
pub const NODE_BOARD: &str = "board.toml";

/// The hashes `dagon gen` writes beside the node's definitions: one
/// `<SHA-256, 64 lowercase hex digits>  <file name>` line per file, as
/// `sha256sum` writes them ([`defs_text`]). Every build that reads a
/// node's definitions checks them with [`check_defs`], this crate's build
/// helpers and DAGON's board and boot stage builds alike.
pub const DEFS_FILE: &str = "defs.sha256";

fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(bytes))
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The text of [`DEFS_FILE`] for `files` (file name, text).
pub fn defs_text(files: &[(&str, &str)]) -> String {
    files
        .iter()
        .map(|(name, text)| format!("{}  {name}\n", sha256_hex(text.as_bytes())))
        .collect()
}

/// Checks the boot folder `folder` against its [`DEFS_FILE`]: every file
/// it lists hashes to its line, and the node's [`NODE_DEFINITION`] and
/// [`NODE_BOARD`], where present, are listed. `read` is told each file
/// read (a build script reruns when one changes). A folder without
/// [`DEFS_FILE`] is refused: `dagon gen` did not write it, or not all of
/// it. The source tree's own boot folders (the lint and stage0 size
/// folders) carry their hashes like any other.
pub fn check_defs(folder: &Path, read: &mut dyn FnMut(&Path)) -> Result<(), String> {
    let path = folder.join(DEFS_FILE);
    read(&path);
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "{}: no {DEFS_FILE} beside the node's definitions; run `dagon gen` first",
                folder.display()
            ))
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let gen = "the project's overrides in dagon.toml say what to change; run `dagon gen`";
    let mut listed = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let parsed = line.split_once("  ").filter(|(h, name)| {
            h.len() == 64
                && h.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && !name.is_empty()
                && !name.contains(['/', '\\', ':'])
                && *name != ".."
        });
        let Some((want, name)) = parsed else {
            return Err(format!(
                "{}: line {} is not `<sha256>  <file>`; {gen}",
                path.display(),
                i + 1
            ));
        };
        let file = folder.join(name);
        read(&file);
        let bytes = fs::read(&file).map_err(|e| {
            format!(
                "{}: {e}, though {DEFS_FILE} lists it; {gen}",
                file.display()
            )
        })?;
        if sha256_hex(&bytes) != want {
            return Err(format!(
                "{}: changed since `dagon gen` wrote it (it no longer matches its line in {DEFS_FILE}); {gen}",
                file.display()
            ));
        }
        listed.push(name);
    }
    for f in [NODE_DEFINITION, NODE_BOARD, RECOVERY_DEFINITION] {
        if folder.join(f).is_file() && !listed.contains(&f) {
            return Err(format!(
                "{}: no line in {DEFS_FILE} for it; {gen}",
                folder.join(f).display()
            ));
        }
    }
    Ok(())
}

fn read(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Whether the definition `given` is of the same MCU (`id`) and in the same
/// definition format (`format`, 1 when absent) as the package's own `own`.
pub fn same_mcu(own: &str, given: &str) -> Result<(), String> {
    let id_format = |text: &str| -> Result<(String, i64), String> {
        let t: toml::Table = text
            .parse()
            .map_err(|e| format!("not an MCU definition: {e}"))?;
        let id = t.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let format = t.get("format").and_then(|v| v.as_integer()).unwrap_or(1);
        Ok((id.to_string(), format))
    };
    let (own_id, own_format) = id_format(own)?;
    let (id, format) = id_format(given)?;
    if id != own_id {
        return Err(format!(
            "the definition of MCU '{id}'; this package is for MCU '{own_id}'"
        ));
    }
    if format != own_format {
        return Err(format!(
            "definition format {format}; this package reads definition format {own_format}"
        ));
    }
    Ok(())
}

/// One row of an ESP-IDF partition table.
struct Partition {
    name: String,
    ptype: u8,
    subtype: u8,
    offset: u32,
    size: u32,
}

/// An MCU definition as a build reads it.
pub struct Definition {
    t: toml::Table,
    partitions: Vec<Partition>,
}

/// Where a `[[slot]]` keeps its image header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Header {
    Inline,
    Trailer,
    Detached(u32),
}

impl Definition {
    /// `text`; `file` reads a file the definition names (its partition
    /// table), relative to the defining package.
    pub fn parse(text: &str, file: impl Fn(&str) -> String) -> Definition {
        let t: toml::Table = text.parse().expect("dagon-mcu.toml");
        let partitions = match t.get("partitions").and_then(|v| v.as_str()) {
            Some(rel) => partition_table(&file(rel)),
            None => Vec::new(),
        };
        Definition { t, partitions }
    }

    /// The generated module.
    pub fn rs(&self) -> String {
        constants_rs(&self.t) + &self.layout_rs()
    }

    /// The slots of image kind `image` (`[[slot]]`), slot A first: (start,
    /// bytes).
    pub fn slots(&self, image: &str) -> Vec<(u32, u32)> {
        let mut out: Vec<(u8, (u32, u32))> = self
            .slot_rows()
            .into_iter()
            .filter(|(i, _, _)| i == image)
            .map(|(_, index, place)| (index, place))
            .collect();
        out.sort_by_key(|(index, _)| *index);
        out.into_iter().map(|(_, place)| place).collect()
    }

    /// Where each slot of `image` holds its header and body, as the
    /// `header` placement of its `[[slot]]` says, slot A first: (header,
    /// body, body room, placement byte of the image header: 0 inline, 1
    /// detached, 2 trailer).
    pub fn places(&self, image: &str) -> Vec<(u32, u32, u32, u8)> {
        let mut out: Vec<(u8, (u32, u32, u32, u8))> = self
            .slot_rows()
            .into_iter()
            .zip(self.slot_headers())
            .filter(|((i, _, _), _)| i == image)
            .map(|((_, index, (start, size)), header)| {
                let hs = 0x200;
                let place = match header {
                    Header::Inline => (start, start + hs, size - hs, 0),
                    Header::Trailer => (start + size - hs, start, size - hs, 2),
                    Header::Detached(at) => (at, start, size, 1),
                };
                (index, place)
            })
            .collect();
        out.sort_by_key(|(index, _)| *index);
        out.into_iter().map(|(_, place)| place).collect()
    }

    /// Every `[[slot]]`'s header placement, in table order (as
    /// [`Self::slot_rows`]); `header_at`, or `header_partition` plus
    /// `header_offset`, place a detached header.
    fn slot_headers(&self) -> Vec<Header> {
        let rows = self
            .t
            .get("slot")
            .and_then(|v| v.as_array())
            .map_or(&[][..], |v| v.as_slice());
        rows.iter()
            .map(|s| match s.get("header").and_then(|v| v.as_str()) {
                None | Some("inline") => Header::Inline,
                Some("trailer") => Header::Trailer,
                Some("detached") => {
                    let at = match (s.get("header_at"), s.get("header_partition")) {
                        (Some(at), None) => int(at, "slot header_at"),
                        (None, Some(name)) => {
                            let name = name.as_str().expect("[[slot]] header_partition");
                            let p = self
                                .partitions
                                .iter()
                                .find(|p| p.name == name)
                                .unwrap_or_else(|| {
                                    panic!("[[slot]]: no partition '{name}' in the table")
                                });
                            let off = s
                                .get("header_offset")
                                .map_or(0, |v| int(v, "header_offset"));
                            p.offset + off
                        }
                        _ => panic!(
                            "[[slot]]: a detached header needs header_at or header_partition"
                        ),
                    };
                    Header::Detached(at)
                }
                Some(other) => {
                    panic!("[[slot]] header {other:?} is not inline, trailer or detached")
                }
            })
            .collect()
    }

    /// Slot A (or B with `slot_b`) of `image`; a build for a slot the
    /// definition does not give is refused.
    pub fn slot(&self, image: &str, slot_b: bool) -> (u32, u32) {
        let name = if slot_b { "b" } else { "a" };
        *self
            .slots(image)
            .get(usize::from(slot_b))
            .unwrap_or_else(|| panic!("the MCU definition has no {image} slot {name}"))
    }

    /// `[layout] stage0`: (start, bytes).
    pub fn stage0(&self) -> Option<(u32, u32)> {
        region(self.t.get("layout")?.get("stage0")?, "[layout] stage0")
    }

    /// `ram`: (start, bytes).
    pub fn ram(&self) -> Option<(u32, u32)> {
        region(self.t.get("ram")?, "ram")
    }

    /// `[boot.record]`: (start, bytes).
    pub fn boot_record(&self) -> Option<(u32, u32)> {
        region(self.t.get("boot")?.get("record")?, "[boot.record]")
    }

    /// A `[layout]` log: its two faces and the face bytes.
    fn log(&self, key: &str) -> Option<([u32; 2], u32)> {
        let l = self.t.get("layout")?.get(key)?;
        let what = format!("[layout] {key}");
        let faces: Vec<u32> = l
            .get("faces")
            .and_then(|f| f.as_array())
            .unwrap_or_else(|| panic!("{what}: faces is missing"))
            .iter()
            .map(|f| int(f, &what))
            .collect();
        let faces: [u32; 2] = faces
            .try_into()
            .unwrap_or_else(|_| panic!("{what}: two faces"));
        let size = l
            .get("face_size")
            .unwrap_or_else(|| panic!("{what}: face_size is missing"));
        Some((faces, int(size, &what)))
    }

    /// Every `[[slot]]`: image kind, slot index (a and any 0, b 1), (start,
    /// bytes).
    fn slot_rows(&self) -> Vec<(String, u8, (u32, u32))> {
        let rows = self
            .t
            .get("slot")
            .and_then(|v| v.as_array())
            .map_or(&[][..], |v| v.as_slice());
        rows.iter()
            .map(|s| {
                let word = |k: &str| s.get(k).and_then(|v| v.as_str());
                let image = word("image").expect("[[slot]] image is missing");
                let index = match word("binding") {
                    Some("a" | "any") => 0,
                    Some("b") => 1,
                    other => panic!("[[slot]] binding {other:?} is not a, b or any"),
                };
                let place = match (s.get("at"), s.get("size"), word("partition")) {
                    (Some(at), Some(size), None) => (int(at, "slot at"), int(size, "slot size")),
                    (None, None, Some(name)) => {
                        let p = self
                            .partitions
                            .iter()
                            .find(|p| p.name == name)
                            .unwrap_or_else(|| {
                                panic!("[[slot]]: no partition '{name}' in the table")
                            });
                        (p.offset, p.size)
                    }
                    _ => panic!("[[slot]] {image}: give either at + size or partition"),
                };
                (image.to_string(), index, place)
            })
            .collect()
    }

    fn layout_rs(&self) -> String {
        let pair = |(a, b): (u32, u32)| format!("({a:#x}, {b:#x})");
        let mut out = String::new();
        if let Some(id) = self.t.get("wire_id") {
            let id = u16::try_from(int(id, "wire_id")).expect("wire_id fits a u16");
            out += "/// The MCU's id in image headers (`wire_id`).\n";
            out += &format!("pub const MCU_WIRE_ID: u16 = {id:#x};\n");
        }
        let first_map = self
            .t
            .get("flash_map")
            .and_then(|v| v.as_array())
            .and_then(|v| v.first());
        if let Some(m) = first_map {
            let num = |k: &str| int(m.get(k).unwrap_or(&toml::Value::Integer(-1)), k);
            out += "/// The default flash map (the first `[[flash_map]]`): base, bytes.\n";
            out += &format!(
                "pub const DEFAULT_FLASH: (u32, u32) = {};\n",
                pair((num("base"), num("bytes")))
            );
        }
        for (name, what, v) in [
            ("RAM_REGION", "RAM (`ram`)", self.ram()),
            (
                "BOOT_RECORD_REGION",
                "The boot record (`[boot.record]`)",
                self.boot_record(),
            ),
            (
                "STAGE0_REGION",
                "Stage0's region (`[layout] stage0`)",
                self.stage0(),
            ),
        ] {
            if let Some(v) = v {
                out += &format!("/// {what}: start, bytes.\n");
                out += &format!("pub const {name}: (u32, u32) = {};\n", pair(v));
            }
        }
        for (name, key) in [
            ("APP_LOG_REGION", "app_log"),
            ("BOOTCORE_LOG_REGION", "bootcore_log"),
        ] {
            if let Some((faces, size)) = self.log(key) {
                out += &format!("/// The `[layout] {key}`: its two faces, face bytes.\n");
                out += &format!(
                    "pub const {name}: ([u32; 2], u32) = ([{:#x}, {:#x}], {size:#x});\n",
                    faces[0], faces[1]
                );
            }
        }
        out += "/// Each image kind's slots (`[[slot]]`), slot A first: start, bytes.\n";
        out += "pub mod slots {\n";
        let kinds: std::collections::BTreeSet<String> =
            self.slot_rows().into_iter().map(|(i, _, _)| i).collect();
        for kind in kinds {
            assert!(
                kind.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                "[[slot]] image {kind:?} is not a name"
            );
            let slots: Vec<String> = self.slots(&kind).into_iter().map(pair).collect();
            out += &format!("    /// `image = \"{kind}\"`.\n");
            out += &format!(
                "    pub const {}: [(u32, u32); {}] = [{}];\n",
                kind.to_ascii_uppercase().replace('-', "_"),
                slots.len(),
                slots.join(", ")
            );
            let places: Vec<String> = self
                .places(&kind)
                .into_iter()
                .map(|(h, b, r, p)| format!("({h:#x}, {b:#x}, {r:#x}, {p})"))
                .collect();
            out += "    /// Its header and body: header, body, body room, placement.\n";
            out += &format!(
                "    pub const {}_PLACES: [(u32, u32, u32, u8); {}] = [{}];\n",
                kind.to_ascii_uppercase().replace('-', "_"),
                places.len(),
                places.join(", ")
            );
        }
        out += "}\n";
        if !self.partitions.is_empty() {
            out += "/// The partition table (`partitions`), in table order: name, type,\n";
            out += "/// subtype, offset, bytes.\n";
            out += &format!(
                "pub const PARTITION_TABLE: [(&str, u8, u8, u32, u32); {}] = [\n",
                self.partitions.len()
            );
            for p in &self.partitions {
                out += &format!(
                    "    ({:?}, {:#x}, {:#x}, {:#x}, {:#x}),\n",
                    p.name, p.ptype, p.subtype, p.offset, p.size
                );
            }
            out += "];\n";
        }
        out
    }
}

/// `{ at, size }`.
fn region(v: &toml::Value, what: &str) -> Option<(u32, u32)> {
    let num = |k: &str| {
        int(
            v.get(k).unwrap_or_else(|| panic!("{what}: {k} is missing")),
            what,
        )
    };
    Some((num("at"), num("size")))
}

/// A number of an ESP-IDF partition table: decimal or `0x` hex, with an
/// optional `K` or `M` multiplier.
fn csv_number(s: &str) -> Option<u32> {
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

/// An ESP-IDF partition table (`# Name, Type, SubType, Offset, Size`,
/// offsets explicit). Type and subtype names are ESP-IDF's (Partition
/// Tables, "Type field" and "SubType"); anything else is a number.
fn partition_table(csv: &str) -> Vec<Partition> {
    csv.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            let bad = || -> ! { panic!("partition row '{l}': name, type, subtype, offset, size") };
            let byte = |s: &str| csv_number(s).and_then(|n| u8::try_from(n).ok());
            let ptype = match f.get(1).copied() {
                Some("app") => 0x00,
                Some("data") => 0x01,
                Some(s) => byte(s).unwrap_or_else(|| bad()),
                None => bad(),
            };
            let subtype = match (ptype, f.get(2).copied().unwrap_or_else(|| bad())) {
                (0x00, "factory") => 0x00,
                (0x00, "test") => 0x20,
                (0x00, s) if s.starts_with("ota_") => s[4..]
                    .parse::<u8>()
                    .ok()
                    .filter(|n| *n < 16)
                    .map_or_else(|| bad(), |n| 0x10 + n),
                (0x01, "ota") => 0x00,
                (0x01, "phy") => 0x01,
                (0x01, "nvs") => 0x02,
                (0x01, "coredump") => 0x03,
                (0x01, "nvs_keys") => 0x04,
                (0x01, "efuse") => 0x05,
                (0x01, "undefined") => 0x06,
                (0x01, "esphttpd") => 0x80,
                (0x01, "fat") => 0x81,
                (0x01, "spiffs") => 0x82,
                (0x01, "littlefs") => 0x83,
                (_, s) => byte(s).unwrap_or_else(|| bad()),
            };
            let num = |i: usize| {
                f.get(i)
                    .and_then(|s| csv_number(s))
                    .unwrap_or_else(|| bad())
            };
            Partition {
                name: f[0].to_string(),
                ptype,
                subtype,
                offset: num(3),
                size: num(4),
            }
        })
        .collect()
}

fn required<'a>(t: &'a toml::Table, table: &str, key: &str) -> &'a toml::Value {
    t.get(table)
        .and_then(|v| v.as_table())
        .and_then(|v| v.get(key))
        .unwrap_or_else(|| panic!("dagon-mcu.toml: [{table}] {key} is missing"))
}

fn int(v: &toml::Value, what: &str) -> u32 {
    v.as_integer()
        .and_then(|i| u32::try_from(i).ok())
        .unwrap_or_else(|| panic!("dagon-mcu.toml: {what} is not a u32"))
}

/// A hardware fact (`N`, `{ unverified = N }` or `"unmeasured"`): its value
/// and whether it is checked against its source.
fn fact(v: &toml::Value, what: &str) -> (Option<u32>, bool) {
    if v.as_str() == Some("unmeasured") {
        return (None, false);
    }
    match v.as_table().and_then(|t| t.get("unverified")) {
        Some(u) => (Some(int(u, what)), false),
        None => (Some(int(v, what)), true),
    }
}

/// The generated source for a definition's text (one that names no
/// package file).
pub fn definition_rs(text: &str) -> String {
    Definition::parse(text, |f| panic!("{f}: no package files here")).rs()
}

/// The watchdog, trial, execution, erase and conformance constants.
fn constants_rs(t: &toml::Table) -> String {
    let mut out = String::new();
    let attempts = int(required(t, "watchdog", "trial_attempts"), "trial_attempts");
    let attempts = u8::try_from(attempts).expect("[watchdog] trial_attempts fits a u8");
    out += "/// Boots the boot stage gives a pending image before it rolls back\n";
    out += "/// (`[watchdog] trial_attempts`).\n";
    out += &format!("pub const TRIAL_ATTEMPTS: u8 = {attempts};\n");
    // The periods the MCU's watchdog can run: a period outside them would
    // build firmware whose watchdog cannot be armed.
    let range = required(t, "watchdog", "range_ms");
    let bound = |k: &str| {
        int(
            range
                .get(k)
                .unwrap_or_else(|| panic!("dagon-mcu.toml: [watchdog] range_ms.{k} is missing")),
            "range_ms",
        )
    };
    let (min, max) = (bound("min"), bound("max"));
    out += "/// The periods the MCU's watchdog can run, ms, both included\n";
    out += "/// (`[watchdog] range_ms`).\n";
    out += &format!("pub const WATCHDOG_RANGE_MS: (u32, u32) = ({min}, {max});\n");
    let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    for (key, name, what) in [
        (
            "trial_ms",
            "TRIAL_WDT_MS",
            "a trial boot (stage0 arms it before a core on trial)",
        ),
        ("confirmed_ms", "CONFIRMED_WDT_MS", "a confirmed boot"),
    ] {
        let ms = int(required(t, "watchdog", key), key);
        assert!(
            (min..=max).contains(&ms),
            "MCU '{id}': [watchdog] {key} {ms} is outside the periods its watchdog can run, \
             {min}..={max} ms ([watchdog] range_ms)"
        );
        out += &format!("/// Watchdog period of {what}, ms (`[watchdog] {key}`).\n");
        out += &format!("pub const {name}: u32 = {ms};\n");
    }

    out += &stage0_key_place_rs(t);
    out += &stage0_internal_hz_rs(t);
    out += &flash_write_rs(t);

    let (masked, masked_known) = fact(required(t, "execution", "masked_max_us"), "masked_max_us");
    out += "/// Longest DAGON's own code masks interrupts, us (`[execution]\n";
    out += "/// masked_max_us`); `None` while it is not measured.\n";
    out += &format!("pub const MASKED_MAX_US: Option<u32> = {masked:?};\n");
    out += "/// `MASKED_MAX_US` is checked against its source.\n";
    out += &format!("pub const MASKED_MAX_US_KNOWN: bool = {masked_known};\n");

    out += &erase_layout_rs(t);
    out += &relay_links_rs(t);

    let scratch = t
        .get("conformance")
        .and_then(|c| c.get("scratch"))
        .and_then(|s| Some((s.get("at")?.as_integer()?, s.get("size")?.as_integer()?)));
    out += "/// Flash a conformance build may destroy (`[conformance] scratch`).\n";
    out += &match scratch {
        Some((at, size)) => {
            format!("pub const CONFORM_SCRATCH: Option<(u32, u32)> = Some(({at:#x}, {size:#x}));\n")
        }
        None => "pub const CONFORM_SCRATCH: Option<(u32, u32)> = None;\n".into(),
    };
    out
}

fn array<'a>(t: &'a toml::Table, key: &str) -> &'a [toml::Value] {
    t.get(key)
        .and_then(|v| v.as_array())
        .map_or(&[][..], |v| v.as_slice())
}

fn text<'a>(v: &'a toml::Value, key: &str, what: &str) -> &'a str {
    v.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("dagon-mcu.toml: {what} {key} is missing"))
}

/// `[[relay_link]]` as `RELAY_LINKS`, the same values the definitions
/// loader (dagon-topology) reads for the PC, each with its `[[link_port]]`'s
/// index and kind; each link's buffer next to it (`RELAY_LINK_BUFFERS`, one
/// frame of its `max_payload`); and each link's id by its port (`RELAY_LINK_<PORT>`), for
/// the code that drives that port's link. The loader checks the entries;
/// this only writes them out.
fn relay_links_rs(t: &toml::Table) -> String {
    let ports = array(t, "link_port");
    let links = array(t, "relay_link");
    let mut out = String::new();
    out += "/// The links a node of this MCU relays over (`[[relay_link]]`).\n";
    out += &format!(
        "pub const RELAY_LINKS: [dagon_port::RelayLinkDef; {}] = [\n",
        links.len()
    );
    let mut ids = String::new();
    let mut buffers = Vec::new();
    for l in links {
        let name = text(l, "port", "[[relay_link]]");
        let port = ports
            .iter()
            .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
            .unwrap_or_else(|| {
                panic!("dagon-mcu.toml: relay_link port '{name}' is no [[link_port]]")
            });
        let index = int(port.get("index").expect("[[link_port]] index"), "index");
        // The kind's name in the 構成 (`usb-cdc`) as its variant (`UsbCdc`).
        let kind: String = text(port, "kind", "[[link_port]]")
            .split('-')
            .map(|w| w[..1].to_uppercase() + &w[1..])
            .collect();
        let addressing = match l.get("addressing") {
            Some(v) if v.as_str() == Some("seven-bit") => "SevenBit".to_string(),
            Some(v) => match v.get("fixed") {
                Some(a) => format!("Fixed({})", int(a, "addressing.fixed")),
                None => {
                    panic!("dagon-mcu.toml: relay_link addressing is seven-bit or {{ fixed = N }}")
                }
            },
            None => panic!("dagon-mcu.toml: relay_link addressing is missing"),
        };
        let id = int(l.get("id").expect("[[relay_link]] id"), "id");
        let max = int(
            l.get("max_payload").expect("[[relay_link]] max_payload"),
            "max_payload",
        );
        out += &format!(
            "    dagon_port::RelayLinkDef {{ id: {id}, port: {index}, kind: dagon_port::LinkKind::{kind}, \
             addressing: dagon_port::Addressing::{addressing}, max_payload: {max} }},\n"
        );
        buffers.push(format!("RELAY_LINKS[{}].buffer_len()", buffers.len()));
        let constant = name.to_uppercase().replace('-', "_");
        ids += &format!("/// The id of the relay link on port `{name}`.\n");
        ids += &format!("pub const RELAY_LINK_{constant}: u8 = {id};\n");
    }
    out += "];\n";
    out += "/// The bytes each relay link's port and worker hold, in `RELAY_LINKS`\n";
    out += "/// order: `RelayLinkDef::buffer_len`, one frame of the link's `max_payload`.\n";
    out += &format!(
        "pub const RELAY_LINK_BUFFERS: [usize; {}] = [{}];\n",
        buffers.len(),
        buffers.join(", ")
    );
    out + &ids
}

/// Where stage0 reads its verifying key (`[stage0] key`, `"flash"` when
/// left out), for a definition with `[stage0]`: `"flash"`, the `PUBKEY`
/// constant [`embed_pubkey`] writes into stage0's own flash, is the one
/// place a DAGON stage0 reads; any other is refused.
fn stage0_key_place_rs(t: &toml::Table) -> String {
    let Some(stage0) = t.get("stage0") else {
        return String::new();
    };
    let place = stage0.get("key").map_or(Some("flash"), |k| k.as_str());
    assert!(
        place == Some("flash"),
        "dagon-mcu.toml: [stage0] key {:?}: this MCU's stage0 cannot read a key from that place \
         (it reads one from: flash)",
        place.unwrap_or("(not a string)")
    );
    "/// Where stage0 reads its verifying key (`[stage0] key`): `flash`, the\n\
     /// `PUBKEY` constant built into stage0's own flash.\n\
     pub const STAGE0_KEY_PLACE: &str = \"flash\";\n"
        .into()
}

/// `STAGE0_INTERNAL_HZ`, the `out_hz` of the internal `[[stage0.clock]]`
/// setup (the MCU's own oscillator as it runs from reset), for a
/// definition with `[stage0]`.
fn stage0_internal_hz_rs(t: &toml::Table) -> String {
    let Some(stage0) = t.get("stage0") else {
        return String::new();
    };
    let hz = array_in(stage0, "clock")
        .iter()
        .find(|c| c.get("source").and_then(|s| s.as_str()) == Some("internal"))
        .and_then(|c| c.get("out_hz"))
        .unwrap_or_else(|| panic!("dagon-mcu.toml: [stage0] has no internal [[stage0.clock]]"));
    format!(
        "/// The MCU's own oscillator as it runs from reset, Hz (the internal\n\
         /// `[[stage0.clock]]` setup's `out_hz`).\n\
         pub const STAGE0_INTERNAL_HZ: u32 = {};\n",
        int(hz, "[[stage0.clock]] out_hz")
    )
}

/// The flash's write unit and erased value (`[execution] write_unit`,
/// `erased_value`) and, for a definition with `[stage0]`, the head the boot
/// ROM reads before stage0 (`[stage0] head_bytes`). Hardware facts with no
/// default: a definition without one is refused, naming the key.
fn flash_write_rs(t: &toml::Table) -> String {
    let unit = int(required(t, "execution", "write_unit"), "write_unit");
    let erased = int(required(t, "execution", "erased_value"), "erased_value");
    let erased = u8::try_from(erased).expect("dagon-mcu.toml: [execution] erased_value fits a u8");
    let mut out = format!(
        "/// Bytes one flash write covers (`[execution] write_unit`).\n\
         pub const WRITE_UNIT: u32 = {unit};\n\
         /// The byte erased flash reads as (`[execution] erased_value`).\n\
         pub const ERASED_VALUE: u8 = {erased:#x};\n"
    );
    if t.contains_key("stage0") {
        out += &format!(
            "/// Bytes at the start of stage0's region the boot ROM reads before\n\
             /// stage0 (`[stage0] head_bytes`); 0: none.\n\
             pub const STAGE0_HEAD_LEN: u32 = {:#x};\n",
            stage0_head_of(t)
        );
    }
    out
}

fn stage0_head_of(t: &toml::Table) -> u32 {
    int(required(t, "stage0", "head_bytes"), "head_bytes")
}

fn array_in<'a>(v: &'a toml::Value, key: &str) -> &'a [toml::Value] {
    v.get(key)
        .and_then(|v| v.as_array())
        .map_or(&[][..], |v| v.as_slice())
}

/// The source of `$OUT_DIR/clock.rs`: the clock setup stage0 runs on, as
/// the definitions loader chose it for the node (`dagon_topology`'s
/// `stage0_build`): `SOURCE` (`"internal"`, `"crystal"`, `"external"`),
/// `IN_HZ`, `OUT_HZ` and `PLL`, a `Pll` with the setup's `pll` fields (the
/// MCU's own; the stage0 that includes the file defines `Pll`).
pub fn stage0_clock_rs(
    source: &str,
    in_hz: Option<u32>,
    out_hz: u32,
    pll: &[(&str, u32)],
) -> String {
    let pll = if pll.is_empty() {
        "None".to_string()
    } else {
        let fields: Vec<String> = pll.iter().map(|(k, v)| format!("{k}: {v}")).collect();
        format!("Some(Pll {{ {} }})", fields.join(", "))
    };
    format!(
        "// Written by the stage0's build script from the node's definitions.\n\n\
         /// What drives the clock stage0 runs on.\n\
         pub const SOURCE: &str = {source:?};\n\
         /// The frequency of that input, Hz (none for the internal oscillator).\n\
         pub const IN_HZ: Option<u32> = {in_hz:?};\n\
         /// The core clock stage0 runs at, Hz.\n\
         pub const OUT_HZ: u32 = {out_hz};\n\
         /// The PLL settings, when the setup has a PLL.\n\
         pub const PLL: Option<Pll> = {pll};\n"
    )
}

/// Writes `$OUT_DIR/clock.rs` ([`stage0_clock_rs`]).
pub fn stage0_clock(source: &str, in_hz: Option<u32>, out_hz: u32, pll: &[(&str, u32)]) {
    fs::write(
        out_dir().join("clock.rs"),
        stage0_clock_rs(source, in_hz, out_hz, pll),
    )
    .unwrap();
}

/// Lines of stage0's linker script that fail the link when the image (its
/// code, read-only data and initial data image, from the start of the
/// `STAGE0` region) leaves less than 1/16 of the region free. Stage0 is
/// never updated in the field, yet it is rebuilt for every new node with
/// that build's toolchain; the free part keeps such a build linking.
pub const STAGE0_ASSERT: &str = "__stage0_image_end = LOADADDR(.data) + SIZEOF(.data);\n\
ASSERT(__stage0_image_end - ORIGIN(STAGE0) <= LENGTH(STAGE0) - LENGTH(STAGE0) / 16, \
\"stage0 does not leave 1/16 of its region free\");\n";

/// `[layout] stage0` start and `[stage0] region_bytes` of the MCU
/// definition the node build is given ([`definition`]): stage0's whole
/// region.
pub fn stage0_region() -> (u32, u32) {
    stage0_region_of(&node_definition_text())
}

fn stage0_region_of(text: &str) -> (u32, u32) {
    let t: toml::Table = text.parse().expect("dagon-mcu.toml");
    let at = t
        .get("layout")
        .and_then(|l| l.get("stage0"))
        .and_then(|s| s.get("at"))
        .unwrap_or_else(|| panic!("dagon-mcu.toml: [layout] stage0 is missing"));
    let at = int(at, "[layout] stage0 at");
    (
        at,
        int(required(&t, "stage0", "region_bytes"), "region_bytes"),
    )
}

/// Writes `$OUT_DIR/memory.x` for stage0: `STAGE0` is its whole region
/// ([`stage0_region`]); its first `[stage0] head_bytes` are the `HEAD`
/// region (a block the boot ROM reads before stage0, such as the RP2040's
/// boot2; none when 0) placed by `head_sections`, `FLASH` the rest; RAM at
/// `ram`/`ram_len`; with [`STAGE0_ASSERT`].
pub fn stage0_memory_x(head_sections: &str, ram: u32, ram_len: u32) {
    let text = node_definition_text();
    let t: toml::Table = text.parse().expect("dagon-mcu.toml");
    write_memory_x(&stage0_memory_text(
        stage0_region_of(&text),
        stage0_head_of(&t),
        head_sections,
        (ram, ram_len),
    ));
}

fn stage0_memory_text(
    (at, len): (u32, u32),
    head: u32,
    head_sections: &str,
    (ram, ram_len): (u32, u32),
) -> String {
    let head_region = if head > 0 {
        format!("  HEAD   : ORIGIN = {at:#010x}, LENGTH = {head:#x}\n")
    } else {
        String::new()
    };
    format!(
        "MEMORY\n{{\n  STAGE0 : ORIGIN = {at:#010x}, LENGTH = {len:#x}\n{head_region}  \
         FLASH  : ORIGIN = {:#010x}, LENGTH = {:#x}\n  \
         RAM    : ORIGIN = {ram:#010x}, LENGTH = {ram_len:#x}\n}}\n\n{head_sections}{STAGE0_ASSERT}",
        at + head,
        len - head
    )
}

/// `[execution] erase_regions` (the MCU's default regions) and
/// `[[slot_layout]]` as constants: a slot store composes its erase layout
/// from the slot layouts and the regions its board returns. Also the
/// longest single erase of the defaults, with whether every erase time is
/// checked.
fn erase_layout_rs(t: &toml::Table) -> String {
    let regions = required(t, "execution", "erase_regions")
        .as_array()
        .expect("[execution] erase_regions is a list");
    let field = |r: &toml::Value, key: &str| -> toml::Value {
        r.get(key)
            .cloned()
            .unwrap_or_else(|| panic!("[execution] erase_regions: {key} is missing"))
    };
    let mut out = String::new();
    let (mut longest, mut known) = (0, true);
    out += "/// The MCU's erase regions (`[execution] erase_regions`): what a board\n";
    out += "/// without `[flash]` erases in (its `BoardDef` returns them).\n";
    out += &format!(
        "pub const ERASE_REGIONS: [dagon_port::timeout::EraseRegion; {}] = [\n",
        regions.len()
    );
    for r in regions {
        let (ms, ms_known) = fact(&field(r, "unit_max_ms"), "unit_max_ms");
        let ms = ms.expect("[execution] erase_regions: unit_max_ms needs a value");
        longest = longest.max(ms);
        known &= ms_known;
        out += &format!(
            "    dagon_port::timeout::EraseRegion {{ start: {:#x}, len: {:#x}, unit: {:#x}, \
             unit_max_ms: {ms} }},\n",
            int(&field(r, "start"), "start"),
            int(&field(r, "len"), "len"),
            int(&field(r, "unit"), "unit"),
        );
    }
    out += "];\n";
    let layouts = t
        .get("slot_layout")
        .and_then(|v| v.as_array())
        .map_or(&[][..], |v| v.as_slice());
    out += "/// Every target and slot the node receives into (`[[slot_layout]]`).\n";
    out += &format!(
        "pub const SLOT_LAYOUTS: [dagon_port::timeout::SlotLayout; {}] = [\n",
        layouts.len()
    );
    for l in layouts {
        out += &slot_layout_rs(l);
    }
    out += "];\n";
    out += "/// The longest single erase, ms (the largest `unit_max_ms`).\n";
    out += &format!("pub const ERASE_UNIT_MAX_MS: u32 = {longest};\n");
    out += "/// Every `unit_max_ms` is checked against the flash's datasheet.\n";
    out += &format!("pub const ERASE_UNIT_MAX_MS_KNOWN: bool = {known};\n");
    out
}

/// One `[[slot_layout]]` as a `SlotLayout` element.
fn slot_layout_rs(l: &toml::Value) -> String {
    let word = |key: &str| {
        l.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("[[slot_layout]] {key} is missing"))
    };
    let num = |key: &str| {
        let v = l
            .get(key)
            .unwrap_or_else(|| panic!("[[slot_layout]] {key} is missing"));
        int(v, key)
    };
    let target = match word("target") {
        "app" => "APP",
        "bootcore" => "BOOTCORE",
        "agent" => "AGENT",
        other => panic!("[[slot_layout]] target {other:?} is not a target"),
    };
    let slot = match word("slot") {
        "a" => 0,
        "b" => 1,
        "factory" => 2,
        other => panic!("[[slot_layout]] slot {other:?} is not a slot"),
    };
    let header = match l.get("header") {
        Some(h) if h.as_str() == Some("inline") => "Inline".to_string(),
        Some(h) => {
            let at = h
                .get("detached")
                .unwrap_or_else(|| panic!("[[slot_layout]] header is inline or {{ detached }}"));
            format!("Detached {{ at: {:#x} }}", int(at, "detached"))
        }
        None => panic!("[[slot_layout]] header is missing"),
    };
    format!(
        "    dagon_port::timeout::SlotLayout {{ target: dagon_proto::payload::target::{target}, \
         slot: {slot}, start: {:#x}, capacity: {:#x}, \
         header: dagon_port::timeout::HeaderPlace::{header} }},\n",
        num("start"),
        num("capacity"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A node's boot folder `dagon gen` wrote holds `defs.sha256`
    /// (`sha256sum` lines, [`defs_text`]): a definition edited after gen, a
    /// listed file that is gone, a definition it does not list and a line
    /// that is not one are refused, naming the file, and so is a folder
    /// without it. Every file it reads is reported.
    #[test]
    fn the_node_definitions_must_match_their_hashes() {
        let d = std::env::temp_dir().join(format!("dagon-mcu-build-defs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let check = |d: &Path| check_defs(d, &mut |_| {});
        let e = check(&d).unwrap_err();
        assert!(
            e.contains("no defs.sha256") && e.contains("dagon gen"),
            "{e}"
        );
        let (mcu, board) = ("id = \"m\"\n", "id = \"b\"\n");
        let text = defs_text(&[(NODE_DEFINITION, mcu), (NODE_BOARD, board)]);
        // `printf 'id = "m"\n' | sha256sum`.
        let line = "f9c09336d64fe06078bfb5f10af086c7f5f2604c7416c3e6d91a4f67e778f923  mcu.toml\n";
        assert!(text.starts_with(line), "{text}");
        assert!(text.ends_with("  board.toml\n") && text.lines().count() == 2);
        fs::write(d.join(NODE_DEFINITION), mcu).unwrap();
        fs::write(d.join(NODE_BOARD), board).unwrap();
        fs::write(d.join(DEFS_FILE), &text).unwrap();
        let mut read = Vec::new();
        check_defs(&d, &mut |f| read.push(f.to_path_buf())).unwrap();
        for f in [DEFS_FILE, NODE_DEFINITION, NODE_BOARD] {
            assert!(read.contains(&d.join(f)), "{f}: {read:?}");
        }
        fs::write(d.join(NODE_BOARD), "id = \"c\"\n").unwrap();
        let e = check(&d).unwrap_err();
        assert!(
            e.contains("board.toml") && e.contains(DEFS_FILE) && e.contains("dagon gen"),
            "{e}"
        );
        fs::remove_file(d.join(NODE_BOARD)).unwrap();
        let e = check(&d).unwrap_err();
        assert!(e.contains("board.toml"), "{e}");
        // A definition the hashes do not name.
        fs::write(d.join(DEFS_FILE), line).unwrap();
        check(&d).unwrap();
        fs::write(d.join(NODE_BOARD), board).unwrap();
        let e = check(&d).unwrap_err();
        assert!(e.contains("board.toml") && e.contains("no line"), "{e}");
        fs::write(d.join(DEFS_FILE), "").unwrap();
        let e = check(&d).unwrap_err();
        assert!(e.contains("no line"), "{e}");
        fs::write(d.join(DEFS_FILE), "not a line\n").unwrap();
        assert!(check(&d).is_err());
        fs::remove_dir_all(&d).unwrap();
    }

    const DEF: &str = "[watchdog]\ntrial_attempts = 3\ntrial_ms = 4000\nconfirmed_ms = 8000\n\
                       range_ms = { min = 1, max = 9000 }\n\
                       [execution]\nmasked_max_us = 50\nwrite_unit = 4\nerased_value = 0xFF\n\
                       erase_regions = [{ start = 0x0, len = 0x4000, unit = 0x1000, unit_max_ms = 400 },\n\
                       { start = 0x4000, len = 0x8000, unit = 0x4000, unit_max_ms = 900 }]\n\
                       [conformance]\nscratch = { at = 0x100, size = 0x10 }\n\
                       [[slot_layout]]\ntarget = \"app\"\nslot = \"b\"\nstart = 0x4000\n\
                       capacity = 0x3E00\nheader = \"inline\"\n\
                       [[slot_layout]]\ntarget = \"agent\"\nslot = \"factory\"\nstart = 0x8000\n\
                       capacity = 0x4000\nheader = { detached = 0x1000 }\n";

    #[test]
    fn constants_come_from_the_definition() {
        let rs = definition_rs(DEF);
        assert!(rs.contains("pub const TRIAL_ATTEMPTS: u8 = 3;\n"), "{rs}");
        assert!(rs.contains("pub const TRIAL_WDT_MS: u32 = 4000;\n"), "{rs}");
        assert!(
            rs.contains("pub const CONFIRMED_WDT_MS: u32 = 8000;\n"),
            "{rs}"
        );
        assert!(!rs.contains("STAGE0_KEY_PLACE"), "no [stage0]: {rs}");
        assert!(
            rs.contains(
                "EraseRegion { start: 0x4000, len: 0x8000, unit: 0x4000, unit_max_ms: 900 }"
            ),
            "{rs}"
        );
        assert!(
            rs.contains(
                "SlotLayout { target: dagon_proto::payload::target::APP, slot: 1, \
                 start: 0x4000, capacity: 0x3e00, header: dagon_port::timeout::HeaderPlace::Inline }"
            ),
            "{rs}"
        );
        assert!(
            rs.contains(
                "SlotLayout { target: dagon_proto::payload::target::AGENT, slot: 2, \
                 start: 0x8000, capacity: 0x4000, \
                 header: dagon_port::timeout::HeaderPlace::Detached { at: 0x1000 } }"
            ),
            "{rs}"
        );
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS: u32 = 900;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS_KNOWN: bool = true;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const MASKED_MAX_US: Option<u32> = Some(50);\n"),
            "{rs}"
        );
        assert!(rs.contains("Some((0x100, 0x10))"), "{rs}");
    }

    /// `[[relay_link]]` becomes `RELAY_LINKS`, each entry with its port's
    /// index and kind, and one id constant per port.
    #[test]
    fn relay_links_come_from_the_definition() {
        let def = format!(
            "{DEF}\n[[link_port]]\nindex = 3\nname = \"i2c0\"\nkind = \"i2c\"\n\
             [[link_port]]\nindex = 5\nname = \"pio-uart\"\nkind = \"uart\"\n\
             [[link_port]]\nindex = 6\nname = \"usb\"\nkind = \"usb-cdc\"\n\
             [[relay_link]]\nid = 9\nport = \"pio-uart\"\naddressing = {{ fixed = 0xFF }}\nmax_payload = 1024\n\
             [[relay_link]]\nid = 4\nport = \"i2c0\"\naddressing = \"seven-bit\"\nmax_payload = 256\n"
        );
        let rs = definition_rs(&def);
        assert!(
            rs.contains(
                "pub const RELAY_LINKS: [dagon_port::RelayLinkDef; 2] = [\n    \
                 dagon_port::RelayLinkDef { id: 9, port: 5, kind: dagon_port::LinkKind::Uart, \
                 addressing: dagon_port::Addressing::Fixed(255), max_payload: 1024 },\n    \
                 dagon_port::RelayLinkDef { id: 4, port: 3, kind: dagon_port::LinkKind::I2c, \
                 addressing: dagon_port::Addressing::SevenBit, max_payload: 256 },\n];\n"
            ),
            "{rs}"
        );
        assert!(
            rs.contains("pub const RELAY_LINK_PIO_UART: u8 = 9;\n"),
            "{rs}"
        );
        assert!(rs.contains("pub const RELAY_LINK_I2C0: u8 = 4;\n"), "{rs}");
        // Each link's buffer, next to the table and in its order, from the
        // one limit function (`RelayLinkDef::buffer_len`).
        assert!(
            rs.contains(
                "pub const RELAY_LINK_BUFFERS: [usize; 2] = [\
                 RELAY_LINKS[0].buffer_len(), RELAY_LINKS[1].buffer_len()];\n"
            ),
            "{rs}"
        );
        // An MCU that relays over nothing has an empty table.
        let rs = definition_rs(DEF);
        assert!(
            rs.contains("pub const RELAY_LINKS: [dagon_port::RelayLinkDef; 0] = [\n];\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const RELAY_LINK_BUFFERS: [usize; 0] = [];\n"),
            "{rs}"
        );
    }

    /// The address of the ROM loader on this node's recovery lines comes
    /// from the MCU definition of the node there (`[rom_i2c] addr`), which
    /// the boot folder holds beside the node's own; without one, or for an
    /// MCU with no ROM loader, there is none. An unlisted one is refused.
    #[test]
    fn the_recovery_rom_address_comes_from_the_linked_definition() {
        let some = "pub const RECOVERY_ROM_ADDR: Option<u8> = Some(0x3b);\n";
        let none = "pub const RECOVERY_ROM_ADDR: Option<u8> = None;\n";
        let linked = "id = \"m\"\n[rom_i2c]\naddr = 0x3B\n";
        let rs = recovery_rom_rs(Some(linked));
        assert!(rs.contains(some), "{rs}");
        assert!(recovery_rom_rs(Some("id = \"m\"\n")).contains(none));
        assert!(recovery_rom_rs(None).contains(none));

        let d = std::env::temp_dir().join(format!("dagon-mcu-build-rec-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        assert_eq!(recovery_text(&d), None);
        let mcu = "id = \"n\"\n";
        fs::write(d.join(NODE_DEFINITION), mcu).unwrap();
        fs::write(d.join(RECOVERY_DEFINITION), linked).unwrap();
        fs::write(d.join(DEFS_FILE), defs_text(&[(NODE_DEFINITION, mcu)])).unwrap();
        let e = check_defs(&d, &mut |_| {}).unwrap_err();
        assert!(
            e.contains(RECOVERY_DEFINITION) && e.contains("no line"),
            "{e}"
        );
        let listed = defs_text(&[(NODE_DEFINITION, mcu), (RECOVERY_DEFINITION, linked)]);
        fs::write(d.join(DEFS_FILE), listed).unwrap();
        check_defs(&d, &mut |_| {}).unwrap();
        assert_eq!(recovery_text(&d).as_deref(), Some(linked));
        fs::remove_dir_all(&d).unwrap();
    }

    /// The key place stage0 reads comes from `[stage0] key` (flash when
    /// left out); a place stage0 cannot read is refused.
    #[test]
    fn stage0_key_place_comes_from_the_definition() {
        let want = "pub const STAGE0_KEY_PLACE: &str = \"flash\";\n";
        let clock = "[[stage0.clock]]\nsource = \"internal\"\nout_hz = 6_500_000\n";
        for stage0 in [
            "[stage0]\nregion_bytes = 0x4000\nhead_bytes = 0\n",
            "[stage0]\nkey = \"flash\"\nhead_bytes = 0\n",
        ] {
            let rs = definition_rs(&format!("{DEF}{stage0}{clock}"));
            assert!(rs.contains(want), "{rs}");
        }
        let otp = std::panic::catch_unwind(|| {
            definition_rs(&format!(
                "{DEF}[stage0]\nkey = \"otp\"\nhead_bytes = 0\n{clock}"
            ))
        });
        let msg = *otp.unwrap_err().downcast::<String>().unwrap();
        assert!(msg.contains("cannot read a key from that place"), "{msg}");
    }

    /// The repository root: the nearest folder above the compiling crate's
    /// manifest that holds this crate (other crates include this file).
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .find(|d| d.join("targets/mcu/build/Cargo.toml").is_file())
            .expect("repository root")
            .canonicalize()
            .unwrap()
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            if p.is_dir() {
                if !["target", "node_modules", ".git", "pkg"].contains(&name.as_ref()) {
                    rust_files(&p, out);
                }
            } else if name.ends_with(".rs") {
                out.push(p);
            }
        }
    }

    /// The MCU packages: each folder of `targets/mcu` with a definition.
    fn mcu_packages() -> Vec<PathBuf> {
        let dir = repo_root().join("targets/mcu");
        let mut v: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("dagon-mcu.toml").is_file())
            .collect();
        v.sort();
        assert!(v.len() >= 3, "{v:?}");
        v
    }

    /// Every MCU package gets its key from [`embed_pubkey`] and includes the
    /// file it writes, and where its definition puts the key
    /// (`[stage0] key`) is the one place that file serves (`flash`).
    #[test]
    fn every_mcu_key_lands_where_its_definition_says() {
        for pkg in mcu_packages() {
            let name = pkg.file_name().unwrap().to_string_lossy().into_owned();
            let build = fs::read_to_string(pkg.join("build.rs")).unwrap();
            assert!(build.contains("embed_pubkey()"), "{name}: build.rs");
            let lib = fs::read_to_string(pkg.join("src/lib.rs")).unwrap();
            assert!(lib.contains("/pubkey.rs\""), "{name}: src/lib.rs");
            let def = fs::read_to_string(pkg.join("dagon-mcu.toml")).unwrap();
            let table: toml::Table = def.parse().unwrap();
            if table.contains_key("stage0") {
                let rs = definition_rs(&def);
                assert!(rs.contains("STAGE0_KEY_PLACE: &str = \"flash\""), "{name}");
            }
        }
    }

    /// The embedded constants are the key's bytes in order, and `TEST_KEY`
    /// is set for the RFC 8032 key alone.
    #[test]
    fn pubkey_constants_hold_the_key_bytes() {
        let key = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1F";
        let rs = pubkey_rs(key);
        let bytes: Vec<String> = (0..32).map(|i| format!("{i:#04x}")).collect();
        assert!(rs.contains(&format!("[{}]", bytes.join(", "))), "{rs}");
        assert!(rs.contains("TEST_KEY: bool = false"), "{rs}");
        let rs = pubkey_rs(&TEST_PUBKEY_HEX.to_uppercase());
        assert!(rs.contains("0xd7, 0x5a, 0x98, 0x01"), "{rs}");
        assert!(rs.contains("TEST_KEY: bool = true"), "{rs}");
    }

    /// The code that reads the key from the build environment and writes it
    /// into a build exists in this crate only.
    #[test]
    fn key_embedding_code_exists_only_in_this_crate() {
        let root = repo_root();
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        let here = root.join("targets/mcu/build/src/lib.rs");
        let words = [
            "DAGON_PUBKEY_HEX",
            "var(\"DAGON_PUBKEY\")",
            "rustc-env=DAGON_PUBKEY",
        ];
        let mut found = Vec::new();
        for f in files.iter().filter(|f| f.canonicalize().unwrap() != here) {
            let text = fs::read_to_string(f).unwrap_or_default();
            if words.iter().any(|w| text.contains(w)) {
                found.push(f.strip_prefix(&root).unwrap().display().to_string());
            }
        }
        assert!(found.is_empty(), "key embedding outside: {found:?}");
    }

    /// `STAGE0_INTERNAL_HZ` is the internal setup's `out_hz`; a stage0
    /// without one does not build.
    #[test]
    fn stage0_internal_hz_comes_from_the_internal_setup() {
        let rs = definition_rs(&format!(
            "{DEF}[stage0]\nhead_bytes = 0\n[[stage0.clock]]\nsource = \"crystal\"\nin_hz = 12\nout_hz = 125\n\
             [[stage0.clock]]\nsource = \"internal\"\nout_hz = 6_500_000\n"
        ));
        assert!(
            rs.contains("pub const STAGE0_INTERNAL_HZ: u32 = 6500000;\n"),
            "{rs}"
        );
        assert!(!definition_rs(DEF).contains("STAGE0_INTERNAL_HZ"));
        let none =
            std::panic::catch_unwind(|| definition_rs(&format!("{DEF}[stage0]\nhead_bytes = 0\n")));
        let msg = *none.unwrap_err().downcast::<&str>().unwrap();
        assert!(msg.contains("no internal [[stage0.clock]]"), "{msg}");
    }

    /// `clock.rs` names the chosen setup, its PLL in the stage0's own
    /// `Pll` type.
    #[test]
    fn stage0_clock_writes_the_chosen_setup() {
        let rs = stage0_clock_rs(
            "crystal",
            Some(12_000_000),
            125_000_000,
            &[("fbdiv", 125), ("postdiv1", 6)],
        );
        for want in [
            "pub const SOURCE: &str = \"crystal\";\n",
            "pub const IN_HZ: Option<u32> = Some(12000000);\n",
            "pub const OUT_HZ: u32 = 125000000;\n",
            "pub const PLL: Option<Pll> = Some(Pll { fbdiv: 125, postdiv1: 6 });\n",
        ] {
            assert!(rs.contains(want), "{want} in {rs}");
        }
        let rs = stage0_clock_rs("internal", None, 16_000_000, &[]);
        assert!(
            rs.contains("pub const IN_HZ: Option<u32> = None;\n"),
            "{rs}"
        );
        assert!(rs.contains("pub const PLL: Option<Pll> = None;\n"), "{rs}");
    }

    /// Stage0's linker script: the whole region as STAGE0, the head
    /// block and the rest, and the assertion on the image's end.
    #[test]
    fn stage0_memory_holds_the_region_and_its_assertion() {
        let def = "[layout]\nstage0 = { at = 0x1000_0000, size = 0x4000 }\n\
                   [stage0]\nregion_bytes = 0x4000\nhead_bytes = 0x100\n";
        assert_eq!(stage0_region_of(def), (0x1000_0000, 0x4000));
        assert_eq!(stage0_head_of(&def.parse().unwrap()), 0x100);
        let x = stage0_memory_text(
            (0x1000_0000, 0x4000),
            0x100,
            "SECTIONS {}\n",
            (0x2000_0000, 0x1000),
        );
        for want in [
            "STAGE0 : ORIGIN = 0x10000000, LENGTH = 0x4000",
            "HEAD   : ORIGIN = 0x10000000, LENGTH = 0x100",
            "FLASH  : ORIGIN = 0x10000100, LENGTH = 0x3f00",
            "SECTIONS {}\n__stage0_image_end = LOADADDR(.data) + SIZEOF(.data);",
            "ASSERT(__stage0_image_end - ORIGIN(STAGE0) <= LENGTH(STAGE0) - LENGTH(STAGE0) / 16,",
        ] {
            assert!(x.contains(want), "{want} in\n{x}");
        }
        let x = stage0_memory_text((0x0800_0000, 0x4000), 0, "", (0x2000_0000, 0x1000));
        assert!(!x.contains("HEAD"), "{x}");
        assert!(
            x.contains("FLASH  : ORIGIN = 0x08000000, LENGTH = 0x4000"),
            "{x}"
        );
    }

    /// Facts not yet established stay marked so in the constants.
    #[test]
    fn unverified_and_unmeasured_facts_are_marked() {
        let def = DEF
            .replace("masked_max_us = 50", "masked_max_us = \"unmeasured\"")
            .replace("unit_max_ms = 400", "unit_max_ms = { unverified = 400 }");
        let rs = definition_rs(&def);
        assert!(
            rs.contains("pub const MASKED_MAX_US: Option<u32> = None;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const MASKED_MAX_US_KNOWN: bool = false;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS: u32 = 900;\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const ERASE_UNIT_MAX_MS_KNOWN: bool = false;\n"),
            "{rs}"
        );
    }

    /// A definition with every layout fact the generated module carries:
    /// boot core slot B written before A, one app slot.
    const LAYOUT: &str = "format = 3\nid = \"m\"\nwire_id = 0x0042\n\
                          ram = { at = 0x2000_0000, size = 0x1_0000 }\n\
                          [[flash_map]]\nname = \"m-1m\"\nxip = true\nbase = 0x1000_0000\nbytes = 0x10_0000\n\
                          [layout]\nstage0 = { at = 0x1000_0000, size = 0x4000 }\n\
                          app_log = { faces = [0x1000_4000, 0x1000_5000], face_size = 0x1000 }\n\
                          bootcore_log = { faces = [0x1000_6000, 0x1000_7000], face_size = 0x1000 }\n\
                          [[slot]]\nimage = \"bootcore\"\nbinding = \"b\"\nheader = \"inline\"\nat = 0x1002_0000\nsize = 0x4000\n\
                          [[slot]]\nimage = \"bootcore\"\nbinding = \"a\"\nheader = \"inline\"\nat = 0x1000_8000\nsize = 0x2000\n\
                          [[slot]]\nimage = \"app\"\nbinding = \"a\"\nheader = \"inline\"\nat = 0x1003_0000\nsize = 0x1_0000\n\
                          [watchdog]\ntrial_attempts = 3\ntrial_ms = 4000\nconfirmed_ms = 8000\n\
                       range_ms = { min = 1, max = 9000 }\n\
                          [execution]\nmasked_max_us = 50\nwrite_unit = 1\nerased_value = 0xFF\n\
                          erase_regions = [{ start = 0x1000_0000, len = 0x10_0000, unit = 0x1000, unit_max_ms = 400 }]\n\
                          [boot.record]\nat = 0x2000_F000\nsize = 0x800\n";

    fn layout() -> Definition {
        Definition::parse(LAYOUT, |f| panic!("no file {f}"))
    }

    /// Each image kind gets its slots in slot order (A first), as many as
    /// the definition lists, whatever order it lists them in.
    #[test]
    fn the_slots_of_each_kind_come_from_the_definition() {
        let rs = layout().rs();
        assert!(rs.contains("pub mod slots {"), "{rs}");
        assert!(
            rs.contains(
                "pub const BOOTCORE: [(u32, u32); 2] = [(0x10008000, 0x2000), (0x10020000, 0x4000)];"
            ),
            "{rs}"
        );
        assert!(
            rs.contains("pub const APP: [(u32, u32); 1] = [(0x10030000, 0x10000)];"),
            "{rs}"
        );
        // Inline headers: the body follows the header's 512 bytes.
        assert!(
            rs.contains(
                "pub const BOOTCORE_PLACES: [(u32, u32, u32, u8); 2] = \
                 [(0x10008000, 0x10008200, 0x1e00, 0), (0x10020000, 0x10020200, 0x3e00, 0)];"
            ),
            "{rs}"
        );
        let d = layout();
        assert_eq!(
            d.slots("bootcore"),
            [(0x1000_8000, 0x2000), (0x1002_0000, 0x4000)]
        );
        assert_eq!(d.slot("bootcore", true), (0x1002_0000, 0x4000));
        assert_eq!(d.slot("app", false), (0x1003_0000, 0x1_0000));
    }

    /// A build linked for a slot the definition does not give is refused
    /// by name.
    #[test]
    #[should_panic(expected = "no app slot b")]
    fn a_build_for_a_slot_the_definition_lacks_is_refused() {
        layout().slot("app", true);
    }

    #[test]
    fn the_layout_regions_come_from_the_definition() {
        let rs = layout().rs();
        for want in [
            "pub const MCU_WIRE_ID: u16 = 0x42;",
            "pub const DEFAULT_FLASH: (u32, u32) = (0x10000000, 0x100000);",
            "pub const RAM_REGION: (u32, u32) = (0x20000000, 0x10000);",
            "pub const BOOT_RECORD_REGION: (u32, u32) = (0x2000f000, 0x800);",
            "pub const STAGE0_REGION: (u32, u32) = (0x10000000, 0x4000);",
            "pub const APP_LOG_REGION: ([u32; 2], u32) = ([0x10004000, 0x10005000], 0x1000);",
            "pub const BOOTCORE_LOG_REGION: ([u32; 2], u32) = ([0x10006000, 0x10007000], 0x1000);",
        ] {
            assert!(rs.contains(want), "{want}\n{rs}");
        }
        let d = layout();
        assert_eq!(d.stage0(), Some((0x1000_0000, 0x4000)));
        assert_eq!(d.ram(), Some((0x2000_0000, 0x1_0000)));
        assert_eq!(d.boot_record(), Some((0x2000_F000, 0x800)));
    }

    /// A definition without stage0, logs, RAM or boot record states none of
    /// them (its package has no use for them).
    #[test]
    fn facts_a_definition_leaves_out_are_not_made_up() {
        let def =
            format!("wire_id = 7\n[[flash_map]]\nname = \"x\"\nbase = 0\nbytes = 0x1000\n{DEF}");
        let rs = Definition::parse(&def, |f| panic!("no file {f}")).rs();
        for absent in [
            "STAGE0_REGION",
            "APP_LOG_REGION",
            "RAM_REGION",
            "BOOT_RECORD_REGION",
            "PARTITION_TABLE",
        ] {
            assert!(!rs.contains(absent), "{absent}\n{rs}");
        }
    }

    /// A slot given as a partition takes its place from the partition
    /// table the definition names; the table itself is generated too.
    #[test]
    fn a_partition_slot_takes_the_partition_table() {
        let def = "format = 3\nid = \"e\"\nwire_id = 3\npartitions = \"p.csv\"\n\
                   [[flash_map]]\nname = \"e-16m\"\nxip = false\nbase = 0x0\nbytes = 0x100_0000\n\
                   [[slot]]\nimage = \"app\"\nbinding = \"any\"\nheader = \"detached\"\npartition = \"ota_0\"\n\
                   header_partition = \"dagon\"\nheader_offset = 0x1000\n\
                   [watchdog]\ntrial_attempts = 3\ntrial_ms = 4000\nconfirmed_ms = 4000\n\
                   range_ms = { min = 1, max = 9000 }\n\
                   [execution]\nmasked_max_us = 50\nwrite_unit = 4\nerased_value = 0xFF\n\
                   erase_regions = [{ start = 0x0, len = 0x100_0000, unit = 0x1000, unit_max_ms = 500 }]\n";
        let csv = "# Name, Type, SubType, Offset, Size\n\
                   otadata, data, ota, 0x00D000, 0x2000\n\
                   factory, app, factory, 0x010000, 1M\n\
                   ota_0, app, ota_0, 0x110000, 3M\n\
                   dagon, 0x40, 0x00, 0x710000, 64K\n";
        let d = Definition::parse(def, |f| {
            assert_eq!(f, "p.csv");
            csv.to_string()
        });
        let rs = d.rs();
        assert!(
            rs.contains("pub const APP: [(u32, u32); 1] = [(0x110000, 0x300000)];"),
            "{rs}"
        );
        // The header in its own sector of the dagon partition; the body
        // takes the whole slot.
        assert!(
            rs.contains(
                "pub const APP_PLACES: [(u32, u32, u32, u8); 1] = [(0x711000, 0x110000, 0x300000, 1)];"
            ),
            "{rs}"
        );
        for row in [
            "(\"otadata\", 0x1, 0x0, 0xd000, 0x2000)",
            "(\"factory\", 0x0, 0x0, 0x10000, 0x100000)",
            "(\"ota_0\", 0x0, 0x10, 0x110000, 0x300000)",
            "(\"dagon\", 0x40, 0x0, 0x710000, 0x10000)",
        ] {
            assert!(rs.contains(row), "{row}\n{rs}");
        }
        assert!(
            rs.contains("pub const PARTITION_TABLE: [(&str, u8, u8, u32, u32); 4]"),
            "{rs}"
        );
    }

    /// The node's definition (`$DAGON_BOOT_CRATE/mcu.toml`) is taken only
    /// for this package's MCU, in this package's definition format.
    #[test]
    fn the_node_definition_is_the_packages_mcu_in_its_format() {
        let own = "format = 3\nid = \"m\"\n";
        assert_eq!(
            same_mcu(own, "format = 3\nid = \"m\"\nwire_id = 1\n"),
            Ok(())
        );
        let e = same_mcu(own, "format = 3\nid = \"other\"\n").unwrap_err();
        assert!(e.contains("'other'") && e.contains("'m'"), "{e}");
        let e = same_mcu(own, "format = 4\nid = \"m\"\n").unwrap_err();
        assert!(e.contains("format 4") && e.contains("format 3"), "{e}");
        let e = same_mcu(own, "id = \"m\"\n").unwrap_err();
        assert!(e.contains("format 1"), "{e}");
        assert!(same_mcu(own, "not toml [").is_err());
    }

    /// The generated periods lie inside the range the MCU's watchdog can
    /// run (`[watchdog] range_ms`): both boundaries are taken.
    #[test]
    fn periods_on_the_boundaries_of_the_watchdog_range_are_taken() {
        let def = DEF.replace(
            "range_ms = { min = 1, max = 9000 }",
            "range_ms = { min = 4000, max = 8000 }",
        );
        let rs = definition_rs(&format!("id = \"m\"\n{def}"));
        assert!(rs.contains("pub const TRIAL_WDT_MS: u32 = 4000;\n"), "{rs}");
        assert!(
            rs.contains("pub const WATCHDOG_RANGE_MS: (u32, u32) = (4000, 8000);\n"),
            "{rs}"
        );
        assert!(
            rs.contains("pub const CONFIRMED_WDT_MS: u32 = 8000;\n"),
            "{rs}"
        );
    }

    #[test]
    #[should_panic(
        expected = "MCU 'm': [watchdog] confirmed_ms 8001 is outside the periods its watchdog can run, 4000..=8000 ms"
    )]
    fn a_period_past_the_watchdog_range_is_refused() {
        let def = DEF
            .replace(
                "range_ms = { min = 1, max = 9000 }",
                "range_ms = { min = 4000, max = 8000 }",
            )
            .replace("confirmed_ms = 8000", "confirmed_ms = 8001");
        definition_rs(&format!("id = \"m\"\n{def}"));
    }

    #[test]
    #[should_panic(
        expected = "MCU 'm': [watchdog] trial_ms 3999 is outside the periods its watchdog can run, 4000..=8000 ms"
    )]
    fn a_period_below_the_watchdog_range_is_refused() {
        let def = DEF
            .replace(
                "range_ms = { min = 1, max = 9000 }",
                "range_ms = { min = 4000, max = 8000 }",
            )
            .replace("trial_ms = 4000", "trial_ms = 3999");
        definition_rs(&format!("id = \"m\"\n{def}"));
    }

    /// The ESP32-S3 definition ends its watchdog range at 3,600,000 ms: a
    /// period on it generates, one past it is refused.
    #[test]
    fn the_esp32s3_definition_bounds_its_watchdog_at_3_600_000_ms() {
        let text = include_str!("../../esp32s3/dagon-mcu.toml");
        let csv = include_str!("../../esp32s3/partitions.csv");
        let gen = |t: &str| {
            Definition::parse(t, |f| {
                assert_eq!(f, "partitions.csv");
                csv.to_string()
            })
            .rs()
        };
        let with = |ms: &str| text.replace("confirmed_ms = 4_000", &format!("confirmed_ms = {ms}"));
        assert!(gen(&with("3_600_000")).contains("pub const CONFIRMED_WDT_MS: u32 = 3600000;\n"));
        let past = with("3_600_001");
        let e = std::panic::catch_unwind(|| gen(&past)).unwrap_err();
        let e = e.downcast_ref::<String>().unwrap();
        assert!(
            e.contains("MCU 'esp32s3': [watchdog] confirmed_ms 3600001 is outside the periods its watchdog can run, 1..=3600000 ms"),
            "{e}"
        );
    }

    #[test]
    #[should_panic(expected = "[watchdog] range_ms is missing")]
    fn a_definition_without_the_watchdog_range_is_refused() {
        definition_rs(&DEF.replace("range_ms = { min = 1, max = 9000 }\n", ""));
    }

    /// The panic message of `f`.
    fn refusal(f: impl FnOnce() -> String + std::panic::UnwindSafe) -> String {
        let e = std::panic::catch_unwind(f).expect_err("refused");
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    }

    /// The flash's write unit and erased value (`[execution]`) and the
    /// stage0 head length (`[stage0] head_bytes`) are generated; each is a
    /// hardware fact with no default, so a definition without it is
    /// refused, naming the key.
    #[test]
    fn the_flash_write_facts_come_from_the_definition() {
        let stage0 = "[stage0]\nregion_bytes = 0x4000\nhead_bytes = 0x100\n\
                      [[stage0.clock]]\nsource = \"internal\"\nout_hz = 6_500_000\n";
        let rs = definition_rs(&format!("{DEF}{stage0}"));
        for want in [
            "pub const WRITE_UNIT: u32 = 4;\n",
            "pub const ERASED_VALUE: u8 = 0xff;\n",
            "pub const STAGE0_HEAD_LEN: u32 = 0x100;\n",
        ] {
            assert!(rs.contains(want), "{want}\n{rs}");
        }
        assert!(!definition_rs(DEF).contains("STAGE0_HEAD_LEN"));
        for (line, key) in [
            ("write_unit = 4\n", "[execution] write_unit"),
            ("erased_value = 0xFF\n", "[execution] erased_value"),
        ] {
            let def = DEF.replace(line, "");
            let msg = refusal(move || definition_rs(&def));
            assert!(msg.contains(&format!("{key} is missing")), "{msg}");
        }
        let def = format!("{DEF}{}", stage0.replace("head_bytes = 0x100\n", ""));
        let msg = refusal(move || definition_rs(&def));
        assert!(msg.contains("[stage0] head_bytes is missing"), "{msg}");
        let def = DEF.replace("erased_value = 0xFF", "erased_value = 0x100");
        let msg = refusal(move || definition_rs(&def));
        assert!(msg.contains("erased_value"), "{msg}");
    }

    /// The numeric literals of Rust source `src` a layout file may not
    /// hold: every one outside comments, strings, characters and
    /// `#[cfg(test)]` items but 0, 1, a shift amount and a tuple field.
    fn own_literals(src: &str) -> Vec<String> {
        let code = without_tests(&blank_text(src));
        let b = code.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let starts = b[i].is_ascii_digit()
                && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'));
            if !starts {
                i += 1;
                continue;
            }
            let at = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let before = code[..at].trim_end();
            let field = before.ends_with('.') && !before.ends_with("..");
            if field || before.ends_with("<<") || before.ends_with(">>") {
                continue;
            }
            let lit = code[at..i].replace('_', "");
            let digits = [
                "usize", "u64", "u32", "u16", "u8", "isize", "i64", "i32", "i16", "i8",
            ]
            .iter()
            .find_map(|s| lit.strip_suffix(s))
            .unwrap_or(&lit);
            let value = match digits.get(..2) {
                Some("0x") => u64::from_str_radix(&digits[2..], 16).ok(),
                Some("0b") => u64::from_str_radix(&digits[2..], 2).ok(),
                _ => digits.parse::<u64>().ok(),
            };
            if !matches!(value, Some(0 | 1)) {
                out.push(code[at..i].to_string());
            }
        }
        out
    }

    /// `src` with comments, string and character literals blanked.
    fn blank_text(src: &str) -> String {
        let b = src.as_bytes();
        let mut out = b.to_vec();
        let mut i = 0;
        let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
            for c in &mut out[from..to] {
                if *c != b'\n' {
                    *c = b' ';
                }
            }
        };
        while i < b.len() {
            let rest = &b[i..];
            let end = if rest.starts_with(b"//") {
                i + rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len())
            } else if rest.starts_with(b"/*") {
                i + 2 + src[i + 2..].find("*/").map_or(rest.len() - 2, |e| e + 2)
            } else if rest[0] == b'"' {
                let mut j = i + 1;
                while j < b.len() && b[j] != b'"' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                j + 1
            } else if rest[0] == b'\''
                && (rest.get(2) == Some(&b'\'') || rest.get(1) == Some(&b'\\'))
            {
                i + 1
                    + rest[1..]
                        .iter()
                        .skip(1)
                        .position(|&c| c == b'\'')
                        .map_or(0, |e| e + 2)
            } else {
                i += 1;
                continue;
            };
            let end = end.min(b.len());
            blank(&mut out, i, end);
            i = end;
        }
        String::from_utf8(out).unwrap()
    }

    /// `code` without each `#[cfg(test)]` item (up to its closing brace or
    /// semicolon).
    fn without_tests(code: &str) -> String {
        let mut out = String::new();
        let mut rest = code;
        while let Some(at) = rest.find("#[cfg(test)]") {
            out += &rest[..at];
            let item = &rest[at..];
            let mut depth = 0;
            let mut end = item.len();
            for (k, c) in item.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' if depth == 1 => {
                        end = k + 1;
                        break;
                    }
                    '}' => depth -= 1,
                    ';' if depth == 0 => {
                        end = k + 1;
                        break;
                    }
                    _ => {}
                }
            }
            rest = &item[end..];
        }
        out + rest
    }

    /// The files of an MCU package that state its flash layout.
    const LAYOUT_FILES: [&str; 3] = ["src/layout.rs", "src/flash.rs", "build.rs"];

    /// No MCU package's layout, flash port or build script holds a number
    /// of its own: each takes the generated constants (`definition.rs`),
    /// so a hand-written copy of a definition value fails here.
    #[test]
    fn layout_files_hold_no_number_of_their_own() {
        let mut found = Vec::new();
        for pkg in mcu_packages() {
            for rel in LAYOUT_FILES {
                let Ok(src) = fs::read_to_string(pkg.join(rel)) else {
                    continue;
                };
                for lit in own_literals(&src) {
                    found.push(format!(
                        "{}/{rel}: {lit}",
                        pkg.file_name().unwrap().to_string_lossy()
                    ));
                }
            }
        }
        assert!(
            found.is_empty(),
            "numbers outside the definition:\n{}",
            found.join("\n")
        );
    }

    /// The scan finds a planted copy and passes what is allowed.
    #[test]
    fn the_number_scan_finds_a_planted_copy() {
        assert_eq!(
            own_literals("pub const WRITE_UNIT: u32 = 4;\nconst E: u8 = 0xFF;\nlet b = [0u8; 8];"),
            ["4", "0xFF", "8"]
        );
        let allowed = "// 0x100 bytes\nconst A: u32 = 0; let b = 1u8; let c = 1 << 8; x.2;\n\
                       let s = \"0x100\"; let c = '7'; fn f<'d>() {}\n\
                       #[cfg(test)]\nmod tests { const T: u32 = 0x1000_8000; }\n\
                       #[cfg(test)]\nconst U: u32 = 42;\n";
        assert!(
            own_literals(allowed).is_empty(),
            "{:?}",
            own_literals(allowed)
        );
        assert_eq!(
            own_literals(&format!("{allowed}const V: u32 = 0x200;")),
            ["0x200"]
        );
    }

    /// `bytes` as the README tables write a size.
    fn size_text(bytes: u32) -> String {
        const KIB: u32 = 1 << 10;
        const MIB: u32 = 1 << 20;
        match bytes {
            b if b % MIB == 0 => format!("{} MiB", b / MIB),
            b if b % KIB == 0 => format!("{} KiB", b / KIB),
            b => format!("{b} B"),
        }
    }

    /// The flash table of an MCU package's README, from its definition:
    /// stage0 (with its head), the state log faces, the conformance
    /// scratch and every slot the node receives into, by address.
    fn flash_table(d: &Definition) -> String {
        let mut rows: Vec<(u32, String, String)> = Vec::new();
        if let Some((at, size)) = d.stage0() {
            let head = d.t.get("stage0").and_then(|s| s.get("head_bytes"));
            let what = match head.map_or(0, |h| int(h, "head_bytes")) {
                0 => "stage0".to_string(),
                h => format!("{} head the boot ROM reads, then stage0", size_text(h)),
            };
            rows.push((at, size_text(size), what));
        }
        let logs = [
            ("app", d.log("app_log")),
            ("boot core", d.log("bootcore_log")),
        ];
        if logs[0].1.is_some() && logs[0].1 == logs[1].1 {
            let (faces, size) = logs[0].1.unwrap();
            let what = "state log faces A/B (app and boot core records)".to_string();
            rows.push((faces[0], format!("2 x {}", size_text(size)), what));
        } else {
            for (name, log) in logs {
                if let Some((faces, size)) = log {
                    let what = format!("{name} state log faces A/B");
                    rows.push((faces[0], format!("2 x {}", size_text(size)), what));
                }
            }
        }
        let scratch = d.t.get("conformance").and_then(|c| c.get("scratch"));
        if let Some((at, size)) = scratch.and_then(|s| region(s, "[conformance] scratch")) {
            rows.push((at, size_text(size), "conformance scratch".into()));
        }
        for l in array(&d.t, "slot_layout") {
            let word = |k: &str| text(l, k, "[[slot_layout]]");
            let num = |k: &str| int(l.get(k).expect("[[slot_layout]]"), k);
            let (target, slot) = (word("target"), word("slot"));
            let (start, capacity) = (num("start"), num("capacity"));
            let name = match target {
                "bootcore" => "boot core",
                t => t,
            };
            let slot_name = match slot {
                "factory" => "factory".to_string(),
                s => s.to_ascii_uppercase(),
            };
            let row = match l.get("header").and_then(|h| h.get("detached")) {
                Some(at) => (
                    start,
                    size_text(capacity),
                    format!(
                        "{name} slot {slot_name}, header at 0x{:08X}",
                        int(at, "detached")
                    ),
                ),
                None => {
                    let size = d
                        .slot_rows()
                        .into_iter()
                        .find(|(i, _, (at, _))| i == target && *at == start)
                        .map_or(capacity, |(_, _, (_, size))| size);
                    let header = size - capacity;
                    (
                        start,
                        size_text(size),
                        format!(
                            "{name} slot {slot_name}: {header}-byte header, body at 0x{:08X}",
                            start + header
                        ),
                    )
                }
            };
            rows.push(row);
        }
        rows.sort_by_key(|r| r.0);
        let mut out = String::from("| Address | Size | Content |\n|---|---|---|\n");
        for (at, size, what) in rows {
            out += &format!("| 0x{at:08X} | {size} | {what} |\n");
        }
        out
    }

    /// The flash table of `readme`: from its header row to the blank line.
    fn readme_table(readme: &str) -> Option<&str> {
        let at = readme.find("| Address | Size | Content |")?;
        let end = readme[at..]
            .find("\n\n")
            .map_or(readme.len(), |e| at + e + 1);
        Some(&readme[at..end])
    }

    /// Each MCU package's README shows its built-in definition's flash
    /// layout as [`flash_table`] writes it.
    #[test]
    fn every_readme_flash_table_is_the_definitions() {
        let mut wrong = Vec::new();
        for pkg in mcu_packages() {
            let text = fs::read_to_string(pkg.join("dagon-mcu.toml")).unwrap();
            let d = Definition::parse(&text, |rel| fs::read_to_string(pkg.join(rel)).unwrap());
            let want = flash_table(&d);
            let readme = fs::read_to_string(pkg.join("README.md"))
                .unwrap()
                .replace("\r\n", "\n");
            if readme_table(&readme) != Some(want.as_str()) {
                wrong.push(format!(
                    "{}/README.md: the table is\n{want}",
                    pkg.file_name().unwrap().to_string_lossy()
                ));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// A README table that differs from the definition is found.
    #[test]
    fn a_planted_readme_address_is_found() {
        let d = layout();
        let want = flash_table(&d);
        assert!(
            want.contains("| 0x10000000 | 16 KiB | stage0 |\n"),
            "{want}"
        );
        let readme = format!("# x\n\n{want}\nmore\n");
        assert_eq!(readme_table(&readme), Some(want.as_str()));
        let planted = readme.replace("0x10000000", "0x10000100");
        assert_ne!(readme_table(&planted), Some(want.as_str()));
    }
}
