[private]
default:
    @just --list --unsorted

identity_dir := "identity"
frontend_dir := "stemma-app/frontend"
app_bundle := "target/release/bundle/macos/Stemma.app"

# Local test instances live here, away from anything installed.
test_state := justfile_directory() / ".test-state"
server_config := test_state / "server" / "server.toml"

# ---------------------------------------------------------------------------
# Setting up

[group("setup")]
[doc("Install the identity's and the frontend's npm dependencies.")]
setup:
    npm ci --prefix {{ identity_dir }}
    npm ci --prefix {{ frontend_dir }}

# ---------------------------------------------------------------------------
# Everyday development

[group("develop")]
[doc("Format the Rust code in place.")]
format:
    cargo fmt --all

alias fmt := format

[group("develop")]
[doc("Regenerate the identity's tokens.css and tokens.typ from tokens.json.")]
tokens:
    npm run build --prefix {{ identity_dir }}

[group("develop")]
[doc("Run the desktop application against the development frontend.")]
app-dev: sidecar
    npm run tauri:dev --prefix {{ frontend_dir }}

[group("develop")]
[doc("Answer the questions that configure the local test server.")]
server-setup:
    mkdir -p "{{ test_state }}/server"
    cargo run -p stemma-server -- --config "{{ server_config }}" setup --data-dir "{{ test_state }}/server/data"

[group("develop")]
[doc("Run a server command against the local test server; `just server` serves it.")]
server *args="serve":
    cargo run -p stemma-server -- --config "{{ server_config }}" {{ args }}

[group("develop")]
[doc("Run a `stemma` command with the local test account's home.")]
client *args:
    STEMMA_HOME="{{ test_state }}/client" cargo run -p stemma -- {{ args }}

# ---------------------------------------------------------------------------
# Checking

[group("check")]
[doc("Run everything a pull request must pass.")]
check: check-format check-tokens frontend docs lean lint test

[group("check")]
[doc("Check the Rust formatting.")]
check-format:
    cargo fmt --all --check

[group("check")]
[doc("Check the identity's generated tokens are up to date.")]
check-tokens:
    npm run check --prefix {{ identity_dir }}

[group("check")]
[doc("Run Clippy on every target, with warnings denied.")]
lint:
    cargo clippy --workspace --all-targets -- -D warnings

[group("check")]
[doc("Build the Rust documentation, private items included, with warnings denied.")]
docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items

[group("check")]
[doc("Run the Rust tests, of one crate when given (e.g. `just test stemma-server`).")]
test crate="":
    cargo test {{ if crate == "" { "--workspace" } else { "-p " + crate } }}

[group("check")]
[doc("Test, type-check and build the frontend.")]
frontend:
    npm test --prefix {{ frontend_dir }}
    npm run check --prefix {{ frontend_dir }}
    npm run build --prefix {{ frontend_dir }}

[group("check")]
[doc("Test the desktop application: the frontend, then its Rust shell.")]
test-app: frontend
    cargo test -p stemma-app

[group("check")]
[doc("Test the Git repository transport.")]
test-repositories:
    cargo test -p stemma-server --test repositories

[group("check")]
[doc("Build and test the Stemma Lean package.")]
lean:
    cd lean && lake build
    cd lean && lake test

# ---------------------------------------------------------------------------
# Building the application

[group("build")]
[doc("Build the command line the application carries with it.")]
sidecar:
    cargo build --release -p stemma
    mkdir -p stemma-app/binaries
    cp target/release/stemma "stemma-app/binaries/stemma-$(rustc -vV | sed -n 's/^host: //p')"

[group("build")]
[doc("Build the application binary alone, with the frontend and the command line inside it.")]
app-bin: sidecar frontend
    cargo build --release -p stemma-app --features custom-protocol

[group("build")]
[doc("Build and ad-hoc sign the macOS application.")]
app: sidecar
    npm run tauri:build --prefix {{ frontend_dir }}
    codesign --force --sign - "{{ app_bundle }}"

[group("build")]
[doc("Build the macOS application and validate its bundle.")]
app-smoke: app
    test -x "{{ app_bundle }}/Contents/MacOS/stemma-app"
    test -x "{{ app_bundle }}/Contents/MacOS/stemma"
    plutil -lint "{{ app_bundle }}/Contents/Info.plist"
    codesign --verify --deep --strict "{{ app_bundle }}"
