//! Windows FIDO2 via the OS WebAuthn API (`webauthn.dll`) — no admin required.
//!
//! MakeCredential mirrors Chromium's `win/webauthn_api.cc`:
//! - cross-platform attachment
//! - hmac-secret BOOL extension **and** `bEnablePrf`
//! - non-resident key, UV discouraged (touch, no PIN required)
//! - empty exclude-credential list pointer (non-null)
//! - API v8+ credential hint `"security-key"` to prefer USB keys over phone/hybrid

use std::ffi::c_void;
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use ciborium::Value;
use rand::RngCore;
use windows::Win32::Foundation::HWND;
use windows::Win32::Networking::WindowsWebServices::{
    WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE, WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY,
    WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM, WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM,
    WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS,
    WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_4,
    WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_6,
    WEBAUTHN_AUTHENTICATOR_HMAC_SECRET_VALUES_FLAG, WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS,
    WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_7, WEBAUTHN_CLIENT_DATA,
    WEBAUTHN_CLIENT_DATA_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETER,
    WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION, WEBAUTHN_COSE_CREDENTIAL_PARAMETERS,
    WEBAUTHN_CREDENTIAL_ATTESTATION, WEBAUTHN_CREDENTIAL_EX,
    WEBAUTHN_CREDENTIAL_EX_CURRENT_VERSION, WEBAUTHN_CREDENTIAL_LIST,
    WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY, WEBAUTHN_CREDENTIALS, WEBAUTHN_CTAP_TRANSPORT_HYBRID,
    WEBAUTHN_EXTENSION, WEBAUTHN_EXTENSIONS, WEBAUTHN_EXTENSIONS_IDENTIFIER_HMAC_SECRET,
    WEBAUTHN_HASH_ALGORITHM_SHA_256, WEBAUTHN_HMAC_SECRET_SALT, WEBAUTHN_HMAC_SECRET_SALT_VALUES,
    WEBAUTHN_RP_ENTITY_INFORMATION, WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
    WEBAUTHN_USER_ENTITY_INFORMATION, WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
    WEBAUTHN_USER_VERIFICATION_REQUIREMENT_DISCOURAGED,
    WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED, WebAuthNAuthenticatorGetAssertion,
    WebAuthNAuthenticatorMakeCredential, WebAuthNFreeAssertion, WebAuthNFreeCredentialAttestation,
    WebAuthNGetApiVersionNumber, WebAuthNGetErrorName,
    WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable,
};
use windows::Win32::UI::WindowsAndMessaging::{GetDesktopWindow, GetForegroundWindow};
use windows::core::{BOOL, HRESULT, PCWSTR};

/// Parent HWND for WebAuthn dialogs (set from the Tauri main window).
static PARENT_HWND: AtomicIsize = AtomicIsize::new(0);

/// Bind the SilentSilo main window so Windows can show the security-key prompt.
pub fn set_parent_hwnd(hwnd: isize) {
    if hwnd != 0 {
        PARENT_HWND.store(hwnd, Ordering::SeqCst);
    }
}

/// Waits for the platform's own ceremony window to be gone.
///
/// Windows runs one WebAuthn ceremony at a time, and enrolment needs two: one
/// to create the credential, one to derive the wrap key from it. Starting the
/// second while the first dialog is still tearing down makes Windows fail it
/// *inside its own prompt*, so "Something went wrong" lands in front of
/// someone who is deciding whether to trust this app with their files. Our
/// own retry cannot reach that: the call is still sitting inside the dialog
/// and has returned nothing to retry.
///
/// This used to be a fixed sleep, which is a guess by construction: too short
/// on a machine slower than the one it was measured on, and pure delay on
/// every other. Focus returns to the window the ceremony was parented to once
/// the dialog has really closed, so that is what is waited for instead.
pub(crate) fn wait_for_ceremony_teardown(timeout_ms: u64) {
    // The broker finishes putting itself away a moment after it hands the
    // focus back, and starting inside that moment fails the same way.
    const AFTER_FOCUS: Duration = Duration::from_millis(500);
    const POLL: Duration = Duration::from_millis(50);

    let parent = PARENT_HWND.load(Ordering::SeqCst);
    if parent == 0 {
        // Nothing to compare against, so a guess is all there is.
        std::thread::sleep(Duration::from_millis(timeout_ms.min(1500)));
        return;
    }

    // Capped, because a user who alt-tabs to another window during the
    // ceremony would otherwise be waited on for ever. Going ahead after the
    // cap is no worse than the fixed sleep this replaces.
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if unsafe { GetForegroundWindow() }.0 as isize == parent {
            break;
        }
        std::thread::sleep(POLL);
    }

    std::thread::sleep(AFTER_FOCUS);
}

use crate::client_data::{create_client_data_json, get_client_data_json, hmac_salt_from_string};
use crate::types::{
    Authenticator, CredentialInfo, Enrollment, EnrollmentChallenge, UnlockMaterial,
};
use crate::{FidoError, RP_ID, dek_salt_for_vault};

const ES256_ALG: i32 = -7;
const TIMEOUT_MS: u32 = 120_000;
/// webauthn.h — not yet in windows crate 0.61.
const MAKE_CRED_OPTIONS_VERSION_8: u32 = 8;
const API_VERSION_8: u32 = 8;
/// The attestation version that first carries `pHmacSecret`. The crate knows
/// up to 6.
const CREDENTIAL_ATTESTATION_VERSION_7: u32 = 7;

/// The attestation from version 7 of `webauthn.h`, which added
/// `pHmacSecret`: the PRF output evaluated while the credential was being
/// created. The `windows` crate stops at version 6, so the field is added
/// here the same way the options struct below is extended.
///
/// Laid out as C does it, base first: the crate's struct ends on a pointer
/// and is pointer-aligned, so there is no padding between the two halves.
/// Only ever read behind a `dwVersion` check, because before version 7
/// these bytes belong to whatever came next in memory.
#[repr(C)]
struct CredentialAttestationV7 {
    base: WEBAUTHN_CREDENTIAL_ATTESTATION,
    p_hmac_secret: *mut WEBAUTHN_HMAC_SECRET_SALT,
}

/// Extended options: crate struct ends at v7; Chrome uses v8 + `security-key` hint.
#[repr(C)]
struct MakeCredOptionsV8 {
    base: WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS,
    p_prf_global_eval: *mut WEBAUTHN_HMAC_SECRET_SALT,
    c_credential_hints: u32,
    ppwsz_credential_hints: *const PCWSTR,
    b_third_party_payment: BOOL,
}

pub(crate) fn fido_key_present() -> bool {
    webauthn_api_available()
}

pub(crate) fn fido_interface_accessible() -> bool {
    webauthn_api_available()
}

fn webauthn_api_available() -> bool {
    unsafe { WebAuthNGetApiVersionNumber() >= 1 }
}

fn webauthn_api_version() -> u32 {
    unsafe { WebAuthNGetApiVersionNumber() }
}

pub fn probe_device() -> Result<(), FidoError> {
    if !webauthn_api_available() {
        return Err(FidoError::NotAvailable);
    }
    Ok(())
}

pub fn begin_enrollment(
    vault_id: &str,
    key_slot: u8,
    authenticator: Authenticator,
) -> Result<EnrollmentChallenge, FidoError> {
    probe_device()?;
    if authenticator == Authenticator::ThisDevice && !platform_authenticator_available() {
        return Err(FidoError::NotAvailable);
    }
    let mut challenge = [0u8; 32];
    rand::rng().fill_bytes(&mut challenge);
    Ok(EnrollmentChallenge {
        challenge: challenge.to_vec(),
        rp_id: RP_ID.into(),
        user_id: vault_id.to_string(),
        key_slot,
        authenticator,
    })
}

/// Whether Windows Hello is set up and usable as an authenticator. The API
/// answers the right question: not "is there a TPM" but "has the user
/// enrolled a face, fingerprint or PIN". A machine with a TPM and no Hello
/// would fail halfway through the ceremony.
pub(crate) fn platform_authenticator_available() -> bool {
    unsafe { WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable() }
        .map(|available| available.as_bool())
        .unwrap_or(false)
}

pub fn complete_enrollment(challenge: &EnrollmentChallenge) -> Result<Enrollment, FidoError> {
    let hwnd = fido_hwnd()?;
    let client_data_json = create_client_data_json(&challenge.challenge);
    let client_data = build_client_data(&client_data_json);

    let rp_id = to_wide(RP_ID);
    let rp_name = to_wide("SilentSilo");
    let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_RP_ENTITY_INFORMATION_CURRENT_VERSION,
        pwszId: PCWSTR(rp_id.as_ptr()),
        pwszName: PCWSTR(rp_name.as_ptr()),
        pwszIcon: PCWSTR::null(),
    };

    let mut user_id = challenge.user_id.as_bytes().to_vec();
    if user_id.len() > 64 {
        user_id.truncate(64);
    }
    // What a passkey provider files the credential under, and what it syncs.
    // Generic on purpose: the silo's own name would leak to the provider.
    let user_name = to_wide("silo");
    let display_name = to_wide("SilentSilo");
    let user = WEBAUTHN_USER_ENTITY_INFORMATION {
        dwVersion: WEBAUTHN_USER_ENTITY_INFORMATION_CURRENT_VERSION,
        cbId: user_id.len() as u32,
        pbId: user_id.as_mut_ptr(),
        pwszName: PCWSTR(user_name.as_ptr()),
        pwszIcon: PCWSTR::null(),
        pwszDisplayName: PCWSTR(display_name.as_ptr()),
    };

    let mut cred_param = WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
        dwVersion: WEBAUTHN_COSE_CREDENTIAL_PARAMETER_CURRENT_VERSION,
        pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
        lAlg: ES256_ALG,
    };
    let cred_params = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
        cCredentialParameters: 1,
        pCredentialParameters: &mut cred_param,
    };

    let platform = challenge.authenticator == Authenticator::ThisDevice;

    // Two authenticators, two vocabularies. A removable key is asked in CTAP
    // terms, so the `hmac-secret` extension travels down the wire to the key
    // itself. Windows Hello is asked in WebAuthn terms, through `bEnablePrf`
    // below, and it answers `bPrfEnabled = FALSE` when the raw CTAP extension
    // is in the list as well: there is no wire to put it on, and Windows will
    // not quietly drop a request it was given.
    let mut hmac_secret = BOOL::from(true);
    let mut extension = WEBAUTHN_EXTENSION {
        pwszExtensionIdentifier: WEBAUTHN_EXTENSIONS_IDENTIFIER_HMAC_SECRET,
        cbExtension: std::mem::size_of::<BOOL>() as u32,
        pvExtension: &mut hmac_secret as *mut _ as *mut c_void,
    };
    let extensions = WEBAUTHN_EXTENSIONS {
        cExtensions: if platform { 0 } else { 1 },
        pExtensions: if platform {
            ptr::null_mut()
        } else {
            &mut extension
        },
    };

    // Hello reports PRF as enabled only when the ceremony actually evaluated
    // it, so the salt the silo will ask for at unlock has to be present at
    // creation as well. `bEnablePrf` on its own describes an intention;
    // `pPRFGlobalEval` is the question, and the flag below says these 32 bytes
    // are a raw hmac-secret salt rather than a PRF input for Windows to hash.
    // A removable key answers the CTAP extension instead and needs none of it.
    let mut prf_salt = hmac_salt_from_string(&dek_salt_for_vault(&challenge.user_id));
    let mut prf_global = WEBAUTHN_HMAC_SECRET_SALT {
        cbFirst: prf_salt.len() as u32,
        pbFirst: prf_salt.as_mut_ptr(),
        cbSecond: 0,
        pbSecond: ptr::null_mut(),
    };

    // Chromium always passes a non-null exclude list (may be empty).
    let mut exclude_list = WEBAUTHN_CREDENTIAL_LIST {
        cCredentials: 0,
        ppCredentials: ptr::null_mut(),
    };

    let api_ver = webauthn_api_version();
    let use_v8 = api_ver >= API_VERSION_8;

    // Steers the OS picker: without a hint Windows offers phone/hybrid
    // alongside, which is not what either choice means here.
    let hint = to_wide(if platform {
        "client-device"
    } else {
        "security-key"
    });
    let hint_list: [PCWSTR; 1] = [PCWSTR(hint.as_ptr())];

    let make_base = WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS {
        dwVersion: if use_v8 {
            MAKE_CRED_OPTIONS_VERSION_8
        } else {
            WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS_VERSION_7
        },
        dwTimeoutMilliseconds: TIMEOUT_MS,
        CredentialList: WEBAUTHN_CREDENTIALS {
            cCredentials: 0,
            pCredentials: ptr::null_mut(),
        },
        Extensions: extensions,
        dwAuthenticatorAttachment: if platform {
            WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM
        } else {
            WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM
        },
        // Hello keeps the PRF secret with the credential in its own container,
        // so a non-discoverable credential comes back with nothing attached.
        // A removable key stays non-discoverable on purpose: those have a
        // handful of resident slots and the silo already knows the credential
        // id it needs to present.
        bRequireResidentKey: BOOL::from(platform),
        // For a removable key: touch only, avoiding the PIN/passkey UV paths
        // that confuse Win11 + YubiKey. For Hello the opposite — user
        // verification *is* the authentication, and asking for it discouraged
        // would be asking Windows to unlock the vault with no check at all.
        dwUserVerificationRequirement: if platform {
            WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED
        } else {
            WEBAUTHN_USER_VERIFICATION_REQUIREMENT_DISCOURAGED
        },
        dwAttestationConveyancePreference: WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE,
        dwFlags: if platform && use_v8 {
            WEBAUTHN_AUTHENTICATOR_HMAC_SECRET_VALUES_FLAG
        } else {
            0
        },
        pCancellationId: ptr::null_mut(),
        pExcludeCredentialList: &mut exclude_list,
        dwEnterpriseAttestation: 0,
        dwLargeBlobSupport: 0,
        bPreferResidentKey: BOOL::from(platform),
        bBrowserInPrivateMode: BOOL::from(false),
        bEnablePrf: BOOL::from(true),
        pLinkedDevice: ptr::null_mut(),
        cbJsonExt: 0,
        pbJsonExt: ptr::null_mut(),
    };

    let make_v8 = MakeCredOptionsV8 {
        base: make_base,
        p_prf_global_eval: if platform && use_v8 {
            &mut prf_global
        } else {
            ptr::null_mut()
        },
        c_credential_hints: if use_v8 { 1 } else { 0 },
        ppwsz_credential_hints: if use_v8 {
            hint_list.as_ptr()
        } else {
            ptr::null()
        },
        b_third_party_payment: BOOL::from(false),
    };

    let ptr = unsafe {
        // Pass as the crate type; Windows reads by dwVersion.
        let opts_ref: &WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS = &*(&make_v8
            as *const MakeCredOptionsV8
            as *const WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS);
        WebAuthNAuthenticatorMakeCredential(
            hwnd,
            &rp,
            &user,
            &cred_params,
            &client_data,
            Some(opts_ref),
        )
        .map_err(map_enroll_webauthn_err)?
    };
    let _keep = (
        &client_data_json,
        &user_id,
        &user_name,
        &display_name,
        &rp_id,
        &rp_name,
        &hint,
        &hint_list,
        hmac_secret,
        &prf_salt,
        &prf_global,
        make_v8,
    );

    if ptr.is_null() {
        return Err(FidoError::EnrollmentFailed(
            "WebAuthn returned null attestation".into(),
        ));
    }
    let attestation = AttestationGuard(ptr);
    let att = attestation.as_ref();
    let credential_id = copy_bytes(att.pbCredentialId, att.cbCredentialId);
    let auth_data = extract_auth_data_from_attestation(att)?;
    let public_key = extract_public_key_der(&auth_data)?;

    let hmac_ok = (att.dwVersion >= 5 && att.bPrfEnabled.as_bool())
        || read_make_cred_hmac_extension(&att.Extensions);
    if !hmac_ok {
        // The `security-key` hint only reorders the Windows picker, it does not
        // remove "iPhone, iPad, or Android device" from it. A phone whose passkey
        // provider supports hmac-secret over hybrid enrolls fine and never reaches
        // this branch; one that does not hands back a passkey there is nothing to
        // derive a key from. Naming a YubiKey there would be answering a question
        // the person did not ask: the problem is the phone, not their key.
        let via_phone = att.dwVersion >= 3 && att.dwUsedTransport == WEBAUTHN_CTAP_TRANSPORT_HYBRID;
        return Err(FidoError::EnrollmentFailed(if via_phone {
            "This phone's passkey did not provide the hmac-secret extension the silo \
             key is derived from, so it cannot unlock a silo. A phone with a newer \
             passkey provider may work; otherwise plug a FIDO2 key into this computer, \
             or use Windows Hello."
                .into()
        } else if platform {
            "Windows Hello did not return a PRF secret, which is what the silo key is \
             derived from. Hello only offers one from the February 2026 update to Windows 11 \
             25H2 (build 26200.7840). On an older build, use a FIDO2 key instead (YubiKey 5, \
             Nitrokey 3, SoloKeys)."
                .into()
        } else {
            "Security key did not enable hmac-secret. Use a FIDO2 key with hmac-secret \
             (YubiKey 5, Nitrokey 3, SoloKeys, etc.)."
                .into()
        }));
    }

    // Taken only from a platform authenticator, and the reason is safety
    // rather than capability. CTAP keeps two hmac-secret outputs per
    // credential, one for verified assertions and one for unverified, and a
    // removable key is asked for verification `discouraged` at unlock while
    // creation may well have verified. Deriving from the wrong one would
    // wrap the vault key under something no later unlock reproduces, which
    // is a silo nobody can open. Windows Hello verifies on every ceremony,
    // so there is one output there and it is the same one both times.
    let unlock = if platform {
        read_make_cred_hmac_secret(att).map(|hmac| UnlockMaterial {
            wrap_key: blake3_key(&hmac),
            credential_id: credential_id.clone(),
        })
    } else {
        None
    };

    Ok(Enrollment {
        credential: CredentialInfo {
            credential_id,
            public_key,
            key_slot: challenge.key_slot,
            rp_id: challenge.rp_id.clone(),
            authenticator: challenge.authenticator,
        },
        unlock,
    })
}

pub fn derive_unlock_material(
    credential_ids: &[Vec<u8>],
    vault_id: &str,
    on: Option<Authenticator>,
) -> Result<UnlockMaterial, FidoError> {
    if credential_ids.is_empty() {
        return Err(FidoError::UnlockFailed("No enrolled security keys".into()));
    }
    let mut challenge = [0u8; 32];
    rand::rng().fill_bytes(&mut challenge);
    let salt_str = dek_salt_for_vault(vault_id);
    let salt = hmac_salt_from_string(&salt_str);
    let assertion = get_assertion_with_hmac(credential_ids, &challenge, Some(&salt), on)?;
    let att = assertion.as_ref();
    let used_cred = copy_bytes(att.Credential.pbId, att.Credential.cbId);
    let used_cred = if used_cred.is_empty() {
        credential_ids[0].clone()
    } else {
        used_cred
    };
    let hmac = read_assertion_hmac_secret(att).ok_or_else(|| {
        FidoError::UnlockFailed(
            "Security key did not return hmac-secret. Re-enroll your key.".into(),
        )
    })?;
    Ok(UnlockMaterial {
        wrap_key: blake3_key(&hmac),
        credential_id: used_cred,
    })
}

struct AttestationGuard(*mut WEBAUTHN_CREDENTIAL_ATTESTATION);
impl AttestationGuard {
    fn as_ref(&self) -> &WEBAUTHN_CREDENTIAL_ATTESTATION {
        unsafe { &*self.0 }
    }
}
impl Drop for AttestationGuard {
    fn drop(&mut self) {
        unsafe {
            WebAuthNFreeCredentialAttestation(Some(self.0));
        }
    }
}

struct AssertionGuard(*mut windows::Win32::Networking::WindowsWebServices::WEBAUTHN_ASSERTION);
impl AssertionGuard {
    fn as_ref(&self) -> &windows::Win32::Networking::WindowsWebServices::WEBAUTHN_ASSERTION {
        unsafe { &*self.0 }
    }
}
impl Drop for AssertionGuard {
    fn drop(&mut self) {
        unsafe {
            WebAuthNFreeAssertion(self.0);
        }
    }
}

fn get_assertion_with_hmac(
    credential_ids: &[Vec<u8>],
    challenge: &[u8],
    hmac_salt: Option<&[u8; 32]>,
    on: Option<Authenticator>,
) -> Result<AssertionGuard, FidoError> {
    let hwnd = fido_hwnd()?;
    let client_data_json = get_client_data_json(challenge);
    let client_data = build_client_data(&client_data_json);

    let mut cred_id_bufs: Vec<Vec<u8>> = credential_ids.to_vec();
    let mut cred_ex_list: Vec<WEBAUTHN_CREDENTIAL_EX> = cred_id_bufs
        .iter_mut()
        .map(|cred_id| WEBAUTHN_CREDENTIAL_EX {
            dwVersion: WEBAUTHN_CREDENTIAL_EX_CURRENT_VERSION,
            cbId: cred_id.len() as u32,
            pbId: cred_id.as_mut_ptr(),
            pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
            // 0 means "wherever this credential lives". Naming USB here was a
            // filter, not a hint: a Windows Hello credential is `internal` and
            // would never have been offered the assertion, so a silo unlocked
            // by Hello could be enrolled and then never opened again.
            dwTransports: 0,
        })
        .collect();
    let mut cred_ex_ptrs: Vec<*mut WEBAUTHN_CREDENTIAL_EX> =
        cred_ex_list.iter_mut().map(|c| c as *mut _).collect();
    let mut cred_list = WEBAUTHN_CREDENTIAL_LIST {
        cCredentials: cred_ex_ptrs.len() as u32,
        ppCredentials: cred_ex_ptrs.as_mut_ptr(),
    };

    let mut salt_first = hmac_salt.copied().unwrap_or([0u8; 32]);
    let mut hmac_salt_struct = WEBAUTHN_HMAC_SECRET_SALT {
        cbFirst: if hmac_salt.is_some() { 32 } else { 0 },
        pbFirst: if hmac_salt.is_some() {
            salt_first.as_mut_ptr()
        } else {
            ptr::null_mut()
        },
        cbSecond: 0,
        pbSecond: ptr::null_mut(),
    };
    let mut salt_values = WEBAUTHN_HMAC_SECRET_SALT_VALUES {
        pGlobalHmacSalt: if hmac_salt.is_some() {
            &mut hmac_salt_struct
        } else {
            ptr::null_mut()
        },
        cCredWithHmacSecretSaltList: 0,
        pCredWithHmacSecretSaltList: ptr::null_mut(),
    };

    let rp_id = to_wide(RP_ID);
    let opts_version = if hmac_salt.is_some() {
        WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_6
    } else {
        WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_4
    };

    let get_opts = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS {
        dwVersion: opts_version,
        dwTimeoutMilliseconds: TIMEOUT_MS,
        CredentialList: WEBAUTHN_CREDENTIALS {
            cCredentials: 0,
            pCredentials: ptr::null_mut(),
        },
        Extensions: WEBAUTHN_EXTENSIONS {
            cExtensions: 0,
            pExtensions: ptr::null_mut(),
        },
        // Any when unlocking, since the allow-list already pins this to the
        // vault's credentials. Pinned during enrolment, or Windows offers
        // the whole menu again and step two lands on a QR code. Verification
        // stays discouraged either way: a platform authenticator performs it
        // regardless, and asking explicitly drags removable keys into the
        // PIN flow.
        dwAuthenticatorAttachment: match on {
            Some(Authenticator::ThisDevice) => WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM,
            Some(Authenticator::SecurityKey) => WEBAUTHN_AUTHENTICATOR_ATTACHMENT_CROSS_PLATFORM,
            None => WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY,
        },
        dwUserVerificationRequirement: WEBAUTHN_USER_VERIFICATION_REQUIREMENT_DISCOURAGED,
        dwFlags: if hmac_salt.is_some() {
            WEBAUTHN_AUTHENTICATOR_HMAC_SECRET_VALUES_FLAG
        } else {
            0
        },
        pwszU2fAppId: PCWSTR::null(),
        pbU2fAppId: ptr::null_mut(),
        pCancellationId: ptr::null_mut(),
        pAllowCredentialList: &mut cred_list,
        dwCredLargeBlobOperation: 0,
        cbCredLargeBlob: 0,
        pbCredLargeBlob: ptr::null_mut(),
        pHmacSecretSaltValues: if hmac_salt.is_some() {
            &mut salt_values
        } else {
            ptr::null_mut()
        },
        ..Default::default()
    };

    let ptr = unsafe {
        WebAuthNAuthenticatorGetAssertion(
            hwnd,
            PCWSTR(rp_id.as_ptr()),
            &client_data,
            Some(&get_opts),
        )
        .map_err(map_webauthn_err)?
    };
    let _keep = (
        &client_data_json,
        &cred_id_bufs,
        &cred_ex_list,
        &cred_ex_ptrs,
        &rp_id,
        &salt_first,
        &salt_values,
    );
    if ptr.is_null() {
        return Err(FidoError::UnlockFailed(
            "WebAuthn returned null assertion".into(),
        ));
    }
    Ok(AssertionGuard(ptr))
}

fn build_client_data(json: &[u8]) -> WEBAUTHN_CLIENT_DATA {
    WEBAUTHN_CLIENT_DATA {
        dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
        cbClientDataJSON: json.len() as u32,
        pbClientDataJSON: json.as_ptr() as *mut u8,
        pwszHashAlgId: WEBAUTHN_HASH_ALGORITHM_SHA_256,
    }
}

fn fido_hwnd() -> Result<HWND, FidoError> {
    // Prefer the real Tauri window (set at startup). Foreground can be a toast/other UI.
    let stored = PARENT_HWND.load(Ordering::SeqCst);
    if stored != 0 {
        return Ok(HWND(stored as *mut _));
    }
    unsafe {
        let fg = GetForegroundWindow();
        if !fg.0.is_null() {
            return Ok(fg);
        }
        Ok(GetDesktopWindow())
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn copy_bytes(ptr: *const u8, len: u32) -> Vec<u8> {
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    unsafe { slice::from_raw_parts(ptr, len as usize).to_vec() }
}

fn webauthn_error_name(err: &windows::core::Error) -> String {
    unsafe {
        let name = WebAuthNGetErrorName(HRESULT(err.code().0));
        if name.is_null() {
            format!("0x{:08X}", err.code().0 as u32)
        } else {
            name.to_string()
                .unwrap_or_else(|_| format!("0x{:08X}", err.code().0 as u32))
        }
    }
}

fn map_webauthn_err(err: windows::core::Error) -> FidoError {
    map_webauthn_err_impl(err, false)
}
fn map_enroll_webauthn_err(err: windows::core::Error) -> FidoError {
    map_webauthn_err_impl(err, true)
}

fn map_webauthn_err_impl(err: windows::core::Error, enroll: bool) -> FidoError {
    let code = err.code().0 as u32;
    let name = webauthn_error_name(&err);
    let lower = name.to_lowercase();
    let tip = if code == 0x80070057 || lower.contains("invalid") || lower.contains("unknown") {
        " Insert the security key before continuing, close other FIDO apps (e.g. Yubico Authenticator), \
         then try again. Choose Security key (not iPhone/tablet)."
    } else {
        ""
    };
    let text = format!("{name}{tip}");
    if lower.contains("cancel") || code == 0x800704C7 {
        let t = "Security key operation cancelled".to_string();
        return if enroll {
            FidoError::EnrollmentFailed(t)
        } else {
            FidoError::UnlockFailed(t)
        };
    }
    if enroll {
        FidoError::EnrollmentFailed(text)
    } else {
        FidoError::UnlockFailed(text)
    }
}

fn extract_auth_data_from_attestation(
    att: &WEBAUTHN_CREDENTIAL_ATTESTATION,
) -> Result<Vec<u8>, FidoError> {
    let obj = copy_bytes(att.pbAttestationObject, att.cbAttestationObject);
    let value: Value = ciborium::de::from_reader(obj.as_slice())
        .map_err(|e| FidoError::EnrollmentFailed(format!("attestation CBOR: {e}")))?;
    let Value::Map(map) = value else {
        return Err(FidoError::EnrollmentFailed(
            "invalid attestation object".into(),
        ));
    };
    for (k, v) in map {
        if let Value::Text(key) = k
            && key == "authData"
            && let Value::Bytes(bytes) = v
        {
            return Ok(bytes);
        }
    }
    Err(FidoError::EnrollmentFailed(
        "attestation object missing authData".into(),
    ))
}

fn extract_public_key_der(auth_data: &[u8]) -> Result<Vec<u8>, FidoError> {
    if auth_data.len() < 37 {
        return Err(FidoError::EnrollmentFailed("authData too short".into()));
    }
    let flags = auth_data[32];
    if flags & 0x40 == 0 {
        return Err(FidoError::EnrollmentFailed(
            "authData missing attested credential data".into(),
        ));
    }
    let cred_id_len = u16::from_be_bytes([auth_data[53], auth_data[54]]) as usize;
    let cose_start = 55 + cred_id_len;
    if auth_data.len() <= cose_start {
        return Err(FidoError::EnrollmentFailed(
            "authData missing COSE key".into(),
        ));
    }
    cose_ec2_to_spki_der(&auth_data[cose_start..])
}

fn cose_ec2_to_spki_der(cose: &[u8]) -> Result<Vec<u8>, FidoError> {
    let value: Value = ciborium::de::from_reader(cose)
        .map_err(|e| FidoError::EnrollmentFailed(format!("COSE parse: {e}")))?;
    let Value::Map(map) = value else {
        return Err(FidoError::EnrollmentFailed("COSE key not a map".into()));
    };
    let mut x = None;
    let mut y = None;
    for (k, v) in map {
        if let Value::Integer(i) = k {
            match i.into() {
                -2 => x = value_as_bytes(&v),
                -3 => y = value_as_bytes(&v),
                _ => {}
            }
        }
    }
    let (x, y) = match (x, y) {
        (Some(x), Some(y)) if x.len() == 32 && y.len() == 32 => (x, y),
        _ => return Err(FidoError::EnrollmentFailed("invalid EC2 COSE key".into())),
    };
    let prefix = hex::decode("3059301306072a8648ce3d020106082a8648ce3d030107034200")
        .map_err(|e| FidoError::EnrollmentFailed(e.to_string()))?;
    let mut der = prefix;
    der.push(0x04);
    der.extend_from_slice(&x);
    der.extend_from_slice(&y);
    Ok(der)
}

fn value_as_bytes(v: &Value) -> Option<Vec<u8>> {
    match v {
        Value::Bytes(b) => Some(b.clone()),
        _ => None,
    }
}

fn read_make_cred_hmac_extension(ext: &WEBAUTHN_EXTENSIONS) -> bool {
    if ext.pExtensions.is_null() {
        return false;
    }
    for i in 0..ext.cExtensions {
        let e = unsafe { &*ext.pExtensions.add(i as usize) };
        let id = wide_to_string(e.pwszExtensionIdentifier);
        if id == "hmac-secret" && e.cbExtension >= std::mem::size_of::<BOOL>() as u32 {
            let val = unsafe { *(e.pvExtension as *const BOOL) };
            return val.as_bool();
        }
    }
    false
}

/// The PRF output the platform produced while creating the credential, if
/// it produced one.
///
/// Reads a field the `windows` crate does not know about, so the version
/// check is what makes it sound: before version 7 there is no `pHmacSecret`
/// and those bytes are something else entirely.
fn read_make_cred_hmac_secret(att: &WEBAUTHN_CREDENTIAL_ATTESTATION) -> Option<[u8; 32]> {
    if att.dwVersion < CREDENTIAL_ATTESTATION_VERSION_7 {
        return None;
    }
    let extended = unsafe {
        &*(att as *const WEBAUTHN_CREDENTIAL_ATTESTATION as *const CredentialAttestationV7)
    };
    let salt = unsafe { extended.p_hmac_secret.as_ref()? };
    if salt.cbFirst != 32 || salt.pbFirst.is_null() {
        return None;
    }
    copy_bytes(salt.pbFirst, salt.cbFirst).try_into().ok()
}

fn read_assertion_hmac_secret(
    att: &windows::Win32::Networking::WindowsWebServices::WEBAUTHN_ASSERTION,
) -> Option<[u8; 32]> {
    if att.dwVersion < 3 {
        return None;
    }
    let salt = unsafe { att.pHmacSecret.as_ref()? };
    if salt.cbFirst != 32 || salt.pbFirst.is_null() {
        return None;
    }
    let bytes = copy_bytes(salt.pbFirst, salt.cbFirst);
    bytes.try_into().ok()
}

fn wide_to_string(w: PCWSTR) -> String {
    if w.is_null() {
        return String::new();
    }
    unsafe { w.to_string().unwrap_or_default() }
}

fn blake3_key(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn webauthn_api_exists_on_windows() {
        unsafe {
            assert!(WebAuthNGetApiVersionNumber() >= 1);
        }
    }
}
