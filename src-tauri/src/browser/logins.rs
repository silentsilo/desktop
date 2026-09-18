//! The browser extension's whole view of a silo: login entries, and of each
//! only its label, username, saved address and, once a fill is confirmed,
//! its password.
//!
//! This is the one place the extension's requests reach the vault, and it
//! makes exactly one call there, `list_passwords`. Files, folders, notes,
//! one-time codes, passkeys, attachments and protected folders are not read
//! by this module and so cannot appear in an answer; the tests at the end
//! hold the rest of `browser/` to never reaching the vault another way.
//!
//! `list_passwords` decrypts every entry, secrets included. Listing keeps
//! the metadata and wipes the rest at once; the one password a fill needs is
//! read again only after the person confirmed. A listing that never
//! decrypts the secrets belongs in core (a follow-up, see ARCHITECTURE.md).

use silentsilo_vault::VaultSession;
use silentsilo_vfs::Vfs;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

/// One login entry, reduced to what a list and a confirmation show. No
/// password: see [`secret`].
pub struct Login {
    pub id: Uuid,
    pub label: String,
    pub username: String,
    /// The address it was saved for, free text as the entry keeps it.
    pub url: String,
}

impl Drop for Login {
    fn drop(&mut self) {
        self.username.zeroize();
    }
}

/// What a confirmed fill sends, wiped when dropped.
pub struct Secret {
    pub username: Zeroizing<String>,
    pub password: Zeroizing<String>,
}

/// The row as stored, read field by field. Everything else in it, notes and
/// codes included, is skipped by the parser rather than copied.
#[derive(serde::Deserialize, Default, Zeroize)]
#[serde(default)]
struct Row {
    id: String,
    #[serde(rename = "type")]
    kind: Option<String>,
    service: String,
    username: String,
    password: String,
    url: String,
}

impl Row {
    /// A login with a password to fill. An entry with no `type` is a
    /// login: that is what every entry was before the others.
    fn fillable(&self) -> bool {
        self.kind.as_deref().is_none_or(|kind| kind == "login") && !self.password.is_empty()
    }
}

/// Every entry in the open silo, parsed, each wiped when dropped. The raw
/// JSON is wiped before this returns.
fn rows(session: &VaultSession) -> Result<Vec<Zeroizing<Row>>, String> {
    let mut raw = Vfs::new(session)
        .list_passwords()
        .map_err(|e| e.to_string())?;
    let rows = raw
        .iter()
        .filter_map(|json| serde_json::from_str::<Row>(json).ok())
        .map(Zeroizing::new)
        .collect();
    for json in &mut raw {
        json.zeroize();
    }
    Ok(rows)
}

/// Every login in the open silo that has a password to fill, without the
/// password.
pub fn read(session: &VaultSession) -> Result<Vec<Login>, String> {
    Ok(rows(session)?
        .iter_mut()
        .filter(|row| row.fillable())
        .filter_map(|row| {
            Some(Login {
                id: Uuid::parse_str(&row.id).ok()?,
                label: std::mem::take(&mut row.service),
                username: std::mem::take(&mut row.username),
                url: std::mem::take(&mut row.url),
            })
        })
        .collect())
}

/// The username and password of one login, read fresh. `None` when it is
/// gone or no longer a login with a password.
pub fn secret(session: &VaultSession, id: Uuid) -> Result<Option<Secret>, String> {
    Ok(rows(session)?
        .iter_mut()
        .find(|row| Uuid::parse_str(&row.id).ok() == Some(id))
        .filter(|row| row.fillable())
        .map(|row| Secret {
            username: Zeroizing::new(std::mem::take(&mut row.username)),
            password: Zeroizing::new(std::mem::take(&mut row.password)),
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::protocol::{self, Item, parse_origin};

    /// The code before its tests, which is what the checks below read.
    fn code(source: &str) -> &str {
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    /// Nothing in `browser/` but this module may hold the vault's file API,
    /// and this module may call nothing on it but `list_passwords`.
    #[test]
    fn only_this_module_reaches_the_vault_and_only_for_logins() {
        let here = code(include_str!("logins.rs"));
        assert_eq!(here.matches("Vfs::new(").count(), 1);
        let call = here.split("Vfs::new(session)").nth(1).unwrap();
        assert!(
            call.trim_start().starts_with(".list_passwords()"),
            "the one vault call must be list_passwords"
        );

        for (name, source) in [
            ("mod.rs", include_str!("mod.rs")),
            ("protocol.rs", include_str!("protocol.rs")),
        ] {
            let source = code(source);
            for forbidden in [
                "Vfs",
                "silentsilo_vfs",
                "with_vfs",
                "with_session_id",
                "snapshot_focused_session",
                "list_folder",
                "search_files",
                "vault_search",
                "export",
                "attachment",
                "protected_folders",
                "blob",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "browser/{name} mentions {forbidden}: vault access belongs in logins.rs"
                );
            }
        }
    }

    struct Silo {
        _dir: tempfile::TempDir,
        session: VaultSession,
    }

    fn silo() -> Silo {
        let dir = tempfile::tempdir().unwrap();
        let session =
            VaultSession::provision(dir.path().join("silo"), Uuid::new_v4(), "s").unwrap();
        let vfs = Vfs::new(&session);
        vfs.ensure_initialized().unwrap();
        let root = vfs.root_folder_id().unwrap();
        let taxes = vfs.create_folder(root, "Taxes 2025").unwrap();
        vfs.add_file(
            taxes.id,
            "tax-return-2025.pdf",
            Uuid::new_v4(),
            5,
            "00",
            None,
            "key",
        )
        .unwrap();
        let entries = [
            serde_json::json!({
                "id": Uuid::new_v4().to_string(), "service": "GitHub",
                "username": "alex@example.com", "password": "gh-secret",
                "url": "https://github.com/login",
                "notes": "recovery codes in tax-return-2025.pdf",
                "totp_secret": "JBSWY3DPEHPK3PXP",
                "attachments": [{ "blob_id": Uuid::new_v4().to_string(),
                    "name": "tax-return-2025.pdf", "size_bytes": 5, "blob_key": "k" }],
            }),
            serde_json::json!({
                "id": Uuid::new_v4().to_string(), "type": "login", "service": "Bank",
                "username": "alex", "password": "bank-secret", "url": "bank.example",
            }),
            serde_json::json!({
                "id": Uuid::new_v4().to_string(), "type": "note",
                "service": "Taxes 2025 note", "username": "", "password": "",
                "url": "", "notes": "tax-return-2025.pdf is in Taxes 2025",
            }),
            serde_json::json!({
                "id": Uuid::new_v4().to_string(), "type": "card", "service": "Visa",
                "username": "", "password": "", "url": "", "card_number": "4111111111111111",
            }),
            serde_json::json!({
                "id": Uuid::new_v4().to_string(), "service": "Passkey only",
                "username": "alex", "password": "", "url": "https://github.com",
            }),
        ];
        for entry in entries {
            let id = Uuid::parse_str(entry["id"].as_str().unwrap()).unwrap();
            vfs.upsert_password(id, &entry.to_string()).unwrap();
        }
        Silo { _dir: dir, session }
    }

    fn answer_text(found: &[&Login], kind: &'static str) -> String {
        let mut refs = protocol::RefTable::default();
        let items: Vec<Item> = found
            .iter()
            .map(|login| Item {
                reference: refs.issue(Uuid::nil(), 0, login.id),
                label: &login.label,
                username: &login.username,
                site: None,
            })
            .collect();
        String::from_utf8(protocol::list_answer("1", kind, None, &items)).unwrap()
    }

    #[test]
    fn only_logins_with_a_password_are_read() {
        let silo = silo();
        let logins = read(&silo.session).unwrap();
        let mut labels: Vec<&str> = logins.iter().map(|l| l.label.as_str()).collect();
        labels.sort();
        assert_eq!(labels, ["Bank", "GitHub"]);
        let github = logins.iter().find(|l| l.label == "GitHub").unwrap();
        assert_eq!(github.url, "https://github.com/login");

        let secret = secret(&silo.session, github.id).unwrap().unwrap();
        assert_eq!(secret.password.as_str(), "gh-secret");
        assert_eq!(secret.username.as_str(), "alex@example.com");
        assert!(
            super::secret(&silo.session, Uuid::new_v4())
                .unwrap()
                .is_none()
        );
    }

    /// A note or a passkey-only entry has no password to hand out, even by
    /// its id.
    #[test]
    fn only_a_login_with_a_password_has_a_secret() {
        let silo = silo();
        let rows = rows(&silo.session).unwrap();
        for row in rows.iter().filter(|row| !row.fillable()) {
            let id = Uuid::parse_str(&row.id).unwrap();
            assert!(
                secret(&silo.session, id).unwrap().is_none(),
                "{}",
                row.service
            );
        }
        assert!(
            rows.iter().any(|row| !row.fillable()),
            "the silo has such entries"
        );
    }

    #[test]
    fn a_file_name_finds_nothing_and_no_answer_names_a_file() {
        let silo = silo();
        let logins = read(&silo.session).unwrap();

        for query in [
            "tax-return-2025.pdf",
            "tax-return",
            "Taxes 2025",
            "JBSWY3DP",
            "4111",
        ] {
            assert!(
                protocol::search(&logins, query).is_empty(),
                "search for {query:?} found something"
            );
        }

        let github = parse_origin("https://github.com").unwrap().unwrap();
        let mut answers = vec![
            answer_text(&protocol::logins_for(&logins, &github), "logins"),
            answer_text(&protocol::search(&logins, "al"), "search"),
            answer_text(&protocol::search(&logins, "ex"), "search"),
        ];
        for login in &logins {
            let secret = secret(&silo.session, login.id).unwrap().unwrap();
            answers.push(
                String::from_utf8(protocol::fill_answer(
                    "4",
                    &secret.username,
                    &secret.password,
                ))
                .unwrap(),
            );
        }
        assert!(
            answers.iter().any(|a| a.contains("GitHub")),
            "the check itself sees answers"
        );
        for answer in &answers {
            for secret_of_the_silo in [
                "tax-return",
                "Taxes 2025",
                "recovery codes",
                "JBSWY3DP",
                "4111",
                "Visa",
            ] {
                assert!(
                    !answer.contains(secret_of_the_silo),
                    "an answer carries {secret_of_the_silo:?}: {answer}"
                );
            }
        }
    }

    #[test]
    fn logins_for_a_site_are_the_ones_saved_for_it() {
        let silo = silo();
        let logins = read(&silo.session).unwrap();
        let bank = parse_origin("https://www.bank.example").unwrap().unwrap();
        let found: Vec<&str> = protocol::logins_for(&logins, &bank)
            .iter()
            .map(|l| l.label.as_str())
            .collect();
        assert_eq!(found, ["Bank"]);
        let elsewhere = parse_origin("https://bank-login.example").unwrap().unwrap();
        assert!(protocol::logins_for(&logins, &elsewhere).is_empty());
    }
}
