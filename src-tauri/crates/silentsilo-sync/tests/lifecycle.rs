//! What a silo and its copies look like after a long life of ordinary use.
//!
//! The other suites here test one operation against a store. These drive
//! sequences: files added, renamed, replaced, trashed, purged, targets added
//! and removed in between, and after every sync they ask the only question
//! that matters about a backup.
//!
//! **Can a device that has never seen this silo rebuild it from this one
//! copy alone?**
//!
//! That question is the point. A bug shipped where a target added after the
//! first was offered nothing and reported itself up to date; every function
//! involved was correct on its own, and no test asked what the copy actually
//! held. Asking it here is what makes the class of bug visible.
//!
//! The sync below deliberately mirrors `commands/sync.rs` rather than
//! calling `sync_ops`: the bug was in *how the queue was computed*, so a
//! harness that takes the queue as a parameter cannot see it.

use rusqlite::Connection;
use silentsilo_crypto::MasterDek;
use silentsilo_store::{FolderStore, ObjectStore};
use silentsilo_sync::{fetch_ops_from, push_ops, push_pending_blobs, put_manifest};
use silentsilo_vault::{
    VaultSession, list_undelivered_blob_ids, record_blob_present, settle_blob_delivery,
};
use silentsilo_vfs::{
    Vfs, digest, digest_difference, mark_delivered, pending_ops_for, replay, settle_delivery,
};
use uuid::Uuid;

/// One place a silo backs up to.
struct Target {
    id: Uuid,
    _dir: tempfile::TempDir,
    store: FolderStore,
}

impl Target {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        Self {
            id: Uuid::new_v4(),
            _dir: dir,
            store,
        }
    }

    async fn keys(&self, prefix: &str) -> Vec<String> {
        self.store
            .list(prefix)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.key)
            .collect()
    }
}

/// A silo with its own vault, its blobs on disk, and a list of copies.
struct Silo {
    _dir: tempfile::TempDir,
    session: VaultSession,
    dek: MasterDek,
    vault_id: Uuid,
    targets: Vec<Target>,
    /// What each file was before it was encrypted, so a copy can be made
    /// to hand the bytes back rather than merely list a name.
    plaintext: std::collections::HashMap<Uuid, Vec<u8>>,
}

impl Silo {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let vault_id = Uuid::new_v4();
        let session =
            VaultSession::provision(dir.path().to_path_buf(), vault_id, "test-secret").unwrap();
        Vfs::new(&session).ensure_initialized().unwrap();
        Self {
            _dir: dir,
            dek: session.dek.clone(),
            session,
            vault_id,
            targets: Vec::new(),
            plaintext: std::collections::HashMap::new(),
        }
    }

    fn vfs(&self) -> Vfs<'_> {
        Vfs::new(&self.session)
    }

    fn conn(&self) -> &Connection {
        &self.session.conn
    }

    fn root(&self) -> Uuid {
        self.vfs().root_folder_id().unwrap()
    }

    /// Writes a file the way an import does, encryption included: a content
    /// key of its own, the blob encrypted under it, that key wrapped under
    /// the silo's KEK and carried in the record. Real ciphertext rather than
    /// stand-in bytes, so the checks below exercise the whole chain from the
    /// vault key down to the plaintext.
    fn add_file(&mut self, folder: Uuid, name: &str, contents: &[u8]) -> Uuid {
        let blob_id = Uuid::new_v4();
        let file_id = Uuid::new_v4();
        let blobs = self.session.paths.root.join("blobs");
        std::fs::create_dir_all(&blobs).unwrap();

        let plain_dir = tempfile::tempdir().unwrap();
        let plain = plain_dir.path().join("in");
        std::fs::write(&plain, contents).unwrap();

        let key = silentsilo_crypto::generate_content_key();
        let result = silentsilo_crypto::encrypt_file(
            &plain,
            &blobs.join(format!("{blob_id}.sslo")),
            &key,
            file_id,
            blob_id,
        )
        .unwrap();
        let wrapped = silentsilo_crypto::wrap_content_key(&key, &self.session.kek).unwrap();

        record_blob_present(
            &self.session.paths.root,
            blob_id,
            result.size_bytes as i64,
            false,
        )
        .unwrap();
        let entry = self
            .vfs()
            .add_file(
                folder,
                name,
                blob_id,
                contents.len() as i64,
                &result
                    .header
                    .content_hash
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                None,
                &wrapped,
            )
            .unwrap();
        self.plaintext.insert(entry.id, contents.to_vec());
        entry.id
    }

    fn add_target(&mut self) -> Uuid {
        let target = Target::new();
        let id = target.id;
        self.targets.push(target);
        id
    }

    fn remove_target(&mut self, id: Uuid) {
        self.targets.retain(|t| t.id != id);
    }

    fn target_ids(&self) -> Vec<Uuid> {
        self.targets.iter().map(|t| t.id).collect()
    }

    /// One sync pass, as `commands/sync.rs` runs it: per target, ask what it
    /// is owed, send that, note the delivery, then settle across every
    /// configured target.
    async fn sync(&mut self) {
        let ids = self.target_ids();
        let base = silentsilo_vfs::read_base(self.conn()).unwrap();
        for target in &self.targets {
            put_manifest(&target.store, self.vault_id).await.unwrap();

            // Before any record: a log this device compacted starts part
            // way through, and a copy holding only that is one a joining
            // device reads as a smaller vault.
            if let Some(base) = &base {
                silentsilo_sync::publish_base_if_missing(&target.store, &self.dek, base)
                    .await
                    .unwrap();
            }

            let owed = pending_ops_for(self.conn(), target.id).unwrap();
            push_ops(&target.store, &self.dek, &owed).await.unwrap();
            mark_delivered(self.conn(), target.id, &owed).unwrap();

            push_pending_blobs(&target.store, &self.session.paths.root, target.id)
                .await
                .unwrap();
        }
        settle_delivery(&mut self.session.conn, &ids).unwrap();
        settle_blob_delivery(&self.session.paths.root, &ids).unwrap();
    }

    /// Compaction as the app runs it: capture, publish to every target,
    /// then drop what the snapshot covers from the local log.
    async fn compact(&mut self) {
        let policy = silentsilo_vfs::CompactionPolicy {
            retain_seconds: 0,
            keep_recent: 1,
            min_records: 2,
        };
        let now = 2_000_000_000;
        let Ok(Some(snapshot)) =
            silentsilo_sync::plan_compaction(self.conn(), self.vault_id, &policy, now)
        else {
            return;
        };
        for target in &self.targets {
            silentsilo_sync::publish_compaction(&target.store, &self.dek, &snapshot, true)
                .await
                .unwrap();
        }
        silentsilo_sync::finish_compaction(&mut self.session.conn, &snapshot).unwrap();
    }

    fn tree(&self) -> Vec<String> {
        digest(self.conn()).unwrap()
    }

    /// The blobs the live tree still points at, which is what every copy
    /// must hold for a restore to produce files rather than names.
    fn referenced_blobs(&self) -> std::collections::HashSet<Uuid> {
        silentsilo_vfs::referenced_blobs(self.conn()).unwrap()
    }

    /// The question every copy has to answer: rebuild from this one alone
    /// and compare. Content is checked separately, because a tree of names
    /// with no bytes behind it restores nothing.
    async fn assert_every_copy_is_whole(&self, when: &str) {
        let expected = self.tree();
        let needed = self.referenced_blobs();

        for target in &self.targets {
            let newcomer_dir = tempfile::tempdir().unwrap();
            let newcomer = VaultSession::provision(
                newcomer_dir.path().to_path_buf(),
                self.vault_id,
                "other-secret",
            )
            .unwrap();
            Vfs::new(&newcomer).ensure_initialized().unwrap();

            // Exactly the two ways in a real joining device has: from the
            // snapshot plus what followed, or from the whole log when the
            // silo has never been compacted.
            let mut newcomer = newcomer;
            match silentsilo_sync::fetch_rebuild(&target.store, &self.dek)
                .await
                .unwrap()
            {
                Some((snapshot, tail)) => {
                    silentsilo_sync::apply_rebuild(&mut newcomer.conn, &snapshot, tail).unwrap();
                }
                None => {
                    let records = fetch_ops_from(&target.store, &self.dek, 0).await.unwrap();
                    replay(&newcomer.conn, records).unwrap();
                }
            }

            let rebuilt = digest(&newcomer.conn).unwrap();
            let differences = digest_difference(&expected, &rebuilt);
            assert!(
                differences.is_empty(),
                "{when}: a copy cannot rebuild the silo. Missing or wrong: {differences:?}"
            );

            let present: std::collections::HashSet<Uuid> = target
                .keys("blobs/")
                .await
                .iter()
                .filter_map(|k| {
                    k.strip_prefix("blobs/")
                        .and_then(|n| n.strip_suffix(".sslo"))
                        .and_then(|n| Uuid::parse_str(n).ok())
                })
                .collect();
            let missing: Vec<&Uuid> = needed.difference(&present).collect();
            assert!(
                missing.is_empty(),
                "{when}: a copy is missing {} of {} files' content: {missing:?}",
                missing.len(),
                needed.len()
            );

            self.assert_files_decrypt(&newcomer, &target.store, when)
                .await;
        }
    }

    /// Every live file comes back as the bytes that went in.
    ///
    /// The whole chain, from the far end: the rebuilt index gives the
    /// wrapped content key, the silo's KEK unwraps it, the object comes
    /// down from the copy, and the plaintext has to match what was
    /// imported. A tree of correct names over ciphertext nobody can open is
    /// the failure this catches, and no amount of listing would.
    async fn assert_files_decrypt(&self, newcomer: &VaultSession, store: &FolderStore, when: &str) {
        let rebuilt = Vfs::new(newcomer);
        for (file_id, expected) in &self.plaintext {
            let Ok(entry) = rebuilt.get_file(*file_id) else {
                // Trashed and purged, so no longer part of the tree.
                continue;
            };
            let wrapped = rebuilt.blob_key(*file_id).unwrap();
            assert!(
                !wrapped.is_empty(),
                "{when}: the rebuilt index has no content key for {}",
                entry.name
            );
            let key = silentsilo_crypto::unwrap_content_key(&wrapped, &self.session.kek).unwrap();

            let bytes = silentsilo_sync::fetch_blob_bytes(store, entry.blob_id)
                .await
                .unwrap();
            let dir = tempfile::tempdir().unwrap();
            let sealed = dir.path().join("blob.sslo");
            let out = dir.path().join("out");
            std::fs::write(&sealed, &bytes).unwrap();

            silentsilo_crypto::decrypt_blob(&sealed, &out, &key, entry.blob_id).unwrap_or_else(
                |e| panic!("{when}: {} would not decrypt from a copy: {e}", entry.name),
            );
            assert_eq!(
                &std::fs::read(&out).unwrap(),
                expected,
                "{when}: {} came back as different bytes",
                entry.name
            );
        }
    }

    /// Nothing is owed once a pass has finished, which is what the screen
    /// reports as "up to date". Checked separately from wholeness, because
    /// the bug was exactly these two disagreeing.
    fn assert_nothing_owed(&self, when: &str) {
        for target in &self.targets {
            assert_eq!(
                pending_ops_for(self.conn(), target.id).unwrap().len(),
                0,
                "{when}: records still owed to a copy"
            );
            assert!(
                list_undelivered_blob_ids(&self.session.paths.root, target.id).is_empty(),
                "{when}: content still owed to a copy"
            );
        }
    }
}

#[tokio::test]
async fn a_target_added_after_the_silo_was_filled_receives_everything() {
    // The shipped bug, end to end: the copy reported itself up to date and
    // held nothing but metadata.
    let mut silo = Silo::new();
    let root = silo.root();
    for i in 0..5 {
        silo.add_file(root, &format!("early-{i}.txt"), &[i as u8; 64]);
    }

    silo.add_target();
    silo.sync().await;
    silo.assert_every_copy_is_whole("first target, filled beforehand")
        .await;
    silo.assert_nothing_owed("after the first pass");
}

#[tokio::test]
async fn a_second_target_receives_the_history_not_only_what_follows() {
    let mut silo = Silo::new();
    let root = silo.root();
    silo.add_file(root, "before.txt", b"old");
    silo.add_target();
    silo.sync().await;

    silo.add_target();
    silo.add_file(root, "after.txt", b"new");
    silo.sync().await;

    silo.assert_every_copy_is_whole("a second copy added later")
        .await;
    silo.assert_nothing_owed("after the second pass");
}

#[tokio::test]
async fn replacing_a_target_leaves_the_new_one_whole() {
    // Add, fill, remove, add another: the sequence a user runs when they
    // move from one provider to another.
    let mut silo = Silo::new();
    let root = silo.root();
    silo.add_file(root, "kept.txt", b"one");
    let first = silo.add_target();
    silo.sync().await;

    silo.remove_target(first);
    silo.add_target();
    silo.add_file(root, "later.txt", b"two");
    silo.sync().await;

    silo.assert_every_copy_is_whole("after replacing the copy")
        .await;
    silo.assert_nothing_owed("after replacing the copy");
}

#[tokio::test]
async fn a_copy_survives_the_whole_range_of_edits() {
    // Everything a user does to a tree, then the question.
    let mut silo = Silo::new();
    let root = silo.root();
    silo.add_target();

    let folder = silo.vfs().create_folder(root, "Documents").unwrap().id;
    let keep = silo.add_file(folder, "keep.txt", b"keep");
    let rename = silo.add_file(folder, "rename.txt", b"rename");
    let trash = silo.add_file(folder, "trash.txt", b"trash");
    let purge = silo.add_file(root, "purge.txt", b"purge");
    silo.sync().await;

    silo.vfs().rename_file(rename, "renamed.txt").unwrap();
    silo.vfs().trash_file(trash).unwrap();
    silo.vfs().purge_items(&[purge]).unwrap();
    silo.vfs()
        .upsert_password(Uuid::new_v4(), "sealed-entry")
        .unwrap();
    silo.vfs().set_file_favorite(keep, true).unwrap();
    silo.vfs().rename_folder(folder, "Papers").unwrap();
    silo.sync().await;

    silo.assert_every_copy_is_whole("after a range of edits")
        .await;
    silo.assert_nothing_owed("after a range of edits");
}

#[tokio::test]
async fn three_copies_stay_in_step_through_a_long_sequence() {
    // Targets coming and going while the tree keeps changing, which is the
    // shape of the bug: state that drifts over a sequence rather than
    // breaking in one call.
    let mut silo = Silo::new();
    let root = silo.root();

    let a = silo.add_target();
    silo.add_file(root, "one.txt", b"1");
    silo.sync().await;

    let b = silo.add_target();
    silo.add_file(root, "two.txt", b"2");
    silo.sync().await;

    silo.remove_target(a);
    silo.add_file(root, "three.txt", b"3");
    silo.sync().await;

    let c = silo.add_target();
    silo.add_file(root, "four.txt", b"4");
    silo.sync().await;

    silo.remove_target(b);
    silo.remove_target(c);
    silo.add_target();
    silo.add_file(root, "five.txt", b"5");
    silo.sync().await;

    silo.assert_every_copy_is_whole("after targets came and went")
        .await;
    silo.assert_nothing_owed("after targets came and went");
}

#[tokio::test]
async fn a_pass_with_nothing_to_do_leaves_every_copy_whole() {
    // "Already up to date" has to mean it. Syncing twice with no changes
    // in between must not be how a copy quietly falls behind.
    let mut silo = Silo::new();
    let root = silo.root();
    silo.add_target();
    silo.add_file(root, "only.txt", b"x");
    silo.sync().await;
    silo.sync().await;
    silo.sync().await;

    silo.assert_every_copy_is_whole("after repeated empty passes")
        .await;
    silo.assert_nothing_owed("after repeated empty passes");
}

#[tokio::test]
async fn removing_every_copy_leaves_the_content_pinned_locally() {
    // With no copy configured nothing is backed up, so nothing may be
    // treated as evictable: that is how a local file gets thrown away with
    // no copy anywhere.
    let mut silo = Silo::new();
    let root = silo.root();
    let only = silo.add_target();
    silo.add_file(root, "precious.txt", b"irreplaceable");
    silo.sync().await;

    silo.remove_target(only);
    settle_blob_delivery(&silo.session.paths.root, &[]).unwrap();

    assert!(
        !silentsilo_vault::list_unsynced_blob_ids(&silo.session.paths.root).is_empty(),
        "with no copy configured, no blob may be marked safe to evict"
    );
}

#[tokio::test]
async fn a_target_added_after_a_compaction_can_still_be_joined_from() {
    // The sharpest version of the problem. Once the log has been compacted
    // the records below the horizon are gone from this device, so a copy
    // added afterwards can only be whole if it receives the snapshot that
    // stands in for them.
    let mut silo = Silo::new();
    let root = silo.root();
    let first = silo.add_target();
    for i in 0..6 {
        silo.add_file(root, &format!("old-{i}.txt"), &[i as u8; 32]);
    }
    silo.sync().await;
    silo.compact().await;

    silo.add_target();
    silo.add_file(root, "after.txt", b"new");
    silo.sync().await;

    silo.assert_every_copy_is_whole("a copy added after a compaction")
        .await;
    silo.assert_nothing_owed("a copy added after a compaction");
    let _ = first;
}

#[tokio::test]
async fn compaction_leaves_every_copy_joinable() {
    let mut silo = Silo::new();
    let root = silo.root();
    silo.add_target();
    silo.add_target();
    for i in 0..6 {
        silo.add_file(root, &format!("f-{i}.txt"), &[i as u8; 16]);
    }
    silo.sync().await;

    silo.compact().await;
    silo.add_file(root, "post.txt", b"after the horizon");
    silo.sync().await;

    silo.assert_every_copy_is_whole("after compacting with two copies")
        .await;
}

/// A generator rather than a dependency, so a failure is reproducible from
/// the seed printed with it.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Sequences nobody thought to write by hand.
///
/// Every seed drives a different order of ordinary actions, with copies
/// added and removed along the way, and after each sync asks the same
/// question: can every copy rebuild this silo on its own? The hand-written
/// tests above cover the cases that were reasoned about; this covers the
/// ones that were not.
#[tokio::test]
async fn any_order_of_ordinary_use_leaves_every_copy_whole() {
    for seed in 0..40u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut silo = Silo::new();
        let root = silo.root();
        let mut folders = vec![root];
        let mut files: Vec<Uuid> = Vec::new();
        let mut live_targets: Vec<Uuid> = Vec::new();
        silo.add_target();
        live_targets.push(silo.target_ids()[0]);

        for step in 0..25 {
            match rng.below(9) {
                0 => {
                    let parent = folders[rng.below(folders.len())];
                    if let Ok(folder) = silo.vfs().create_folder(parent, &format!("d{step}")) {
                        folders.push(folder.id);
                    }
                }
                1 | 2 => {
                    let parent = folders[rng.below(folders.len())];
                    files.push(silo.add_file(parent, &format!("f{step}.txt"), &[step as u8; 24]));
                }
                3 if !files.is_empty() => {
                    let file = files[rng.below(files.len())];
                    let _ = silo.vfs().rename_file(file, &format!("r{step}.txt"));
                }
                4 if !files.is_empty() => {
                    let file = files.remove(rng.below(files.len()));
                    let _ = silo.vfs().trash_file(file);
                }
                5 if !files.is_empty() => {
                    let file = files.remove(rng.below(files.len()));
                    let _ = silo.vfs().purge_items(&[file]);
                }
                6 => {
                    let _ = silo
                        .vfs()
                        .upsert_password(Uuid::new_v4(), &format!("sealed-{step}"));
                }
                7 => {
                    let id = silo.add_target();
                    live_targets.push(id);
                }
                8 if live_targets.len() > 1 => {
                    let gone = live_targets.remove(rng.below(live_targets.len()));
                    silo.remove_target(gone);
                }
                _ => {}
            }

            // Not every step: a pass after every single action would hide
            // the case where several changes queue up between passes.
            if rng.below(3) == 0 {
                silo.sync().await;
                silo.assert_every_copy_is_whole(&format!("seed {seed}, step {step}"))
                    .await;
            }
        }

        silo.sync().await;
        silo.assert_every_copy_is_whole(&format!("seed {seed}, final"))
            .await;
        silo.assert_nothing_owed(&format!("seed {seed}, final"));
    }
}

impl Silo {
    /// Rotation as the app runs it: stage the new key, re-seal every copy,
    /// then commit. Content is never re-encrypted; only the envelopes move.
    async fn rotate(&mut self) {
        let old = self.dek.clone();
        let new = silentsilo_crypto::generate_dek();
        silentsilo_vault::rotation::stage_rotation(
            &self.session.paths.root,
            &new,
            &self.session.kek,
            &old,
        )
        .unwrap();
        for target in &self.targets {
            silentsilo_sync::reseal_under_new_key(&target.store, &old, &new, &mut |_, _| {})
                .await
                .unwrap();
        }
        silentsilo_vault::rotation::commit_rotation(&self.session.paths.root).unwrap();
        self.dek = new;
    }
}

#[tokio::test]
async fn every_copy_still_opens_after_the_key_is_rotated() {
    // Rotation touches each copy in turn. Nothing here had ever been tested
    // with more than one, and a copy left under the old key is one nobody
    // can read afterwards.
    let mut silo = Silo::new();
    let root = silo.root();
    silo.add_target();
    silo.add_target();
    silo.add_file(root, "before.txt", b"written before the rotation");
    silo.sync().await;

    silo.rotate().await;

    silo.assert_every_copy_is_whole("straight after a rotation")
        .await;

    // And the silo keeps working: a file written afterwards reaches every
    // copy and comes back as itself.
    silo.add_file(root, "after.txt", b"written after the rotation");
    silo.sync().await;
    silo.assert_every_copy_is_whole("after rotating and writing again")
        .await;
    silo.assert_nothing_owed("after rotating and writing again");
}

#[tokio::test]
async fn a_copy_added_after_a_rotation_is_whole() {
    // The new copy receives records sealed under the new key, while the
    // older copy holds the same records resealed. Both have to rebuild.
    let mut silo = Silo::new();
    let root = silo.root();
    silo.add_target();
    silo.add_file(root, "old.txt", b"before everything");
    silo.sync().await;
    silo.rotate().await;

    silo.add_target();
    silo.add_file(root, "new.txt", b"after the rotation");
    silo.sync().await;

    silo.assert_every_copy_is_whole("a copy added after a rotation")
        .await;
    silo.assert_nothing_owed("a copy added after a rotation");
}
