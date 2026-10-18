#!/bin/bash
# Run the demo web e2e suite against a fresh server.
#
#   DEMO_BIN=../../../target/release/demo ./run_all.sh [test_*.py …]
#
# Needs: a built demo binary with the web client embedded (`make demo`),
# uv (Playwright is pulled in with `uv run --with playwright`), and Google
# Chrome (Playwright channel "chrome").
set -euo pipefail
cd "$(dirname "$0")"
DEMO_BIN=${DEMO_BIN:-../../../target/release/demo}
WORK=${DEMO_WORK:-$(mktemp -d)}
PORT=${DEMO_PORT:-8893}
export DEMO_URL="http://127.0.0.1:$PORT" DEMO_SHOTS=${DEMO_SHOTS:-$WORK/shots}
PY=${PY:-"uv run --quiet --with playwright python"}
TESTS=("$@")
[ ${#TESTS[@]} -eq 0 ] && TESTS=(smoke.py test_shell.py test_controls.py test_collab.py)

run_server() {
  rm -f "$WORK/e2e.db" "$WORK/e2e.db-wal" "$WORK/e2e.db-shm"
  "$DEMO_BIN" serve --db "$WORK/e2e.db" --bind "127.0.0.1:$PORT" >"$WORK/server.log" 2>&1 &
  SERVER=$!
  for _ in $(seq 1 50); do curl -s -o /dev/null "$DEMO_URL/api/health" && break; sleep 0.1; done
}

pkill -f "demo serve .*--bind 127.0.0.1:$PORT" 2>/dev/null || true
sleep 0.3
if curl -s -o /dev/null "$DEMO_URL/api/health"; then
  echo "port $PORT is in use by another server; set DEMO_PORT" >&2
  exit 2
fi
SERVER=
trap '[ -n "$SERVER" ] && kill $SERVER 2>/dev/null || true' EXIT

fail=0
for t in "${TESTS[@]}"; do
  # every script starts from the defaults
  [ -n "$SERVER" ] && { kill $SERVER 2>/dev/null || true; wait $SERVER 2>/dev/null || true; }
  run_server
  echo "== $t"
  if ! $PY "$t"; then fail=1; echo "!! $t failed"; fi
done
echo "screenshots: $DEMO_SHOTS"
exit $fail
