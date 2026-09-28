// Typed wrappers over the desktop shell's commands.

import type {
  AuditAction, AuditTarget, Beginning, Claim, Conflict, Decision, Known, LineDocument, Machine,
  Offer, OpenLine, Preview, Review, Signed, Stepped, Vault, Verdict, Waiting,
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

/** Hears a view chosen from the application's menu. */
export function onViewChosen(handler: (view: string) => void): Promise<() => void> {
  return window.__TAURI__?.event?.listen<string>("view", (event) => handler(event.payload)) ?? Promise.resolve(() => {});
}

/** An error as a sentence, without the `Error:` prefix. */
export function said(error: unknown): string {
  return String(error).replace(/^Error:\s*/, "");
}

export const api = {
  whoami: () => call<Signed | null>("whoami"),
  signInStart: (server: string) => call<Beginning>("sign_in_start", { server }),
  signInPoll: () => call<string>("sign_in_poll"),
  signOut: () => call<string>("sign_out"),
  openLink: (url: string) => call<void>("open_link", { url }),

  sync: () => call<Stepped[]>("sync"),
  groups: () => call<string[]>("groups"),
  managedGroups: () => call<string[]>("managed_groups"),
  lines: () => call<Known[]>("lines"),

  newLine: (group: string, title: string, description: string | null, shared: boolean) =>
    call<string>("new_line", { group, title, description, shared }),
  deleteLine: (path: string) => call<string>("delete_line", { path }),
  shareLine: (path: string, history: boolean, mineToWrite: boolean) =>
    call<string>("share_line", { path, history, mineToWrite }),

  openLine: (path: string) => call<OpenLine>("open_line", { path }),
  saveLine: (path: string, document: LineDocument) => call<OpenLine>("save_line", { path, document }),
  recordLine: (path: string) => call<string | null>("record_line", { path }),
  preview: (path: string, document: LineDocument) => call<Preview>("preview", { path, document }),

  claim: (path: string) => call<Claim>("claim", { path }),
  claimStatus: (path: string) => call<Claim>("claim_status", { path }),
  release: (path: string) => call<Claim>("release", { path }),
  requestClaim: (path: string) => call<Claim>("request_claim", { path }),
  parted: (path: string) => call<Conflict[]>("parted", { path }),
  settle: (path: string, decisions: Decision[]) => call<string[]>("settle", { path, decisions }),

  offers: (group: string) => call<Offer[]>("offers", { group }),
  launch: (path: string, task: string, agent: string, machine: string) =>
    call<string>("launch", { path, task, agent, here: false, machine }),
  machine: () => call<Machine>("machine"),
  jobOutput: (job: string) => call<string>("job_output", { job }),
  nodeInit: (groups: string[]) => call<string>("node_init", { groups }),
  daemonInstall: () => call<string>("daemon_install"),
  installCli: () => call<string>("install_cli"),

  proposals: () => call<Waiting[]>("proposals"),
  proposal: (proposal: string) => call<Review>("proposal", { proposal }),
  decideProposal: (proposal: string, decision: Verdict) =>
    call<string>("decide_proposal", { proposal, decision }),
  applyProposal: (proposal: string) => call<string>("apply_proposal", { proposal }),

  vault: (group: string) => call<Vault>("vault", { group }),
  vaultAdmit: (group: string, siglum: string, name: string, prose: string, lean: string) =>
    call<string>("vault_admit", { group, siglum, name, prose, lean }),
  vaultBridge: (group: string, left: string, right: string, kind: string, lean: string) =>
    call<string>("vault_bridge", { group, left, right, kind, lean }),
  vaultAudit: (group: string, targetKind: AuditTarget, target: string, action: AuditAction, note: string) =>
    call<string>("vault_audit", { group, targetKind, target, action, note }),
};
