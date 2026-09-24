// Shapes of what the desktop shell's commands return (`stemma-app/src`).

export type Signed = { server: string; user: string };

export type Beginning = { code: string; url: string; url_complete: string; interval: number };

export type Stepped = {
  group: string;
  side: string;
  outcome: string;
  parted: boolean;
  dirty: boolean;
  recorded: string[];
};

export type Known = {
  group: string;
  side: string;
  name: string;
  title: string | null;
  question: string | null;
  path: string;
};

export type Entry = { group: string; side: string; name: string; path: string };

export type Node = {
  siglum: string;
  name: string | null;
  centrality: "central" | "ordinary";
  uses: string[];
  proved: boolean;
};

export type BlackBox = {
  after: string | null;
  before: string | null;
  interior: string[];
  state: string;
};

export type LeanDigest = {
  lean: string;
  mathlib: string;
  compiled: boolean;
  statements: string[];
  diagnostic: string;
};

export type OpenLine = {
  entry: Entry;
  source: string;
  line: { nodes: Node[] } | null;
  boxes: BlackBox[];
  diagnostics: string[];
  lean: LeanDigest | null;
};

export type Preview = { svg: string; diagnostics: string[] };

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

export type Disagreement = {
  siglum: string;
  part: string;
  mark: string;
  here: string | null;
  there: string | null;
  said: string;
};

export type Decision = { siglum: string; part: string; keep: Keep };

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
