//go:build js && wasm

// Command wasm is the Go demo web client (built to web/dist/app.wasm).
package main

import "git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/webapp"

func main() {
	webapp.Run()
	select {} // keep the Go runtime alive for callbacks
}
