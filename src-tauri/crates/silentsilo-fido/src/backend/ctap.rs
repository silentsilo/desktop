use ctap_hid_fido2::fidokey::FidoKeyHid;
use ctap_hid_fido2::fidokey::get_assertion::Extension as Gext;
use ctap_hid_fido2::fidokey::get_assertion::GetAssertionArgsBuilder;
use ctap_hid_fido2::fidokey::make_credential::Extension as Mext;
use ctap_hid_fido2::fidokey::make_credential::MakeCredentialArgsBuilder;
use ctap_hid_fido2::public_key_credential_user_entity::PublicKeyCredentialUserEntity;
use ctap_hid_fido2::{Cfg, FidoKeyHidFactory, HidParam, get_fidokey_devices, get_hid_devices};
use rand::RngCore;

use crate::client_data::{create_client_data_json, get_client_data_json, hmac_salt_from_string};
use crate::types::{
    Authenticator, CredentialInfo, Enrollment, EnrollmentChallenge, UnlockMaterial,
};
use crate::{FidoError, RP_ID, dek_salt_for_vault};

const KNOWN_FIDO_VIDS: &[u16] = &[0x1050, 0x096E, 0x0483, 0x20A0, 0x32A3, 0x2581];

pub(crate) fn fido_key_present() -> bool {
    if !get_fidokey_devices().is_empty() {
        return true;
    }
    get_hid_devices().iter().any(|d| {
        KNOWN_FIDO_VIDS.contains(&d.vid)
            || d.info.contains("usage_page=61904")
            || matches_known_vid_pid(d.vid, d.pid)
    })
}

fn matches_known_vid_pid(vid: u16, pid: u16) -> bool {
    HidParam::get().iter().any(|param| {
        matches!(param, HidParam::VidPid { vid: known_vid, pid: known_pid } if *known_vid == vid && *known_pid == pid)
    })
}

pub(crate) fn fido_interface_accessible() -> bool {
    if !fido_key_present() {
        return false;
    }
    fido_ctap_works()
}

fn fido_ctap_works() -> bool {
    match open_device() {
        Ok(device) => device.get_info().is_ok(),
        Err(_) => false,
    }
}

fn map_device_error(err: impl std::fmt::Display) -> FidoError {
    FidoError::UnlockFailed(err.to_string())
}

fn map_check_in_error(err: impl std::fmt::Display) -> FidoError {
    FidoError::CheckInFailed(err.to_string())
}

fn map_enroll_error(err: impl std::fmt::Display) -> FidoError {
    FidoError::EnrollmentFailed(err.to_string())
}

fn open_device() -> Result<FidoKeyHid, FidoError> {
    let cfg = Cfg::init();
    if let Ok(device) = FidoKeyHidFactory::create(&cfg) {
        return Ok(device);
    }
    for path in fido_hid_paths() {
        let params = [HidParam::Path(path)];
        if let Ok(device) = FidoKeyHid::new(&params, &cfg) {
            return Ok(device);
        }
    }
    Err(FidoError::UnlockFailed(
        "Could not open the FIDO2 security key. Replug the key and retry.".into(),
    ))
}

fn fido_hid_paths() -> Vec<String> {
    let mut paths = Vec::new();
    for dev in get_fidokey_devices() {
        if let HidParam::Path(path) = &dev.param {
            paths.push(path.clone());
        }
    }
    for d in get_hid_devices() {
        let HidParam::Path(path) = &d.param else {
            continue;
        };
        if path.contains("MI_01") && !path.ends_with("\\KBD") {
            paths.push(path.clone());
        }
        if path.contains("MI_00") {
            let fido = path
                .replace("MI_00", "MI_01")
                .trim_end_matches("\\KBD")
                .to_string();
            paths.push(fido);
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

pub fn probe_device() -> Result<(), FidoError> {
    if !fido_key_present() {
        return Err(FidoError::NoDevice);
    }
    open_device()?;
    Ok(())
}

/// Always false here: this backend speaks CTAP2 over USB HID, which by
/// definition only reaches removable keys. Reaching Touch ID would mean
/// going through the OS, not the wire.
pub(crate) fn platform_authenticator_available() -> bool {
    false
}

/// Nothing to wait for: this backend talks to the key over USB HID and no
/// dialog of the platform's own stands between the two ceremonies.
pub(crate) fn wait_for_ceremony_teardown(_timeout_ms: u64) {}

pub fn begin_enrollment(
    vault_id: &str,
    key_slot: u8,
    authenticator: Authenticator,
) -> Result<EnrollmentChallenge, FidoError> {
    if authenticator == Authenticator::ThisDevice {
        return Err(FidoError::NotAvailable);
    }
    probe_device()?;
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

pub fn complete_enrollment(challenge: &EnrollmentChallenge) -> Result<Enrollment, FidoError> {
    let device = open_device()?;

    // The same pair the Windows backend sends, and just as deliberately
    // generic: the silo's own name would leak to the provider.
    let user = PublicKeyCredentialUserEntity::new(
        Some(challenge.user_id.as_bytes()),
        Some("silo"),
        Some("SilentSilo"),
    );
    let extensions = vec![Mext::HmacSecret(Some(true))];
    let client_data = create_client_data_json(&challenge.challenge);
    let args = MakeCredentialArgsBuilder::new(&challenge.rp_id, &client_data)
        .without_pin_and_uv()
        .user_entity(&user)
        .extensions(&extensions)
        .build();
    let att = device
        .make_credential_with_args(&args)
        .map_err(map_enroll_error)?;

    let hmac_enabled = att
        .extensions
        .iter()
        .any(|ext| matches!(ext, Mext::HmacSecret(Some(true))));
    if !hmac_enabled {
        return Err(FidoError::EnrollmentFailed(
            "Security key did not enable hmac-secret. Use a FIDO2 key with hmac-secret support \
             (e.g. YubiKey 5, Nitrokey 3, SoloKeys)."
                .into(),
        ));
    }

    // CTAP's `hmac-secret` says only that the credential has one; the output
    // itself comes from an assertion, so the second ceremony is not optional
    // here. There is also no dialog of the platform's own to race with.
    Ok(Enrollment {
        credential: CredentialInfo {
            credential_id: att.credential_descriptor.id.clone(),
            public_key: att.credential_publickey.der.clone(),
            key_slot: challenge.key_slot,
            rp_id: challenge.rp_id.clone(),
            authenticator: challenge.authenticator,
        },
        unlock: None,
    })
}

pub fn derive_unlock_material(
    credential_ids: &[Vec<u8>],
    vault_id: &str,
    // CTAP over USB has one attachment by definition, so there is
    // nothing here to pin.
    _on: Option<Authenticator>,
) -> Result<UnlockMaterial, FidoError> {
    if credential_ids.is_empty() {
        return Err(FidoError::UnlockFailed("No enrolled security keys".into()));
    }
    let device = open_device()?;
    let salt = dek_salt_for_vault(vault_id);
    let salt_bytes = hmac_salt_from_string(&salt);
    let extensions = vec![Gext::HmacSecret(Some(salt_bytes))];

    let mut last_err = None;
    for credential_id in credential_ids {
        let mut challenge = [0u8; 32];
        rand::rng().fill_bytes(&mut challenge);
        let client_data = get_client_data_json(&challenge);
        let args = GetAssertionArgsBuilder::new(RP_ID, &client_data)
            .without_pin_and_uv()
            .credential_id(credential_id)
            .extensions(&extensions)
            .build();
        match device.get_assertion_with_args(&args) {
            Ok(assertions) => {
                let assertion = assertions.first().ok_or_else(|| {
                    FidoError::UnlockFailed("Security key returned no assertion".into())
                })?;
                let hmac =
                    extract_hmac_from_extensions(&assertion.extensions).ok_or_else(|| {
                        FidoError::UnlockFailed(
                        "Security key did not return hmac-secret. Enrollment may be incomplete."
                            .into(),
                    )
                    })?;
                return Ok(UnlockMaterial {
                    wrap_key: blake3_key(&hmac),
                    credential_id: credential_id.clone(),
                });
            }
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err
        .map(map_device_error)
        .unwrap_or_else(|| FidoError::UnlockFailed("No matching security key".into())))
}

fn extract_hmac_from_extensions(extensions: &[Gext]) -> Option<[u8; 32]> {
    for ext in extensions {
        if let Gext::HmacSecret(Some(bytes)) = ext {
            return Some(*bytes);
        }
    }
    None
}

fn blake3_key(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_yubikey_via_hid_when_fido_page_hidden() {
        let hid_yubico = get_hid_devices().iter().any(|d| d.vid == 0x1050);
        let fido_enum = !get_fidokey_devices().is_empty();
        if hid_yubico && !fido_enum {
            assert!(fido_key_present());
        }
    }
}
