//! The VERSION field table of site/src/content/docs/en/explanation/lifecycle-events.md (5.4) is
//! `version_id::TABLE`, row for row, and every id the reply encodes is in
//! it: an id added to one side only fails here.

use dagon_format::Version;
use dagon_proto::payload::{version_id, VersionResp, VersionSlots};
use dagon_proto::table;

fn doc_rows() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../site/src/content/docs/en/explanation/lifecycle-events.md"
    );
    let doc = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let section = doc
        .split("### 5.4 VERSION query")
        .nth(1)
        .expect("section 5.4")
        .split("\n### ")
        .next()
        .unwrap();
    section
        .lines()
        .filter(|l| l.starts_with("| 0x"))
        .map(str::to_string)
        .collect()
}

fn code_rows() -> Vec<String> {
    version_id::TABLE
        .iter()
        .map(|(id, value)| format!("| {id:#04X} | {value} |").replace("0X", "0x"))
        .collect()
}

#[test]
fn the_doc_table_is_the_code_table() {
    let (doc, code) = (doc_rows(), code_rows());
    assert!(
        doc == code,
        "site/src/content/docs/en/explanation/lifecycle-events.md 5.4 must hold exactly these rows:\n{}",
        code.join("\n")
    );
}

#[test]
fn every_id_the_reply_encodes_is_in_the_table() {
    for f in table::version::RESP {
        assert!(
            version_id::TABLE
                .iter()
                .any(|(t, _)| u16::from(*t) == f.num),
            "field {} is sent but not in version_id::TABLE",
            f.num
        );
    }
    // Every number of the set is the command's own (1 to 31).
    assert!(version_id::TABLE.iter().all(|(t, _)| (1..=31).contains(t)));
    let mut b = [0u8; 64];
    let full = VersionResp {
        wire: Some(2),
        mcu_id: Some(dagon_format::header::McuId([1; 8])),
        stage0: Some(Version::new(0, 1, 0)),
        boot_core: Some(Version::new(1, 0, 0)),
        core_slots: Some(VersionSlots::default()),
        app_slots: Some(VersionSlots::default()),
    };
    assert_eq!(full.encode(&mut b), Ok(VersionResp::MAX_LEN));
}
