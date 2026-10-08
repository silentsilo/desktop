//! Errors the window translates.
//!
//! A command still fails with a string, so code that compares one or writes
//! it to a log goes on working. A coded one carries, after its English text
//! and a separator, the key the window translates it by and the values that
//! go into it. The English comes first: it is what a log, a test and anything
//! reading the string as plain text sees. The window's side is
//! `decodeAppError` in `src/lib/errors.ts`.

use std::fmt::Display;

/// ASCII unit separator: never in a sentence, so never a false match.
pub const SEP: char = '\u{1f}';

/// A fixed message with its key, usable in a `const`.
macro_rules! coded {
    ($code:literal, $english:literal) => {
        concat!($english, "\u{1f}", $code)
    };
}
pub(crate) use coded;

/// A message with values in it: `english` already has them written in, and
/// `params` names each one for the translation.
pub fn coded_with(code: &str, english: impl Display, params: &[(&str, &dyn Display)]) -> String {
    let params: serde_json::Map<String, serde_json::Value> = params
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string().into()))
        .collect();
    format!(
        "{english}{SEP}{code}{SEP}{}",
        serde_json::Value::Object(params)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_english_comes_first_and_the_key_after() {
        const FIXED: &str = coded!("err.key_not_found", "That key was not found.");
        assert_eq!(FIXED, "That key was not found.\u{1f}err.key_not_found");
        assert!(FIXED.starts_with("That key was not found."));

        let name = "Work";
        let with = coded_with(
            "err.silo_name_taken",
            format!("You already have a silo called “{name}”."),
            &[("name", &name)],
        );
        let parts: Vec<&str> = with.split(SEP).collect();
        assert_eq!(parts[0], "You already have a silo called “Work”.");
        assert_eq!(parts[1], "err.silo_name_taken");
        assert_eq!(parts[2], r#"{"name":"Work"}"#);
    }
}
