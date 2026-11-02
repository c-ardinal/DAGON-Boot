fn main() {
    println!("cargo:rerun-if-env-changed=DAGON_PUBKEY");
    if std::env::var_os("DAGON_PUBKEY").is_none() {
        println!(
            "cargo:warning=DAGON_PUBKEY is unset: built with the public RFC 8032 test key \
             (TEST KEY), which accepts images signed with a published secret"
        );
    }
}
