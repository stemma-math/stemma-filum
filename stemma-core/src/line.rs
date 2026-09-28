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
//!
//! What is read here is the line's [`Document`]: the definitions and
//! theorems in it become nodes, its proofs mark what they prove, and
//! what a statement leans on is what its author declared plus what its
//! text and its proofs cite.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::document::{self, Block, BlockKind, Document, Unreadable};
use crate::id::{BlockId, BridgeId, UserId, WordFault, check_word};
use crate::model::{BoxState, Centrality};

/// What kind of thing a node is.
///
/// It is what a siglum's prefix says, so that the siglum alone tells a
/// definition from a result wherever it travels. Every kind but the
/// definition is a theorem block; which of them a theorem is follows
/// from the name its author gives the environment.
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
        }
    }

    /// What it is called in a sentence.
    pub fn word(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Theorem => "theorem",
            Self::Lemma => "lemma",
            Self::Proposition => "proposition",
            Self::Corollary => "corollary",
        }
    }

    /// The kind a theorem block is, given what its environment is called.
    ///
    /// An environment with a name of its own — `Claim`, `Teorema` — is a
    /// theorem, which is what every result is when nothing says otherwise.
    pub fn for_environment(environment: &str) -> Self {
        match environment.trim().to_lowercase().as_str() {
            "lemma" => Self::Lemma,
            "proposition" => Self::Proposition,
            "corollary" => Self::Corollary,
            _ => Self::Theorem,
        }
    }

    /// The kind a prefix names.
    pub fn from_prefix(prefix: &str) -> Option<Self> {
        Self::all().into_iter().find(|kind| kind.prefix() == prefix)
    }

    /// Every kind.
    pub fn all() -> [Self; 5] {
        [
            Self::Definition,
            Self::Theorem,
            Self::Lemma,
            Self::Proposition,
            Self::Corollary,
        ]
    }

    /// Whether it is something that can be proved.
    pub fn is_statement(self) -> bool {
        !matches!(self, Self::Definition)
    }

    /// The block that declares a node of this kind.
    pub fn block(self) -> BlockKind {
        match self {
            Self::Definition => BlockKind::Definition,
            _ => BlockKind::Theorem,
        }
    }
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
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

        check_word(input).map_err(|fault| match fault {
            WordFault::Character(character) => BadName::BadCharacter(character),
            WordFault::DoubleHyphen => BadName::DoubleHyphen,
        })?;

        if input.starts_with('-') || input.ends_with('-') {
            return Err(BadName::Hyphenated);
        }

        Ok(Self(input.to_owned()))
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// What a line called this would be called on disk.
    ///
    /// A directory name is not a title: it is lowercase, it has no
    /// accents and no punctuation, and it is short. Somebody who wants
    /// a different one renames the directory, which is the ordinary way
    /// to rename anything in a repository.
    pub fn from_title(title: &str) -> Option<Self> {
        let mut name = String::new();

        for character in title.trim().chars() {
            let plain = plainly(character);

            for character in plain.chars() {
                match character {
                    'a'..='z' | '0'..='9' => name.push(character),
                    'A'..='Z' => name.push(character.to_ascii_lowercase()),
                    _ if name.ends_with('-') => {}
                    _ if name.is_empty() => {}
                    _ => name.push('-'),
                }
            }
        }

        while name.len() > LONGEST_NAME {
            name.pop();
        }

        Self::parse(name.trim_end_matches('-')).ok()
    }
}

/// One letter without whatever is written over it.
///
/// Enough of Latin-1 for the languages this is written in; anything
/// else becomes a hyphen, which is honest and still a usable name.
fn plainly(character: char) -> String {
    match character {
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' => "a".to_owned(),
        'é' | 'è' | 'ë' | 'ê' => "e".to_owned(),
        'í' | 'ì' | 'ï' | 'î' => "i".to_owned(),
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' => "o".to_owned(),
        'ú' | 'ù' | 'ü' | 'û' => "u".to_owned(),
        'ñ' => "n".to_owned(),
        'ç' => "c".to_owned(),
        'ß' => "ss".to_owned(),
        'Á' | 'À' | 'Ä' | 'Â' | 'Ã' | 'Å' => "A".to_owned(),
        'É' | 'È' | 'Ë' | 'Ê' => "E".to_owned(),
        'Í' | 'Ì' | 'Ï' | 'Î' => "I".to_owned(),
        'Ó' | 'Ò' | 'Ö' | 'Ô' | 'Õ' => "O".to_owned(),
        'Ú' | 'Ù' | 'Ü' | 'Û' => "U".to_owned(),
        'Ñ' => "N".to_owned(),
        'Ç' => "C".to_owned(),
        other => other.to_string(),
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

serde_as_text!(Name);

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

serde_as_text!(Siglum);

/// One thing a line declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// What it is called.
    pub siglum: Siglum,
    /// The block that declares it.
    pub block: BlockId,
    /// What its environment is called on the page.
    pub environment: String,
    /// What it is called in prose, where the author gave it one.
    pub name: Option<String>,
    /// Whether the line rests on it.
    pub centrality: Centrality,
    /// What it leans on: what its author declared, then what its text
    /// and its proofs cite.
    pub uses: Vec<Siglum>,
    /// Audited substitution licences this statement relies on.
    #[serde(default)]
    pub bridges: Vec<BridgeId>,
    /// The person responsible for downstream decisions, when declared.
    #[serde(default)]
    pub author: Option<UserId>,
    /// Whether a proof of it has been written.
    pub proved: bool,
    /// Which block of the line declares it, counting from one.
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
    /// The central block it comes after, if it comes after one.
    pub after: Option<BlockId>,
    /// The central block it runs up to, if it runs up to one.
    pub before: Option<BlockId>,
    /// The statement blocks inside it, in narrative order.
    pub interior: Vec<BlockId>,
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
        let document = Document::parse(source).map_err(|unreadable| vec![unreadable.into()])?;
        Self::of(&document)
    }

    /// Reads a line out of a document already read.
    pub fn of(document: &Document) -> Result<Self, Vec<Fault>> {
        let mut faults = Vec::new();
        check_blocks(document, &mut faults);

        let mut nodes = collect(document, &mut faults);
        check_proofs(document, &mut nodes, &mut faults);
        lean(document, &mut nodes, &mut faults);
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

    /// The node declared by this block, where it declares one.
    pub fn node_at(&self, block: &BlockId) -> Option<&Node> {
        self.nodes.iter().find(|node| &node.block == block)
    }

    /// The statements the line rests on, in order: the central results
    /// that divide it into boxes.
    ///
    /// Definitions are left out even when they are central. They are
    /// nodes of their own, frozen like any central block, but they are
    /// not the ends of a box.
    pub fn central(&self) -> Vec<&Node> {
        self.nodes
            .iter()
            .filter(|node| node.is_central() && node.siglum.kind != NodeKind::Definition)
            .collect()
    }

    /// The boxes between the central statements.
    ///
    /// A box with nothing in it and a box three agents failed at look the
    /// same here, because in both cases nothing is known. What tells them
    /// apart is the record of attempts, which is not part of the source.
    pub fn boxes(&self) -> Vec<BlackBox> {
        let mut boxes = Vec::new();
        let mut interior: Vec<BlockId> = Vec::new();
        let mut after: Option<BlockId> = None;

        for node in &self.nodes {
            // A definition is a node of the graph in its own right: it is
            // neither buried in a box nor one of a box's ends, central or
            // not.
            if node.siglum.kind == NodeKind::Definition {
                continue;
            }

            if node.is_central() {
                boxes.push(BlackBox {
                    after,
                    before: Some(node.block),
                    state: state_of(&interior),
                    interior: std::mem::take(&mut interior),
                });

                after = Some(node.block);
                continue;
            }

            interior.push(node.block);
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
fn state_of(interior: &[BlockId]) -> BoxState {
    match interior.is_empty() {
        true => BoxState::Empty,
        // Everything in a line's source was written by a person. What an
        // agent proposed becomes theirs when they accept it, so the
        // source never says otherwise.
        false => BoxState::FilledByHand,
    }
}

/// Reports two blocks with one identifier, which nothing could tell apart.
fn check_blocks(document: &Document, faults: &mut Vec<Fault>) {
    let mut first_seen: BTreeMap<&BlockId, usize> = BTreeMap::new();

    for (at, block) in numbered(document) {
        match first_seen.get(block.id()) {
            Some(first) => faults.push(Fault::RepeatedBlock {
                id: *block.id(),
                at,
                first: *first,
            }),
            None => {
                first_seen.insert(block.id(), at);
            }
        }
    }
}

/// The blocks, counting from one.
fn numbered(document: &Document) -> impl Iterator<Item = (usize, &Block)> {
    document
        .blocks
        .iter()
        .enumerate()
        .map(|(at, block)| (at + 1, block))
}

/// Reads the nodes out of the statements.
fn collect(document: &Document, faults: &mut Vec<Fault>) -> Vec<Node> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut first_seen: BTreeMap<Siglum, usize> = BTreeMap::new();

    for (at, block) in numbered(document) {
        let (Some(statement), Some(siglum)) = (block.statement(), block.siglum()) else {
            continue;
        };

        if siglum.kind.block() != block.kind() {
            faults.push(Fault::KindDisagrees {
                siglum,
                declared: block.kind(),
                at,
            });
            continue;
        }

        if let Some(first) = first_seen.get(&siglum) {
            faults.push(Fault::Twice {
                siglum,
                at,
                first: *first,
            });
            continue;
        }
        first_seen.insert(siglum.clone(), at);

        nodes.push(Node {
            siglum,
            block: statement.id,
            environment: statement.environment.clone(),
            name: statement.title.clone(),
            centrality: match statement.central {
                true => Centrality::Central,
                false => Centrality::Ordinary,
            },
            uses: Vec::new(),
            bridges: statement.bridges.clone(),
            author: statement.author.clone(),
            proved: false,
            at,
        });
    }

    nodes
}

/// Checks every proof against what it proves, and marks what is proved.
fn check_proofs(document: &Document, nodes: &mut [Node], faults: &mut Vec<Fault>) {
    let mut proved_at: BTreeMap<BlockId, usize> = BTreeMap::new();

    for (at, block) in numbered(document) {
        let Block::Proof(proof) = block else {
            continue;
        };

        let Some(proved) = document.block(&proof.of) else {
            faults.push(Fault::ProofOfMissing { of: proof.of, at });
            continue;
        };

        let Some(statement) = proved.statement() else {
            faults.push(Fault::NotProvable {
                of: proof.of,
                kind: proved.kind(),
                at,
            });
            continue;
        };

        let siglum = proved.siglum().expect("a statement has a siglum");

        if !statement.proof {
            faults.push(Fault::ProofNotCarried { siglum, at });
            continue;
        }

        if let Some(first) = proved_at.get(&proof.of) {
            faults.push(Fault::ProofTwice {
                siglum,
                at,
                first: *first,
            });
            continue;
        }
        proved_at.insert(proof.of, at);

        if !proof.content.trim().is_empty()
            && let Some(node) = nodes.iter_mut().find(|node| node.block == proof.of)
        {
            node.proved = true;
        }
    }

    for (at, block) in numbered(document) {
        if let Some(statement) = block.statement()
            && statement.proof
            && !proved_at.contains_key(&statement.id)
        {
            faults.push(Fault::ProofMissing {
                siglum: block.siglum().expect("a statement has a siglum"),
                at,
            });
        }
    }
}

/// Works out what every node leans on.
///
/// First what its author declared, in the order they declared it; then
/// what its text and its proofs cite, in the order they cite it. A
/// citation of something that is not a statement of this line — an
/// equation, a figure, a line elsewhere — is not a dependency.
fn lean(document: &Document, nodes: &mut [Node], faults: &mut Vec<Fault>) {
    let by_block: BTreeMap<BlockId, Siglum> = nodes
        .iter()
        .map(|node| (node.block, node.siglum.clone()))
        .collect();
    let by_label: BTreeMap<String, Siglum> = nodes
        .iter()
        .map(|node| (node.siglum.to_string(), node.siglum.clone()))
        .collect();

    for node in nodes.iter_mut() {
        let Some(statement) = document.block(&node.block).and_then(Block::statement) else {
            continue;
        };
        let mut uses: Vec<Siglum> = Vec::new();

        for used in &statement.uses {
            if used == &node.block {
                faults.push(Fault::UsesItself {
                    siglum: node.siglum.clone(),
                    at: node.at,
                });
                continue;
            }

            match (by_block.get(used), document.block(used)) {
                (Some(siglum), _) => {
                    if !uses.contains(siglum) {
                        uses.push(siglum.clone());
                    }
                }
                (None, Some(block)) if block.statement().is_none() => {
                    faults.push(Fault::UsesWhatIsNotAStatement {
                        by: node.siglum.clone(),
                        block: *used,
                        kind: block.kind(),
                        at: node.at,
                    });
                }
                // A statement that is there and did not become a node
                // already has a fault of its own.
                (None, Some(_)) => {}
                (None, None) => faults.push(Fault::UsesMissing {
                    by: node.siglum.clone(),
                    block: *used,
                    at: node.at,
                }),
            }
        }

        let cited = std::iter::once(statement.content.as_str())
            .chain(
                document
                    .proofs_of(&node.block)
                    .map(|proof| proof.content.as_str()),
            )
            .flat_map(document::references);

        for label in cited {
            if let Some(siglum) = by_label.get(&label)
                && siglum != &node.siglum
                && !uses.contains(siglum)
            {
                uses.push(siglum.clone());
            }
        }

        node.uses = uses;
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
    Unreadable(#[from] Unreadable),

    /// Two blocks with one identifier.
    #[error("block {at}: `{id}` is already the identifier of block {first}")]
    RepeatedBlock {
        /// The identifier they share.
        id: BlockId,
        /// Where the second one is.
        at: usize,
        /// Where the first one is.
        first: usize,
    },

    /// A siglum whose kind is not the block's.
    #[error("block {at}: `{siglum}` names a {}, and this block is a {declared}", .siglum.kind)]
    KindDisagrees {
        /// What was written.
        siglum: Siglum,
        /// What the block is.
        declared: BlockKind,
        /// Where it is.
        at: usize,
    },

    /// Two statements with one name.
    #[error("block {at}: `{siglum}` is already the name of block {first}")]
    Twice {
        /// The name they share.
        siglum: Siglum,
        /// Where the second one is.
        at: usize,
        /// Where the first one is.
        first: usize,
    },

    /// A statement leaning on a block that is not there.
    #[error("block {at}: `{by}` uses `{block}`, which is not in this line")]
    UsesMissing {
        /// The statement that leans.
        by: Siglum,
        /// What it names.
        block: BlockId,
        /// Where the statement is.
        at: usize,
    },

    /// A statement leaning on prose or on a proof.
    #[error("block {at}: `{by}` uses a {kind} block, and only a statement can be used")]
    UsesWhatIsNotAStatement {
        /// The statement that leans.
        by: Siglum,
        /// What it names.
        block: BlockId,
        /// What that block is.
        kind: BlockKind,
        /// Where the statement is.
        at: usize,
    },

    /// A statement leaning on itself.
    #[error("block {at}: `{siglum}` uses itself")]
    UsesItself {
        /// What does it.
        siglum: Siglum,
        /// Where it is.
        at: usize,
    },

    /// A proof of a block that is not there.
    #[error("block {at}: this proof is of `{of}`, which is not in this line")]
    ProofOfMissing {
        /// What it names.
        of: BlockId,
        /// Where the proof is.
        at: usize,
    },

    /// A proof of prose, or of another proof.
    #[error("block {at}: this proof is of a {kind} block, which is not something to prove")]
    NotProvable {
        /// What it names.
        of: BlockId,
        /// What that block is.
        kind: BlockKind,
        /// Where the proof is.
        at: usize,
    },

    /// A proof of a statement that says it carries none.
    #[error("block {at}: `{siglum}` says it is not proved here, and this is a proof of it")]
    ProofNotCarried {
        /// The statement.
        siglum: Siglum,
        /// Where the proof is.
        at: usize,
    },

    /// A statement that carries a proof with no proof block.
    #[error("block {at}: `{siglum}` carries a proof and there is no proof block for it")]
    ProofMissing {
        /// The statement.
        siglum: Siglum,
        /// Where the statement is.
        at: usize,
    },

    /// Two proofs of one statement.
    #[error("block {at}: `{siglum}` already has its proof in block {first}")]
    ProofTwice {
        /// The statement.
        siglum: Siglum,
        /// Where the second proof is.
        at: usize,
        /// Where the first one is.
        first: usize,
    },

    /// Mathematics resting on itself.
    #[error("these rest on each other: {}", .path.iter().map(ToString::to_string).collect::<Vec<_>>().join(" → "))]
    Circular {
        /// The way round.
        path: Vec<Siglum>,
    },
}

impl Fault {
    /// Which block of the line it is about, counting from one, where it
    /// is about one.
    pub fn at(&self) -> Option<usize> {
        match self {
            Self::RepeatedBlock { at, .. }
            | Self::KindDisagrees { at, .. }
            | Self::Twice { at, .. }
            | Self::UsesMissing { at, .. }
            | Self::UsesWhatIsNotAStatement { at, .. }
            | Self::UsesItself { at, .. }
            | Self::ProofOfMissing { at, .. }
            | Self::NotProvable { at, .. }
            | Self::ProofNotCarried { at, .. }
            | Self::ProofMissing { at, .. }
            | Self::ProofTwice { at, .. } => Some(*at),
            Self::Unreadable(_) | Self::Circular { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_becomes_a_name_a_directory_can_carry() {
        let named = |title: &str| Name::from_title(title).map(|name| name.as_str().to_owned());

        assert_eq!(named("Quotient groups"), Some("quotient-groups".to_owned()));
        assert_eq!(
            named("  Álgebra: ¿se parte?  "),
            Some("algebra-se-parte".to_owned())
        );
        assert_eq!(named("Two  spaces"), Some("two-spaces".to_owned()));
        assert_eq!(named("§§§"), None);
        assert_eq!(named(""), None);
        assert!(named(&"a very long title ".repeat(10)).unwrap().len() <= LONGEST_NAME);
        assert!(
            !named(&"a very long title ".repeat(10))
                .unwrap()
                .ends_with('-')
        );
    }

    fn siglum(text: &str) -> Siglum {
        text.parse().unwrap()
    }

    /// A statement with a name and no proof to write, which keeps these
    /// tests about what they are about.
    fn stated(named: &str, content: &str) -> Block {
        let siglum = siglum(named);
        let mut block = match siglum.kind {
            NodeKind::Definition => Block::definition(Some(siglum), content),
            kind => Block::theorem(kind.word(), Some(siglum), content),
        };
        let statement = block.statement_mut().unwrap();
        statement.proof = false;
        statement.central = false;
        block
    }

    fn central(mut block: Block) -> Block {
        block.statement_mut().unwrap().central = true;
        block
    }

    fn using(mut block: Block, used: &Block) -> Block {
        block.statement_mut().unwrap().uses.push(*used.id());
        block
    }

    fn proved(mut block: Block) -> Block {
        block.statement_mut().unwrap().proof = true;
        block
    }

    fn written(blocks: Vec<Block>) -> String {
        let mut document = Document::new("A line", None);
        document.blocks = blocks;
        document.to_text()
    }

    /// A line with two central statements and work between them.
    fn ordinary() -> String {
        let quotient = stated("def:quotient", "Let $G$ be a group and $N$ normal in it.");
        let first = proved(central(stated(
            "thm:first",
            "Every homomorphism factors through @def:quotient.",
        )));
        let proof = Block::proof(*first.id(), "By the universal property.");
        let kernel = using(stated("lem:kernel", "The kernel is normal."), &quotient);
        let image = stated("lem:image", "The image is a subgroup.");
        let second = central(stated(
            "thm:second",
            "And so, by @lem:kernel and @lem:image, the two agree.",
        ));

        written(vec![
            Block::prose("We start from quotients."),
            quotient,
            first,
            proof,
            kernel,
            image,
            second,
        ])
    }

    #[test]
    fn a_line_is_read_with_its_nodes_in_order() {
        let line = Line::read(&ordinary()).unwrap();

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
        assert_eq!(line.nodes[0].at, 2, "prose counts as a block");
    }

    #[test]
    fn what_the_line_rests_on_is_what_the_source_says_it_does() {
        let line = Line::read(&ordinary()).unwrap();

        let central: Vec<String> = line
            .central()
            .iter()
            .map(|node| node.siglum.to_string())
            .collect();

        assert_eq!(central, ["thm:first", "thm:second"]);
        assert!(!line.node(&siglum("lem:kernel")).unwrap().is_central());
    }

    #[test]
    fn a_written_proof_marks_what_it_proves_and_an_empty_one_does_not() {
        let line = Line::read(&ordinary()).unwrap();

        assert!(line.node(&siglum("thm:first")).unwrap().proved);
        assert!(!line.node(&siglum("thm:second")).unwrap().proved);

        let theorem = proved(stated("thm:open", "Open."));
        let empty = Block::proof(*theorem.id(), "  \n");
        let line = Line::read(&written(vec![theorem, empty])).unwrap();
        assert!(!line.nodes[0].proved, "an empty proof is still to write");
    }

    #[test]
    fn everything_between_two_central_statements_is_a_box() {
        let line = Line::read(&ordinary()).unwrap();
        let boxes = line.boxes();

        assert_eq!(
            boxes.len(),
            3,
            "before the first, between, and after the last"
        );

        assert_eq!(boxes[0].after, None);
        assert_eq!(
            boxes[0].before,
            Some(line.node(&siglum("thm:first")).unwrap().block)
        );
        assert_eq!(boxes[0].state, BoxState::Empty);

        assert_eq!(
            boxes[1].interior,
            [
                line.node(&siglum("lem:kernel")).unwrap().block,
                line.node(&siglum("lem:image")).unwrap().block,
            ]
        );
        assert_eq!(boxes[1].state, BoxState::FilledByHand);

        assert_eq!(boxes[2].before, None, "the end of a line is open");
        assert_eq!(boxes[2].state, BoxState::Empty);
    }

    #[test]
    fn a_definition_is_neither_inside_a_box_nor_one_of_its_ends() {
        let definition = Block::definition(Some(siglum("def:group")), "A group.");
        let lemma = stated("lem:a", "A step.");
        let line = Line::read(&written(vec![definition, lemma])).unwrap();

        assert!(line.nodes[0].is_central(), "definitions are central");
        let boxes = line.boxes();
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].after, None);
        assert_eq!(
            boxes[0].interior,
            [line.node(&siglum("lem:a")).unwrap().block]
        );
    }

    #[test]
    fn the_graph_shows_the_definitions_the_central_statements_and_the_boxes() {
        let line = Line::read(&ordinary()).unwrap();
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
    fn what_a_node_leans_on_is_declared_or_cited() {
        let line = Line::read(&ordinary()).unwrap();
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
    fn what_a_proof_cites_is_leaned_on_by_what_it_proves() {
        let lemma = stated("lem:step", "A step.");
        let theorem = proved(stated("thm:main", "The result."));
        let proof = Block::proof(*theorem.id(), "By @lem:step, and @eq:unrelated.");
        let line = Line::read(&written(vec![lemma, theorem, proof])).unwrap();

        assert_eq!(line.nodes[1].uses, [siglum("lem:step")]);
    }

    #[test]
    fn a_statement_nobody_named_is_still_a_node() {
        let theorem = Block::theorem("Lemma", None, "Unnamed.");
        let proof = Block::proof(*theorem.id(), "");
        let line = Line::read(&written(vec![theorem, proof])).unwrap();

        assert_eq!(line.nodes[0].siglum.kind, NodeKind::Lemma);
        assert_eq!(line.nodes[0].environment, "Lemma");
    }

    #[test]
    fn a_siglum_whose_kind_is_not_the_blocks_is_refused() {
        let mut theorem = stated("lem:confused", "x");
        theorem = match theorem {
            Block::Theorem(statement) => Block::Definition(statement),
            other => other,
        };
        let faults = Line::read(&written(vec![theorem])).unwrap_err();

        assert!(matches!(faults[0], Fault::KindDisagrees { .. }));
    }

    #[test]
    fn two_things_with_one_name_are_reported_with_both_places() {
        let faults = Line::read(&written(vec![
            stated("lem:a", "one"),
            stated("lem:a", "another"),
        ]))
        .unwrap_err();

        let Fault::Twice { at, first, .. } = &faults[0] else {
            panic!("expected a repeat, got {:?}", faults[0]);
        };

        assert_eq!((*first, *at), (1, 2));
    }

    #[test]
    fn two_blocks_with_one_identifier_are_reported() {
        let prose = Block::prose("x");
        let faults = Line::read(&written(vec![prose.clone(), prose])).unwrap_err();

        assert!(matches!(faults[0], Fault::RepeatedBlock { .. }));
    }

    #[test]
    fn leaning_on_what_is_not_there_is_reported() {
        let gone = stated("def:missing", "x");
        let faults = Line::read(&written(vec![using(stated("lem:a", "x"), &gone)])).unwrap_err();

        let Fault::UsesMissing { by, .. } = &faults[0] else {
            panic!("expected an unknown reference, got {:?}", faults[0]);
        };
        assert_eq!(by.to_string(), "lem:a");
    }

    #[test]
    fn leaning_on_prose_is_reported() {
        let prose = Block::prose("An idea.");
        let lemma = using(stated("lem:a", "x"), &prose);
        let faults = Line::read(&written(vec![prose, lemma])).unwrap_err();

        assert!(matches!(
            faults[0],
            Fault::UsesWhatIsNotAStatement {
                kind: BlockKind::Prose,
                ..
            }
        ));
    }

    #[test]
    fn leaning_on_itself_is_reported_and_citing_itself_is_not() {
        let lemma = stated("lem:a", "As @lem:a says.");
        let own = lemma.clone();
        assert!(Line::read(&written(vec![lemma])).is_ok());

        let faults = Line::read(&written(vec![using(own.clone(), &own)])).unwrap_err();
        assert!(matches!(faults[0], Fault::UsesItself { .. }));
    }

    #[test]
    fn mathematics_that_rests_on_itself_is_reported_once_and_in_full() {
        let faults = Line::read(&written(vec![
            stated("lem:a", "By @lem:c."),
            stated("lem:b", "By @lem:a."),
            stated("lem:c", "By @lem:b."),
        ]))
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
        let faults =
            Line::read(&written(vec![Block::proof(BlockId::generate(), "x")])).unwrap_err();

        assert!(matches!(faults[0], Fault::ProofOfMissing { .. }));
    }

    #[test]
    fn a_proof_of_prose_is_reported() {
        let prose = Block::prose("x");
        let proof = Block::proof(*prose.id(), "y");
        let faults = Line::read(&written(vec![prose, proof])).unwrap_err();

        assert!(matches!(faults[0], Fault::NotProvable { .. }));
    }

    #[test]
    fn a_definition_may_carry_a_proof_when_it_says_so() {
        let definition = proved(stated("def:well", "Well defined."));
        let proof = Block::proof(*definition.id(), "It is.");
        let line = Line::read(&written(vec![definition, proof])).unwrap();

        assert!(line.nodes[0].proved);
    }

    #[test]
    fn a_proof_of_what_says_it_is_not_proved_here_is_reported() {
        let theorem = stated("thm:quoted", "From elsewhere.");
        let proof = Block::proof(*theorem.id(), "Anyway.");
        let faults = Line::read(&written(vec![theorem, proof])).unwrap_err();

        assert!(matches!(faults[0], Fault::ProofNotCarried { .. }));
    }

    #[test]
    fn a_theorem_carrying_a_proof_needs_its_proof_block() {
        let theorem = Block::theorem("Theorem", Some(siglum("thm:main")), "x");
        let faults = Line::read(&written(vec![theorem.clone()])).unwrap_err();
        assert!(matches!(faults[0], Fault::ProofMissing { at: 1, .. }));

        let first = Block::proof(*theorem.id(), "One.");
        let second = Block::proof(*theorem.id(), "Two.");
        let faults = Line::read(&written(vec![theorem, first, second])).unwrap_err();
        assert!(matches!(
            faults[0],
            Fault::ProofTwice {
                at: 3,
                first: 2,
                ..
            }
        ));
    }

    #[test]
    fn every_fault_is_collected_rather_than_only_the_first() {
        let gone = stated("def:missing", "x");
        let faults = Line::read(&written(vec![
            using(stated("lem:a", "x"), &gone),
            Block::theorem("Theorem", Some(siglum("thm:b")), "y"),
            Block::proof(BlockId::generate(), "z"),
        ]))
        .unwrap_err();

        assert_eq!(faults.len(), 3);
    }

    #[test]
    fn a_source_that_cannot_be_read_says_so_once() {
        let faults = Line::read("{\"format\": 1, \"title\": ").unwrap_err();

        assert_eq!(faults.len(), 1);
        assert!(matches!(faults[0], Fault::Unreadable(_)));
    }

    #[test]
    fn a_line_with_nothing_in_it_is_a_line_with_nothing_in_it() {
        let line = Line::read(&written(vec![Block::prose("Just an idea.")])).unwrap();

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
