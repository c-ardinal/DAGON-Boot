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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_fills_the_field() {
        let t = checked_tag("seg-f401-7seg");
        assert_eq!(&t[..13], b"seg-f401-7seg");
        assert_eq!(t[13..], [0, 0, 0]);
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
