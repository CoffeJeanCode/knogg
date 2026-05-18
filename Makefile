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
		cp /app/target/release/knogg /app/dist/.knogg.new && \
		chmod +x /app/dist/.knogg.new && \
		mv -f /app/dist/.knogg.new /app/dist/knogg && \
		cp /app/target/x86_64-pc-windows-gnu/release/knogg.exe /app/dist/.knogg.exe.new && \
		mv -f /app/dist/.knogg.exe.new /app/dist/knogg.exe'

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

clean:
	docker compose run --rm dev cargo clean
	rm -rf dist/

lint:
	docker compose run --rm dev cargo clippy -- -D warnings

fmt:
	docker compose run --rm dev cargo fmt

fmt-check:
	docker compose run --rm dev cargo fmt --check

check: fmt-check lint test
