package store_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/store"
)

// The Go server embeds a copy of docs/demo/schema.sql; it must not drift.
func TestSchemaMatchesDocs(t *testing.T) {
	doc, err := os.ReadFile("../../../../docs/demo/schema.sql")
	if err != nil {
		t.Fatal(err)
	}
	if string(doc) != store.Schema {
		t.Fatal("go/internal/demo/store/schema.sql differs from docs/demo/schema.sql — copy it over")
	}
}

func TestInitPutReset(t *testing.T) {
	path := filepath.Join(t.TempDir(), "demo.db")
	st, err := store.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	all, err := st.All()
	if err != nil || len(all) != len(core.Keys) {
		t.Fatal(len(all), err)
	}
	for _, k := range core.Keys {
		e := all[k]
		if string(e.Value) != string(core.Default(k)) || e.Version != 1 || e.By != "" || len(e.At) != 20 {
			t.Fatalf("%s: %+v", k, e)
		}
	}
	if err := st.Put("select.status", core.Entry{Value: json.RawMessage(`"done"`), Version: 2, By: "bob", At: "2026-10-18T09:00:00Z"}); err != nil {
		t.Fatal(err)
	}
	if err := st.Put("nope", core.Entry{Value: json.RawMessage(`1`), Version: 2}); err == nil {
		t.Fatal("unknown key stored")
	}
	st.Close()

	// Reopening keeps values and doesn't re-insert defaults.
	st, err = store.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer st.Close()
	e, err := st.Get("select.status")
	if err != nil || string(e.Value) != `"done"` || e.Version != 2 || e.By != "bob" {
		t.Fatal(e, err)
	}
	out, err := st.Reset([]string{"select.status"}, "amy", "2026-10-18T10:00:00Z")
	if err != nil || len(out) != 1 || out[0].Version != 3 || string(out[0].Value) != `"in_progress"` || out[0].By != "amy" {
		t.Fatal(out, err)
	}
	if _, err := st.Reset([]string{"text.name", "nope"}, "amy", "x"); err == nil {
		t.Fatal("unknown key reset")
	}
	if e, _ := st.Get("text.name"); e.Version != 1 {
		t.Fatal("partial reset was not rolled back")
	}
	out, err = st.Reset(nil, "", "2026-10-18T11:00:00Z")
	if err != nil || len(out) != len(core.Keys) {
		t.Fatal(err)
	}
	all, _ = st.All()
	if all["select.status"].Version != 4 || all["text.name"].Version != 2 || all["text.name"].By != "" {
		t.Fatal(all["select.status"], all["text.name"])
	}
}

func TestBadStoredJSONReadsAsDefault(t *testing.T) {
	st, err := store.OpenMemory()
	if err != nil {
		t.Fatal(err)
	}
	defer st.Close()
	if err := st.Put("text.name", core.Entry{Value: json.RawMessage(`{broken`), Version: 5, At: "x"}); err != nil {
		t.Fatal(err)
	}
	e, _ := st.Get("text.name")
	if string(e.Value) != `"Ada Lovelace"` || e.Version != 5 {
		t.Fatal(e)
	}
}
