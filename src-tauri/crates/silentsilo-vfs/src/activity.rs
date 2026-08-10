//! What has happened to this silo, read back out of the operation log.
//!
//! Activity, not an audit log. Every record is written by a device holding
//! the vault key, so any of them can author whatever it likes and nothing
//! signs a record as coming from a person. The question it answers is the
//! ordinary one: what happened to my files, and which machine did it.
//!
//! Two things the display must be honest about. Order is the log's, not
//! the clock's: entries come out in Lamport order, and the timestamp
//! beside each is the writing machine's wall clock, a label that can be
//! wrong. And history can begin part way through, since compaction removes
//! records below the horizon; [`ActivityPage::truncated_before`] says
//! where the log now starts.
//!
//! No unlock or key-touch events: authentication is local to each device
//! and never enters the log.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use silentsilo_core::{CoreError, CoreResult};
use uuid::Uuid;

use crate::oplog::{OpBody, OpRecord, VaultOp};

fn db(e: rusqlite::Error) -> CoreError {
    CoreError::Database(e.to_string())
}

/// One thing that happened, ready to show.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityEntry {
    pub op_id: Uuid,
    /// Position in the order every device agrees on. What the list is sorted
    /// by, and what "before" and "after" mean here.
    pub lamport: u64,
    pub device_id: Uuid,
    /// Whatever this device is called, empty when nobody has named it.
    pub device_label: String,
    /// The author's wall clock. Shown as approximate, never used to sort.
    pub at: i64,
    /// A short phrase: "Added report.pdf", "Renamed Invoices to 2026".
    pub summary: String,
    /// True for a record this build stored but does not understand, which a
    /// newer version wrote. Shown rather than hidden: a gap the user cannot
    /// see is worse than one labelled as such.
    pub unknown: bool,
}

/// Where a page ended, so the next one starts exactly after it.
///
/// The full sort key rather than a row offset. A log grows at the top: new
/// records take higher Lamport values, so an offset counted from the newest
/// entry means every record written while someone is reading shifts the page
/// under them and shows a line twice or skips one. A key cannot drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityCursor {
    pub lamport: u64,
    pub device_id: Uuid,
    pub op_id: Uuid,
}

/// What to show.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ActivityQuery {
    /// Matched against what the list actually displays: the summary line and
    /// the device's name. Empty means everything.
    #[serde(default)]
    pub search: String,
    /// Where the previous page ended. `None` for the first one.
    #[serde(default)]
    pub after: Option<ActivityCursor>,
    #[serde(default)]
    pub limit: usize,
}

/// A window onto the log, and where the log begins.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityPage {
    pub entries: Vec<ActivityEntry>,
    /// Lamport value below which nothing is left, or 0 when the whole history
    /// is here. Non-zero means compaction has been through, and the view has
    /// to say so instead of implying the silo began there.
    pub truncated_before: u64,
    /// Where to continue from, or `None` when the log has been reached to the
    /// end. What a "show more" button is enabled by.
    pub next: Option<ActivityCursor>,
    /// How many records the log holds, when that is cheap to know.
    ///
    /// `None` while searching. A search is matched against text this module
    /// computes rather than a column, so counting the matches would mean
    /// decoding every record in the silo to answer a question nobody asked.
    /// A missing number is better than a slow one, and far better than a
    /// wrong one.
    pub total: Option<usize>,
}

/// The most recent `limit` entries, newest first.
pub fn recent(conn: &Connection, limit: usize) -> CoreResult<ActivityPage> {
    page(
        conn,
        &ActivityQuery {
            limit,
            ..ActivityQuery::default()
        },
    )
}

/// How many records a single search may decode before giving up and
/// offering to continue. A silo can hold hundreds of thousands, and
/// without a ceiling a search for something absent would freeze the window
/// for as long as the log is long.
const SEARCH_SCAN_LIMIT: usize = 5_000;

/// One page, newest first.
///
/// Newest first because that is what someone opening this wants: the question
/// is almost always "what just happened", and the answer to "what happened in
/// March" is a scroll either way.
pub fn page(conn: &Connection, query: &ActivityQuery) -> CoreResult<ActivityPage> {
    let limit = query.limit.clamp(1, 500);
    let needle = query.search.trim().to_lowercase();
    let searching = !needle.is_empty();

    // Read in chunks rather than all at once: without a search a single chunk
    // answers the page, and with one this is how the scan stays bounded.
    let chunk = if searching { limit.max(256) } else { limit };

    let mut entries = Vec::with_capacity(limit);
    // Where the scan has reached, which is also where the next page starts.
    // Advanced per record rather than per batch: when a page fills partway
    // through a batch, continuing from the end of that batch would silently
    // drop everything between the last line shown and the batch boundary.
    let mut cursor = query.after.clone();
    let mut scanned = 0usize;
    let mut exhausted = false;

    'scan: while entries.len() < limit && scanned < SEARCH_SCAN_LIMIT {
        let batch = read_chunk(conn, cursor.as_ref(), chunk)?;
        // Short of what was asked for means the log ended inside this batch.
        // Waiting for an empty one instead would offer a further page that
        // comes back with nothing.
        let short = batch.len() < chunk;
        scanned += batch.len();

        for entry in batch {
            cursor = Some(cursor_for(&entry));
            if !searching || matches(&entry, &needle) {
                entries.push(entry);
                if entries.len() == limit {
                    break 'scan;
                }
            }
        }

        if short {
            exhausted = true;
            break;
        }
        if !searching {
            break;
        }
    }

    // Offering to continue when there is demonstrably nothing left would be a
    // button that answers with an empty page.
    let next = if exhausted { None } else { cursor };

    let total = if searching {
        None
    } else {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM oplog", [], |row| row.get(0))
            .map_err(db)?;
        Some(count as usize)
    };

    let truncated_before: Option<i64> = conn
        .query_row("SELECT horizon FROM vault_base WHERE id = 0", [], |row| {
            row.get(0)
        })
        .optional()
        .map_err(db)?;

    Ok(ActivityPage {
        entries,
        truncated_before: truncated_before.unwrap_or(0) as u64,
        next,
        total,
    })
}

fn cursor_for(entry: &ActivityEntry) -> ActivityCursor {
    ActivityCursor {
        lamport: entry.lamport,
        device_id: entry.device_id,
        op_id: entry.op_id,
    }
}

/// Matched against the line as shown, not against the stored record.
///
/// Searching the payload would find the words this build happens to use for
/// its JSON keys and the hexadecimal of every id: typing "file" would match
/// every `AddFile` ever written. What the user is looking for is what they
/// can see.
fn matches(entry: &ActivityEntry, needle: &str) -> bool {
    entry.summary.to_lowercase().contains(needle)
        || entry.device_label.to_lowercase().contains(needle)
}

fn read_chunk(
    conn: &Connection,
    after: Option<&ActivityCursor>,
    limit: usize,
) -> CoreResult<Vec<ActivityEntry>> {
    let mut stmt = conn
        .prepare(
            "SELECT o.payload, COALESCE(d.label, '')
             FROM oplog o
             LEFT JOIN device_labels d ON d.device_id = o.device_id
             WHERE ?1 = 0
                OR (o.lamport, o.device_id, o.op_id) < (?2, ?3, ?4)
             ORDER BY o.lamport DESC, o.device_id DESC, o.op_id DESC
             LIMIT ?5",
        )
        .map_err(db)?;

    let (bounded, lamport, device, op) = match after {
        Some(c) => (
            1i64,
            c.lamport as i64,
            c.device_id.to_string(),
            c.op_id.to_string(),
        ),
        None => (0i64, 0i64, String::new(), String::new()),
    };

    let rows = stmt
        .query_map(
            rusqlite::params![bounded, lamport, device, op, limit as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(db)?;

    let mut out = Vec::new();
    for row in rows {
        let (payload, label) = row.map_err(db)?;
        let record = OpRecord::from_bytes(payload.as_bytes())?;
        out.push(entry_for(&record, label));
    }
    Ok(out)
}

fn entry_for(record: &OpRecord, device_label: String) -> ActivityEntry {
    let (summary, unknown) = match &record.op {
        OpBody::Known(op) => (describe(op), false),
        // Named rather than described: this build does not know what the
        // record means, and inventing a sentence for it would be a guess
        // presented as fact.
        OpBody::Unknown { op, .. } => (
            format!("Something this version does not know about ({op})"),
            true,
        ),
    };
    ActivityEntry {
        op_id: record.op_id,
        lamport: record.lamport,
        device_id: record.device_id,
        device_label,
        at: record.at,
        summary,
        unknown,
    }
}

/// One line for an operation.
///
/// Names are included because they are what makes the line useful, and the
/// person reading this already has the key that decrypts them. Credentials
/// are the exception: an entry's contents never appear, not even its service
/// name, because this list is the one screen in the app that shows a long
/// stretch of history at a glance and a shoulder is enough to read it.
fn describe(op: &VaultOp) -> String {
    match op {
        VaultOp::CreateFolder { name, .. } => format!("Created the folder {name}"),
        VaultOp::AddFile { name, .. } => format!("Added {name}"),
        VaultOp::ReplaceFileContent { .. } => "Replaced a file's contents".into(),
        VaultOp::RenameFolder { name, .. } => format!("Renamed a folder to {name}"),
        VaultOp::RenameFile { name, .. } => format!("Renamed a file to {name}"),
        VaultOp::TrashFolder { .. } => "Moved a folder to the trash".into(),
        VaultOp::TrashFile { .. } => "Moved a file to the trash".into(),
        VaultOp::RestoreFolder { .. } => "Restored a folder from the trash".into(),
        VaultOp::RestoreFile { .. } => "Restored a file from the trash".into(),
        VaultOp::Purge {
            folder_ids,
            file_ids,
        } => {
            let count = folder_ids.len() + file_ids.len();
            format!(
                "Deleted {count} item{} for good",
                if count == 1 { "" } else { "s" }
            )
        }
        VaultOp::UpsertPassword { .. } => "Saved a credential".into(),
        VaultOp::DeletePassword { .. } => "Deleted a credential".into(),
        VaultOp::SetFileFavorite { favorite, .. } => {
            if *favorite {
                "Starred a file".into()
            } else {
                "Unstarred a file".into()
            }
        }
        VaultOp::SetFolderFavorite { favorite, .. } => {
            if *favorite {
                "Starred a folder".into()
            } else {
                "Unstarred a folder".into()
            }
        }
        VaultOp::SetDeviceLabel { label, .. } => format!("Named a device {label}"),
        VaultOp::AnnounceDevice { platform, .. } => {
            format!("A device announced itself ({platform})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oplog::replay;
    use crate::oplog::tests::bare_device;
    use crate::{capture_at, compact_local, root_folder_id_for};

    fn record(lamport: u64, device_id: Uuid, op: VaultOp) -> OpRecord {
        OpRecord::authored(
            Uuid::new_v4(),
            lamport,
            device_id,
            1_700_000_000 + lamport as i64,
            lamport,
            None,
            op,
        )
    }

    fn silo() -> (Connection, Uuid, Uuid) {
        let vault_id = Uuid::new_v4();
        let conn = bare_device(vault_id);
        let root = root_folder_id_for(vault_id);
        let device = Uuid::new_v4();
        let folder = Uuid::new_v4();

        replay(
            &conn,
            vec![
                record(
                    1,
                    device,
                    VaultOp::CreateFolder {
                        id: folder,
                        parent_id: root,
                        name: "Invoices".into(),
                    },
                ),
                record(
                    2,
                    device,
                    VaultOp::AddFile {
                        id: Uuid::new_v4(),
                        folder_id: folder,
                        name: "march.pdf".into(),
                        blob_id: Uuid::new_v4(),
                        size_bytes: 1,
                        content_hash: "h".into(),
                        mime_type: None,
                        blob_key: String::new(),
                    },
                ),
                record(
                    3,
                    device,
                    VaultOp::UpsertPassword {
                        id: Uuid::new_v4(),
                        data: "sealed-bytes-for-my-bank".into(),
                    },
                ),
            ],
        )
        .unwrap();
        (conn, vault_id, device)
    }

    fn query(search: &str, after: Option<ActivityCursor>, limit: usize) -> ActivityQuery {
        ActivityQuery {
            search: search.into(),
            after,
            limit,
        }
    }

    /// A silo with `count` added files, newest last.
    fn many(count: u64) -> (Connection, Uuid) {
        let vault_id = Uuid::new_v4();
        let conn = bare_device(vault_id);
        let root = root_folder_id_for(vault_id);
        let device = Uuid::new_v4();
        let records: Vec<OpRecord> = (1..=count)
            .map(|i| {
                record(
                    i,
                    device,
                    VaultOp::AddFile {
                        id: Uuid::new_v4(),
                        folder_id: root,
                        name: format!("file-{i:03}.txt"),
                        blob_id: Uuid::new_v4(),
                        size_bytes: 1,
                        content_hash: "h".into(),
                        mime_type: None,
                        blob_key: String::new(),
                    },
                )
            })
            .collect();
        replay(&conn, records).unwrap();
        (conn, device)
    }

    #[test]
    fn a_page_continues_exactly_where_the_last_one_stopped() {
        let (conn, _) = many(10);

        let first = page(&conn, &query("", None, 4)).unwrap();
        let second = page(&conn, &query("", first.next.clone(), 4)).unwrap();

        let firsts: Vec<u64> = first.entries.iter().map(|e| e.lamport).collect();
        let seconds: Vec<u64> = second.entries.iter().map(|e| e.lamport).collect();
        assert_eq!(firsts, vec![10, 9, 8, 7]);
        assert_eq!(seconds, vec![6, 5, 4, 3], "the page skipped or repeated");
    }

    #[test]
    fn a_record_written_while_reading_does_not_shift_the_page() {
        // The reason the cursor is the sort key and not a row offset: a log
        // grows at the top, so counting from the newest entry would move
        // every page under the reader.
        let (conn, device) = many(6);
        let first = page(&conn, &query("", None, 3)).unwrap();

        replay(
            &conn,
            vec![record(
                99,
                device,
                VaultOp::CreateFolder {
                    id: Uuid::new_v4(),
                    parent_id: root_folder_id_for(
                        Uuid::parse_str(
                            &conn
                                .query_row(
                                    "SELECT value FROM vault_meta WHERE key = 'vault_id'",
                                    [],
                                    |r| r.get::<_, String>(0),
                                )
                                .unwrap(),
                        )
                        .unwrap(),
                    ),
                    name: "Arrived mid-read".into(),
                },
            )],
        )
        .unwrap();

        let second = page(&conn, &query("", first.next.clone(), 3)).unwrap();

        let seconds: Vec<u64> = second.entries.iter().map(|e| e.lamport).collect();
        assert_eq!(
            seconds,
            vec![3, 2, 1],
            "a new record at the top pushed the second page along"
        );
    }

    #[test]
    fn the_last_page_stops_offering_more() {
        let (conn, _) = many(4);

        let first = page(&conn, &query("", None, 3)).unwrap();
        let second = page(&conn, &query("", first.next.clone(), 3)).unwrap();

        assert!(first.next.is_some());
        assert_eq!(second.entries.len(), 1);
        assert!(
            second.next.is_none(),
            "offered a page that would have come back empty"
        );
    }

    #[test]
    fn a_search_matches_the_line_that_is_shown() {
        let (conn, _) = many(20);

        let found = page(&conn, &query("file-007", None, 50)).unwrap();

        assert_eq!(found.entries.len(), 1);
        assert_eq!(found.entries[0].summary, "Added file-007.txt");
    }

    #[test]
    fn a_search_does_not_match_the_stored_record() {
        // Searching the payload would find the words this build uses for its
        // JSON keys and the hexadecimal of every id: "AddFile" is in every
        // record ever written, and nobody typing it means that.
        let (conn, _) = many(5);

        let found = page(&conn, &query("AddFile", None, 50)).unwrap();

        assert!(
            found.entries.is_empty(),
            "matched the record's shape rather than what it says: {:?}",
            found.entries
        );
    }

    #[test]
    fn a_search_pages_like_everything_else() {
        let (conn, _) = many(30);

        // Nine matches out of thirty: file-001 to file-009. "file-0" would
        // have matched all thirty, which proves nothing about paging.
        let first = page(&conn, &query("file-00", None, 5)).unwrap();
        let second = page(&conn, &query("file-00", first.next.clone(), 5)).unwrap();

        assert_eq!(first.entries.len(), 5);
        assert_eq!(
            second.entries.len(),
            4,
            "file-001 to file-009 is nine lines"
        );
        let all: Vec<&str> = first
            .entries
            .iter()
            .chain(second.entries.iter())
            .map(|e| e.summary.as_str())
            .collect();
        assert_eq!(all.len(), 9);
        assert!(all.contains(&"Added file-001.txt"));
        assert!(all.contains(&"Added file-009.txt"));
    }

    #[test]
    fn a_total_is_offered_only_when_it_is_cheap() {
        // Counting matches would mean decoding every record in the silo to
        // answer a question nobody asked.
        let (conn, _) = many(7);

        assert_eq!(page(&conn, &query("", None, 3)).unwrap().total, Some(7));
        assert_eq!(page(&conn, &query("file", None, 3)).unwrap().total, None);
    }

    #[test]
    fn the_newest_change_comes_first() {
        let (conn, _, _) = silo();

        let page = recent(&conn, 10).unwrap();

        let order: Vec<u64> = page.entries.iter().map(|e| e.lamport).collect();
        assert_eq!(order, vec![3, 2, 1]);
        assert_eq!(page.total, Some(3));
        assert!(page.next.is_none());
    }

    #[test]
    fn names_are_shown_but_credentials_are_not() {
        // The one screen that puts a long stretch of history on display at
        // once, which is the wrong place for the contents of a login.
        let (conn, _, _) = silo();

        let page = recent(&conn, 10).unwrap();
        let summaries: Vec<&str> = page.entries.iter().map(|e| e.summary.as_str()).collect();

        assert!(summaries.contains(&"Added march.pdf"));
        assert!(summaries.contains(&"Created the folder Invoices"));
        assert!(summaries.contains(&"Saved a credential"));
        assert!(
            !summaries.iter().any(|s| s.contains("bank")),
            "a credential's contents reached the activity list: {summaries:?}"
        );
    }

    #[test]
    fn a_compacted_silo_says_where_its_history_starts() {
        // Otherwise the oldest line on screen reads as the beginning of the
        // silo, and it is only the beginning of what is left.
        let (mut conn, vault_id, _) = silo();
        conn.execute("UPDATE oplog SET pushed = 1", []).unwrap();
        let snapshot = capture_at(&conn, vault_id, 2).unwrap();
        compact_local(&mut conn, &snapshot).unwrap();

        let page = recent(&conn, 10).unwrap();

        assert_eq!(page.truncated_before, 2);
        assert_eq!(page.entries.len(), 1, "records below the horizon are gone");
    }

    #[test]
    fn an_uncompacted_silo_claims_no_truncation() {
        let (conn, _, _) = silo();

        assert_eq!(recent(&conn, 10).unwrap().truncated_before, 0);
    }

    #[test]
    fn a_short_page_offers_the_rest() {
        let (conn, _, _) = silo();

        let page = recent(&conn, 2).unwrap();

        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.total, Some(3));
        assert!(
            page.next.is_some(),
            "the third record had nowhere to be read from"
        );
    }

    #[test]
    fn a_record_from_a_newer_build_is_labelled_rather_than_hidden() {
        let (conn, _, device) = silo();
        let future = OpRecord {
            op_id: Uuid::new_v4(),
            lamport: 9,
            device_id: device,
            at: 1_700_000_009,
            skippable: true,
            seq: 9,
            prev: None,
            op: OpBody::Unknown {
                op: "SomethingLater".into(),
                fields: serde_json::Map::new(),
            },
        };
        replay(&conn, vec![future]).unwrap();

        let newest = recent(&conn, 1).unwrap().entries.remove(0);

        assert!(newest.unknown);
        assert!(newest.summary.contains("SomethingLater"));
    }
}
