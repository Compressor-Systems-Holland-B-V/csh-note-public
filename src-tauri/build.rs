fn main() {
    // the vendored openssl's objects name ossl_static.pdb, which it never ships; MSVC prints
    // LNK4099 for each of them
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/ignore:4099");
    }
    tauri_build::build()
}
