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

The bundle carries the `stemma` command with it: the application's
"This machine" panel installs it into `~/.local/bin`, so one install
gives both surfaces. Everything the application does can be done from
the command line, and everything the command line does is in the
application — writing mathematics is the one thing only the application
offers.

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

`identity/` is the single source of Stemma's look. It is both a Rust crate
(`stemma-identity`), which the server embeds, and an npm package
(`@stemma-math/identity`), which the desktop frontend imports:

- `tokens.json` holds every colour, face, text role, type step, spacing,
  radius, layout width, shadow, duration and control density in the Design
  Tokens Community Group format. Style
  Dictionary generates `tokens.css` (custom properties) and `tokens.typ`
  (colours and faces for Typst) from it: run `npm run build` in `identity/`
  after editing it; `just check` fails while they are out of date. Neither
  stylesheet defines tokens of its own. Its `dark` group redefines the
  semantic colours for the dark appearance, which follows the system unless
  the root says `data-theme="light"` or `data-theme="dark"`: the web sets it
  from a cookie its appearance switch writes, and the desktop window from
  the Appearance settings.
- `components.css` holds the shared components as classes over native HTML,
  in cascade layers so each surface's own rules win: `.button` (with
  `--primary`, `--quiet`, `--danger`, `--link` and `--icon`), `.field`,
  `.input`, `.choice`, `.fieldset`, `.chip`, `.count`, `.avatar`, `.badge`,
  `.card`, `.table`, `.menu`, `.dialog`, `.problem`, `.notice`, `.toast`,
  `.tooltip`, `.segmented`, `.skeleton`, `.spinner`, `.label` and the
  wordmark. Controls are
  comfortable by default and compact inside `[data-density="compact"]`, as the
  desktop window is.
- `fonts.css` and `fonts/` carry Instrument Sans (the interface), Inter (the
  wordmark), JetBrains Mono (identifiers) and Libertinus Serif (mathematics),
  subset to Latin, each beside its SIL Open Font License. The server
  serves them from its own origin, so no page sends a reader to a font host.
- `mark-on-dark.svg` and `mark-on-light.svg` are the mark for navy and for
  light surfaces, `mark-animated.js` draws it live on the front page (with
  `symbol.webp` as its still), and `glyphs.svg` holds the line glyphs as a
  sprite; `web/` holds the favicons and the home-screen icons,
  `icons/` the application icons, and `noise.svg` the surface texture.
- `descent.js` draws a line's descent graph from the placement
  `stemma_core::descent` computes. Neither surface shows the graph at present.

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
