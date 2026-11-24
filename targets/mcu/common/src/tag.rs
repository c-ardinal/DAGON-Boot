//! The node's image target tag, set per node at build time.

/// The 16-byte header field for `tag`. Fails (at compile time in a const)
/// if `tag` is empty, longer than 16 bytes or not printable ASCII, so a
/// boot core is never built without its node's tag.
pub const fn checked_tag(tag: &str) -> [u8; 16] {
    let b = tag.as_bytes();
    assert!(
        !b.is_empty(),
        "DAGON_TARGET_TAG is unset: set it to the node's image target tag (as in the topology)"
    );
    assert!(b.len() <= 16, "DAGON_TARGET_TAG: at most 16 characters");
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < b.len() {
        assert!(
            b[i] > 0x20 && b[i] < 0x7F,
            "DAGON_TARGET_TAG: printable ASCII without spaces"
        );
        out[i] = b[i];
        i += 1;
    }
    out
}

/// The tag stage0 compares a fallback app's header with: [`checked_tag`]
/// of `tag`, or all zeros (`dagon_core::bootcore::NO_TAG`) when the build
/// names no tag (a stage0 built to check or measure it): such a stage0
/// never starts an app.
pub const fn stage0_tag(tag: &str) -> [u8; 16] {
    if tag.is_empty() {
        [0; 16]
    } else {
        checked_tag(tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_fills_the_field() {
        let t = checked_tag("node-f401");
        assert_eq!(&t[..9], b"node-f401");
        assert_eq!(t[9..], [0; 7]);
    }

    #[test]
    fn a_stage0_without_a_tag_matches_no_image() {
        assert_eq!(stage0_tag(""), [0; 16]);
        assert_eq!(stage0_tag("seg"), checked_tag("seg"));
    }

    #[test]
    #[should_panic]
    fn empty_tag_is_refused() {
        checked_tag("");
    }

    #[test]
    #[should_panic]
    fn long_tag_is_refused() {
        checked_tag("seventeen-chars-x");
    }
}
