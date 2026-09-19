APP_BUNDLE := target/release/bundle/macos/Stemma.app

.PHONY: setup fmt check test test-app test-server test-repositories frontend app-dev app app-smoke

setup:
	npm ci --prefix stemma-app/frontend

fmt:
	cargo fmt --all --check

frontend:
	npm run check --prefix stemma-app/frontend
	npm run build --prefix stemma-app/frontend

test:
	cargo test --workspace

test-app:
	cargo test -p stemma-app
	npm run check --prefix stemma-app/frontend
	npm run build --prefix stemma-app/frontend

test-server:
	cargo test -p stemma-server

test-repositories:
	cargo test -p stemma-server --test repositories

check: fmt frontend
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

app-dev:
	npm run tauri:dev --prefix stemma-app/frontend

app:
	npm run tauri:build --prefix stemma-app/frontend
	codesign --force --sign - "$(APP_BUNDLE)"

app-smoke: app
	test -x "$(APP_BUNDLE)/Contents/MacOS/stemma-app"
	plutil -lint "$(APP_BUNDLE)/Contents/Info.plist"
	codesign --verify --deep --strict "$(APP_BUNDLE)"
