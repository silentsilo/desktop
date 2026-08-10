//! Virtual filesystem over SQLCipher vault database.

pub mod activity;
pub mod digest;
mod like;
pub mod names;
pub mod oplog;
mod ops;
mod schema;
pub mod snapshot;

pub use activity::{ActivityEntry, ActivityPage};
pub use digest::{digest, digest_difference};
pub use names::{MAX_NAME_BYTES, check as check_name, sanitize as sanitize_name};
pub use oplog::{
    ApplyOutcome, OpBody, OpRecord, ReplayReport, VaultOp, all_op_ids, all_ops, apply_op,
    chain_tip, clear_pushed, device_id, emit, highest_applied_lamport, init_delivery,
    init_oplog_core, init_oplog_derived, mark_delivered, next_lamport, observe_lamport,
    pending_count, pending_count_for, pending_ops, pending_ops_for, rebuild_derived,
    record_target_failure, record_target_success, replay, reset_target_backoff, settle_delivery,
    target_failures, target_last_success, target_ready, target_retry_in, verify_chains,
};
pub use ops::{AttachmentBlob, Vfs, guess_mime};
pub use schema::{SCHEMA_VERSION, init_schema, root_folder_id_for};
pub use snapshot::{
    CompactionPolicy, PruneReport, SNAPSHOT_VERSION, Snapshot, base_horizon, capture_at,
    choose_horizon, compact_local, read_base, referenced_blobs, restore as restore_snapshot,
};
