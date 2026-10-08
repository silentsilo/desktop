//! Moving files and folders inside one silo: by drag and drop, Cut and
//! Paste, or "Move to…". Every move goes through core's `move_file` and
//! `move_folder`, which record the entries again under new ids and send the
//! old rows to the trash in one transaction, so nothing here touches the
//! database itself. A name already in the destination is never overwritten:
//! it is kept beside the moved one (core shows them as "name (2)"), or the
//! move of that one item is skipped.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use silentsilo_core::{CoreResult, VaultEntry};
use silentsilo_vfs::Vfs;
use silentsilo_vfs::names::fold;
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::state::{AppState, with_vfs};

/// One selected item, as the explorer knows it.
#[derive(Deserialize, Clone, Debug)]
pub struct MoveItem {
    /// "file" or "folder".
    pub kind: String,
    pub id: String,
}

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct MoveReport {
    pub moved: u32,
    /// What moved, for the activity log; the window has the names already.
    #[serde(skip)]
    pub moved_names: Vec<String>,
    /// Names left where they were because the destination had one already.
    pub skipped: Vec<String>,
    pub failed: Vec<MoveFailure>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct MoveFailure {
    pub name: String,
    pub reason: String,
}

/// The items whose name the destination already has, for the question
/// "keep both or skip" before anything moves.
#[tauri::command(async)]
pub fn vault_move_clashes(
    items: Vec<MoveItem>,
    folder_id: String,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    let dest = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    with_vfs(&state, |_session, vfs| clashes(vfs, &items, dest))
}

#[tauri::command(async)]
pub fn vault_move_entries(
    app: AppHandle,
    items: Vec<MoveItem>,
    folder_id: String,
    skip_clashes: bool,
    state: State<AppState>,
) -> Result<MoveReport, String> {
    let dest = Uuid::parse_str(&folder_id).map_err(|e| e.to_string())?;
    let (report, to) = with_vfs(&state, |_session, vfs| {
        let report = move_entries(vfs, &items, dest, skip_clashes)?;
        Ok((report, vfs.get_folder(dest)?.path))
    })?;
    log_moved(&app, &report, &to);
    Ok(report)
}

/// One event per move, after it: by name when it is one item, otherwise the
/// count and the first names. Never refused, since the move is done.
fn log_moved(app: &AppHandle, report: &MoveReport, to: &str) {
    if report.moved == 0 {
        return;
    }
    let mut event = crate::audit::event(crate::audit::codes::FILE_MOVED).with("to", to);
    if let [one] = report.moved_names.as_slice() {
        event = event.with("name", one.clone());
    } else {
        event = event.with("count", report.moved).with(
            "names",
            report
                .moved_names
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>(),
        );
    }
    let _ = crate::audit::record(app, event);
}

/// What an item is called and where it is now.
struct Found {
    name: String,
    parent: Option<Uuid>,
}

fn find(vfs: &Vfs<'_>, item: &MoveItem) -> CoreResult<Found> {
    let id = parse(&item.id)?;
    if item.kind == "folder" {
        let folder = vfs.get_folder(id)?;
        Ok(Found {
            name: folder.name,
            parent: folder.parent_id,
        })
    } else {
        let file = vfs.get_file(id)?;
        Ok(Found {
            name: file.name,
            parent: Some(file.folder_id),
        })
    }
}

fn parse(id: &str) -> CoreResult<Uuid> {
    Uuid::parse_str(id).map_err(|e| silentsilo_core::CoreError::InvalidPath(e.to_string()))
}

/// The destination's names, compared the way core decides a clash.
fn names_in(vfs: &Vfs<'_>, dest: Uuid) -> CoreResult<HashSet<String>> {
    Ok(vfs
        .list_folder(dest)?
        .into_iter()
        .map(|entry| match entry {
            VaultEntry::Folder(f) => fold(&f.name),
            VaultEntry::File(f) => fold(&f.name),
        })
        .collect())
}

fn clashes(vfs: &Vfs<'_>, items: &[MoveItem], dest: Uuid) -> CoreResult<Vec<String>> {
    vfs.get_folder(dest)?;
    let taken = names_in(vfs, dest)?;
    let mut out = Vec::new();
    for item in items {
        let found = find(vfs, item)?;
        if found.parent != Some(dest) && taken.contains(&fold(&found.name)) {
            out.push(found.name);
        }
    }
    Ok(out)
}

/// Moves each item on its own: one that fails (a folder into itself, an
/// item gone meanwhile) is reported and the others still move. An item
/// already in the destination is left alone and not counted.
fn move_entries(
    vfs: &Vfs<'_>,
    items: &[MoveItem],
    dest: Uuid,
    skip_clashes: bool,
) -> CoreResult<MoveReport> {
    vfs.get_folder(dest)?;
    let mut taken = names_in(vfs, dest)?;
    let mut report = MoveReport::default();
    for item in items {
        let found = match find(vfs, item) {
            Ok(found) => found,
            Err(e) => {
                report.failed.push(MoveFailure {
                    name: item.id.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
        };
        if found.parent == Some(dest) {
            continue;
        }
        let key = fold(&found.name);
        if skip_clashes && taken.contains(&key) {
            report.skipped.push(found.name);
            continue;
        }
        let id = parse(&item.id)?;
        let moved = if item.kind == "folder" {
            vfs.move_folder(id, dest).map(|_| ())
        } else {
            vfs.move_file(id, dest).map(|_| ())
        };
        match moved {
            Ok(()) => {
                report.moved += 1;
                report.moved_names.push(found.name);
                taken.insert(key);
            }
            Err(e) => report.failed.push(MoveFailure {
                name: found.name,
                reason: e.to_string(),
            }),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use silentsilo_vault::VaultSession;

    struct Silo {
        _dir: tempfile::TempDir,
        session: VaultSession,
    }

    fn silo() -> Silo {
        let dir = tempfile::tempdir().unwrap();
        let session =
            VaultSession::provision(dir.path().join("silo"), Uuid::new_v4(), "s").unwrap();
        Vfs::new(&session).ensure_initialized().unwrap();
        Silo { _dir: dir, session }
    }

    fn file(vfs: &Vfs<'_>, folder: Uuid, name: &str) -> (Uuid, Uuid) {
        let blob = Uuid::new_v4();
        let f = vfs
            .add_file(folder, name, blob, 42, &format!("hash-{name}"), None, "key")
            .unwrap();
        (f.id, blob)
    }

    fn item(kind: &str, id: Uuid) -> MoveItem {
        MoveItem {
            kind: kind.into(),
            id: id.to_string(),
        }
    }

    /// Every live file as (path of its folder, name, blob, size, hash).
    fn files(vfs: &Vfs<'_>) -> Vec<(String, String, Uuid, i64, Option<String>)> {
        let mut out = Vec::new();
        for folder in vfs.list_all_folders().unwrap() {
            for entry in vfs.list_folder(folder.id).unwrap() {
                if let VaultEntry::File(f) = entry {
                    out.push((
                        folder.path.clone(),
                        f.name,
                        f.blob_id,
                        f.size_bytes,
                        f.content_hash,
                    ));
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn a_file_and_a_folder_tree_move_with_their_content_unchanged() {
        let s = silo();
        let vfs = Vfs::new(&s.session);
        let root = vfs.root_folder_id().unwrap();
        let docs = vfs.create_folder(root, "Docs").unwrap();
        let archive = vfs.create_folder(root, "Archive").unwrap();
        let inner = vfs.create_folder(docs.id, "2025").unwrap();
        let (a, blob_a) = file(&vfs, root, "a.pdf");
        file(&vfs, docs.id, "b.pdf");
        file(&vfs, inner.id, "c.pdf");
        let before = files(&vfs);

        let report = move_entries(
            &vfs,
            &[item("file", a), item("folder", docs.id)],
            archive.id,
            false,
        )
        .unwrap();

        assert_eq!(report.moved, 2);
        assert!(report.failed.is_empty() && report.skipped.is_empty());
        let after = files(&vfs);
        assert_eq!(after.len(), before.len(), "no file appears or disappears");
        let paths: Vec<_> = after.iter().map(|f| format!("{}/{}", f.0, f.1)).collect();
        assert!(paths.contains(&"/Archive/a.pdf".to_string()));
        assert!(paths.contains(&"/Archive/Docs/b.pdf".to_string()));
        assert!(paths.contains(&"/Archive/Docs/2025/c.pdf".to_string()));
        // The same content: blob, size and hash travel with the entry.
        let blobs = |v: &[(String, String, Uuid, i64, Option<String>)]| {
            let mut b: Vec<_> = v.iter().map(|f| (f.2, f.3, f.4.clone())).collect();
            b.sort();
            b
        };
        assert_eq!(blobs(&after), blobs(&before));
        assert!(after.iter().any(|f| f.2 == blob_a));
    }

    #[test]
    fn emptying_the_trash_after_a_move_keeps_the_moved_content() {
        let s = silo();
        let vfs = Vfs::new(&s.session);
        let root = vfs.root_folder_id().unwrap();
        let archive = vfs.create_folder(root, "Archive").unwrap();
        let (a, blob) = file(&vfs, root, "a.pdf");

        move_entries(&vfs, &[item("file", a)], archive.id, false).unwrap();
        let (_, released) = vfs.empty_trash().unwrap();

        assert!(
            !released.contains(&blob),
            "the moved file still uses the blob, so it is not handed out for deletion"
        );
        assert!(
            vfs.referenced_blobs_with_attachments()
                .unwrap()
                .contains(&blob)
        );
        assert_eq!(files(&vfs).len(), 1);
    }

    #[test]
    fn a_folder_cannot_go_into_itself_and_the_rest_still_moves() {
        let s = silo();
        let vfs = Vfs::new(&s.session);
        let root = vfs.root_folder_id().unwrap();
        let docs = vfs.create_folder(root, "Docs").unwrap();
        let inner = vfs.create_folder(docs.id, "Inner").unwrap();
        let (a, _) = file(&vfs, root, "a.pdf");
        let before = files(&vfs);

        let report = move_entries(
            &vfs,
            &[item("folder", docs.id), item("file", a)],
            inner.id,
            false,
        )
        .unwrap();

        assert_eq!(report.moved, 1);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].name, "Docs");
        assert_eq!(files(&vfs).len(), before.len());
        assert_eq!(
            vfs.get_folder(docs.id).unwrap().path,
            "/Docs",
            "left where it was"
        );
    }

    #[test]
    fn a_name_already_there_is_kept_beside_or_skipped_never_replaced() {
        let s = silo();
        let vfs = Vfs::new(&s.session);
        let root = vfs.root_folder_id().unwrap();
        let archive = vfs.create_folder(root, "Archive").unwrap();
        let (_, kept_blob) = file(&vfs, archive.id, "Report.pdf");
        let (a, a_blob) = file(&vfs, root, "report.pdf");

        assert_eq!(
            clashes(&vfs, &[item("file", a)], archive.id).unwrap(),
            vec!["report.pdf".to_string()],
            "compared the way core does: case and composition folded"
        );

        let skipped = move_entries(&vfs, &[item("file", a)], archive.id, true).unwrap();
        assert_eq!(skipped.skipped, vec!["report.pdf".to_string()]);
        assert_eq!(skipped.moved, 0);
        assert_eq!(
            vfs.get_file(a).unwrap().folder_id,
            root,
            "a skipped file stays put"
        );

        let both = move_entries(&vfs, &[item("file", a)], archive.id, false).unwrap();
        assert_eq!(both.moved, 1);
        let in_archive: Vec<_> = files(&vfs)
            .into_iter()
            .filter(|f| f.0 == "/Archive")
            .map(|f| f.2)
            .collect();
        assert_eq!(in_archive.len(), 2, "both are kept");
        assert!(in_archive.contains(&kept_blob) && in_archive.contains(&a_blob));
    }

    #[test]
    fn an_item_already_in_the_destination_is_left_alone() {
        let s = silo();
        let vfs = Vfs::new(&s.session);
        let root = vfs.root_folder_id().unwrap();
        let (a, _) = file(&vfs, root, "a.pdf");

        let report = move_entries(&vfs, &[item("file", a)], root, false).unwrap();

        assert_eq!(report, MoveReport::default());
        assert_eq!(
            vfs.get_file(a).unwrap().folder_id,
            root,
            "same id, not copied"
        );
        assert!(vfs.list_trash().unwrap().is_empty());
    }

    #[test]
    fn a_destination_in_the_trash_moves_nothing() {
        let s = silo();
        let vfs = Vfs::new(&s.session);
        let root = vfs.root_folder_id().unwrap();
        let gone = vfs.create_folder(root, "Gone").unwrap();
        vfs.trash_folder(gone.id).unwrap();
        let (a, _) = file(&vfs, root, "a.pdf");

        assert!(move_entries(&vfs, &[item("file", a)], gone.id, false).is_err());
        assert_eq!(vfs.get_file(a).unwrap().folder_id, root);
    }

    #[test]
    fn the_root_and_the_inbox_do_not_move() {
        let s = silo();
        let vfs = Vfs::new(&s.session);
        let root = vfs.root_folder_id().unwrap();
        let docs = vfs.create_folder(root, "Docs").unwrap();
        let mut items = vec![item("folder", root)];
        if let Ok(inbox) = vfs.folder_by_path("/Inbox") {
            items.push(item("folder", inbox.id));
        }

        let report = move_entries(&vfs, &items, docs.id, false).unwrap();

        assert_eq!(report.moved, 0);
        assert_eq!(report.failed.len(), items.len());
        assert_eq!(vfs.get_folder(root).unwrap().path, "/");
    }
}
