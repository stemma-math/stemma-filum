//! Reading what a Typst source declares.
//!
//! Centrality, dependencies and names are written in the source, as
//! arguments to the mathematical environments, so that they are
//! versioned, diffable, readable with no machine running, and something
//! an agent can declare in what it proposes. Nothing about a line is kept
//! anywhere the author cannot see and edit.
//!
//! What is read here is only the declarations. Typesetting the document
//! is Typst's work and happens elsewhere; this has to run on a server
//! that compiles nothing and in an editor between keystrokes.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

/// The environments a line is written with.
pub const ENVIRONMENTS: [&str; 7] = [
    "definition",
    "theorem",
    "lemma",
    "proposition",
    "corollary",
    "remark",
    "proof",
];

/// What an argument was given as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A quoted string.
    Text(String),
    /// `true` or `false`.
    Flag(bool),
    /// A parenthesised list of strings.
    List(Vec<String>),
    /// A bare word, which is almost always a mistake worth reporting.
    Word(String),
}

impl Value {
    /// The value as text, when it is text.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The value as a flag, when it is one.
    pub fn flag(&self) -> Option<bool> {
        match self {
            Self::Flag(flag) => Some(*flag),
            _ => None,
        }
    }

    /// The value as a list. A single string counts as a list of one,
    /// because `uses: "def:group"` is what everybody writes first.
    pub fn list(&self) -> Option<Vec<String>> {
        match self {
            Self::List(items) => Some(items.clone()),
            Self::Text(text) => Some(vec![text.clone()]),
            _ => None,
        }
    }
}

/// One environment, as it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    /// Which environment it is.
    pub environment: String,
    /// What it was given, by name.
    pub arguments: BTreeMap<String, Value>,
    /// Where it starts, counting from one.
    pub line: usize,
    /// What is between its brackets, if it has any.
    pub body: Option<Range<usize>>,
}

impl Declaration {
    /// One argument as text.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.arguments.get(name)?.text()
    }

    /// One argument as a flag.
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.arguments.get(name)?.flag()
    }

    /// One argument as a list.
    pub fn list(&self, name: &str) -> Vec<String> {
        self.arguments
            .get(name)
            .and_then(Value::list)
            .unwrap_or_default()
    }
}

/// Why a source could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {problem}")]
pub struct Problem {
    /// Where it is, counting from one.
    pub line: usize,
    /// What is wrong.
    pub problem: Wrong,
}

/// The ways a declaration can be malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wrong {
    /// The argument list never closes.
    Unclosed,
    /// The body never closes.
    UnclosedBody,
    /// An argument was given without a name.
    Unnamed(String),
    /// The same argument was given twice.
    Twice(String),
}

impl fmt::Display for Wrong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unclosed => f.write_str("this declaration's arguments are never closed"),
            Self::UnclosedBody => f.write_str("this declaration's body is never closed"),
            Self::Unnamed(given) => {
                write!(f, "`{given}` has no name; every argument here is named")
            }
            Self::Twice(name) => write!(f, "`{name}` was given twice"),
        }
    }
}

/// Everything a source declares.
pub fn read(source: &str) -> Result<Vec<Declaration>, Problem> {
    Scanner::new(source).run()
}

/// What a line calls itself, which is its first heading.
///
/// A line with no heading has no title, and that is not worth reporting:
/// the directory it is in already names it.
pub fn title(source: &str) -> Option<String> {
    source
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("= "))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
}

/// Walks a source once, looking for environments.
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

    /// Reads the whole source.
    fn run(mut self) -> Result<Vec<Declaration>, Problem> {
        let mut found = Vec::new();

        while self.at < self.bytes.len() {
            match self.bytes[self.at] {
                b'"' => self.skip_string(),
                b'/' if self.peek(1) == Some(b'/') => self.skip_to_end_of_line(),
                b'/' if self.peek(1) == Some(b'*') => self.skip_block_comment(),
                b'#' => match self.read_declaration()? {
                    Some(declaration) => found.push(declaration),
                    None => self.at += 1,
                },
                _ => self.at += 1,
            }
        }

        Ok(found)
    }

    /// The byte `ahead` positions on, if there is one.
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.at + ahead).copied()
    }

    /// Which line the current position is on, counting from one.
    fn line_at(&self, at: usize) -> usize {
        self.source[..at.min(self.source.len())]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1
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

        while self.at < self.bytes.len() {
            if self.bytes[self.at] == b'*' && self.peek(1) == Some(b'/') {
                self.at += 2;
                return;
            }

            self.at += 1;
        }

        self.at = self.bytes.len();
    }

    /// Reads one environment, if that is what starts here.
    fn read_declaration(&mut self) -> Result<Option<Declaration>, Problem> {
        let start = self.at;
        let after_hash = start + 1;

        let name_end = self.source[after_hash..]
            .find(|character: char| !character.is_ascii_alphabetic())
            .map(|length| after_hash + length)
            .unwrap_or(self.source.len());
        let name = &self.source[after_hash..name_end];

        if !ENVIRONMENTS.contains(&name) || self.bytes.get(name_end) != Some(&b'(') {
            return Ok(None);
        }

        let line = self.line_at(start);
        self.at = name_end;

        let arguments_range = self.balanced(b'(', b')').ok_or(Problem {
            line,
            problem: Wrong::Unclosed,
        })?;
        let arguments = parse_arguments(&self.source[arguments_range], line)?;

        // A body is optional: `#proof(of: "thm:one")` on its own is a
        // statement that a proof exists somewhere else, and reading it
        // must not swallow the next paragraph.
        let mut lookahead = self.at;
        while matches!(self.bytes.get(lookahead), Some(b' ' | b'\t')) {
            lookahead += 1;
        }

        let body = match self.bytes.get(lookahead) {
            Some(b'[') => {
                self.at = lookahead;
                Some(self.balanced(b'[', b']').ok_or(Problem {
                    line,
                    problem: Wrong::UnclosedBody,
                })?)
            }
            _ => None,
        };

        Ok(Some(Declaration {
            environment: name.to_owned(),
            arguments,
            line,
            body,
        }))
    }

    /// Reads from an opening delimiter to the one that matches it, and
    /// answers with what is between them.
    fn balanced(&mut self, open: u8, close: u8) -> Option<Range<usize>> {
        debug_assert_eq!(self.bytes.get(self.at), Some(&open));

        let inside = self.at + 1;
        let mut depth = 0_u32;

        while self.at < self.bytes.len() {
            match self.bytes[self.at] {
                b'"' => {
                    self.skip_string();
                    continue;
                }
                b'/' if self.peek(1) == Some(b'/') => {
                    self.skip_to_end_of_line();
                    continue;
                }
                byte if byte == open => depth += 1,
                byte if byte == close => {
                    depth -= 1;

                    if depth == 0 {
                        let ends = self.at;
                        self.at += 1;

                        return Some(inside..ends);
                    }
                }
                // Brackets nest inside a body, and a body is written by
                // somebody who is thinking about mathematics, not about
                // this parser.
                b'[' if close == b']' => depth += 1,
                b']' if close == b']' => depth -= 1,
                _ => {}
            }

            self.at += 1;
        }

        None
    }
}

/// Reads the arguments between the parentheses.
fn parse_arguments(text: &str, line: usize) -> Result<BTreeMap<String, Value>, Problem> {
    let mut arguments = BTreeMap::new();

    for piece in split_top_level(text) {
        let piece = piece.trim();

        if piece.is_empty() {
            continue;
        }

        let Some((name, value)) = split_name(piece) else {
            return Err(Problem {
                line,
                problem: Wrong::Unnamed(piece.to_owned()),
            });
        };

        if arguments.contains_key(name) {
            return Err(Problem {
                line,
                problem: Wrong::Twice(name.to_owned()),
            });
        }

        arguments.insert(name.to_owned(), parse_value(value));
    }

    Ok(arguments)
}

/// Splits on the commas that are not inside anything.
fn split_top_level(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut pieces = Vec::new();
    let (mut start, mut depth, mut at) = (0, 0_i32, 0);

    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b',' if depth == 0 => {
                pieces.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }

        at += 1;
    }

    pieces.push(&text[start.min(text.len())..]);
    pieces
}

/// Splits `name: value`, on the colon that is not inside anything.
fn split_name(piece: &str) -> Option<(&str, &str)> {
    let bytes = piece.as_bytes();
    let mut depth = 0_i32;
    let mut at = 0;

    while at < bytes.len() {
        match bytes[at] {
            b'"' => {
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b':' if depth == 0 => {
                let name = piece[..at].trim();

                return (!name.is_empty() && name.chars().all(is_name_character))
                    .then(|| (name, piece[at + 1..].trim()));
            }
            _ => {}
        }

        at += 1;
    }

    None
}

/// Whether a character can be part of an argument's name.
fn is_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-' || character == '_'
}

/// Reads one value.
fn parse_value(text: &str) -> Value {
    let text = text.trim();

    if let Some(quoted) = unquote(text) {
        return Value::Text(quoted);
    }

    match text {
        "true" => return Value::Flag(true),
        "false" => return Value::Flag(false),
        _ => {}
    }

    if text.starts_with('(') && text.ends_with(')') {
        let inside = &text[1..text.len() - 1];

        return Value::List(
            split_top_level(inside)
                .into_iter()
                .filter_map(|item| unquote(item.trim()))
                .collect(),
        );
    }

    Value::Word(text.to_owned())
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
    fn a_line_is_called_what_its_first_heading_says() {
        assert_eq!(
            title("#import \"x\"\n\n= Quotient groups\n\nProse.\n").as_deref(),
            Some("Quotient groups")
        );
        assert_eq!(
            title("== A subheading first\n= The title\n").as_deref(),
            Some("The title")
        );
        assert_eq!(title("No heading at all.\n"), None);
        assert_eq!(title("=\n= \n"), None);
    }

    #[test]
    fn an_environment_is_read_with_its_arguments() {
        let declarations = read(
            r#"
            #theorem(id: "thm:main", name: "The main one", central: true)[
              Every finite group is finite.
            ]
            "#,
        )
        .unwrap();

        assert_eq!(declarations.len(), 1);

        let theorem = &declarations[0];
        assert_eq!(theorem.environment, "theorem");
        assert_eq!(theorem.text("id"), Some("thm:main"));
        assert_eq!(theorem.text("name"), Some("The main one"));
        assert_eq!(theorem.flag("central"), Some(true));
        assert!(theorem.body.is_some());
    }

    #[test]
    fn what_a_statement_uses_is_read_as_a_list_however_it_is_written() {
        let one = read(r#"#lemma(id: "lem:a", uses: "def:group")[x]"#).unwrap();
        let several = read(r#"#lemma(id: "lem:a", uses: ("def:group", "thm:main"))[x]"#).unwrap();

        assert_eq!(one[0].list("uses"), ["def:group"]);
        assert_eq!(several[0].list("uses"), ["def:group", "thm:main"]);
        assert!(
            read(r#"#lemma(id: "lem:a")[x]"#).unwrap()[0]
                .list("uses")
                .is_empty()
        );
    }

    #[test]
    fn a_body_may_hold_brackets_of_its_own() {
        let declarations = read(
            r#"#theorem(id: "thm:one")[
                 Let $[0, 1]$ be the interval. #footnote[As usual.]
               ]"#,
        )
        .unwrap();

        let body = declarations[0].body.clone().unwrap();
        let text = &r#"#theorem(id: "thm:one")[
                 Let $[0, 1]$ be the interval. #footnote[As usual.]
               ]"#[body];

        assert!(text.contains("#footnote[As usual.]"));
        assert!(text.trim_end().ends_with("As usual.]"));
    }

    #[test]
    fn a_bracket_inside_a_string_does_not_end_the_body() {
        let source =
            r#"#remark(id: "rem:a")[He wrote "]" on the board.] #remark(id: "rem:b")[Then left.]"#;
        let declarations = read(source).unwrap();

        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[1].text("id"), Some("rem:b"));
    }

    #[test]
    fn a_declaration_in_a_comment_is_not_one() {
        let declarations = read(
            r#"
            // #theorem(id: "thm:commented")[Not really.]
            /* #lemma(id: "lem:also-not")[Nor this.] */
            #theorem(id: "thm:real")[This one.]
            "#,
        )
        .unwrap();

        assert_eq!(declarations.len(), 1);
        assert_eq!(declarations[0].text("id"), Some("thm:real"));
    }

    #[test]
    fn a_declaration_inside_a_string_is_not_one() {
        let declarations = read(r##"#let example = "#theorem(id: \"thm:x\")[y]""##).unwrap();

        assert!(declarations.is_empty());
    }

    #[test]
    fn a_proof_may_stand_without_a_body() {
        let declarations = read(
            r#"#proof(of: "thm:one")
               Some ordinary prose that follows.
               #theorem(id: "thm:two")[Another.]"#,
        )
        .unwrap();

        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[0].environment, "proof");
        assert!(declarations[0].body.is_none());
        assert_eq!(declarations[1].text("id"), Some("thm:two"));
    }

    #[test]
    fn everything_else_in_the_document_is_left_alone() {
        let declarations = read(
            r#"
            #import "@preview/example:0.1.0": *
            #set page(margin: 2cm)
            = A heading

            Ordinary prose with #emph[emphasis] in it.

            #theorem(id: "thm:one")[The only declaration.]
            "#,
        )
        .unwrap();

        assert_eq!(declarations.len(), 1);
    }

    #[test]
    fn the_line_a_declaration_is_on_is_reported() {
        let declarations = read("\n\n#theorem(id: \"thm:one\")[x]\n").unwrap();

        assert_eq!(declarations[0].line, 3);
    }

    #[test]
    fn an_argument_list_that_never_closes_is_reported_where_it_starts() {
        let problem = read("\n#theorem(id: \"thm:one\"\n").unwrap_err();

        assert_eq!(problem.line, 2);
        assert_eq!(problem.problem, Wrong::Unclosed);
        assert!(problem.to_string().contains("never closed"));
    }

    #[test]
    fn a_body_that_never_closes_is_reported() {
        let problem = read(r#"#theorem(id: "thm:one")[unfinished"#).unwrap_err();

        assert_eq!(problem.problem, Wrong::UnclosedBody);
    }

    #[test]
    fn an_argument_with_no_name_is_reported_with_what_was_written() {
        let problem = read(r#"#theorem("thm:one")[x]"#).unwrap_err();

        assert_eq!(problem.problem, Wrong::Unnamed("\"thm:one\"".to_owned()));
    }

    #[test]
    fn the_same_argument_twice_is_refused_rather_than_one_of_them_winning() {
        let problem = read(r#"#theorem(id: "thm:one", id: "thm:two")[x]"#).unwrap_err();

        assert_eq!(problem.problem, Wrong::Twice("id".to_owned()));
    }

    #[test]
    fn an_escaped_quote_survives_being_read() {
        let declarations = read(r#"#theorem(id: "thm:one", name: "The \"big\" one")[x]"#).unwrap();

        assert_eq!(declarations[0].text("name"), Some("The \"big\" one"));
    }

    #[test]
    fn a_trailing_comma_is_allowed_because_everybody_writes_one() {
        let declarations = read(r#"#theorem(id: "thm:one", central: true,)[x]"#).unwrap();

        assert_eq!(declarations[0].arguments.len(), 2);
    }

    #[test]
    fn a_word_where_a_string_was_meant_is_kept_as_a_word() {
        let declarations = read(r#"#theorem(id: thm-one)[x]"#).unwrap();

        assert_eq!(
            declarations[0].arguments.get("id"),
            Some(&Value::Word("thm-one".to_owned()))
        );
        assert_eq!(declarations[0].text("id"), None);
    }
}
