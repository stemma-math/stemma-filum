//! Reading what a Typst document pulls in.
//!
//! A line is written as blocks and typeset as one Typst document, and
//! what any of its blocks imports travels with it. This finds those
//! imports without compiling anything, because it has to run on a
//! server that compiles nothing.

/// Every file this source pulls in, as it was written.
///
/// Nothing is resolved here: a path is what the author typed. What it
/// is for is the one mechanical rule about sharing — nothing the group
/// can see may lean on something only its author can.
pub fn imports(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut scanner = Scanner::new(source);
    while scanner.at < scanner.bytes.len() {
        if scanner.skip_literal() {
            continue;
        }
        if scanner.bytes[scanner.at] == b'#' {
            let keyword = ["#import", "#include"].into_iter().find(|keyword| {
                source[scanner.at..].starts_with(keyword)
                    && scanner
                        .bytes
                        .get(scanner.at + keyword.len())
                        .is_some_and(|byte| {
                            byte.is_ascii_whitespace() || *byte == b'"' || *byte == b'/'
                        })
            });
            if let Some(keyword) = keyword {
                scanner.at += keyword.len();
                scanner.skip_trivia();
                if scanner.peek(0) == Some(b'"') {
                    let start = scanner.at;
                    scanner.skip_string();
                    if let Some(path) = unquote(&source[start..scanner.at.min(source.len())])
                        .filter(|path| !path.trim().is_empty())
                    {
                        found.push(path);
                    }
                }
                continue;
            }
        }
        scanner.at += 1;
    }

    found
}

/// Walks a source once, skipping what cannot introduce an import.
struct Scanner<'a> {
    source: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Scanner<'a> {
    /// Starts at the beginning.
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            at: 0,
        }
    }

    /// The byte `ahead` positions on, if there is one.
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.at + ahead).copied()
    }

    /// Skips text whose punctuation cannot introduce a declaration or delimiter.
    fn skip_literal(&mut self) -> bool {
        match self.peek(0) {
            Some(b'"') => self.skip_string(),
            Some(b'/') if self.peek(1) == Some(b'/') => self.skip_to_end_of_line(),
            Some(b'/') if self.peek(1) == Some(b'*') => self.skip_block_comment(),
            Some(b'`') => self.skip_raw(),
            Some(b'\\') => self.at = (self.at + 2).min(self.bytes.len()),
            _ => return false,
        }
        true
    }

    /// Skips whitespace and comments between a keyword and its argument.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek(0) {
                Some(byte) if byte.is_ascii_whitespace() => self.at += 1,
                Some(b'/') if self.peek(1) == Some(b'/') => self.skip_to_end_of_line(),
                Some(b'/') if self.peek(1) == Some(b'*') => self.skip_block_comment(),
                _ => return,
            }
        }
    }

    /// Skips a raw example, including fenced blocks containing single backticks.
    fn skip_raw(&mut self) {
        let start = self.at;
        while self.peek(0) == Some(b'`') {
            self.at += 1;
        }
        let fence = &self.source[start..self.at];
        self.at = self.source[self.at..]
            .find(fence)
            .map_or(self.bytes.len(), |offset| self.at + offset + fence.len());
    }

    /// Steps over a quoted string, escapes and all.
    fn skip_string(&mut self) {
        self.at += 1;

        while self.at < self.bytes.len() {
            match self.bytes[self.at] {
                b'\\' => self.at += 2,
                b'"' => {
                    self.at += 1;
                    return;
                }
                _ => self.at += 1,
            }
        }
    }

    /// Steps over the rest of a line.
    fn skip_to_end_of_line(&mut self) {
        while self.at < self.bytes.len() && self.bytes[self.at] != b'\n' {
            self.at += 1;
        }
    }

    /// Steps over a block comment.
    fn skip_block_comment(&mut self) {
        self.at += 2;
        let mut depth = 1;

        while self.at < self.bytes.len() {
            if self.peek(0) == Some(b'/') && self.peek(1) == Some(b'*') {
                depth += 1;
                self.at += 2;
            } else if self.peek(0) == Some(b'*') && self.peek(1) == Some(b'/') {
                self.at += 2;
                depth -= 1;
                if depth == 0 {
                    return;
                }
            } else {
                self.at += 1;
            }
        }

        self.at = self.bytes.len();
    }
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
}
