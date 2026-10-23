# pt and demo (Go) builds. `make web` compiles the Go WebAssembly client into
# internal/pt/web/dist/, which the server embeds; `make pt` builds the
# binary with it. `make demo-web` / `make demo` do the same for demo
# (internal/demo/web/dist/, bin/demo). `make ttt-tui` builds the terminal-only
# tic-tac-toe binary (no web server linked in); `make ttt-tui-windows`
# cross-compiles it to bin/ttt-tui.exe. Needs only the Go toolchain.

WEBDIST  := internal/pt/web/dist
DEMODIST := internal/demo/web/dist
GOROOT   := $(shell go env GOROOT)

.PHONY: web pt demo-web demo ttt-tui ttt-tui-windows test clean-web clean-demo-web

web:
	mkdir -p $(WEBDIST)
	GOOS=js GOARCH=wasm go build -trimpath -ldflags="-s -w" -o $(WEBDIST)/app.wasm ./internal/pt/webapp/wasm
	cp $(GOROOT)/lib/wasm/wasm_exec.js $(WEBDIST)/wasm_exec.js
	cp internal/pt/webapp/static/index.html internal/pt/webapp/static/boot.js internal/pt/webapp/static/style.css $(WEBDIST)/
	gzip -9 -k -f $(WEBDIST)/app.wasm $(WEBDIST)/wasm_exec.js $(WEBDIST)/style.css
	@ls -l $(WEBDIST)/app.wasm | awk '{printf "app.wasm: %.1f MB\n", $$5/1048576}'

pt: web
	go build -trimpath -ldflags="-s -w" -o bin/pt ./cmd/pt

demo-web:
	mkdir -p $(DEMODIST)
	GOOS=js GOARCH=wasm go build -trimpath -ldflags="-s -w" -o $(DEMODIST)/app.wasm ./internal/demo/webapp/wasm
	cp $(GOROOT)/lib/wasm/wasm_exec.js $(DEMODIST)/wasm_exec.js
	cp internal/demo/webapp/static/index.html internal/demo/webapp/static/boot.js internal/demo/webapp/static/style.css $(DEMODIST)/
	gzip -9 -k -f $(DEMODIST)/app.wasm $(DEMODIST)/wasm_exec.js $(DEMODIST)/style.css
	@ls -l $(DEMODIST)/app.wasm | awk '{printf "app.wasm: %.1f MB\n", $$5/1048576}'

demo: demo-web
	go build -trimpath -ldflags="-s -w" -o bin/demo ./cmd/demo

ttt-tui:
	CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o bin/ttt-tui ./cmd/ttt-tui

ttt-tui-windows:
	CGO_ENABLED=0 GOOS=windows GOARCH=amd64 go build -trimpath -ldflags="-s -w" -o bin/ttt-tui.exe ./cmd/ttt-tui

test:
	go vet ./...
	GOOS=js GOARCH=wasm go vet ./internal/pt/webapp/...
	GOOS=js GOARCH=wasm go vet ./internal/demo/core/... ./internal/demo/webapp/...
	go test ./...

clean-web:
	find $(WEBDIST) -type f ! -name .gitkeep -delete

clean-demo-web:
	find $(DEMODIST) -type f ! -name .gitkeep -delete
