//! Process mitigations applied at startup. None of this stops malware
//! already running as the user; what it removes is the cheap way in that
//! needs no exploit: extension points (AppInit_DLLs, SetWindowsHookEx),
//! where the system itself loads someone else's code into this process.
//! It is inherited by child processes and cannot be undone, so it goes
//! first.
//!
//! Deliberately not attempted: anti-debugging and obfuscation, which cost
//! real complexity and make no sense in a project whose source is
//! published.

/// Applies every mitigation this process can set for itself.
///
/// Best-effort by design. A policy an older Windows build does not know
/// about simply fails, and refusing to start over that would trade a real
/// benefit for an imaginary one.
pub fn harden_process() {
    #[cfg(windows)]
    imp::apply();
}

#[cfg(windows)]
mod imp {
    use ::windows::Win32::System::SystemServices::PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY;
    use ::windows::Win32::System::Threading::{
        ProcessExtensionPointDisablePolicy, SetProcessMitigationPolicy,
    };

    pub fn apply() {
        unsafe {
            // Bit 0 is DisableExtensionPoints, set through the union's flag
            // word: the bitfield is not portable across generated bindings.
            let mut extension_points = PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY::default();
            extension_points.Anonymous.Flags = 1;
            let _ = SetProcessMitigationPolicy(
                ProcessExtensionPointDisablePolicy,
                &extension_points as *const _ as *const core::ffi::c_void,
                size_of::<PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY>(),
            );
        }
    }
}

// `ProcessSignaturePolicy` with MicrosoftSignedOnly must not come back: it
// blocks every non-Microsoft DLL, and the file dialogs in this process load
// shell and network-provider DLLs. See docs/CRYPTO.md.
