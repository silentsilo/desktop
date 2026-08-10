//! What a silo can say about itself with no key in hand.
//!
//! Read from the picker, which is also the lock screen. So: what answers
//! "why will this not open", and nothing a stranger could use. No key
//! labels, no storage, no archived folders, nothing about content.

use std::path::Path;

use serde::Serialize;
use silentsilo_vault::VaultPaths;
use tauri::AppHandle;
use uuid::Uuid;

use crate::state::app_data_dir;

/// One file the silo needs, and whether it is there.
#[derive(Debug, Clone, Serialize)]
pub struct FileFact {
    /// Relative to the silo folder, so the report reads the same wherever
    /// the silo lives.
    pub name: String,
    pub present: bool,
    pub bytes: Option<u64>,
    /// Seconds since the epoch, or `None` when the filesystem will not say.
    pub modified: Option<i64>,
}

fn fact(name: &str, path: &Path) -> FileFact {
    match std::fs::metadata(path) {
        Ok(meta) => FileFact {
            name: name.into(),
            present: true,
            bytes: Some(meta.len()),
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64),
        },
        Err(_) => FileFact {
            name: name.into(),
            present: false,
            bytes: None,
            modified: None,
        },
    }
}

/// How many keys open this silo, and of what sort. Counts only: a label
/// would tell a stranger what to go and look for.
#[derive(Debug, Clone, Default, Serialize)]
pub struct KeyCounts {
    pub total: usize,
    /// Sealed to this machine, Windows Hello and the like.
    pub platform: usize,
    /// Carried around, so they open the silo on another machine too.
    pub portable: usize,
    pub revoked: usize,
}

/// The format versions this build writes, so a report from a machine that
/// will not open a silo can be compared against one that does.
#[derive(Debug, Clone, Serialize)]
pub struct Formats {
    pub silo_files: u32,
    pub marker: u32,
    pub index_schema: u32,
    pub sealed_payload: u8,
    pub blob: u16,
    pub recovery_envelope: u32,
}

impl Default for Formats {
    fn default() -> Self {
        Self {
            silo_files: silentsilo_vault::format::SILO_FILE_VERSION,
            marker: silentsilo_vault::registry::MARKER_VERSION,
            index_schema: silentsilo_vfs::SCHEMA_VERSION,
            sealed_payload: silentsilo_crypto::SEAL_VERSION,
            blob: silentsilo_crypto::SSLO_VERSION,
            recovery_envelope: silentsilo_vault::recovery::RECOVERY_ENVELOPE_VERSION,
        }
    }
}

/// Everything the picker will show about one silo.
#[derive(Debug, Clone, Serialize)]
pub struct SiloReport {
    pub app_version: String,
    pub name: String,
    pub path: String,
    /// From the marker. Storage holds it in plain sight anyway, and it is
    /// what tells two silos apart in a support thread.
    pub silo_id: Option<String>,
    /// `None` when the marker is missing or unreadable, which is itself the
    /// answer to several ways a silo fails to open.
    pub marker_version: Option<u32>,
    pub formats: Formats,
    pub files: Vec<FileFact>,
    /// `None` when the enrolled keys cannot be read at all.
    pub keys: Option<KeyCounts>,
    pub recovery_envelope: bool,
    /// A decrypted index left in the machine-local scratch directory. Its
    /// presence means the last session did not close cleanly.
    pub working_copy: Option<FileFact>,
    pub sync_provider: Option<String>,
    pub disk_free_bytes: Option<u64>,
    pub disk_total_bytes: Option<u64>,
}

/// Builds the report by reading the folder. Split from the command so the
/// rule above can be tested against a folder rather than a running app.
pub fn report_for(app_version: String, name: String, root: &Path) -> SiloReport {
    let paths = VaultPaths::new(root.to_path_buf());

    let marker = silentsilo_vault::read_marker(root).ok();
    let keys = silentsilo_vault::load_fido_keys(root).ok().map(|stored| {
        let mut counts = KeyCounts {
            total: stored.keys.len(),
            ..KeyCounts::default()
        };
        for key in &stored.keys {
            if key.revoked {
                counts.revoked += 1;
            }
            if key.platform {
                counts.platform += 1;
            } else {
                counts.portable += 1;
            }
        }
        counts
    });

    let working_copy = paths
        .db_path()
        .exists()
        .then(|| fact("vault.db (working copy)", &paths.db_path()));

    let space = silentsilo_shell::disk_space::space_at(root);

    SiloReport {
        app_version,
        name,
        path: root.display().to_string(),
        silo_id: marker.as_ref().map(|m| m.vault_id.to_string()),
        marker_version: marker.as_ref().map(|m| m.version),
        formats: Formats::default(),
        files: vec![
            fact("vault.db.enc", &paths.db_enc_path()),
            fact("vault.db.enc.bak", &paths.db_enc_backup_path()),
            fact("vault.db.enc.next", &paths.db_enc_staged_path()),
            fact("master.dek.enc", &root.join("master.dek.enc")),
            fact("content.kek.enc", &root.join("content.kek.enc")),
            fact("vault.salt", &paths.salt_path()),
            fact("silo.json", &silentsilo_vault::marker_path(root)),
            fact("keys/fido.json", &root.join("keys").join("fido.json")),
            fact(
                "keys/recovery.json",
                &root.join("keys").join("recovery.json"),
            ),
        ],
        keys,
        recovery_envelope: silentsilo_vault::load_recovery_envelope(root).is_ok(),
        working_copy,
        sync_provider: silentsilo_vault::detect_sync_provider(root).map(str::to_string),
        disk_free_bytes: space.as_ref().map(|s| s.available),
        disk_total_bytes: space.as_ref().map(|s| s.total),
    }
}

/// What the picker's info button reads.
#[tauri::command]
pub fn silo_report(app: AppHandle, id: String) -> Result<SiloReport, String> {
    let silo_id = Uuid::parse_str(&id).map_err(|_| "unknown silo".to_string())?;
    let registry = silentsilo_vault::load_registry(&app_data_dir(&app)?);
    let entry = registry
        .silos
        .iter()
        .find(|s| s.id == silo_id)
        .ok_or("unknown silo")?;

    Ok(report_for(
        app.package_info().version.to_string(),
        entry.name.clone(),
        &entry.path,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use silentsilo_vault::{Authority, StoredFidoCredential, StoredFidoKeys, save_fido_keys};

    fn labelled_key(label: &str, platform: bool, revoked: bool) -> StoredFidoCredential {
        StoredFidoCredential {
            kind: "fido2".into(),
            derivation: "hmac-secret-v1".into(),
            policy: String::new(),
            credential_id: "aa11bb22".into(),
            public_key: "3059".into(),
            key_slot: 0,
            rp_id: "silentsilo.com".into(),
            label: label.into(),
            wrapped_dek: "the-wrapped-dek".into(),
            platform,
            revoked,
        }
    }

    #[test]
    fn nothing_a_stranger_could_use_reaches_the_lock_screen() {
        // The whole reason this module is separate: this is read without
        // unlocking anything.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        save_fido_keys(
            root,
            &StoredFidoKeys {
                keys: vec![labelled_key("YubiKey in the office safe", false, false)],
            },
            Authority::Machine,
        )
        .unwrap();

        let report = report_for("1.0.0".into(), "Personal".into(), root);
        let json = serde_json::to_string(&report).unwrap();

        assert!(!json.contains("office safe"), "a key label leaked: {json}");
        assert!(!json.contains("the-wrapped-dek"), "an envelope leaked");
        assert!(!json.contains("aa11bb22"), "a credential id leaked");
        assert!(!json.contains("hmac-secret"), "a derivation leaked");
    }

    #[test]
    fn keys_are_counted_by_what_they_are() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        save_fido_keys(
            root,
            &StoredFidoKeys {
                keys: vec![
                    labelled_key("hello", true, false),
                    labelled_key("stick", false, false),
                    labelled_key("retired", false, true),
                ],
            },
            Authority::Machine,
        )
        .unwrap();

        let counts = report_for("1.0.0".into(), "Personal".into(), root)
            .keys
            .expect("the keys file was written");

        assert_eq!(counts.total, 3);
        assert_eq!(counts.platform, 1);
        assert_eq!(counts.portable, 2);
        assert_eq!(counts.revoked, 1);
    }

    #[test]
    fn a_missing_file_is_reported_rather_than_hidden() {
        // A silo that will not open is usually missing exactly one of these,
        // so absence is the answer the report exists to give.
        let dir = tempfile::tempdir().unwrap();
        let report = report_for("1.0.0".into(), "Empty".into(), dir.path());

        let dek = report
            .files
            .iter()
            .find(|f| f.name == "master.dek.enc")
            .expect("the wrapped DEK is always listed");
        assert!(!dek.present);
        assert!(dek.bytes.is_none());
        assert!(report.silo_id.is_none(), "no marker, so no id to report");
        assert!(!report.recovery_envelope);
        assert!(report.working_copy.is_none());
    }

    #[test]
    fn a_leftover_working_copy_is_named() {
        // The plaintext index outliving its session means the last run did
        // not close cleanly, which is worth seeing before unlocking.
        let dir = tempfile::tempdir().unwrap();
        let paths = VaultPaths::new(dir.path().to_path_buf());
        paths.ensure_work_dir().unwrap();
        std::fs::write(paths.db_path(), b"a crashed session").unwrap();

        let report = report_for("1.0.0".into(), "Personal".into(), dir.path());

        let copy = report.working_copy.expect("the leftover must be reported");
        assert!(copy.present);
        assert_eq!(copy.bytes, Some(17));
    }
}
