//! The package README agrees with the code it describes.

use dagon_mcu_stm32f401re::links::LINK_KINDS;

/// README.md's link matrix is the one `LINK_KINDS` generates.
#[test]
fn the_readme_link_matrix_is_generated_from_link_kinds() {
    let mut want = String::new();
    dagon_mcu_common::link::write_matrix(LINK_KINDS, &mut want).unwrap();
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let at = readme
        .find("| Kind | Up | Down |")
        .expect("README.md has the matrix");
    let end = readme[at..]
        .find("\n\n")
        .map_or(readme.len(), |e| at + e + 1);
    assert_eq!(
        &readme[at..end],
        want,
        "README.md: replace the matrix with this"
    );
}
