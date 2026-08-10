//! `LIKE` patterns built from paths the user chose. Every subtree
//! operation asks which rows live under a folder, and `path LIKE
//! '/parent/%'` is only correct while the folder's name has no `LIKE`
//! metacharacter. Names may contain both: `%` is allowed, and `_` is
//! *produced* by `sanitize` for every character Windows forbids. Unescaped,
//! such a folder matches its neighbours and purge takes their blobs with
//! it on every device.
//!
//! So no query builds a pattern by hand: use the helpers here and write
//! `ESCAPE '!'` in the SQL, `!` rather than backslash so neither Rust nor
//! SQLite needs a second layer of escaping.

/// Everything strictly below `path`, with the path itself taken literally.
///
/// A trailing separator on the input is ignored, so the root (`/`) yields
/// `/%` and matches the whole tree rather than nothing.
pub(crate) fn subtree(path: &str) -> String {
    format!("{}/%", escape(path.trim_end_matches('/')))
}

/// Everything below a prefix that already carries its trailing separator.
///
/// Separate from [`subtree`] because rename needs the prefix itself as well,
/// to compute how much of each descendant path to replace, and building it
/// twice from different halves is how the two drift apart.
pub(crate) fn below_prefix(prefix: &str) -> String {
    format!("{}%", escape(prefix))
}

/// Anything containing `text`, for search.
pub(crate) fn containing(text: &str) -> String {
    format!("%{}%", escape(text))
}

/// Neutralises the three characters `LIKE ... ESCAPE '!'` treats specially.
///
/// The escape character has to come first, or it would be applied again to
/// the markers the other two replacements just inserted.
fn escape(text: &str) -> String {
    text.replace('!', "!!")
        .replace('%', "!%")
        .replace('_', "!_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_in_a_folder_name_stay_literal() {
        assert_eq!(subtree("/a%b"), "/a!%b/%");
        assert_eq!(subtree("/a_b"), "/a!_b/%");
        assert_eq!(below_prefix("/a%b/"), "/a!%b/%");
    }

    #[test]
    fn the_escape_character_escapes_itself() {
        // Escaping `!` last would turn the `!` of `!%` into `!!%`, which
        // matches a literal `!` followed by anything.
        assert_eq!(subtree("/a!b"), "/a!!b/%");
        assert_eq!(subtree("/a!%b"), "/a!!!%b/%");
    }

    #[test]
    fn the_root_matches_the_whole_tree() {
        assert_eq!(subtree("/"), "/%");
    }

    #[test]
    fn an_ordinary_path_is_left_alone() {
        assert_eq!(subtree("/photos/2026"), "/photos/2026/%");
        assert_eq!(containing("report"), "%report%");
    }
}
