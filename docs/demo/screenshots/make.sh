#!/bin/bash
# Regenerate the demo gallery for one or both languages: web and TUI
# recordings of each control (steps.py), web | TUI side-by-side stills, and
# (with both languages captured) Rust-vs-Go 2x2 stills, clips and contact sheets.
#
#   docs/demo/screenshots/make.sh [rust] [go]
#
# Needs: the release binaries (rust: `make demo`, go: `make demo`), vhs (with
# ttyd and ffmpeg), uv, and Google Chrome. Each language gets a throwaway
# database and its own server on a spare port; your own server and database
# are not touched.
set -euo pipefail
cd "$(dirname "$0")"
HERE=$PWD
ROOT=$(cd ../../.. && pwd)
LANGS=("$@")
[ ${#LANGS[@]} -eq 0 ] && LANGS=(rust go)

python3 gen_tapes.py

for lang in "${LANGS[@]}"; do
  case $lang in
    rust) BIN=$ROOT/rust/target/release/demo; PORT=18797 ;;
    go)   BIN=$ROOT/go/bin/demo;              PORT=18796 ;;
    *) echo "unknown language: $lang" >&2; exit 2 ;;
  esac
  [ -x "$BIN" ] || { echo "build $BIN first (make demo)" >&2; exit 1; }

  WORK=$(mktemp -d)
  # a fresh database per pass, so every pass starts from the defaults
  start_server() {
    rm -f "$WORK"/demo.db*
    "$BIN" serve --db "$WORK/demo.db" --bind 127.0.0.1:$PORT >>"$WORK/server.log" 2>&1 &
    SERVER=$!
    for _ in $(seq 50); do curl -sf http://127.0.0.1:$PORT/api/health >/dev/null && return; sleep 0.2; done
    echo "server did not start" >&2; exit 1
  }
  stop_server() { kill $SERVER 2>/dev/null || true; wait $SERVER 2>/dev/null || true; }
  trap 'stop_server; rm -rf "$WORK"' EXIT

  OUT=$HERE/$lang
  rm -rf "$OUT"; mkdir -p "$OUT/raw"

  echo "== $lang: web stills, then web recording"
  for mode in "" --video; do
    start_server
    DEMO_URL=http://127.0.0.1:$PORT uv run -q --with playwright python capture_web.py "$lang" "$OUT/raw" $mode
    stop_server
  done

  echo "== $lang: TUI recordings"
  start_server
  cd "$OUT"
  for tape in ../tapes/[0-9]*.tape ../tapes/tour.tape; do
    DEMO_BIN=$BIN DEMO_SERVER=http://127.0.0.1:$PORT vhs "$tape" >/dev/null 2>&1 \
      || { echo "vhs failed: $tape" >&2; exit 1; }
    # smaller GIFs: 10 fps, 1200 px wide, a 96-colour palette of what changes
    gif=tui-$(basename "$tape" .tape).gif
    ffmpeg -y -loglevel error -i "$gif" -filter_complex \
      "fps=10,scale=1200:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
      "$WORK/small.gif" && mv "$WORK/small.gif" "$gif"
    echo "ok $(basename "$tape" .tape)"
  done
  cd "$HERE"
  stop_server

  echo "== $lang: web clips"
  python3 cut_web.py "$lang"

  echo "== $lang: side-by-side"
  uv run -q --with pillow python compose.py "$lang"

  rm -rf "$WORK"
  trap - EXIT
done

# Rust vs Go: 2x2 stills, clips and contact sheets, once both languages have raw captures
if [ -d rust/raw ] && [ -d go/raw ]; then
  echo "== Rust vs Go: 2x2"
  uv run -q --with pillow python compose4.py
fi
