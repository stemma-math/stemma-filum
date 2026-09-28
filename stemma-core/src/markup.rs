//! Walking Typst markup without parsing it.
//!
//! Two things are read out of a line's markup without compiling it: the
//! labels its text cites and the files its source pulls in. Both need to
//! step over what Typst never reads as markup — an escaped character, a
//! raw span, a comment — and both walk the text byte by byte, because
//! everything they look for starts with an ASCII character.

/// A position in some markup, moved forward one construct at a time.
pub(crate) struct Scanner<'a> {
    source: &'a str,
    bytes: &'a [u8],
    /// The byte the scanner is at. It may run past the end of the source
    /// when an escape is the last thing in it.
    pub(crate) at: usize,
}

impl<'a> Scanner<'a> {
    /// Starts at the beginning.
    pub(crate) fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            at: 0,
        }
    }

    /// Whether everything has been walked.
    pub(crate) fn is_done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    /// The byte `ahead` positions on, if there is one.
    pub(crate) fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.at + ahead).copied()
    }

    /// The byte just before the current one, if there is one.
    pub(crate) fn previous(&self) -> Option<u8> {
        self.at
            .checked_sub(1)
            .and_then(|before| self.bytes.get(before))
            .copied()
    }

    /// Whether the rest of the source starts with `text`.
    pub(crate) fn looks_at(&self, text: &str) -> bool {
        self.bytes
            .get(self.at..)
            .is_some_and(|rest| rest.starts_with(text.as_bytes()))
    }

    /// The source between `start` and the current position.
    ///
    /// Both ends must sit on ASCII bytes the scanner stopped at, which
    /// every caller's do.
    pub(crate) fn since(&self, start: usize) -> &'a str {
        &self.source[start..self.at.min(self.source.len())]
    }

    /// Steps over an escape, a raw span or a comment starting here, and
    /// says whether there was one.
    ///
    /// None of them can hold anything markup means: an escaped `@` is an
    /// at sign, and a comment or a raw example is not read at all.
    pub(crate) fn skip_inert(&mut self) -> bool {
        match self.peek(0) {
            Some(b'\\') => self.at += 2,
            Some(b'/') if self.peek(1) == Some(b'/') => self.skip_to_end_of_line(),
            Some(b'/') if self.peek(1) == Some(b'*') => self.skip_block_comment(),
            Some(b'`') => self.skip_raw(),
            _ => return false,
        }
        true
    }

    /// Skips whitespace and comments.
    pub(crate) fn skip_trivia(&mut self) {
        loop {
            match self.peek(0) {
                Some(byte) if byte.is_ascii_whitespace() => self.at += 1,
                Some(b'/') if self.peek(1) == Some(b'/') => self.skip_to_end_of_line(),
                Some(b'/') if self.peek(1) == Some(b'*') => self.skip_block_comment(),
                _ => return,
            }
        }
    }

    /// Steps over a quoted string, escapes and all.
    pub(crate) fn skip_string(&mut self) {
        self.at += 1;

        while let Some(byte) = self.peek(0) {
            match byte {
                b'\\' => self.at += 2,
                b'"' => {
                    self.at += 1;
                    return;
                }
                _ => self.at += 1,
            }
        }
    }

    /// Steps over a raw span, however many backticks fence it.
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

    /// Steps over the rest of a line, stopping at its newline.
    fn skip_to_end_of_line(&mut self) {
        while self.peek(0).is_some_and(|byte| byte != b'\n') {
            self.at += 1;
        }
    }

    /// Steps over a block comment, which in Typst may nest.
    fn skip_block_comment(&mut self) {
        self.at += 2;
        let mut depth = 1;

        while !self.is_done() {
            if self.looks_at("/*") {
                depth += 1;
                self.at += 2;
            } else if self.looks_at("*/") {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the scanner stops after skipping whatever starts the text.
    fn skipped(text: &str) -> usize {
        let mut scanner = Scanner::new(text);
        assert!(scanner.skip_inert(), "nothing inert at the start of {text:?}");
        scanner.at
    }

    #[test]
    fn a_comment_ends_where_typst_ends_it() {
        assert_eq!(skipped("// note\nafter"), 7, "the newline is not the comment's");
        assert_eq!(skipped("/* a /* b */ c */ after"), 17, "block comments nest");
        assert_eq!(skipped("/* never closed"), 15);
    }

    #[test]
    fn a_raw_span_ends_at_its_own_fence() {
        assert_eq!(skipped("```a ` b``` after"), 11);
        assert_eq!(skipped("`never closed"), 13);
    }

    #[test]
    fn an_escape_takes_the_next_byte_with_it() {
        assert_eq!(skipped("\\@x"), 2);
        assert!(!Scanner::new("plain").skip_inert());
    }

    #[test]
    fn a_string_ends_at_the_first_quote_nobody_escaped() {
        let mut scanner = Scanner::new(r#""a \" b" after"#);
        scanner.skip_string();
        assert_eq!(scanner.since(0), r#""a \" b""#);
    }
}
