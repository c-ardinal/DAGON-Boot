//! Each test image's linker script (`core.x`, `app.x`) and the ROM's
//! functions they call (`rom.x`) on the linker's search path.

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    println!("cargo:rustc-link-search={dir}");
    println!("cargo:rustc-link-arg=-nostartfiles");
    println!("cargo:rustc-link-arg-bin=core=-Tcore.x");
    println!("cargo:rustc-link-arg-bin=app=-Tapp.x");
    for f in ["core.x", "app.x", "rom.x"] {
        println!("cargo:rerun-if-changed={f}");
    }
}
