// Package web holds the embedded browser client that the Go server serves.
// The WASM build writes its output into dist/ (a placeholder ships until then).
package web

import (
	"embed"
	"io/fs"
)

//go:embed all:dist
var dist embed.FS

// Dist is the web client's file tree (index.html at the root).
func Dist() fs.FS {
	sub, err := fs.Sub(dist, "dist")
	if err != nil {
		panic(err) // the directory is embedded at build time
	}
	return sub
}
