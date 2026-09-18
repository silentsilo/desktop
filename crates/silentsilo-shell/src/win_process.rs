//! Who is at the other end of a local channel: a process's image, user,
//! parent and Authenticode signer. Used by the browser pipe on both sides,
//! the app checking its client and the host checking its server and the
//! browser that started it.
//!
//! These checks raise the cost for another program of the same user; they do
//! not stop one. Same-user code can inject into a process it may open, and
//! can pick the parent of a process it starts. ARCHITECTURE.md says which
//! attacks remain.

use std::io;
use std::os::windows::io::RawHandle;
use std::path::{Path, PathBuf};

use ::windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, HLOCAL, HWND, LocalFree};
use ::windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT,
};
use ::windows::Win32::Security::Cryptography::{CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW};
use ::windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvCertFromChain,
    WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust,
};
use ::windows::Win32::Security::{
    GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use ::windows::Win32::System::Com::CoTaskMemFree;
use ::windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use ::windows::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};
use ::windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use ::windows::Win32::UI::Shell::{
    FOLDERID_LocalAppData, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_System,
    FOLDERID_SystemX86, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
};
use ::windows::core::{GUID, HSTRING, PCWSTR, PWSTR};

fn other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// A process handle closed on drop.
struct Process(HANDLE);

impl Process {
    fn open(pid: u32) -> io::Result<Self> {
        // SAFETY: plain Win32 call; the handle is closed in Drop.
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map(Self)
            .map_err(other)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: a handle this struct opened.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The process at the client end of a server pipe handle.
pub fn pipe_client_pid(pipe: RawHandle) -> io::Result<u32> {
    let mut pid = 0u32;
    // SAFETY: the caller's handle is valid for the call.
    unsafe { GetNamedPipeClientProcessId(HANDLE(pipe), &mut pid) }.map_err(other)?;
    Ok(pid)
}

/// The process at the server end of a client pipe handle.
pub fn pipe_server_pid(pipe: RawHandle) -> io::Result<u32> {
    let mut pid = 0u32;
    // SAFETY: the caller's handle is valid for the call.
    unsafe { GetNamedPipeServerProcessId(HANDLE(pipe), &mut pid) }.map_err(other)?;
    Ok(pid)
}

/// The full path of the executable a process runs.
pub fn image_path(pid: u32) -> io::Result<PathBuf> {
    let process = Process::open(pid)?;
    let mut buf = vec![0u16; 32 * 1024];
    let mut len = buf.len() as u32;
    // SAFETY: the buffer and its length describe the same memory.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    }
    .map_err(other)?;
    buf.truncate(len as usize);
    Ok(PathBuf::from(String::from_utf16_lossy(&buf)))
}

fn sid_string(sid: PSID) -> io::Result<String> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` is valid for the call; the string is freed after copying.
    unsafe {
        ConvertSidToStringSidW(sid, &mut text).map_err(other)?;
        let out = text.to_string().map_err(other);
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        out
    }
}

fn token_user_sid(process: HANDLE) -> io::Result<String> {
    // SAFETY: the token is closed before return. The buffer is u64-aligned
    // for TOKEN_USER, sized by the first call, and outlives the SID copy.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(other)?;
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        let got = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            len,
            &mut len,
        );
        let _ = CloseHandle(token);
        got.map_err(other)?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        sid_string(user.User.Sid)
    }
}

/// The SID of the user this process runs as, `S-1-5-21-…`.
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: the pseudo-handle needs no closing.
    token_user_sid(unsafe { GetCurrentProcess() })
}

/// The SID of the user another process runs as.
pub fn user_sid(pid: u32) -> io::Result<String> {
    token_user_sid(Process::open(pid)?.0)
}

/// The owner of a kernel object, such as a pipe, as a SID string.
pub fn owner_sid(object: RawHandle) -> io::Result<String> {
    let mut owner = PSID::default();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `owner` points into `sd`, which is freed after the copy.
    unsafe {
        GetSecurityInfo(
            HANDLE(object),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            Some(&mut sd),
        )
        .ok()
        .map_err(other)?;
        let out = sid_string(owner);
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        out
    }
}

/// When a process started, in 100 ns ticks since 1601.
pub fn creation_time(pid: u32) -> io::Result<u64> {
    let process = Process::open(pid)?;
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: four out-pointers to locals.
    unsafe { GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user) }
        .map_err(other)?;
    Ok(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
}

/// The process that started `pid`, if it is still the one holding that
/// number: a parent that exited leaves its id free for reuse, and a process
/// started after the child cannot be its parent.
pub fn parent_pid(pid: u32) -> io::Result<u32> {
    // SAFETY: the snapshot is closed before return; the entry is sized.
    let parent = unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(other)?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            if entry.th32ProcessID == pid {
                found = Some(entry.th32ParentProcessID);
                break;
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        found.ok_or_else(|| other("the process is gone"))?
    };
    if creation_time(parent)? > creation_time(pid)? {
        return Err(other("the parent exited and its id was reused"));
    }
    Ok(parent)
}

/// The two paths name the same file. Compared after resolving both, and
/// without regard to case, as NTFS does.
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy()),
        _ => false,
    }
}

/// Who signed a file, once Windows accepted the signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signer {
    /// The signing certificate's display name, e.g. `Google LLC`.
    pub name: String,
    /// The signing certificate, DER.
    pub certificate: Vec<u8>,
}

/// The Authenticode signer of a file with an embedded signature that chains
/// to a trusted root. Revocation is not checked online: this runs on every
/// connection and must not wait on the network.
pub fn signer(path: &Path) -> io::Result<Signer> {
    let wide = HSTRING::from(path.as_os_str());
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: `data` and `file` outlive both calls; the state is closed
    // with the second call whatever the first answered, and the certificate
    // is copied out before that.
    unsafe {
        let status = WinVerifyTrust(
            HWND::default(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        );
        let result = if status != 0 {
            Err(other(format_args!(
                "{} is not validly signed ({status:#x})",
                path.display()
            )))
        } else {
            signer_of_state(data.hWVTStateData)
        };
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        let _ = WinVerifyTrust(
            HWND::default(),
            &mut action,
            (&mut data as *mut WINTRUST_DATA).cast(),
        );
        result
    }
}

/// SAFETY: `state` is the verify state of a successful WinVerifyTrust call
/// that has not been closed.
unsafe fn signer_of_state(state: HANDLE) -> io::Result<Signer> {
    let missing = || other("the signature names no signer");
    unsafe {
        let provider = WTHelperProvDataFromStateData(state);
        if provider.is_null() {
            return Err(missing());
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, false, 0);
        if signer.is_null() {
            return Err(missing());
        }
        let cert = WTHelperGetProvCertFromChain(signer, 0);
        if cert.is_null() || (*cert).pCert.is_null() {
            return Err(missing());
        }
        let context = (*cert).pCert;
        let der =
            std::slice::from_raw_parts((*context).pbCertEncoded, (*context).cbCertEncoded as usize)
                .to_vec();
        let mut name = vec![0u16; 512];
        let len = CertGetNameStringW(
            context,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            None,
            Some(&mut name),
        );
        let name = String::from_utf16_lossy(&name[..(len as usize).saturating_sub(1)]);
        Ok(Signer {
            name,
            certificate: der,
        })
    }
}

/// A folder the system names, such as Program Files.
fn known_folder(id: GUID) -> Option<PathBuf> {
    // SAFETY: the returned string is copied, then freed with the allocator
    // that made it.
    unsafe {
        let path = SHGetKnownFolderPath(&id, KF_FLAG_DEFAULT, None).ok()?;
        let out = path.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(path.0 as *const _));
        out
    }
}

/// Where browsers install: Program Files (both), and the user's local
/// application data for a per-user install.
pub fn install_roots() -> Vec<PathBuf> {
    [
        FOLDERID_ProgramFiles,
        FOLDERID_ProgramFilesX86,
        FOLDERID_LocalAppData,
    ]
    .into_iter()
    .filter_map(known_folder)
    .collect()
}

/// The system directories, where `cmd.exe` lives.
pub fn system_dirs() -> Vec<PathBuf> {
    [FOLDERID_System, FOLDERID_SystemX86]
        .into_iter()
        .filter_map(known_folder)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_known_by_its_own_image_and_user() {
        let me = std::process::id();
        let exe = std::env::current_exe().unwrap();
        assert!(same_file(&image_path(me).unwrap(), &exe));
        assert_eq!(user_sid(me).unwrap(), current_user_sid().unwrap());
        let parent = parent_pid(me).unwrap();
        assert!(creation_time(parent).unwrap() <= creation_time(me).unwrap());
    }

    #[test]
    fn different_files_are_different() {
        let exe = std::env::current_exe().unwrap();
        assert!(!same_file(&exe, &exe.with_file_name("no-such-file.exe")));
        let upper = PathBuf::from(exe.to_string_lossy().to_uppercase());
        assert!(same_file(&exe, &upper), "case does not matter on NTFS");
    }

    #[test]
    fn an_unsigned_file_has_no_signer() {
        // Test binaries are never signed.
        assert!(signer(&std::env::current_exe().unwrap()).is_err());
    }

    /// Against the browsers where they are installed; skipped where not.
    #[test]
    fn an_installed_browser_names_its_publisher() {
        for (dir, exe, publisher) in [
            (r"Google\Chrome\Application", "chrome.exe", "Google LLC"),
            (
                r"Microsoft\Edge\Application",
                "msedge.exe",
                "Microsoft Corporation",
            ),
        ] {
            for root in install_roots() {
                let path = root.join(dir).join(exe);
                if path.exists() {
                    let signer = signer(&path).unwrap();
                    assert_eq!(signer.name, publisher);
                    assert!(!signer.certificate.is_empty());
                }
            }
        }
    }

    #[test]
    fn the_system_folders_are_found() {
        assert!(!install_roots().is_empty());
        assert!(system_dirs().iter().any(|d| d.join("cmd.exe").exists()));
    }
}
