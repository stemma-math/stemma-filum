import "@fontsource/archivo/latin-400.css";
import "@fontsource/archivo/latin-600.css";
import "@fontsource/iosevka/latin-400.css";
import "@fontsource/libertinus-serif/latin-400.css";
import "./style.css";
import { escapeHtml as escape, serially, latestRequest } from "./lifecycle";
import { draw } from "../../../assets/descent.js";

declare global {
  interface Window {
    __TAURI__?: {
      core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> };
      window?: { getCurrentWindow(): {
        onCloseRequested(handler: (event: { preventDefault(): void }) => Promise<void>): Promise<() => void>;
      } };
    };
  }
}

// Anything that escapes lands on the page. A window that fails in
// silence is a window nobody can report: it looks like one that never
// opened, and there is nowhere to read what happened.
const complain = (said: string) => { const target = document.querySelector("#app"); if (document.querySelector("#said")) tell(said); else if (target) target.textContent = said; };
window.addEventListener("error", (event) => complain(event.message));
window.addEventListener("unhandledrejection", (event) => complain(String(event.reason)));

// Read when it is called and not when this file loads, so that a window
// without the application behind it says so instead of going blank.
const invoke = <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
  const bridge = window.__TAURI__?.core?.invoke;
  if (!bridge) return Promise.reject(new Error("this window has no application behind it"));
  return bridge<T>(command, args);
};

type Signed = { server: string; user: string };
type Beginning = { code: string; url: string; url_complete: string; interval: number };
type Stepped = { group: string; side: string; outcome: string; parted: boolean; dirty: boolean; recorded: string[] };
type Known = { group: string; side: string; name: string; title: string | null; question: string | null; path: string };
type Entry = { group: string; side: string; name: string; path: string };
type Node = { siglum: string; name: string | null; centrality: "central" | "auxiliary"; uses: string[]; proved: boolean; at: number };
type Line = { nodes: Node[] };
type Box = { after: string | null; before: string | null; interior: string[]; state: string };
type Lean = { lean: string; mathlib: string; compiled: boolean; statements: string[]; diagnostic: string };
type OpenLine = { entry: Entry; source: string; line: Line | null; boxes: Box[]; diagnostics: string[]; lean: Lean | null };
type Machine = { label: string; presence: string; agents: Agent[]; jobs: Job[] };
type Agent = { name: string; called: string; availability: string; reason: string | null };
type Job = { id: string; state: string; task: string; agent: string; said: string; raw_output: boolean };
type Offer = { machine: string; label: string; presence: string; agents: Agent[] };
type Claim = { standing: "writing" | "waiting" | "open"; label: string | null; left: number | null; asked: boolean; said: string };
type Disagreement = { siglum: string; part: string; mark: string; here: string | null; there: string | null; said: string };
type Waiting = { proposal: string; group: string; line: string; task: string; agent: string; vendor: string; model: string };
type Moved = { change: "added" | "removed" | "kept"; text: string; before: number | null; after: number | null; frozen: boolean };
type Review = { proposal: string; group: string; side: string; line: string; task: string; agent: string; vendor: string; model: string; prompt_revision: string; state: string; extent: string; rows: Moved[]; source: string; lean_source: string | null; findings: string[] };
type Vault = { entries: { id: string; siglum: string; name: string; correspondence: string }[]; bridges: { id: string; left: string; right: string; kind: string; correspondence: string; derived: boolean }[] };

/// What fills the middle of the window.
///
/// One of these at a time, and a line is the ordinary one: the rest are
/// the places you pass through on the way to writing one.
type Stage = "welcome" | "editor" | "new-line" | "vault";

/// What the settings window is showing.
///
/// Settings is where the machine is set up and the work it did is judged.
/// Neither is mathematics, and neither belongs in the rail beside the
/// lines, so both live behind one door that opens over everything.
type Panel = "machine" | "reviews" | "account";

const app = document.querySelector<HTMLDivElement>("#app")!;
const RENEW = 5 * 60 * 1000;

const enqueueSave = serially();
const openRequest = latestRequest();
const previewRequest = latestRequest();
const descentRequest = latestRequest();
const signInRequest = latestRequest();
const reviewRequest = latestRequest();
const vaultRequest = latestRequest();
let renewalTimer = 0;
let signInTimer = 0;
let signed: Signed | null = null;
let stage: Stage = "welcome";
let panel: Panel | null = null;
let group: string | null = null;
let allGroups: string[] = [];
let known: Known[] = [];
let waiting: Waiting[] = [];
let reading: Review | null = null;
let stepped: Stepped[] = [];
let current: OpenLine | null = null;
let choices: Offer[] = [];
let writing = false;
let decisions = new Map<string, "here" | "there">();
let saveTimer = 0;
let savesInFlight = 0;
let previewTimer = 0;
let spans: string[][] = [];
let descent: unknown = null;
let showing: "document" | "descent" = "document";

function said(error: unknown) { return String(error).replace(/^Error:\s*/, ""); }
function tell(message: string) { const bar = document.querySelector<HTMLDivElement>("#said"); if (bar) { bar.textContent = message; bar.hidden = !message; } }

function brandMark() {
  return `<svg class="brand-mark" viewBox="0 0 64 64" fill="none" aria-hidden="true">
    <path class="brand-mark__edge brand-mark__edge--left" d="M32 10C32 24 29 31 20 36C15 39 13 43 14 48"/>
    <path class="brand-mark__edge brand-mark__edge--base" d="M14 48C24 40 31 44 39 47C44 49 48 49 51 47"/>
    <path class="brand-mark__edge brand-mark__edge--right" d="M51 47C53 31 47 17 32 10"/>
    <circle class="brand-mark__node brand-mark__node--top" cx="32" cy="10" r="6"/>
    <circle class="brand-mark__node brand-mark__node--left" cx="14" cy="48" r="6"/>
    <circle class="brand-mark__node brand-mark__node--right" cx="51" cy="47" r="6"/>
  </svg>`;
}

function brand() { return `<span class="brand">${brandMark()}<span>Stemma</span></span>`; }

// Whatever was being read may come back after the view that asked for
// it was thrown away: the shell is redrawn whole, and everything in it
// goes. Nothing is left to write on then, and that is not a failure —
// it is somebody who moved on — so the answer is dropped in silence.
function say(selector: string, message: string) { const target = document.querySelector(selector); if (target) target.textContent = message; }

/* ── The shell ─────────────────────────────────────────────────────── */

/// The window: a bar across the top, the rail of lines, and the stage.
///
/// Drawn once per sign-in rather than per view. What changes as somebody
/// works is the rail's contents and the stage, and redrawing the frame
/// around them would throw away the scroll position of a list they are
/// reading.
function shell() {
  const initial = shortName(signed?.user).charAt(0).toUpperCase() || "S";
  app.innerHTML = `<div class="app-shell">
    <header class="topbar">
      ${brand()}
      <span class="topbar__where" id="where"></span>
      <span class="spacer"></span>
      <button id="sync" class="ghost" title="Bring this machine into step with the instance"><span class="status-dot"></span>Sync</button>
      <button id="settings" class="ghost">Settings<span id="settings-count"></span></button>
      <button id="account" class="account"><span class="avatar" aria-hidden="true">${escape(initial)}</span><span class="identity">${signed ? escape(signed.user) : ""}<small>${signed ? escape(signed.server) : ""}</small></span></button>
    </header>
    <div class="app-main">
      <aside class="rail">
        <button id="switch-group" class="rail__group" aria-haspopup="menu" aria-expanded="false">
          <span class="rail__group-name" id="group-name"></span>
          <span class="rail__chevron" aria-hidden="true"></span>
        </button>
        <div id="group-menu" class="menu" role="menu" hidden></div>
        <div class="rail__body" id="rail-lines"></div>
        <div class="rail__foot">
          <button id="go-vault" class="rail__entry"><span class="rail__entry-mark" aria-hidden="true">§</span>Definitions</button>
        </div>
      </aside>
      <section class="stage" id="stage"></section>
    </div>
    <div id="said" hidden></div>
    <div id="scrim" class="scrim" hidden></div>
  </div>`;

  document.querySelector<HTMLButtonElement>("#sync")!.onclick = bringIntoStep;
  document.querySelector<HTMLButtonElement>("#settings")!.onclick = () => openPanel("machine");
  document.querySelector<HTMLButtonElement>("#account")!.onclick = () => openPanel("account");
  document.querySelector<HTMLButtonElement>("#switch-group")!.onclick = toggleGroupMenu;
  document.querySelector<HTMLButtonElement>("#go-vault")!.onclick = () => goTo("vault");
  document.querySelector<HTMLDivElement>("#scrim")!.onclick = (event) => {
    if (event.target === event.currentTarget) closePanel();
  };
  countWaiting();
}

function stageOf() { return document.querySelector<HTMLDivElement>("#stage")!; }

/// A person's name out of their identifier.
///
/// `usr:mario` is what the instance calls them and `mario` is what they
/// are called, which is what belongs on an avatar: an identifier's first
/// letter is the first letter of the word `usr`, the same for everybody.
function shortName(id: string | undefined) {
  return (id ?? "").trim().split(":").pop() ?? "";
}

/// What a line is called.
///
/// The opened line carries what it is, and the listing carries what it
/// is called: a heading somebody wrote. Where there is no heading yet,
/// the directory name is the honest answer.
function titleOf(entry: Entry) {
  return known.find((line) => line.path === entry.path)?.title ?? entry.name;
}

/// What the bar says you are looking at.
function nameTheView() {
  const where = document.querySelector<HTMLSpanElement>("#where");
  if (!where) return;
  const words = stage === "editor" ? (current ? titleOf(current.entry) : "Editor")
    : stage === "vault" ? "Definitions"
    : stage === "new-line" ? "New line"
    : signed?.server ?? "";
  where.textContent = words;
}

/* ── The rail ──────────────────────────────────────────────────────── */

/// Every group this machine has, and the one being worked in.
///
/// The rail is about one group at a time. Somebody who belongs to three
/// is writing in one of them, and showing all three at once would ask
/// them to read past two before finding the line they came for.
async function loadGroups() {
  try { allGroups = await invoke<string[]>("groups"); } catch { allGroups = []; }
  if (group && !allGroups.includes(group)) group = null;
  if (!group) group = allGroups[0] ?? null;
}

async function loadLines() {
  try { known = await invoke<Known[]>("lines"); } catch (error) { tell(said(error)); known = []; }
}

/// The rail's contents: the group, then its lines by who can see them.
function drawRail() {
  const name = document.querySelector<HTMLSpanElement>("#group-name");
  if (name) name.textContent = group ?? "No group";
  const target = document.querySelector<HTMLDivElement>("#rail-lines");
  if (!target) return;

  if (!group) {
    target.innerHTML = `<p class="rail__empty">No groups are on this machine yet. Sync to bring them here.</p>`;
    return;
  }

  const mine = known.filter((line) => line.group === group && line.side === "private");
  const ours = known.filter((line) => line.group === group && line.side !== "private");
  const here = current?.entry.path;

  const list = (lines: Known[]) => lines.map((line) => `<button class="line${line.path === here ? " line--here" : ""}" data-open="${escape(line.path)}">
    <span class="line__name">${escape(line.title ?? line.name)}</span>
    ${line.question ? `<span class="line__question">${escape(line.question)}</span>` : ""}
  </button>`).join("");

  target.innerHTML = `<button id="start-line" class="rail__entry rail__entry--make"><span class="rail__entry-mark" aria-hidden="true">+</span>New line</button>
    <p class="rail__label">Yours alone</p>
    ${mine.length ? list(mine) : `<p class="rail__empty">Nothing of your own here yet.</p>`}
    <p class="rail__label">Shared with the group</p>
    ${ours.length ? list(ours) : `<p class="rail__empty">Nothing has been shared yet.</p>`}`;

  target.querySelectorAll<HTMLButtonElement>("[data-open]").forEach((button) => {
    button.onclick = () => open(button.dataset.open!);
  });
  document.querySelector<HTMLButtonElement>("#start-line")!.onclick = () => goTo("new-line");
}

/// Marks the open line without redrawing the rail under the pointer.
function markOpenLine() {
  const here = current?.entry.path;
  document.querySelectorAll<HTMLButtonElement>("#rail-lines .line").forEach((button) => {
    button.classList.toggle("line--here", button.dataset.open === here);
  });
}

function toggleGroupMenu() {
  const menu = document.querySelector<HTMLDivElement>("#group-menu");
  const button = document.querySelector<HTMLButtonElement>("#switch-group");
  if (!menu || !button) return;
  if (!menu.hidden) { closeGroupMenu(); return; }

  menu.innerHTML = allGroups.length
    ? allGroups.map((one) => `<button role="menuitem" data-group="${escape(one)}"${one === group ? ' class="here"' : ""}>${escape(one)}</button>`).join("")
    : `<p class="rail__empty">No groups on this machine.</p>`;
  menu.hidden = false;
  button.setAttribute("aria-expanded", "true");
  menu.querySelectorAll<HTMLButtonElement>("[data-group]").forEach((item) => {
    item.onclick = () => { closeGroupMenu(); void chooseGroup(item.dataset.group!); };
  });
  window.setTimeout(() => document.addEventListener("pointerdown", awayFromMenu), 0);
}

function closeGroupMenu() {
  const menu = document.querySelector<HTMLDivElement>("#group-menu");
  if (menu) menu.hidden = true;
  document.querySelector<HTMLButtonElement>("#switch-group")?.setAttribute("aria-expanded", "false");
  document.removeEventListener("pointerdown", awayFromMenu);
}

function awayFromMenu(event: PointerEvent) {
  const menu = document.querySelector<HTMLDivElement>("#group-menu");
  const button = document.querySelector<HTMLButtonElement>("#switch-group");
  const at = event.target as HTMLElement;
  if (menu?.contains(at) || button?.contains(at)) return;
  closeGroupMenu();
}

/// Moves to another group, leaving the line you were writing behind.
///
/// The open line belongs to the group it is in, so changing group puts
/// it away — but only after what is in the editor is on the disk and in
/// the history, the same as any other way of leaving a line.
async function chooseGroup(next: string) {
  if (next === group) return;
  if (stage === "editor") { if (!await leaveEditor()) return; }
  group = next;
  current = null;
  drawRail();
  await goTo("welcome");
}

/* ── Moving about ──────────────────────────────────────────────────── */

/// Puts the open line down: on the disk, then in the history.
///
/// Returns whether it worked. A line whose last edits could not be
/// saved is not left behind quietly.
async function leaveEditor(): Promise<boolean> {
  try { await flushEdits(); } catch (error) { tell(said(error)); return false; }
  window.clearTimeout(saveTimer);
  window.clearTimeout(previewTimer);
  await record();
  writing = false;
  return true;
}

async function goTo(next: Stage) {
  if (stage === "editor" && next !== "editor") {
    if (!await leaveEditor()) return;
    current = null;
  }
  openRequest();
  vaultRequest();
  stage = next;
  nameTheView();
  markOpenLine();
  if (next === "welcome") drawWelcome();
  if (next === "editor") drawEditor();
  if (next === "new-line") await drawNewLine();
  if (next === "vault") await drawVault();
}

/// The stage before a line is open.
function drawWelcome() {
  stageOf().innerHTML = `<div class="welcome">
    <div class="welcome__mark">${brandMark()}</div>
    <p class="welcome__group">${group ? escape(group) : "No group"}</p>
    <p class="quiet">${group ? "Choose a line on the left, or start one." : "Sync this machine to bring your groups here."}</p>
    ${group ? `<button id="welcome-start" class="primary">Start a line</button>` : ""}
  </div>`;
  const start = document.querySelector<HTMLButtonElement>("#welcome-start");
  if (start) start.onclick = () => goTo("new-line");
}

/* ── Signing in ────────────────────────────────────────────────────── */

function signInScreen(problem?: string) {
  signInRequest();
  window.clearTimeout(signInTimer);
  app.innerHTML = `<main class="entrance-surface"><div class="entrance-brand">${brandMark()}</div><div class="entrance">
    ${brand()}
    <span class="entrance-kicker">Desktop workspace</span>
    <h1>Connect this machine</h1>
    <p class="quiet">Enter the address of your Stemma instance. You will confirm access in your browser.</p>
    ${problem ? `<p class="problem">${escape(problem)}</p>` : ""}
    <form id="sign-in"><input type="text" id="server" placeholder="stemma.example.org" aria-label="Instance address" autofocus><button type="submit">Continue</button></form>
  </div></main>`;
  document.querySelector<HTMLFormElement>("#sign-in")!.onsubmit = async (event) => {
    event.preventDefault();
    const server = document.querySelector<HTMLInputElement>("#server")!.value.trim();
    if (!server) return;
    try { await beginSignIn(server); } catch (error) { signInScreen(said(error)); }
  };
}

async function beginSignIn(server: string) {
  const active = signInRequest();
  const beginning = await invoke<Beginning>("sign_in_start", { server });
  if (!active()) return;
  app.innerHTML = `<main class="entrance-surface"><div class="entrance-brand">${brandMark()}</div><div class="entrance">
    ${brand()}
    <span class="entrance-kicker">Secure connection</span>
    <h1>Type this code</h1>
    <p class="code">${escape(beginning.code)}</p>
    <p class="quiet">at ${escape(beginning.url)}</p>
    <button id="open">Open it in the browser</button>
    <p class="quiet" id="state">Waiting for approval in your browser...</p>
    <button id="give-up" class="quiet-button">Use a different instance</button>
  </div></main>`;
  document.querySelector<HTMLButtonElement>("#open")!.onclick = () => invoke("open_link", { url: beginning.url_complete }).catch(() => {});
  document.querySelector<HTMLButtonElement>("#give-up")!.onclick = () => signInScreen();
  const poll = async () => {
    if (!active()) return;
    let answer: string;
    try { answer = await invoke<string>("sign_in_poll"); } catch (error) { answer = said(error); }
    if (!active()) return;
    if (answer === "signed") { await boot(); return; }
    if (answer === "waiting" || answer === "nothing") {
      signInTimer = window.setTimeout(poll, Math.max(2, beginning.interval) * 1000);
      return;
    }
    signInScreen(answer === "refused" ? "That sign-in was refused." : answer === "ran_out" ? "Nobody answered in time." : answer);
  };
  signInTimer = window.setTimeout(poll, Math.max(2, beginning.interval) * 1000);
}

/* ── Starting a line ───────────────────────────────────────────────── */

async function drawNewLine() {
  const target = stageOf();
  if (!group) { target.innerHTML = `<div class="pane"><p class="quiet">No groups are available on this machine.</p></div>`; return; }
  target.innerHTML = `<div class="pane pane--narrow">
    <h1>Start a line</h1>
    <p class="quiet">A line is one question and the work of answering it. It begins on this machine.</p>
    <form id="start" class="stack">
      <label>Title<input type="text" id="title" required autofocus></label>
      <label>The question<textarea id="question" rows="3" required></textarea>
        <span class="quiet">Describe the research question this line will address.</span></label>
      <fieldset><legend>Who can see it</legend>
        <label class="choice"><input type="radio" name="seen" value="me" checked><span>Only me</span></label>
        <label class="choice"><input type="radio" name="seen" value="group"><span>Everybody in ${escape(group)}</span></label>
      </fieldset>
      <div class="row__actions">
        <button type="submit" class="primary">Start it</button>
        <button type="button" id="not-now" class="quiet-button">Not now</button>
      </div>
    </form>
  </div>`;
  document.querySelector<HTMLButtonElement>("#not-now")!.onclick = () => goTo("welcome");
  document.querySelector<HTMLFormElement>("#start")!.onsubmit = async (event) => {
    event.preventDefault();
    const title = document.querySelector<HTMLInputElement>("#title")!.value;
    const question = document.querySelector<HTMLTextAreaElement>("#question")!.value;
    const shared = (document.querySelector<HTMLInputElement>('input[name="seen"]:checked')!).value === "group";
    try {
      tell(await invoke<string>("new_line", { group, title, question, shared }));
      await bringIntoStep();
      await loadLines();
      drawRail();
      await goTo("welcome");
    } catch (error) { tell(said(error)); }
  };
}

/* ── What the group sees of a line ─────────────────────────────────── */

/// Sharing is a move, and it asks its two questions where the line is.
///
/// It belongs in the editor rather than beside a name in a list: what is
/// being handed over is what is on the screen, and the decisions are
/// about that text, not about a row.
function share(path: string) {
  const host = document.querySelector<HTMLElement>("#privacy");
  if (!host || host.querySelector(".sharing")) return;
  const panel = document.createElement("form");
  panel.className = "sharing stack";
  panel.innerHTML = `<p class="quiet">Move this line to the shared group repository.</p>
    <fieldset><legend>What the group sees of what you have written</legend>
      <label class="choice"><input type="radio" name="travels" value="today" checked><span>Today's state only</span></label>
      <label class="choice"><input type="radio" name="travels" value="everything"><span>Everything from the beginning</span></label>
    </fieldset>
    <fieldset><legend>Who can write</legend>
      <label class="choice"><input type="radio" name="writing" value="author" checked><span>Only me. Everybody else proposes</span></label>
      <label class="choice"><input type="radio" name="writing" value="anyone"><span>Anybody in the group, one at a time</span></label>
    </fieldset>
    <div class="row__actions"><button type="submit">Move it to the group</button><button type="button" class="quiet-button">Not now</button></div>`;
  host.append(panel);
  panel.querySelector<HTMLButtonElement>("button.quiet-button")!.onclick = () => panel.remove();
  panel.onsubmit = async (event) => {
    event.preventDefault();
    const history = (panel.querySelector<HTMLInputElement>('input[name="travels"]:checked')!).value === "everything";
    const mine = (panel.querySelector<HTMLInputElement>('input[name="writing"]:checked')!).value === "author";
    try {
      tell(await invoke<string>("share_line", { path, history, mineToWrite: mine }));
      await bringIntoStep();
      await loadLines();
      drawRail();
      await goTo("welcome");
    } catch (error) { tell(said(error)); }
  };
}

async function takeOut(path: string) {
  if (!window.confirm("Take this line out of your repository?\n\nEvery version of it stays in the history.")) return;
  try {
    tell(await invoke<string>("delete_line", { path }));
    await bringIntoStep();
    await loadLines();
    drawRail();
    await goTo("welcome");
  } catch (error) { tell(said(error)); }
}

/* ── Synchronising ─────────────────────────────────────────────────── */

async function bringIntoStep() {
  tell("Syncing this machine...");
  try { stepped = await invoke<Stepped[]>("sync"); } catch (error) { tell(said(error)); return; }
  const parted = stepped.filter((one) => one.parted);
  const recorded = stepped.flatMap((one) => one.recorded);
  // Work still only on this machine. It is never nothing quietly: a
  // line somebody is writing is held back on purpose, and one that is
  // held back for any other reason is something they need to know.
  const behind = stepped.filter((one) => one.dirty);
  tell(stepped.map((one) => `${one.group}/${one.side}: ${one.outcome}`).join(" · ") || "Everything is up to date.");
  if (recorded.length) tell(`Written into history before syncing: ${recorded.join(", ")}.`);
  if (behind.length) tell(`${behind.map((one) => `${one.group}/${one.side}`).join(", ")}: there is work here that is not in the history yet, so it has not gone anywhere. A line you are writing is written when you let it go.`);
  if (parted.length) tell(`${parted.map((one) => `${one.group}/${one.side}`).join(", ")}: both versions changed. Open the line to resolve the conflict.`);
}

/* ── The editor ────────────────────────────────────────────────────── */

async function open(path: string) {
  const active = openRequest();
  try {
    await save();
    const opened = await invoke<OpenLine>("open_line", { path });
    const [offers, bridges] = await Promise.all([
      invoke<Offer[]>("offers", { group: opened.entry.group }).catch(() => []),
      audited(opened.entry.group),
    ]);
    if (!active()) return;
    await flushEdits();
    // The last moment the line being left is still the open one. Going
    // from one line to another does not pass through `go`, so without
    // this the line you just left would be the only one never written.
    await record();
    if (!active()) return;
    current = opened;
    writing = false;
    descent = null;
    choices = offers;
    spans = bridges;
    await goTo("editor");
    await takeClaim();
    if (current !== opened || stage !== "editor") return;
    await drawParted();
    if (current !== opened || stage !== "editor") return;
    drawNodes(opened);
    drawLauncher();
    await render();
  } catch (error) { tell(said(error)); }
}

/// The pairs of definitions an audited substitution joins.
async function audited(group: string): Promise<string[][]> {
  try {
    const vault = await invoke<Vault>("vault", { group });
    const sigla = new Map(vault.entries.map((entry) => [entry.id, entry.siglum]));
    return vault.bridges
      .filter((bridge) => bridge.correspondence === "audited")
      .map((bridge) => [sigla.get(bridge.left), sigla.get(bridge.right)])
      .filter((pair): pair is string[] => pair.every((siglum) => typeof siglum === "string"));
  } catch { return []; }
}

function drawEditor() {
  if (!current) { void goTo("welcome"); return; }
  const mine = current.entry.side === "private";
  stageOf().innerHTML = `<main class="apparatus">
    <header class="line-head">
      <div>
        <p class="line-head__name">${escape(titleOf(current.entry))}</p>
        <p class="quiet identifier">${escape(current.entry.group)} · ${escape(current.entry.name)}</p>
      </div>
      <span class="seen ${mine ? "seen--mine" : "seen--ours"}">${mine ? "Yours alone" : "Shared"}</span>
      <span class="spacer"></span>
      <span id="save" class="quiet">saved</span>
      <div id="claim" class="line-head__claim">No active claim</div>
    </header>
    <section class="writing"><textarea id="editor" spellcheck="false"></textarea></section>
    <section class="preview">
      <div class="tabs"><button data-pane="document" class="here">Document</button><button data-pane="descent">Descent</button></div>
      <div id="page"><p class="empty">The composed document will appear here.</p></div>
      <figure class="descent" hidden><canvas class="descent__canvas" role="img" aria-label="The descent of this line"></canvas></figure>
    </section>
    <aside id="register">
      <section><p class="label">Apparatus</p><div id="nodes"></div><div id="notation"></div><pre id="source-diagnostics"></pre></section>
      <section id="parted-block" hidden><p class="label">Two versions</p><div id="parted"></div></section>
      <section><p class="label">Ask a machine</p><div id="launcher"></div></section>
      <section><p class="label">Jobs</p><div id="jobs">No jobs</div></section>
      <section id="privacy"><p class="label">Who can see it</p><div id="privacy-actions"></div></section>
    </aside>
  </main>`;
  const editor = document.querySelector<HTMLTextAreaElement>("#editor")!;
  editor.disabled = !writing;
  editor.value = current.source;
  editor.oninput = changed;
  stageOf().querySelectorAll<HTMLButtonElement>(".tabs button").forEach((button) => button.onclick = () => showPane(button.dataset.pane as "document" | "descent"));
  drawPrivacy();
  showPane(showing);
  refreshJobs();
}

/// Where a line's reach is changed, beside the line itself.
function drawPrivacy() {
  const target = document.querySelector<HTMLDivElement>("#privacy-actions");
  if (!target || !current) return;
  const path = current.entry.path;
  if (current.entry.side === "private") {
    target.innerHTML = `<p class="quiet">Only you can read this line. Sharing moves it into the group's repository, and it cannot be taken back.</p>
      <div class="row__actions"><button id="do-share">Share with the group</button><button id="do-remove" class="quiet-button">Take it out</button></div>`;
    document.querySelector<HTMLButtonElement>("#do-share")!.onclick = () => share(path);
    document.querySelector<HTMLButtonElement>("#do-remove")!.onclick = () => takeOut(path);
    return;
  }
  target.innerHTML = `<p class="quiet">Everybody in ${escape(current.entry.group)} reads this line. One person writes it at a time.</p>`;
}

function editor() { return document.querySelector<HTMLTextAreaElement>("#editor")!; }

/// The composed document, or the shape of the line. Both are of what
/// is in the editor right now, not of the last thing a machine built.
function showPane(pane: "document" | "descent") {
  showing = pane;
  const page = document.querySelector<HTMLElement>("#page");
  const figure = document.querySelector<HTMLElement>(".descent");
  if (!page || !figure) return;
  page.hidden = pane !== "document";
  figure.hidden = pane !== "descent";
  document.querySelectorAll<HTMLButtonElement>(".tabs button").forEach((button) => button.classList.toggle("here", button.dataset.pane === pane));
  if (pane === "descent") drawDescent();
}

/// Draws the descent of what is in the editor.
async function drawDescent() {
  const figure = document.querySelector<HTMLElement>(".descent");
  if (!figure || !current) return;
  const active = descentRequest();
  const source = editor().value;
  const verified = current.lean?.compiled ? current.lean.statements : [];
  try {
    const next = await invoke<unknown>("descent", { source: editor().value, verified, bridges: spans });
    if (!active() || !figure.isConnected || editor().value !== source) return;
    // Handing the previous one over is what lets the drawing part a
    // box when a lemma has just been promoted out of it.
    draw(figure, next, descent);
    descent = next;
  } catch (error) { tell(said(error)); }
}
function saved(state: string) { const mark = document.querySelector("#save"); if (mark) mark.textContent = state; }

function changed() {
  if (!current || !writing) return;
  saved("saving");
  window.clearTimeout(saveTimer); window.clearTimeout(previewTimer);
  saveTimer = window.setTimeout(() => { void save().catch((error) => tell(said(error))); }, 450);
  previewTimer = window.setTimeout(() => { if (showing === "descent") drawDescent(); else render(); }, 650);
}

async function save() {
  const input = document.querySelector<HTMLTextAreaElement>("#editor");
  if (!current || !input || (input.value === current.source && savesInFlight === 0)) return;
  const path = current.entry.path;
  const source = input.value;
  window.clearTimeout(saveTimer);
  savesInFlight += 1;
  try {
    const opened = await enqueueSave(() => invoke<OpenLine>("save_line", { path, source }));
    if (!input.isConnected || current?.entry.path !== path) return;
    current = opened;
    if (input.value === source) { drawNodes(opened); saved("saved"); }
  } catch (error) {
    if (input.isConnected) saved("not saved");
    throw error;
  } finally {
    savesInFlight -= 1;
  }
}

/// Writes the open line into the clone's history.
///
/// This is where a commit belongs: the end of a stretch of writing, not
/// the end of a keystroke. Saving keeps the file safe every time
/// somebody pauses; this is what the rest of the group reads, and it
/// happens when they stop writing.
async function record() {
  if (!current) return;
  const path = current.entry.path;
  try {
    const written = await invoke<string | null>("record_line", { path });
    if (written) saved("written");
  } catch (error) { tell(`This line is saved on this machine and not in its history yet: ${said(error)}`); }
}

/** Navigation must also save edits made while an earlier write was in flight. */
async function flushEdits() {
  const input = document.querySelector<HTMLTextAreaElement>("#editor");
  while (input?.isConnected && current && (input.value !== current.source || savesInFlight > 0)) await save();
}

async function render() {
  if (!current) return;
  const page = document.querySelector<HTMLDivElement>("#page");
  if (!page) return;
  const active = previewRequest();
  const source = editor().value;
  try {
    const preview = await invoke<{ svg: string; diagnostics: string[] }>("preview", { path: current.entry.path, source: editor().value });
    if (!active() || !page.isConnected || editor().value !== source) return;
    page.innerHTML = preview.svg + (preview.diagnostics.length ? `<pre class="diagnostic">${escape(preview.diagnostics.join("\n"))}</pre>` : "");
  } catch (error) { if (!active() || !page.isConnected) return; page.innerHTML = `<pre class="diagnostic">${escape(said(error))}</pre>`; }
}

function drawNodes(opened: OpenLine) {
  const verified = new Set(opened.lean?.compiled ? opened.lean.statements : []);
  const nodes = (opened.line?.nodes ?? []).map((node) => `<div class="mark ${node.centrality === "central" ? "central" : ""}"><code>${escape(node.siglum)}</code><span>${node.centrality}</span><span>${verified.has(node.siglum) ? "Lean verified" : "not formalized"}</span>${node.centrality === "central" ? `<button data-task="refute@${node.siglum}">counterexample</button><button data-task="formalize@${node.siglum}">formalize in Lean</button>` : `<button data-task="promote@${node.siglum}">promote</button>`}</div>`).join("");
  const boxes = opened.boxes.map((box) => { const done = box.interior.length > 0 && box.interior.every((statement) => verified.has(statement)); return `<div class="mark boxmark"><span>black box · ${done ? "verified" : box.state}</span><button data-task="${box.after ? `fill@${box.after}` : "fill"}">fill this box</button>${box.interior.length && !done ? `<button data-task="${box.after ? `verify@${box.after}` : "verify"}">verify in Lean</button>` : ""}</div>`; }).join("");
  const gutter = document.querySelector("#nodes");
  if (!gutter) return;
  gutter.innerHTML = nodes + boxes;
  drawNotation(opened);
  say("#source-diagnostics", [...opened.diagnostics, ...(opened.lean && !opened.lean.compiled ? [opened.lean.diagnostic] : [])].join("\n"));
  gutter.querySelectorAll<HTMLButtonElement>("button").forEach((button) => button.onclick = () => drawLauncher(button.dataset.task));
}

/// Offers to write the import a line was started without.
///
/// Lines made before the notation was a package use environments that
/// nothing defines, so they will not compose and nothing about the
/// error says why. The fix is one line at the top, and there is no
/// reason to make somebody type it.
function drawNotation(opened: OpenLine) {
  const target = document.querySelector<HTMLDivElement>("#notation");
  if (!target) return;
  const missing = opened.diagnostics.some((one) => one.includes("does not say where its environments come from"));
  target.hidden = !missing;
  if (!missing) { target.innerHTML = ""; return; }
  target.innerHTML = `<div class="mark"><span>this line has no notation import, so it cannot compose</span><button id="add-notation"${writing ? "" : " disabled"}>${writing ? "add it" : "write this line to add it"}</button></div>`;
  const add = target.querySelector<HTMLButtonElement>("#add-notation");
  if (add) add.onclick = async () => {
    const path = opened.entry.path;
    try {
      // It reads the file, so what is in the editor has to be in it.
      await save();
      const next = await invoke<OpenLine>("add_notation", { path });
      if (current?.entry.path !== path) return;
      current = next;
      const input = document.querySelector<HTMLTextAreaElement>("#editor");
      if (input) input.value = next.source;
      drawNodes(next);
      await render();
      tell("The notation import was added to this line.");
    } catch (error) { tell(said(error)); }
  };
}

async function takeClaim() { await askAbout("claim"); }

async function askAbout(command: string) {
  if (!current) return;
  const target = document.querySelector<HTMLDivElement>("#claim");
  if (!target) return;
  try {
    if (command === "let_go") { await save(); await record(); }
    const claim = await invoke<Claim>(command, { path: current.entry.path });
    if (!target.isConnected) return;
    writing = claim.standing === "writing";
    editor().disabled = !writing;
    if (current) drawNotation(current);
    const minutes = claim.left === null ? "" : `<span class="quiet">it lapses in ${Math.max(1, Math.ceil(claim.left / 60))} minutes</span>`;
    const action = claim.standing === "open" ? `<button id="take-claim">Write this line</button>` : writing ? `<button id="let-go">Let it go</button>` : `<button id="ask-for-it"${claim.asked ? " disabled" : ""}>${claim.asked ? "Asked for" : "Ask for it"}</button>`;
    target.innerHTML = `<div class="job"><span>${escape(claim.said)}</span>${minutes}${action}</div>`;
    const take = target.querySelector<HTMLButtonElement>("#take-claim");
    if (take) take.onclick = takeClaim;
    const go = document.querySelector<HTMLButtonElement>("#let-go");
    if (go) go.onclick = () => askAbout("let_go");
    const ask = document.querySelector<HTMLButtonElement>("#ask-for-it");
    if (ask) ask.onclick = () => askAbout("ask_for_line");
  } catch (error) {
    if (!target.isConnected) return;
    writing = false;
    editor().disabled = true;
    target.textContent = said(error);
  }
}

async function drawParted() {
  if (!current) return;
  const block = document.querySelector<HTMLElement>("#parted-block");
  const target = document.querySelector<HTMLDivElement>("#parted");
  if (!block || !target) return;
  decisions = new Map();
  let found: Disagreement[] = [];
  try { found = await invoke<Disagreement[]>("parted", { path: current.entry.path }); }
  catch (error) { block.hidden = false; target.textContent = said(error); return; }
  if (!target.isConnected) return;
  block.hidden = found.length === 0;
  if (!found.length) return;
  target.innerHTML = found.map((one) => `<div class="job"><strong>${escape(one.mark)}</strong><span class="quiet">${escape(one.said)}</span>
    <pre class="version">${escape(one.here ?? "it is not here")}</pre>
    <pre class="version">${escape(one.there ?? "it is not there")}</pre>
    <span><button data-keep="here" data-mark="${escape(one.siglum)}|${escape(one.part)}">Keep this machine's</button><button data-keep="there" data-mark="${escape(one.siglum)}|${escape(one.part)}">Keep the other</button></span></div>`).join("")
    + `<button id="bring-together" disabled>Bring them together</button>`
    + `<p class="quiet">The version you do not select will remain in this machine's history.</p>`;
  target.querySelectorAll<HTMLButtonElement>("button[data-keep]").forEach((button) => button.onclick = () => {
    decisions.set(button.dataset.mark!, button.dataset.keep as "here" | "there");
    button.parentElement!.querySelectorAll("button").forEach((other) => other.classList.remove("kept"));
    button.classList.add("kept");
    document.querySelector<HTMLButtonElement>("#bring-together")!.disabled = decisions.size < found.length;
  });
  document.querySelector<HTMLButtonElement>("#bring-together")!.onclick = async () => {
    try {
      await invoke("settle", { path: current!.entry.path, decisions: [...decisions].map(([mark, keep]) => ({ siglum: mark.split("|")[0], part: mark.split("|")[1], keep })) });
      await open(current!.entry.path);
    } catch (error) { target.textContent = said(error); }
  };
}

function drawLauncher(anchored?: string) {
  const target = document.querySelector<HTMLDivElement>("#launcher");
  if (!target || !current) return;
  const options = choices.flatMap((machine) => machine.agents.map((agent) => ({ machine, agent })));
  if (!options.length) { target.innerHTML = `<p class="quiet">No machine works for this group yet.</p>`; return; }
  target.innerHTML = `<select id="task"><option value="${escape(anchored ?? "explore")}">${escape(anchored ?? "explore")}</option></select>
    <select id="on">${options.map(({ machine, agent }) => `<option value="${escape(machine.machine)}|${escape(agent.name)}"${agent.reason ? " disabled" : ""}>${escape(machine.label)} · ${escape(machine.presence)} · ${escape(agent.called)}${agent.reason ? ` · ${escape(agent.reason)}` : ""}</option>`).join("")}</select>
    <button id="ask"${options.every(({ agent }) => agent.reason) ? " disabled" : ""}>Ask</button>`;
  document.querySelector<HTMLButtonElement>("#ask")!.onclick = async () => {
    try {
      const path = current!.entry.path;
      const task = document.querySelector<HTMLSelectElement>("#task")!.value;
      const [machine, agent] = document.querySelector<HTMLSelectElement>("#on")!.value.split("|");
      await save();
      tell(await invoke<string>("launch", { path, task, agent, here: false, machine }));
      await refreshJobs();
    } catch (error) { tell(said(error)); }
  };
}

async function refreshJobs() {
  const target = document.querySelector<HTMLDivElement>("#jobs");
  if (!target) return;
  try {
    const panel = await invoke<Machine>("machine");
    target.innerHTML = panel.jobs.length ? panel.jobs.map(jobRow).join("") : "Nothing is running here.";
    rawOutputs(target);
  } catch (error) { target.textContent = said(error); }
}

function jobRow(job: Job) {
  return `<div class="job"><code>${escape(job.id)}</code><span>${escape(job.task)} · ${escape(job.agent)}</span><span>${escape(job.said)}</span>${job.raw_output ? `<button class="raw" data-job="${escape(job.id)}">raw output</button><pre class="raw-output" hidden></pre>` : ""}</div>`;
}

function rawOutputs(target: HTMLElement) {
  target.querySelectorAll<HTMLButtonElement>("button.raw").forEach((button) => button.onclick = async () => {
    const output = button.nextElementSibling as HTMLPreElement;
    try { output.textContent = await invoke<string>("job_output", { job: button.dataset.job }); output.hidden = false; }
    catch (error) { output.textContent = said(error); output.hidden = false; }
  });
}

/* ── Settings ──────────────────────────────────────────────────────── */

/// Setting the machine up, and judging what it did.
///
/// A window painted over the work rather than a place in the rail: it is
/// not where writing happens, you go in to do one thing, and you come
/// back out to the line you were on. The work behind it stays exactly
/// where it was, which is the point of not navigating away.
function openPanel(which: Panel) {
  panel = which;
  const scrim = document.querySelector<HTMLDivElement>("#scrim");
  if (!scrim) return;
  scrim.hidden = false;
  scrim.innerHTML = `<div class="panel" role="dialog" aria-modal="true" aria-label="Settings">
    <header class="panel__head">
      <strong>Settings</strong>
      <button id="panel-close" class="quiet-button" aria-label="Close settings">Done</button>
    </header>
    <div class="panel__body">
      <nav class="panel__nav" aria-label="Settings">
        <button data-panel="machine">This machine</button>
        <button data-panel="reviews">Reviews<span id="panel-count"></span></button>
        <button data-panel="account">Account</button>
      </nav>
      <div class="panel__pane" id="panel-pane"></div>
    </div>
  </div>`;
  document.querySelector<HTMLButtonElement>("#panel-close")!.onclick = closePanel;
  scrim.querySelectorAll<HTMLButtonElement>("[data-panel]").forEach((button) => {
    button.onclick = () => showPanel(button.dataset.panel as Panel);
  });
  document.addEventListener("keydown", escapeCloses);
  void showPanel(which);
}

function closePanel() {
  panel = null;
  const scrim = document.querySelector<HTMLDivElement>("#scrim");
  if (scrim) { scrim.hidden = true; scrim.innerHTML = ""; }
  document.removeEventListener("keydown", escapeCloses);
}

function escapeCloses(event: KeyboardEvent) {
  if (event.key === "Escape") closePanel();
}

async function showPanel(which: Panel) {
  panel = which;
  document.querySelectorAll<HTMLButtonElement>("[data-panel]").forEach((button) => {
    button.classList.toggle("here", button.dataset.panel === which);
  });
  countWaiting();
  if (which === "machine") await drawMachine();
  if (which === "reviews") await drawReview();
  if (which === "account") drawAccount();
}

function pane() { return document.querySelector<HTMLDivElement>("#panel-pane"); }

/* ── Settings · account ────────────────────────────────────────────── */

function drawAccount() {
  const target = pane();
  if (!target) return;
  target.innerHTML = `<h2>Account</h2>
    <div class="field"><p class="label">Signed in as</p><p class="row__name">${signed ? escape(signed.user) : ""}</p></div>
    <div class="field"><p class="label">Instance</p><p class="quiet identifier">${signed ? escape(signed.server) : ""}</p></div>
    <div class="row__actions"><button id="sign-out">Sign out of this machine</button></div>
    <p class="quiet">Signing out leaves your work on this machine. It is read again when you sign back in.</p>`;
  document.querySelector<HTMLButtonElement>("#sign-out")!.onclick = async () => {
    try {
      if (stage === "editor" && !await leaveEditor()) return;
      await invoke("sign_out");
      window.clearInterval(renewalTimer);
      window.clearTimeout(saveTimer);
      window.clearTimeout(previewTimer);
      closePanel();
      current = null;
      writing = false;
      signed = null;
      await boot();
    } catch (error) { tell(said(error)); }
  };
}

/* ── Settings · reviews ────────────────────────────────────────────── */

async function drawReview() {
  const target = pane();
  if (!target) return;
  target.innerHTML = `<h2>Reviews</h2><p class="quiet">What a machine proposed, waiting for somebody to judge it.</p><div id="waiting">Loading...</div><section id="reading"></section>`;
  try { waiting = await invoke<Waiting[]>("proposals"); } catch (error) { say("#waiting", said(error)); return; }
  if (!target.isConnected) return;
  countWaiting();
  const listing = document.querySelector<HTMLDivElement>("#waiting");
  if (!listing) return;
  if (!waiting.length) { listing.innerHTML = `<p class="quiet">There are no proposals awaiting your review.</p>`; return; }
  listing.innerHTML = waiting.map((one) => `<div class="row">
    <div><p class="row__name">${escape(one.line)}</p><p class="quiet">${escape(one.task)} · ${escape(one.agent)} · ${escape(one.vendor)} ${escape(one.model)}</p></div>
    <div class="row__actions"><button data-read="${escape(one.proposal)}">Read it</button></div></div>`).join("");
  listing.querySelectorAll<HTMLButtonElement>("[data-read]").forEach((button) => button.onclick = () => read(button.dataset.read!));
}

/// Keeps the number on the settings button honest.
///
/// A count is on somebody's decision, so it sits where the decision is
/// made — and on the door to it, because a judgement waiting behind a
/// closed panel that says nothing is a judgement nobody makes.
function countWaiting() {
  const mark = waiting.length ? `<span class="count">${waiting.length}</span>` : "";
  const button = document.querySelector<HTMLSpanElement>("#settings-count");
  if (button) button.innerHTML = mark;
  const inside = document.querySelector<HTMLSpanElement>("#panel-count");
  if (inside) inside.innerHTML = mark;
}

async function read(proposal: string) {
  const active = reviewRequest();
  try { const next = await invoke<Review>("proposal", { proposal }); if (!active() || panel !== "reviews") return; reading = next; } catch (error) { tell(said(error)); return; }
  const target = document.querySelector<HTMLElement>("#reading");
  if (!target) return;
  const rows = reading.rows.map((row, at) => {
    // The frame goes around the run, not around each of its lines: it
    // is one statement the guard forbids touching, not five.
    const held = row.frozen
      ? ` diff__row--held${reading!.rows[at - 1]?.frozen ? "" : " diff__row--held-first"}${reading!.rows[at + 1]?.frozen ? "" : " diff__row--held-last"}`
      : "";
    const mark = row.change === "added" ? "+" : row.change === "removed" ? "−" : " ";
    return `<div class="diff__row diff__row--${row.change}${held}"><span class="diff__at">${row.before ?? ""}</span><span class="diff__at">${row.after ?? ""}</span><span class="diff__mark">${mark}</span><span class="diff__text">${escape(row.text)}</span></div>`;
  }).join("");
  target.innerHTML = `<p class="label">${escape(reading.line)} · ${escape(reading.task)}</p>
    <p class="quiet identifier">${escape(reading.vendor)} · ${escape(reading.model)} · prompt ${escape(reading.prompt_revision)}</p>
    <p class="label">What changed · ${escape(reading.extent)}</p>
    <div class="diff">${rows || `<pre class="diff__text">${escape(reading.source)}</pre>`}</div>
    ${reading.lean_source ? `<p class="label">Lean module</p><pre class="diff">${escape(reading.lean_source)}</pre>` : ""}
    <p class="label">What was checked</p>
    <ul class="findings">${reading.findings.map((finding) => `<li>${escape(finding)}</li>`).join("")}</ul>
    ${reading.state === "awaiting_review"
      ? `<div class="row__actions"><button data-decide="reject">Reject</button><button data-decide="ask_again">Ask again</button><button data-decide="accept" class="primary">Accept</button></div>
         <p class="quiet">Accepting records you as the reviewer and preserves the model information.</p>`
      : `<p class="quiet">This proposal was ${escape(reading.state)}.</p><button id="write-it">Write it to this machine</button>`}`;
  target.querySelectorAll<HTMLButtonElement>("[data-decide]").forEach((button) => button.onclick = async () => {
    try { tell(await invoke<string>("decide_proposal", { proposal, decision: button.dataset.decide })); }
    catch (error) { tell(said(error)); return; }
    if (button.dataset.decide === "accept") { try { tell(await invoke<string>("apply_proposal", { proposal })); } catch (error) { tell(`accepted, and not written here: ${said(error)}`); } }
    await showPanel("reviews");
  });
  const write = document.querySelector<HTMLButtonElement>("#write-it");
  if (write) write.onclick = async () => { try { tell(await invoke<string>("apply_proposal", { proposal })); } catch (error) { tell(said(error)); } };
}

/* ── Settings · this machine ───────────────────────────────────────── */

/// Everything that makes this Mac a machine the group can use.
///
/// Installing the command line, running the daemon and offering the
/// machine for work are three steps of one job, so they are one pane in
/// the order somebody does them, and each says what it is for.
async function drawMachine() {
  const target = pane();
  if (!target) return;
  target.innerHTML = `<h2>This machine</h2>
    <div id="panel-machine" class="field">Loading...</div>
    <section class="field">
      <p class="label">The command line</p>
      <p class="quiet">Install the bundled <code>stemma</code> command to manage this workspace from a terminal.</p>
      <div class="row__actions"><button id="install-cli">Install it</button></div>
    </section>
    <section class="field">
      <p class="label">The background service</p>
      <p class="quiet">The daemon accepts jobs while this machine is running, without the application open.</p>
      <div class="row__actions"><button id="daemon">Install the daemon</button></div>
    </section>
    <section class="field">
      <p class="label">Offer it for work</p>
      <form id="node" class="stack">
        <div id="node-groups"></div>
        <div class="row__actions"><button type="submit">Offer this machine</button></div>
      </form>
    </section>
    <section class="field"><p class="label">Jobs</p><div id="jobs">No jobs</div></section>`;

  const where = document.querySelector<HTMLDivElement>("#node-groups")!;
  where.innerHTML = allGroups.map((one) => `<label class="choice"><input type="checkbox" name="group" value="${escape(one)}" checked><span>${escape(one)}</span></label>`).join("")
    || `<p class="quiet">No groups are available on this machine.</p>`;

  document.querySelector<HTMLFormElement>("#node")!.onsubmit = async (event) => {
    event.preventDefault();
    const groups = [...document.querySelectorAll<HTMLInputElement>('input[name="group"]:checked')].map((box) => box.value);
    try { tell(await invoke<string>("node_init", { groups })); await drawMachine(); } catch (error) { tell(said(error)); }
  };
  document.querySelector<HTMLButtonElement>("#daemon")!.onclick = async () => {
    try { tell(await invoke<string>("daemon_install")); } catch (error) { tell(said(error)); }
  };
  document.querySelector<HTMLButtonElement>("#install-cli")!.onclick = async () => {
    try { tell(await invoke<string>("install_cli")); } catch (error) { tell(said(error)); }
  };

  try {
    const machine = await invoke<Machine>("machine");
    if (!target.isConnected) return;
    const about = document.querySelector<HTMLDivElement>("#panel-machine");
    const jobs = document.querySelector<HTMLDivElement>("#jobs");
    if (!about || !jobs) return;
    about.innerHTML = `<p class="row__name">${escape(machine.label)}</p><p class="quiet">${escape(machine.presence)}</p>
      ${machine.agents.map((agent) => `<div class="agent ${agent.reason ? "out" : ""}">${escape(agent.called)} <code>${escape(agent.reason ?? agent.availability)}</code></div>`).join("")}`;
    jobs.innerHTML = machine.jobs.length ? machine.jobs.map(jobRow).join("") : "No jobs have been assigned to this machine.";
    rawOutputs(jobs);
  } catch (error) { say("#panel-machine", said(error)); }
}

/* ── The vault ─────────────────────────────────────────────────────── */

async function drawVault() {
  const target = stageOf();
  if (!group) { target.innerHTML = `<div class="pane"><p class="quiet">No groups are available on this machine.</p></div>`; return; }
  target.innerHTML = `<div class="pane">
    <h1>Definitions</h1>
    <p class="quiet">What ${escape(group)} has admitted as settled, and the bridges between those statements.</p>
    <section><p class="label">Admitted</p><div id="vault-entries">Loading...</div></section>
    <section><p class="label">Admit a definition</p><form id="admit" class="stack">
      <label>Siglum<input type="text" id="admit-siglum" placeholder="def:group" required></label>
      <label>Distinguishing name<input type="text" id="admit-name" required></label>
      <label>The prose admitted<textarea id="admit-prose" rows="3" required></textarea></label>
      <label>The complete Lean module<textarea id="admit-lean" rows="5" required></textarea></label>
      <button type="submit">Admit it</button>
      <p class="quiet">The module is compiled locally against the group's pinned version before submission.</p></form></section>
    <section><p class="label">Admit a bridge</p><form id="bridge" class="stack">
      <label>From<input type="text" id="bridge-left" required></label>
      <label>To<input type="text" id="bridge-right" required></label>
      <label>Kind<select id="bridge-kind"><option value="predicate">predicate</option><option value="structure">structure</option></select></label>
      <label>The complete Lean module<textarea id="bridge-lean" rows="5" required></textarea></label>
      <button type="submit">Admit the bridge</button>
      <p class="quiet">Bridges must be unconditional. Equivalences with hypotheses should be added as regular theorems.</p></form></section>
  </div>`;
  const chosen = () => group!;
  document.querySelector<HTMLFormElement>("#admit")!.onsubmit = async (event) => {
    event.preventDefault();
    try {
      tell(await invoke<string>("vault_admit", {
        group: chosen(),
        siglum: document.querySelector<HTMLInputElement>("#admit-siglum")!.value,
        name: document.querySelector<HTMLInputElement>("#admit-name")!.value,
        prose: document.querySelector<HTMLTextAreaElement>("#admit-prose")!.value,
        lean: document.querySelector<HTMLTextAreaElement>("#admit-lean")!.value,
      }));
      await showVault(chosen());
    } catch (error) { tell(said(error)); }
  };
  document.querySelector<HTMLFormElement>("#bridge")!.onsubmit = async (event) => {
    event.preventDefault();
    try {
      tell(await invoke<string>("vault_bridge", {
        group: chosen(),
        left: document.querySelector<HTMLInputElement>("#bridge-left")!.value,
        right: document.querySelector<HTMLInputElement>("#bridge-right")!.value,
        kind: document.querySelector<HTMLSelectElement>("#bridge-kind")!.value,
        lean: document.querySelector<HTMLTextAreaElement>("#bridge-lean")!.value,
      }));
      await showVault(chosen());
    } catch (error) { tell(said(error)); }
  };
  await showVault(chosen());
}

async function showVault(group: string) {
  const active = vaultRequest();
  const target = document.querySelector<HTMLDivElement>("#vault-entries");
  if (!target) return;
  try {
    const vault = await invoke<Vault>("vault", { group });
    if (!active() || !target.isConnected) return;
    const names = new Map(vault.entries.map((entry) => [entry.id, entry.name]));
    const entries = vault.entries.map((entry) => `<div class="row"><div><p class="row__name">${escape(entry.name)}</p><p class="quiet identifier">${escape(entry.siglum)} · ${escape(entry.correspondence)}</p></div>
      <div class="row__actions"><button data-attest="${escape(entry.id)}">Attest</button><button data-withdraw="${escape(entry.id)}">Withdraw</button></div></div>`).join("");
    const bridges = vault.bridges.map((bridge) => `<div class="row"><div><p class="row__name">${escape(names.get(bridge.left) ?? bridge.left)} ↔ ${escape(names.get(bridge.right) ?? bridge.right)}</p><p class="quiet identifier">${escape(bridge.kind)}${bridge.derived ? " · derived" : ""} · ${escape(bridge.correspondence)}</p></div>
      <div class="row__actions"><button data-attest-bridge="${escape(bridge.id)}">Attest</button><button data-withdraw-bridge="${escape(bridge.id)}">Withdraw</button></div></div>`).join("");
    target.innerHTML = (entries || `<p class="quiet">Nothing has been admitted yet.</p>`) + (bridges ? `<p class="label">Bridges</p>${bridges}` : "");
    const audit = (kind: string, id: string, action: string, from: HTMLElement) => {
      const row = from.closest(".row");
      if (!row || row.querySelector(".auditing")) return;
      const panel = document.createElement("form");
      panel.className = "auditing stack";
      panel.innerHTML = `<label>${action === "attest" ? "What did you check?" : "Why is the guarantee withdrawn?"}<textarea rows="2" required></textarea></label>
        <div class="row__actions"><button type="submit">${action === "attest" ? "Attest it" : "Withdraw it"}</button><button type="button" class="quiet-button">Not now</button></div>`;
      row.append(panel);
      panel.querySelector<HTMLButtonElement>("button.quiet-button")!.onclick = () => panel.remove();
      panel.onsubmit = async (event) => {
        event.preventDefault();
        const note = panel.querySelector<HTMLTextAreaElement>("textarea")!.value;
        try { tell(await invoke<string>("vault_audit", { group, targetKind: kind, target: id, action, note })); await showVault(group); }
        catch (error) { tell(said(error)); }
      };
    };
    target.querySelectorAll<HTMLButtonElement>("[data-attest]").forEach((button) => button.onclick = () => audit("entry", button.dataset.attest!, "attest", button));
    target.querySelectorAll<HTMLButtonElement>("[data-withdraw]").forEach((button) => button.onclick = () => audit("entry", button.dataset.withdraw!, "withdraw", button));
    target.querySelectorAll<HTMLButtonElement>("[data-attest-bridge]").forEach((button) => button.onclick = () => audit("bridge", button.dataset.attestBridge!, "attest", button));
    target.querySelectorAll<HTMLButtonElement>("[data-withdraw-bridge]").forEach((button) => button.onclick = () => audit("bridge", button.dataset.withdrawBridge!, "withdraw", button));
  } catch (error) { target.textContent = said(error); }
}

/* ── Starting up ───────────────────────────────────────────────────── */

async function boot() {
  // Painted before anything is asked for: what follows reads a file and
  // then talks to an instance, and a window showing nothing while it
  // waits looks like a window that did not open.
  app.innerHTML = `<main class="entrance-surface"><div class="entrance-brand">${brandMark()}</div><div class="entrance">${brand()}<p class="quiet">Starting your workspace...</p></div></main>`;
  try { signed = await invoke<Signed | null>("whoami"); } catch { signed = null; }
  if (!signed) { signInScreen(); return; }
  try { waiting = await invoke<Waiting[]>("proposals"); } catch { waiting = []; }
  shell();
  await loadGroups();
  await loadLines();
  drawRail();
  await goTo("welcome");
  window.clearInterval(renewalTimer);
  renewalTimer = window.setInterval(() => { if (stage === "editor" && current && writing) void takeClaim(); }, RENEW);
}

async function start() {
  let closing = false;
  await window.__TAURI__?.window?.getCurrentWindow().onCloseRequested(async (event) => {
    if (closing) { event.preventDefault(); return; }
    closing = true;
    const input = document.querySelector<HTMLTextAreaElement>("#editor");
    if (input) input.readOnly = true;
    try {
      await flushEdits();
      await record();
    } catch (error) {
      event.preventDefault();
      if (input) input.readOnly = false;
      tell(`The window remains open because your changes could not be saved: ${said(error)}`);
    } finally {
      closing = false;
    }
  });
  await boot();
}

start().catch((error) => { app.textContent = said(error); });
