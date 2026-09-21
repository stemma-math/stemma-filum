//! The native shell around Stemma's existing client library.
//!
//! Commands here translate desktop requests into the same Rust operations the
//! command line uses. Repository rules, source extraction and job launching
//! remain in `stemma`; the webview owns presentation only.
//!
//! Every command is declared `async` even though none of them are: that
//! is what puts them on a thread of their own instead of the one that
//! draws the window. They talk to an instance over the network, run git
//! and compile documents, and any of those on the drawing thread is a
//! window that stops answering the mouse until it finishes.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use stemma::home::{Environment, Home};
use stemma_core::line::{Line, Name};
use stemma_core::repository;
use typst_as_lib::{TypstEngine, typst_kit_options::TypstKitFontOptions};
use typst_library::layout::PagedDocument;

#[derive(Serialize)]
struct LineEntry {
    group: String,
    side: String,
    name: String,
    path: String,
}

#[derive(Serialize)]
struct OpenLine {
    entry: LineEntry,
    source: String,
    line: Option<Line>,
    boxes: Vec<stemma_core::line::BlackBox>,
    diagnostics: Vec<String>,
    lean: Option<stemma_core::artefact::LeanDigest>,
}

#[derive(Serialize)]
struct Preview {
    svg: String,
    diagnostics: Vec<String>,
}

#[derive(Serialize)]
struct MachinePanel {
    label: String,
    presence: String,
    agents: Vec<Agent>,
    jobs: Vec<Job>,
}

#[derive(Serialize)]
struct Agent {
    name: String,
    called: String,
    availability: String,
    reason: Option<String>,
}

#[derive(Serialize)]
struct Job {
    id: String,
    state: String,
    task: String,
    agent: String,
    said: String,
    raw_output: bool,
}

/// Who this machine acts for, when it acts for anybody.
#[derive(Serialize)]
struct Signed {
    server: String,
    user: String,
}

/// A sign-in waiting for somebody to approve it.
#[derive(Serialize)]
struct Beginning {
    code: String,
    url: String,
    url_complete: String,
    interval: u32,
}

/// The sign-in this window is in the middle of, if it is in one.
#[derive(Default)]
struct SigningIn(Mutex<Option<stemma::session::Beginning>>);

/// What one repository did when the machine last brought it into step.
#[derive(Serialize)]
struct Stepped {
    group: String,
    side: String,
    outcome: String,
    parted: bool,
    dirty: bool,
}

/// One line as this machine has it on disk.
#[derive(Serialize)]
struct Known {
    group: String,
    side: String,
    name: String,
    title: Option<String>,
    question: Option<String>,
    path: String,
}

/// One proposal waiting to be read.
#[derive(Serialize)]
struct Waiting {
    proposal: String,
    group: String,
    line: String,
    task: String,
    agent: String,
    vendor: String,
    model: String,
}

/// One line of a proposal's comparison.
#[derive(Serialize)]
struct Moved {
    change: String,
    text: String,
    before: Option<usize>,
    after: Option<usize>,
    frozen: bool,
}

/// A proposal, as the review screen needs it.
#[derive(Serialize)]
struct Review {
    proposal: String,
    group: String,
    side: String,
    line: String,
    task: String,
    agent: String,
    vendor: String,
    model: String,
    prompt_revision: String,
    state: String,
    extent: String,
    rows: Vec<Moved>,
    source: String,
    lean_source: Option<String>,
    findings: Vec<String>,
}

#[derive(Serialize)]
struct Claim {
    standing: String,
    label: Option<String>,
    left: Option<u64>,
    asked: bool,
    said: String,
}

#[derive(Serialize)]
struct Disagreement {
    siglum: String,
    part: String,
    mark: String,
    here: Option<String>,
    there: Option<String>,
    said: String,
}

#[derive(serde::Deserialize)]
struct Decision {
    siglum: String,
    part: String,
    keep: String,
}

#[derive(Serialize)]
struct Offer {
    machine: String,
    label: String,
    presence: String,
    agents: Vec<Agent>,
}

fn home() -> Home {
    Home::locate(&Environment::from_process())
}

/// Runs one of the client's commands and hands back what it printed.
fn carried_out(
    work: impl FnOnce(&Home, &mut Vec<u8>) -> anyhow::Result<()>,
) -> Result<String, String> {
    let mut written = Vec::new();

    work(&home(), &mut written).map_err(|error| format!("{error:#}"))?;

    String::from_utf8(written).map_err(said)
}

/// Who this machine acts for.
#[tauri::command(async)]
fn whoami() -> Result<Option<Signed>, String> {
    let home = home();
    let credentials =
        stemma::credentials::Credentials::read(&home.credentials_file()).map_err(said)?;

    Ok(credentials.map(|credentials| Signed {
        server: credentials.server,
        user: credentials.user,
    }))
}

/// Asks an instance to let this machine in.
#[tauri::command(async)]
fn sign_in_start(
    server: String,
    signing_in: tauri::State<'_, SigningIn>,
) -> Result<Beginning, String> {
    let beginning = stemma::session::begin(server.trim()).map_err(|error| format!("{error:#}"))?;
    let answer = Beginning {
        code: beginning.sign_in.user_code.clone(),
        url: beginning.sign_in.verification_uri.clone(),
        url_complete: beginning.sign_in.verification_uri_complete.clone(),
        interval: beginning.sign_in.interval.max(1),
    };

    *signing_in.0.lock().map_err(said)? = Some(beginning);

    Ok(answer)
}

/// Asks once whether somebody has approved it.
#[tauri::command(async)]
fn sign_in_poll(signing_in: tauri::State<'_, SigningIn>) -> Result<String, String> {
    let beginning = signing_in.0.lock().map_err(said)?.clone();
    let Some(beginning) = beginning else {
        return Ok("nothing".to_owned());
    };

    match stemma::session::collect(&home(), &beginning).map_err(|error| format!("{error:#}"))? {
        stemma::session::Collected::Waiting => Ok("waiting".to_owned()),
        stemma::session::Collected::Signed(_) => {
            *signing_in.0.lock().map_err(said)? = None;

            Ok("signed".to_owned())
        }
        stemma::session::Collected::Refused => Ok("refused".to_owned()),
        stemma::session::Collected::RanOut => Ok("ran_out".to_owned()),
    }
}

/// Gives the credential back and forgets it.
#[tauri::command(async)]
fn sign_out() -> Result<String, String> {
    carried_out(stemma::session::log_out)
}

/// Brings every group this machine can work in into step.
#[tauri::command(async)]
fn sync() -> Result<Vec<Stepped>, String> {
    let mut written = Vec::new();
    let reports = stemma::sync::run(&home(), &mut written).map_err(|error| format!("{error:#}"))?;

    Ok(reports
        .into_iter()
        .map(|report| Stepped {
            group: report.group,
            side: report.side.to_string(),
            outcome: report.outcome.to_string(),
            parted: matches!(report.outcome, stemma::sync::Outcome::Parted(_, _)),
            dirty: report.dirty,
        })
        .collect())
}

/// The groups this machine has a repository for.
#[tauri::command(async)]
fn groups() -> Result<Vec<String>, String> {
    let home = home();
    let config = stemma::config::Config::read(&home.config_file()).map_err(said)?;
    let workspace = config.workspace_or(home.workspace());
    let mut found: Vec<String> = config.groups.keys().cloned().collect();

    if let Ok(entries) = std::fs::read_dir(&workspace) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();

            if entry.path().is_dir() && !found.contains(&name) {
                found.push(name);
            }
        }
    }

    found.sort();

    Ok(found)
}

/// Starts a line.
#[tauri::command(async)]
fn new_line(
    group: String,
    title: String,
    question: String,
    shared: bool,
) -> Result<String, String> {
    carried_out(|home, out| stemma::lines::start(home, &group, &title, &question, shared, out))
}

/// Takes one of this person's own lines out.
#[tauri::command(async)]
fn delete_line(path: String) -> Result<String, String> {
    let path = admitted(&path)?;
    let at = path
        .parent()
        .ok_or_else(|| "the line has no directory".to_owned())?
        .to_owned();

    carried_out(|home, out| stemma::lines::delete(home, &at, None, out))
}

/// Moves one of them to the group.
#[tauri::command(async)]
fn share_line(path: String, history: bool, mine_to_write: bool) -> Result<String, String> {
    let path = admitted(&path)?;
    let at = path
        .parent()
        .ok_or_else(|| "the line has no directory".to_owned())?
        .to_owned();

    carried_out(|home, out| stemma::lines::share(home, &at, None, history, mine_to_write, out))
}

/// Offers this machine to the instance as one that runs work.
#[tauri::command(async)]
fn node_init(groups: Vec<String>) -> Result<String, String> {
    carried_out(|home, out| stemma::node::init(home, &groups, out).map(|_| ()))
}

/// Puts the daemon under whatever supervises this machine.
#[tauri::command(async)]
fn daemon_install() -> Result<String, String> {
    carried_out(stemma::daemon::install)
}

/// Where everything in this line sits, as a stemma codicum.
///
/// Worked out from what is in the editor rather than from the last
/// thing a machine built, so the graph moves while somebody writes.
/// The instance draws the same placement with the same file.
#[tauri::command(async)]
fn descent(
    source: String,
    verified: Vec<String>,
    bridges: Vec<Vec<String>>,
) -> Result<serde_json::Value, String> {
    let Ok(line) = Line::read(&source) else {
        return Ok(serde_json::Value::Null);
    };

    let verified: Vec<stemma_core::line::Siglum> =
        verified.iter().filter_map(|at| at.parse().ok()).collect();
    let spanned: Vec<(stemma_core::line::Siglum, stemma_core::line::Siglum)> = bridges
        .iter()
        .filter_map(|pair| match pair.as_slice() {
            [left, right] => Some((left.parse().ok()?, right.parse().ok()?)),
            _ => None,
        })
        .collect();

    serde_json::to_value(stemma_core::descent::of(&line, &verified, &spanned)).map_err(said)
}

/// Puts the command line where a shell will find it.
///
/// The application carries `stemma` with it, so installing one
/// installs both. The link goes somewhere that needs no privileges:
/// asking a mathematician for an administrator password to get a
/// command is asking for the wrong thing.
#[tauri::command(async)]
fn install_cli() -> Result<String, String> {
    let carried = std::env::current_exe()
        .ok()
        .and_then(|program| program.parent().map(|beside| beside.join("stemma")))
        .filter(runnable)
        .ok_or_else(|| {
            "this build does not carry the command line: build the application with `just app`"
                .to_owned()
        })?;

    let home = std::env::var("HOME").map_err(|_| "there is no home directory".to_owned())?;
    let directory = PathBuf::from(&home).join(".local").join("bin");
    let placed = directory.join("stemma");

    std::fs::create_dir_all(&directory).map_err(said)?;

    if placed.exists() || std::fs::symlink_metadata(&placed).is_ok() {
        std::fs::remove_file(&placed).map_err(said)?;
    }

    #[cfg(unix)]
    std::os::unix::fs::symlink(&carried, &placed).map_err(said)?;

    #[cfg(not(unix))]
    std::fs::copy(&carried, &placed).map_err(said)?;

    let on_path = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .any(|entry| Path::new(entry) == directory);

    Ok(match on_path {
        true => format!("`stemma` is at {}, and on your path.", placed.display()),
        false => format!(
            "`stemma` is at {}. Add that directory to your path: \
             echo \'export PATH=\"$HOME/.local/bin:$PATH\"\' >> ~/.zshrc",
            placed.display()
        ),
    })
}

/// Whether a path is something that could be run.
#[cfg(unix)]
fn runnable(path: &PathBuf) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .is_ok_and(|about| about.is_file() && about.permissions().mode() & 0o111 != 0)
}

/// Whether a path is something that could be run.
#[cfg(not(unix))]
fn runnable(path: &PathBuf) -> bool {
    path.is_file()
}

/// Opens a link in whatever the machine uses for links.
///
/// Signing in happens in the instance's own pages, in the person's own
/// browser, where they can see the address they are typing a code into.
#[tauri::command(async)]
fn open_link(url: String) -> Result<(), String> {
    let opener = match std::env::consts::OS {
        "macos" => "open",
        "windows" => "explorer",
        _ => "xdg-open",
    };

    std::process::Command::new(opener)
        .arg(&url)
        .spawn()
        .map_err(|error| format!("cannot open {url}: {error}"))?;

    Ok(())
}

/// What is waiting for this person to read it.
#[tauri::command(async)]
fn proposals() -> Result<Vec<Waiting>, String> {
    let home = home();
    let credentials = stemma::session::required(&home).map_err(said)?;
    let server = stemma::api::Server::at(&credentials.server).map_err(said)?;

    Ok(server
        .proposals(&credentials.token)
        .map_err(|error| format!("{error:#}"))?
        .into_iter()
        .map(|waiting| Waiting {
            proposal: waiting.proposal,
            group: waiting.group,
            line: waiting.line,
            task: waiting.task,
            agent: waiting.agent,
            vendor: waiting.vendor,
            model: waiting.model,
        })
        .collect())
}

/// One proposal, as what moved.
#[tauri::command(async)]
fn proposal(proposal: String) -> Result<Review, String> {
    let home = home();
    let credentials = stemma::session::required(&home).map_err(said)?;
    let server = stemma::api::Server::at(&credentials.server).map_err(said)?;
    let offered = server
        .proposal(&credentials.token, &proposal)
        .map_err(|error| format!("{error:#}"))?;

    let stands = offered.stands.clone().unwrap_or_default();
    let rows = stemma_core::diff::between(&stands, &offered.source);
    let frozen = stemma_core::diff::frozen(&stands);
    let extent = stemma_core::diff::extent(&rows).said();

    Ok(Review {
        proposal: offered.proposal,
        group: offered.group,
        side: offered.side,
        line: offered.line,
        task: offered.task,
        agent: offered.agent,
        vendor: offered.vendor,
        model: offered.model,
        prompt_revision: offered.prompt_revision,
        state: offered.state,
        extent,
        rows: rows
            .into_iter()
            .map(|row| Moved {
                change: match row.change {
                    stemma_core::diff::Change::Added => "added".to_owned(),
                    stemma_core::diff::Change::Removed => "removed".to_owned(),
                    stemma_core::diff::Change::Kept => "kept".to_owned(),
                },
                frozen: row.before.is_some_and(|at| frozen.contains(&at)),
                text: row.text,
                before: row.before,
                after: row.after,
            })
            .collect(),
        source: offered.source,
        lean_source: offered.lean_source,
        findings: offered.findings,
    })
}

/// Records what the person decided about it.
#[tauri::command(async)]
fn decide_proposal(proposal: String, decision: String) -> Result<String, String> {
    carried_out(|home, out| stemma::proposals::decide(home, &proposal, &decision, out))
}

/// Writes an accepted proposal into this machine's clone.
#[tauri::command(async)]
fn apply_proposal(proposal: String) -> Result<String, String> {
    carried_out(|home, out| stemma::proposals::apply(home, &proposal, out))
}

/// Admits a definition to the group's vault.
#[tauri::command(async)]
fn vault_admit(
    group: String,
    siglum: String,
    name: String,
    prose: String,
    lean: String,
) -> Result<String, String> {
    carried_out(|home, out| {
        stemma::vault::admit_text(home, &group, &siglum, &name, &prose, &lean, out)
    })
}

/// Admits a proved licence to substitute one definition for another.
#[tauri::command(async)]
fn vault_bridge(
    group: String,
    left: String,
    right: String,
    kind: String,
    lean: String,
) -> Result<String, String> {
    carried_out(|home, out| {
        stemma::vault::bridge_text(home, &group, &left, &right, &kind, &lean, out)
    })
}

/// Attests a correspondence, or withdraws one.
#[tauri::command(async)]
fn vault_audit(
    group: String,
    target_kind: String,
    target: String,
    action: String,
    note: String,
) -> Result<String, String> {
    carried_out(|home, out| {
        stemma::vault::audit(home, &group, &target_kind, &target, &action, &note, out)
    })
}

#[tauri::command(async)]
fn lines() -> Result<Vec<Known>, String> {
    let home = home();
    let config = stemma::config::Config::read(&home.config_file()).map_err(said)?;
    let workspace = config.workspace_or(home.workspace());
    let groups = std::fs::read_dir(&workspace).map_err(said)?;
    let mut found = Vec::new();
    for group in groups
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
    {
        let group_name = group.file_name().to_string_lossy().into_owned();
        for (side, directory) in [("shared", "shared"), ("private", "private")] {
            let root = group.path().join(directory).join(repository::LINES);
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            for entry in entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
            {
                let name = entry.file_name().to_string_lossy().into_owned();
                if Name::parse(&name).is_err() {
                    continue;
                }
                let path = entry.path().join(repository::ENTRY);

                if path.is_file() {
                    let source = std::fs::read_to_string(&path).unwrap_or_default();

                    found.push(Known {
                        group: group_name.clone(),
                        side: side.to_owned(),
                        name,
                        title: stemma_core::source::title(&source),
                        question: stemma_core::source::question(&source),
                        path: path.display().to_string(),
                    });
                }
            }
        }
    }
    found.sort_by(|a, b| (&a.group, &a.side, &a.name).cmp(&(&b.group, &b.side, &b.name)));
    Ok(found)
}

#[tauri::command(async)]
fn open_line(path: String) -> Result<OpenLine, String> {
    let path = admitted(&path)?;
    let source = std::fs::read_to_string(&path).map_err(said)?;
    let (group, side, name) = identity(&path)?;
    let (line, boxes, diagnostics) = analyse(&source);
    Ok(OpenLine {
        entry: LineEntry {
            group,
            side,
            name,
            path: path.display().to_string(),
        },
        source,
        line,
        boxes,
        diagnostics,
        lean: lean_status(&path)?,
    })
}

#[tauri::command(async)]
fn save_line(path: String, source: String) -> Result<OpenLine, String> {
    let path = admitted(&path)?;
    std::fs::write(&path, &source).map_err(said)?;
    let (group, side, name) = identity(&path)?;
    let (line, boxes, diagnostics) = analyse(&source);
    Ok(OpenLine {
        entry: LineEntry {
            group,
            side,
            name,
            path: path.display().to_string(),
        },
        source,
        line,
        boxes,
        diagnostics,
        lean: lean_status(&path)?,
    })
}

#[tauri::command(async)]
fn preview(path: String, source: String) -> Result<Preview, String> {
    let path = admitted(&path)?;
    let root = path
        .parent()
        .ok_or_else(|| "the line has no directory".to_owned())?;
    let engine = TypstEngine::builder()
        .main_file(source)
        .with_file_system_resolver(root)
        .search_fonts_with(
            TypstKitFontOptions::default()
                .include_system_fonts(true)
                .include_embedded_fonts(true),
        )
        .build();
    let compiled = engine.compile();
    let diagnostics = compiled
        .warnings
        .iter()
        .map(|warning| format!("{:?}", warning))
        .collect();
    let document: PagedDocument = compiled.output.map_err(|errors| format!("{:?}", errors))?;
    let svg = document
        .pages
        .iter()
        .map(typst_svg::svg)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Preview { svg, diagnostics })
}

#[tauri::command(async)]
fn machine() -> Result<MachinePanel, String> {
    let home = home();
    let credentials = stemma::session::required(&home).map_err(said)?;
    let server = stemma::api::Server::at(&credentials.server).map_err(said)?;
    let node = server
        .node(&credentials.token)
        .map_err(said)?
        .ok_or_else(|| "run `stemma node init` to offer this machine".to_owned())?;
    let jobs = server
        .jobs(&credentials.token)
        .map_err(said)?
        .into_iter()
        .map(|job| {
            let raw_output = stemma::work::output_exists(&home, &job.job);
            Job {
                id: job.job,
                state: job.state,
                task: job.task,
                agent: job.agent,
                said: job.said,
                raw_output,
            }
        })
        .collect();
    let agents = node
        .agents
        .into_iter()
        .map(|agent| Agent {
            called: stemma_core::machine::known(agent.agent.as_str())
                .map(|known| known.called)
                .unwrap_or(agent.agent.as_str())
                .to_owned(),
            name: agent.agent.to_string(),
            availability: agent.availability.to_string(),
            reason: agent.availability.reason().map(str::to_owned),
        })
        .collect();
    Ok(MachinePanel {
        label: node.label,
        presence: node.presence.to_string(),
        agents,
        jobs,
    })
}

#[tauri::command(async)]
fn job_output(job: String) -> Result<String, String> {
    let mut output = Vec::new();
    stemma::jobs::output(&home(), &job, &mut output).map_err(said)?;
    String::from_utf8(output).map_err(said)
}

#[tauri::command(async)]
fn offers(group: String) -> Result<Vec<Offer>, String> {
    let home = home();
    let credentials = stemma::session::required(&home).map_err(said)?;
    let server = stemma::api::Server::at(&credentials.server).map_err(said)?;
    Ok(server
        .machines(&credentials.token, &group)
        .map_err(said)?
        .into_iter()
        .map(|machine| Offer {
            machine: machine.machine,
            label: machine.label,
            presence: machine.presence,
            agents: machine
                .agents
                .into_iter()
                .map(|agent| Agent {
                    called: stemma_core::machine::known(agent.agent.as_str())
                        .map(|known| known.called)
                        .unwrap_or(agent.agent.as_str())
                        .to_owned(),
                    name: agent.agent.to_string(),
                    availability: agent.availability.to_string(),
                    reason: agent.availability.reason().map(str::to_owned),
                })
                .collect(),
        })
        .collect())
}

/// Takes the line for this machine, which also renews a claim it holds.
#[tauri::command(async)]
fn claim(path: String) -> Result<Claim, String> {
    with_claim(&path, |server, token, about| {
        server.take_claim(token, about)
    })
}

/// Lets the line go.
#[tauri::command(async)]
fn let_go(path: String) -> Result<Claim, String> {
    with_claim(&path, |server, token, about| {
        server.release_claim(token, about)
    })
}

/// Asks whichever machine has the line to hand it back.
#[tauri::command(async)]
fn ask_for_line(path: String) -> Result<Claim, String> {
    with_claim(&path, |server, token, about| {
        server.ask_for_claim(token, about)
    })
}

/// One of the four things a machine says to the instance about a claim.
fn with_claim(
    path: &str,
    speak: impl Fn(
        &stemma::api::Server,
        &str,
        &stemma::api::About,
    ) -> anyhow::Result<stemma::api::Claimed>,
) -> Result<Claim, String> {
    let path = admitted(path)?;
    let (group, side, line) = identity(&path)?;
    let home = home();
    let credentials = stemma::session::required(&home).map_err(said)?;
    let server = stemma::api::Server::at(&credentials.server).map_err(said)?;
    let claimed = speak(
        &server,
        &credentials.token,
        &stemma::api::About { group, side, line },
    )
    .map_err(said)?;

    Ok(Claim {
        said: claimed
            .said
            .clone()
            .unwrap_or_else(|| "nobody is writing this line".to_owned()),
        standing: claimed.standing,
        label: claimed.label,
        left: claimed.left,
        asked: claimed.asked,
    })
}

/// Asks an agent to reconcile the two versions, as a proposal.
#[tauri::command(async)]
fn reconcile(path: String, agent: String) -> Result<String, String> {
    let path = admitted(&path)?;
    let at = path
        .parent()
        .ok_or_else(|| "the line has no directory".to_owned())?;
    let mut output = Vec::new();

    stemma::conflicts::reconcile(&home(), at, &agent, &mut output).map_err(said)?;

    String::from_utf8(output).map_err(said)
}

/// What the two versions of this line disagree about.
#[tauri::command(async)]
fn parted(path: String) -> Result<Vec<Disagreement>, String> {
    let path = admitted(&path)?;
    let (clone, name) = clone_and_line(&path)?;
    let _ = stemma::git::fetch(&clone);

    let found = stemma::conflicts::between(&clone).map_err(said)?;
    let Some(line) = found.into_iter().find(|line| line.line.as_str() == name) else {
        return Ok(Vec::new());
    };

    Ok(line
        .divergences
        .into_iter()
        .map(|divergence| Disagreement {
            siglum: divergence.mark.siglum.to_string(),
            part: match divergence.mark.part {
                stemma_core::reconcile::Part::Statement => "statement".to_owned(),
                stemma_core::reconcile::Part::Proof => "proof".to_owned(),
            },
            mark: divergence.mark.to_string(),
            said: divergence.said().to_owned(),
            here: divergence.here,
            there: divergence.there,
        })
        .collect())
}

/// Writes what the person decided, as one merge commit.
#[tauri::command(async)]
fn settle(path: String, decisions: Vec<Decision>) -> Result<Vec<String>, String> {
    let path = admitted(&path)?;
    let (clone, name) = clone_and_line(&path)?;
    let mut decided = std::collections::BTreeMap::new();

    for decision in decisions {
        let mark = stemma_core::reconcile::Mark {
            siglum: decision.siglum.parse().map_err(said)?,
            part: match decision.part.as_str() {
                "proof" => stemma_core::reconcile::Part::Proof,
                _ => stemma_core::reconcile::Part::Statement,
            },
        };
        let keep = match decision.keep.as_str() {
            "there" => stemma_core::reconcile::Keep::There,
            _ => stemma_core::reconcile::Keep::Here,
        };

        decided.insert(mark, keep);
    }

    let settled =
        stemma::conflicts::settle(&clone, &std::collections::BTreeMap::from([(name, decided)]))
            .map_err(said)?;

    Ok(settled.iter().map(ToString::to_string).collect())
}

/// The repository a line is in, and what the line is called.
fn clone_and_line(path: &Path) -> Result<(PathBuf, String), String> {
    let clone = path
        .ancestors()
        .nth(3)
        .ok_or_else(|| "the line has no repository".to_owned())?
        .to_owned();
    let (_, _, name) = identity(path)?;

    Ok((clone, name))
}

#[tauri::command(async)]
fn vault(group: String) -> Result<stemma::api::Vault, String> {
    let home = home();
    let credentials = stemma::session::required(&home).map_err(said)?;
    stemma::api::Server::at(&credentials.server)
        .map_err(said)?
        .vault(&credentials.token, &group)
        .map_err(said)
}

#[tauri::command(async)]
fn launch(
    path: String,
    task: String,
    agent: String,
    here: bool,
    machine: Option<String>,
) -> Result<String, String> {
    let path = admitted(&path)?;
    let at = path
        .parent()
        .ok_or_else(|| "the line has no directory".to_owned())?;
    let mut output = Vec::new();
    stemma::jobs::run(
        &home(),
        at,
        &stemma::jobs::Wanted {
            line: None,
            task,
            agent,
            machine,
            here,
        },
        &mut output,
    )
    .map_err(said)
}

fn admitted(given: &str) -> Result<PathBuf, String> {
    let path = Path::new(given).canonicalize().map_err(said)?;
    let home = home();
    let config = stemma::config::Config::read(&home.config_file()).map_err(said)?;
    let workspace = config
        .workspace_or(home.workspace())
        .canonicalize()
        .map_err(said)?;
    if !path.starts_with(workspace)
        || path.file_name().and_then(|name| name.to_str()) != Some(repository::ENTRY)
    {
        return Err("that is not a line in this machine's workspace".to_owned());
    }
    Ok(path)
}

fn identity(path: &Path) -> Result<(String, String, String), String> {
    let name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .ok_or("the line has no name")?;
    let side = path
        .ancestors()
        .nth(3)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .ok_or("the line has no repository")?;
    let group = path
        .ancestors()
        .nth(4)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .ok_or("the line has no group")?;
    Ok((group.to_owned(), side.to_owned(), name.to_owned()))
}

fn analyse(source: &str) -> (Option<Line>, Vec<stemma_core::line::BlackBox>, Vec<String>) {
    match Line::read(source) {
        Ok(line) => {
            let boxes = line.boxes();
            (Some(line), boxes, Vec::new())
        }
        Err(faults) => (
            None,
            Vec::new(),
            faults.into_iter().map(|fault| fault.to_string()).collect(),
        ),
    }
}

fn lean_status(path: &Path) -> Result<Option<stemma_core::artefact::LeanDigest>, String> {
    let clone = path
        .ancestors()
        .nth(3)
        .ok_or_else(|| "the line has no repository".to_owned())?;
    let manifest = stemma::build::read_manifest(clone).map_err(said)?;
    let pins = stemma::build::group_pins(clone, &manifest);
    let name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .ok_or_else(|| "the line has no name".to_owned())?;
    stemma::lean::check(clone, &Name::parse(name).map_err(said)?, &pins).map_err(said)
}

fn said(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Starts the desktop application.
pub fn run() {
    tauri::Builder::default()
        .manage(SigningIn::default())
        .invoke_handler(tauri::generate_handler![
            whoami,
            sign_in_start,
            sign_in_poll,
            sign_out,
            sync,
            groups,
            new_line,
            delete_line,
            share_line,
            node_init,
            daemon_install,
            descent,
            install_cli,
            open_link,
            proposals,
            proposal,
            decide_proposal,
            apply_proposal,
            vault_admit,
            vault_bridge,
            vault_audit,
            lines,
            open_line,
            save_line,
            preview,
            machine,
            job_output,
            offers,
            vault,
            launch,
            claim,
            let_go,
            ask_for_line,
            parted,
            settle,
            reconcile
        ])
        .run(tauri::generate_context!())
        .expect("the Stemma application could not start");
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{analyse, identity};

    #[test]
    fn a_line_identity_comes_from_its_repository_path() {
        let path = Path::new("/workspace/algebra/private/lines/quotients/main.typ");

        assert_eq!(
            identity(path),
            Ok((
                "algebra".to_owned(),
                "private".to_owned(),
                "quotients".to_owned()
            ))
        );
    }

    #[test]
    fn unfinished_source_remains_available_with_diagnostics() {
        let (line, boxes, diagnostics) = analyse(r#"#theorem(central: true)[Unfinished.]"#);

        assert!(line.is_none());
        assert!(boxes.is_empty());
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn valid_source_keeps_its_apparatus() {
        let (line, _, diagnostics) =
            analyse(r#"#theorem(id: "thm:main", central: true)[A statement.]"#);

        assert!(line.is_some());
        assert!(diagnostics.is_empty());
    }
}
