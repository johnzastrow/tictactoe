#!/bin/bash
# Run the pt web e2e suite against a freshly seeded server.
#
#   PT_BIN=../../../target/debug/pt PY=python3 ./run_all.sh [test_*.py …]
#
# Needs: a built pt binary (with the web client built: `make web`), Python
# with Playwright, and Google Chrome (Playwright channel "chrome").
set -euo pipefail
cd "$(dirname "$0")"
PT_BIN=${PT_BIN:-../../../target/debug/pt}
PY=${PY:-python3}
WORK=${PT_WORK:-$(mktemp -d)}
PORT=${PT_PORT:-8792}
export PT_URL="http://127.0.0.1:$PORT" PT_PASSWORD=web-pass-123 PT_SHOTS=${PT_SHOTS:-$WORK/shots}
TESTS=("$@")
[ ${#TESTS[@]} -eq 0 ] && TESTS=(smoke.py test_grid.py test_collab.py test_views.py tour.py)

pkill -f "pt.* serve .*--bind 127.0.0.1:$PORT" 2>/dev/null || true
sleep 0.3
rm -f "$WORK/e2e.db" "$WORK/e2e.db-wal" "$WORK/e2e.db-shm"
"$PT_BIN" seed --db "$WORK/e2e.db" --password "$PT_PASSWORD" >/dev/null
"$PT_BIN" serve --db "$WORK/e2e.db" --bind "127.0.0.1:$PORT" >"$WORK/server.log" 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null || true' EXIT
for _ in $(seq 1 50); do curl -s -o /dev/null "$PT_URL/api/health" && break; sleep 0.1; done

fail=0
for t in "${TESTS[@]}"; do
  echo "== $t"
  if ! "$PY" "$t"; then fail=1; echo "!! $t failed"; fi
done
echo "screenshots: $PT_SHOTS"
exit $fail
