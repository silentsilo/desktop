use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rusqlite::{Connection, OptionalExtension, params};
use silentsilo_core::{
    CoreError, CoreResult, DeviceInfo, FileEntry, FolderEntry, SearchHit, TrashItem, VaultEntry,
    VaultMeta,
};
use silentsilo_vault::VaultSession;
use uuid::Uuid;

use crate::schema::{bump_revision, init_schema};

/// Rejects anything that isn't a plain path segment: empty, `.`/`..`
/// (traversal), or containing a path separator (`/` or `\`, since the
/// desktop app also runs on Windows). Without this, a name like `".."`
/// stored in the vault index can walk `Path::join` outside the directory
/// the user chose when exporting a folder.
/// What a name typed here has to satisfy.
///
/// Stricter than what replay accepts, deliberately: a record from another
/// device gets repaired by `names::sanitize`, because it cannot be refused,
/// while a name the user just typed should come back as an error they can
/// act on rather than be silently stored as something else.
fn validate_entry_name(name: &str) -> CoreResult<()> {
    if name == "." || name == ".." {
        return Err(CoreError::InvalidName(format!("{name} is not a name")));
    }
    crate::names::check(name)
}

/// A typed or dropped name, as it is compared and emitted: trimmed and
/// NFC-composed, so the decomposed spelling a macOS drop hands over matches
/// the row replay will store rather than reading as a different name.
fn normalized_input(name: &str) -> CoreResult<String> {
    let name = crate::names::compose(name.trim());
    validate_entry_name(&name)?;
    Ok(name)
}

pub struct Vfs<'a> {
    session: &'a VaultSession,
}

/// One password attachment's content, as the entry JSON records it.
#[derive(Debug, Clone)]
pub struct AttachmentBlob {
    pub blob_id: Uuid,
    pub size_bytes: i64,
    /// Wrapped under the content KEK, like a file's.
    pub blob_key: String,
}

impl<'a> Vfs<'a> {
    pub fn new(session: &'a VaultSession) -> Self {
        Self { session }
    }

    fn conn(&self) -> &Connection {
        &self.session.conn
    }

    pub fn ensure_initialized(&self) -> CoreResult<()> {
        init_schema(self.conn(), self.session.vault_id)
    }

    pub fn meta(&self) -> CoreResult<VaultMeta> {
        let revision: i64 = self
            .conn()
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM vault_meta WHERE key = 'revision'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(VaultMeta {
            revision,
            vault_id: self.session.vault_id,
        })
    }

    pub fn root_folder_id(&self) -> CoreResult<Uuid> {
        let id: String = self
            .conn()
            .query_row(
                "SELECT id FROM folders WHERE path = '/' AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|_| CoreError::NotFound("root folder".into()))?;
        Uuid::parse_str(&id).map_err(|_| CoreError::Database("invalid root id".into()))
    }

    pub fn list_folder(&self, folder_id: Uuid) -> CoreResult<Vec<VaultEntry>> {
        let fid = folder_id.to_string();
        let mut entries = Vec::new();

        let mut folder_stmt = self
            .conn()
            .prepare(
                "SELECT id, parent_id, name, path, created_at, updated_at, favorite
                 FROM folders
                 WHERE parent_id = ?1 AND deleted_at IS NULL
                 ORDER BY name COLLATE NOCASE",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let folders = folder_stmt
            .query_map([&fid], |row| {
                Ok(FolderEntry {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    parent_id: row
                        .get::<_, Option<String>>(1)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    name: row.get(2)?,
                    path: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    favorite: row.get::<_, i64>(6)? != 0,
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        for f in folders {
            entries.push(VaultEntry::Folder(
                f.map_err(|e| CoreError::Database(e.to_string()))?,
            ));
        }

        let mut file_stmt = self
            .conn()
            .prepare(
                "SELECT id, folder_id, name, blob_id, size_bytes, mime_type, content_hash, created_at, updated_at, favorite
                 FROM files
                 WHERE folder_id = ?1 AND deleted_at IS NULL
                 ORDER BY name COLLATE NOCASE",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let files = file_stmt
            .query_map([&fid], |row| {
                Ok(FileEntry {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    folder_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                    name: row.get(2)?,
                    blob_id: Uuid::parse_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                    size_bytes: row.get(4)?,
                    mime_type: row.get(5)?,
                    content_hash: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    favorite: row.get::<_, i64>(9)? != 0,
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        for f in files {
            entries.push(VaultEntry::File(
                f.map_err(|e| CoreError::Database(e.to_string()))?,
            ));
        }

        Ok(entries)
    }

    pub fn create_folder(&self, parent_id: Uuid, name: &str) -> CoreResult<FolderEntry> {
        let name = &normalized_input(name)?;
        // Fail early if the parent is gone, rather than emitting an
        // operation that every device would just discard as obsolete.
        let _parent = self.get_folder(parent_id)?;

        // A clash the user can still see and fix gets an error, not a quiet
        // "Docs (2)". Compared folded rather than by path, because that is
        // how replay decides: "docs" next to "Docs" would pass an exact
        // check and come back suffixed anyway.
        let mut stmt = self
            .conn()
            .prepare("SELECT name FROM folders WHERE parent_id = ?1 AND deleted_at IS NULL")
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let siblings = stmt
            .query_map([parent_id.to_string()], |row| row.get::<_, String>(0))
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let wanted = crate::names::fold(name);
        for sibling in siblings {
            let sibling = sibling.map_err(|e| CoreError::Database(e.to_string()))?;
            if crate::names::fold(&sibling) == wanted {
                return Err(CoreError::NameConflict(name.to_string()));
            }
        }
        drop(stmt);

        let id = Uuid::now_v7();
        let (_record, _outcome) = crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::CreateFolder {
                id,
                parent_id,
                name: name.to_string(),
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        self.get_folder(id)
    }

    /// The folder as [`Self::create_folder`] would make it, or the live one
    /// already holding that name: what dropping a folder onto an existing
    /// tree means. Only a folder satisfies the name; a clash with a file
    /// still comes back as the conflict it is.
    pub fn create_or_get_folder(&self, parent_id: Uuid, name: &str) -> CoreResult<FolderEntry> {
        match self.create_folder(parent_id, name) {
            Err(CoreError::NameConflict(taken)) => {
                let wanted = crate::names::fold(&taken);
                let mut stmt = self
                    .conn()
                    .prepare(
                        "SELECT id, name FROM folders
                         WHERE parent_id = ?1 AND deleted_at IS NULL",
                    )
                    .map_err(|e| CoreError::Database(e.to_string()))?;
                let siblings = stmt
                    .query_map([parent_id.to_string()], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(|e| CoreError::Database(e.to_string()))?;
                for sibling in siblings {
                    let (id, sibling_name) =
                        sibling.map_err(|e| CoreError::Database(e.to_string()))?;
                    if crate::names::fold(&sibling_name) == wanted {
                        return self.get_folder(parse_id(&id)?);
                    }
                }
                Err(CoreError::NameConflict(taken))
            }
            other => other,
        }
    }

    /// One file's content key, still wrapped. A lookup rather than a field
    /// on `FileEntry`: that type is serialised to the interface, and key
    /// material has no business crossing into a webview even wrapped. An
    /// empty answer means the row predates a content key or the file is
    /// gone; callers report that rather than guessing.
    pub fn blob_key(&self, file_id: Uuid) -> CoreResult<String> {
        let key: Option<Option<String>> = self
            .conn()
            .query_row(
                "SELECT blob_key FROM files WHERE id = ?1",
                [file_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CoreError::Database(e.to_string()))?;
        Ok(key.flatten().unwrap_or_default())
    }

    /// Every file's content key, still wrapped, by blob id. For checking a
    /// silo against its storage. Trashed files are included: their content
    /// is still in storage until a purge, and skipping them would call a
    /// silo sound while something it still holds had rotted.
    pub fn blob_keys(&self) -> CoreResult<std::collections::HashMap<Uuid, String>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT blob_id, blob_key FROM files WHERE blob_key IS NOT NULL")
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (id, key) = row.map_err(|e| CoreError::Database(e.to_string()))?;
            if let Ok(id) = Uuid::parse_str(&id) {
                out.insert(id, key);
            }
        }
        Ok(out)
    }

    /// Every password attachment's blob, with its plaintext size and its
    /// wrapped content key. Attachments have no row in `files`: the sealed
    /// entry is their only reference, so anything deciding which blobs are
    /// still needed, or which key opens one, has to ask here too. An entry
    /// without attachments contributes nothing; a row that will not unseal
    /// fails the call, like `list_passwords`.
    pub fn attachment_blobs(&self) -> CoreResult<Vec<AttachmentBlob>> {
        let mut out = Vec::new();
        for raw in self.list_passwords()? {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let Some(attachments) = value.get("attachments").and_then(|a| a.as_array()) else {
                continue;
            };
            for attachment in attachments {
                let Some(id) = attachment
                    .get("blob_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                else {
                    continue;
                };
                out.push(AttachmentBlob {
                    blob_id: id,
                    size_bytes: attachment
                        .get("size_bytes")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    blob_key: attachment
                        .get("blob_key")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        }
        Ok(out)
    }

    /// Every blob the silo still needs: the tree's content, trashed files
    /// included, plus password attachments. The set a sweep must never
    /// delete from, and the set a full copy has to hold.
    pub fn referenced_blobs_with_attachments(&self) -> CoreResult<std::collections::HashSet<Uuid>> {
        let mut out = crate::snapshot::referenced_blobs(self.conn())?;
        for attachment in self.attachment_blobs()? {
            out.insert(attachment.blob_id);
        }
        Ok(out)
    }

    /// `blob_key` is this content's key, already wrapped under the vault
    /// DEK by the caller. It travels in the record so that rotating the
    /// vault key never has to rewrite the content itself.
    #[allow(clippy::too_many_arguments)]
    pub fn add_file(
        &self,
        folder_id: Uuid,
        name: &str,
        blob_id: Uuid,
        size_bytes: i64,
        content_hash: &str,
        mime_type: Option<&str>,
        blob_key: &str,
    ) -> CoreResult<FileEntry> {
        let name = &normalized_input(name)?;

        // Uploading over an existing name replaces its content in place —
        // what Explorer and Finder do on a drag-and-drop overwrite. That is
        // a different operation from adding a file, because it targets a
        // known id and so can never collide with anything.
        let existing_id: Option<(String, String)> = self
            .conn()
            .query_row(
                "SELECT id, blob_id FROM files
                 WHERE folder_id = ?1 AND name = ?2 AND deleted_at IS NULL",
                params![folder_id.to_string(), name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let op = match &existing_id {
            Some((raw, previous_blob)) => {
                let id = Uuid::parse_str(raw).map_err(|e| CoreError::Database(e.to_string()))?;
                crate::oplog::VaultOp::ReplaceFileContent {
                    id,
                    blob_id,
                    size_bytes,
                    content_hash: content_hash.to_string(),
                    mime_type: mime_type.map(str::to_string),
                    blob_key: blob_key.to_string(),
                    // What this edit is being made on top of, which is how
                    // another device can tell later that it was working from
                    // a version this one had already moved past.
                    replaces: Uuid::parse_str(previous_blob).ok(),
                }
            }
            None => crate::oplog::VaultOp::AddFile {
                id: Uuid::now_v7(),
                folder_id,
                name: name.to_string(),
                blob_id,
                size_bytes,
                content_hash: content_hash.to_string(),
                mime_type: mime_type.map(str::to_string),
                blob_key: blob_key.to_string(),
            },
        };

        let id = match &op {
            crate::oplog::VaultOp::ReplaceFileContent { id, .. } => *id,
            crate::oplog::VaultOp::AddFile { id, .. } => *id,
            _ => unreachable!("only file operations are built here"),
        };

        crate::oplog::emit(self.conn(), op)?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        self.get_file(id)
    }

    pub fn get_file(&self, file_id: Uuid) -> CoreResult<FileEntry> {
        self.conn()
            .query_row(
                "SELECT id, folder_id, name, blob_id, size_bytes, mime_type, content_hash, created_at, updated_at, favorite
                 FROM files WHERE id = ?1 AND deleted_at IS NULL",
                [file_id.to_string()],
                |row| {
                    Ok(FileEntry {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                        folder_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                        name: row.get(2)?,
                        blob_id: Uuid::parse_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                        size_bytes: row.get(4)?,
                        mime_type: row.get(5)?,
                        content_hash: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                        favorite: row.get::<_, i64>(9)? != 0,
                    })
                },
            )
            .map_err(|_| CoreError::NotFound(file_id.to_string()))
    }

    pub fn get_folder(&self, folder_id: Uuid) -> CoreResult<FolderEntry> {
        self.conn()
            .query_row(
                "SELECT id, parent_id, name, path, created_at, updated_at, favorite
                 FROM folders WHERE id = ?1 AND deleted_at IS NULL",
                [folder_id.to_string()],
                |row| {
                    Ok(FolderEntry {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                        parent_id: row
                            .get::<_, Option<String>>(1)?
                            .and_then(|s| Uuid::parse_str(&s).ok()),
                        name: row.get(2)?,
                        path: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                        favorite: row.get::<_, i64>(6)? != 0,
                    })
                },
            )
            .map_err(|_| CoreError::NotFound(folder_id.to_string()))
    }

    pub fn folder_by_path(&self, path: &str) -> CoreResult<FolderEntry> {
        self.conn()
            .query_row(
                "SELECT id, parent_id, name, path, created_at, updated_at, favorite
                 FROM folders WHERE path = ?1 AND deleted_at IS NULL",
                [path],
                |row| {
                    Ok(FolderEntry {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                        parent_id: row
                            .get::<_, Option<String>>(1)?
                            .and_then(|s| Uuid::parse_str(&s).ok()),
                        name: row.get(2)?,
                        path: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                        favorite: row.get::<_, i64>(6)? != 0,
                    })
                },
            )
            .map_err(|_| CoreError::NotFound(path.to_string()))
    }

    /// All non-deleted folders in the vault, for pickers that need the whole
    /// tree rather than one level (e.g. choosing an upload destination).
    pub fn list_all_folders(&self) -> CoreResult<Vec<FolderEntry>> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT id, parent_id, name, path, created_at, updated_at, favorite
                 FROM folders
                 WHERE deleted_at IS NULL
                 ORDER BY path COLLATE NOCASE",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let folders = stmt
            .query_map([], |row| {
                Ok(FolderEntry {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    parent_id: row
                        .get::<_, Option<String>>(1)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    name: row.get(2)?,
                    path: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    favorite: row.get::<_, i64>(6)? != 0,
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        folders
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(e.to_string()))
    }

    /// Stars or unstars a file. Idempotent: setting what is already set
    /// emits nothing, so a double click on a star does not put a redundant
    /// record in the log every device then has to replay.
    pub fn set_file_favorite(&self, file_id: Uuid, favorite: bool) -> CoreResult<FileEntry> {
        let file = self.get_file(file_id)?;
        if file.favorite == favorite {
            return Ok(file);
        }
        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::SetFileFavorite {
                id: file_id,
                favorite,
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        self.get_file(file_id)
    }

    pub fn set_folder_favorite(&self, folder_id: Uuid, favorite: bool) -> CoreResult<FolderEntry> {
        let folder = self.get_folder(folder_id)?;
        if folder.path == "/" {
            return Err(CoreError::InvalidPath("cannot star the root".into()));
        }
        if folder.favorite == favorite {
            return Ok(folder);
        }
        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::SetFolderFavorite {
                id: folder_id,
                favorite,
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        self.get_folder(folder_id)
    }

    /// Everything starred, folders first, each with the path of the folder it
    /// lives in. Carries the path for the same reason search does: two
    /// starred files called "notes.txt" are otherwise indistinguishable.
    pub fn list_favorites(&self) -> CoreResult<Vec<SearchHit>> {
        let mut hits = Vec::new();

        let mut folder_stmt = self
            .conn()
            .prepare(
                "SELECT id, parent_id, name, path, created_at, updated_at, favorite
                   FROM folders
                  WHERE deleted_at IS NULL AND favorite = 1 AND parent_id IS NOT NULL
                  ORDER BY name COLLATE NOCASE",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let folders = folder_stmt
            .query_map([], |row| {
                let path: String = row.get(3)?;
                let parent_path = path
                    .rsplit_once('/')
                    .map(|(p, _)| p)
                    .unwrap_or("")
                    .to_string();
                Ok(SearchHit {
                    entry: VaultEntry::Folder(FolderEntry {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                        parent_id: row
                            .get::<_, Option<String>>(1)?
                            .and_then(|s| Uuid::parse_str(&s).ok()),
                        name: row.get(2)?,
                        path,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                        favorite: row.get::<_, i64>(6)? != 0,
                    }),
                    folder_path: if parent_path.is_empty() {
                        "/".into()
                    } else {
                        parent_path
                    },
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        for f in folders {
            hits.push(f.map_err(|e| CoreError::Database(e.to_string()))?);
        }

        let mut file_stmt = self
            .conn()
            .prepare(
                "SELECT f.id, f.name, f.blob_id, f.size_bytes, f.mime_type, f.created_at,
                        f.updated_at, f.folder_id, d.path, f.favorite
                   FROM files f
                   JOIN folders d ON d.id = f.folder_id
                  WHERE f.deleted_at IS NULL AND d.deleted_at IS NULL AND f.favorite = 1
                  ORDER BY f.name COLLATE NOCASE",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let files = file_stmt
            .query_map([], |row| {
                Ok(SearchHit {
                    entry: VaultEntry::File(FileEntry {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                        name: row.get(1)?,
                        blob_id: Uuid::parse_str(&row.get::<_, String>(2)?).unwrap_or_default(),
                        size_bytes: row.get(3)?,
                        mime_type: row.get(4)?,
                        content_hash: None,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                        folder_id: Uuid::parse_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                        favorite: row.get::<_, i64>(9)? != 0,
                    }),
                    folder_path: row.get(8)?,
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        for f in files {
            hits.push(f.map_err(|e| CoreError::Database(e.to_string()))?);
        }

        Ok(hits)
    }

    /// Every device that has written to this silo, busiest first.
    ///
    /// Read from the operation log, which is the only record of who has
    /// touched the silo. This device is always listed, even on a silo it has
    /// not changed yet, because "which one am I" is the first question the
    /// list has to answer.
    pub fn list_devices(&self) -> CoreResult<Vec<DeviceInfo>> {
        let this_device = crate::oplog::device_id(self.conn())?;

        let mut stmt = self
            .conn()
            .prepare(
                "SELECT a.device_id, COUNT(*), MAX(a.at), l.label, l.system_name, l.platform
                   FROM oplog a
                   LEFT JOIN device_labels l ON l.device_id = a.device_id
                  GROUP BY a.device_id",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                // An empty label is the absence of one: the column is NOT
                // NULL because a device may be announced before it is named.
                let label: Option<String> =
                    row.get::<_, Option<String>>(3)?.filter(|l| !l.is_empty());
                Ok(DeviceInfo {
                    id: Uuid::parse_str(&id).unwrap_or_default(),
                    operations: row.get(1)?,
                    last_change_at: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    label,
                    system_name: row.get(4)?,
                    platform: row.get(5)?,
                    is_this_device: false,
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let mut devices: Vec<DeviceInfo> = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(e.to_string()))?;

        for device in &mut devices {
            device.is_this_device = device.id == this_device;
        }

        if !devices.iter().any(|d| d.is_this_device) {
            let (label, system_name, platform) = self.device_names(this_device)?;
            devices.push(DeviceInfo {
                id: this_device,
                label,
                system_name,
                platform,
                is_this_device: true,
                operations: 0,
                last_change_at: 0,
            });
        }

        // This device first, then whoever has done the most.
        devices.sort_by(|a, b| {
            b.is_this_device
                .cmp(&a.is_this_device)
                .then(b.operations.cmp(&a.operations))
        });
        Ok(devices)
    }

    /// The three names a device can carry, for one that has written nothing
    /// and so has no row in the log to join against.
    #[allow(clippy::type_complexity)]
    fn device_names(
        &self,
        device_id: Uuid,
    ) -> CoreResult<(Option<String>, Option<String>, Option<String>)> {
        let row: Option<(String, Option<String>, Option<String>)> = self
            .conn()
            .query_row(
                "SELECT label, system_name, platform FROM device_labels WHERE device_id = ?1",
                [device_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|e| CoreError::Database(e.to_string()))?;

        Ok(match row {
            Some((label, system_name, platform)) => {
                (Some(label).filter(|l| !l.is_empty()), system_name, platform)
            }
            None => (None, None, None),
        })
    }

    /// Records what this machine calls itself, if that has changed since the
    /// last time it said so.
    ///
    /// Guarded by the comparison rather than emitted on every open: an
    /// operation per unlock would grow the log for nothing, and every device
    /// would have to replay it.
    pub fn announce_device(&self, system_name: Option<&str>, platform: &str) -> CoreResult<()> {
        let device_id = crate::oplog::device_id(self.conn())?;
        let current: Option<(Option<String>, Option<String>)> = self
            .conn()
            .query_row(
                "SELECT system_name, platform FROM device_labels WHERE device_id = ?1",
                [device_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let wanted = (system_name.map(str::to_string), Some(platform.to_string()));
        if current.as_ref() == Some(&wanted) {
            return Ok(());
        }

        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::AnnounceDevice {
                subject: device_id,
                system_name: wanted.0,
                platform: platform.to_string(),
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        Ok(())
    }

    /// Names a device, on every device. An empty name clears it rather than
    /// storing a blank, so the list falls back to the short id.
    pub fn set_device_label(&self, device_id: Uuid, label: &str) -> CoreResult<()> {
        let label = label.trim();
        if label.chars().count() > 60 {
            return Err(CoreError::InvalidPath("device name is too long".into()));
        }
        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::SetDeviceLabel {
                subject: device_id,
                label: label.to_string(),
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn inbox_folder(&self) -> CoreResult<FolderEntry> {
        match self.folder_by_path("/Inbox") {
            Ok(folder) => Ok(folder),
            Err(_) => self.create_folder(self.root_folder_id()?, "Inbox"),
        }
    }

    pub fn list_blob_ids(&self) -> CoreResult<Vec<Uuid>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT DISTINCT blob_id FROM files WHERE deleted_at IS NULL")
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                Ok(Uuid::parse_str(&id).unwrap_or_default())
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(e.to_string()))
    }

    /// Every blob the live tree references, with the plaintext size the
    /// explorer already shows. Grouped rather than DISTINCT: two rows can
    /// point at one blob, and rows differing only in `size_bytes` would
    /// otherwise count the same content twice.
    pub fn list_blob_sizes(&self) -> CoreResult<Vec<(Uuid, i64)>> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT blob_id, MAX(size_bytes) FROM files
                 WHERE deleted_at IS NULL
                 GROUP BY blob_id",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                Ok((Uuid::parse_str(&id).unwrap_or_default(), row.get(1)?))
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(e.to_string()))
    }

    /// Every blob referenced by a folder and everything beneath it.
    ///
    /// Used before exporting a folder, to work out what has to be downloaded
    /// first. Matching on the stored path rather than recursing keeps this a
    /// single query, and the trailing slash is what stops `/Work` from
    /// dragging in `/Workshop`.
    pub fn subtree_blob_ids(&self, folder_id: Uuid) -> CoreResult<Vec<Uuid>> {
        let folder = self.get_folder(folder_id)?;
        let subtree = crate::like::subtree(&folder.path);

        let mut stmt = self
            .conn()
            .prepare(
                "SELECT DISTINCT f.blob_id
                   FROM files f
                   JOIN folders d ON d.id = f.folder_id
                  WHERE f.deleted_at IS NULL
                    AND d.deleted_at IS NULL
                    AND (d.id = ?1 OR d.path LIKE ?2 ESCAPE '!')",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![folder_id.to_string(), subtree], |row| {
                let id: String = row.get(0)?;
                Ok(Uuid::parse_str(&id).unwrap_or_default())
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(e.to_string()))
    }

    /// Every live entry whose name contains `query`, anywhere in the vault.
    /// Names only: indexing contents would mean keeping a plaintext index,
    /// the one thing this design refuses to do.
    pub fn search_entries(&self, query: &str, limit: u32) -> CoreResult<Vec<SearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        // LIKE's own wildcards have to be neutralised, or searching for "%"
        // would match every file in the vault — which reads as broken search
        // rather than as anything clever.
        let pattern = crate::like::containing(query);

        let mut stmt = self
            .conn()
            .prepare(
                "SELECT f.id, f.name, f.blob_id, f.size_bytes, f.mime_type, f.created_at,
                        f.updated_at, f.folder_id, d.path, f.favorite
                   FROM files f
                   JOIN folders d ON d.id = f.folder_id
                  WHERE f.deleted_at IS NULL
                    AND d.deleted_at IS NULL
                    AND f.name LIKE ?1 ESCAPE '!'
                  ORDER BY f.updated_at DESC
                  LIMIT ?2",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![pattern, limit], |row| {
                Ok(SearchHit {
                    entry: VaultEntry::File(FileEntry {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                        name: row.get(1)?,
                        blob_id: Uuid::parse_str(&row.get::<_, String>(2)?).unwrap_or_default(),
                        size_bytes: row.get(3)?,
                        mime_type: row.get(4)?,
                        content_hash: None,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                        folder_id: Uuid::parse_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                        favorite: row.get::<_, i64>(9)? != 0,
                    }),
                    folder_path: row.get(8)?,
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let mut hits = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let mut stmt = self
            .conn()
            .prepare(
                "SELECT id, parent_id, name, path, created_at, updated_at, favorite
                   FROM folders
                  WHERE deleted_at IS NULL
                    AND parent_id IS NOT NULL
                    AND name LIKE ?1 ESCAPE '!'
                  ORDER BY updated_at DESC
                  LIMIT ?2",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let folder_rows = stmt
            .query_map(params![pattern, limit], |row| {
                let path: String = row.get(3)?;
                // The containing folder, which is what the user needs in
                // order to know *which* "Invoices" this is.
                let parent_path = path
                    .rsplit_once('/')
                    .map(|(p, _)| p)
                    .unwrap_or("")
                    .to_string();
                Ok(SearchHit {
                    entry: VaultEntry::Folder(FolderEntry {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                        parent_id: row
                            .get::<_, Option<String>>(1)?
                            .and_then(|s| Uuid::parse_str(&s).ok()),
                        name: row.get(2)?,
                        path,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                        favorite: row.get::<_, i64>(6)? != 0,
                    }),
                    folder_path: if parent_path.is_empty() {
                        "/".into()
                    } else {
                        parent_path
                    },
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        // Folders first: someone searching a name that is both is nearly
        // always looking for the place, not the file.
        let mut folders = folder_rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Database(e.to_string()))?;
        folders.append(&mut hits);
        folders.truncate(limit as usize);
        Ok(folders)
    }

    pub fn get_meta(&self, key: &str) -> CoreResult<Option<String>> {
        self.conn()
            .query_row(
                "SELECT value FROM vault_meta WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CoreError::Database(e.to_string()))
    }

    pub fn set_meta(&self, key: &str, value: &str) -> CoreResult<()> {
        self.conn()
            .execute(
                "INSERT INTO vault_meta(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, value],
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn rename_file(&self, file_id: Uuid, new_name: &str) -> CoreResult<FileEntry> {
        let new_name = &normalized_input(new_name)?;

        let file = self.get_file(file_id)?;
        if file.name == *new_name {
            return Ok(file);
        }

        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::RenameFile {
                id: file_id,
                name: new_name.to_string(),
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        self.get_file(file_id)
    }

    pub fn rename_folder(&self, folder_id: Uuid, new_name: &str) -> CoreResult<FolderEntry> {
        let new_name = &normalized_input(new_name)?;

        let folder = self.get_folder(folder_id)?;
        if folder.path == "/" {
            return Err(CoreError::InvalidPath("cannot rename root".into()));
        }
        if folder.name == *new_name {
            return Ok(folder);
        }

        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::RenameFolder {
                id: folder_id,
                name: new_name.to_string(),
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        self.get_folder(folder_id)
    }

    pub fn trash_file(&self, file_id: Uuid) -> CoreResult<()> {
        let (_record, outcome) = crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::TrashFile { id: file_id },
        )?;
        if outcome == crate::oplog::ApplyOutcome::Obsolete {
            return Err(CoreError::NotFound(file_id.to_string()));
        }
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        Ok(())
    }

    /// Every password entry, as the JSON each was stored with. Returned raw
    /// rather than parsed, so the store cannot break when a field is added.
    /// This is where sealed entries become plaintext, in memory, for as
    /// long as the caller holds them.
    pub fn list_passwords(&self) -> CoreResult<Vec<String>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT data FROM passwords ORDER BY updated_at DESC")
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let mut out = Vec::new();
        for row in rows {
            let sealed = row.map_err(|e| CoreError::Database(e.to_string()))?;
            // A row that will not open means the store is damaged. Skipping
            // it would show the user a shorter list and no reason for it,
            // which for a password manager is worse than refusing to load.
            out.push(self.unseal_entry(&sealed)?);
        }
        Ok(out)
    }

    /// Creates or replaces one entry. `data` is the whole entry as JSON,
    /// sealed with the vault key before it reaches the database: the
    /// working copy of `vault.db` is plaintext on disk while unlocked,
    /// acceptable for file names and not for credentials. Sealing here also
    /// means the operation record carries ciphertext, so every device
    /// stores the same bytes.
    /// Sealed under the content KEK rather than the vault DEK, for the same
    /// reason blob keys are: this value goes inside an operation record, and
    /// a record's fingerprint covers its body. Re-sealing it to rotate the
    /// vault key would change the fingerprint, break the `prev` chain and
    /// leave this device disagreeing with every other about the bytes of the
    /// same operation. Under the KEK it never has to change.
    pub fn upsert_password(&self, id: Uuid, data: &str) -> CoreResult<()> {
        let sealed = silentsilo_crypto::seal_with_key(data.as_bytes(), self.session.kek.as_bytes())
            .map_err(|e| CoreError::Crypto(e.to_string()))?;
        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::UpsertPassword {
                id,
                data: BASE64.encode(sealed),
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        Ok(())
    }

    fn unseal_entry(&self, sealed_b64: &str) -> CoreResult<String> {
        let sealed = BASE64
            .decode(sealed_b64)
            .map_err(|e| CoreError::Crypto(format!("password entry is not readable: {e}")))?;
        let plain = silentsilo_crypto::unseal_with_key(&sealed, self.session.kek.as_bytes())
            .map_err(|e| CoreError::Crypto(format!("password entry is not readable: {e}")))?;
        String::from_utf8(plain)
            .map_err(|e| CoreError::Crypto(format!("password entry is not readable: {e}")))
    }

    /// Removes one entry. Deleting something that is already gone is not an
    /// error: the caller asked for a state, and that state holds.
    pub fn delete_password(&self, id: Uuid) -> CoreResult<()> {
        crate::oplog::emit(self.conn(), crate::oplog::VaultOp::DeletePassword { id })?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn trash_folder(&self, folder_id: Uuid) -> CoreResult<()> {
        let folder = self.get_folder(folder_id)?;
        if folder.path == "/" || folder.path == "/Inbox" {
            return Err(CoreError::InvalidPath("protected folder".into()));
        }

        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::TrashFolder { id: folder_id },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        Ok(())
    }

    /// Trashed folders/files whose parent isn't *also* trashed — i.e. the
    /// items the user actually deleted, not their cascaded children.
    pub fn list_trash(&self) -> CoreResult<Vec<TrashItem>> {
        let mut entries = Vec::new();

        // Trashed folders: `original_path` is the *parent's* path (where the
        // folder used to live), not its own — its own name/path is already
        // shown via the entry itself, so showing that again here would be
        // redundant with the Name column in the UI.
        let mut folder_stmt = self
            .conn()
            .prepare(
                "SELECT f.id, f.parent_id, f.name, f.path, f.created_at, f.updated_at, f.favorite,
                        COALESCE(p.path, '/') AS original_path
                 FROM folders f
                 LEFT JOIN folders p ON p.id = f.parent_id
                 WHERE f.deleted_at IS NOT NULL
                   AND (f.parent_id IS NULL OR f.parent_id NOT IN (
                       SELECT id FROM folders WHERE deleted_at IS NOT NULL
                   ))
                 ORDER BY f.updated_at DESC",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let folders = folder_stmt
            .query_map([], |row| {
                let entry = FolderEntry {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    parent_id: row
                        .get::<_, Option<String>>(1)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    name: row.get(2)?,
                    path: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    favorite: row.get::<_, i64>(6)? != 0,
                };
                Ok(TrashItem {
                    entry: VaultEntry::Folder(entry),
                    original_path: row.get(7)?,
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;
        for f in folders {
            entries.push(f.map_err(|e| CoreError::Database(e.to_string()))?);
        }

        // Trashed files: `original_path` is the containing (still-live)
        // folder's path — files whose folder was *also* trashed are covered
        // by that folder's own entry above instead, per the NOT IN below.
        let mut file_stmt = self
            .conn()
            .prepare(
                "SELECT fi.id, fi.folder_id, fi.name, fi.blob_id, fi.size_bytes, fi.mime_type,
                        fi.content_hash, fi.created_at, fi.updated_at, fi.favorite,
                        fo.path AS original_path
                 FROM files fi
                 JOIN folders fo ON fo.id = fi.folder_id
                 WHERE fi.deleted_at IS NOT NULL
                   AND fi.folder_id NOT IN (SELECT id FROM folders WHERE deleted_at IS NOT NULL)
                 ORDER BY fi.updated_at DESC",
            )
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let files = file_stmt
            .query_map([], |row| {
                let entry = FileEntry {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    folder_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                    name: row.get(2)?,
                    blob_id: Uuid::parse_str(&row.get::<_, String>(3)?).unwrap_or_default(),
                    size_bytes: row.get(4)?,
                    mime_type: row.get(5)?,
                    content_hash: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    favorite: row.get::<_, i64>(9)? != 0,
                };
                Ok(TrashItem {
                    entry: VaultEntry::File(entry),
                    original_path: row.get(10)?,
                })
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;
        for f in files {
            entries.push(f.map_err(|e| CoreError::Database(e.to_string()))?);
        }

        entries.sort_by(|a, b| {
            let updated = |item: &TrashItem| match &item.entry {
                VaultEntry::Folder(f) => f.updated_at,
                VaultEntry::File(f) => f.updated_at,
            };
            updated(b).cmp(&updated(a))
        });

        Ok(entries)
    }

    pub fn restore_file(&self, file_id: Uuid) -> CoreResult<FileEntry> {
        let (_record, outcome) = crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::RestoreFile { id: file_id },
        )?;
        if outcome == crate::oplog::ApplyOutcome::Obsolete {
            return Err(CoreError::NotFound(file_id.to_string()));
        }
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        self.get_file(file_id)
    }

    pub fn restore_folder(&self, folder_id: Uuid) -> CoreResult<FolderEntry> {
        let trashed: Option<i64> = self
            .conn()
            .query_row(
                "SELECT 1 FROM folders WHERE id = ?1 AND deleted_at IS NOT NULL",
                [folder_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CoreError::Database(e.to_string()))?;
        if trashed.is_none() {
            return Err(CoreError::NotFound(folder_id.to_string()));
        }

        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::RestoreFolder { id: folder_id },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        self.get_folder(folder_id)
    }

    /// Permanently remove all trashed items from the local index. Returns
    /// the number of rows removed and the blob IDs of the files among them,
    /// so the caller can also delete the corresponding blob objects (local
    /// cache and/or cloud) — this function only ever touches `vault.db`.
    pub fn empty_trash(&self) -> CoreResult<(u64, Vec<Uuid>)> {
        // The operation carries the exact ids being removed rather than
        // meaning "empty whatever is in the trash". Each device's trash may
        // differ at the moment this runs, and a self-referential operation
        // like that would delete different things on every device.
        let (file_ids, blob_ids) = self.trashed_files()?;
        let folder_ids = self.trashed_folder_ids()?;
        let removed = (file_ids.len() + folder_ids.len()) as u64;

        if removed == 0 {
            return Ok((0, Vec::new()));
        }

        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::Purge {
                folder_ids,
                file_ids,
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        Ok((removed, self.orphaned_by_purge(blob_ids)?))
    }

    /// Of the blobs a purge released, the ones nothing points at any more.
    ///
    /// Run after the operation has been applied, so what is left in `files`
    /// is what survived it. A blob id is not a file's private property: a
    /// concurrent edit resolves into a conflict copy, and that copy is a
    /// second row carrying the *same* `blob_id` (see `oplog::conflict_copy`).
    /// Purging one of the pair and then deleting its content from storage
    /// and the local cache would leave the other row pointing at bytes that
    /// no longer exist anywhere, which is the one failure this app cannot
    /// let the user recover from.
    fn orphaned_by_purge(&self, released: Vec<Uuid>) -> CoreResult<Vec<Uuid>> {
        if released.is_empty() {
            return Ok(released);
        }
        let still_referenced = crate::snapshot::referenced_blobs(self.conn())?;
        Ok(released
            .into_iter()
            .filter(|id| !still_referenced.contains(id))
            .collect())
    }

    /// Permanently removes the named trashed entries. A folder takes
    /// everything beneath it. Ids not in the trash are ignored rather than
    /// refused, since the only way to name one is a stale list. Returns the
    /// rows removed and the blobs they referenced, so the caller can delete
    /// the content too.
    pub fn purge_items(&self, ids: &[Uuid]) -> CoreResult<(u64, Vec<Uuid>)> {
        let db = |e: rusqlite::Error| CoreError::Database(e.to_string());
        let mut folder_ids: Vec<Uuid> = Vec::new();
        let mut file_ids: Vec<Uuid> = Vec::new();
        let mut blob_ids: Vec<Uuid> = Vec::new();

        for id in ids {
            let raw = id.to_string();
            let folder_path: Option<String> = self
                .conn()
                .query_row(
                    "SELECT path FROM folders WHERE id = ?1 AND deleted_at IS NOT NULL",
                    [&raw],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db)?;

            if let Some(path) = folder_path {
                let subtree = crate::like::subtree(&path);
                let mut stmt = self
                    .conn()
                    .prepare("SELECT id FROM folders WHERE id = ?1 OR path LIKE ?2 ESCAPE '!'")
                    .map_err(db)?;
                let rows = stmt
                    .query_map(params![&raw, &subtree], |row| row.get::<_, String>(0))
                    .map_err(db)?;
                for row in rows {
                    folder_ids.push(parse_id(&row.map_err(db)?)?);
                }

                let mut stmt = self
                    .conn()
                    .prepare(
                        "SELECT f.id, f.blob_id FROM files f
                           JOIN folders d ON d.id = f.folder_id
                          WHERE d.id = ?1 OR d.path LIKE ?2 ESCAPE '!'",
                    )
                    .map_err(db)?;
                let rows = stmt
                    .query_map(params![&raw, &subtree], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(db)?;
                for row in rows {
                    let (file_id, blob_id) = row.map_err(db)?;
                    file_ids.push(parse_id(&file_id)?);
                    blob_ids.push(parse_id(&blob_id)?);
                }
                continue;
            }

            let file: Option<String> = self
                .conn()
                .query_row(
                    "SELECT blob_id FROM files WHERE id = ?1 AND deleted_at IS NOT NULL",
                    [&raw],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db)?;
            if let Some(blob_id) = file {
                file_ids.push(*id);
                blob_ids.push(parse_id(&blob_id)?);
            }
        }

        // A file can be named twice: on its own and through its folder.
        folder_ids.sort();
        folder_ids.dedup();
        file_ids.sort();
        file_ids.dedup();
        blob_ids.sort();
        blob_ids.dedup();

        let removed = (folder_ids.len() + file_ids.len()) as u64;
        if removed == 0 {
            return Ok((0, Vec::new()));
        }

        crate::oplog::emit(
            self.conn(),
            crate::oplog::VaultOp::Purge {
                folder_ids,
                file_ids,
            },
        )?;
        bump_revision(self.conn()).map_err(|e| CoreError::Database(e.to_string()))?;
        Ok((removed, self.orphaned_by_purge(blob_ids)?))
    }

    /// Ids and blob ids of every trashed file, so a purge can name them
    /// explicitly and the caller can clean up the blobs afterwards.
    fn trashed_files(&self) -> CoreResult<(Vec<Uuid>, Vec<Uuid>)> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id, blob_id FROM files WHERE deleted_at IS NOT NULL")
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CoreError::Database(e.to_string()))?;

        let (mut ids, mut blobs) = (Vec::new(), Vec::new());
        for row in rows {
            let (id, blob) = row.map_err(|e| CoreError::Database(e.to_string()))?;
            ids.push(Uuid::parse_str(&id).unwrap_or_default());
            blobs.push(Uuid::parse_str(&blob).unwrap_or_default());
        }
        Ok((ids, blobs))
    }

    fn trashed_folder_ids(&self) -> CoreResult<Vec<Uuid>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id FROM folders WHERE deleted_at IS NOT NULL")
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let mut ids = Vec::new();
        for row in rows {
            let id = row.map_err(|e| CoreError::Database(e.to_string()))?;
            ids.push(Uuid::parse_str(&id).unwrap_or_default());
        }
        Ok(ids)
    }
}

fn parse_id(raw: &str) -> CoreResult<Uuid> {
    Uuid::parse_str(raw).map_err(|e| CoreError::Database(e.to_string()))
}

pub fn guess_mime(path: &Path) -> Option<String> {
    mime_guess::from_path(path).first().map(|m| m.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use silentsilo_vault::VaultSession;
    use tempfile::tempdir;

    fn new_session() -> (tempfile::TempDir, VaultSession) {
        let dir = tempdir().unwrap();
        let session =
            VaultSession::provision(dir.path().to_path_buf(), Uuid::new_v4(), "test-secret")
                .unwrap();
        let vfs = Vfs::new(&session);
        vfs.ensure_initialized().unwrap();
        (dir, session)
    }

    #[test]
    fn a_decomposed_name_replaces_the_composed_row_instead_of_duplicating_it() {
        // macOS hands back decomposed names. Uploading the same file again
        // from a Mac used to miss the duplicate check, which compares the
        // exact string, and came back as "name (2)" instead of a replace.
        use unicode_normalization::UnicodeNormalization;
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let composed = "ștampilă.pdf";
        let decomposed: String = composed.nfd().collect();
        assert_ne!(composed, decomposed.as_str());

        vfs.add_file(root, composed, Uuid::new_v4(), 1, "one", None, "k")
            .unwrap();
        vfs.add_file(root, &decomposed, Uuid::new_v4(), 2, "two", None, "k")
            .unwrap();

        let entries = vfs.list_folder(root).unwrap();
        assert_eq!(
            entries.len(),
            1,
            "the second upload made a copy: {entries:?}"
        );
    }

    #[test]
    fn creating_a_folder_that_exists_can_hand_back_the_existing_one() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let first = vfs.create_or_get_folder(root, "Photos").unwrap();
        let again = vfs.create_or_get_folder(root, "photos").unwrap();

        assert_eq!(first.id, again.id, "the drop should merge, not duplicate");
        // The strict path still refuses, for the rename dialog and friends.
        assert!(matches!(
            vfs.create_folder(root, "Photos"),
            Err(CoreError::NameConflict(_))
        ));
    }

    #[test]
    fn attachments_count_as_referenced_content() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let file_blob = Uuid::new_v4();
        vfs.add_file(root, "a.txt", file_blob, 3, "hash", None, "wrapped")
            .unwrap();

        let attached = Uuid::new_v4();
        vfs.upsert_password(
            Uuid::new_v4(),
            &format!(
                r#"{{"id":"e1","attachments":[{{"blob_id":"{attached}","name":"scan.pdf","size_bytes":7,"blob_key":"wrapped-attachment"}}]}}"#
            ),
        )
        .unwrap();
        // An entry with no attachments contributes nothing and breaks
        // nothing.
        vfs.upsert_password(Uuid::new_v4(), r#"{"id":"e2","title":"plain"}"#)
            .unwrap();

        let attachments = vfs.attachment_blobs().unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].blob_id, attached);
        assert_eq!(attachments[0].size_bytes, 7);
        assert_eq!(attachments[0].blob_key, "wrapped-attachment");

        let referenced = vfs.referenced_blobs_with_attachments().unwrap();
        assert!(referenced.contains(&file_blob));
        assert!(referenced.contains(&attached));
    }

    #[test]
    fn a_device_announces_itself_once_and_then_stays_quiet() {
        // One record per change, not one per unlock: the log is replayed by
        // every device, so a per-open operation would cost all of them.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);

        let pending = |vfs: &Vfs| crate::oplog::pending_ops(vfs.conn()).unwrap().len();
        let before = pending(&vfs);

        vfs.announce_device(Some("DESK-1"), "Windows 11 Pro")
            .unwrap();
        assert_eq!(pending(&vfs), before + 1);

        vfs.announce_device(Some("DESK-1"), "Windows 11 Pro")
            .unwrap();
        assert_eq!(pending(&vfs), before + 1, "nothing changed, nothing to say");

        vfs.announce_device(Some("DESK-1"), "Windows 11 Pro (24H2)")
            .unwrap();
        assert_eq!(pending(&vfs), before + 2, "a new answer is worth a record");
    }

    #[test]
    fn announcing_does_not_overwrite_a_name_someone_typed() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let this = crate::oplog::device_id(vfs.conn()).unwrap();

        vfs.announce_device(Some("DESK-1"), "Windows").unwrap();
        vfs.set_device_label(this, "Alex's desktop").unwrap();
        vfs.announce_device(Some("DESK-1-RENAMED"), "Windows")
            .unwrap();

        let device = vfs
            .list_devices()
            .unwrap()
            .into_iter()
            .find(|d| d.is_this_device)
            .unwrap();
        assert_eq!(device.label.as_deref(), Some("Alex's desktop"));
        assert_eq!(device.system_name.as_deref(), Some("DESK-1-RENAMED"));
    }

    #[test]
    fn clearing_a_typed_name_falls_back_to_what_the_machine_calls_itself() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let this = crate::oplog::device_id(vfs.conn()).unwrap();

        vfs.announce_device(Some("DESK-1"), "Windows").unwrap();
        vfs.set_device_label(this, "Temporary").unwrap();
        vfs.set_device_label(this, "  ").unwrap();

        let device = vfs
            .list_devices()
            .unwrap()
            .into_iter()
            .find(|d| d.is_this_device)
            .unwrap();
        assert_eq!(device.label, None);
        assert_eq!(device.system_name.as_deref(), Some("DESK-1"));
    }

    #[test]
    fn this_device_is_listed_even_before_it_has_changed_anything() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);

        let devices = vfs.list_devices().unwrap();
        assert_eq!(devices.len(), 1);
        assert!(devices[0].is_this_device);
    }

    #[test]
    fn create_folder_and_list() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let docs = vfs.create_folder(root, "Docs").unwrap();
        assert_eq!(docs.path, "/Docs");

        let entries = vfs.list_folder(root).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(&entries[0], VaultEntry::Folder(f) if f.id == docs.id));
    }

    #[test]
    fn create_folder_rejects_duplicate_name() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        vfs.create_folder(root, "Docs").unwrap();
        let err = vfs.create_folder(root, "Docs").unwrap_err();
        assert!(matches!(err, CoreError::NameConflict(_)));
    }

    #[test]
    fn create_folder_rejects_slash_in_name() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let err = vfs.create_folder(root, "a/b").unwrap_err();
        assert!(matches!(err, CoreError::InvalidName(_)));
    }

    #[test]
    fn create_folder_rejects_traversal_names() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        for bad in ["..", ".", "a\\b", "..\\.."] {
            let err = vfs.create_folder(root, bad).unwrap_err();
            assert!(
                matches!(err, CoreError::InvalidName(_)),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn rename_rejects_traversal_names() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let folder = vfs.create_folder(root, "Docs").unwrap();
        assert!(matches!(
            vfs.rename_folder(folder.id, "..").unwrap_err(),
            CoreError::InvalidName(_)
        ));

        let file = vfs
            .add_file(root, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        assert!(matches!(
            vfs.rename_file(file.id, "..").unwrap_err(),
            CoreError::InvalidName(_)
        ));
    }

    #[test]
    fn add_file_and_rename() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let file = vfs
            .add_file(
                root,
                "notes.txt",
                Uuid::new_v4(),
                42,
                "deadbeef",
                Some("text/plain"),
                "",
            )
            .unwrap();
        assert_eq!(file.name, "notes.txt");

        let renamed = vfs.rename_file(file.id, "renamed.txt").unwrap();
        assert_eq!(renamed.name, "renamed.txt");
        assert_eq!(
            renamed.blob_id, file.blob_id,
            "renaming must not touch the blob"
        );
    }

    #[test]
    fn add_file_with_same_name_replaces_in_place_instead_of_erroring() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let original = vfs
            .add_file(
                root,
                "report.pdf",
                Uuid::new_v4(),
                100,
                "hash-v1",
                Some("application/pdf"),
                "",
            )
            .unwrap();

        let replaced = vfs
            .add_file(
                root,
                "report.pdf",
                Uuid::new_v4(),
                250,
                "hash-v2",
                Some("application/pdf"),
                "",
            )
            .unwrap();

        assert_eq!(replaced.id, original.id, "replace keeps the same file id");
        assert_ne!(replaced.blob_id, original.blob_id);
        assert_eq!(replaced.size_bytes, 250);
        assert_eq!(replaced.content_hash.as_deref(), Some("hash-v2"));

        // No duplicate row was created — the folder still lists exactly one
        // "report.pdf".
        let entries = vfs.list_folder(root).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn add_file_does_not_replace_a_same_named_file_in_a_different_folder() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();
        let docs = vfs.create_folder(root, "Docs").unwrap();

        let in_root = vfs
            .add_file(root, "report.pdf", Uuid::new_v4(), 100, "hash-a", None, "")
            .unwrap();
        let in_docs = vfs
            .add_file(
                docs.id,
                "report.pdf",
                Uuid::new_v4(),
                200,
                "hash-b",
                None,
                "",
            )
            .unwrap();

        assert_ne!(in_root.id, in_docs.id);
        assert_eq!(vfs.get_file(in_root.id).unwrap().size_bytes, 100);
        assert_eq!(vfs.get_file(in_docs.id).unwrap().size_bytes, 200);
    }

    #[test]
    fn rename_folder_cascades_to_child_paths() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let parent = vfs.create_folder(root, "Parent").unwrap();
        let child = vfs.create_folder(parent.id, "Child").unwrap();
        assert_eq!(child.path, "/Parent/Child");

        vfs.rename_folder(parent.id, "Renamed").unwrap();

        let child_after = vfs.get_folder(child.id).unwrap();
        assert_eq!(child_after.path, "/Renamed/Child");
    }

    #[test]
    fn rename_root_folder_is_rejected() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let err = vfs.rename_folder(root, "NewRoot").unwrap_err();
        assert!(matches!(err, CoreError::InvalidPath(_)));
    }

    #[test]
    fn trash_file_hides_it_from_listing() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let file = vfs
            .add_file(root, "temp.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        vfs.trash_file(file.id).unwrap();

        assert!(vfs.list_folder(root).unwrap().is_empty());
        assert!(matches!(
            vfs.get_file(file.id).unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn trash_folder_cascades_to_children() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let parent = vfs.create_folder(root, "Parent").unwrap();
        let child_file = vfs
            .add_file(parent.id, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();

        vfs.trash_folder(parent.id).unwrap();

        assert!(vfs.list_folder(root).unwrap().is_empty());
        assert!(matches!(
            vfs.get_file(child_file.id).unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn trash_folder_protects_root_and_inbox() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        assert!(matches!(
            vfs.trash_folder(root).unwrap_err(),
            CoreError::InvalidPath(_)
        ));

        let inbox = vfs.inbox_folder().unwrap();
        assert!(matches!(
            vfs.trash_folder(inbox.id).unwrap_err(),
            CoreError::InvalidPath(_)
        ));
    }

    #[test]
    fn list_all_folders_includes_nested_paths_sorted() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        vfs.create_folder(root, "Zeta").unwrap();
        let alpha = vfs.create_folder(root, "Alpha").unwrap();
        vfs.create_folder(alpha.id, "Nested").unwrap();

        let paths: Vec<String> = vfs
            .list_all_folders()
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();

        assert_eq!(paths, vec!["/", "/Alpha", "/Alpha/Nested", "/Zeta"]);
    }

    #[test]
    fn meta_roundtrip_and_revision_bumps() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);

        assert_eq!(vfs.get_meta("missing").unwrap(), None);

        let before = vfs.meta().unwrap().revision;
        vfs.set_meta("vault_db_remote_etag", "\"abc123\"").unwrap();
        assert_eq!(
            vfs.get_meta("vault_db_remote_etag").unwrap().as_deref(),
            Some("\"abc123\"")
        );
        assert!(vfs.meta().unwrap().revision > before);
    }

    #[test]
    fn list_trash_reports_the_original_containing_folder_path() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let docs = vfs.create_folder(root, "Documents").unwrap();
        let receipts = vfs.create_folder(docs.id, "Receipts").unwrap();
        assert_eq!(receipts.path, "/Documents/Receipts");

        let file = vfs
            .add_file(
                receipts.id,
                "invoice.pdf",
                Uuid::new_v4(),
                10,
                "hash",
                None,
                "",
            )
            .unwrap();
        vfs.trash_file(file.id).unwrap();

        let old_reports = vfs.create_folder(docs.id, "OldReports").unwrap();
        vfs.trash_folder(old_reports.id).unwrap();

        let trash = vfs.list_trash().unwrap();
        assert_eq!(trash.len(), 2);

        let file_item = trash
            .iter()
            .find(|t| matches!(&t.entry, VaultEntry::File(f) if f.id == file.id))
            .expect("trashed file present");
        assert_eq!(file_item.original_path, "/Documents/Receipts");

        let folder_item = trash
            .iter()
            .find(|t| matches!(&t.entry, VaultEntry::Folder(f) if f.id == old_reports.id))
            .expect("trashed folder present");
        // A folder's own path ("/Documents/OldReports") already appears via
        // the entry itself — original_path is its *parent's* path instead.
        assert_eq!(folder_item.original_path, "/Documents");
    }

    #[test]
    fn trash_then_restore_file() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let file = vfs
            .add_file(root, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        vfs.trash_file(file.id).unwrap();
        assert!(vfs.list_folder(root).unwrap().is_empty());

        let trash = vfs.list_trash().unwrap();
        assert_eq!(trash.len(), 1);

        let restored = vfs.restore_file(file.id).unwrap();
        assert_eq!(restored.name, "a.txt");
        assert_eq!(vfs.list_folder(root).unwrap().len(), 1);
        assert!(vfs.list_trash().unwrap().is_empty());
    }

    #[test]
    fn trash_then_restore_folder_brings_back_children() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let parent = vfs.create_folder(root, "Parent").unwrap();
        let file = vfs
            .add_file(parent.id, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();

        vfs.trash_folder(parent.id).unwrap();
        vfs.restore_folder(parent.id).unwrap();

        assert_eq!(vfs.list_folder(root).unwrap().len(), 1);
        assert_eq!(vfs.list_folder(parent.id).unwrap().len(), 1);
        assert!(vfs.get_file(file.id).is_ok());
        assert!(vfs.list_trash().unwrap().is_empty());
    }

    #[test]
    fn list_trash_hides_cascaded_children_of_a_trashed_folder() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let parent = vfs.create_folder(root, "Parent").unwrap();
        vfs.add_file(parent.id, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        vfs.trash_folder(parent.id).unwrap();

        // Only the trashed folder itself shows up, not the file cascaded
        // along with it.
        let trash = vfs.list_trash().unwrap();
        assert_eq!(trash.len(), 1);
        assert!(matches!(&trash[0].entry, VaultEntry::Folder(f) if f.id == parent.id));
    }

    #[test]
    fn empty_trash_permanently_removes_entries() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let file = vfs
            .add_file(root, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        vfs.trash_file(file.id).unwrap();

        let (removed, blob_ids) = vfs.empty_trash().unwrap();
        assert_eq!(removed, 1);
        assert_eq!(blob_ids, vec![file.blob_id]);
        assert!(vfs.list_trash().unwrap().is_empty());
        assert!(matches!(
            vfs.restore_file(file.id).unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn purging_one_item_leaves_the_rest_of_the_trash_alone() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let doomed = vfs
            .add_file(root, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        let spared = vfs
            .add_file(root, "b.txt", Uuid::new_v4(), 1, "bb", None, "")
            .unwrap();
        vfs.trash_file(doomed.id).unwrap();
        vfs.trash_file(spared.id).unwrap();

        let (removed, blob_ids) = vfs.purge_items(&[doomed.id]).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(blob_ids, vec![doomed.blob_id]);

        let trash = vfs.list_trash().unwrap();
        assert_eq!(trash.len(), 1);
        assert!(matches!(&trash[0].entry, VaultEntry::File(f) if f.id == spared.id));
        // The spared one can still come back, which is the whole point of
        // it having been spared.
        assert!(vfs.restore_file(spared.id).is_ok());
    }

    #[test]
    fn purging_one_of_two_rows_sharing_content_does_not_release_it() {
        // A concurrent edit resolves into a conflict copy, and that copy is a
        // second row carrying the *same* blob id. The caller deletes whatever
        // ids come back from the local cache and from every bucket, so
        // handing it a blob the survivor still points at loses the survivor's
        // content everywhere at once, with nothing left to fetch it from.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let original = vfs
            .add_file(root, "report.pdf", Uuid::new_v4(), 100, "aa", None, "k")
            .unwrap();
        let copy = vfs
            .add_file(
                root,
                "report (copy).pdf",
                original.blob_id,
                100,
                "aa",
                None,
                "k",
            )
            .unwrap();

        vfs.trash_file(copy.id).unwrap();
        let (removed, blob_ids) = vfs.purge_items(&[copy.id]).unwrap();

        assert_eq!(removed, 1);
        assert!(
            blob_ids.is_empty(),
            "released content the surviving row still points at: {blob_ids:?}"
        );

        // And once the last row goes, it really is released.
        vfs.trash_file(original.id).unwrap();
        let (_, blob_ids) = vfs.purge_items(&[original.id]).unwrap();
        assert_eq!(blob_ids, vec![original.blob_id]);
    }

    #[test]
    fn emptying_the_trash_keeps_content_a_live_row_shares() {
        // Same rule through the other door. Emptying the trash collects every
        // trashed file's blob id, so a shared one has to be filtered there
        // too or the bulk action is the dangerous one.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let kept = vfs
            .add_file(root, "kept.pdf", Uuid::new_v4(), 100, "aa", None, "k")
            .unwrap();
        let copy = vfs
            .add_file(root, "copy.pdf", kept.blob_id, 100, "aa", None, "k")
            .unwrap();
        let alone = vfs
            .add_file(root, "alone.pdf", Uuid::new_v4(), 10, "bb", None, "k")
            .unwrap();
        vfs.trash_file(copy.id).unwrap();
        vfs.trash_file(alone.id).unwrap();

        let (removed, blob_ids) = vfs.empty_trash().unwrap();

        assert_eq!(removed, 2);
        assert_eq!(
            blob_ids,
            vec![alone.blob_id],
            "the shared blob must survive with the row that still uses it"
        );
    }

    #[test]
    fn purging_a_trashed_folder_takes_everything_under_it() {
        // The trash lists the folder and hides its contents, so picking the
        // folder has to mean the folder as the user last saw it. Leaving the
        // children behind would also orphan rows pointing at a parent that
        // no longer exists.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let parent = vfs.create_folder(root, "Parent").unwrap();
        let child = vfs.create_folder(parent.id, "Child").unwrap();
        let deep = vfs
            .add_file(child.id, "deep.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        vfs.trash_folder(parent.id).unwrap();

        let (removed, blob_ids) = vfs.purge_items(&[parent.id]).unwrap();
        assert_eq!(removed, 3, "parent, child folder and the file in it");
        assert_eq!(blob_ids, vec![deep.blob_id]);
        assert!(vfs.list_trash().unwrap().is_empty());
    }

    #[test]
    fn purging_something_that_is_not_in_the_trash_does_nothing() {
        // The only way to name one is a stale list, which is a race rather
        // than a mistake worth failing the whole batch over.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let live = vfs
            .add_file(root, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();

        let (removed, blobs) = vfs.purge_items(&[live.id, Uuid::new_v4()]).unwrap();
        assert_eq!(removed, 0);
        assert!(blobs.is_empty());
        assert!(vfs.get_file(live.id).is_ok(), "a live file is untouched");
    }

    #[test]
    fn empty_trash_handles_nested_trashed_folders() {
        // Regression test: `folders.parent_id` self-references `folders.id`,
        // so bulk-deleting a whole trashed subtree in one statement can hit
        // a parent row before its child row and trip the FK check.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let parent = vfs.create_folder(root, "Parent").unwrap();
        let child = vfs.create_folder(parent.id, "Child").unwrap();
        let file = vfs
            .add_file(child.id, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();

        vfs.trash_folder(parent.id).unwrap();

        let (removed, blob_ids) = vfs.empty_trash().unwrap();
        assert_eq!(removed, 3); // parent + child folder + file
        assert_eq!(blob_ids, vec![file.blob_id]);
        assert!(vfs.list_trash().unwrap().is_empty());
        assert!(matches!(
            vfs.restore_folder(child.id).unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn blob_sizes_cover_live_files_only_and_count_shared_content_once() {
        // What a "download everything" figure is built from: trashed content
        // is not owed to this device, and one blob behind two rows is one
        // download, not two.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let kept = vfs
            .add_file(root, "a.txt", Uuid::new_v4(), 100, "aa", None, "")
            .unwrap();
        let gone = vfs
            .add_file(root, "b.txt", Uuid::new_v4(), 500, "bb", None, "")
            .unwrap();
        vfs.trash_file(gone.id).unwrap();
        // A second row pointing at the same content, as a copy would leave.
        vfs.add_file(root, "c.txt", kept.blob_id, 100, "aa", None, "")
            .unwrap();

        let sizes = vfs.list_blob_sizes().unwrap();
        assert_eq!(sizes, vec![(kept.blob_id, 100)]);
    }

    #[test]
    fn subtree_blob_ids_covers_the_folder_and_everything_under_it() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let work = vfs.create_folder(root, "Work").unwrap();
        let deep = vfs.create_folder(work.id, "Deep").unwrap();
        let top = vfs
            .add_file(work.id, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        let nested = vfs
            .add_file(deep.id, "b.txt", Uuid::new_v4(), 1, "bb", None, "")
            .unwrap();

        let mut found = vfs.subtree_blob_ids(work.id).unwrap();
        found.sort();
        let mut expected = vec![top.blob_id, nested.blob_id];
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn subtree_blob_ids_does_not_leak_into_a_sibling_with_a_shared_prefix() {
        // "/Work" must not match "/Workshop" — the whole reason the LIKE
        // pattern carries a trailing slash.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let work = vfs.create_folder(root, "Work").unwrap();
        let workshop = vfs.create_folder(root, "Workshop").unwrap();
        let mine = vfs
            .add_file(work.id, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        vfs.add_file(workshop.id, "b.txt", Uuid::new_v4(), 1, "bb", None, "")
            .unwrap();

        assert_eq!(vfs.subtree_blob_ids(work.id).unwrap(), vec![mine.blob_id]);
    }

    // A folder name may contain `%` and `_`, and `sanitize` produces `_`
    // itself by replacing what Windows forbids. Both are `LIKE` wildcards, so
    // before the patterns were escaped every subtree operation reached into
    // unrelated folders whose names merely matched. One test per operation,
    // because they build their patterns in different places.

    #[test]
    fn a_percent_in_a_folder_name_does_not_trash_the_neighbours() {
        // The pattern is `/a%b/%`, so what it reaches is what lies *under* a
        // similarly named folder. Hence the subfolder: without one there is
        // nothing for the trailing wildcard to match, and the bug hides.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let wild = vfs.create_folder(root, "a%b").unwrap();
        let bystander = vfs.create_folder(root, "aXXXb").unwrap();
        let sub = vfs.create_folder(bystander.id, "sub").unwrap();
        vfs.add_file(sub.id, "theirs.txt", Uuid::new_v4(), 1, "bb", None, "")
            .unwrap();

        vfs.trash_folder(wild.id).unwrap();

        assert_eq!(
            vfs.list_folder(bystander.id).unwrap().len(),
            1,
            "/aXXXb/sub must survive"
        );
        assert_eq!(
            vfs.list_folder(sub.id).unwrap().len(),
            1,
            "and so must the file in it"
        );
    }

    #[test]
    fn a_percent_in_a_folder_name_does_not_restore_the_neighbours() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let wild = vfs.create_folder(root, "a%b").unwrap();
        let bystander = vfs.create_folder(root, "aXXXb").unwrap();
        let sub = vfs.create_folder(bystander.id, "sub").unwrap();
        vfs.trash_folder(wild.id).unwrap();
        vfs.trash_folder(sub.id).unwrap();

        vfs.restore_folder(wild.id).unwrap();

        let still_trashed = vfs.list_trash().unwrap();
        assert!(
            still_trashed
                .iter()
                .any(|item| matches!(&item.entry, VaultEntry::Folder(f) if f.id == sub.id)),
            "restoring /a%b must leave /aXXXb/sub in the trash: {still_trashed:?}"
        );
    }

    #[test]
    fn a_percent_in_a_folder_name_does_not_rewrite_the_neighbours_paths() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let wild = vfs.create_folder(root, "a%b").unwrap();
        let bystander = vfs.create_folder(root, "aQQQb").unwrap();
        let theirs = vfs.create_folder(bystander.id, "child").unwrap();
        assert_eq!(theirs.path, "/aQQQb/child");

        vfs.rename_folder(wild.id, "renamed").unwrap();

        assert_eq!(
            vfs.get_folder(theirs.id).unwrap().path,
            "/aQQQb/child",
            "a rename elsewhere must not move this folder"
        );
    }

    #[test]
    fn a_percent_in_a_folder_name_does_not_purge_the_neighbours() {
        // The worst of the set: purge collects ids into `VaultOp::Purge`, so
        // the wrong ones replicate to every device and take the blobs out of
        // the bucket too. Irreversible.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let wild = vfs.create_folder(root, "a%b").unwrap();
        let bystander = vfs.create_folder(root, "aZZZb").unwrap();
        let sub = vfs.create_folder(bystander.id, "sub").unwrap();
        let theirs = vfs
            .add_file(sub.id, "theirs.txt", Uuid::new_v4(), 1, "bb", None, "")
            .unwrap();
        vfs.trash_folder(wild.id).unwrap();

        let (removed, blob_ids) = vfs.purge_items(&[wild.id]).unwrap();

        assert_eq!(removed, 1, "only the empty /a%b");
        assert!(
            blob_ids.is_empty(),
            "no blob of a live folder may be handed to the purge: {blob_ids:?}"
        );
        assert_eq!(vfs.get_file(theirs.id).unwrap().name, "theirs.txt");
    }

    #[test]
    fn an_underscore_in_a_folder_name_does_not_collect_foreign_blobs() {
        // `_` matches any single character, so `/a_b/%` used to match
        // `/axb/deep`. Export and purge both read this list.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let wild = vfs.create_folder(root, "a_b").unwrap();
        let bystander = vfs.create_folder(root, "axb").unwrap();
        let deep = vfs.create_folder(bystander.id, "deep").unwrap();
        vfs.add_file(deep.id, "theirs.txt", Uuid::new_v4(), 1, "bb", None, "")
            .unwrap();

        assert!(
            vfs.subtree_blob_ids(wild.id).unwrap().is_empty(),
            "/a_b is empty"
        );
    }

    #[test]
    fn renaming_a_folder_with_a_non_ascii_name_keeps_child_paths_intact() {
        // SQLite's `substr` counts characters on a TEXT value while
        // `str::len` counts bytes, so a multi-byte folder name used to cut
        // its children's paths at the wrong offset. Names are normalised to
        // NFC rather than rejected, so this is an ordinary vault, not an
        // exotic one.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        // The name being replaced is the one that matters: `substr` skips
        // over the *old* prefix, so the multi-byte characters have to be in
        // the folder as it was before the rename.
        let parent = vfs.create_folder(root, "chitanțe").unwrap();
        let child = vfs.create_folder(parent.id, "2026").unwrap();
        let grandchild = vfs.create_folder(child.id, "ianuarie").unwrap();

        vfs.rename_folder(parent.id, "arhiva").unwrap();

        assert_eq!(vfs.get_folder(child.id).unwrap().path, "/arhiva/2026");
        assert_eq!(
            vfs.get_folder(grandchild.id).unwrap().path,
            "/arhiva/2026/ianuarie"
        );
    }

    #[test]
    fn subtree_blob_ids_skips_trashed_content() {
        // Export writes what the user can see; a trashed file is not that.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();

        let work = vfs.create_folder(root, "Work").unwrap();
        let kept = vfs
            .add_file(work.id, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        let gone = vfs
            .add_file(work.id, "b.txt", Uuid::new_v4(), 1, "bb", None, "")
            .unwrap();
        vfs.trash_file(gone.id).unwrap();

        assert_eq!(vfs.subtree_blob_ids(work.id).unwrap(), vec![kept.blob_id]);
    }

    #[test]
    fn search_finds_a_file_anywhere_in_the_tree() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();
        let deep = vfs.create_folder(root, "Work").unwrap();
        let deeper = vfs.create_folder(deep.id, "2026").unwrap();
        vfs.add_file(
            deeper.id,
            "invoice-april.pdf",
            Uuid::new_v4(),
            1,
            "aa",
            None,
            "",
        )
        .unwrap();

        let hits = vfs.search_entries("invoice", 50).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            hits[0].folder_path, "/Work/2026",
            "the path is what makes a hit actionable"
        );
    }

    #[test]
    fn search_ignores_case_and_matches_partial_names() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();
        vfs.add_file(
            root,
            "Tax Return 2025.pdf",
            Uuid::new_v4(),
            1,
            "aa",
            None,
            "",
        )
        .unwrap();

        assert_eq!(vfs.search_entries("tax", 50).unwrap().len(), 1);
        assert_eq!(vfs.search_entries("RETURN", 50).unwrap().len(), 1);
        assert_eq!(vfs.search_entries("2025", 50).unwrap().len(), 1);
    }

    #[test]
    fn a_wildcard_in_the_query_matches_itself_and_nothing_else() {
        // Unescaped, "%" would match every file in the vault — which reads
        // as "search is broken" rather than as an injection.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();
        vfs.add_file(root, "100% done.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        vfs.add_file(root, "other.txt", Uuid::new_v4(), 1, "bb", None, "")
            .unwrap();

        let hits = vfs.search_entries("%", 50).unwrap();
        assert_eq!(hits.len(), 1);
        let VaultEntry::File(ref f) = hits[0].entry else {
            panic!("expected a file")
        };
        assert_eq!(f.name, "100% done.txt");
    }

    #[test]
    fn search_skips_trashed_entries() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();
        let gone = vfs
            .add_file(root, "secret-plan.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        vfs.trash_file(gone.id).unwrap();

        assert!(vfs.search_entries("secret", 50).unwrap().is_empty());
    }

    #[test]
    fn passwords_live_outside_the_file_tree() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();
        let id = Uuid::new_v4();

        vfs.upsert_password(id, &format!(r#"{{"id":"{id}","service":"Bank"}}"#))
            .unwrap();

        assert_eq!(vfs.list_passwords().unwrap().len(), 1);
        // The store used to be a file hidden from the listing by name. It is
        // not a file at all now, so there is nothing to hide.
        assert!(vfs.list_folder(root).unwrap().is_empty());
        assert!(vfs.search_entries("Bank", 50).unwrap().is_empty());
    }

    #[test]
    fn saving_a_password_twice_replaces_it_rather_than_adding_a_second() {
        // The regression this table exists for: the store was rewritten as a
        // new file on every save, and a trashed entry keeps its name claim,
        // so each save produced "passwords (1).silo", "(2)", and left the
        // reader unable to find any of them.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let id = Uuid::new_v4();

        vfs.upsert_password(id, &format!(r#"{{"id":"{id}","password":"first"}}"#))
            .unwrap();
        vfs.upsert_password(id, &format!(r#"{{"id":"{id}","password":"second"}}"#))
            .unwrap();

        let rows = vfs.list_passwords().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("second"));
    }

    #[test]
    fn a_stored_password_is_not_readable_from_the_database_itself() {
        // The working copy of vault.db sits in plaintext on disk while the
        // silo is unlocked, so anything that reads that file, a backup agent
        // sweeping the working directory or a crash leaving it behind, must
        // not find credentials in it.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let id = Uuid::new_v4();
        let secret = "hunter2-correct-horse-battery";

        vfs.upsert_password(
            id,
            &format!(r#"{{"id":"{id}","service":"Bank","password":"{secret}"}}"#),
        )
        .unwrap();

        let stored: String = session
            .conn
            .query_row(
                "SELECT data FROM passwords WHERE id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            !stored.contains(secret),
            "the password is stored in the clear"
        );
        assert!(
            !stored.contains("Bank"),
            "the service name is stored in the clear"
        );

        // And it still round-trips for the panel that asked for it.
        let rows = vfs.list_passwords().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains(secret));
    }

    #[test]
    fn a_password_row_that_will_not_open_fails_loudly() {
        // Showing a shorter list with no explanation is the one outcome a
        // password manager must not have.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let id = Uuid::new_v4();

        vfs.upsert_password(id, &format!(r#"{{"id":"{id}"}}"#))
            .unwrap();
        session
            .conn
            .execute(
                "UPDATE passwords SET data = 'not-base64!!' WHERE id = ?1",
                [id.to_string()],
            )
            .unwrap();

        assert!(vfs.list_passwords().is_err());
    }

    #[test]
    fn deleting_a_password_removes_it_and_deleting_twice_is_not_an_error() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let id = Uuid::new_v4();

        vfs.upsert_password(id, &format!(r#"{{"id":"{id}"}}"#))
            .unwrap();
        vfs.delete_password(id).unwrap();
        assert!(vfs.list_passwords().unwrap().is_empty());

        // The caller asked for a state, and that state already holds.
        vfs.delete_password(id).unwrap();
    }

    #[test]
    fn search_finds_folders_and_lists_them_first() {
        // Someone typing a name that is both is nearly always after the
        // place, not the file.
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();
        vfs.add_file(root, "Receipts.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();
        vfs.create_folder(root, "Receipts").unwrap();

        let hits = vfs.search_entries("receipts", 50).unwrap();
        assert_eq!(hits.len(), 2);
        assert!(matches!(hits[0].entry, VaultEntry::Folder(_)));
    }

    #[test]
    fn an_empty_query_returns_nothing_rather_than_everything() {
        let (_dir, session) = new_session();
        let vfs = Vfs::new(&session);
        let root = vfs.root_folder_id().unwrap();
        vfs.add_file(root, "a.txt", Uuid::new_v4(), 1, "aa", None, "")
            .unwrap();

        assert!(vfs.search_entries("", 50).unwrap().is_empty());
        assert!(vfs.search_entries("   ", 50).unwrap().is_empty());
    }
}
