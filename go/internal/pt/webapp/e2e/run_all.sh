#!/bin/sh
# Run every e2e script against a freshly seeded Go server.
#   PT_E2E_RESEED: command that reseeds the DB and (re)starts the server on $PT_E2E_URL
#   PT_E2E_RESTART: command that restarts the server keeping the DB (misc.py reconnect check)
#   PYTHON: a python with playwright installed (Chrome is used via channel="chrome")
set -e
cd "$(dirname "$0")"
PY=${PYTHON:-python3}
for t in smoke interact collab views misc; do
  [ -n "$PT_E2E_RESEED" ] && sh -c "$PT_E2E_RESEED" >/dev/null
  echo "== $t"
  $PY $t.py | grep -v "^  ok" || exit 1
done
