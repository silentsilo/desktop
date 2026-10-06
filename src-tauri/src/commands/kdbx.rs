//! KeePass databases (`.kdbx`): reading one in, and writing the silo's
//! entries out as one.
//!
//! Only the KeePass side lives here. What an entry becomes is decided in the
//! window (`lib/kdbx.ts`), with the CSV importers' rules: this side hands
//! over what the database says and takes back the entries as they are.
//! Attachments are the exception, because their bytes should not cross the
//! IPC boundary or touch the disk in clear: they are encrypted into the silo
//! as they are read, and decrypted only to be written into the database.

use std::collections::HashMap;
use std::io::Cursor;

use keepass::config::{DatabaseConfig, KdfConfig};
use keepass::db::{Entry, History, Value, fields};
use keepass::{Database, DatabaseKey};
use serde::Serialize;
use serde_json::Value as Json;
use tauri::{AppHandle, Manager};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::commands::fido::run_blocking;
use crate::commands::vault::{
    PasswordAttachment, decrypt_attachment_bytes, encrypt_attachment_bytes, ensure_blobs_local,
};
use crate::state::AppState;

/// The largest database read in. KeePass keeps attachments inside the file,
/// so a big one is possible, but this is read whole into memory.
const MAX_KDBX_BYTES: u64 = 512 * 1024 * 1024;

/// One field the database names beyond the standard five.
#[derive(Serialize, Debug, PartialEq)]
pub struct KdbxField {
    name: String,
    value: String,
    protected: bool,
}

/// What one version of an entry says.
#[derive(Serialize, Debug, PartialEq)]
pub struct KdbxVersion {
    title: String,
    username: String,
    password: String,
    url: String,
    notes: String,
    otp: Option<String>,
    fields: Vec<KdbxField>,
    /// Milliseconds since the epoch, when the database says.
    modified: Option<i64>,
}

#[derive(Serialize)]
pub struct KdbxEntry {
    /// Group names from the top, the root left out.
    group: Vec<String>,
    tags: Vec<String>,
    created: Option<i64>,
    #[serde(flatten)]
    current: KdbxVersion,
    attachments: Vec<PasswordAttachment>,
    /// Earlier versions, newest first.
    history: Vec<KdbxVersion>,
}

fn millis(at: Option<chrono::NaiveDateTime>) -> Option<i64> {
    at.map(|t| t.and_utc().timestamp_millis())
}

fn field(entry: &Entry, name: &str) -> String {
    entry.get(name).unwrap_or_default().to_string()
}

/// KeePass 2.47 keeps a TOTP secret in fields of its own; KeePassXC in one
/// `otp` URI. Either comes back as the URI the importers already read.
const TIME_OTP: [&str; 4] = [
    "TimeOtp-Secret-Base32",
    "TimeOtp-Length",
    "TimeOtp-Period",
    "TimeOtp-Algorithm",
];

fn otp_of(entry: &Entry) -> Option<String> {
    if let Some(uri) = entry.get(fields::OTP).filter(|v| !v.is_empty()) {
        return Some(uri.to_string());
    }
    let secret = entry.get(TIME_OTP[0]).filter(|v| !v.is_empty())?;
    let mut uri = format!("otpauth://totp/?secret={}", secret.replace(' ', ""));
    if let Some(digits) = entry.get(TIME_OTP[1]).filter(|v| !v.is_empty()) {
        uri.push_str(&format!("&digits={digits}"));
    }
    if let Some(period) = entry.get(TIME_OTP[2]).filter(|v| !v.is_empty()) {
        uri.push_str(&format!("&period={period}"));
    }
    if let Some(algorithm) = entry.get(TIME_OTP[3]).filter(|v| !v.is_empty()) {
        // KeePass writes HMAC-SHA-256; the URI form is SHA256.
        let short = algorithm.trim_start_matches("HMAC-").replace('-', "");
        uri.push_str(&format!("&algorithm={short}"));
    }
    Some(uri)
}

fn version_of(entry: &Entry) -> KdbxVersion {
    let mut extra: Vec<KdbxField> = entry
        .fields
        .iter()
        .filter(|(name, _)| {
            !fields::KNOWN_FIELDS.contains(&name.as_str())
                && name.as_str() != fields::OTP
                && !TIME_OTP.contains(&name.as_str())
        })
        .map(|(name, value)| KdbxField {
            name: name.clone(),
            value: value.get().clone(),
            protected: value.is_protected(),
        })
        .collect();
    // A HashMap has no order; the database's own is lost, so at least the
    // same file gives the same entries every time.
    extra.sort_by(|a, b| a.name.cmp(&b.name));
    KdbxVersion {
        title: field(entry, fields::TITLE),
        username: field(entry, fields::USERNAME),
        password: field(entry, fields::PASSWORD),
        url: field(entry, fields::URL),
        notes: field(entry, fields::NOTES),
        otp: otp_of(entry),
        fields: extra,
        modified: millis(entry.times.last_modification),
    }
}

fn open_database(
    bytes: &[u8],
    password: Option<&str>,
    key_file: Option<&str>,
) -> Result<Database, String> {
    let mut key = DatabaseKey::new();
    if let Some(password) = password.filter(|p| !p.is_empty()) {
        key = key.with_password(password);
    }
    if let Some(path) = key_file.filter(|p| !p.is_empty()) {
        let mut file = std::fs::File::open(path)
            .map_err(|e| format!("The key file could not be read: {e}"))?;
        key = key
            .with_keyfile(&mut file)
            .map_err(|e| format!("The key file could not be read: {e}"))?;
    }
    if key.is_empty() {
        return Err("Enter the database's password, or choose its key file.".into());
    }
    Database::open(&mut Cursor::new(bytes), key).map_err(|e| match e {
        keepass::db::DatabaseOpenError::Key(_) => {
            "That password or key file does not open this database.".to_string()
        }
        keepass::db::DatabaseOpenError::UnsupportedVersion => {
            "This KeePass version is not supported. Save it as KDBX 4 in KeePass first.".to_string()
        }
        other => format!("This is not a KeePass database SilentSilo can read: {other}"),
    })
}

/// Every entry of the database at `path`, outside its recycle bin, with its
/// attachments already encrypted into the silo on screen.
#[tauri::command]
pub async fn passwords_read_kdbx(
    app: AppHandle,
    path: String,
    password: Option<String>,
    key_file: Option<String>,
) -> Result<Vec<KdbxEntry>, String> {
    let password = password.map(Zeroizing::new);
    run_blocking(move || {
        let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if size > MAX_KDBX_BYTES {
            return Err("That database is larger than 512 MB.".into());
        }
        let bytes = Zeroizing::new(std::fs::read(&path).map_err(|e| e.to_string())?);
        let db = open_database(
            &bytes,
            password.as_deref().map(|p| p.as_str()),
            key_file.as_deref(),
        )?;
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;

        let bin = db.recycle_bin().map(|g| g.id());
        let mut out = Vec::new();
        for entry in db.iter_all_entries() {
            let mut group = Vec::new();
            let mut at = Some(entry.parent().id());
            let mut binned = false;
            while let Some(id) = at {
                if Some(id) == bin {
                    binned = true;
                    break;
                }
                let Some(g) = db.group(id) else { break };
                let parent = g.parent().map(|p| p.id());
                if parent.is_some() {
                    group.push(g.name.clone());
                }
                at = parent;
            }
            if binned {
                continue;
            }
            group.reverse();

            let mut attachments = Vec::new();
            for (name, attachment) in entry.attachments_named() {
                attachments.push(encrypt_attachment_bytes(
                    &snapshot,
                    name,
                    attachment.data.get(),
                )?);
            }

            let mut history: Vec<KdbxVersion> = entry
                .history
                .as_ref()
                .map(|h| h.get_entries().iter().map(version_of).collect())
                .unwrap_or_default();
            history.sort_by_key(|v| std::cmp::Reverse(v.modified));

            out.push(KdbxEntry {
                group,
                tags: entry.tags.clone(),
                created: millis(entry.times.creation),
                current: version_of(&entry),
                attachments,
                history,
            });
        }
        Ok(out)
    })
    .await
}

/// A string field of an entry's JSON.
fn text<'a>(entry: &'a Json, key: &str) -> &'a str {
    entry.get(key).and_then(Json::as_str).unwrap_or_default()
}

/// The `otp` URI KeePassXC and KeePassDX read, from the entry's TOTP fields.
fn otp_uri(entry: &Json) -> Option<String> {
    let secret = text(entry, "totp_secret");
    if secret.is_empty() {
        return None;
    }
    let mut uri = format!("otpauth://totp/?secret={secret}");
    if let Some(digits) = entry.get("totp_digits").and_then(Json::as_u64) {
        uri.push_str(&format!("&digits={digits}"));
    }
    if let Some(period) = entry.get("totp_period").and_then(Json::as_u64) {
        uri.push_str(&format!("&period={period}"));
    }
    let algorithm = text(entry, "totp_algorithm").replace('-', "");
    if !algorithm.is_empty() {
        uri.push_str(&format!("&algorithm={algorithm}"));
    }
    Some(uri)
}

/// The fields of a card, an identity or an SSH key, as named fields: KeePass
/// has no kinds of entry, only fields. Secrets go in protected.
const KIND_FIELDS: [(&str, &str, bool); 18] = [
    ("card_holder", "Cardholder", false),
    ("card_number", "Card number", true),
    ("card_brand", "Card brand", false),
    ("card_exp_month", "Expiry month", false),
    ("card_exp_year", "Expiry year", false),
    ("card_code", "Security code", true),
    ("id_full_name", "Full name", false),
    ("id_company", "Company", false),
    ("id_email", "Email", false),
    ("id_phone", "Phone", false),
    ("id_address", "Address", false),
    ("id_city", "City", false),
    ("id_state", "State", false),
    ("id_zip", "Postal code", false),
    ("id_country", "Country", false),
    ("ssh_private_key", "SSH private key", true),
    ("ssh_public_key", "SSH public key", false),
    ("ssh_fingerprint", "SSH fingerprint", false),
];

/// One field of `target`. An empty one is left out, except the five every
/// KeePass entry has.
fn put(target: &mut Entry, name: &str, value: &str, protected: bool) {
    if value.is_empty() && !fields::KNOWN_FIELDS.contains(&name) {
        return;
    }
    let value = if protected {
        Value::protected(value.to_string())
    } else {
        Value::unprotected(value.to_string())
    };
    target.fields.insert(name.to_string(), value);
}

/// Writes one version of an entry (the entry itself, or one in its history)
/// into `target`'s fields, replacing what was there.
fn write_fields(target: &mut Entry, version: &Json) {
    target.fields.clear();
    put(target, fields::TITLE, text(version, "service"), false);
    put(target, fields::USERNAME, text(version, "username"), false);
    put(target, fields::PASSWORD, text(version, "password"), true);
    put(target, fields::URL, text(version, "url"), false);
    put(target, fields::NOTES, text(version, "notes"), false);
    if let Some(uri) = otp_uri(version) {
        put(target, fields::OTP, &uri, true);
    }
    for (key, name, protected) in KIND_FIELDS {
        put(target, name, text(version, key), protected);
    }
    // A name the database already uses gets a suffix rather than replacing
    // the standard field: a custom field called "Password" is not the password.
    for custom in version
        .get("fields")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let base = text(custom, "name");
        let base = if base.is_empty() { "Field" } else { base };
        let mut name = base.to_string();
        let mut n = 2;
        while target.fields.contains_key(&name) {
            name = format!("{base} ({n})");
            n += 1;
        }
        let hidden = custom
            .get("hidden")
            .and_then(Json::as_bool)
            .unwrap_or(false);
        put(target, &name, text(custom, "value"), hidden);
    }
}

fn naive(ms: Option<i64>) -> Option<chrono::NaiveDateTime> {
    ms.and_then(chrono::DateTime::from_timestamp_millis)
        .map(|t| t.naive_utc())
}

/// KDBX 4 with Argon2id at 64 MiB, near what KeePassXC chooses: the file
/// leaves the silo, so its own key derivation is all that protects it.
fn export_config() -> DatabaseConfig {
    let mut config = DatabaseConfig::default();
    if let KdfConfig::Argon2 { version, .. } = config.kdf_config {
        config.kdf_config = KdfConfig::Argon2id {
            iterations: 10,
            memory: 64 * 1024 * 1024,
            parallelism: 2,
            version,
        };
    }
    config
}

/// An attached file's content, from its entry's attachment JSON.
type AttachmentBytes<'a> = dyn FnMut(&Json) -> Result<Zeroizing<Vec<u8>>, String> + 'a;

/// Builds the database the silo's entries become, attachments included.
fn build_database(
    entries: &[Json],
    config: DatabaseConfig,
    attachment: &mut AttachmentBytes<'_>,
) -> Result<Database, String> {
    let mut db = Database::with_config(config);
    db.root_mut().name = "SilentSilo".into();
    let root = db.root().id();
    let mut groups: HashMap<String, keepass::db::GroupId> = HashMap::new();

    for entry in entries {
        // "Work / Servers" is the category an import made of nested groups;
        // it goes back out as the same nesting.
        let category = text(entry, "category").trim().to_string();
        let mut parent = root;
        let mut path = String::new();
        for part in category
            .split(" / ")
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            path = if path.is_empty() {
                part.to_string()
            } else {
                format!("{path} / {part}")
            };
            parent = match groups.get(&path) {
                Some(id) => *id,
                None => {
                    let mut above = db
                        .group_mut(parent)
                        .ok_or("A group went missing while it was written.")?;
                    let mut group = above.add_group();
                    group.name = part.to_string();
                    let id = group.id();
                    groups.insert(path.clone(), id);
                    id
                }
            };
        }

        let mut group = db
            .group_mut(parent)
            .ok_or("A group went missing while it was written.")?;
        let mut kp = group.add_entry();

        // History first, while the entry has no attachments: a version holds
        // none, as in the silo. Newest first, so the file ends oldest first,
        // the order KeePass keeps.
        let mut history = History::default();
        for version in entry
            .get("history")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            let mut old: Entry = (*kp).clone();
            old.history = None;
            write_fields(&mut old, version);
            old.times.last_modification = naive(version.get("saved_at").and_then(Json::as_i64));
            history.add_entry(old);
        }

        write_fields(&mut kp, entry);
        kp.times.creation = naive(entry.get("created_at").and_then(Json::as_i64));
        kp.times.last_modification = naive(entry.get("updated_at").and_then(Json::as_i64));
        kp.history = Some(history);
        if entry.get("favorite").and_then(Json::as_bool) == Some(true) {
            kp.tags.push("Favorite".into());
        }

        for att in entry
            .get("attachments")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            let bytes = attachment(att)?;
            let mut name = text(att, "name").to_string();
            if name.is_empty() {
                name = "attachment".into();
            }
            kp.add_attachment(name, Value::unprotected(bytes.to_vec()));
        }
    }
    Ok(db)
}

/// Writes `entries` (the silo's, as the window holds them) to `path` as a
/// KDBX 4 database under `password`.
#[tauri::command]
pub async fn passwords_write_kdbx(
    app: AppHandle,
    path: String,
    password: String,
    entries: String,
) -> Result<(), String> {
    let password = Zeroizing::new(password);
    if password.chars().count() < 8 {
        return Err("Use a password of at least 8 characters for the KeePass file.".into());
    }
    let entries: Vec<Json> = serde_json::from_str(&entries).map_err(|e| e.to_string())?;

    // Every attached file has to be here before it can be written out.
    let blob_ids: Vec<Uuid> = entries
        .iter()
        .flat_map(|e| {
            e.get("attachments")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|a| {
            a.get("blob_id")
                .and_then(Json::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
        })
        .collect();
    ensure_blobs_local(&app, &blob_ids).await?;

    run_blocking(move || {
        let snapshot = crate::state::snapshot_focused_session(&app.state::<AppState>())?;
        crate::audit::record_in(
            &app,
            snapshot.id,
            crate::audit::event(crate::audit::codes::PASSWORDS_EXPORTED)
                .with("format", "kdbx")
                .with("count", entries.len()),
        )?;
        let db = build_database(&entries, export_config(), &mut |att| {
            decrypt_attachment_bytes(&snapshot, text(att, "blob_id"), text(att, "blob_key"))
        })?;
        let mut out = Zeroizing::new(Vec::new());
        db.save(&mut *out, DatabaseKey::new().with_password(&password))
            .map_err(|e| format!("The KeePass file could not be written: {e}"))?;
        std::fs::write(&path, &*out).map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entries() -> Vec<Json> {
        vec![
            json!({
                "id": "e1", "service": "Bank", "username": "ana", "password": "now",
                "url": "https://bank.example", "notes": "a note", "category": "Money / Main",
                "created_at": 1_700_000_000_000i64, "updated_at": 1_700_000_500_000i64,
                "favorite": true, "totp_secret": "JBSWY3DPEHPK3PXP", "totp_digits": 8,
                "fields": [
                    { "name": "Customer number", "value": "40021", "hidden": false },
                    { "name": "Password", "value": "not the password", "hidden": true }
                ],
                "history": [
                    { "saved_at": 1_700_000_300_000i64, "service": "Bank", "username": "ana", "password": "before" },
                    { "saved_at": 1_700_000_100_000i64, "service": "Bank", "username": "ana", "password": "first" }
                ],
                "attachments": [{ "blob_id": "b1", "name": "codes.txt", "blob_key": "k" }]
            }),
            json!({
                "id": "e2", "type": "card", "service": "Visa", "username": "", "password": "",
                "url": "", "notes": "", "category": "", "created_at": 0, "updated_at": 0,
                "card_number": "4111111111111111", "card_code": "123"
            }),
        ]
    }

    /// Cheap to derive: a debug build runs Argon2 slowly.
    fn test_config() -> DatabaseConfig {
        let mut config = export_config();
        if let KdfConfig::Argon2id { version, .. } = config.kdf_config {
            config.kdf_config = KdfConfig::Argon2id {
                iterations: 1,
                memory: 1024 * 1024,
                parallelism: 1,
                version,
            };
        }
        config
    }

    fn round_trip() -> Database {
        let db = build_database(&entries(), test_config(), &mut |_| {
            Ok(Zeroizing::new(b"a file".to_vec()))
        })
        .unwrap();
        let mut bytes = Vec::new();
        db.save(
            &mut bytes,
            DatabaseKey::new().with_password("correct horse"),
        )
        .unwrap();
        open_database(&bytes, Some("correct horse"), None).unwrap()
    }

    #[test]
    fn what_goes_out_comes_back_in() {
        let db = round_trip();
        let bank = db
            .iter_all_entries()
            .find(|e| e.get_title() == Some("Bank"))
            .unwrap();
        let read = version_of(&bank);
        assert_eq!(read.password, "now");
        assert_eq!(
            read.otp.as_deref(),
            Some("otpauth://totp/?secret=JBSWY3DPEHPK3PXP&digits=8")
        );
        assert!(read.fields.contains(&KdbxField {
            name: "Customer number".into(),
            value: "40021".into(),
            protected: false
        }));
        // The custom field called Password did not replace the password.
        assert!(
            read.fields
                .iter()
                .any(|f| f.name == "Password (2)" && f.protected)
        );
        assert_eq!(bank.parent().name, "Main");
        assert_eq!(bank.parent().parent().unwrap().name, "Money");
        assert!(bank.tags.contains(&"Favorite".to_string()));
        let (name, attachment) = bank.attachments_named().next().unwrap();
        assert_eq!(name, "codes.txt");
        assert_eq!(attachment.data.get(), b"a file");

        let history: Vec<String> = bank
            .history
            .as_ref()
            .unwrap()
            .get_entries()
            .iter()
            .map(|e| field(e, fields::PASSWORD))
            .collect();
        assert_eq!(history, vec!["first", "before"], "oldest first in the file");
        // Read back newest first, as the silo keeps it, and with no files.
        for (i, older) in [1, 0].into_iter().enumerate() {
            let entry = bank.historical(older).unwrap();
            assert_eq!(entry.attachments().count(), 0, "version {i}");
        }
    }

    #[test]
    fn the_export_uses_argon2id() {
        assert!(matches!(
            export_config().kdf_config,
            KdfConfig::Argon2id { memory, .. } if memory == 64 * 1024 * 1024
        ));
    }

    #[test]
    fn a_card_goes_out_as_named_fields_with_its_secrets_protected() {
        let db = round_trip();
        let visa = db
            .iter_all_entries()
            .find(|e| e.get_title() == Some("Visa"))
            .unwrap();
        let number = visa.fields.get("Card number").unwrap();
        assert!(number.is_protected());
        assert_eq!(number.get(), "4111111111111111");
        assert!(visa.fields.get("Security code").unwrap().is_protected());
    }

    #[test]
    fn a_wrong_password_says_so() {
        let db = build_database(&entries(), test_config(), &mut |_| {
            Ok(Zeroizing::new(Vec::new()))
        })
        .unwrap();
        let mut bytes = Vec::new();
        db.save(
            &mut bytes,
            DatabaseKey::new().with_password("correct horse"),
        )
        .unwrap();
        let err = open_database(&bytes, Some("wrong"), None).unwrap_err();
        assert!(err.contains("does not open"), "{err}");
    }
}
