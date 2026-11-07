#!/usr/bin/env bash
# Runs every *.test.js under Node 8.12 (webOS 6) and Node 20. ws comes from
# WS_PATH (a node_modules dir holding ws 7.5.10), since none lives in the tree.
set -euo pipefail
cd "$(dirname "$0")"
: "${WS_PATH:?set WS_PATH to a node_modules dir with ws@7.5.10}"
status=0
for v in v8.12.0 v20.12.2; do
  for f in *.test.js; do
    echo "== $f on $v"
    NODE_PATH="$WS_PATH" fnm exec --using="$v" node "$f" || status=1
  done
done
exit "$status"
