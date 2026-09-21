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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_imports_everything_the_environments_need() {
        assert_eq!(import(), "#import \"@stemma/notation:0.1.0\": *");
    }
}
