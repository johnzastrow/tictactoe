// Package web holds the embedded browser client that the Go demo server
// serves. `make demo-web` writes the WASM build into dist/ (a placeholder
// page is served until then).
package web

import (
	"embed"
	"io/fs"
	"testing/fstest"
)

//go:embed all:dist
var dist embed.FS

const placeholder = `<!doctype html><meta charset=utf-8><title>demo</title>
<p>The Go web client has not been built. Run <code>make demo-web</code> in <code>go/</code>, then rebuild.</p>
`

// Dist is the web client's file tree (index.html at the root). Build
// outputs are not committed; until `make demo-web` has run, it is a
// one-page placeholder.
func Dist() fs.FS {
	sub, err := fs.Sub(dist, "dist")
	if err != nil {
		panic(err) // the directory is embedded at build time
	}
	if _, err := fs.Stat(sub, "index.html"); err != nil {
		return fstest.MapFS{"index.html": {Data: []byte(placeholder)}}
	}
	return sub
}
