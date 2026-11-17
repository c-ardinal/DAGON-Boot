//! The link kinds as the page reads them: codes to decide on, names only to
//! show a kind and to read and write a 構成.

#[cfg(test)]
mod tests {
    use super::*;
    use dagon_topology::schema::LinkKind as Kind;

    #[test]
    fn the_table_is_every_kind_in_code_order() {
        assert_eq!(KINDS.len(), Kind::ALL.len());
        let names: Vec<&str> = KINDS.iter().map(|&(_, n)| n).collect();
        let all: Vec<&str> = Kind::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(names, all);
        assert!(KINDS.windows(2).all(|w| w[0].0 < w[1].0), "{KINDS:?}");
    }

    #[test]
    fn codes_go_both_ways() {
        for &k in Kind::ALL {
            let c = code(k);
            assert_eq!(of_code(c), Some(k));
            assert!(KINDS.contains(&(c, k.name())), "{k:?}");
        }
        assert_eq!(of_code(0), None);
    }
}
