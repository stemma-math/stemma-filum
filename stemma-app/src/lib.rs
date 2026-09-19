//! The native shell around Stemma's existing client library.
//!
//! Commands here translate desktop requests into the same Rust operations the
//! command line uses. Repository rules, source extraction and job launching
//! remain in `stemma`; the webview owns presentation only.

use std::path::{Path, PathBuf};

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

#[tauri::command]
fn lines() -> Result<Vec<LineEntry>, String> {
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
                    found.push(LineEntry {
                        group: group_name.clone(),
                        side: side.to_owned(),
                        name,
                        path: path.display().to_string(),
                    });
                }
            }
        }
    }
    found.sort_by(|a, b| (&a.group, &a.side, &a.name).cmp(&(&b.group, &b.side, &b.name)));
    Ok(found)
}

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
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

#[tauri::command]
fn job_output(job: String) -> Result<String, String> {
    let mut output = Vec::new();
    stemma::jobs::output(&home(), &job, &mut output).map_err(said)?;
    String::from_utf8(output).map_err(said)
}

#[tauri::command]
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

#[tauri::command]
fn vault(group: String) -> Result<stemma::api::Vault, String> {
    let home = home();
    let credentials = stemma::session::required(&home).map_err(said)?;
    stemma::api::Server::at(&credentials.server)
        .map_err(said)?
        .vault(&credentials.token, &group)
        .map_err(said)
}

#[tauri::command]
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
        .invoke_handler(tauri::generate_handler![
            lines, open_line, save_line, preview, machine, job_output, offers, vault, launch
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
