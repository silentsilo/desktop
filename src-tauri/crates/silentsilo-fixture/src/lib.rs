//! Builds a silo with something of every kind in it, and reads one back.
//! Development only. It exists so `fixtures/` can hold silos written by
//! past releases and every build can prove it still opens them.
//!
//! Two properties make it possible: storage can be a plain folder, so a
//! "bucket" commits to the repo, and a silo rebuilds from storage with
//! only a recovery code, so it runs headless.
//!
//! The corpus is awkward on purpose. Every entry has broken something or
//! is one platform away from it: the two Unicode spellings of a word,
//! names differing only in case, a name Windows refuses, replaced
//! content, things in the bin.

use std::path::{Path, PathBuf};

use silentsilo_core::CoreError;
use silentsilo_store::{ObjectStore, StoreConfig};
use silentsilo_vault::{VaultSession, save_recovery_envelope, unwrap_with_code};
use silentsilo_vfs::{Vfs, all_ops, pending_ops, replay};
use uuid::Uuid;

/// A stand-in for a wrapped content key.
///
/// The fixture never decrypts content, and a real wrapping carries a random
/// nonce, which would make the recorded bytes differ on every run and turn a
/// deterministic fixture into a coin toss.
const FIXTURE_BLOB_KEY: &str = "fixture-blob-key";

/// The code every fixture is sealed with. A fixed value on purpose: it is
/// written into the tests, and a fixture nobody can open is worse than none.
pub const FIXTURE_CODE: &str = "TEST-TEST-TEST-TEST-TEST-TEST-TEST-TEST";

/// Where the two halves of a fixture live under its directory.
pub fn silo_dir(root: &Path) -> PathBuf {
    root.join("silo")
}

pub fn store_dir(root: &Path) -> PathBuf {
    root.join("store")
}

fn store_at(root: &Path) -> Result<Box<dyn ObjectStore>, String> {
    StoreConfig::Folder {
        path: store_dir(root),
    }
    .open()
    .map_err(|e| e.to_string())
}

/// Writes a complete fixture: a silo, its storage, and a recovery envelope
/// that opens it.
pub async fn create(root: &Path) -> Result<(), String> {
    create_inner(root, false).await
}

/// The same corpus, compacted: a snapshot in the store and the records
/// below its horizon pruned. A separate fixture because the two prove
/// different things: without a committed compacted silo the snapshot
/// format has no golden bytes, and a break in it would pass every test
/// and then refuse a real archive.
pub async fn create_compacted(root: &Path) -> Result<(), String> {
    create_inner(root, true).await
}

async fn create_inner(root: &Path, compact: bool) -> Result<(), String> {
    std::fs::create_dir_all(store_dir(root)).map_err(|e| e.to_string())?;
    let vault_id = Uuid::from_u128(0x5117_0000_0000_0000_0000_0000_0000_0001);

    let session = VaultSession::provision(silo_dir(root), vault_id, "fixture-device-secret")
        .map_err(|e| e.to_string())?;
    Vfs::new(&session)
        .ensure_initialized()
        .map_err(|e| e.to_string())?;
    populate(&session)?;

    let store = store_at(root)?;
    silentsilo_sync::put_manifest(&*store, vault_id)
        .await
        .map_err(|e| e.to_string())?;

    // The content key wrapper, without which the blobs a future build finds
    // here are ciphertext it has no way into. A fixture that omits it is not
    // a backup, it is a tree.
    let kek_envelope =
        silentsilo_vault::wrap_kek_bytes(&session.kek, &session.dek).map_err(|e| e.to_string())?;
    silentsilo_sync::publish_content_kek(&*store, &kek_envelope)
        .await
        .map_err(|e| e.to_string())?;

    let pending = pending_ops(&session.conn).map_err(|e| e.to_string())?;
    silentsilo_sync::push_ops(&*store, &session.dek, &pending)
        .await
        .map_err(|e| e.to_string())?;

    if compact {
        // A horizon in the middle of the log, so the fixture holds both
        // halves of a compacted silo: state that exists only in the snapshot,
        // and records that still replay on top of it.
        let ops = all_ops(&session.conn).map_err(|e| e.to_string())?;
        let mut lamports: Vec<u64> = ops.iter().map(|r| r.lamport).collect();
        lamports.sort_unstable();
        lamports.dedup();
        let horizon = lamports[lamports.len() / 2];
        let snapshot = silentsilo_vfs::capture_at(&session.conn, vault_id, horizon)
            .map_err(|e| e.to_string())?;
        silentsilo_sync::publish_compaction(&*store, &session.dek, &snapshot, true)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Sealed under a known code rather than a security key, which is what
    // lets the fixture be opened by a test with no hardware attached.
    let envelope = silentsilo_vault::seal_under_code_for_fixtures(&session.dek, FIXTURE_CODE)
        .map_err(|e| e.to_string())?;
    save_recovery_envelope(&session.paths.root, &envelope).map_err(|e| e.to_string())?;
    silentsilo_sync::push_recovery_envelope(&*store, &envelope)
        .await
        .map_err(|e| e.to_string())?;

    session.backup_locally().map_err(|e| e.to_string())?;
    Ok(())
}

/// Rebuilds the silo from storage alone, using only the recovery code, and
/// returns what it found.
///
/// Deliberately not a copy of the silo folder: the question a fixture answers
/// is whether a build years later can reconstruct a vault from a bucket and a
/// piece of paper, which is the only path that matters when the machine is
/// gone.
pub async fn digest_from_store(root: &Path, code: &str) -> Result<Vec<String>, String> {
    let store = store_at(root)?;
    let manifest = silentsilo_sync::read_manifest(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no manifest in the fixture store".to_string())?;
    let envelope = silentsilo_sync::fetch_recovery_envelope(&*store)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no recovery envelope in the fixture store".to_string())?;
    let dek = unwrap_with_code(&envelope, code).map_err(|e| e.to_string())?;

    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    // The fixture's own KEK, taken from the store like any joining device.
    // Replay never opens content, so a missing one is not fatal here: a
    // fresh key lets the replay proceed and proves the tree, which is what
    // this fixture is for.
    let kek = match silentsilo_sync::fetch_content_kek(&*store)
        .await
        .map_err(|e| e.to_string())?
    {
        Some(envelope) => {
            silentsilo_vault::unwrap_kek_bytes(&envelope, &dek).map_err(|e| e.to_string())?
        }
        None => silentsilo_crypto::generate_content_kek(),
    };
    let session = VaultSession::provision_with_dek(
        scratch.path().to_path_buf(),
        manifest.vault_id,
        "fixture-replay-device",
        dek.clone(),
        kek,
    )
    .map_err(|e| e.to_string())?;
    Vfs::new(&session)
        .ensure_initialized()
        .map_err(|e| e.to_string())?;

    // A compacted silo starts from its snapshot, exactly as the app's own
    // recovery does: the records below the horizon are gone from storage,
    // and replaying only what is left onto an empty tree would rebuild the
    // tail of the history rather than the silo.
    if let Some(snapshot) = silentsilo_sync::latest_snapshot(&*store, &dek)
        .await
        .map_err(|e| e.to_string())?
    {
        silentsilo_vfs::snapshot::restore(&session.conn, &snapshot).map_err(|e| e.to_string())?;
    }

    let ops = silentsilo_sync::fetch_ops_from(&*store, &dek, 0)
        .await
        .map_err(|e| e.to_string())?;
    replay(&session.conn, ops).map_err(|e| e.to_string())?;

    digest(&session.conn).map_err(|e| e.to_string())
}

/// Everything the vault holds, as sorted lines.
///
/// Lives in silentsilo-vfs now, because a test restore asks the same
/// question of a live silo that a fixture asks of an old one, and two copies
/// of this would drift into disagreeing about what "the same vault" means.
pub use silentsilo_vfs::digest;

/// The corpus. Every line here is a case that has broken something, or is
/// one platform away from breaking it.
fn populate(session: &VaultSession) -> Result<(), String> {
    let vfs = Vfs::new(session);
    let err = |e: CoreError| e.to_string();
    let root = vfs.root_folder_id().map_err(err)?;

    let docs = vfs.create_folder(root, "Acte").map_err(err)?;
    let nested = vfs.create_folder(docs.id, "2026").map_err(err)?;
    // Diacritics, so the fixture fails if Unicode handling ever changes.
    let stamps = vfs.create_folder(root, "Ștampile").map_err(err)?;

    let contract = vfs
        .add_file(
            docs.id,
            "contract.pdf",
            Uuid::from_u128(0x11),
            2048,
            "hash-contract",
            Some("application/pdf"),
            FIXTURE_BLOB_KEY,
        )
        .map_err(err)?;
    let old = vfs
        .add_file(
            nested.id,
            "vechi.txt",
            Uuid::from_u128(0x12),
            10,
            "hash-old",
            None,
            FIXTURE_BLOB_KEY,
        )
        .map_err(err)?;
    vfs.add_file(
        root,
        "notițe.md",
        Uuid::from_u128(0x13),
        100,
        "hash-notes-first",
        Some("text/markdown"),
        FIXTURE_BLOB_KEY,
    )
    .map_err(err)?;
    // A file with no content at all: the size-zero edge that chunking gets
    // wrong first.
    vfs.add_file(
        stamps.id,
        "gol.bin",
        Uuid::from_u128(0x14),
        0,
        "hash-empty",
        None,
        FIXTURE_BLOB_KEY,
    )
    .map_err(err)?;
    // A leading dot is a name, not an extension.
    vfs.add_file(
        root,
        ".gitignore",
        Uuid::from_u128(0x15),
        7,
        "hash-dot",
        None,
        FIXTURE_BLOB_KEY,
    )
    .map_err(err)?;

    // Same name again, which replaces the content in place and supersedes
    // the first blob. Nothing collects that blob, which is exactly what M2
    // will have to deal with, and the fixture is where it will show up.
    vfs.add_file(
        root,
        "notițe.md",
        Uuid::from_u128(0x16),
        250,
        "hash-notes-second",
        Some("text/markdown"),
        FIXTURE_BLOB_KEY,
    )
    .map_err(err)?;

    vfs.set_file_favorite(contract.id, true).map_err(err)?;
    vfs.set_folder_favorite(docs.id, true).map_err(err)?;
    vfs.trash_file(old.id).map_err(err)?;

    vfs.upsert_password(
        Uuid::from_u128(0x21),
        r#"{"id":"00000000-0000-0000-0000-000000000021","service":"bank","username":"alex"}"#,
    )
    .map_err(err)?;
    vfs.upsert_password(
        Uuid::from_u128(0x22),
        r#"{"id":"00000000-0000-0000-0000-000000000022","service":"mail","username":"alex"}"#,
    )
    .map_err(err)?;

    // Renames: the record carries the new name, and replay must land on it,
    // not on the one it was created under.
    let draft = vfs
        .add_file(
            docs.id,
            "ciornă.txt",
            Uuid::from_u128(0x17),
            64,
            "hash-draft",
            None,
            FIXTURE_BLOB_KEY,
        )
        .map_err(err)?;
    vfs.rename_file(draft.id, "final.txt").map_err(err)?;
    let temp = vfs.create_folder(root, "Temp").map_err(err)?;
    vfs.rename_folder(temp.id, "Arhivă").map_err(err)?;

    // A folder in the bin with its file still inside it.
    let bin = vfs.create_folder(root, "Vechituri").map_err(err)?;
    vfs.add_file(
        bin.id,
        "ruginit.dat",
        Uuid::from_u128(0x18),
        5,
        "hash-rust",
        None,
        FIXTURE_BLOB_KEY,
    )
    .map_err(err)?;
    vfs.trash_folder(bin.id).map_err(err)?;

    // Trashed and brought back: the restore must undo the trash exactly.
    vfs.trash_file(draft.id).map_err(err)?;
    vfs.restore_file(draft.id).map_err(err)?;

    // Purged outright. The id must stay gone at replay, not come back as a
    // file whose blob no longer exists.
    let ephemeral = vfs
        .add_file(
            root,
            "efemer.tmp",
            Uuid::from_u128(0x19),
            3,
            "hash-tmp",
            None,
            FIXTURE_BLOB_KEY,
        )
        .map_err(err)?;
    vfs.trash_file(ephemeral.id).map_err(err)?;
    vfs.purge_items(&[ephemeral.id]).map_err(err)?;

    // A password that was deleted: deletion is a record, not an absence.
    vfs.upsert_password(
        Uuid::from_u128(0x23),
        r#"{"id":"00000000-0000-0000-0000-000000000023","service":"old","username":"x"}"#,
    )
    .map_err(err)?;
    vfs.delete_password(Uuid::from_u128(0x23)).map_err(err)?;

    // What a machine says about itself, and what someone renamed it to.
    // Decoration, but decoration that has to replay forever.
    vfs.announce_device(Some("FIXTURE-PC"), "Windows 11 Pro")
        .map_err(err)?;
    let this_device = silentsilo_vfs::device_id(&session.conn).map_err(err)?;
    vfs.set_device_label(this_device, "Biroul").map_err(err)?;

    // A name typed in decomposed Unicode (s + U+0326), the spelling macOS
    // produces. The stored name must come out precomposed, on every
    // platform, forever.
    vfs.add_file(
        root,
        "s\u{0326}iret.txt",
        Uuid::from_u128(0x1A),
        9,
        "hash-nfd",
        None,
        FIXTURE_BLOB_KEY,
    )
    .map_err(err)?;

    // Sanity: the corpus has to be big enough to be worth comparing.
    let ops = all_ops(&session.conn).map_err(err)?;
    if ops.len() < 25 {
        return Err(format!("the corpus is too thin: {} operations", ops.len()));
    }
    Ok(())
}
