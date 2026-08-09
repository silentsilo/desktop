//! Process mitigations applied at startup. None of this stops malware
//! already running as the user; what it removes are the cheap ways in that
//! need no exploit: extension points (AppInit_DLLs, SetWindowsHookEx),
//! where the system itself loads someone else's code into this process,
//! and unsigned images dropped next to the executable. Both are inherited
//! by child processes and cannot be undone, so they go first.
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
    use ::windows::Win32::System::SystemServices::{
        PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY,
        PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY,
    };
    use ::windows::Win32::System::Threading::{
        ProcessExtensionPointDisablePolicy, ProcessSignaturePolicy, SetProcessMitigationPolicy,
    };

    pub fn apply() {
        unsafe {
            // Bit 0 is DisableExtensionPoints. Set through the union's flag
            // word rather than the bitfield, which is what the generated
            // bindings expose portably.
            let mut extension_points = PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY::default();
            extension_points.Anonymous.Flags = 1;
            let _ = SetProcessMitigationPolicy(
                ProcessExtensionPointDisablePolicy,
                &extension_points as *const _ as *const core::ffi::c_void,
                size_of::<PROCESS_MITIGATION_EXTENSION_POINT_DISABLE_POLICY>(),
            );

            // Bit 0 is MicrosoftSignedOnly. This is about *loaded images*,
            // not about our own signature: a third-party DLL dropped beside
            // the executable stops being loadable.
            let mut signature = PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY::default();
            signature.Anonymous.Flags = 1;
            let _ = SetProcessMitigationPolicy(
                ProcessSignaturePolicy,
                &signature as *const _ as *const core::ffi::c_void,
                size_of::<PROCESS_MITIGATION_BINARY_SIGNATURE_POLICY>(),
            );
        }
    }
}
