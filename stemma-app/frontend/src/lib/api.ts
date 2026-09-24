// Typed wrappers over the desktop shell's commands.

import type {
  AuditAction, AuditTarget, Beginning, Claim, Decision, Disagreement, Known, Machine, Offer,
  OpenLine, Preview, Review, Signed, Stepped, Vault, Verdict, Waiting,
} from "./types";

type Bridge = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };

declare global {
  interface Window {
    __TAURI__?: Bridge & {
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
  lines: () => call<Known[]>("lines"),

  newLine: (group: string, title: string, question: string, shared: boolean) =>
    call<string>("new_line", { group, title, question, shared }),
  deleteLine: (path: string) => call<string>("delete_line", { path }),
  shareLine: (path: string, history: boolean, mineToWrite: boolean) =>
    call<string>("share_line", { path, history, mineToWrite }),

  openLine: (path: string) => call<OpenLine>("open_line", { path }),
  saveLine: (path: string, source: string) => call<OpenLine>("save_line", { path, source }),
  recordLine: (path: string) => call<string | null>("record_line", { path }),
  addNotation: (path: string) => call<OpenLine>("add_notation", { path }),
  preview: (path: string, source: string) => call<Preview>("preview", { path, source }),
  descent: (source: string, verified: string[], bridges: string[][]) =>
    call<unknown>("descent", { source, verified, bridges }),

  claim: (path: string) => call<Claim>("claim", { path }),
  letGo: (path: string) => call<Claim>("let_go", { path }),
  askForLine: (path: string) => call<Claim>("ask_for_line", { path }),
  parted: (path: string) => call<Disagreement[]>("parted", { path }),
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

/** Pairs of definition sigla joined by an audited bridge in `group`'s vault. */
export async function auditedBridges(group: string): Promise<string[][]> {
  try {
    const vault = await api.vault(group);
    const sigla = new Map(vault.entries.map((entry) => [entry.id, entry.siglum]));
    return vault.bridges
      .filter((bridge) => bridge.correspondence === "audited")
      .map((bridge) => [sigla.get(bridge.left), sigla.get(bridge.right)])
      .filter((pair): pair is string[] => pair.every((siglum) => typeof siglum === "string"));
  } catch {
    return [];
  }
}
