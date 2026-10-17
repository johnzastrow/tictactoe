//go:build js && wasm

package webapp

import "runtime"

func goVersion() string { return runtime.Version() }
