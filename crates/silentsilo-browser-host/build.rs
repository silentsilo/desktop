//! Links the Visual C++ runtime statically, as tauri-build does for the app.
//! Without it the host needs VCRUNTIME140.dll, which a clean Windows does
//! not have, and the browser fails to start it while the app runs fine.
//! The Universal CRT stays dynamic: it is part of Windows 10 and later.
//!
//! Adapted from tauri-build's `static_vcruntime.rs` (MIT or Apache-2.0),
//! itself taken from https://github.com/ChrisDenton/static_vcruntime.

use std::io::Write;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    override_msvcrt_lib();

    for lib in [
        "libvcruntimed.lib",
        "vcruntime.lib",
        "vcruntimed.lib",
        "libcmtd.lib",
        "msvcrt.lib",
        "msvcrtd.lib",
        "libucrt.lib",
        "libucrtd.lib",
    ] {
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:{lib}");
    }
    for lib in ["libcmt.lib", "libvcruntime.lib", "ucrt.lib"] {
        println!("cargo:rustc-link-arg=/DEFAULTLIB:{lib}");
    }
}

/// Rust's std names msvcrt.lib outright, which /NODEFAULTLIB cannot undo, so
/// an empty msvcrt.lib in OUT_DIR is found first.
fn override_msvcrt_lib() {
    let machine: &[u8] = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => &[0x64, 0x86],
        Ok("x86") => &[0x4C, 0x01],
        _ => return,
    };
    let bytes: &[u8] = &[
        1, 0, 94, 3, 96, 98, 60, 0, 0, 0, 1, 0, 0, 0, 0, 0, 132, 1, 46, 100, 114, 101, 99, 116,
        118, 101, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 10, 16, 0, 46, 100, 114, 101, 99, 116, 118, 101, 0, 0, 0, 0, 1, 0, 0, 0, 3, 0, 4, 0,
        0, 0,
    ];
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let path = Path::new(&out_dir).join("msvcrt.lib");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        f.write_all(machine).expect("write msvcrt.lib");
        f.write_all(bytes).expect("write msvcrt.lib");
    }
    println!("cargo:rustc-link-search=native={out_dir}");
}
