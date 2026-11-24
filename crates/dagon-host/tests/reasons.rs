//! `REASONS` is every text `reason` gives, and nothing else: a reason added
//! to `reason` fails here until it is listed (and so until the page has its
//! words for it).

use std::collections::BTreeSet;

#[test]
fn reasons_lists_every_text_reason_gives() {
    let src = include_str!("../src/ops.rs");
    let start = src.find("pub fn reason<E>").expect("reason is in ops.rs");
    let body = &src[start..];
    let body = &body[..body.find("\n}\n").expect("the end of reason")];
    let given: BTreeSet<&str> = body.split('"').skip(1).step_by(2).collect();
    let listed: BTreeSet<&str> = dagon_host::REASONS.iter().copied().collect();
    assert_eq!(listed, given);
    assert_eq!(listed.len(), dagon_host::REASONS.len(), "listed twice");
    assert!(dagon_host::REASONS
        .iter()
        .all(|r| r.is_ascii() && r.len() <= 32));
}
