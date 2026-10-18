#!/bin/bash
# Regenerate the pt form-control gallery: side-by-side stills (web | TUI) and
# TUI recordings, for one or both languages.
#
#   docs/pt/screenshots/make.sh [rust] [go]
#
# Needs: the release binaries (rust: `make pt`, go: `make pt`), vhs (with ttyd
# and ffmpeg), uv, and Google Chrome. Each language gets a throwaway database
# seeded with a demo password and its own server on a spare port; your own
# server and database are not touched.
set -euo pipefail
cd "$(dirname "$0")"
HERE=$PWD
ROOT=$(cd ../../.. && pwd)
LANGS=("$@")
[ ${#LANGS[@]} -eq 0 ] && LANGS=(rust go)
PASSWORD=demo-pass-123  # matches tapes/_setup.tape

for lang in "${LANGS[@]}"; do
  case $lang in
    rust) BIN=$ROOT/rust/target/release/pt; PORT=8799 ;;
    go)   BIN=$ROOT/go/bin/pt;              PORT=8798 ;;
    *) echo "unknown language: $lang" >&2; exit 2 ;;
  esac
  [ -x "$BIN" ] || { echo "build $BIN first (make pt)" >&2; exit 1; }

  WORK=$(mktemp -d)
  "$BIN" seed --db "$WORK/demo.db" --password "$PASSWORD" >/dev/null
  "$BIN" serve --db "$WORK/demo.db" --bind 127.0.0.1:$PORT >"$WORK/server.log" 2>&1 &
  SERVER=$!
  trap 'kill $SERVER 2>/dev/null || true; rm -rf "$WORK"' EXIT
  for _ in $(seq 50); do curl -sf http://127.0.0.1:$PORT/api/health >/dev/null && break; sleep 0.2; done

  OUT=$HERE/$lang
  rm -rf "$OUT/raw"; mkdir -p "$OUT/raw"

  echo "== $lang: web stills"
  PT_URL=http://127.0.0.1:$PORT PT_PASSWORD=$PASSWORD \
    uv run -q --with playwright python capture_web.py "$lang" "$OUT/raw"

  echo "== $lang: TUI recordings"
  cd "$OUT"
  for tape in ../tapes/[0-9]*.tape ../tapes/views.tape; do
    # a fresh config dir per tape, so no cached login skips the login form
    rm -rf "$WORK/cfg"; mkdir "$WORK/cfg"
    PT_BIN=$BIN PT_SERVER=http://127.0.0.1:$PORT PT_CONFIG_DIR=$WORK/cfg \
      vhs "$tape" >/dev/null 2>&1 || { echo "vhs failed: $tape" >&2; exit 1; }
    # smaller GIFs: 10 fps, 1200 px wide, a 96-colour palette of what changes
    gif=$(basename "$tape" .tape).gif
    ffmpeg -y -loglevel error -i "$gif" -filter_complex \
      "fps=10,scale=1200:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=96:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
      "$WORK/small.gif" && mv "$WORK/small.gif" "$gif"
    echo "ok $(basename "$tape" .tape)"
  done
  cd "$HERE"

  echo "== $lang: side-by-side"
  uv run -q --with pillow python compose.py "$lang"

  kill $SERVER 2>/dev/null || true
  wait $SERVER 2>/dev/null || true
  rm -rf "$WORK"
  trap - EXIT
done
