//! Where this user's sockets live, and what the system says about the
//! process on the other end of one. Shared by the browser's channel and the
//! SSH agent's.

use std::io;
use std::path::PathBuf;

/// The directory the sockets go in, this user's alone.
///
/// Linux: `$XDG_RUNTIME_DIR/silentsilo`, with no fallback to a directory
/// other users share. macOS has no runtime directory, so it is
/// `~/Library/Application Support/SilentSilo`: a path that stays the same
/// from one start to the next, which `SSH_AUTH_SOCK` needs.
#[cfg(target_os = "linux")]
pub fn socket_dir() -> io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|dir| !dir.is_empty())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "XDG_RUNTIME_DIR is not set, so there is no private place for the connection",
            )
        })?;
    Ok(PathBuf::from(runtime).join("silentsilo"))
}

#[cfg(not(target_os = "linux"))]
pub fn socket_dir() -> io::Result<PathBuf> {
    let data = dirs::data_dir().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "no Application Support directory")
    })?;
    Ok(data.join("SilentSilo"))
}

/// `name` in [`socket_dir`], refused when too long to bind: a socket's path
/// fits in 104 bytes on macOS and 108 on Linux, and a long user name in
/// `/Users/` can pass that.
pub fn socket_in(name: &str) -> io::Result<PathBuf> {
    const MAX: usize = if cfg!(target_os = "linux") { 107 } else { 103 };
    let path = socket_dir()?.join(name);
    if path.as_os_str().len() > MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "the socket's path is too long for the system: {}",
                path.display()
            ),
        ));
    }
    Ok(path)
}

/// The executable process `pid` runs, by its path.
#[cfg(target_os = "macos")]
pub fn image_path(pid: u32) -> io::Result<PathBuf> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is as large as the call is told.
    let len = unsafe { libc::proc_pidpath(pid as i32, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if len <= 0 {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(len as usize);
    Ok(PathBuf::from(String::from_utf8_lossy(&buf).into_owned()))
}

/// The parent of process `pid`.
#[cfg(target_os = "macos")]
pub fn parent_pid(pid: u32) -> Option<u32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: `info` is as large as the call is told, and zeroed.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (got == size).then_some(info.pbi_ppid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_socket_is_named_in_the_private_directory() {
        let Ok(dir) = socket_dir() else {
            return; // A Linux session without XDG_RUNTIME_DIR, as in some CI.
        };
        assert_eq!(socket_in("browser.sock").unwrap(), dir.join("browser.sock"));
    }

    #[test]
    fn a_name_past_the_limit_is_refused() {
        if socket_dir().is_err() {
            return;
        }
        let long = "x".repeat(200);
        assert!(socket_in(&long).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_process_is_found_with_its_parent() {
        let me = std::process::id();
        let canonical = |p: PathBuf| std::fs::canonicalize(p).unwrap();
        assert_eq!(
            canonical(image_path(me).unwrap()),
            canonical(std::env::current_exe().unwrap())
        );
        assert!(parent_pid(me).is_some());
    }
}
