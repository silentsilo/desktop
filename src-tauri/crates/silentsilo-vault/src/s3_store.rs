//! Persistence for the user's S3 connection details.
//!
//! Kept next to `device_store` and using the same keyring-with-DPAPI-fallback
//! discipline, because the secret access key is exactly the kind of thing
//! that must not sit in a plain config file: it grants write access to the
//! user's own bucket.

use std::path::PathBuf;

use keyring::Entry;
use silentsilo_store::StoreConfig;
use uuid::Uuid;

use crate::dpapi;
use crate::error::VaultError;

const KEYRING_SERVICE: &str = "com.silentsilo.desktop";
const KEYRING_USER: &str = "s3-config";
const DPAPI_MAGIC: &[u8] = b"SSDPAPI1";
const S3_CONFIG_FILE: &str = "s3.config.json";

/// Keyed by silo: each one syncs to its own bucket or prefix, and a shared
/// keyring entry would quietly point them all at whichever was configured
/// last. That is the fastest way to have a family silo replaying a work
/// silo's operation log.
fn keyring_entry(silo_id: Uuid) -> Result<Entry, keyring::Error> {
    Entry::new(KEYRING_SERVICE, &format!("{KEYRING_USER}:{silo_id}"))
}

/// Machine-local, for the same reason as the credentials file: this holds the
/// secret access key, the WebDAV password or the SSH private key, which is
/// full write and delete access to the user's backup storage. A silo folder
/// is made to be moved and may sit in a synced directory.
fn s3_config_path(silo_id: Uuid) -> PathBuf {
    crate::workdir::secrets_dir_for(silo_id).join(S3_CONFIG_FILE)
}

/// `None` means sync simply isn't set up, which is a normal state: the app
/// is fully usable without it.
pub fn load_s3_config(silo_id: Uuid) -> Option<StoreConfig> {
    if let Ok(entry) = keyring_entry(silo_id)
        && let Ok(json) = entry.get_password()
        && let Ok(config) =
            crate::format::decode::<StoreConfig>("the storage settings", json.as_bytes())
    {
        return Some(config);
    }

    read_fallback_config(silo_id)
}

/// The keyring-unavailable path, split out so the per-silo separation can be
/// tested without depending on whatever the test machine's keyring does.
fn read_fallback_config(silo_id: Uuid) -> Option<StoreConfig> {
    let raw = std::fs::read(s3_config_path(silo_id)).ok()?;
    let json_bytes = match raw.strip_prefix(DPAPI_MAGIC) {
        Some(protected) => dpapi::unprotect(protected)?,
        None => raw,
    };
    crate::format::decode("the storage settings", &json_bytes).ok()
}

fn write_fallback_config(silo_id: Uuid, config: &StoreConfig) -> Result<(), VaultError> {
    let json = crate::format::encode(config)?;
    let to_write = match dpapi::protect(&json) {
        Some(protected) => {
            let mut out = DPAPI_MAGIC.to_vec();
            out.extend_from_slice(&protected);
            out
        }
        None => json,
    };
    crate::workdir::write_private(&s3_config_path(silo_id), &to_write)?;
    Ok(())
}

pub fn save_s3_config(silo_id: Uuid, config: &StoreConfig) -> Result<(), VaultError> {
    let json = String::from_utf8(crate::format::encode(config)?)
        .map_err(|e| VaultError::Crypto(e.to_string()))?;

    // Same verify-after-write as device credentials: some Windows Credential
    // Manager setups report success without the entry becoming readable.
    if let Ok(entry) = keyring_entry(silo_id)
        && entry.set_password(&json).is_ok()
        && let Ok(verify) = keyring_entry(silo_id)
        && matches!(verify.get_password(), Ok(stored) if stored == json)
    {
        let _ = std::fs::remove_file(s3_config_path(silo_id));
        return Ok(());
    }

    write_fallback_config(silo_id, config)
}

/// Disconnects sync and forgets every target. Best-effort: a missing entry
/// isn't an error. Both hold the same secrets, so both go.
pub fn clear_s3_config(silo_id: Uuid) {
    forget_keyring_entry(|| keyring_entry(silo_id));
    let _ = std::fs::remove_file(s3_config_path(silo_id));
    forget_keyring_entry(|| targets_keyring(silo_id));
    let _ = std::fs::remove_file(targets_path(silo_id));
}

/// Deletes a keyring entry and reads back to confirm it is gone.
///
/// Windows Credential Manager can report a delete as done while the entry
/// stays readable, the mirror of the write problem `save_s3_config` guards
/// against. Here that would leave storage credentials on a machine told to
/// forget the silo, so the delete is retried rather than assumed.
fn forget_keyring_entry(open: impl Fn() -> Result<Entry, keyring::Error>) {
    for _ in 0..5 {
        match open() {
            Ok(entry) => {
                let _ = entry.delete_credential();
            }
            Err(_) => return,
        }
        match open() {
            Ok(entry) => match entry.get_password() {
                Ok(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(_) => return,
            },
            Err(_) => return,
        }
    }
}

// ── More than one target ────────────────────────────────────────────

/// What the app is allowed to do to a target. The distinction is deletion,
/// and it is a promise: an archive target never receives a DELETE, so
/// ransomware holding this machine cannot clear it either. The cost is
/// that it grows for ever, which the panel says rather than hides.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetRole {
    /// Full capabilities. Deletes are issued and expected to work.
    #[default]
    Working,
    /// Append-only. The app never issues a delete here, whether or not the
    /// storage would refuse one.
    Archive,
}

impl TargetRole {
    pub fn allows_delete(self) -> bool {
        matches!(self, Self::Working)
    }
}

/// One place a silo backs up to, with the role it plays.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupTarget {
    pub config: StoreConfig,
    /// Shown to the user, so "the NAS" and "Backblaze" are distinguishable
    /// without reading a bucket name. Empty falls back to what the store
    /// says about itself.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub role: TargetRole,
}

const TARGETS_FILE: &str = "targets.config.json";

fn targets_path(silo_id: Uuid) -> PathBuf {
    crate::workdir::secrets_dir_for(silo_id).join(TARGETS_FILE)
}

fn targets_keyring(silo_id: Uuid) -> Result<Entry, keyring::Error> {
    Entry::new(KEYRING_SERVICE, &format!("{KEYRING_USER}-list:{silo_id}"))
}

/// Every target this silo backs up to, in the order they were added.
///
/// Joining a silo and restoring one from a recovery code both write a single
/// connection and no list, so the single slot is read as a list of one rather
/// than rewritten. Rewriting a working connection to change nothing but its
/// shape is a way to lose it on the machine where the rewrite fails.
pub fn load_targets(silo_id: Uuid) -> Vec<BackupTarget> {
    if let Ok(entry) = targets_keyring(silo_id)
        && let Ok(json) = entry.get_password()
        && let Ok(list) =
            crate::format::decode::<Vec<BackupTarget>>("the backup targets", json.as_bytes())
    {
        return list;
    }

    if let Some(list) = read_fallback_targets(silo_id) {
        return list;
    }

    // A device that has only ever joined or recovered.
    load_s3_config(silo_id)
        .map(|config| {
            vec![BackupTarget {
                config,
                label: String::new(),
                role: TargetRole::Working,
            }]
        })
        .unwrap_or_default()
}

/// Replaces the list.
///
/// The first entry is also written to the single-target slot, so the paths
/// that read a silo's connection without going through the list see a
/// working one rather than none.
pub fn save_targets(silo_id: Uuid, targets: &[BackupTarget]) -> Result<(), VaultError> {
    let json = String::from_utf8(crate::format::encode(&targets.to_vec())?)
        .map_err(|e| VaultError::Crypto(e.to_string()))?;

    let stored_in_keyring = targets_keyring(silo_id)
        .ok()
        .filter(|entry| entry.set_password(&json).is_ok())
        .and_then(|_| targets_keyring(silo_id).ok())
        .is_some_and(|verify| matches!(verify.get_password(), Ok(held) if held == json));

    // The single slot below holds only the first entry, so a real list keeps
    // the file too: a lost keyring entry would otherwise read as one target
    // of several. Best-effort, and removed rather than left stale.
    if stored_in_keyring {
        if targets.len() < 2 || write_fallback_targets(silo_id, json.as_bytes()).is_err() {
            let _ = std::fs::remove_file(targets_path(silo_id));
        }
    } else {
        write_fallback_targets(silo_id, json.as_bytes())?;
    }

    match targets.first() {
        Some(first) => save_s3_config(silo_id, &first.config),
        None => {
            clear_s3_config(silo_id);
            Ok(())
        }
    }
}

/// The keyring-unavailable path, split out for the same reason as the
/// single-target one: so the per-silo separation can be tested without
/// depending on whatever the test machine's keyring does.
fn read_fallback_targets(silo_id: Uuid) -> Option<Vec<BackupTarget>> {
    let raw = std::fs::read(targets_path(silo_id)).ok()?;
    let json_bytes = match raw.strip_prefix(DPAPI_MAGIC) {
        Some(protected) => dpapi::unprotect(protected)?,
        None => raw,
    };
    crate::format::decode("the backup targets", &json_bytes).ok()
}

fn write_fallback_targets(silo_id: Uuid, json: &[u8]) -> Result<(), VaultError> {
    let to_write = match dpapi::protect(json) {
        Some(protected) => {
            let mut out = DPAPI_MAGIC.to_vec();
            out.extend_from_slice(&protected);
            out
        }
        None => json.to_vec(),
    };
    crate::workdir::write_private(&targets_path(silo_id), &to_write)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StoreConfig {
        StoreConfig::S3(silentsilo_core::S3Config {
            endpoint: "https://s3.example.com".into(),
            region: "us-east-1".into(),
            bucket: "my-bucket".into(),
            prefix: "silentsilo".into(),
            access_key_id: "AKIAEXAMPLE".into(),
            secret_access_key: "secret".into(),
            path_style: true,
        })
    }

    /// The S3 details on their own, for the tests that are about how a
    /// prefix becomes a key rather than about how a config is stored.
    fn s3_sample() -> silentsilo_core::S3Config {
        match sample() {
            StoreConfig::S3(c) => c,
            other => panic!("expected an S3 config, got {other:?}"),
        }
    }

    fn bucket_of(config: &StoreConfig) -> &str {
        match config {
            StoreConfig::S3(c) => &c.bucket,
            other => panic!("expected an S3 config, got {other:?}"),
        }
    }

    /// Same reasoning as `device_store`: this file now lives under
    /// `work_base()`, a real directory on the machine running the tests, so
    /// each test takes a random id and cleans up after itself.
    pub(super) struct Scratch(Uuid);

    impl Scratch {
        pub(super) fn new() -> Self {
            Scratch(Uuid::new_v4())
        }
        pub(super) fn id(&self) -> Uuid {
            self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(crate::workdir::secrets_dir_for(self.0));
        }
    }

    #[test]
    fn absent_config_reads_as_none() {
        // A random id nothing has ever written a config for.
        assert!(load_s3_config(Uuid::new_v4()).is_none());
    }

    /// The secret access key is full write and delete access to the user's
    /// backup storage, and the silo folder is made to be carried around and
    /// may sit in a synced directory.
    #[test]
    fn the_config_never_lands_in_the_silo_folder() {
        let scratch = Scratch::new();
        let silo_root = std::path::Path::new(r"C:\Users\alex\OneDrive\Documents\Silo");

        let path = s3_config_path(scratch.id());

        assert!(!path.starts_with(silo_root));
        assert!(path.starts_with(crate::workdir::work_base()));
    }

    #[test]
    fn two_silos_keep_separate_configs_on_disk() {
        // A shared file would point both silos at whichever bucket was
        // configured last, which is how a family silo ends up replaying a
        // work silo's operation log.
        let first = Scratch::new();
        let second = Scratch::new();

        let StoreConfig::S3(mut inner) = sample() else {
            unreachable!()
        };
        inner.bucket = "personal-bucket".into();
        write_fallback_config(first.id(), &StoreConfig::S3(inner.clone())).unwrap();

        inner.bucket = "work-bucket".into();
        write_fallback_config(second.id(), &StoreConfig::S3(inner)).unwrap();

        assert_eq!(
            bucket_of(&read_fallback_config(first.id()).unwrap()),
            "personal-bucket"
        );
        assert_eq!(
            bucket_of(&read_fallback_config(second.id()).unwrap()),
            "work-bucket"
        );
    }

    #[test]
    fn keys_are_built_under_the_prefix() {
        let config = s3_sample();
        assert_eq!(config.key("blobs/abc.sslo"), "silentsilo/blobs/abc.sslo");
        assert_eq!(config.key("/blobs/abc.sslo"), "silentsilo/blobs/abc.sslo");
    }

    #[test]
    fn an_empty_prefix_puts_objects_at_the_bucket_root() {
        let mut config = s3_sample();
        config.prefix = String::new();
        assert_eq!(config.key("blobs/abc.sslo"), "blobs/abc.sslo");
        config.prefix = "/".into();
        assert_eq!(config.key("blobs/abc.sslo"), "blobs/abc.sslo");
    }

    #[test]
    fn surrounding_slashes_in_the_prefix_do_not_double_up() {
        let mut config = s3_sample();
        config.prefix = "/vaults/mine/".into();
        assert_eq!(config.key("blobs/a"), "vaults/mine/blobs/a");
    }
}

#[cfg(test)]
mod target_list_tests {
    use super::tests::Scratch;
    use super::*;

    fn folder(path: &str) -> StoreConfig {
        StoreConfig::Folder {
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn a_silo_that_only_ever_joined_reads_as_a_list_of_one() {
        // Joining and recovering write the single slot and no list, so it is
        // read rather than rewritten: changing nothing but the shape of a
        // working connection is a way to lose it on the machine where the
        // rewrite fails.
        let scratch = Scratch::new();
        write_fallback_config(scratch.id(), &folder("D:/Backups")).unwrap();

        let targets = load_targets(scratch.id());

        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0].config.target_id(),
            folder("D:/Backups").target_id()
        );
    }

    #[test]
    fn a_list_of_several_survives_losing_the_keyring_entry() {
        // The failure this guards: the list lives in one keyring entry and
        // the first target in another. Lose the list alone and the fallback
        // chain lands on the single slot, which answers "one target" for a
        // silo that has three. Eviction then counts one copy where the user
        // configured three, deletions stop reaching the other two, and the
        // next edit saves the shortened list over the real one.
        let scratch = Scratch::new();
        let list = vec![
            BackupTarget {
                config: folder("D:/Backups"),
                label: "Disk".into(),
                role: TargetRole::Working,
            },
            BackupTarget {
                config: folder("E:/Offsite"),
                label: "Offsite".into(),
                role: TargetRole::Working,
            },
        ];
        save_targets(scratch.id(), &list).unwrap();

        // Whatever this machine's keyring did, the file has to be there: it
        // is the only copy that still holds both.
        assert!(
            targets_path(scratch.id()).is_file(),
            "a list of two must keep the file the single slot cannot represent"
        );
        let recovered = read_fallback_targets(scratch.id()).expect("the file still parses");
        assert_eq!(recovered.len(), 2);
        assert_eq!(
            recovered[1].config.target_id(),
            folder("E:/Offsite").target_id()
        );
    }

    #[test]
    fn forgetting_a_silo_takes_its_target_secrets_with_it() {
        // Every target carries full write and delete access to storage: an
        // access key, a WebDAV password or an SSH private key. Removing the
        // silo and its files while leaving those behind keeps that access on
        // a machine that was told to forget it.
        let scratch = Scratch::new();
        save_targets(
            scratch.id(),
            &[
                BackupTarget {
                    config: folder("D:/Backups"),
                    label: "Disk".into(),
                    role: TargetRole::Working,
                },
                BackupTarget {
                    config: folder("E:/Offsite"),
                    label: "Offsite".into(),
                    role: TargetRole::Working,
                },
            ],
        )
        .unwrap();

        clear_s3_config(scratch.id());

        assert!(!targets_path(scratch.id()).exists());
        assert!(
            load_targets(scratch.id()).is_empty(),
            "nothing about the storage may survive forgetting the silo"
        );
    }

    #[test]
    fn a_silo_with_no_storage_has_no_targets() {
        let scratch = Scratch::new();

        assert!(load_targets(scratch.id()).is_empty());
    }

    #[test]
    fn the_fallback_file_round_trips_a_list() {
        // The keyring is whatever the test machine has, so this exercises the
        // path that does not depend on it.
        let scratch = Scratch::new();
        let list = vec![
            BackupTarget {
                config: folder("D:/Backups"),
                label: "The NAS".into(),
                role: TargetRole::Working,
            },
            BackupTarget {
                config: folder("E:/Offsite"),
                label: String::new(),
                role: TargetRole::Archive,
            },
        ];
        let json = crate::format::encode(&list).unwrap();

        write_fallback_targets(scratch.id(), &json).unwrap();
        let back = read_fallback_targets(scratch.id()).unwrap();

        assert_eq!(back.len(), 2);
        assert_eq!(back[0].label, "The NAS");
        // The role travels with the target. Reading it back as Working
        // would mean the app deleting from a place the user asked it never
        // to delete from, which is the one promise this field makes.
        assert!(back[0].role.allows_delete());
        assert!(!back[1].role.allows_delete());
        assert_eq!(back[1].config.target_id(), folder("E:/Offsite").target_id());
    }

    #[test]
    fn two_silos_keep_separate_lists() {
        let first = Scratch::new();
        let second = Scratch::new();

        write_fallback_targets(
            first.id(),
            &crate::format::encode(&vec![BackupTarget {
                config: folder("D:/One"),
                label: String::new(),
                role: TargetRole::Working,
            }])
            .unwrap(),
        )
        .unwrap();

        assert!(read_fallback_targets(second.id()).is_none());
    }
}
