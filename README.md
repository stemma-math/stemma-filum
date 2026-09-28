# Stemma

Stemma is a working platform for mathematics research groups. This repository
contains the Rust client and daemon, the central server, the shared domain
model, and the Tauri desktop application.

## Layout

| Path | What it is |
|---|---|
| `stemma-core/` | Domain vocabulary and the versioned HTTP protocol, shared by client and server |
| `stemma/` | The `stemma` client: command line, daemon, group repositories, Typst and Lean tooling |
| `stemma-server/` | The `stemma-server` binary: API, web interface, database, git remotes, job queue |
| `stemma-app/` | The Tauri desktop application; its Svelte frontend is in `stemma-app/frontend` |
| `identity/` | The visual identity: design tokens, faces, the mark, icons and the descent drawing |
| `typst/` | The `@stemma/line` Typst package every line is typeset with |
| `lean/` | The `stemma` Lean package for marks and formal-library extraction |

Dependencies point one way: `stemma` and `stemma-server` depend on
`stemma-core`, never on each other, and the app depends on `stemma`.

## Lines

A line lives in `lines/<name>/line.json` inside a group repository. It is a
title, an optional description and an ordered list of blocks:

- `prose`: intuition and connecting text;
- `definition`: central unless marked otherwise;
- `theorem`: under any environment name (Theorem, Lemma, …), ordinary unless
  marked central;
- `proof`: the proof of one definition or theorem, linked by block id.

Every block has a stable `blk:` identifier; definitions and theorems may also
have a siglum such as `thm:main`, which `@thm:main` cites in Typst content.
Only `line.json` is committed. The Typst document, and everything built from
it, is rendered from it by `stemma-core` (`render::typst`) with the package in
`typst/`, which also leaves each block's fields as queryable `metadata`.

## Synchronisation

The server owns every line's history, one revision at a time. A machine keeps
a mirror of the lines its owner can see (`stemma::mirror`): the engine's own
store lives in the client's state directory, and the files in the workspace
are written from it. Saving a line sends it a couple of seconds later, typeset
to HTML with the group's pinned Typst, against the revision it was written
from; offline, it waits. When the server moved on meanwhile, the line is
merged block by block, and only a block changed on both sides waits for a
decision (`stemma conflict list`). A file edited by hand is written again from
the store, and the edited version is kept aside.

`stemma sync` runs one round at once, and `stemma status` says which lines are
not sent yet or need a decision.

The same engine runs in the application and in the daemon (`stemma daemon
run`), one at a time: the application keeps the machine while it is open, and
the daemon takes over when it closes. Once a stretch of writing is over, the
machine that wrote it builds the repository and publishes what it holds, each
line named by its revision; when nothing arrives a few minutes after the
writing ended, the server queues a `build` job for another machine of the
group. `stemma export GROUP` makes a read-only git clone of a repository, for
anyone who can read it.

## Agent jobs

Each requested agent job runs in its own directory below the operating
system's temporary directory. The directory contains the group's repository,
the target `line.json` at the requested revision, versioned `AGENTS.md` and
`TASK.md` instructions, and `scratch/`. It is deliberately left in place for
the machine owner to inspect and is reported with the job.

Agents submit only scoped block and library-file operations through the
`stemma-job` command placed on their `PATH`. The command validates each action,
builds proposed Lean changes on request, records an explanation or a negative
result, and submits with `stemma-job done`. Raw vendor output stays on the
machine. Accepted line operations are merged block by block, and library
operations file by file, against the revision from which they were made.

Models, effort levels, defaults, and the time limit are machine-owned settings
in the client configuration. For example:

```toml
[jobs]
timeout_minutes = 45

[agents.codex]
models = ["gpt-5-codex"]
efforts = ["medium", "high"]
model = "gpt-5-codex"
effort = "medium"

[agents.codex.tasks.prove]
effort = "high"
```

Only declared models and effort levels are offered by the launcher. A launch
may override the defaults among those choices. Queued and running jobs can be
cancelled from the application or with `stemma job cancel JOB`.

## Requirements

- macOS 12 or newer to build the macOS application
- Rust from `rust-toolchain.toml`
- Node.js 22.12 or newer and npm
- Just
- Xcode Command Line Tools
- Git
- Lean/Lake and Typst when exercising formalization or composition features

Install the frontend dependencies once:

```sh
just setup
```

## Desktop application

Run the app against the development frontend:

```sh
just app-dev
```

Build a local macOS application with an ad-hoc signature:

```sh
just app
open target/release/bundle/macos/Stemma.app
```

The bundle carries the `stemma` command with it. *Set Up This Machine…*
(after the first sign-in, in the application menu, and as Settings → This
machine) is a checklist of the background service, the command line (a
link in `~/.local/bin`, and the line that puts that directory on the
shell's path), the groups the machine works for, the agents and Lean.
The service runs the link rather than the bundle, and every launch of the
application points the link at the command it carries, rewrites the
service if it names anything else, and restarts a daemon from an earlier
build. So one install gives both surfaces. Everything the application
does can be done from the command line, and everything the command line
does is in the application — writing mathematics is the one thing only
the application offers.

*Remove Stemma from This Machine…* (in the application menu and in
Settings), or `stemma uninstall`, sends what is not on the server yet,
removes the service, the link and the path line, takes the machine back
on the server, signs out, and asks whether to delete what Stemma keeps on
the machine.

The bundle is suitable for local testing. Its ad-hoc signature has no Apple
Developer identity, so distribution to another Mac still requires Developer ID
signing and notarization; those credentials are intentionally not part of this
repository.

The app uses the same XDG client state and local workspace as the `stemma`
command. Set `STEMMA_HOME` to an absolute directory to isolate a test account:

```sh
STEMMA_HOME="$PWD/.test-state/client" just app-dev
```

## Visual identity

`identity/` is the single source of Stemma's look: design tokens, shared
components, faces, the mark, icons and the descent drawing. It is both a Rust
crate (`stemma-identity`), which the server and the application embed, and an
npm package (`@stemma-math/identity`), which the desktop frontend imports. Its
own [README](identity/README.md) describes the layout and how to regenerate
the tokens after editing `identity/tokens/tokens.json`; `just check` fails
while the generated files are out of date.

## Verification

The normal pre-commit suite is:

```sh
just check
```

It checks formatting, tests, type-checks and builds the frontend, builds the
Rust documentation, runs Clippy with warnings denied, and runs the complete Rust
workspace test suite. The
client HTTP tests and repository transport integration tests bind loopback TCP
ports. Run the suite with local network permission if a sandbox blocks binding.
The repository transport tests can also be run separately:

```sh
just test-repositories
```

Useful narrower commands are:

```sh
just test          # all Rust tests
just test-app      # desktop Rust shell and frontend
just test-server   # server library and HTTP integration tests
```

## Local server

Use an isolated directory so testing never touches an installed instance:

```sh
export STEMMA_TEST_ROOT="$PWD/.test-state/server"
mkdir -p "$STEMMA_TEST_ROOT"
cargo run -p stemma-server -- --config "$STEMMA_TEST_ROOT/server.toml" setup \
  --data-dir "$STEMMA_TEST_ROOT/data"
```

After completing the generated configuration:

```sh
cargo run -p stemma-server -- --config "$STEMMA_TEST_ROOT/server.toml" migrate
cargo run -p stemma-server -- --config "$STEMMA_TEST_ROOT/server.toml" check
cargo run -p stemma-server -- --config "$STEMMA_TEST_ROOT/server.toml" serve
```

Run the CLI and app with a separate `STEMMA_HOME`, sign in to that address, and
keep the server in the foreground so failures remain visible.
