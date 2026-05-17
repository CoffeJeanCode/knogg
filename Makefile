dev:
	docker compose run --rm dev bash

test:
	docker compose run --rm dev cargo test

build:
	docker compose run --rm dev cargo build --release

# Build Unix (Linux) + Windows executables into ./dist.
release:
	docker compose build dev
	docker compose run --rm dev cargo build --release
	docker compose run --rm dev cargo build --release --target x86_64-pc-windows-gnu
	mkdir -p dist
	docker compose run --rm dev sh -c '\
		cp /app/target/release/knogg /app/dist/knogg && \
		chmod +x /app/dist/knogg && \
		cp /app/target/x86_64-pc-windows-gnu/release/knogg.exe /app/dist/knogg.exe'

run:
	docker compose run --rm dev cargo run --

init:
	docker compose run --rm dev cargo run -- init --path ./.knogg

status:
	docker compose run --rm dev cargo run -- status --path ./.knogg

handoff:
	docker compose run --rm dev cargo run -- handoff --to cursor --path ./.knogg

sync:
	docker compose run --rm dev cargo run -- sync --path ./.knogg

# --- kindle-to-obsidian (nested crate; build artifacts under kindle-to-obsidian/target/) ---
kindle-test:
	docker compose run --rm dev cargo test --manifest-path kindle-to-obsidian/Cargo.toml

kindle-build:
	docker compose run --rm dev cargo build --release --manifest-path kindle-to-obsidian/Cargo.toml

kindle-release:
	docker compose build dev
	docker compose run --rm dev cargo build --release --manifest-path kindle-to-obsidian/Cargo.toml
	docker compose run --rm dev cargo build --release --manifest-path kindle-to-obsidian/Cargo.toml --target x86_64-pc-windows-gnu
	mkdir -p dist
	docker compose run --rm dev sh -c '\
		cp /app/kindle-to-obsidian/target/release/kindle-to-obsidian /app/dist/kindle-to-obsidian && \
		chmod +x /app/dist/kindle-to-obsidian && \
		cp /app/kindle-to-obsidian/target/x86_64-pc-windows-gnu/release/kindle-to-obsidian.exe /app/dist/kindle-to-obsidian.exe'
