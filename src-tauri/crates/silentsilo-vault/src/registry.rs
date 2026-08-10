//! The list of silos this machine knows about, and only an index:
//! everything that makes a silo a silo lives inside the silo's own
//! directory, so losing this file costs the list, not the data.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dpapi;
use crate::error::VaultError;

const REGISTRY_FILE: &str = "silos.json";
const DPAPI_MAGIC: &[u8] = b"SSDPAPI1";

/// One silo, as this machine knows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiloEntry {
    pub id: Uuid,
    /// What the user calls it. Purely a label — nothing keys off it.
    pub name: String,
    pub path: PathBuf,
    /// Unix seconds, so the picker can lead with the one they actually use.
    #[serde(default)]
    pub last_opened: i64,
    /// How long this silo may sit unused before it locks itself, in
    /// minutes; `None` follows the app-wide default. A preference, not a
    /// secret, so it can live in the plaintext index.
    #[serde(default)]
    pub auto_lock_minutes: Option<u32>,
}

impl SiloEntry {
    /// Whether the silo's files are actually where the registry says.
    ///
    /// An external drive that isn't plugged in, or a folder the user moved,
    /// should show up as unavailable rather than as a silo that fails to
    /// open for reasons nobody can see.
    pub fn is_present(&self) -> bool {
        self.path.join("vault.db.enc").is_file() || self.path.join("vault.db").is_file()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SiloRegistry {
    #[serde(default)]
    pub silos: Vec<SiloEntry>,
    /// The one to open on launch, when there is more than one.
    #[serde(default)]
    pub active: Option<Uuid>,
}

impl SiloRegistry {
    pub fn get(&self, id: Uuid) -> Option<&SiloEntry> {
        self.silos.iter().find(|s| s.id == id)
    }

    /// The silo to open on launch: the one explicitly marked active, else
    /// the most recently opened. Never guesses when the list is empty.
    pub fn preferred(&self) -> Option<&SiloEntry> {
        self.active
            .and_then(|id| self.get(id))
            .or_else(|| self.silos.iter().max_by_key(|s| s.last_opened))
    }

    pub fn upsert(&mut self, entry: SiloEntry) {
        match self.silos.iter_mut().find(|s| s.id == entry.id) {
            Some(existing) => *existing = entry,
            None => self.silos.push(entry),
        }
    }

    pub fn remove(&mut self, id: Uuid) {
        self.silos.retain(|s| s.id != id);
        if self.active == Some(id) {
            self.active = None;
        }
    }

    /// Whether `name` is already taken by a different silo.
    ///
    /// Not a correctness problem — silos are keyed by id — but two entries
    /// called "Work" in the picker is a way to open the wrong one.
    pub fn name_taken(&self, name: &str, except: Option<Uuid>) -> bool {
        let name = name.trim().to_lowercase();
        self.silos
            .iter()
            .any(|s| Some(s.id) != except && s.name.trim().to_lowercase() == name)
    }
}

/// Names the silo a folder holds, in the clear: "which silo is this?"
/// comes up before anything can be unlocked, and a random UUID is all it
/// discloses. Without it, re-adding a folder would depend on a credentials
/// file that normally does not exist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiloMarker {
    pub vault_id: Uuid,
    pub version: u32,
}

const MARKER_FILE: &str = "silo.json";
pub const MARKER_VERSION: u32 = 1;

pub fn marker_path(silo_root: &Path) -> PathBuf {
    silo_root.join(MARKER_FILE)
}

pub fn write_marker(silo_root: &Path, vault_id: Uuid) -> Result<(), VaultError> {
    let marker = SiloMarker {
        vault_id,
        version: MARKER_VERSION,
    };
    let json = serde_json::to_vec_pretty(&marker).map_err(|e| VaultError::Crypto(e.to_string()))?;
    std::fs::create_dir_all(silo_root)?;
    crate::workdir::write_private(&marker_path(silo_root), &json)?;
    Ok(())
}

pub fn read_marker(silo_root: &Path) -> Result<SiloMarker, VaultError> {
    let bytes = std::fs::read(marker_path(silo_root))?;
    let marker: SiloMarker =
        serde_json::from_slice(&bytes).map_err(|e| VaultError::Corrupted(e.to_string()))?;
    if marker.version > MARKER_VERSION {
        return Err(VaultError::Corrupted(
            "this silo was written by a newer version of SilentSilo: update to open it".into(),
        ));
    }
    Ok(marker)
}

pub fn registry_path(app_data: &Path) -> PathBuf {
    app_data.join(REGISTRY_FILE)
}

/// An empty registry when there is no file yet — a fresh install, not an
/// error.
pub fn load_registry(app_data: &Path) -> SiloRegistry {
    let Ok(raw) = std::fs::read(registry_path(app_data)) else {
        return SiloRegistry::default();
    };
    let bytes = match raw.strip_prefix(DPAPI_MAGIC) {
        Some(protected) => match dpapi::unprotect(protected) {
            Some(bytes) => bytes,
            None => return SiloRegistry::default(),
        },
        None => raw,
    };
    crate::format::decode("the silo list", &bytes).unwrap_or_default()
}

/// Names and paths only — but those describe a person, so this gets the
/// same DPAPI treatment as the credentials file rather than sitting in the
/// clear next to it. "Divorce", "Second family" is a filename nobody wants
/// readable by everything that can read their app data.
pub fn save_registry(app_data: &Path, registry: &SiloRegistry) -> Result<(), VaultError> {
    let json = crate::format::encode(registry)?;
    let to_write = match dpapi::protect(&json) {
        Some(protected) => {
            let mut out = DPAPI_MAGIC.to_vec();
            out.extend_from_slice(&protected);
            out
        }
        None => json,
    };
    std::fs::create_dir_all(app_data)?;
    crate::workdir::write_private(&registry_path(app_data), &to_write)?;
    Ok(())
}

/// Turns a silo name into something safe to use as a directory name.
///
/// The user's name for a silo is free text and ends up as a folder, so
/// separators and the Windows reserved characters have to go — otherwise
/// "Work/2026" would silently create a nested directory, and a trailing dot
/// would produce a folder Explorer cannot delete.
pub fn folder_name_for(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').trim();
    // Not merely "is it empty": a name made entirely of separators cleans up
    // to something like "---", which is a legal folder name and a useless
    // one. If nothing recognisable survived, the name did not survive.
    if trimmed.chars().any(|c| c.is_alphanumeric()) {
        trimmed.to_string()
    } else {
        "Silo".to_string()
    }
}

/// A directory under `parent` that no silo occupies yet.
///
/// Creating "Work" twice should not land the second one on top of the
/// first, and silently merging two silos' files would be much worse than a
/// suffix nobody notices.
pub fn available_path(parent: &Path, name: &str) -> PathBuf {
    let base = folder_name_for(name);
    let mut candidate = parent.join(&base);
    let mut n = 2;
    while candidate.exists() {
        candidate = parent.join(format!("{base} {n}"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, last_opened: i64) -> SiloEntry {
        SiloEntry {
            id: Uuid::new_v4(),
            name: name.into(),
            path: PathBuf::from(format!("/silos/{name}")),
            last_opened,
            auto_lock_minutes: None,
        }
    }

    /// Registries written before per-silo auto-lock existed have to keep
    /// loading — the field is a preference, and its absence means "use the
    /// app-wide default", which is exactly what `None` says.
    #[test]
    fn a_registry_written_without_an_auto_lock_setting_still_loads() {
        let json = r#"{"silos":[{"id":"6d3f6f2e-0c1a-4a5e-9c1b-2b1d5f0a7e11",
            "name":"Personal","path":"/silos/personal","last_opened":42}]}"#;
        let registry: SiloRegistry = serde_json::from_str(json).expect("old registries must load");

        assert_eq!(registry.silos.len(), 1);
        assert_eq!(registry.silos[0].auto_lock_minutes, None);
    }

    #[test]
    fn an_empty_registry_prefers_nothing() {
        assert!(SiloRegistry::default().preferred().is_none());
    }

    #[test]
    fn the_active_silo_wins_over_the_most_recent() {
        // Switching silos and then closing the app should not silently put
        // you back in the one you switched away from.
        let recent = entry("Personal", 200);
        let chosen = entry("Work", 100);
        let registry = SiloRegistry {
            active: Some(chosen.id),
            silos: vec![recent, chosen.clone()],
        };
        assert_eq!(registry.preferred().unwrap().id, chosen.id);
    }

    #[test]
    fn without_an_active_silo_the_most_recent_is_used() {
        let old = entry("Personal", 100);
        let recent = entry("Work", 300);
        let registry = SiloRegistry {
            active: None,
            silos: vec![old, recent.clone()],
        };
        assert_eq!(registry.preferred().unwrap().id, recent.id);
    }

    #[test]
    fn an_active_id_that_no_longer_exists_falls_back() {
        // The silo was forgotten but the pointer survived; launching should
        // still open something rather than nothing.
        let remaining = entry("Personal", 100);
        let registry = SiloRegistry {
            active: Some(Uuid::new_v4()),
            silos: vec![remaining.clone()],
        };
        assert_eq!(registry.preferred().unwrap().id, remaining.id);
    }

    #[test]
    fn removing_the_active_silo_clears_the_pointer() {
        let silo = entry("Work", 100);
        let mut registry = SiloRegistry {
            active: Some(silo.id),
            silos: vec![silo.clone()],
        };
        registry.remove(silo.id);
        assert!(registry.silos.is_empty());
        assert!(
            registry.active.is_none(),
            "a dangling active id would point at nothing"
        );
    }

    #[test]
    fn upsert_replaces_rather_than_duplicates() {
        let mut silo = entry("Work", 100);
        let mut registry = SiloRegistry::default();
        registry.upsert(silo.clone());
        silo.name = "Firm".into();
        registry.upsert(silo.clone());

        assert_eq!(registry.silos.len(), 1);
        assert_eq!(registry.silos[0].name, "Firm");
    }

    #[test]
    fn duplicate_names_are_detected_case_insensitively() {
        let silo = entry("Work", 100);
        let registry = SiloRegistry {
            active: None,
            silos: vec![silo.clone()],
        };
        assert!(registry.name_taken("work", None));
        assert!(registry.name_taken("  WORK ", None));
        assert!(
            !registry.name_taken("Work", Some(silo.id)),
            "renaming a silo to what it is already called is not a clash"
        );
        assert!(!registry.name_taken("Personal", None));
    }

    #[test]
    fn a_name_with_separators_cannot_escape_its_parent() {
        assert_eq!(folder_name_for("Work/2026"), "Work-2026");
        assert_eq!(folder_name_for("../etc"), "..-etc");
        assert_eq!(folder_name_for("C:\\Windows"), "C--Windows");
    }

    #[test]
    fn a_trailing_dot_is_stripped() {
        // Windows creates such a folder and then cannot delete it.
        assert_eq!(folder_name_for("Work."), "Work");
        assert_eq!(folder_name_for("Work..."), "Work");
    }

    #[test]
    fn a_name_with_nothing_usable_still_yields_a_folder() {
        assert_eq!(folder_name_for("   "), "Silo");
        assert_eq!(folder_name_for("///"), "Silo");
    }

    #[test]
    fn a_second_silo_of_the_same_name_gets_its_own_directory() {
        // Merging two silos' files into one directory would be far worse
        // than a suffix.
        let tmp = tempfile::tempdir().unwrap();
        let first = available_path(tmp.path(), "Work");
        std::fs::create_dir_all(&first).unwrap();

        let second = available_path(tmp.path(), "Work");
        assert_ne!(first, second);
        assert_eq!(second.file_name().unwrap(), "Work 2");
    }

    #[test]
    fn a_registry_survives_a_save_and_load() {
        let tmp = tempfile::tempdir().unwrap();
        let silo = entry("Personal", 42);
        let registry = SiloRegistry {
            active: Some(silo.id),
            silos: vec![silo.clone()],
        };

        save_registry(tmp.path(), &registry).unwrap();
        let loaded = load_registry(tmp.path());

        assert_eq!(loaded.silos.len(), 1);
        assert_eq!(loaded.silos[0].id, silo.id);
        assert_eq!(loaded.silos[0].name, "Personal");
        assert_eq!(loaded.active, Some(silo.id));
    }

    #[test]
    fn a_missing_registry_reads_as_empty_rather_than_failing() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            load_registry(&tmp.path().join("nothing-here"))
                .silos
                .is_empty()
        );
    }

    #[test]
    fn a_silo_whose_files_are_gone_reports_itself_absent() {
        // An external drive that isn't plugged in, or a folder the user
        // moved. The picker needs to say so rather than offer a silo that
        // fails to open for invisible reasons.
        let tmp = tempfile::tempdir().unwrap();
        let mut silo = entry("Portable", 0);
        silo.path = tmp.path().join("gone");
        assert!(!silo.is_present());

        std::fs::create_dir_all(&silo.path).unwrap();
        std::fs::write(silo.path.join("vault.db.enc"), b"x").unwrap();
        assert!(silo.is_present());
    }

    #[test]
    fn a_folder_identifies_its_own_silo_without_any_secret() {
        // The whole point: adding a folder back has to work on a machine
        // whose keyring holds nothing for it, and normally the credentials
        // file does not exist at all — the keyring took it.
        let tmp = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4();
        write_marker(tmp.path(), id).unwrap();

        assert_eq!(read_marker(tmp.path()).unwrap().vault_id, id);
    }

    #[test]
    fn a_folder_with_no_marker_is_not_a_silo() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_marker(tmp.path()).is_err());
    }

    #[test]
    fn a_marker_from_a_newer_version_is_refused_rather_than_misread() {
        let tmp = tempfile::tempdir().unwrap();
        let future = serde_json::json!({ "vault_id": Uuid::new_v4(), "version": 999 });
        std::fs::write(
            marker_path(tmp.path()),
            serde_json::to_vec(&future).unwrap(),
        )
        .unwrap();

        let err = read_marker(tmp.path()).unwrap_err().to_string();
        assert!(err.contains("newer version"), "got: {err}");
    }
}
