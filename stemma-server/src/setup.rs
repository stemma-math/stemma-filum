//! The wizard that fills in the configuration file.
//!
//! It is a convenience on top of editing the file, never the only way to get
//! an instance configured: every question it asks has a key in the file and
//! a variable in the environment, so an instance can be provisioned with
//! nobody at a terminal.
//!
//! It asks only what has no safe default, shows the current value as the
//! answer to accept, edits the file in place so the comments survive, and
//! shows what it is about to change before it changes it.

use std::fmt::Write as _;
use std::io::{IsTerminal, Write as _};
use std::path::Path;

use anyhow::{Context, Result, bail};
use toml_edit::{DocumentMut, Item, Table, Value};

use crate::config;

/// Asks the questions and writes the answers.
///
/// Returns whether the file was changed.
pub fn run(config_path: &Path, data_dir: &Path) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!(
            "setup asks questions, and nobody is at the terminal to answer them: \
             edit {} instead",
            config_path.display()
        );
    }

    let before = match std::fs::read_to_string(config_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => config::skeleton(data_dir),
        Err(error) => {
            return Err(error).with_context(|| format!("cannot read {}", config_path.display()));
        }
    };

    let mut document: DocumentMut = before
        .parse()
        .with_context(|| format!("{} is not valid configuration", config_path.display()))?;

    ask_everything(&mut document)?;

    let after = document.to_string();

    if after == before {
        println!("Nothing changed.");
        return Ok(false);
    }

    println!();
    println!("{}", diff(&before, &after));

    if !ask_yes_no("Write these changes", true)? {
        println!("Nothing was written.");
        return Ok(false);
    }

    std::fs::write(config_path, &after)
        .with_context(|| format!("cannot write {}", config_path.display()))?;

    println!("Wrote {}.", config_path.display());
    println!("Run `stemma-server check` before starting the instance.");

    Ok(true)
}

/// Puts every question to the operator.
fn ask_everything(document: &mut DocumentMut) -> Result<()> {
    println!("Each answer shows the current value. Press enter to keep it.");
    println!();

    if let Some(answer) = ask(
        "The address this instance answers on",
        text_at(document, &["public_url"]),
    )? {
        set(document.as_table_mut(), "public_url", answer);
    }

    if let Some(answer) = ask(
        "The address to bind",
        text_at(document, &["listen"]).or(Some(config::DEFAULT_LISTEN.to_owned())),
    )? {
        set(document.as_table_mut(), "listen", answer);
    }

    let open = ask_yes_no(
        "May anyone create an account",
        flag_at(document, &["registration", "open"]).unwrap_or(true),
    )?;
    set(table(document, "registration"), "open", open);

    let group_requests = ask_yes_no(
        "May account holders request a group",
        flag_at(document, &["registration", "group_requests"]).unwrap_or(true),
    )?;
    set(
        table(document, "registration"),
        "group_requests",
        group_requests,
    );

    ask_about_mail(document)
}

/// Asks whether mail goes out, and where.
fn ask_about_mail(document: &mut DocumentMut) -> Result<()> {
    let configured = document.contains_key("mail");

    if !ask_yes_no("Send mail from this instance", configured)? {
        if configured {
            document.remove("mail");
            println!("  Invitations and password resets will be links only.");
        }
        return Ok(());
    }

    let from = ask(
        "  The address mail comes from",
        text_at(document, &["mail", "from"]),
    )?;
    let smtp_url = ask(
        "  The SMTP server to hand mail to",
        text_at(document, &["mail", "smtp_url"]).or(Some("smtp://localhost:25".to_owned())),
    )?;
    let password_env = ask(
        "  The environment variable holding the password, if any",
        text_at(document, &["mail", "password_env"]),
    )?;

    let mail = table(document, "mail");

    if let Some(from) = from {
        set(mail, "from", from);
    }
    if let Some(smtp_url) = smtp_url {
        set(mail, "smtp_url", smtp_url);
    }
    match password_env {
        Some(name) if !name.is_empty() => set(mail, "password_env", name),
        Some(_) => {
            mail.remove("password_env");
        }
        None => {}
    }

    Ok(())
}

/// Writes a value, keeping whatever comment sat beside the old one.
///
/// The comments in this file are the only documentation an operator has in
/// front of them while they edit it, and answering a question is no reason
/// to take them away.
fn set(table: &mut Table, key: &str, new: impl Into<Value>) {
    let decor = table
        .get(key)
        .and_then(Item::as_value)
        .map(|existing| existing.decor().clone());

    table[key] = Item::Value(new.into());

    if let (Some(decor), Some(written)) = (decor, table[key].as_value_mut()) {
        *written.decor_mut() = decor;
    }
}

/// The table `name`, created if the file has none.
fn table<'a>(document: &'a mut DocumentMut, name: &str) -> &'a mut Table {
    if !document.contains_key(name) {
        document[name] = Item::Table(Table::new());
    }

    document[name]
        .as_table_mut()
        .expect("the table was just put there")
}

/// The string at `path`, if there is one.
fn text_at(document: &DocumentMut, path: &[&str]) -> Option<String> {
    item_at(document, path)?.as_str().map(str::to_owned)
}

/// The boolean at `path`, if there is one.
fn flag_at(document: &DocumentMut, path: &[&str]) -> Option<bool> {
    item_at(document, path)?.as_bool()
}

/// Walks down to `path`.
fn item_at<'a>(document: &'a DocumentMut, path: &[&str]) -> Option<&'a Item> {
    let (first, rest) = path.split_first()?;
    let mut item = document.get(first)?;

    for step in rest {
        item = item.get(step)?;
    }

    Some(item)
}

/// Asks one question, offering `current` as the answer to accept.
fn ask(question: &str, current: Option<String>) -> Result<Option<String>> {
    match &current {
        Some(current) => print!("{question} [{current}]: "),
        None => print!("{question}: "),
    }

    let answer = read_line()?;

    Ok(match answer.is_empty() {
        true => current,
        false => Some(answer),
    })
}

/// Asks a question that can only be answered yes or no.
fn ask_yes_no(question: &str, current: bool) -> Result<bool> {
    loop {
        print!(
            "{question}? [{}]: ",
            match current {
                true => "Y/n",
                false => "y/N",
            }
        );

        match read_line()?.to_ascii_lowercase().as_str() {
            "" => return Ok(current),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("  Answer yes or no."),
        }
    }
}

/// Reads one answer.
fn read_line() -> Result<String> {
    std::io::stdout().flush().ok();

    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .context("cannot read the answer")?;

    Ok(line.trim().to_owned())
}

/// How many unchanged lines to show around a change.
const CONTEXT: usize = 2;

/// What changed, in the shape everybody already reads diffs in.
fn diff(before: &str, after: &str) -> String {
    let before: Vec<&str> = before.lines().collect();
    let after: Vec<&str> = after.lines().collect();
    let changes = line_changes(&before, &after);

    let interesting: Vec<bool> = changes
        .iter()
        .enumerate()
        .map(|(index, (marker, _))| {
            *marker != ' '
                || changes
                    .iter()
                    .skip(index.saturating_sub(CONTEXT))
                    .take(CONTEXT * 2 + 1)
                    .any(|(marker, _)| *marker != ' ')
        })
        .collect();

    let mut rendered = String::new();
    let mut skipping = false;

    for (index, (marker, line)) in changes.iter().enumerate() {
        if interesting[index] {
            let _ = writeln!(rendered, "{marker} {line}");
            skipping = false;
        } else if !skipping {
            let _ = writeln!(rendered, "  ...");
            skipping = true;
        }
    }

    rendered
}

/// Lines marked as kept, removed or added.
fn line_changes(before: &[&str], after: &[&str]) -> Vec<(char, String)> {
    let common = longest_common(before, after);

    let mut changes = Vec::new();
    let (mut left, mut right) = (0, 0);

    while left < before.len() && right < after.len() {
        if common[left][right] > 0 && before[left] == after[right] {
            changes.push((' ', before[left].to_owned()));
            left += 1;
            right += 1;
        } else if common[left + 1][right] >= common[left][right + 1] {
            changes.push(('-', before[left].to_owned()));
            left += 1;
        } else {
            changes.push(('+', after[right].to_owned()));
            right += 1;
        }
    }

    changes.extend(before[left..].iter().map(|line| ('-', (*line).to_owned())));
    changes.extend(after[right..].iter().map(|line| ('+', (*line).to_owned())));

    changes
}

/// The length of the longest common subsequence of every pair of suffixes.
fn longest_common(before: &[&str], after: &[&str]) -> Vec<Vec<usize>> {
    let mut lengths = vec![vec![0; after.len() + 1]; before.len() + 1];

    for left in (0..before.len()).rev() {
        for right in (0..after.len()).rev() {
            lengths[left][right] = if before[left] == after[right] {
                lengths[left + 1][right + 1] + 1
            } else {
                lengths[left + 1][right].max(lengths[left][right + 1])
            };
        }
    }

    lengths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_value_is_shown_as_a_removal_and_an_addition() {
        let before = "public_url = \"https://old.example.org\"\nlisten = \"127.0.0.1:8080\"\n";
        let after = "public_url = \"https://new.example.org\"\nlisten = \"127.0.0.1:8080\"\n";

        let rendered = diff(before, after);

        assert!(rendered.contains("- public_url = \"https://old.example.org\""));
        assert!(rendered.contains("+ public_url = \"https://new.example.org\""));
        assert!(rendered.contains("  listen = \"127.0.0.1:8080\""));
    }

    #[test]
    fn untouched_stretches_are_collapsed() {
        let before = (1..=20).map(|n| format!("line {n}")).collect::<Vec<_>>();
        let mut after = before.clone();
        after[10] = "line eleven".to_owned();

        let rendered = diff(&before.join("\n"), &after.join("\n"));

        assert!(rendered.contains("- line 11"));
        assert!(rendered.contains("+ line eleven"));
        assert!(rendered.contains("  ..."));
        assert!(!rendered.contains("line 2\n"));
    }

    #[test]
    fn editing_a_value_leaves_the_comments_around_it_alone() {
        let text = "# The address this instance answers on.\n\
                    public_url = \"https://old.example.org\"  # it is in every link\n";
        let mut document: DocumentMut = text.parse().unwrap();

        set(
            document.as_table_mut(),
            "public_url",
            "https://new.example.org",
        );

        assert_eq!(
            document.to_string(),
            "# The address this instance answers on.\n\
             public_url = \"https://new.example.org\"  # it is in every link\n"
        );
    }

    #[test]
    fn a_table_is_created_only_when_it_is_missing() {
        let mut document: DocumentMut = "[registration]\nopen = false\n".parse().unwrap();

        set(table(&mut document, "registration"), "group_requests", true);

        assert_eq!(flag_at(&document, &["registration", "open"]), Some(false));
        assert_eq!(
            flag_at(&document, &["registration", "group_requests"]),
            Some(true)
        );
    }

    #[test]
    fn the_skeleton_can_be_edited_without_losing_its_comments() {
        let text = config::skeleton(Path::new("/var/lib/stemma"));
        let mut document: DocumentMut = text.parse().unwrap();

        set(document.as_table_mut(), "public_url", "https://stemma.test");
        let edited = document.to_string();

        assert!(edited.contains("https://stemma.test"));
        assert!(edited.contains("# Stemma server configuration."));
        assert!(edited.contains("# [mail]"));
        assert!(edited.contains("# or \"unix:/run/stemma/server.sock\""));
    }
}
