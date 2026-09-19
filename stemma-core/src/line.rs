//! A line of work, as its source declares it.
//!
//! The atomic unit is the line, not the article. A line is an ordered
//! sequence of declared things; the ones declared central divide it, and
//! **everything between two central statements is a black box** — derived
//! from the source rather than written down, because a box is not
//! something an author maintains, it is what is left between the things
//! they committed to.
//!
//! Boxes do not nest. That is not enforced by refusing anything: it is
//! true by construction, because a box is the run of ordinary statements
//! between two central ones and a run has no inside.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::model::{BoxState, Centrality};
use crate::source::{self, Declaration};

/// What kind of thing a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Something given a meaning.
    Definition,
    /// A result the line rests on, or could.
    Theorem,
    /// A step towards one.
    Lemma,
    /// A result in its own right, smaller than a theorem.
    Proposition,
    /// What follows from one immediately.
    Corollary,
    /// Something said about the mathematics rather than proved.
    Remark,
}

impl NodeKind {
    /// The prefix its siglum carries.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Definition => "def",
            Self::Theorem => "thm",
            Self::Lemma => "lem",
            Self::Proposition => "prop",
            Self::Corollary => "cor",
            Self::Remark => "rem",
        }
    }

    /// The environment that declares it.
    pub fn environment(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Theorem => "theorem",
            Self::Lemma => "lemma",
            Self::Proposition => "proposition",
            Self::Corollary => "corollary",
            Self::Remark => "remark",
        }
    }

    /// The kind an environment declares.
    pub fn from_environment(environment: &str) -> Option<Self> {
        Self::all()
            .into_iter()
            .find(|kind| kind.environment() == environment)
    }

    /// The kind a prefix names.
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        Self::all().into_iter().find(|kind| kind.prefix() == prefix)
    }

    /// Every kind.
    pub fn all() -> [Self; 6] {
        [
            Self::Definition,
            Self::Theorem,
            Self::Lemma,
            Self::Proposition,
            Self::Corollary,
            Self::Remark,
        ]
    }

    /// Whether it is something that can be proved.
    pub fn is_statement(self) -> bool {
        !matches!(self, Self::Definition | Self::Remark)
    }
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.environment())
    }
}

/// The part of a siglum after the kind.
///
/// Deliberately not the rule a group or an account name follows: nobody
/// puts a siglum in a URL, and `thm:1` and `lem:a` are what people
/// actually write on a blackboard.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

/// The longest a name may be.
pub const LONGEST_NAME: usize = 48;

impl Name {
    /// Validates a name.
    pub fn parse(input: &str) -> Result<Self, BadName> {
        if input.is_empty() {
            return Err(BadName::Empty);
        }
        if input.len() > LONGEST_NAME {
            return Err(BadName::TooLong);
        }

        let mut previous_was_hyphen = false;
        for character in input.chars() {
            match character {
                'a'..='z' | '0'..='9' => previous_was_hyphen = false,
                '-' if previous_was_hyphen => return Err(BadName::DoubleHyphen),
                '-' => previous_was_hyphen = true,
                other => return Err(BadName::BadCharacter(other)),
            }
        }

        if input.starts_with('-') || input.ends_with('-') {
            return Err(BadName::Hyphenated);
        }

        Ok(Self(input.to_owned()))
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Name {
    type Err = BadName;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl Serialize for Name {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Name {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;

        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Why a name will not do.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BadName {
    /// Nothing after the kind.
    #[error("a siglum needs a name after its kind")]
    Empty,
    /// Longer than [`LONGEST_NAME`].
    #[error("a name may be at most {} characters", LONGEST_NAME)]
    TooLong,
    /// A character outside the accepted set.
    #[error("`{0}` is not allowed in a name: use lowercase letters, digits and hyphens")]
    BadCharacter(char),
    /// Two hyphens in a row.
    #[error("a name must not contain two hyphens in a row")]
    DoubleHyphen,
    /// A hyphen at one end.
    #[error("a name must not start or end with a hyphen")]
    Hyphenated,
}

/// What a node is called: `thm:main`, `def:quotient`.
///
/// The siglum is the one element guaranteed to travel with a statement
/// into every context — a table row, a search result, another group's
/// citation — so it carries the kind as well as the name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Siglum {
    /// What kind of thing it names.
    pub kind: NodeKind,
    /// What the author called it.
    pub name: Name,
}

impl Siglum {
    /// Builds a siglum.
    pub fn new(kind: NodeKind, name: Name) -> Self {
        Self { kind, name }
    }
}

impl fmt::Display for Siglum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind.prefix(), self.name)
    }
}

/// Why a string is not a siglum.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BadSiglum {
    /// It carries no kind.
    #[error("`{0}` has no kind: write it as `thm:something`")]
    NoKind(String),
    /// Its kind is not one of ours.
    #[error("`{0}` is not a kind of node here")]
    UnknownKind(String),
    /// The name after the kind will not do.
    #[error(transparent)]
    Name(#[from] BadName),
}

impl FromStr for Siglum {
    type Err = BadSiglum;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let (prefix, name) = input
            .split_once(':')
            .ok_or_else(|| BadSiglum::NoKind(input.to_owned()))?;

        let kind = NodeKind::from_prefix(prefix)
            .ok_or_else(|| BadSiglum::UnknownKind(prefix.to_owned()))?;

        Ok(Self::new(kind, Name::parse(name)?))
    }
}

impl Serialize for Siglum {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Siglum {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;

        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// One thing a line declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// What it is called.
    pub siglum: Siglum,
    /// What it is called in prose, where the author gave it one.
    pub name: Option<String>,
    /// Whether the line rests on it.
    pub centrality: Centrality,
    /// What it leans on.
    pub uses: Vec<Siglum>,
    /// Whether the source carries a proof of it.
    pub proved: bool,
    /// Which line of the source declares it, counting from one.
    pub at: usize,
}

impl Node {
    /// Whether the line rests on this one.
    pub fn is_central(&self) -> bool {
        self.centrality.is_central()
    }
}

/// What lies between two central statements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlackBox {
    /// The central statement it comes after, if it comes after one.
    pub after: Option<Siglum>,
    /// The central statement it runs up to, if it runs up to one.
    pub before: Option<Siglum>,
    /// What is inside it.
    pub interior: Vec<Siglum>,
    /// How much of it is filled in.
    pub state: BoxState,
}

/// A line of work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// Everything it declares, in the order it declares it.
    pub nodes: Vec<Node>,
}

impl Line {
    /// Reads a line out of its source.
    ///
    /// Every fault is collected rather than the first one stopping the
    /// rest: somebody fixing a line wants the whole list.
    pub fn read(source: &str) -> Result<Self, Vec<Fault>> {
        let declarations = match source::read(source) {
            Ok(declarations) => declarations,
            Err(problem) => return Err(vec![Fault::Malformed(problem)]),
        };

        let (mut nodes, mut faults) = collect(&declarations);
        mark_proofs(&declarations, &mut nodes, &mut faults);

        let known: BTreeSet<Siglum> = nodes.iter().map(|node| node.siglum.clone()).collect();
        check_references(&nodes, &known, &mut faults);
        check_cycles(&nodes, &mut faults);

        match faults.is_empty() {
            true => Ok(Self { nodes }),
            false => Err(faults),
        }
    }

    /// The node with this siglum.
    pub fn node(&self, siglum: &Siglum) -> Option<&Node> {
        self.nodes.iter().find(|node| &node.siglum == siglum)
    }

    /// The statements the line rests on, in order.
    pub fn central(&self) -> Vec<&Node> {
        self.nodes.iter().filter(|node| node.is_central()).collect()
    }

    /// The boxes between the central statements.
    ///
    /// A box with nothing in it and a box three agents failed at look the
    /// same here, because in both cases nothing is known. What tells them
    /// apart is the record of attempts, which is not part of the source.
    pub fn boxes(&self) -> Vec<BlackBox> {
        let mut boxes = Vec::new();
        let mut interior: Vec<Siglum> = Vec::new();
        let mut after: Option<Siglum> = None;

        for node in &self.nodes {
            if node.is_central() {
                boxes.push(BlackBox {
                    after: after.clone(),
                    before: Some(node.siglum.clone()),
                    state: state_of(&interior),
                    interior: std::mem::take(&mut interior),
                });

                after = Some(node.siglum.clone());
                continue;
            }

            // A definition is a node of the graph in its own right and
            // not something buried in a box.
            if node.siglum.kind != NodeKind::Definition {
                interior.push(node.siglum.clone());
            }
        }

        // The end of a line is open: whatever comes after the last
        // central statement is a box with only one boundary, and that is
        // the ordinary state of work in progress.
        if after.is_some() || !interior.is_empty() {
            boxes.push(BlackBox {
                after,
                before: None,
                state: state_of(&interior),
                interior,
            });
        }

        boxes
    }

    /// Every dependency, as a pair.
    pub fn dependencies(&self) -> Vec<(Siglum, Siglum)> {
        self.nodes
            .iter()
            .flat_map(|node| {
                node.uses
                    .iter()
                    .map(|used| (node.siglum.clone(), used.clone()))
            })
            .collect()
    }

    /// What the graph shows, in the order the line declares it.
    ///
    /// Definitions, the statements the line rests on, and the boxes
    /// between them. Not every lemma: a line with forty of them has to
    /// stay readable.
    pub fn skeleton(&self) -> Vec<Element> {
        let mut shown = Vec::new();
        let boxes = self.boxes();
        let mut remaining = boxes.into_iter();
        let mut pending = remaining.next();

        for node in &self.nodes {
            if node.siglum.kind == NodeKind::Definition {
                shown.push(Element::Node(node.siglum.clone()));
                continue;
            }

            if node.is_central() {
                if let Some(black_box) = pending.take() {
                    shown.push(Element::Box(black_box));
                }

                shown.push(Element::Node(node.siglum.clone()));
                pending = remaining.next();
            }
        }

        if let Some(black_box) = pending {
            shown.push(Element::Box(black_box));
        }

        shown.extend(remaining.map(Element::Box));
        shown
    }
}

/// One thing the graph shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "element", rename_all = "snake_case")]
pub enum Element {
    /// A definition or a central statement.
    Node(Siglum),
    /// What lies between two of them.
    Box(BlackBox),
}

/// How much of a box is filled in.
fn state_of(interior: &[Siglum]) -> BoxState {
    match interior.is_empty() {
        true => BoxState::Empty,
        // Everything in a line's source was written by a person. What an
        // agent proposed becomes theirs when they accept it, so the
        // source never says otherwise.
        false => BoxState::FilledByHand,
    }
}

/// Reads the nodes out of the declarations.
fn collect(declarations: &[Declaration]) -> (Vec<Node>, Vec<Fault>) {
    let mut nodes: Vec<Node> = Vec::new();
    let mut faults = Vec::new();
    let mut first_seen: BTreeMap<Siglum, usize> = BTreeMap::new();

    for declaration in declarations {
        let Some(kind) = NodeKind::from_environment(&declaration.environment) else {
            continue;
        };

        let Some(written) = declaration.text("id") else {
            faults.push(Fault::Nameless {
                environment: declaration.environment.clone(),
                at: declaration.line,
            });
            continue;
        };

        let siglum = match written.parse::<Siglum>() {
            Ok(siglum) => siglum,
            Err(why) => {
                faults.push(Fault::BadSiglum {
                    written: written.to_owned(),
                    at: declaration.line,
                    why,
                });
                continue;
            }
        };

        if siglum.kind != kind {
            faults.push(Fault::KindDisagrees {
                siglum: siglum.clone(),
                declared: kind,
                at: declaration.line,
            });
            continue;
        }

        if let Some(first) = first_seen.get(&siglum) {
            faults.push(Fault::Twice {
                siglum,
                at: declaration.line,
                first: *first,
            });
            continue;
        }

        first_seen.insert(siglum.clone(), declaration.line);

        let mut uses = Vec::new();
        for written in declaration.list("uses") {
            match written.parse::<Siglum>() {
                Ok(used) if used == siglum => faults.push(Fault::UsesItself {
                    siglum: siglum.clone(),
                    at: declaration.line,
                }),
                Ok(used) => uses.push(used),
                Err(why) => faults.push(Fault::BadSiglum {
                    written,
                    at: declaration.line,
                    why,
                }),
            }
        }

        nodes.push(Node {
            siglum,
            name: declaration.text("name").map(str::to_owned),
            centrality: match declaration.flag("central") {
                Some(true) => Centrality::Central,
                _ => Centrality::Ordinary,
            },
            uses,
            proved: false,
            at: declaration.line,
        });
    }

    (nodes, faults)
}

/// Marks the statements a proof is written for.
fn mark_proofs(declarations: &[Declaration], nodes: &mut [Node], faults: &mut Vec<Fault>) {
    for declaration in declarations.iter().filter(|one| one.environment == "proof") {
        let Some(written) = declaration.text("of") else {
            faults.push(Fault::ProofOfNothing {
                at: declaration.line,
            });
            continue;
        };

        let Ok(siglum) = written.parse::<Siglum>() else {
            faults.push(Fault::BadSiglum {
                written: written.to_owned(),
                at: declaration.line,
                why: BadSiglum::NoKind(written.to_owned()),
            });
            continue;
        };

        let Some(node) = nodes.iter_mut().find(|node| node.siglum == siglum) else {
            faults.push(Fault::Unknown {
                siglum,
                by: None,
                at: declaration.line,
            });
            continue;
        };

        if !node.siglum.kind.is_statement() {
            faults.push(Fault::NotProvable {
                siglum,
                at: declaration.line,
            });
            continue;
        }

        node.proved = true;
    }
}

/// Reports what a node leans on that is not there.
fn check_references(nodes: &[Node], known: &BTreeSet<Siglum>, faults: &mut Vec<Fault>) {
    for node in nodes {
        for used in &node.uses {
            if !known.contains(used) {
                faults.push(Fault::Unknown {
                    siglum: used.clone(),
                    by: Some(node.siglum.clone()),
                    at: node.at,
                });
            }
        }
    }
}

/// Reports mathematics that rests on itself.
fn check_cycles(nodes: &[Node], faults: &mut Vec<Fault>) {
    let edges: BTreeMap<&Siglum, &Vec<Siglum>> = nodes
        .iter()
        .map(|node| (&node.siglum, &node.uses))
        .collect();

    let mut settled: BTreeSet<&Siglum> = BTreeSet::new();
    let mut found: BTreeSet<Vec<Siglum>> = BTreeSet::new();

    for node in nodes {
        let mut path: Vec<&Siglum> = Vec::new();
        walk(&node.siglum, &edges, &mut settled, &mut path, &mut found);
    }

    faults.extend(found.into_iter().map(|path| Fault::Circular { path }));
}

/// Walks the dependencies depth first, remembering the way back.
fn walk<'a>(
    siglum: &'a Siglum,
    edges: &BTreeMap<&'a Siglum, &'a Vec<Siglum>>,
    settled: &mut BTreeSet<&'a Siglum>,
    path: &mut Vec<&'a Siglum>,
    found: &mut BTreeSet<Vec<Siglum>>,
) {
    if let Some(start) = path.iter().position(|seen| *seen == siglum) {
        let mut cycle: Vec<Siglum> = path[start..].iter().map(|one| (*one).clone()).collect();

        // One cycle has as many spellings as it has members, and they are
        // all the same cycle: it is reported from its smallest name.
        let smallest = cycle
            .iter()
            .enumerate()
            .min_by_key(|(_, one)| (*one).clone())
            .map(|(at, _)| at)
            .unwrap_or(0);

        cycle.rotate_left(smallest);

        found.insert(cycle);
        return;
    }

    if settled.contains(siglum) {
        return;
    }

    path.push(siglum);

    if let Some(uses) = edges.get(siglum) {
        for used in uses.iter() {
            let known = edges.get_key_value(used).map(|(key, _)| *key);

            if let Some(used) = known {
                walk(used, edges, settled, path, found);
            }
        }
    }

    path.pop();
    settled.insert(siglum);
}

/// Something wrong with a line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Fault {
    /// The source could not be read at all.
    #[error(transparent)]
    Malformed(#[from] source::Problem),

    /// A declaration with no name, which nothing can refer to.
    #[error("line {at}: this {environment} has no `id`, so nothing can refer to it")]
    Nameless {
        /// Which environment it is.
        environment: String,
        /// Where it is.
        at: usize,
    },

    /// A siglum that will not do.
    #[error("line {at}: {why}")]
    BadSiglum {
        /// What was written.
        written: String,
        /// Where it is.
        at: usize,
        /// Why it will not do.
        why: BadSiglum,
    },

    /// A siglum whose kind is not the environment's.
    #[error("line {at}: `{siglum}` is declared as a {declared}, and its name says otherwise")]
    KindDisagrees {
        /// What was written.
        siglum: Siglum,
        /// What declared it.
        declared: NodeKind,
        /// Where it is.
        at: usize,
    },

    /// Two things with one name.
    #[error("line {at}: `{siglum}` was already declared on line {first}")]
    Twice {
        /// The name they share.
        siglum: Siglum,
        /// Where the second one is.
        at: usize,
        /// Where the first one is.
        first: usize,
    },

    /// Something leaning on what is not there.
    #[error("line {at}: `{siglum}` is not declared in this line")]
    Unknown {
        /// What was named.
        siglum: Siglum,
        /// What named it, where something did.
        by: Option<Siglum>,
        /// Where it is.
        at: usize,
    },

    /// Something leaning on itself.
    #[error("line {at}: `{siglum}` uses itself")]
    UsesItself {
        /// What does it.
        siglum: Siglum,
        /// Where it is.
        at: usize,
    },

    /// A proof of nothing in particular.
    #[error("line {at}: this proof does not say what it proves; give it `of`")]
    ProofOfNothing {
        /// Where it is.
        at: usize,
    },

    /// A proof of something that is not proved.
    #[error("line {at}: `{siglum}` is not something that can be proved")]
    NotProvable {
        /// What the proof named.
        siglum: Siglum,
        /// Where it is.
        at: usize,
    },

    /// Mathematics resting on itself.
    #[error("these rest on each other: {}", .path.iter().map(ToString::to_string).collect::<Vec<_>>().join(" → "))]
    Circular {
        /// The way round.
        path: Vec<Siglum>,
    },
}

impl Fault {
    /// Which line of the source it is about, where it is about one.
    pub fn at(&self) -> Option<usize> {
        match self {
            Self::Malformed(problem) => Some(problem.line),
            Self::Nameless { at, .. }
            | Self::BadSiglum { at, .. }
            | Self::KindDisagrees { at, .. }
            | Self::Twice { at, .. }
            | Self::Unknown { at, .. }
            | Self::UsesItself { at, .. }
            | Self::ProofOfNothing { at }
            | Self::NotProvable { at, .. } => Some(*at),
            Self::Circular { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line with two central statements and work between them.
    const ORDINARY: &str = r#"
        #definition(id: "def:quotient", name: "Quotient group")[
          Let $G$ be a group and $N$ normal in it.
        ]

        #theorem(id: "thm:first", name: "First isomorphism", central: true,
                 uses: "def:quotient")[
          Every homomorphism factors through its quotient.
        ]

        #proof(of: "thm:first")[By the universal property.]

        #lemma(id: "lem:kernel", uses: "def:quotient")[
          The kernel is normal.
        ]

        #lemma(id: "lem:image")[
          The image is a subgroup.
        ]

        #theorem(id: "thm:second", central: true, uses: ("lem:kernel", "lem:image"))[
          And so the two agree.
        ]
    "#;

    fn siglum(text: &str) -> Siglum {
        text.parse().unwrap()
    }

    #[test]
    fn a_line_is_read_with_its_nodes_in_order() {
        let line = Line::read(ORDINARY).unwrap();

        let names: Vec<String> = line
            .nodes
            .iter()
            .map(|node| node.siglum.to_string())
            .collect();

        assert_eq!(
            names,
            [
                "def:quotient",
                "thm:first",
                "lem:kernel",
                "lem:image",
                "thm:second"
            ]
        );
    }

    #[test]
    fn what_the_line_rests_on_is_what_the_source_says_it_does() {
        let line = Line::read(ORDINARY).unwrap();

        let central: Vec<String> = line
            .central()
            .iter()
            .map(|node| node.siglum.to_string())
            .collect();

        assert_eq!(central, ["thm:first", "thm:second"]);
        assert!(!line.node(&siglum("lem:kernel")).unwrap().is_central());
    }

    #[test]
    fn a_proof_in_the_source_marks_what_it_proves() {
        let line = Line::read(ORDINARY).unwrap();

        assert!(line.node(&siglum("thm:first")).unwrap().proved);
        assert!(!line.node(&siglum("thm:second")).unwrap().proved);
    }

    #[test]
    fn everything_between_two_central_statements_is_a_box() {
        let line = Line::read(ORDINARY).unwrap();
        let boxes = line.boxes();

        assert_eq!(
            boxes.len(),
            3,
            "before the first, between, and after the last"
        );

        assert_eq!(boxes[0].after, None);
        assert_eq!(
            boxes[0].before.as_ref().map(ToString::to_string).as_deref(),
            Some("thm:first")
        );
        assert_eq!(boxes[0].state, BoxState::Empty);

        assert_eq!(
            boxes[1]
                .interior
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["lem:kernel", "lem:image"]
        );
        assert_eq!(boxes[1].state, BoxState::FilledByHand);

        assert_eq!(boxes[2].before, None, "the end of a line is open");
        assert_eq!(boxes[2].state, BoxState::Empty);
    }

    #[test]
    fn a_definition_is_a_node_and_not_something_buried_in_a_box() {
        let line = Line::read(ORDINARY).unwrap();

        assert!(
            line.boxes()
                .iter()
                .all(|black_box| !black_box.interior.contains(&siglum("def:quotient")))
        );
    }

    #[test]
    fn the_graph_shows_the_definitions_the_central_statements_and_the_boxes() {
        let line = Line::read(ORDINARY).unwrap();
        let shown: Vec<String> = line
            .skeleton()
            .iter()
            .map(|element| match element {
                Element::Node(siglum) => siglum.to_string(),
                Element::Box(black_box) => format!("box of {}", black_box.interior.len()),
            })
            .collect();

        assert_eq!(
            shown,
            [
                "def:quotient",
                "box of 0",
                "thm:first",
                "box of 2",
                "thm:second",
                "box of 0",
            ]
        );
    }

    #[test]
    fn what_a_node_leans_on_is_read_out_of_the_source() {
        let line = Line::read(ORDINARY).unwrap();
        let edges: Vec<String> = line
            .dependencies()
            .iter()
            .map(|(from, to)| format!("{from} → {to}"))
            .collect();

        assert_eq!(
            edges,
            [
                "thm:first → def:quotient",
                "lem:kernel → def:quotient",
                "thm:second → lem:kernel",
                "thm:second → lem:image",
            ]
        );
    }

    #[test]
    fn a_declaration_with_no_name_is_reported_rather_than_skipped() {
        let faults = Line::read(r#"#theorem(central: true)[Nameless.]"#).unwrap_err();

        assert!(matches!(faults[0], Fault::Nameless { .. }));
        assert!(faults[0].to_string().contains("has no `id`"));
    }

    #[test]
    fn a_siglum_whose_kind_is_not_the_environments_is_refused() {
        let faults = Line::read(r#"#theorem(id: "lem:confused")[x]"#).unwrap_err();

        assert!(matches!(faults[0], Fault::KindDisagrees { .. }));
    }

    #[test]
    fn two_things_with_one_name_are_reported_with_both_places() {
        let faults =
            Line::read("#lemma(id: \"lem:a\")[one]\n#lemma(id: \"lem:a\")[another]").unwrap_err();

        let Fault::Twice { at, first, .. } = &faults[0] else {
            panic!("expected a repeat, got {:?}", faults[0]);
        };

        assert_eq!((*first, *at), (1, 2));
    }

    #[test]
    fn leaning_on_what_is_not_there_is_reported() {
        let faults = Line::read(r#"#lemma(id: "lem:a", uses: "def:missing")[x]"#).unwrap_err();

        let Fault::Unknown { siglum, by, .. } = &faults[0] else {
            panic!("expected an unknown reference, got {:?}", faults[0]);
        };

        assert_eq!(siglum.to_string(), "def:missing");
        assert_eq!(
            by.as_ref().map(ToString::to_string).as_deref(),
            Some("lem:a")
        );
    }

    #[test]
    fn leaning_on_itself_is_reported() {
        let faults = Line::read(r#"#lemma(id: "lem:a", uses: "lem:a")[x]"#).unwrap_err();

        assert!(matches!(faults[0], Fault::UsesItself { .. }));
    }

    #[test]
    fn mathematics_that_rests_on_itself_is_reported_once_and_in_full() {
        let faults = Line::read(
            r#"
            #lemma(id: "lem:a", uses: "lem:c")[x]
            #lemma(id: "lem:b", uses: "lem:a")[x]
            #lemma(id: "lem:c", uses: "lem:b")[x]
            "#,
        )
        .unwrap_err();

        let circles: Vec<&Fault> = faults
            .iter()
            .filter(|fault| matches!(fault, Fault::Circular { .. }))
            .collect();

        assert_eq!(circles.len(), 1, "one cycle, however many ways round it");

        let said = circles[0].to_string();
        for member in ["lem:a", "lem:b", "lem:c"] {
            assert!(said.contains(member), "{said} left out {member}");
        }
    }

    #[test]
    fn a_proof_of_something_that_is_not_there_is_reported() {
        let faults = Line::read(r#"#proof(of: "thm:missing")[x]"#).unwrap_err();

        assert!(matches!(faults[0], Fault::Unknown { by: None, .. }));
    }

    #[test]
    fn a_proof_of_a_definition_is_reported() {
        let faults = Line::read(
            r#"
            #definition(id: "def:a")[x]
            #proof(of: "def:a")[y]
            "#,
        )
        .unwrap_err();

        assert!(matches!(faults[0], Fault::NotProvable { .. }));
    }

    #[test]
    fn every_fault_is_collected_rather_than_only_the_first() {
        let faults = Line::read(
            r#"
            #lemma(id: "lem:a", uses: "def:missing")[x]
            #theorem(id: "not-a-siglum")[y]
            #proof(of: "thm:nowhere")[z]
            "#,
        )
        .unwrap_err();

        assert_eq!(faults.len(), 3);
    }

    #[test]
    fn a_source_that_cannot_be_read_says_so_once() {
        let faults = Line::read(r#"#theorem(id: "thm:a"[x]"#).unwrap_err();

        assert_eq!(faults.len(), 1);
        assert!(matches!(faults[0], Fault::Malformed(_)));
    }

    #[test]
    fn a_line_with_nothing_in_it_is_a_line_with_nothing_in_it() {
        let line = Line::read("= A heading and nothing else\n").unwrap();

        assert!(line.nodes.is_empty());
        assert!(line.boxes().is_empty());
        assert!(line.skeleton().is_empty());
    }

    #[test]
    fn a_siglum_is_written_and_read_the_same_way() {
        for written in [
            "thm:main",
            "def:quotient-group",
            "lem:step-2",
            "thm:1",
            "lem:a",
        ] {
            assert_eq!(written.parse::<Siglum>().unwrap().to_string(), written);
        }
    }

    #[test]
    fn what_is_not_a_siglum_says_which_part_is_wrong() {
        assert!(matches!(
            "main".parse::<Siglum>(),
            Err(BadSiglum::NoKind(_))
        ));
        assert!(matches!(
            "xyz:main".parse::<Siglum>(),
            Err(BadSiglum::UnknownKind(_))
        ));
        assert!(matches!(
            "thm:Main".parse::<Siglum>(),
            Err(BadSiglum::Name(BadName::BadCharacter('M')))
        ));
        assert!(matches!(
            "thm:".parse::<Siglum>(),
            Err(BadSiglum::Name(BadName::Empty))
        ));
        assert!(matches!(
            "thm:a--b".parse::<Siglum>(),
            Err(BadSiglum::Name(BadName::DoubleHyphen))
        ));
        assert!(matches!(
            "thm:-a".parse::<Siglum>(),
            Err(BadSiglum::Name(BadName::Hyphenated))
        ));
    }
}
