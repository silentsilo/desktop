fn main() {
    // OpenSSL, vendored by core for SQLCipher, is built without its PDB, and
    // MSVC's LNK4099 about it reaches rustc as a `linker_messages` warning on
    // every link. Here rather than in `.cargo/config.toml`, which this
    // repository keeps for the local core patch and never commits.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/ignore:4099");
    }
    require_google_secret();
    tauri_build::build()
}

/// Google Drive exists only in a build that has Google's client secret: core
/// reads it at compile time (`silentsilo-cloud`, `option_env!`). A release
/// without it ships without Google Drive while the store listing, the site
/// and the privacy page promise it, so a release build stops instead. A
/// build meant to go without it says so with `SILENTSILO_WITHOUT_GOOGLE=1`.
fn require_google_secret() {
    println!("cargo:rerun-if-env-changed=SILENTSILO_GOOGLE_CLIENT_SECRET");
    println!("cargo:rerun-if-env-changed=SILENTSILO_WITHOUT_GOOGLE");
    let present =
        std::env::var("SILENTSILO_GOOGLE_CLIENT_SECRET").is_ok_and(|s| !s.trim().is_empty());
    let waived = std::env::var_os("SILENTSILO_WITHOUT_GOOGLE").is_some();
    if present || waived {
        return;
    }
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        panic!(
            "{}",
            concat!(
                "SILENTSILO_GOOGLE_CLIENT_SECRET is not set, so this release would have ",
                "no Google Drive. Set it (the release machine keeps it in ",
                "~/.silentsilo-release/google-client-secret.txt), or set ",
                "SILENTSILO_WITHOUT_GOOGLE=1 for a build that goes without it."
            )
        );
    }
}
