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
[doc("Type-check and build the frontend.")]
frontend:
  npm run check --prefix {{frontend_dir}}
  npm run build --prefix {{frontend_dir}}

[group("test")]
[doc("Run all Rust tests.")]
test:
  cargo test --workspace

[group("test")]
[doc("Test the desktop shell and frontend.")]
test-app:
  cargo test -p stemma-app
  npm run check --prefix {{frontend_dir}}
  npm run build --prefix {{frontend_dir}}

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
check: format frontend
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace

[group("app")]
[doc("Run the desktop application with the development frontend.")]
app-dev:
  npm run tauri:dev --prefix {{frontend_dir}}

[group("app")]
[doc("Build and ad-hoc sign the macOS application.")]
app:
  npm run tauri:build --prefix {{frontend_dir}}
  codesign --force --sign - "{{app_bundle}}"

[group("app")]
[doc("Build and validate the macOS application bundle.")]
app-smoke: app
  test -x "{{app_bundle}}/Contents/MacOS/stemma-app"
  plutil -lint "{{app_bundle}}/Contents/Info.plist"
  codesign --verify --deep --strict "{{app_bundle}}"
