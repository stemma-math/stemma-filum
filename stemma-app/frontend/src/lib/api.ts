// Typed wrappers over the desktop shell's commands and the events it
// sends. Nothing else in the window speaks to the shell directly.

import type {
  AuditAction, AuditTarget, Beginning, BridgeKind, Claim, Conflict, Decision, Heard, Known, LineDocument, LineProposal,
  LeanDigest, Machine, Mirrored, Offer, OpenLine, Preview, Review, Reviews, Signed, SignInState, Survey, Tried, Vault, Verdict,
} from "./types";

type Bridge = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };

declare global {
  interface Window {
    __TAURI__?: Bridge & {
      event?: {
        listen<T>(name: string, handler: (event: { payload: T }) => void): Promise<() => void>;
      };
      window?: {
        getCurrentWindow(): {
          onCloseRequested(
            handler: (event: { preventDefault(): void }) => Promise<void>,
          ): Promise<() => void>;
        };
      };
    };
  }
}

/** Calls a command. The bridge is looked up per call so that a page opened
 * outside the application reports that instead of failing at load. */
function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) return Promise.reject(new Error("this window has no application behind it"));
  return invoke<T>(command, args);
}

/** Hears one of the shell's events; resolves to how to stop hearing it.
 * Outside the application there is nothing to hear. */
function listen<T>(name: string, handler: (payload: T) => void): Promise<() => void> {
  return window.__TAURI__?.event?.listen<T>(name, (event) => handler(event.payload)) ?? Promise.resolve(() => {});
}

/** Hears a view chosen from the application's menu. */
export function onViewChosen(handler: (view: string) => void): Promise<() => void> {
  return listen<string>("view", handler);
}

/** Hears Set Up This Machine or Remove Stemma chosen from the application's
 * menu, as `setup` or `remove`. */
export function onMachineChosen(handler: (action: string) => void): Promise<() => void> {
  return listen<string>("machine", handler);
}

/** Hears that repairing this machine at launch has finished. */
export function onRepaired(handler: () => void): Promise<() => void> {
  return listen<null>("stemma:repaired", () => handler());
}

/** Hears what the instance says changed, and whether it can be heard. */
export function onHeard(handler: (heard: Heard) => void): Promise<() => void> {
  return listen<Heard>("stemma:heard", handler);
}

/** Hears where synchronisation stands after each of the engine's rounds. */
export function onMirrored(handler: (mirrored: Mirrored) => void): Promise<() => void> {
  return listen<Mirrored>("stemma:mirror", handler);
}

/** Hears that a proposal link was opened; the link waits for
 * `takeOpenReview`. */
export function onOpenReview(handler: () => void): Promise<() => void> {
  return listen<string>("stemma:open-review", () => handler());
}

/** Hears the window being closed; the handler may refuse it. */
export function onCloseRequested(
  handler: (event: { preventDefault(): void }) => Promise<void>,
): Promise<() => void> {
  return window.__TAURI__?.window?.getCurrentWindow().onCloseRequested(handler) ?? Promise.resolve(() => {});
}

/** An error the shell marked with a remedy (`remedy:<name>:<sentence>`),
 * split into the two. */
export function remedyOf(error: unknown): { remedy: string | null; message: string } {
  const text = String(error).replace(/^Error:\s*/, "");
  const marked = /^remedy:([a-z-]+):/.exec(text);
  return marked ? { remedy: marked[1], message: text.slice(marked[0].length) } : { remedy: null, message: text };
}

/** An error as a sentence, without the `Error:` prefix or a remedy mark. */
export function said(error: unknown): string {
  return remedyOf(error).message;
}

export const api = {
  whoami: () => call<Signed | null>("whoami"),
  signInStart: (server: string) => call<Beginning>("sign_in_start", { server }),
  signInPoll: () => call<SignInState>("sign_in_poll"),
  signOut: () => call<string>("sign_out"),
  openLink: (url: string) => call<void>("open_link", { url }),

  standing: () => call<Mirrored>("standing"),
  sendNow: () => call<void>("send_now"),
  groups: () => call<string[]>("groups"),
  managedGroups: () => call<string[]>("managed_groups"),
  lines: () => call<Known[]>("lines"),

  newLine: (group: string, title: string, description: string | null, shared: boolean) =>
    call<string>("new_line", { group, title, description, shared }),
  deleteLine: (path: string) => call<string>("delete_line", { path }),
  shareLine: (path: string, history: boolean, mineToWrite: boolean) =>
    call<string>("share_line", { path, history, mineToWrite }),

  openLine: (path: string) => call<OpenLine>("open_line", { path }),
  checkLineLean: (path: string) => call<LeanDigest | null>("check_line_lean", { path }),
  saveLine: (path: string, document: LineDocument) => call<OpenLine>("save_line", { path, document }),
  preview: (path: string, document: LineDocument) => call<Preview>("preview", { path, document }),
  reviewPage: (path: string, document: LineDocument) => call<string>("review_page", { path, document }),

  claim: (path: string) => call<Claim>("claim", { path }),
  renewClaim: (path: string) => call<Claim>("renew_claim", { path }),
  claimStatus: (path: string) => call<Claim>("claim_status", { path }),
  release: (path: string) => call<Claim>("release", { path }),
  requestClaim: (path: string) => call<Claim>("request_claim", { path }),
  parted: (path: string) => call<Conflict[]>("parted", { path }),
  settle: (path: string, decisions: Decision[]) => call<string[]>("settle", { path, decisions }),

  offers: (group: string) => call<Offer[]>("offers", { group }),
  /** Asks `machine` to run `task` with `agent` on the line; a custom task
   * carries its request and what it may change. */
  launch: (
    path: string,
    task: string,
    agent: string,
    machine: string,
    custom: { request: string; blocks: string[]; library: boolean } | null = null,
  ) => call<string>("launch", {
    path,
    task,
    agent,
    here: false,
    machine,
    request: custom?.request ?? null,
    grantBlocks: custom?.blocks ?? [],
    grantLibrary: custom?.library ?? false,
  }),
  machine: () => call<Machine | null>("machine"),
  jobOutput: (job: string) => call<string>("job_output", { job }),
  cancelJob: (job: string) => call<void>("cancel_job", { job }),
  clearJob: (job: string) => call<void>("clear_job", { job }),
  nodeInit: (groups: string[]) => call<string>("node_init", { groups }),

  setupSurvey: () => call<Survey>("setup_survey"),
  setupService: () => call<string>("setup_service"),
  setupLink: () => call<string>("setup_link"),
  setupPath: () => call<string>("setup_path"),
  tryAgent: (agent: string) => call<Tried>("try_agent", { agent }),
  removalSend: () => call<string[]>("removal_send"),
  removalLeave: () => call<string[]>("removal_leave"),
  removalData: () => call<string[]>("removal_data"),
  removalDeleteData: () => call<void>("removal_delete_data"),
  canTrash: () => call<boolean>("can_trash"),
  trashAndQuit: () => call<void>("trash_and_quit"),
  quit: () => call<void>("quit"),

  proposals: () => call<Reviews>("proposals"),
  proposal: (proposal: string, current: string | null = null) => call<Review>("proposal", { proposal, current }),
  decideProposal: (proposal: string, decision: Verdict, note: string | null = null) =>
    call<string>("decide_proposal", { proposal, decision, note }),
  applyProposal: (proposal: string) => call<string>("apply_proposal", { proposal }),
  takeOpenReview: () => call<string | null>("take_open_review"),
  handoffProposal: (proposal: string, reviewer: string) => call<void>("handoff_proposal", { proposal, reviewer }),
  lineHistory: (path: string) => call<LineProposal[]>("line_history", { path }),

  vault: (group: string) => call<Vault>("vault", { group }),
  vaultAdmit: (group: string, siglum: string, name: string, prose: string, lean: string) =>
    call<string>("vault_admit", { group, siglum, name, prose, lean }),
  vaultBridge: (group: string, left: string, right: string, kind: BridgeKind, lean: string) =>
    call<string>("vault_bridge", { group, left, right, kind, lean }),
  vaultAudit: (group: string, targetKind: AuditTarget, target: string, action: AuditAction, note: string) =>
    call<string>("vault_audit", { group, targetKind, target, action, note }),
};
