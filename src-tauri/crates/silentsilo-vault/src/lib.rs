//! Local vault provisioning and unlock. `vault.db` is encrypted at rest with
//! AES-256-GCM under the vault's Master DEK (see `vault_file_crypto`) rather
//! than SQLCipher, which needs a C/OpenSSL toolchain that isn't reliably
//! available out of the box on Windows.

/// Whether `seal_under_code_for_fixtures` is compiled into this build.
/// Cargo unifies features across a `--workspace` build, so the app asserts
/// on it at its own crate root: "wrap the DEK under any string you like"
/// has no business in a shipped binary.
pub const FIXTURE_SUPPORT: bool = cfg!(feature = "fixture-support");

pub mod cache_store;
pub mod dek_store;
mod device_store;
mod dpapi;
mod error;
mod fido_store;
pub mod format;
mod kdf;
mod kek_store;
pub mod protected;
pub mod recovery;
pub mod registry;
pub mod rotation;
mod s3_store;
mod session;
mod vault_file_crypto;
pub mod workdir;

pub use cache_store::{
    CacheUsage, DEFAULT_CACHE_LIMIT_BYTES, cache_usage, enforce_cache_limit, get_cache_limit_bytes,
    keep_full_copy, list_local_blob_ids, list_undelivered_blob_ids, list_unsynced_blob_ids,
    record_blob_delivered, record_blob_present, remove_blob_from_cache, set_cache_limit_bytes,
    set_keep_full_copy, settle_blob_delivery, touch_blob_access,
};
pub use dek_store::{
    dek_path, load_dek, save_dek, save_wrapped_dek_bytes, unwrap_dek_hex, wrap_dek_bytes,
};
pub use device_store::{
    LocalVaultAuth, clear_credentials, is_provisioned, load_credentials, save_credentials,
};
pub use error::VaultError;
pub use fido_store::{
    Authority, DERIVATION_HMAC_V1, KEY_SLOT_PRIMARY, KIND_FIDO2, OrgProof, POLICY_ORG,
    StoredFidoCredential, StoredFidoKeys, fido_keys_path, has_backup_key, is_fido_enrolled,
    load_fido_keys, save_fido_keys,
};
pub use kdf::derive_vault_key;
pub use kek_store::{kek_path, load_kek, save_kek, unwrap_kek_bytes, wrap_kek_bytes};
pub use protected::{
    FileStat, PendingImport, ProtectedFolder, ProtectedFolders, load_protected, plan_scan,
    save_protected,
};
#[cfg(feature = "fixture-support")]
pub use recovery::seal_under_code_for_fixtures;
pub use recovery::{
    RecoveryEnvelope, clear_recovery_envelope, create_recovery_envelope, has_recovery_code,
    load_recovery_envelope, save_recovery_envelope, unwrap_with_code,
};
pub use registry::{
    SiloEntry, SiloMarker, SiloRegistry, available_path, folder_name_for, load_registry,
    marker_path, read_marker, registry_path, save_registry, write_marker,
};
pub use s3_store::{BackupTarget, TargetRole, load_targets, save_targets};
pub use s3_store::{clear_s3_config, load_s3_config, save_s3_config};
pub use session::{VaultPaths, VaultSession, wipe_plaintext_working_copy};
pub use workdir::{
    create_private_dir, detect_sync_provider, seal_readonly, wipe_cache_dir, wipe_machine_state,
    wipe_work_dir, work_dir_for,
};

#[cfg(test)]
mod zeroization {
    /// Every type here holds something that opens the vault, and each one is
    /// wiped when it goes out of scope.
    ///
    /// A compile-time check rather than an inspection of freed memory, which
    /// cannot be read without undefined behaviour. Deleting a derive is what
    /// actually happens in practice, during a refactor, and this fails to
    /// build the moment one goes.
    #[test]
    fn what_opens_the_vault_is_wiped_when_dropped() {
        fn wiped_on_drop<T: zeroize::ZeroizeOnDrop>() {}

        // Derived from the device secret or a security key, and enough to
        // unwrap `master.dek.enc` on its own.
        wiped_on_drop::<crate::kdf::VaultKey>();
        // The device secret itself.
        wiped_on_drop::<crate::LocalVaultAuth>();
        // The key everything in the silo is encrypted under.
        wiped_on_drop::<silentsilo_crypto::MasterDek>();
    }
}
