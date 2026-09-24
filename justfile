[private]
default:
    @just --list --unsorted

# Application variables
frontend_dir := "stemma-app/frontend"
app_bundle := "target/release/bundle/macos/Stemma.app"

[group("env")]
[doc("Install the frontend dependencies.")]
setup:
  npm ci --prefix {{frontend_dir}}

[group("test")]
[doc("Check the Rust formatting.")]
format:
  cargo fmt --all --check

alias fmt := format

[group("test")]
[doc("Test, type-check and build the frontend.")]
frontend:
  npm test --prefix {{frontend_dir}}
  npm run check --prefix {{frontend_dir}}
  npm run build --prefix {{frontend_dir}}

[group("test")]
[doc("Build the Rust documentation, failing on broken links.")]
docs:
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

[group("test")]
[doc("Run all Rust tests.")]
test:
  cargo test --workspace

[group("test")]
[doc("Test the desktop shell and frontend.")]
test-app: frontend
  cargo test -p stemma-app

[group("test")]
[doc("Test the server.")]
test-server:
  cargo test -p stemma-server

[group("test")]
[doc("Test the Git repository transport.")]
test-repositories:
  cargo test -p stemma-server --test repositories

[group("test")]
[doc("Run the complete pre-commit verification suite.")]
check: format frontend docs
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace

[group("app")]
[doc("Build the command line the application carries with it.")]
sidecar:
  cargo build --release -p stemma
  mkdir -p stemma-app/binaries
  cp target/release/stemma "stemma-app/binaries/stemma-$(rustc -vV | sed -n 's/^host: //p')"

[group("app")]
[doc("Run the desktop application with the development frontend.")]
app-dev: sidecar
  npm run tauri:dev --prefix {{frontend_dir}}

[group("app")]
[doc("Build the application binary alone, frontend and command line inside it.")]
app-bin: sidecar frontend
  cargo build --release -p stemma-app --features custom-protocol

[group("app")]
[doc("Build and ad-hoc sign the macOS application.")]
app: sidecar
  npm run tauri:build --prefix {{frontend_dir}}
  codesign --force --sign - "{{app_bundle}}"

[group("app")]
[doc("Build and validate the macOS application bundle.")]
app-smoke: app
  test -x "{{app_bundle}}/Contents/MacOS/stemma-app"
  test -x "{{app_bundle}}/Contents/MacOS/stemma"
  plutil -lint "{{app_bundle}}/Contents/Info.plist"
  codesign --verify --deep --strict "{{app_bundle}}"
