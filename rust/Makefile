# pt (Rust) build helpers.
#
# Prerequisites for the web client:
#   rustup target add wasm32-unknown-unknown
#   trunk  (brew install trunk  or  cargo install trunk)
#   wasm-opt (binaryen) is optional but recommended (brew install binaryen)
#
# The web client builds into crates/pt-server/web-dist/, which the server
# embeds at compile time. Build the web client first, then the binary.
# demo works the same way: crates/demo-web builds into crates/demo-server/web-dist/.

WASM_TARGET_DIR ?= $(CURDIR)/target/wasm
# Size beats speed for the browser bundle: opt-level=z halves the .wasm
# (≈1.7 MB raw / 0.7 MB gzip vs 3.4 / 1.1 at the workspace default).
WASM_OPT_LEVEL ?= z

.PHONY: web pt demo-web demo test clean-web serve-demo

web:
	cd crates/pt-web && CARGO_PROFILE_RELEASE_OPT_LEVEL=$(WASM_OPT_LEVEL) CARGO_TARGET_DIR=$(WASM_TARGET_DIR) trunk build --release
	# precompressed copies; the server sends them to browsers that accept gzip
	find crates/pt-server/web-dist -type f \( -name '*.wasm' -o -name '*.js' -o -name '*.css' \) -exec gzip -9 -k -f {} \;

pt: web
	cargo build --release -p pt

demo-web:
	cd crates/demo-web && CARGO_PROFILE_RELEASE_OPT_LEVEL=$(WASM_OPT_LEVEL) CARGO_TARGET_DIR=$(WASM_TARGET_DIR) trunk build --release
	find crates/demo-server/web-dist -type f \( -name '*.wasm' -o -name '*.js' -o -name '*.css' \) -exec gzip -9 -k -f {} \;

demo: demo-web
	cargo build --release -p demo

test:
	cargo test
	CARGO_TARGET_DIR=$(WASM_TARGET_DIR) cargo clippy -p pt-web --target wasm32-unknown-unknown -- -D warnings
	cargo clippy -p demo-core -p demo-server -p demo-tui -p demo --all-targets -- -D warnings
	CARGO_TARGET_DIR=$(WASM_TARGET_DIR) cargo clippy -p demo-web --target wasm32-unknown-unknown -- -D warnings

clean-web:
	rm -rf crates/pt-server/web-dist crates/demo-server/web-dist

# Seed a demo database and serve it on http://127.0.0.1:8788
serve-demo: pt
	./target/release/pt seed --db pt-demo.db --password demo-pass-1 --force
	./target/release/pt serve --db pt-demo.db
