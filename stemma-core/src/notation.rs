//! The package a line's source imports.
//!
//! The environments are Stemma's, and so is what they look like on
//! paper, but a line is an ordinary Typst file: it says at the top
//! which version of the notation it is written against, the way any
//! Typst document names what it imports. Nothing is injected behind
//! the author's back, and `typst compile` on the file works.
//!
//! The names live here because the server writes the import line when
//! it starts a line, and the client installs what that line names.

/// The namespace the package is installed under.
pub const NAMESPACE: &str = "stemma";

/// What the package is called.
pub const NAME: &str = "notation";

/// The version this build writes and carries.
pub const VERSION: &str = "0.1.0";

/// The package, as Typst names one.
pub fn spec() -> String {
    format!("@{NAMESPACE}/{NAME}:{VERSION}")
}

/// The line a source begins with.
pub fn import() -> String {
    format!("#import \"{}\": *", spec())
}

/// Whether a source says where its environments come from.
///
/// The version is not checked. A line pinned to an older notation
/// composes against the older notation, which is the point of writing
/// the version down, and telling its author it is missing something it
/// has would be worse than saying nothing.
pub fn imported_by(source: &str) -> bool {
    source
        .lines()
        .any(|line| line.contains("#import") && line.contains(&format!("@{NAMESPACE}/{NAME}")))
}

/// Whether a source uses the environments without saying where they
/// come from.
///
/// This is what a line written before the package existed looks like.
/// It is not a mistake anybody made: the environments used to be given
/// to the document by whatever composed it, and a line from then is a
/// line that never had an import to lose.
pub fn missing_from(source: &str) -> bool {
    !imported_by(source)
        && crate::source::ENVIRONMENTS.iter().any(|environment| {
            source.contains(&format!("#{environment}("))
                || source.contains(&format!("#{environment}["))
        })
}

/// What to tell somebody whose line has no import.
pub fn how_to_add_it() -> String {
    format!(
        "this line does not say where its environments come from: \
         add `{}` as its first line",
        import()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_imports_everything_the_environments_need() {
        assert_eq!(import(), "#import \"@stemma/notation:0.1.0\": *");
    }

    #[test]
    fn a_line_written_before_the_package_existed_is_recognised() {
        let orphan = "= Hola\n\n#question[Adiós.]\n";

        assert!(missing_from(orphan));
        assert!(!imported_by(orphan));
        assert!(how_to_add_it().contains(&import()));
    }

    #[test]
    fn a_line_that_says_where_its_environments_come_from_is_left_alone() {
        let written = format!("{}\n\n= Hola\n\n#question[Adiós.]\n", import());

        assert!(imported_by(&written));
        assert!(!missing_from(&written));
    }

    #[test]
    fn a_line_pinned_to_another_version_is_not_told_it_is_missing_one() {
        let older = "#import \"@stemma/notation:0.0.1\": *\n\n#question[Is it?]\n";

        assert!(imported_by(older));
        assert!(!missing_from(older));
    }

    #[test]
    fn prose_that_uses_no_environment_needs_no_import() {
        assert!(!missing_from("= Hola\n\nJust words.\n"));
    }
}
