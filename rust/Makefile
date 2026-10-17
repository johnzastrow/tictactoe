# pt (Rust) build helpers.
#
# Prerequisites for the web client:
#   rustup target add wasm32-unknown-unknown
#   trunk  (brew install trunk  or  cargo install trunk)
#   wasm-opt (binaryen) is optional but recommended (brew install binaryen)
#
# The web client builds into crates/pt-server/web-dist/, which the server
# embeds at compile time. Build the web client first, then the binary.

WASM_TARGET_DIR ?= $(CURDIR)/target/wasm
# Size beats speed for the browser bundle: opt-level=z halves the .wasm
# (≈1.7 MB raw / 0.7 MB gzip vs 3.4 / 1.1 at the workspace default).
WASM_OPT_LEVEL ?= z

.PHONY: web pt test clean-web serve-demo

web:
	cd crates/pt-web && CARGO_PROFILE_RELEASE_OPT_LEVEL=$(WASM_OPT_LEVEL) CARGO_TARGET_DIR=$(WASM_TARGET_DIR) trunk build --release

pt: web
	cargo build --release -p pt

test:
	cargo test
	CARGO_TARGET_DIR=$(WASM_TARGET_DIR) cargo clippy -p pt-web --target wasm32-unknown-unknown -- -D warnings

clean-web:
	rm -rf crates/pt-server/web-dist

# Seed a demo database and serve it on http://127.0.0.1:8788
serve-demo: pt
	./target/release/pt seed --db pt-demo.db --password demo-pass-1 --force
	./target/release/pt serve --db pt-demo.db
