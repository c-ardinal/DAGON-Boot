//! Puts m0.ld (the program's memory map) on the linker's search path.

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    println!("cargo:rustc-link-search={dir}");
    println!("cargo:rerun-if-changed=m0.ld");
}
