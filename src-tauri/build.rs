fn main() {
    // OpenSSL, vendored by core for SQLCipher, is built without its PDB, and
    // MSVC's LNK4099 about it reaches rustc as a `linker_messages` warning on
    // every link. Here rather than in `.cargo/config.toml`, which this
    // repository keeps for the local core patch and never commits.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/ignore:4099");
    }
    tauri_build::build()
}
