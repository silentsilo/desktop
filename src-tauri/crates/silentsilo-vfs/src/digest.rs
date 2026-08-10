//! Everything a vault holds, as text two machines can compare. The compat
//! fixtures and the test restore ask the same question: does rebuilding
//! from storage still produce this vault? Canonical on purpose: no paths,
//! no clocks, no device ids. Anything left in here is something a rebuild
//! must produce exactly, so adding a field is a decision.

use rusqlite::Connection;
use silentsilo_core::{CoreError, CoreResult};

/// The vault's contents as sorted lines.
pub fn digest(conn: &Connection) -> CoreResult<Vec<String>> {
    let mut out = Vec::new();
    for (label, sql) in [
        (
            "folder",
            "SELECT path || ' deleted=' || (deleted_at IS NOT NULL) || ' fav=' || favorite
               FROM folders ORDER BY path",
        ),
        (
            "file",
            "SELECT fo.path || '::' || f.name
                    || ' deleted=' || (f.deleted_at IS NOT NULL)
                    || ' fav=' || f.favorite
                    || ' size=' || f.size_bytes
                    || ' hash=' || COALESCE(f.content_hash, '')
                    || ' mime=' || COALESCE(f.mime_type, '')
               FROM files f JOIN folders fo ON fo.id = f.folder_id
              ORDER BY fo.path, f.name",
        ),
        ("password", "SELECT id FROM passwords ORDER BY id"),
        ("ops", "SELECT 'count=' || COUNT(*) FROM oplog"),
    ] {
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| CoreError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CoreError::Database(e.to_string()))?;
        for row in rows {
            out.push(format!(
                "{label} {}",
                row.map_err(|e| CoreError::Database(e.to_string()))?
            ));
        }
    }
    Ok(out)
}

/// The lines two digests disagree on, each marked with which side has it.
///
/// A diff rather than a boolean, because "your backup does not restore to
/// what you have" is the moment someone most needs to know *what* is wrong.
/// The count of a silo's operation records is deliberately part of the
/// comparison, so a restore that produced the right tree from the wrong
/// history still shows up.
pub fn digest_difference(live: &[String], restored: &[String]) -> Vec<String> {
    let restored_set: std::collections::HashSet<&String> = restored.iter().collect();
    let live_set: std::collections::HashSet<&String> = live.iter().collect();

    let mut out: Vec<String> = live
        .iter()
        .filter(|line| !restored_set.contains(line))
        .map(|line| format!("only here: {line}"))
        .chain(
            restored
                .iter()
                .filter(|line| !live_set.contains(line))
                .map(|line| format!("only in the restore: {line}")),
        )
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_digests_differ_in_nothing() {
        let a = vec!["folder /".to_string(), "ops count=3".to_string()];
        assert!(digest_difference(&a, &a.clone()).is_empty());
    }

    #[test]
    fn each_side_is_named_so_the_reader_knows_which_way_it_went() {
        // A file missing from the restore and a file only the restore has are
        // different problems, and telling them apart is the whole use of this.
        let live = vec!["file /::a.txt".to_string(), "ops count=2".to_string()];
        let restored = vec!["file /::b.txt".to_string(), "ops count=2".to_string()];

        let diff = digest_difference(&live, &restored);
        assert_eq!(
            diff,
            vec![
                "only here: file /::a.txt".to_string(),
                "only in the restore: file /::b.txt".to_string(),
            ]
        );
    }

    #[test]
    fn a_restore_from_the_wrong_history_shows_up_even_with_the_right_tree() {
        // The record count is in the digest for exactly this: the tree can
        // match while the log it came from does not.
        let live = vec!["folder /".to_string(), "ops count=40".to_string()];
        let restored = vec!["folder /".to_string(), "ops count=12".to_string()];
        assert_eq!(digest_difference(&live, &restored).len(), 2);
    }
}
