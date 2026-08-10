// Suppresses the console window Windows otherwise allocates when this exe is
// launched with no inherited console — exactly what happens via the
// Explorer shell-verb (Add to / Save here from SilentSilo). Safe when run
// from an actual terminal too: an already-inherited console (e.g. `cargo
// tauri dev`) still receives this process's stdout/stderr either way.
#![windows_subsystem = "windows"]

fn main() {
    silentsilo_lib::run();
}
