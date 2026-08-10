//! The envelope the silo's own settings files are written in:
//! `{"version": 1, "data": {...}}`. Without it these files are read with
//! "unparseable means absent", and a credentials file the app cannot parse
//! would look exactly like a silo never provisioned. Adding a field needs
//! no bump (serde ignores unknowns); the version exists for changes that
//! are not additive.

use serde::{Deserialize, Serialize};

use crate::error::VaultError;

/// Current version of every file below. One number for all of them: they are
/// written by the same build and read by the same build, and per-file
/// versions would be four things to remember instead of one.
pub const SILO_FILE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct Envelope<T> {
    version: u32,
    data: T,
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, VaultError> {
    serde_json::to_vec_pretty(&Envelope {
        version: SILO_FILE_VERSION,
        data: value,
    })
    .map_err(|e| VaultError::Crypto(e.to_string()))
}

/// Refuses a version this build does not know rather than guessing at the
/// shape underneath it.
///
/// Read in two steps on purpose. Deserialising straight into `T` would make
/// the version check run last, so a newer file whose shape had actually
/// changed, the only kind the check exists for, would fail as a parse error
/// and report a missing field instead of a version to update past.
pub fn decode<T: for<'de> Deserialize<'de>>(what: &str, bytes: &[u8]) -> Result<T, VaultError> {
    let envelope: Envelope<serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|e| VaultError::Corrupted(format!("{what}: {e}")))?;
    if envelope.version > SILO_FILE_VERSION {
        return Err(VaultError::Corrupted(format!(
            "{what} was written by a newer version of SilentSilo (format {}). Update to open it.",
            envelope.version
        )));
    }
    serde_json::from_value(envelope.data).map_err(|e| VaultError::Corrupted(format!("{what}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
    }

    #[test]
    fn a_value_round_trips() {
        let value = Sample {
            name: "silo".into(),
        };
        let bytes = encode(&value).unwrap();
        assert_eq!(decode::<Sample>("sample", &bytes).unwrap(), value);
    }

    #[test]
    fn the_version_is_written_where_a_human_can_read_it() {
        let bytes = encode(&Sample { name: "x".into() }).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("\"version\": 1"), "got {text}");
    }

    #[test]
    fn a_newer_file_is_refused_by_name() {
        // The whole reason for the envelope: the message has to say what is
        // wrong, because the alternative is being told the silo is empty.
        let bytes = br#"{"version": 99, "data": {"name": "x"}}"#;
        let err = decode::<Sample>("credentials", bytes).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("credentials"), "got {message}");
        assert!(message.contains("newer version"), "got {message}");
    }

    #[test]
    fn a_newer_file_whose_shape_changed_still_reports_the_version() {
        // The case the check is actually for. Read in one pass, this comes
        // back as "missing field `name`", which tells the user nothing about
        // what to do.
        let bytes = br#"{"version": 99, "data": {"renamed_field": "x"}}"#;
        let message = decode::<Sample>("credentials", bytes)
            .unwrap_err()
            .to_string();
        assert!(message.contains("newer version"), "got {message}");
    }

    #[test]
    fn an_unknown_field_is_not_a_problem() {
        // Additive changes stay compatible in both directions, which is why
        // adding one needs no bump.
        let bytes = br#"{"version": 1, "data": {"name": "x", "colour": "red"}}"#;
        assert_eq!(
            decode::<Sample>("sample", bytes).unwrap(),
            Sample { name: "x".into() }
        );
    }
}
