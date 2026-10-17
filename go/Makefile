# pt (Go) build. `make web` compiles the Go WebAssembly client into
# internal/pt/web/dist/, which the server embeds; `make pt` builds the
# binary with it. Needs only the Go toolchain.

WEBDIST := internal/pt/web/dist
GOROOT  := $(shell go env GOROOT)

.PHONY: web pt test clean-web

web:
	mkdir -p $(WEBDIST)
	GOOS=js GOARCH=wasm go build -trimpath -ldflags="-s -w" -o $(WEBDIST)/app.wasm ./internal/pt/webapp/wasm
	cp $(GOROOT)/lib/wasm/wasm_exec.js $(WEBDIST)/wasm_exec.js
	cp internal/pt/webapp/static/index.html internal/pt/webapp/static/boot.js internal/pt/webapp/static/style.css $(WEBDIST)/
	gzip -9 -k -f $(WEBDIST)/app.wasm $(WEBDIST)/wasm_exec.js $(WEBDIST)/style.css
	@ls -l $(WEBDIST)/app.wasm | awk '{printf "app.wasm: %.1f MB\n", $$5/1048576}'

pt: web
	go build -trimpath -ldflags="-s -w" -o bin/pt ./cmd/pt

test:
	go vet ./...
	GOOS=js GOARCH=wasm go vet ./internal/pt/webapp/...
	go test ./...

clean-web:
	find $(WEBDIST) -type f ! -name .gitkeep -delete
