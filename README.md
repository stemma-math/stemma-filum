# Stemma

Stemma is a working platform for mathematics research groups. This repository
contains the Rust client and daemon, the central server, the shared domain
model, and the Tauri desktop application.

## Requirements

- macOS 12 or newer to build the macOS application
- Rust from `rust-toolchain.toml`
- Node.js and npm
- Xcode Command Line Tools
- Git
- Lean/Lake and Typst when exercising formalization or composition features

Install the frontend dependencies once:

```sh
make setup
```

## Desktop application

Run the app against the development frontend:

```sh
make app-dev
```

Build a local macOS application with an ad-hoc signature:

```sh
make app
open target/release/bundle/macos/Stemma.app
```

The bundle is suitable for local testing. Its ad-hoc signature has no Apple
Developer identity, so distribution to another Mac still requires Developer ID
signing and notarization; those credentials are intentionally not part of this
repository.

The app uses the same XDG client state and local workspace as the `stemma`
command. Set `STEMMA_HOME` to an absolute directory to isolate a test account:

```sh
STEMMA_HOME="$PWD/.test-state/client" make app-dev
```

## Verification

The normal pre-commit suite is:

```sh
make check
```

It checks formatting, runs Clippy with warnings denied, type-checks and builds
the frontend, and runs the complete Rust workspace test suite. The repository
transport integration tests bind a loopback TCP port; run them directly if a
sandbox blocks network binding:

```sh
make test-repositories
```

Useful narrower commands are:

```sh
make test          # all Rust tests
make test-app      # desktop Rust shell and frontend
make test-server   # server library and HTTP integration tests
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
