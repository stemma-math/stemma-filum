//! The Typst package a line is typeset with.
//!
//! The environments are Stemma's, and so is what they look like on
//! paper, but the document built from a line is an ordinary Typst file:
//! it says at the top which version of the package it is written
//! against, the way any Typst document names what it imports, and
//! `typst compile` on it works.
//!
//! The names live here because the document is written by
//! [`crate::render`], and the client installs what that document names.

/// The namespace the package is installed under.
pub const NAMESPACE: &str = "stemma";

/// What the package is called.
pub const NAME: &str = "notation";

/// The version this build writes and carries.
pub const VERSION: &str = "0.2.0";

/// The functions the package defines and a rendered line calls.
///
/// One per kind of block, and the template the document is shown with.
pub const FUNCTIONS: [&str; 5] = ["stemma", "prose", "definition", "theorem", "proof"];

/// The package, as Typst names one.
pub fn spec() -> String {
    format!("@{NAMESPACE}/{NAME}:{VERSION}")
}

/// The line a rendered document begins with.
pub fn import() -> String {
    format!("#import \"{}\": *", spec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_imports_everything_the_environments_need() {
        assert_eq!(import(), "#import \"@stemma/notation:0.2.0\": *");
    }
}
