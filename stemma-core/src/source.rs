//! Reading what a Typst document pulls in.
//!
//! A line is written as blocks and typeset as one Typst document, and
//! what any of its blocks imports travels with it. This finds those
//! imports without compiling anything, because it has to run on a
//! server that compiles nothing.

use crate::markup::Scanner;

/// Every file this source pulls in, as it was written.
///
/// Nothing is resolved here: a path is what the author typed. What it
/// is for is the one mechanical rule about sharing — nothing the group
/// can see may lean on something only its author can.
pub fn imports(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut scanner = Scanner::new(source);

    while !scanner.is_done() {
        if scanner.peek(0) == Some(b'"') {
            scanner.skip_string();
            continue;
        }
        if scanner.skip_inert() {
            continue;
        }

        let Some(keyword) = ["#import", "#include"]
            .into_iter()
            .find(|keyword| introduces(&scanner, keyword))
        else {
            scanner.at += 1;
            continue;
        };

        scanner.at += keyword.len();
        scanner.skip_trivia();
        if scanner.peek(0) == Some(b'"') {
            let start = scanner.at;
            scanner.skip_string();
            if let Some(path) = unquote(scanner.since(start)).filter(|path| !path.trim().is_empty())
            {
                found.push(path);
            }
        }
    }

    found
}

/// Whether `keyword` starts here as a keyword, and not as the beginning
/// of a longer word such as `#important`.
fn introduces(scanner: &Scanner<'_>, keyword: &str) -> bool {
    scanner.looks_at(keyword)
        && scanner
            .peek(keyword.len())
            .is_some_and(|byte| byte.is_ascii_whitespace() || byte == b'"' || byte == b'/')
}

/// The contents of a quoted string, if that is what this is.
fn unquote(text: &str) -> Option<String> {
    let inside = text.strip_prefix('"')?.strip_suffix('"')?;
    let mut unescaped = String::with_capacity(inside.len());
    let mut characters = inside.chars();

    while let Some(character) = characters.next() {
        match character {
            '\\' => unescaped.push(characters.next()?),
            _ => unescaped.push(character),
        }
    }

    Some(unescaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_are_scanned_in_order_across_lines_and_ignore_examples() {
        let source = r##"
/* #include "hidden.typ" */
`#import "example.typ"`
"#include \"string.typ\""
\#include "escaped.typ"
#important "not-an-import.typ"
#include dynamic // "not-a-path.typ"
#include "one.typ"; #import "two.typ": *
#import /* note */
    "three.typ": *
"##;
        assert_eq!(imports(source), ["one.typ", "two.typ", "three.typ"]);
    }

    #[test]
    fn what_a_source_pulls_in_is_read_as_written() {
        let source = "#import \"../shared-notation/main.typ\": *\n\
                      #include \"figures/one.typ\"\n\
                      // #import \"not-this.typ\"\n\
                      #theorem(id: \"thm:one\")[Something.]\n";

        assert_eq!(
            imports(source),
            [
                "../shared-notation/main.typ".to_owned(),
                "figures/one.typ".to_owned()
            ]
        );
        assert!(imports("nothing here").is_empty());
    }

    #[test]
    fn an_import_inside_a_nested_comment_is_not_one() {
        let source = "/* outer /* inner */ #import \"hidden.typ\" */\n#import \"seen.typ\"";

        assert_eq!(imports(source), ["seen.typ"]);
    }

    #[test]
    fn a_source_that_stops_part_way_through_is_read_without_panicking() {
        for source in ["#import \"unterminated", "#import", "#include \"a\\", "é\\é#import"] {
            assert!(imports(source).is_empty(), "for {source:?}");
        }
    }
}
