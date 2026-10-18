#!/bin/bash
# Record the tic-tac-toe live demo: the TUI (X, keyboard) and the browser
# (O, mouse) playing one game at the same moment, for one or both languages.
#
#   docs/screenshots/make.sh [rust] [go]
#
# Writes <lang>/live.gif (both recordings side by side) and <lang>/live.png.
# Needs the release binaries, vhs (with ttyd and ffmpeg), uv and Google Chrome.
set -euo pipefail
cd "$(dirname "$0")"
HERE=$PWD
ROOT=$(cd ../.. && pwd)
LANGS=("$@")
[ ${#LANGS[@]} -eq 0 ] && LANGS=(rust go)

for lang in "${LANGS[@]}"; do
  case $lang in
    rust) BIN=$ROOT/rust/target/release/ttt; PORT=8786 ;;
    go)   BIN=$ROOT/go/bin/ttt;              PORT=8785 ;;
    *) echo "unknown language: $lang" >&2; exit 2 ;;
  esac
  [ -x "$BIN" ] || { echo "build $BIN first" >&2; exit 1; }
  OUT=$HERE/$lang
  rm -rf "$OUT/raw"; mkdir -p "$OUT/raw"

  echo "== $lang: recording"
  # the browser waits for the TUI's web server, then plays O as X moves
  uv run -q --with playwright python live.py http://127.0.0.1:$PORT "$OUT/raw" &
  WEB=$!
  (cd "$OUT/raw" && TTT_BIN=$BIN TTT_PORT=$PORT vhs ../../tapes/live.tape >/dev/null 2>&1)
  wait $WEB

  echo "== $lang: compositing"
  uv run -q --with pillow python compose.py "$lang"
done
