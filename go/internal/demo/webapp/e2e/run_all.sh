#!/bin/sh
# Run every e2e script against a running Go demo server.
#   DEMO_E2E_URL: the server (default http://127.0.0.1:8797)
#   DEMO_E2E_RESEED: optional command run before each script (e.g. stop, `demo reset`, start)
#   DEMO_E2E_RESTART: optional command that restarts the server keeping the DB (misc.py reconnect check)
#   PYTHON: how to run python with playwright (default: uv run --with playwright python)
# Each script also resets every key itself (File ▸ Reset all tabs) first.
set -e
cd "$(dirname "$0")"
PY=${PYTHON:-"uv run --with playwright python"}
for t in smoke interact collab misc; do
  [ -n "$DEMO_E2E_RESEED" ] && sh -c "$DEMO_E2E_RESEED" >/dev/null
  echo "== $t"
  # keep the script's exit status (a pipe into grep would hide it)
  if out=$($PY $t.py 2>&1); then rc=0; else rc=$?; fi
  printf '%s\n' "$out" | grep -v "^  ok" || true
  [ $rc -eq 0 ] || exit $rc
done
