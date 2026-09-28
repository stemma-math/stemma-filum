// Shapes of what the desktop shell's commands return (`stemma-app/src`).

export type Signed = { server: string; user: string };

export type Beginning = { code: string; url: string; url_complete: string; interval: number };

/** Where a line stands: everything sent, something here not sent yet, or
 * a block changed here and elsewhere that needs a decision. */
export type LineState = "sent" | "not_sent" | "needs_decision";

export type Known = {
  group: string;
  side: string;
  name: string;
  title: string | null;
  description: string | null;
  path: string;
  state: LineState;
};

/** One line that is not simply sent. */
export type Standing = {
  id: string;
  group: string;
  side: string;
  name: string;
  state: LineState;
  /** Why it was not sent, the last time that was tried. */
  trouble: string | null;
};

/** What the engine did in its last round. */
export type Round = {
  arrived: string[];
  sent: string[];
  merged: string[];
  deciding: string[];
  gone: string[];
  aside: { line: string; path: string; why: string }[];
  waiting: { line: string; why: string }[];
  stopped: string | null;
};

/** Where synchronisation stands, as the engine says after every round. */
export type Mirrored = {
  /** Whether the last round reached the server. */
  connected: boolean;
  reason: string | null;
  /** Every line not sent yet or needing a decision. */
  lines: Standing[];
  report: Round;
  /** The repositories whose pages were built and published. */
  published: string[];
};

export type Entry = { group: string; side: string; name: string; path: string };

/** A block of a line (`stemma_core::document::Block`). */
export type Prose = { kind: "prose"; id: string; content: string };

export type Statement = {
  kind: "definition" | "theorem";
  id: string;
  environment: string;
  siglum?: string;
  title?: string;
  central: boolean;
  proof: boolean;
  uses?: string[];
  bridges?: string[];
  author?: string;
  content: string;
};

export type Proof = { kind: "proof"; id: string; of: string; content: string };

export type Block = Prose | Statement | Proof;

export type BlockKind = Block["kind"];

/** A line's source, `line.json`. */
export type LineDocument = {
  format: number;
  title: string;
  description?: string;
  blocks: Block[];
};

/** What the line reads as (`stemma_core::line::Node`). */
export type Node = {
  siglum: string;
  block: string;
  environment: string;
  name: string | null;
  centrality: "central" | "ordinary";
  uses: string[];
  proved: boolean;
  at: number;
};

export type LeanDigest = {
  lean: string;
  mathlib: string;
  compiled: boolean;
  blocks: {
    block: string;
    state: "verified" | "refuted" | "unverified";
    diagnostic: string;
    declarations: {
      name: string;
      module: string;
      statement: string;
      sorry_free: boolean;
      axioms: string[];
      reaches: string[];
      statement_closure: string;
    }[];
  }[];
  diagnostic: string;
};

export type FormalReview = {
  blocks: {
    check: LeanDigest["blocks"][number];
    reading: string;
  }[];
  impact: {
    proofs_only: boolean;
    statement_closures: string[];
    line_links: string[];
  };
};

/** Something wrong with a line; `at` is the block, counting from one. */
export type Fault = { at: number | null; message: string };

export type OpenLine = {
  entry: Entry;
  document: LineDocument | null;
  source: string;
  line: { nodes: Node[] } | null;
  faults: Fault[];
  lean: LeanDigest | null;
};

/** What Typst said, placed in the block and line it is about. */
export type Diagnostic = { error: boolean; block: string | null; line: number | null; message: string };

export type Preview = { svg: string | null; diagnostics: Diagnostic[] };

export type Agent = { name: string; called: string; availability: string; reason: string | null };

export type Job = {
  id: string;
  state: string;
  task: string;
  agent: string;
  said: string;
  raw_output: boolean;
};

export type Machine = { label: string; presence: string; agents: Agent[]; jobs: Job[] };

export type Offer = { machine: string; label: string; presence: string; agents: Agent[] };

export type Claim = {
  standing: "writing" | "waiting" | "open";
  label: string | null;
  left: number | null;
  asked: boolean;
  said: string;
};

export type Keep = "here" | "there";

export type Conflict = {
  block: string;
  label: string;
  here: string | null;
  there: string | null;
  said: string;
};

export type Decision = { block: string; keep: Keep };

export type Waiting = {
  proposal: string;
  group: string;
  line: string;
  task: string;
  agent: string;
  vendor: string;
  model: string;
};

export type Moved = {
  change: "added" | "removed" | "kept";
  text: string;
  before: number | null;
  after: number | null;
  frozen: boolean;
};

export type Review = {
  proposal: string;
  group: string;
  side: string;
  line: string;
  task: string;
  agent: string;
  vendor: string;
  model: string;
  prompt_revision: string;
  state: string;
  extent: string;
  rows: Moved[];
  source: string;
  lean_source: string | null;
  formal_review: FormalReview | null;
  library_files: { path: string; rows: Moved[] }[];
  findings: string[];
};

export type Verdict = "accept" | "reject" | "ask_again";

export type Vault = {
  entries: { id: string; siglum: string; name: string; correspondence: string }[];
  bridges: {
    id: string;
    left: string;
    right: string;
    kind: string;
    correspondence: string;
    derived: boolean;
  }[];
};

export type AuditTarget = "entry" | "bridge";

export type AuditAction = "attest" | "withdraw";

/** What `sign_in_poll` answers while a sign-in is pending or once it ends. */
export type SignInState = "signed" | "waiting" | "nothing" | "refused" | "ran_out";

/** What the instance said changed (`stemma_core::event::Event`). */
export type ServerEvent =
  | { kind: "line"; group: string; side: string; line: string; change: "started" | "written" | "shared" | "deleted"; revision?: number }
  | { kind: "claim"; group: string; side: string; line: string; change: "taken" | "released" | "asked" }
  | { kind: "group"; group: string; change: "created" | "joined" }
  | { kind: "job"; group: string; job: string; state: string }
  | { kind: "proposal"; group: string; proposal: string; state: string }
  | { kind: "library"; group: string; side: string; commit: string; change: string }
  | { kind: "check"; group: string; side: string; line: string; revision: number; block: string; state: string; diagnostic?: string }
  | { kind: "formal_audit"; group: string; line: string; block: string; state: string }
  | { kind: "machine"; machine: string }
  | { kind: "reset" };
