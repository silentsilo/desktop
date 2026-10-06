//! Named pipes only this user can open, shared by the browser extension's
//! channel and the SSH agent's.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::time::Duration;

use ::windows::Win32::Foundation::{HLOCAL, LocalFree};
use ::windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use ::windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use ::windows::core::HSTRING;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::watch;

use crate::win_process;

/// How long a server waits before trying again to create an instance
/// (all of them taken, say), doubling up to the second value.
const RETRY_FIRST: Duration = Duration::from_millis(100);
const RETRY_MAX: Duration = Duration::from_secs(5);

/// A security descriptor granting the current user, and nobody else,
/// full access. `P` protects the DACL from inheriting anything.
pub(crate) struct UserOnly {
    sd: PSECURITY_DESCRIPTOR,
    sid: String,
    max_instances: usize,
}

// SAFETY: the descriptor is immutable once built and only read by
// CreateNamedPipe; LocalFree on drop is the only other use.
unsafe impl Send for UserOnly {}
unsafe impl Sync for UserOnly {}

impl UserOnly {
    pub(crate) fn new(max_instances: usize) -> io::Result<Self> {
        let sid = win_process::current_user_sid()?;
        let sddl = HSTRING::from(format!("O:{sid}D:P(A;;GA;;;{sid})"));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the SDDL string outlives the call; the descriptor is
        // allocated by the system and freed in Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &mut sd,
                None,
            )?;
        }
        Ok(Self {
            sd,
            sid,
            max_instances,
        })
    }

    pub(crate) fn create(&self, name: &str, first: bool) -> io::Result<NamedPipeServer> {
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.sd.0,
            bInheritHandle: false.into(),
        };
        // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose
        // descriptor lives as long as `self`.
        let pipe = unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .max_instances(self.max_instances)
                .create_with_security_attributes_raw(
                    name,
                    (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
                )?
        };
        // A later instance joins whatever pipe holds the name. Had every
        // instance of ours closed and another user created the name
        // meanwhile, this one would be theirs.
        if !first && win_process::owner_sid(pipe.as_raw_handle())? != self.sid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the pipe name is held by another user",
            ));
        }
        Ok(pipe)
    }

    /// The next instance to wait on. Creating one fails while every
    /// instance is taken; the server tries again, more slowly each time,
    /// rather than giving the name up. `None` when stopped, an error when
    /// the name turned out to be another user's.
    pub(crate) async fn next_instance<W: Fn(String)>(
        &self,
        name: &str,
        stop: &mut watch::Receiver<bool>,
        warn: &W,
    ) -> io::Result<Option<NamedPipeServer>> {
        let mut delay = RETRY_FIRST;
        let mut warned = false;
        loop {
            match self.create(name, false) {
                Ok(pipe) => return Ok(Some(pipe)),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err(e),
                Err(e) => {
                    if !warned {
                        warn(format!("waiting to open another connection: {e}"));
                        warned = true;
                    }
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = stop.changed() => return Ok(None),
            }
            delay = (delay * 2).min(RETRY_MAX);
        }
    }
}

impl Drop for UserOnly {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.sd.0)));
        }
    }
}
