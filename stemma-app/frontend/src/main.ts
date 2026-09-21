import "@fontsource/archivo/latin-400.css";
import "@fontsource/archivo/latin-600.css";
import "@fontsource/iosevka/latin-400.css";
import "@fontsource/libertinus-serif/latin-400.css";
import "./style.css";
import { draw } from "../../../assets/descent.js";

declare global { interface Window { __TAURI__: { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } } } }
const invoke = window.__TAURI__.core.invoke;

type Signed = { server: string; user: string };
type Beginning = { code: string; url: string; url_complete: string; interval: number };
type Stepped = { group: string; side: string; outcome: string; parted: boolean; dirty: boolean };
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

type View = "lines" | "editor" | "review" | "machine" | "vault";

const app = document.querySelector<HTMLDivElement>("#app")!;
const RENEW = 5 * 60 * 1000;

let signed: Signed | null = null;
let view: View = "lines";
let known: Known[] = [];
let waiting: Waiting[] = [];
let reading: Review | null = null;
let stepped: Stepped[] = [];
let current: OpenLine | null = null;
let choices: Offer[] = [];
let writing = false;
let decisions = new Map<string, "here" | "there">();
let saveTimer = 0;
let previewTimer = 0;
let spans: string[][] = [];
let descent: unknown = null;
let showing: "document" | "descent" = "document";

function escape(text: string) { const node = document.createElement("span"); node.textContent = text; return node.innerHTML; }
function said(error: unknown) { return String(error).replace(/^Error:\s*/, ""); }
function tell(message: string) { const bar = document.querySelector<HTMLDivElement>("#said"); if (bar) { bar.textContent = message; bar.hidden = !message; } }

/* ── The shell ─────────────────────────────────────────────────────── */

function shell() {
  const count = waiting.length ? ` <span class="count">${waiting.length}</span>` : "";
  app.innerHTML = `<header>
    <span class="wordmark">STEMMA</span>
    <nav>
      <button data-view="lines"${view === "lines" || view === "editor" ? ' class="here"' : ""}>Lines</button>
      <button data-view="review"${view === "review" ? ' class="here"' : ""}>Review${count}</button>
      <button data-view="machine"${view === "machine" ? ' class="here"' : ""}>This machine</button>
      <button data-view="vault"${view === "vault" ? ' class="here"' : ""}>Vault</button>
    </nav>
    <span class="spacer"></span>
    <button id="sync">Sync</button>
    <span id="who" class="quiet">${signed ? escape(`${signed.user} · ${signed.server}`) : ""}</span>
    <button id="sign-out">Sign out</button>
  </header>
  <div id="said" hidden></div>
  <div id="body"></div>`;

  app.querySelectorAll<HTMLButtonElement>("nav button").forEach((button) => button.onclick = () => go(button.dataset.view as View));
  document.querySelector<HTMLButtonElement>("#sync")!.onclick = bringIntoStep;
  document.querySelector<HTMLButtonElement>("#sign-out")!.onclick = async () => { await invoke("sign_out"); signed = null; boot(); };
}

function body() { return document.querySelector<HTMLDivElement>("#body")!; }

async function go(next: View) {
  view = next;
  shell();
  if (next === "lines") await drawLines();
  if (next === "editor") drawEditor();
  if (next === "review") await drawReview();
  if (next === "machine") await drawMachine();
  if (next === "vault") await drawVault();
}

/* ── Signing in ────────────────────────────────────────────────────── */

function signInScreen(problem?: string) {
  app.innerHTML = `<div class="entrance">
    <span class="wordmark">STEMMA</span>
    <h1>Sign this machine in</h1>
    <p class="quiet">Give the address of your group's instance. You approve the machine in your own browser, on the instance's own pages.</p>
    ${problem ? `<p class="problem">${escape(problem)}</p>` : ""}
    <form id="sign-in"><input type="text" id="server" placeholder="stemma.example.org" autofocus><button type="submit">Sign in</button></form>
  </div>`;
  document.querySelector<HTMLFormElement>("#sign-in")!.onsubmit = async (event) => {
    event.preventDefault();
    const server = document.querySelector<HTMLInputElement>("#server")!.value.trim();
    if (!server) return;
    try { await beginSignIn(server); } catch (error) { signInScreen(said(error)); }
  };
}

async function beginSignIn(server: string) {
  const beginning = await invoke<Beginning>("sign_in_start", { server });
  app.innerHTML = `<div class="entrance">
    <span class="wordmark">STEMMA</span>
    <h1>Type this code</h1>
    <p class="code">${escape(beginning.code)}</p>
    <p class="quiet">at ${escape(beginning.url)}</p>
    <button id="open">Open it in the browser</button>
    <p class="quiet" id="state">Waiting for somebody to approve this machine…</p>
    <button id="give-up" class="quiet-button">Use a different instance</button>
  </div>`;
  document.querySelector<HTMLButtonElement>("#open")!.onclick = () => invoke("open_link", { url: beginning.url_complete }).catch(() => {});
  document.querySelector<HTMLButtonElement>("#give-up")!.onclick = () => signInScreen();
  const waitingFor = window.setInterval(async () => {
    let answer = "waiting";
    try { answer = await invoke<string>("sign_in_poll"); } catch (error) { answer = said(error); }
    if (answer === "signed") { window.clearInterval(waitingFor); await boot(); return; }
    if (answer === "waiting" || answer === "nothing") return;
    window.clearInterval(waitingFor);
    signInScreen(answer === "refused" ? "That sign-in was refused." : answer === "ran_out" ? "Nobody answered in time." : answer);
  }, Math.max(2, beginning.interval) * 1000);
}

/* ── Lines ─────────────────────────────────────────────────────────── */

async function drawLines() {
  const target = body();
  target.innerHTML = `<div class="sheet"><section><p class="label">Lines on this machine</p><div id="known">reading…</div></section><section id="new-line"></section></div>`;
  await drawNewLine();
  try { known = await invoke<Known[]>("lines"); } catch (error) { document.querySelector("#known")!.textContent = said(error); return; }
  const listing = document.querySelector<HTMLDivElement>("#known")!;
  if (!known.length) { listing.innerHTML = `<p class="quiet">Nothing here yet. Start a line above, or sync to bring down what the instance has.</p>`; return; }
  listing.innerHTML = known.map((line) => `<div class="row">
    <div>
      <p class="row__name">${escape(line.title ?? line.name)}</p>
      ${line.question ? `<p class="quiet">${escape(line.question)}</p>` : ""}
      <p class="quiet identifier">${escape(line.group)} · ${escape(line.side === "private" ? "yours alone" : "shared")} · ${escape(line.name)}</p>
    </div>
    <div class="row__actions">
      <button data-open="${escape(line.path)}">Open</button>
      ${line.side === "private" ? `<button data-share="${escape(line.path)}">Share</button><button data-delete="${escape(line.path)}">Take out</button>` : ""}
    </div>
  </div>`).join("");
  listing.querySelectorAll<HTMLButtonElement>("[data-open]").forEach((button) => button.onclick = () => open(button.dataset.open!));
  listing.querySelectorAll<HTMLButtonElement>("[data-share]").forEach((button) => button.onclick = () => share(button.dataset.share!));
  listing.querySelectorAll<HTMLButtonElement>("[data-delete]").forEach((button) => button.onclick = () => takeOut(button.dataset.delete!));
}

async function drawNewLine() {
  const target = document.querySelector<HTMLElement>("#new-line")!;
  let where: string[] = [];
  try { where = await invoke<string[]>("groups"); } catch { where = []; }
  target.innerHTML = `<details class="starting"><summary>New line</summary>
    ${where.length ? `<form id="start" class="stack">
      <label>Group<select id="group">${where.map((group) => `<option>${escape(group)}</option>`).join("")}</select></label>
      <label>Title<input type="text" id="title" required></label>
      <label>The question<textarea id="question" rows="2" required></textarea>
        <span class="quiet">What this line exists to answer. Without it, it is a folder.</span></label>
      <fieldset><legend>Who can see it</legend>
        <label class="choice"><input type="radio" name="seen" value="me" checked><span>Only me</span></label>
        <label class="choice"><input type="radio" name="seen" value="group"><span>The whole group</span></label>
      </fieldset>
      <button type="submit">Start the line</button>
    </form>` : `<p class="quiet">No group on this machine yet. Sync to bring down the ones you are in.</p>`}</details>`;
  const form = document.querySelector<HTMLFormElement>("#start");
  if (!form) return;
  form.onsubmit = async (event) => {
    event.preventDefault();
    const group = document.querySelector<HTMLSelectElement>("#group")!.value;
    const title = document.querySelector<HTMLInputElement>("#title")!.value;
    const question = document.querySelector<HTMLTextAreaElement>("#question")!.value;
    const shared = (document.querySelector<HTMLInputElement>('input[name="seen"]:checked')!).value === "group";
    try { tell(await invoke<string>("new_line", { group, title, question, shared })); await bringIntoStep(); await go("lines"); }
    catch (error) { tell(said(error)); }
  };
}

/// Sharing is a move, and it asks its two questions where the line is
/// rather than in a dialogue box that says nothing about them.
function share(path: string) {
    const row = document.querySelector<HTMLElement>(`[data-share="${CSS.escape(path)}"]`)?.closest(".row");
    if (!row || row.querySelector(".sharing")) return;
    const panel = document.createElement("form");
    panel.className = "sharing stack";
    panel.innerHTML = `<p class="quiet">Sharing is a move. The group receives the line; what you wrote getting there stays with you.</p>
    <fieldset><legend>What the group sees of what you have written</legend>
      <label class="choice"><input type="radio" name="travels" value="today" checked><span>Today's state only</span></label>
      <label class="choice"><input type="radio" name="travels" value="everything"><span>Everything from the beginning</span></label>
    </fieldset>
    <fieldset><legend>Who can write</legend>
      <label class="choice"><input type="radio" name="writing" value="author" checked><span>Only me. Everybody else proposes</span></label>
      <label class="choice"><input type="radio" name="writing" value="anyone"><span>Anybody in the group, one at a time</span></label>
    </fieldset>
    <div class="row__actions"><button type="submit">Move it to the group</button><button type="button" class="quiet-button">Not now</button></div>`;
    row.append(panel);
    panel.querySelector<HTMLButtonElement>("button.quiet-button")!.onclick = () => panel.remove();
    panel.onsubmit = async (event) => {
        event.preventDefault();
        const history = (panel.querySelector<HTMLInputElement>('input[name="travels"]:checked')!).value === "everything";
        const mine = (panel.querySelector<HTMLInputElement>('input[name="writing"]:checked')!).value === "author";
        try { tell(await invoke<string>("share_line", { path, history, mineToWrite: mine })); await bringIntoStep(); await go("lines"); }
        catch (error) { tell(said(error)); }
    };
}

async function takeOut(path: string) {
  if (!window.confirm("Take this line out of your repository?\n\nEvery version of it stays in the history.")) return;
  try { tell(await invoke<string>("delete_line", { path })); await bringIntoStep(); await go("lines"); }
  catch (error) { tell(said(error)); }
}

/* ── Synchronising ─────────────────────────────────────────────────── */

async function bringIntoStep() {
  tell("bringing this machine into step…");
  try { stepped = await invoke<Stepped[]>("sync"); } catch (error) { tell(said(error)); return; }
  const parted = stepped.filter((one) => one.parted);
  tell(stepped.map((one) => `${one.group}/${one.side}: ${one.outcome}`).join(" · ") || "nothing to bring down");
  if (parted.length) tell(`${parted.map((one) => `${one.group}/${one.side}`).join(", ")} — both sides moved; open the line to settle it.`);
}

/* ── The editor ────────────────────────────────────────────────────── */

async function open(path: string) {
  try {
    current = await invoke<OpenLine>("open_line", { path });
    descent = null;
    choices = await invoke<Offer[]>("offers", { group: current.entry.group }).catch(() => []);
    spans = await audited(current.entry.group);
    await go("editor");
    await takeClaim();
    await drawParted();
    drawNodes(current);
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
  if (!current) { go("lines"); return; }
  body().innerHTML = `<main class="apparatus">
    <aside id="gutter" class="${current.entry.side === "private" ? "private" : ""}">
      <p class="label">APPARATUS</p><div id="nodes"></div><pre id="source-diagnostics"></pre>
    </aside>
    <section class="writing"><textarea id="editor" spellcheck="false"></textarea></section>
    <section class="preview">
      <div class="tabs"><button data-pane="document" class="here">Document</button><button data-pane="descent">Descent</button></div>
      <div id="page"><p class="empty">The composed document will appear here.</p></div>
      <figure class="descent" hidden><canvas class="descent__canvas" role="img" aria-label="The descent of this line"></canvas></figure>
    </section>
    <aside id="register">
      <section><p class="label">LINE</p><p class="row__name">${escape(current.entry.name)}</p><p class="quiet identifier">${escape(current.entry.group)} · ${escape(current.entry.side)}</p><span id="save" class="quiet">saved</span></section>
      <section><p class="label">CLAIM</p><div id="claim">—</div></section>
      <section id="parted-block" hidden><p class="label">TWO VERSIONS</p><div id="parted"></div></section>
      <section><p class="label">LAUNCH</p><div id="launcher"></div></section>
      <section><p class="label">JOBS</p><div id="jobs">—</div></section>
    </aside>
  </main>`;
  const editor = document.querySelector<HTMLTextAreaElement>("#editor")!;
  editor.value = current.source;
  editor.oninput = changed;
  body().querySelectorAll<HTMLButtonElement>(".tabs button").forEach((button) => button.onclick = () => showPane(button.dataset.pane as "document" | "descent"));
  showPane(showing);
  refreshJobs();
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
  const verified = current.lean?.compiled ? current.lean.statements : [];
  try {
    const next = await invoke<unknown>("descent", { source: editor().value, verified, bridges: spans });
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
  saveTimer = window.setTimeout(save, 450);
  previewTimer = window.setTimeout(() => { if (showing === "descent") drawDescent(); else render(); }, 650);
}

async function save() {
  if (!current) return;
  try { current = await invoke<OpenLine>("save_line", { path: current.entry.path, source: editor().value }); drawNodes(current); saved("saved"); }
  catch (error) { saved("not saved"); tell(said(error)); }
}

async function render() {
  if (!current) return;
  const page = document.querySelector<HTMLDivElement>("#page");
  if (!page) return;
  try {
    const preview = await invoke<{ svg: string; diagnostics: string[] }>("preview", { path: current.entry.path, source: editor().value });
    page.innerHTML = preview.svg + (preview.diagnostics.length ? `<pre class="diagnostic">${escape(preview.diagnostics.join("\n"))}</pre>` : "");
  } catch (error) { page.innerHTML = `<pre class="diagnostic">${escape(said(error))}</pre>`; }
}

function drawNodes(opened: OpenLine) {
  const verified = new Set(opened.lean?.compiled ? opened.lean.statements : []);
  const nodes = (opened.line?.nodes ?? []).map((node) => `<div class="mark ${node.centrality === "central" ? "central" : ""}"><code>${escape(node.siglum)}</code><span>${node.centrality}</span><span>${verified.has(node.siglum) ? "Lean verified" : "not formalized"}</span>${node.centrality === "central" ? `<button data-task="refute@${node.siglum}">counterexample</button><button data-task="formalize@${node.siglum}">formalize in Lean</button>` : `<button data-task="promote@${node.siglum}">promote</button>`}</div>`).join("");
  const boxes = opened.boxes.map((box) => { const done = box.interior.length > 0 && box.interior.every((statement) => verified.has(statement)); return `<div class="mark boxmark"><span>black box · ${done ? "verified" : box.state}</span><button data-task="${box.after ? `fill@${box.after}` : "fill"}">fill this box</button>${box.interior.length && !done ? `<button data-task="${box.after ? `verify@${box.after}` : "verify"}">verify in Lean</button>` : ""}</div>`; }).join("");
  const gutter = document.querySelector("#nodes");
  if (!gutter) return;
  gutter.innerHTML = nodes + boxes;
  document.querySelector("#source-diagnostics")!.textContent = [...opened.diagnostics, ...(opened.lean && !opened.lean.compiled ? [opened.lean.diagnostic] : [])].join("\n");
  gutter.querySelectorAll<HTMLButtonElement>("button").forEach((button) => button.onclick = () => drawLauncher(button.dataset.task));
}

async function takeClaim() { await askAbout("claim"); }

async function askAbout(command: string) {
  if (!current) return;
  const target = document.querySelector<HTMLDivElement>("#claim");
  if (!target) return;
  try {
    const claim = await invoke<Claim>(command, { path: current.entry.path });
    writing = claim.standing !== "waiting";
    editor().disabled = !writing;
    const minutes = claim.left === null ? "" : `<span class="quiet">it lapses in ${Math.max(1, Math.ceil(claim.left / 60))} minutes</span>`;
    const action = writing ? `<button id="let-go">Let it go</button>` : `<button id="ask-for-it"${claim.asked ? " disabled" : ""}>${claim.asked ? "Asked for" : "Ask for it"}</button>`;
    target.innerHTML = `<div class="job"><span>${escape(claim.said)}</span>${minutes}${action}</div>`;
    const go = document.querySelector<HTMLButtonElement>("#let-go");
    if (go) go.onclick = () => askAbout("let_go");
    const ask = document.querySelector<HTMLButtonElement>("#ask-for-it");
    if (ask) ask.onclick = () => askAbout("ask_for_line");
  } catch (error) { target.textContent = said(error); }
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
  block.hidden = found.length === 0;
  if (!found.length) return;
  target.innerHTML = found.map((one) => `<div class="job"><strong>${escape(one.mark)}</strong><span class="quiet">${escape(one.said)}</span>
    <pre class="version">${escape(one.here ?? "it is not here")}</pre>
    <pre class="version">${escape(one.there ?? "it is not there")}</pre>
    <span><button data-keep="here" data-mark="${escape(one.siglum)}|${escape(one.part)}">Keep this machine's</button><button data-keep="there" data-mark="${escape(one.siglum)}|${escape(one.part)}">Keep the other</button></span></div>`).join("")
    + `<button id="bring-together" disabled>Bring them together</button>`
    + `<p class="quiet">Neither version is discarded: the one not kept stays in this machine's history.</p>`;
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
    <select id="on">${options.map(({ machine, agent }) => `<option value="${machine.machine}|${agent.name}"${agent.reason ? " disabled" : ""}>${escape(machine.label)} · ${machine.presence} · ${escape(agent.called)}${agent.reason ? ` · ${escape(agent.reason)}` : ""}</option>`).join("")}</select>
    <button id="ask">Ask</button>`;
  document.querySelector<HTMLButtonElement>("#ask")!.onclick = async () => {
    try {
      await save();
      const task = document.querySelector<HTMLSelectElement>("#task")!.value;
      const [machine, agent] = document.querySelector<HTMLSelectElement>("#on")!.value.split("|");
      tell(await invoke<string>("launch", { path: current!.entry.path, task, agent, here: false, machine }));
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

/* ── Review ────────────────────────────────────────────────────────── */

async function drawReview() {
  const target = body();
  target.innerHTML = `<div class="sheet"><section><p class="label">Waiting for you</p><div id="waiting">reading…</div></section><section id="reading"></section></div>`;
  try { waiting = await invoke<Waiting[]>("proposals"); } catch (error) { document.querySelector("#waiting")!.textContent = said(error); return; }
  const listing = document.querySelector<HTMLDivElement>("#waiting")!;
  if (!waiting.length) { listing.innerHTML = `<p class="quiet">Nothing is waiting for you to read it.</p>`; return; }
  listing.innerHTML = waiting.map((one) => `<div class="row">
    <div><p class="row__name">${escape(one.line)}</p><p class="quiet">${escape(one.task)} · ${escape(one.agent)} · ${escape(one.vendor)} ${escape(one.model)}</p></div>
    <div class="row__actions"><button data-read="${escape(one.proposal)}">Read it</button></div></div>`).join("");
  listing.querySelectorAll<HTMLButtonElement>("[data-read]").forEach((button) => button.onclick = () => read(button.dataset.read!));
  countWaiting();
}

/// Keeps the number beside `Review` honest without redrawing the shell.
function countWaiting() {
  const button = document.querySelector<HTMLButtonElement>('nav button[data-view="review"]');
  if (button) button.innerHTML = `Review${waiting.length ? ` <span class="count">${waiting.length}</span>` : ""}`;
}

async function read(proposal: string) {
  try { reading = await invoke<Review>("proposal", { proposal }); } catch (error) { tell(said(error)); return; }
  const target = document.querySelector<HTMLElement>("#reading")!;
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
         <p class="quiet">Accepting signs these words as yours, with the model recorded beside them.</p>`
      : `<p class="quiet">This proposal was ${escape(reading.state)}.</p><button id="write-it">Write it to this machine</button>`}`;
  target.querySelectorAll<HTMLButtonElement>("[data-decide]").forEach((button) => button.onclick = async () => {
    try { tell(await invoke<string>("decide_proposal", { proposal, decision: button.dataset.decide })); }
    catch (error) { tell(said(error)); return; }
    if (button.dataset.decide === "accept") { try { tell(await invoke<string>("apply_proposal", { proposal })); } catch (error) { tell(`accepted, and not written here: ${said(error)}`); } }
    await go("review");
  });
  const write = document.querySelector<HTMLButtonElement>("#write-it");
  if (write) write.onclick = async () => { try { tell(await invoke<string>("apply_proposal", { proposal })); } catch (error) { tell(said(error)); } };
}

/* ── This machine ──────────────────────────────────────────────────── */

async function drawMachine() {
  const target = body();
  target.innerHTML = `<div class="sheet"><section><p class="label">This machine</p><div id="panel">reading…</div></section>
    <section><p class="label">Offer it for work</p><div id="offer"></div></section>
    <section><p class="label">Jobs</p><div id="jobs">—</div></section></div>`;
  let where: string[] = [];
  try { where = await invoke<string[]>("groups"); } catch { where = []; }
  document.querySelector("#offer")!.innerHTML = `<form id="node" class="stack">
    ${where.map((group) => `<label class="choice"><input type="checkbox" name="group" value="${escape(group)}" checked><span>${escape(group)}</span></label>`).join("") || `<p class="quiet">No group on this machine yet.</p>`}
    <div class="row__actions"><button type="submit">Offer this machine</button><button type="button" id="daemon">Install the daemon</button></div>
    <p class="quiet">The daemon takes work while this machine is on. Nothing runs an agent unless you asked for it.</p></form>
    <div class="row"><div><p class="row__name">The command line</p>
      <p class="quiet">This application carries <code>stemma</code> with it. Everything here can be done from a terminal, and everything a terminal can do is here.</p></div>
      <div class="row__actions"><button id="install-cli">Install it</button></div></div>`;
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
    const panel = await invoke<Machine>("machine");
    document.querySelector("#panel")!.innerHTML = `<p class="row__name">${escape(panel.label)}</p><p class="quiet">${escape(panel.presence)}</p>
      ${panel.agents.map((agent) => `<div class="agent ${agent.reason ? "out" : ""}">${escape(agent.called)} <code>${escape(agent.reason ?? agent.availability)}</code></div>`).join("")}`;
    const jobs = document.querySelector<HTMLDivElement>("#jobs")!;
    jobs.innerHTML = panel.jobs.length ? panel.jobs.map(jobRow).join("") : "Nothing has been asked of this machine.";
    rawOutputs(jobs);
  } catch (error) { document.querySelector("#panel")!.textContent = said(error); }
}

/* ── The vault ─────────────────────────────────────────────────────── */

async function drawVault() {
  const target = body();
  let where: string[] = [];
  try { where = await invoke<string[]>("groups"); } catch { where = []; }
  if (!where.length) { target.innerHTML = `<div class="sheet"><p class="quiet">No group on this machine yet.</p></div>`; return; }
  target.innerHTML = `<div class="sheet">
    <label>Group<select id="vault-group">${where.map((group) => `<option>${escape(group)}</option>`).join("")}</select></label>
    <section><p class="label">Definitions</p><div id="vault-entries">reading…</div></section>
    <section><p class="label">Admit a definition</p><form id="admit" class="stack">
      <label>Siglum<input type="text" id="admit-siglum" placeholder="def:group" required></label>
      <label>Distinguishing name<input type="text" id="admit-name" required></label>
      <label>The prose admitted<textarea id="admit-prose" rows="3" required></textarea></label>
      <label>The complete Lean module<textarea id="admit-lean" rows="5" required></textarea></label>
      <button type="submit">Admit it</button>
      <p class="quiet">The module is compiled here, against the group's pin, before anything is sent.</p></form></section>
    <section><p class="label">Admit a bridge</p><form id="bridge" class="stack">
      <label>From<input type="text" id="bridge-left" required></label>
      <label>To<input type="text" id="bridge-right" required></label>
      <label>Kind<select id="bridge-kind"><option value="predicate">predicate</option><option value="structure">structure</option></select></label>
      <label>The complete Lean module<textarea id="bridge-lean" rows="5" required></textarea></label>
      <button type="submit">Admit the bridge</button>
      <p class="quiet">A bridge is unconditional or it is not a bridge. An equivalence under hypotheses is an ordinary theorem.</p></form></section>
  </div>`;
  const chosen = () => document.querySelector<HTMLSelectElement>("#vault-group")!.value;
  document.querySelector<HTMLSelectElement>("#vault-group")!.onchange = () => showVault(chosen());
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
  const target = document.querySelector<HTMLDivElement>("#vault-entries");
  if (!target) return;
  try {
    const vault = await invoke<Vault>("vault", { group });
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
  app.innerHTML = `<div class="entrance"><span class="wordmark">STEMMA</span><p class="quiet">Starting…</p></div>`;
  try { signed = await invoke<Signed | null>("whoami"); } catch { signed = null; }
  if (!signed) { signInScreen(); return; }
  try { waiting = await invoke<Waiting[]>("proposals"); } catch { waiting = []; }
  await go("lines");
  window.setInterval(() => { if (current && writing) invoke("claim", { path: current.entry.path }).catch(() => {}); }, RENEW);
}

boot().catch((error) => { app.textContent = said(error); });
