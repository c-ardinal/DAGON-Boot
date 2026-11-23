//! Stage0's linker script (`stage0.x`) on the linker's search path.

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    println!("cargo:rustc-link-search={dir}");
    println!("cargo:rerun-if-changed=stage0.x");
}
