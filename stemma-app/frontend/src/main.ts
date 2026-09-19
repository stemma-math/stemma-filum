import "@fontsource/archivo/latin-400.css";
import "@fontsource/archivo/latin-600.css";
import "@fontsource/iosevka/latin-400.css";
import "@fontsource/libertinus-serif/latin-400.css";
import "./style.css";

declare global { interface Window { __TAURI__: { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } } } }
const invoke = window.__TAURI__.core.invoke;

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
type Vault = { entries: { id: string; siglum: string; name: string; correspondence: string }[]; bridges: { id: string; left: string; right: string; kind: string; correspondence: string; derived: boolean }[] };

const app = document.querySelector<HTMLDivElement>("#app")!;
let current: OpenLine | null = null;
let saveTimer = 0;
let previewTimer = 0;
let choices: Offer[] = [];

app.innerHTML = `<header><span class="wordmark">STEMMA</span><select id="lines"><option>Choose a line…</option></select><span id="save">—</span></header>
<main><aside id="gutter"><p class="label">APPARATUS</p><div id="nodes"></div><pre id="source-diagnostics"></pre></aside>
<section class="writing"><textarea id="editor" spellcheck="false" disabled></textarea></section>
<section class="preview"><div id="page"><p class="empty">The composed document will appear here.</p></div></section>
<aside id="register"><section><p class="label">THIS MACHINE</p><div id="machine">loading…</div></section><section><p class="label">VAULT</p><div id="vault">Choose a line.</div></section><section><p class="label">JOBS</p><div id="jobs">—</div></section><section><p class="label">LAUNCH</p><div id="launcher">Choose a line.</div></section></aside></main>`;

const select = document.querySelector<HTMLSelectElement>("#lines")!;
const editor = document.querySelector<HTMLTextAreaElement>("#editor")!;
const saved = document.querySelector<HTMLSpanElement>("#save")!;

async function boot() {
  const entries = await invoke<Entry[]>("lines");
  entries.forEach((entry) => select.add(new Option(`${entry.group} · ${entry.side} · ${entry.name}`, entry.path)));
  select.onchange = () => open(select.value);
  editor.oninput = changed;
  await refreshMachine();
}

async function open(path: string) {
  try {
    current = await invoke<OpenLine>("open_line", { path });
    editor.value = current.source; editor.disabled = false; saved.textContent = "saved";
    document.querySelector("#gutter")!.classList.toggle("private", current.entry.side === "private");
    choices = await invoke<Offer[]>("offers", { group: current.entry.group });
    await drawVault(current.entry.group);
    drawNodes(current); drawLauncher(); await render();
  } catch (error) { problem(error); }
}

async function drawVault(group: string) {
  const target = document.querySelector<HTMLDivElement>("#vault")!;
  try {
    const vault = await invoke<Vault>("vault", { group });
    const names = new Map(vault.entries.map((entry) => [entry.id, entry.name]));
    const entries = vault.entries.filter((entry) => entry.correspondence !== "audit_withdrawn")
      .map((entry) => `<div class="job"><strong>${escape(entry.name)}</strong><code>${escape(entry.siglum)}</code><span>${escape(entry.correspondence)}</span></div>`).join("");
    const substitutes = vault.bridges.filter((bridge) => bridge.correspondence === "audited")
      .map((bridge) => `<div class="job"><span>${escape(names.get(bridge.left) ?? bridge.left)} ↔ ${escape(names.get(bridge.right) ?? bridge.right)}</span><code>${escape(bridge.kind)}${bridge.derived ? " · derived" : ""}</code></div>`).join("");
    target.innerHTML = entries + (substitutes ? `<p class="label">AVAILABLE SUBSTITUTIONS</p>${substitutes}` : "");
  } catch (error) { target.textContent = String(error); }
}

function changed() {
  if (!current) return;
  saved.textContent = "saving";
  window.clearTimeout(saveTimer); window.clearTimeout(previewTimer);
  saveTimer = window.setTimeout(save, 450); previewTimer = window.setTimeout(render, 650);
}

async function save() {
  if (!current) return;
  try {
    current = await invoke<OpenLine>("save_line", { path: current.entry.path, source: editor.value });
    drawNodes(current); saved.textContent = "saved";
  } catch (error) { saved.textContent = "not saved"; problem(error); }
}

async function render() {
  if (!current) return;
  const page = document.querySelector<HTMLDivElement>("#page")!;
  try {
    const preview = await invoke<{ svg: string; diagnostics: string[] }>("preview", { path: current.entry.path, source: editor.value });
    page.innerHTML = preview.svg + (preview.diagnostics.length ? `<pre class="diagnostic">${escape(preview.diagnostics.join("\n"))}</pre>` : "");
  } catch (error) { page.innerHTML = `<pre class="diagnostic">${escape(String(error))}</pre>`; }
}

function drawNodes(opened: OpenLine) {
  const verified = new Set(opened.lean?.compiled ? opened.lean.statements : []);
  const nodes = (opened.line?.nodes ?? []).map((node) => `<div class="mark ${node.centrality === "central" ? "central" : ""}"><code>${escape(node.siglum)}</code><span>${node.centrality}</span><span>${verified.has(node.siglum) ? "Lean verified" : "not formalized"}</span>${node.centrality === "central" ? `<button data-task="refute@${node.siglum}">counterexample</button><button data-task="formalize@${node.siglum}">formalize in Lean</button>` : `<button data-task="promote@${node.siglum}">promote</button>`}</div>`).join("");
  const boxes = opened.boxes.map((box) => { const boxVerified = box.interior.length > 0 && box.interior.every((statement) => verified.has(statement)); return `<div class="mark boxmark"><span>black box · ${boxVerified ? "verified" : box.state}</span><button data-task="${box.after ? `fill@${box.after}` : "fill"}">fill this box</button>${box.interior.length && !boxVerified ? `<button data-task="${box.after ? `verify@${box.after}` : "verify"}">verify in Lean</button>` : ""}</div>`; }).join("");
  document.querySelector("#nodes")!.innerHTML = nodes + boxes;
  document.querySelector("#source-diagnostics")!.textContent = [...opened.diagnostics, ...(opened.lean && !opened.lean.compiled ? [opened.lean.diagnostic] : [])].join("\n");
  document.querySelectorAll<HTMLButtonElement>("#nodes button").forEach((button) => button.onclick = () => drawLauncher(undefined, button.dataset.task));
}

async function refreshMachine() {
  try {
    const panel = await invoke<Machine>("machine");
    document.querySelector("#machine")!.innerHTML = `<strong>${escape(panel.label)}</strong><span>${panel.presence}</span>${panel.agents.map((agent) => `<span class="agent ${agent.reason ? "out" : ""}">${escape(agent.called)} <code>${escape(agent.reason ?? agent.availability)}</code></span>`).join("")}`;
    document.querySelector("#jobs")!.innerHTML = panel.jobs.length ? panel.jobs.map((job) => `<div class="job"><code>${escape(job.id)}</code><span>${escape(job.task)} · ${escape(job.agent)}</span><span>${escape(job.said)}</span>${job.raw_output ? `<button class="raw" data-job="${escape(job.id)}">raw output</button><pre class="raw-output" hidden></pre>` : ""}</div>`).join("") : "Nothing is running here.";
    document.querySelectorAll<HTMLButtonElement>("button.raw").forEach((button) => button.onclick = async () => {
      const output = button.nextElementSibling as HTMLPreElement;
      try { output.textContent = await invoke<string>("job_output", { job: button.dataset.job }); output.hidden = false; }
      catch (error) { output.textContent = String(error); output.hidden = false; }
    });
    drawLauncher(panel);
  } catch (error) { document.querySelector("#machine")!.textContent = String(error); }
}

function drawLauncher(panel?: Machine, anchoredTask?: string) {
  const target = document.querySelector<HTMLDivElement>("#launcher")!;
  if (!current) { target.textContent = "Choose a line."; return; }
  const options = choices.flatMap((machine) => machine.agents.map((agent) => ({ machine, agent })));
  target.innerHTML = `<select id="task"><option value="${escape(anchoredTask ?? "explore")}">${escape(anchoredTask ?? "Explore")}</option></select><select id="on">${options.map(({machine,agent}) => `<option value="${machine.machine}|${agent.name}" ${agent.reason ? "disabled" : ""}>${escape(machine.label)} · ${machine.presence} · ${escape(agent.called)}${agent.reason ? ` · ${escape(agent.reason)}` : ""}</option>`).join("")}</select><button id="ask">Ask</button>`;
  document.querySelector<HTMLButtonElement>("#ask")!.onclick = async () => {
    try {
      await save();
      const task = document.querySelector<HTMLSelectElement>("#task")!.value;
      const [machine, agent] = document.querySelector<HTMLSelectElement>("#on")!.value.split("|");
      await invoke("launch", { path: current!.entry.path, task, agent, here: false, machine });
      await refreshMachine();
    } catch (error) { problem(error); }
  };
}

function problem(error: unknown) { saved.textContent = String(error); }
function escape(text: string) { const node = document.createElement("span"); node.textContent = text; return node.innerHTML; }
boot().catch(problem);
